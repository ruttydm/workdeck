//! Workdeck native extension API v1.
//!
//! Third-party extensions are executables speaking JSON-RPC 2.0 over newline-delimited stdio.
//! The host retains terminal ownership: extensions return declarative views and actions rather
//! than terminal escape sequences or Ratatui widgets.

mod bundled_ui;
mod extension_ids;
mod file_views;
mod keys;
mod panes;

pub use bundled_ui::*;
pub use extension_ids::*;
pub use file_views::*;
pub use keys::*;
pub use panes::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use workdeck_core::{
    AgentAnnotationConfidence, Changeset, ReviewFileChangeKind, ReviewNoteSource, ReviewSide,
    ReviewSnapshot,
};

pub use workdeck_core::{WORKDECK_EXTENSION_USER_ERROR_NAME, WorkdeckExtensionUserError};

pub const API_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 2_000;
pub const DEFAULT_HANDSHAKE_TIMEOUT_MS: u64 = 10_000;
/// Network-capable extension CLI commands receive a bounded but human-scale deadline.
pub const DEFAULT_CLI_REQUEST_TIMEOUT_MS: u64 = 30_000;
pub const MAX_VIEW_NODES: usize = 10_000;
pub const MAX_VIEW_DEPTH: usize = 64;
pub const FILE_VIEW_DRAFT_UNAVAILABLE_REASON: &str =
    "File presentations are unavailable while drafting an inline review note • using raw diff";

/// Draft editing remains raw-only; committed notes are placed from validated source bindings.
#[must_use]
pub const fn file_view_unavailable_reason(has_draft_note: bool) -> Option<&'static str> {
    if has_draft_note {
        Some(FILE_VIEW_DRAFT_UNAVAILABLE_REASON)
    } else {
        None
    }
}

/// Return the file-view key actually presented, accounting for host constraints.
#[must_use]
pub fn presented_file_view_key<'a>(
    selections: &'a BTreeMap<String, String>,
    unavailable_reasons: &BTreeMap<String, String>,
    file_id: Option<&str>,
) -> Option<&'a str> {
    let file_id = file_id?;
    if unavailable_reasons.contains_key(file_id) {
        return None;
    }
    selections.get(file_id).map(String::as_str)
}

/// Mask stored choices only while a host constraint requires raw rendering.
///
/// The borrowed result preserves selection identity when no selected file is
/// masked. An owned map is allocated only for a real presentation change.
#[must_use]
pub fn available_file_view_selections<'a>(
    selections: &'a BTreeMap<String, String>,
    unavailable_reasons: &BTreeMap<String, String>,
) -> Cow<'a, BTreeMap<String, String>> {
    if unavailable_reasons.is_empty()
        || !selections
            .keys()
            .any(|file_id| unavailable_reasons.contains_key(file_id))
    {
        return Cow::Borrowed(selections);
    }

    Cow::Owned(
        selections
            .iter()
            .filter(|(file_id, _)| !unavailable_reasons.contains_key(file_id.as_str()))
            .map(|(file_id, view_key)| (file_id.clone(), view_key.clone()))
            .collect(),
    )
}

/// Frozen, method-free keyboard snapshot passed across the extension boundary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionKeyEvent {
    pub name: String,
    pub sequence: String,
    pub ctrl: bool,
    pub meta: bool,
    pub option: bool,
    pub shift: bool,
}

/// Severity selected by a native extension for one user-facing notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionNotifyType {
    Info,
    Warning,
    Error,
}

/// One extension notification normalized for the host-owned TUI queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionNotification {
    pub id: u64,
    pub message: String,
    #[serde(rename = "type")]
    pub notification_type: ExtensionNotifyType,
}

const MAX_BUFFERED_NOTIFICATIONS: usize = 32;
type ExtensionNotificationListener = Arc<dyn Fn(ExtensionNotification) + Send + Sync>;

#[derive(Default)]
struct ExtensionNotificationHubState {
    next_id: u64,
    next_listener_id: u64,
    listener: Option<(u64, ExtensionNotificationListener)>,
    buffered: VecDeque<ExtensionNotification>,
}

/// Process-wide sink behind native extension `notify` calls.
///
/// Notifications sent before the Ratatui surface attaches are buffered in
/// arrival order. Only the latest 32 remain, and a detached surface re-arms
/// buffering so startup/reload messages cannot disappear between mounts.
#[derive(Clone)]
pub struct ExtensionNotificationHub {
    state: Arc<Mutex<ExtensionNotificationHubState>>,
}

impl Default for ExtensionNotificationHub {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ExtensionNotificationHub {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        formatter
            .debug_struct("ExtensionNotificationHub")
            .field("next_id", &state.next_id)
            .field("listening", &state.listener.is_some())
            .field("buffered", &state.buffered.len())
            .finish()
    }
}

impl ExtensionNotificationHub {
    #[must_use]
    pub fn new() -> Self {
        let state = ExtensionNotificationHubState {
            next_id: 1,
            next_listener_id: 1,
            ..ExtensionNotificationHubState::default()
        };
        Self {
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn notify(&self, message: impl Into<String>, notification_type: ExtensionNotifyType) {
        let notification;
        let listener;
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            notification = ExtensionNotification {
                id: state.next_id,
                message: message.into(),
                notification_type,
            };
            state.next_id = state.next_id.saturating_add(1);
            listener = state
                .listener
                .as_ref()
                .map(|(_, listener)| Arc::clone(listener));
            if listener.is_none() {
                state.buffered.push_back(notification);
                while state.buffered.len() > MAX_BUFFERED_NOTIFICATIONS {
                    state.buffered.pop_front();
                }
                return;
            }
        }
        if let Some(listener) = listener {
            deliver_extension_notification(&listener, notification);
        }
    }

    pub fn notify_info(&self, message: impl Into<String>) {
        self.notify(message, ExtensionNotifyType::Info);
    }

    /// Attach the TUI and flush anything buffered before it mounted.
    #[must_use]
    pub fn subscribe(
        &self,
        listener: impl Fn(ExtensionNotification) + Send + Sync + 'static,
    ) -> ExtensionNotificationSubscription {
        let listener: ExtensionNotificationListener = Arc::new(listener);
        let (listener_id, pending) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let listener_id = state.next_listener_id;
            state.next_listener_id = state.next_listener_id.saturating_add(1);
            state.listener = Some((listener_id, Arc::clone(&listener)));
            (listener_id, state.buffered.drain(..).collect::<Vec<_>>())
        };
        for notification in pending {
            deliver_extension_notification(&listener, notification);
        }
        ExtensionNotificationSubscription {
            hub: self.clone(),
            listener_id,
            active: true,
        }
    }
}

fn deliver_extension_notification(
    listener: &ExtensionNotificationListener,
    notification: ExtensionNotification,
) {
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(notification)));
}

/// Once-only subscription guard; dropping it restores hub buffering.
pub struct ExtensionNotificationSubscription {
    hub: ExtensionNotificationHub,
    listener_id: u64,
    active: bool,
}

impl std::fmt::Debug for ExtensionNotificationSubscription {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExtensionNotificationSubscription")
            .field("listener_id", &self.listener_id)
            .field("active", &self.active)
            .finish()
    }
}

impl ExtensionNotificationSubscription {
    pub fn unsubscribe(mut self) {
        self.detach();
    }

    fn detach(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut state = self
            .hub
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state
            .listener
            .as_ref()
            .is_some_and(|(listener_id, _)| *listener_id == self.listener_id)
        {
            state.listener = None;
        }
    }
}

impl Drop for ExtensionNotificationSubscription {
    fn drop(&mut self) {
        self.detach();
    }
}

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to read extension manifest {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid extension manifest {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error(
        "extension id {0:?} must contain only lowercase ASCII letters, digits, dots, dashes, or underscores"
    )]
    InvalidId(String),
    #[error("extension API {found} is incompatible with host API {expected}")]
    ApiVersion { found: u32, expected: u32 },
    #[error("extension executable path must be relative and cannot escape its manifest directory")]
    UnsafeExecutable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: u32,
    pub executable: PathBuf,
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    #[serde(default)]
    pub description: Option<String>,
}

impl ExtensionManifest {
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let source = std::fs::read_to_string(path).map_err(|source| ManifestError::Read {
            path: path.to_owned(),
            source,
        })?;
        let manifest: Self = toml::from_str(&source).map_err(|source| ManifestError::Parse {
            path: path.to_owned(),
            source,
        })?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn validate(&self) -> Result<(), ManifestError> {
        if self.id.is_empty()
            || !self.id.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'.' | b'-' | b'_')
            })
        {
            return Err(ManifestError::InvalidId(self.id.clone()));
        }
        if self.api_version != API_VERSION {
            return Err(ManifestError::ApiVersion {
                found: self.api_version,
                expected: API_VERSION,
            });
        }
        if self.executable.is_absolute()
            || self
                .executable
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            return Err(ManifestError::UnsafeExecutable);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Capability {
    Commands,
    CliCommands,
    Panes,
    VcsAdapters,
    Themes,
    ChangesetTransforms,
    FileViews,
    FileLanguages,
    KeyboardModes,
    LineHighlighters,
    Events,
    Configuration,
    Notifications,
    Dialogs,
    WorkspaceRead,
    WorkspaceWrite,
    ReviewNavigation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: impl Serialize) -> serde_json::Result<Self> {
        Ok(Self {
            jsonrpc: "2.0".into(),
            method: method.into(),
            params: serde_json::to_value(params)?,
        })
    }
}

impl JsonRpcRequest {
    pub fn new(
        id: u64,
        method: impl Into<String>,
        params: impl Serialize,
    ) -> serde_json::Result<Self> {
        Ok(Self {
            jsonrpc: "2.0".into(),
            id,
            method: method.into(),
            params: serde_json::to_value(params)?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeRequest {
    pub host_api_version: u32,
    pub host_version: String,
    pub extension_id: String,
    pub granted_capabilities: Vec<Capability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub extension_api_version: u32,
    pub extension_version: String,
    #[serde(default)]
    pub registrations: Vec<Registration>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Registration {
    Command(CommandRegistration),
    CliCommand(CliCommandRegistration),
    Pane(PaneRegistration),
    Theme(ThemeRegistration),
    VcsAdapter {
        id: String,
        markers: Vec<String>,
    },
    ChangesetTransform {
        id: String,
    },
    FileView {
        id: String,
        title: String,
        priority: i32,
        #[serde(default)]
        interactive_mode: bool,
    },
    FileLanguage(FileLanguageRegistration),
    KeyboardMode(KeyboardModeRegistration),
    LineHighlighter {
        id: String,
    },
    EventSubscription {
        names: Vec<String>,
    },
}

impl Registration {
    pub fn key(&self) -> String {
        match self {
            Self::Command(value) => format!("command:{}", value.id),
            Self::CliCommand(value) => format!("cli-command:{}", value.name),
            Self::Pane(value) => format!("pane:{}", value.id),
            Self::Theme(value) => format!("theme:{}", value.id),
            Self::VcsAdapter { id, .. } => format!("vcs-adapter:{id}"),
            Self::ChangesetTransform { id } => format!("changeset-transform:{id}"),
            Self::FileView { id, .. } => format!("file-view:{id}"),
            Self::FileLanguage(value) => format!("file-language:{}", value.matcher.key()),
            Self::KeyboardMode(value) => format!("keyboard-mode:{}", value.id),
            Self::LineHighlighter { id } => format!("line-highlighter:{id}"),
            Self::EventSubscription { names } => format!("event-subscription:{}", names.join(",")),
        }
    }

    pub fn required_capability(&self) -> Capability {
        match self {
            Self::Command(_) => Capability::Commands,
            Self::CliCommand(_) => Capability::CliCommands,
            Self::Pane(_) => Capability::Panes,
            Self::Theme(_) => Capability::Themes,
            Self::VcsAdapter { .. } => Capability::VcsAdapters,
            Self::ChangesetTransform { .. } => Capability::ChangesetTransforms,
            Self::FileView { .. } => Capability::FileViews,
            Self::FileLanguage(_) => Capability::FileLanguages,
            Self::KeyboardMode(_) => Capability::KeyboardModes,
            Self::LineHighlighter { .. } => Capability::LineHighlighters,
            Self::EventSubscription { .. } => Capability::Events,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FileLanguageMatcher {
    Extension {
        value: String,
    },
    Filename {
        value: String,
    },
    Glob {
        value: String,
        #[serde(default)]
        target: FileLanguageGlobTarget,
    },
}

impl FileLanguageMatcher {
    fn key(&self) -> String {
        match self {
            Self::Extension { value } => format!("extension:{value}"),
            Self::Filename { value } => format!("filename:{value}"),
            Self::Glob { value, target } => format!("glob:{target:?}:{value}"),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileLanguageGlobTarget {
    #[default]
    Basename,
    Path,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileLanguageRegistration {
    pub matcher: FileLanguageMatcher,
    pub language: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRegistration {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardModeRegistration {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliCommandRegistration {
    pub name: String,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
}

/// One invocation of a registered top-level CLI command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliCommandInvocation {
    pub command_name: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

/// The terminal stream targeted by one extension-owned output chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CliOutputStream {
    Stdout,
    Stderr,
}

/// A byte-exact output chunk emitted while a CLI request is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliOutputNotification {
    pub request_id: u64,
    pub stream: CliOutputStream,
    pub bytes: Vec<u8>,
}

/// The validated terminal outcome returned by an extension CLI handler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum CliCommandResult {
    Exit {
        #[serde(default)]
        code: u8,
    },
    Delegate {
        argv: Vec<String>,
    },
}

/// Settlement metadata needed to retain terminal ownership across delegation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliCommandExecution {
    pub result: CliCommandResult,
    #[serde(default)]
    pub stdin_read_started: bool,
    #[serde(default)]
    pub stdin_consumed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanePlacement {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneRegistration {
    pub id: String,
    pub title: String,
    pub placement: PanePlacement,
    /// Whether this pane is open when first registered. User choices made
    /// after registration take precedence for the remainder of the review.
    #[serde(default)]
    pub default_open: bool,
    /// Legacy Workdeck fixed-cell declaration. Native v1 extensions should use
    /// the placement-specific `width` or `height` contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_size: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<ExtensionPaneSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<ExtensionPaneSize>,
    /// Fully qualified pane key whose initial slot this registration replaces.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
    /// Opt into the selected-row address and host-owned current-line paint.
    #[serde(default, rename = "currentLine")]
    pub current_line: bool,
    /// Ask the host to invoke the extension's synchronous availability probe.
    #[serde(default)]
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeRegistration {
    pub id: String,
    pub base: Option<String>,
    #[serde(default)]
    pub colors: std::collections::BTreeMap<String, String>,
}

/// Host-resolved, paint-only palette passed to declarative native extension surfaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPaintTheme {
    pub appearance: ExtensionThemeAppearance,
    pub background: String,
    pub panel: String,
    pub panel_alt: String,
    pub border: String,
    pub accent: String,
    pub accent_muted: String,
    pub text: String,
    pub muted: String,
    pub selected_hunk: String,
    pub badge_added: String,
    pub badge_removed: String,
    pub badge_neutral: String,
    pub file_new: String,
    pub file_deleted: String,
    pub file_renamed: String,
    pub file_modified: String,
    pub file_untracked: String,
    pub note_border: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionThemeAppearance {
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ViewNode {
    Text {
        text: String,
        #[serde(default)]
        style: ViewStyle,
    },
    Row {
        children: Vec<ViewNode>,
        #[serde(default)]
        gap: u16,
    },
    Column {
        children: Vec<ViewNode>,
        #[serde(default)]
        gap: u16,
    },
    List {
        items: Vec<ViewNode>,
        selected: Option<usize>,
    },
    /// A host-rendered subtree that invokes one extension-owned pane action when clicked.
    Action {
        id: String,
        child: Box<ViewNode>,
    },
    Divider,
    Empty,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewStyle {
    pub foreground: Option<String>,
    pub background: Option<String>,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    #[serde(default)]
    pub underline: bool,
    #[serde(default)]
    pub dim: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyInput {
    pub key: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineHighlight {
    pub side: ReviewSide,
    pub start_line: u32,
    pub end_line: u32,
    pub start_column: Option<u32>,
    pub end_column: Option<u32>,
    pub tone: HighlightTone,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HighlightTone {
    Match,
    Current,
    Info,
    Warning,
    Error,
    Dim,
}

/// One structurally validated per-line mark returned by a native extension.
///
/// Lines are one-based source coordinates. `start..end` is a non-empty range
/// of UTF-16 code units, matching the extension wire protocol and editor APIs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedLineHighlight {
    pub side: ReviewSide,
    pub line: u64,
    pub start: u64,
    pub end: u64,
    pub tone: HighlightTone,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionPaneContext {
    /// Extension-local pane ids that are open in the committed host layout.
    #[serde(default)]
    pub open: Vec<String>,
}

/// Frozen view of the public product commands enabled for one native callback.
///
/// Native subprocesses cannot retain an in-process Rust closure. The host therefore sends the
/// result of the same live-table probe with each callback and accepts only matching declarative
/// execution actions in the response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCommandAvailability {
    /// Canonical ids and supported compatibility aliases that are enabled for this callback.
    #[serde(default)]
    pub enabled: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ExtensionCommandExecutionError {
    #[error("Extension command controls require a non-empty command id.")]
    EmptyCommandId,
    #[error("Command execution count must be a positive safe integer no greater than 10000.")]
    CountRange,
}

impl ExtensionCommandAvailability {
    #[must_use]
    pub fn is_enabled(&self, command_id: &str) -> bool {
        !command_id.trim().is_empty()
            && self.enabled.iter().any(|candidate| candidate == command_id)
    }

    /// Build the declarative native equivalent of Hunk's immediate `commands.execute` call.
    pub fn execute(
        &self,
        command_id: &str,
        count: Option<u16>,
    ) -> Result<Option<ExtensionHostAction>, ExtensionCommandExecutionError> {
        if command_id.trim().is_empty() {
            return Err(ExtensionCommandExecutionError::EmptyCommandId);
        }
        if count.is_some_and(|count| !(1..=10_000).contains(&count)) {
            return Err(ExtensionCommandExecutionError::CountRange);
        }
        Ok(self
            .is_enabled(command_id)
            .then(|| ExtensionHostAction::ExecuteReviewCommand {
                id: command_id.to_owned(),
                count,
            }))
    }
}

impl ExtensionPaneContext {
    #[must_use]
    pub fn is_open(&self, pane_id: &str) -> bool {
        self.open.iter().any(|candidate| candidate == pane_id)
    }
}

/// Immutable host state supplied alongside one extension event callback.
///
/// Native extensions return declarative host actions in place of Hunk's
/// in-process callback methods. `sidebars` remains the deprecated alias for
/// `panes` and is required to carry the same state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionEventContext {
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default)]
    pub panes: ExtensionPaneContext,
    #[serde(default)]
    pub sidebars: ExtensionPaneContext,
}

impl ExtensionEventContext {
    #[must_use]
    pub fn new(cwd: PathBuf, open_panes: Vec<String>) -> Self {
        let panes = ExtensionPaneContext { open: open_panes };
        Self {
            cwd,
            sidebars: panes.clone(),
            panes,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewEvent {
    pub name: String,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ExtensionReviewSnapshot>,
    #[serde(default)]
    pub context: ExtensionEventContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransformRequest {
    pub transform_id: String,
    pub changeset: Changeset,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformResponse {
    pub changeset: Changeset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRenderRequest {
    pub pane_id: String,
    pub snapshot: ReviewSnapshot,
    pub placement: PanePlacement,
    pub width: u16,
    pub height: u16,
    pub theme: ExtensionPaintTheme,
}

/// Synchronous native equivalent of Hunk's pane `available(context)` callback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneAvailabilityRequest {
    pub pane_id: String,
    pub placement: PanePlacement,
    pub files: Vec<ExtensionDiffFile>,
    pub selected_file_id: Option<String>,
    pub selected_hunk_index: Option<usize>,
    pub current_line: Option<ExtensionReviewSnapshotLineAddress>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneAvailabilityResponse {
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRenderResponse {
    pub content: ViewNode,
}

/// One click on an extension-owned action subtree inside a host-rendered pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneActionInvocation {
    pub pane_id: String,
    pub action_id: String,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ExtensionReviewSnapshot>,
    #[serde(default)]
    pub open_panes: Vec<String>,
}

/// One invocation of a registered in-review command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandInvocation {
    pub command_id: String,
    pub snapshot: ReviewSnapshot,
    /// Frozen selection projected through the same read-only file model as native file views.
    #[serde(default)]
    pub selection: ExtensionReviewSelection,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ExtensionReviewSnapshot>,
    /// Fully qualified `extension-id:pane-id` keys currently open in the host.
    #[serde(default)]
    pub open_panes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_keyboard_mode: Option<String>,
    /// Immutable reviewed-document capability for this command invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<ExtensionWorkspaceSnapshot>,
    /// Public product commands enabled when this invocation crossed the host boundary.
    #[serde(default)]
    pub commands: ExtensionCommandAvailability,
}

/// One reviewed file exposed through a native command's workspace capability.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionWorkspaceDocument {
    pub file_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<String>,
    pub writable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unavailable_detail: Option<String>,
}

/// Frozen native equivalent of Hunk's review-generation-bound `ctx.workspace` reads and probe.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionWorkspaceSnapshot {
    pub review_generation: u64,
    pub documents: Vec<ExtensionWorkspaceDocument>,
}

impl ExtensionWorkspaceSnapshot {
    #[must_use]
    pub fn read_document(&self, file_id: &str, side: ExtensionFileSide) -> Option<&str> {
        let document = self
            .documents
            .iter()
            .find(|document| document.file_id == file_id)?;
        match side {
            ExtensionFileSide::Old => document.old.as_deref(),
            ExtensionFileSide::New => document.new.as_deref(),
        }
    }

    #[must_use]
    pub fn can_write_document(&self, file_id: &str) -> bool {
        self.documents
            .iter()
            .find(|document| document.file_id == file_id)
            .is_some_and(|document| document.writable)
    }
}

/// Declarative host mutation requested by an in-review command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExtensionHostAction {
    OpenPane {
        id: String,
    },
    ClosePane {
        id: String,
    },
    /// Invalidate one extension-owned pane without changing its open state.
    RefreshPane {
        id: String,
    },
    EnterKeyboardMode {
        id: String,
    },
    ExitKeyboardMode,
    ExecuteReviewCommand {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<u16>,
    },
    /// Execute a semantic review command and notify when its current precondition is absent.
    TryReviewCommand {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        count: Option<u16>,
        unavailable_message: String,
    },
    SelectReviewFile {
        file_id: String,
    },
    SelectReviewHunk {
        file_id: String,
        hunk_index: usize,
    },
    RevealReviewLine {
        file_id: String,
        side: ReviewSide,
        line: u32,
    },
    ToggleFileView {
        id: String,
    },
    /// Atomically select an interactive file view for the selected file and give it input.
    EnterFileViewMode {
        id: String,
    },
    /// Leave the native file-view mode currently owned by this extension.
    ExitFileViewMode,
    /// Invalidate one file-view layout. Omitting `file_id` invalidates every file using it.
    RefreshFileView {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
    },
    /// Ask the host to confirm and perform a working-tree document replacement.
    RequestWorkspaceWrite {
        request_id: String,
        file_id: String,
        text: String,
    },
    OpenInputDialog {
        id: String,
        title: String,
        placeholder: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        initial: Option<String>,
    },
    OpenSelectDialog {
        id: String,
        title: String,
        options: Vec<String>,
    },
    OpenConfirmDialog {
        id: String,
        title: String,
        body: String,
        confirm_label: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cancel_label: Option<String>,
    },
    /// Publish a namespaced extension event to every current subscriber.
    EmitEvent {
        name: String,
        #[serde(default)]
        payload: Value,
    },
    Notify {
        message: String,
        #[serde(rename = "type")]
        notification_type: ExtensionNotifyType,
    },
}

/// Atomic result of one in-review command invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandExecution {
    #[serde(default)]
    pub actions: Vec<ExtensionHostAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardModeLifecycleRequest {
    pub mode_id: String,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub commands: ExtensionCommandAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardModeKeyRequest {
    pub mode_id: String,
    pub key: ExtensionKeyEvent,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub commands: ExtensionCommandAvailability,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyRoutingResult {
    Handled,
    Pass,
    Exit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardModeExecution {
    pub result: KeyRoutingResult,
    #[serde(default)]
    pub actions: Vec<ExtensionHostAction>,
}

/// Lifecycle callback for a mode attached to one registered file presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewModeLifecycleRequest {
    pub view_id: String,
    pub file: ExtensionDiffFile,
    pub cwd: PathBuf,
    pub review_generation: u64,
}

/// Synchronous keyboard delivery to the active file presentation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewModeKeyRequest {
    pub view_id: String,
    pub file: ExtensionDiffFile,
    pub key: ExtensionKeyEvent,
    pub cwd: PathBuf,
    pub review_generation: u64,
}

/// Result of a host-mediated workspace write requested by an extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ExtensionWorkspaceWriteResult {
    Written,
    Cancelled { detail: String },
    Unavailable { detail: String },
    Failed { detail: String },
}

/// Correlates a workspace result with the request that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionWorkspaceWriteCompletion {
    pub request_id: String,
    pub result: ExtensionWorkspaceWriteResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDialogSubmission {
    pub action_id: String,
    pub value: Option<String>,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ExtensionReviewSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_keyboard_mode: Option<String>,
    #[serde(default)]
    pub commands: ExtensionCommandAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectDialogSubmission {
    pub action_id: String,
    pub value: Option<String>,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ExtensionReviewSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_keyboard_mode: Option<String>,
    #[serde(default)]
    pub commands: ExtensionCommandAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmDialogSubmission {
    pub action_id: String,
    pub confirmed: bool,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub cwd: PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ExtensionReviewSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_keyboard_mode: Option<String>,
    #[serde(default)]
    pub commands: ExtensionCommandAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSnapshotFileStats {
    pub additions: usize,
    pub deletions: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSnapshotFileFlags {
    pub untracked: bool,
    pub binary: bool,
    pub too_large: bool,
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSnapshotFile {
    pub file_key: String,
    pub runtime_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub change_kind: ReviewFileChangeKind,
    pub stats: ExtensionReviewSnapshotFileStats,
    pub flags: ExtensionReviewSnapshotFileFlags,
    pub content_identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_attested: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionReviewSnapshotLineAddress {
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSnapshotNoteAnchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred: Option<ExtensionReviewSnapshotLineAddress>,
    pub intersecting_hunk_indices: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_hunk_index: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionReviewNoteResolution {
    Active,
    Stale,
    Orphaned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSnapshotNote {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub source: ReviewNoteSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_source: Option<String>,
    pub file_key: String,
    pub anchor: ExtensionReviewSnapshotNoteAnchor,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub editable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<AgentAnnotationConfidence>,
    pub resolution: ExtensionReviewNoteResolution,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSnapshot {
    pub generation: String,
    pub state_revision: u64,
    pub files: Vec<ExtensionReviewSnapshotFile>,
    pub notes: Vec<ExtensionReviewSnapshotNote>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionPaneView {
    pub extension_id: String,
    pub pane: PaneRegistration,
    pub content: ViewNode,
}

pub fn validate_view(root: &ViewNode) -> Result<(), String> {
    let mut pending = vec![(root, 1_usize)];
    let mut nodes = 0_usize;
    let mut text_bytes = 0_usize;
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_VIEW_DEPTH {
            return Err(format!("view exceeds maximum depth {MAX_VIEW_DEPTH}"));
        }
        nodes += 1;
        if nodes > MAX_VIEW_NODES {
            return Err(format!("view exceeds maximum node count {MAX_VIEW_NODES}"));
        }
        match node {
            ViewNode::Text { text, .. } => text_bytes = text_bytes.saturating_add(text.len()),
            ViewNode::Row { children, .. } | ViewNode::Column { children, .. } => {
                pending.extend(children.iter().map(|child| (child, depth + 1)));
            }
            ViewNode::List { items, .. } => {
                pending.extend(items.iter().map(|child| (child, depth + 1)));
            }
            ViewNode::Action { id, child } => {
                if id.trim().is_empty() || id.len() > 1_024 {
                    return Err("view action ids must be 1..=1024 bytes".into());
                }
                pending.push((child, depth + 1));
            }
            ViewNode::Divider | ViewNode::Empty => {}
        }
        if text_bytes > MAX_MESSAGE_BYTES {
            return Err(format!("view text exceeds {MAX_MESSAGE_BYTES} bytes"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn committed_notes_leave_file_view_availability_to_validated_bindings() {
        assert_eq!(file_view_unavailable_reason(false), None);
    }

    #[test]
    fn draft_notes_require_the_raw_diff() {
        assert_eq!(
            file_view_unavailable_reason(true),
            Some(FILE_VIEW_DRAFT_UNAVAILABLE_REASON)
        );
    }

    #[test]
    fn file_view_mask_preserves_selection_identity_without_a_matching_constraint() {
        let selections = BTreeMap::from([("readme".into(), "preview:rendered".into())]);
        assert!(matches!(
            available_file_view_selections(&selections, &BTreeMap::new()),
            Cow::Borrowed(value) if std::ptr::eq(value, &selections)
        ));

        let unrelated =
            BTreeMap::from([("other".into(), FILE_VIEW_DRAFT_UNAVAILABLE_REASON.into())]);
        assert!(matches!(
            available_file_view_selections(&selections, &unrelated),
            Cow::Borrowed(value) if std::ptr::eq(value, &selections)
        ));
    }

    #[test]
    fn file_view_mask_hides_unavailable_choices_without_mutating_storage() {
        let selections = BTreeMap::from([
            ("other".into(), "ext:view".into()),
            ("readme".into(), "preview:rendered".into()),
        ]);
        let unavailable =
            BTreeMap::from([("readme".into(), FILE_VIEW_DRAFT_UNAVAILABLE_REASON.into())]);

        assert_eq!(
            available_file_view_selections(&selections, &unavailable).into_owned(),
            BTreeMap::from([("other".into(), "ext:view".into())])
        );
        assert_eq!(
            selections,
            BTreeMap::from([
                ("other".into(), "ext:view".into()),
                ("readme".into(), "preview:rendered".into()),
            ])
        );
    }

    #[test]
    fn presented_file_view_reports_rendered_state_not_only_stored_choice() {
        let selections = BTreeMap::from([("readme".into(), "preview:rendered".into())]);
        assert_eq!(
            presented_file_view_key(&selections, &BTreeMap::new(), Some("readme")),
            Some("preview:rendered")
        );
        assert_eq!(
            presented_file_view_key(&selections, &BTreeMap::new(), Some("other")),
            None
        );
        assert_eq!(
            presented_file_view_key(&selections, &BTreeMap::new(), None),
            None
        );

        let unavailable =
            BTreeMap::from([("readme".into(), FILE_VIEW_DRAFT_UNAVAILABLE_REASON.into())]);
        assert_eq!(
            presented_file_view_key(&selections, &unavailable, Some("readme")),
            None
        );
    }

    #[test]
    fn manifest_rejects_parent_directory_executables() {
        let manifest = ExtensionManifest {
            id: "example.review".into(),
            name: "Example".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: "../escape".into(),
            capabilities: vec![Capability::Panes],
            description: None,
        };
        assert!(matches!(
            manifest.validate(),
            Err(ManifestError::UnsafeExecutable)
        ));
    }

    #[test]
    fn json_rpc_request_is_one_line_safe() {
        let request = JsonRpcRequest::new(
            1,
            "workdeck/handshake",
            HandshakeRequest {
                host_api_version: API_VERSION,
                host_version: "0.1.0".into(),
                extension_id: "example.review".into(),
                granted_capabilities: vec![Capability::Commands],
            },
        )
        .unwrap();
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(!encoded.contains('\n'));
        assert_eq!(
            serde_json::from_str::<JsonRpcRequest>(&encoded).unwrap(),
            request
        );
    }

    #[test]
    fn keyboard_mode_protocol_round_trips_lifecycle_keys_and_host_actions() {
        let execution = KeyboardModeExecution {
            result: KeyRoutingResult::Exit,
            actions: vec![
                ExtensionHostAction::ExecuteReviewCommand {
                    id: "workdeck.review.step-down".into(),
                    count: Some(10_000),
                },
                ExtensionHostAction::OpenInputDialog {
                    id: "vim-command".into(),
                    title: "Vim command (:)".into(),
                    placeholder: "top or bottom".into(),
                    initial: None,
                },
            ],
        };
        let encoded = serde_json::to_value(&execution).unwrap();
        assert_eq!(encoded["result"], "exit");
        assert_eq!(encoded["actions"][0]["kind"], "execute-review-command");
        assert_eq!(encoded["actions"][0]["count"], 10_000);
        assert_eq!(
            serde_json::from_value::<KeyboardModeExecution>(encoded).unwrap(),
            execution
        );

        let registration = Registration::KeyboardMode(KeyboardModeRegistration {
            id: "normal".into(),
            title: "Vim navigation".into(),
        });
        assert_eq!(registration.key(), "keyboard-mode:normal");
        assert_eq!(
            registration.required_capability(),
            Capability::KeyboardModes
        );
    }

    #[test]
    fn cli_protocol_preserves_raw_args_bytes_and_default_exit_status() {
        let invocation = CliCommandInvocation {
            command_name: "cli-tools".into(),
            args: vec!["review".into(), "--".into(), "-leading".into()],
            cwd: PathBuf::from("/tmp/review"),
        };
        assert_eq!(
            serde_json::from_value::<CliCommandInvocation>(
                serde_json::to_value(&invocation).unwrap()
            )
            .unwrap(),
            invocation
        );

        let output = CliOutputNotification {
            request_id: 9,
            stream: CliOutputStream::Stdout,
            bytes: vec![0, b'\n', 0xff],
        };
        assert_eq!(
            serde_json::from_value::<CliOutputNotification>(serde_json::to_value(&output).unwrap())
                .unwrap(),
            output
        );
        assert_eq!(
            serde_json::from_value::<CliCommandResult>(serde_json::json!({ "kind": "exit" }))
                .unwrap(),
            CliCommandResult::Exit { code: 0 }
        );
        assert!(
            serde_json::from_value::<CliCommandResult>(
                serde_json::json!({ "kind": "exit", "code": 256 })
            )
            .is_err()
        );
    }

    #[test]
    fn rejects_declarative_views_past_the_depth_budget() {
        let mut view = ViewNode::Empty;
        for _ in 0..=MAX_VIEW_DEPTH {
            view = ViewNode::Column {
                children: vec![view],
                gap: 0,
            };
        }
        assert!(validate_view(&view).unwrap_err().contains("depth"));
    }

    #[test]
    fn pane_actions_and_confirmation_dialogs_round_trip_without_executable_ui_objects() {
        let action = ViewNode::Action {
            id: "select-hunk:4".into(),
            child: Box::new(ViewNode::Text {
                text: "hunk 5".into(),
                style: ViewStyle::default(),
            }),
        };
        assert!(validate_view(&action).is_ok());
        assert!(
            validate_view(&ViewNode::Action {
                id: String::new(),
                child: Box::new(ViewNode::Empty),
            })
            .is_err()
        );

        let execution = CommandExecution {
            actions: vec![
                ExtensionHostAction::RefreshPane {
                    id: "triage".into(),
                },
                ExtensionHostAction::OpenConfirmDialog {
                    id: "clear".into(),
                    title: "Clear?".into(),
                    body: "Session state only".into(),
                    confirm_label: "clear".into(),
                    cancel_label: Some("keep".into()),
                },
                ExtensionHostAction::EmitEvent {
                    name: "review-triage:decision".into(),
                    payload: serde_json::json!({ "status": "approved" }),
                },
            ],
        };
        let encoded = serde_json::to_value(&execution).unwrap();
        assert_eq!(encoded["actions"][0]["kind"], "refresh-pane");
        assert_eq!(encoded["actions"][1]["kind"], "open-confirm-dialog");
        assert_eq!(encoded["actions"][2]["kind"], "emit-event");
        assert_eq!(
            serde_json::from_value::<CommandExecution>(encoded).unwrap(),
            execution
        );
    }

    #[test]
    fn pane_availability_request_is_method_free_and_uses_public_line_addresses() {
        let request = PaneAvailabilityRequest {
            pane_id: "detail".into(),
            placement: PanePlacement::Bottom,
            files: Vec::new(),
            selected_file_id: Some("alpha".into()),
            selected_hunk_index: Some(2),
            current_line: Some(ExtensionReviewSnapshotLineAddress {
                side: ReviewSide::New,
                line: 41,
            }),
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["paneId"], "detail");
        assert_eq!(value["selectedFileId"], "alpha");
        assert_eq!(value["selectedHunkIndex"], 2);
        assert_eq!(value["currentLine"]["side"], "new");
        assert_eq!(value["currentLine"]["line"], 41);
        assert_eq!(
            serde_json::from_value::<PaneAvailabilityRequest>(value).unwrap(),
            request
        );
        assert_eq!(
            serde_json::to_value(PaneAvailabilityResponse { available: true }).unwrap(),
            serde_json::json!({ "available": true })
        );
    }

    #[test]
    fn command_workspace_snapshot_preserves_reads_probes_and_result_reasons() {
        let workspace = ExtensionWorkspaceSnapshot {
            review_generation: 7,
            documents: vec![ExtensionWorkspaceDocument {
                file_id: "alpha".into(),
                path: "src/alpha.rs".into(),
                old: Some("old\n".into()),
                new: Some("new\n".into()),
                writable: true,
                unavailable_detail: None,
            }],
        };
        assert_eq!(
            workspace.read_document("alpha", ExtensionFileSide::New),
            Some("new\n")
        );
        assert_eq!(
            workspace.read_document("missing", ExtensionFileSide::Old),
            None
        );
        assert!(workspace.can_write_document("alpha"));
        assert!(!workspace.can_write_document("missing"));
        assert_eq!(
            serde_json::to_value(ExtensionWorkspaceWriteResult::Cancelled {
                detail: "The write to src/alpha.rs was declined.".into(),
            })
            .unwrap(),
            serde_json::json!({
                "kind": "cancelled",
                "detail": "The write to src/alpha.rs was declined."
            })
        );
        assert_eq!(
            serde_json::to_value(ExtensionWorkspaceWriteResult::Unavailable {
                detail: "The review reloaded before this extension operation could finish.".into(),
            })
            .unwrap()["kind"],
            "unavailable"
        );
    }

    #[test]
    fn command_invocation_freezes_the_complete_native_context() {
        let mut live_selection = workdeck_core::ReviewSelection {
            file_index: 2,
            hunk_index: Some(3),
            side: Some(ReviewSide::New),
            line: Some(41),
        };
        let invocation = CommandInvocation {
            command_id: "run".into(),
            snapshot: ReviewSnapshot {
                generation: 11,
                changeset: Changeset {
                    id: "review-11".into(),
                    title: "Review".into(),
                    source: workdeck_core::ChangesetSource::Patch {
                        label: "stdin".into(),
                    },
                    files: Vec::new(),
                },
                selection: live_selection,
            },
            cwd: PathBuf::from("/repo"),
            review: Some(ExtensionReviewSnapshot {
                generation: "review-11".into(),
                state_revision: 4,
                ..ExtensionReviewSnapshot::default()
            }),
            open_panes: vec!["probe:summary".into()],
            active_keyboard_mode: Some("probe:normal".into()),
            workspace: Some(ExtensionWorkspaceSnapshot {
                review_generation: 11,
                documents: Vec::new(),
            }),
            commands: ExtensionCommandAvailability {
                enabled: vec![
                    "workdeck.review.nextHunk".into(),
                    "workdeck.review.next-hunk".into(),
                ],
            },
            selection: ExtensionReviewSelection::default(),
        };
        live_selection.file_index = 9;

        assert_eq!(live_selection.file_index, 9);
        assert_eq!(invocation.snapshot.selection.file_index, 2);
        let encoded = serde_json::to_value(&invocation).unwrap();
        assert_eq!(encoded["cwd"], "/repo");
        assert_eq!(encoded["snapshot"]["generation"], 11);
        assert_eq!(encoded["snapshot"]["selection"]["line"], 41);
        assert_eq!(encoded["review"]["stateRevision"], 4);
        assert_eq!(encoded["open_panes"], serde_json::json!(["probe:summary"]));
        assert_eq!(encoded["active_keyboard_mode"], "probe:normal");
        assert_eq!(encoded["workspace"]["reviewGeneration"], 11);
        assert_eq!(encoded["selection"], serde_json::json!({}));
        assert_eq!(
            encoded["commands"]["enabled"],
            serde_json::json!(["workdeck.review.nextHunk", "workdeck.review.next-hunk"])
        );
        assert!(invocation.commands.is_enabled("workdeck.review.nextHunk"));
        assert!(
            !invocation
                .commands
                .is_enabled("workdeck.review.previousHunk")
        );
        assert_eq!(
            invocation
                .commands
                .execute("workdeck.review.next-hunk", Some(3))
                .unwrap(),
            Some(ExtensionHostAction::ExecuteReviewCommand {
                id: "workdeck.review.next-hunk".into(),
                count: Some(3),
            })
        );
        assert_eq!(
            invocation
                .commands
                .execute("workdeck.review.previousHunk", None)
                .unwrap(),
            None
        );
        assert_eq!(
            invocation.commands.execute("", None).unwrap_err(),
            ExtensionCommandExecutionError::EmptyCommandId
        );
        assert_eq!(
            invocation
                .commands
                .execute("workdeck.review.nextHunk", Some(0))
                .unwrap_err(),
            ExtensionCommandExecutionError::CountRange
        );
        assert_eq!(
            serde_json::from_value::<CommandInvocation>(encoded).unwrap(),
            invocation
        );
        let mut legacy = serde_json::to_value(&invocation).unwrap();
        legacy.as_object_mut().unwrap().remove("selection");
        assert_eq!(
            serde_json::from_value::<CommandInvocation>(legacy)
                .unwrap()
                .selection,
            ExtensionReviewSelection::default()
        );
    }

    #[test]
    fn event_context_keeps_sidebars_as_the_exact_pane_state_alias() {
        let context = ExtensionEventContext::new(
            PathBuf::from("/repo"),
            vec!["summary".into(), "outline".into()],
        );
        assert_eq!(context.panes, context.sidebars);
        assert!(context.panes.is_open("summary"));
        assert!(!context.sidebars.is_open("missing"));

        let encoded = serde_json::to_value(&context).unwrap();
        assert_eq!(encoded["cwd"], "/repo");
        assert_eq!(encoded["panes"], encoded["sidebars"]);
        assert_eq!(
            encoded["panes"]["open"],
            serde_json::json!(["summary", "outline"])
        );
    }

    #[test]
    fn notification_hub_buffers_until_a_listener_and_flushes_in_order() {
        let hub = ExtensionNotificationHub::new();
        hub.notify_info("first");
        hub.notify("second", ExtensionNotifyType::Warning);

        let seen = Arc::new(Mutex::new(Vec::new()));
        let _subscription = hub.subscribe({
            let seen = Arc::clone(&seen);
            move |notification| {
                seen.lock()
                    .unwrap()
                    .push((notification.message, notification.notification_type));
            }
        });

        assert_eq!(
            *seen.lock().unwrap(),
            [
                ("first".into(), ExtensionNotifyType::Info),
                ("second".into(), ExtensionNotifyType::Warning),
            ]
        );
    }

    #[test]
    fn notification_hub_delivers_directly_without_buffering() {
        let hub = ExtensionNotificationHub::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let _subscription = hub.subscribe({
            let seen = Arc::clone(&seen);
            move |notification| seen.lock().unwrap().push(notification.message)
        });
        hub.notify_info("live");
        assert_eq!(*seen.lock().unwrap(), ["live"]);
    }

    #[test]
    fn notification_hub_rearms_buffering_after_unsubscribe() {
        let hub = ExtensionNotificationHub::new();
        let first = Arc::new(Mutex::new(Vec::new()));
        let subscription = hub.subscribe({
            let first = Arc::clone(&first);
            move |notification| first.lock().unwrap().push(notification.message)
        });
        hub.notify_info("before");
        subscription.unsubscribe();
        hub.notify_info("while detached");

        let second = Arc::new(Mutex::new(Vec::new()));
        let _subscription = hub.subscribe({
            let second = Arc::clone(&second);
            move |notification| second.lock().unwrap().push(notification.message)
        });
        assert_eq!(*first.lock().unwrap(), ["before"]);
        assert_eq!(*second.lock().unwrap(), ["while detached"]);
    }

    #[test]
    fn notification_hub_assigns_strictly_increasing_ids() {
        let hub = ExtensionNotificationHub::new();
        let ids = Arc::new(Mutex::new(Vec::new()));
        let _subscription = hub.subscribe({
            let ids = Arc::clone(&ids);
            move |notification| ids.lock().unwrap().push(notification.id)
        });
        hub.notify_info("a");
        hub.notify_info("b");
        let ids = ids.lock().unwrap();
        assert!(ids[1] > ids[0]);
    }

    #[test]
    fn notification_hub_drops_the_oldest_buffered_entries_at_its_cap() {
        let hub = ExtensionNotificationHub::new();
        for index in 0..40 {
            hub.notify_info(format!("message {index}"));
        }
        let seen = Arc::new(Mutex::new(Vec::new()));
        let _subscription = hub.subscribe({
            let seen = Arc::clone(&seen);
            move |notification| seen.lock().unwrap().push(notification.message)
        });
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 32);
        assert_eq!(seen.first().map(String::as_str), Some("message 8"));
        assert_eq!(seen.last().map(String::as_str), Some("message 39"));
    }

    #[test]
    fn notification_hub_contains_listener_panics() {
        let hub = ExtensionNotificationHub::new();
        let attempts = Arc::new(AtomicUsize::new(0));
        let _subscription = hub.subscribe({
            let attempts = Arc::clone(&attempts);
            move |_| {
                attempts.fetch_add(1, Ordering::Relaxed);
                panic!("ui exploded");
            }
        });
        hub.notify_info("still fine");
        hub.notify_info("also fine");
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
    }
}
