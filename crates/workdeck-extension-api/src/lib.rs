//! Workdeck native extension API v1.
//!
//! Third-party extensions are executables speaking JSON-RPC 2.0 over newline-delimited stdio.
//! The host retains terminal ownership: extensions return declarative views and actions rather
//! than terminal escape sequences or Ratatui widgets.

mod authoring;
mod bundled_ui;
mod cancellation;
mod document_callbacks;
mod document_client;
mod extension_ids;
mod file_views;
mod keys;
mod panes;
mod status_line;
mod vcs;

pub use authoring::*;
pub use bundled_ui::*;
pub use cancellation::*;
pub use document_callbacks::*;
pub use document_client::*;
pub use extension_ids::*;
pub use file_views::*;
pub use keys::*;
pub use panes::*;
pub use status_line::*;
pub use vcs::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
pub use workdeck_core::{AgentAnnotation, AgentFileContext, NamedCustomThemeConfig};
use workdeck_core::{
    AgentAnnotationConfidence, ReviewFileChangeKind, ReviewNoteSource, ReviewSide, ReviewSnapshot,
};

pub use workdeck_core::{WORKDECK_EXTENSION_USER_ERROR_NAME, WorkdeckExtensionUserError};

pub const API_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 2_000;
pub const DEFAULT_HANDSHAKE_TIMEOUT_MS: u64 = 10_000;
/// Network-capable extension CLI commands receive a bounded but human-scale deadline.
pub const DEFAULT_CLI_REQUEST_TIMEOUT_MS: u64 = 30_000;
/// Maximum raw stdin bytes transferred for one extension CLI read request.
pub const MAX_CLI_STDIN_CHUNK_BYTES: usize = 64 * 1024;
pub const MAX_VIEW_NODES: usize = 10_000;
pub const MAX_VIEW_DEPTH: usize = 64;
pub const MAX_PANE_INPUT_BYTES: usize = 64 * 1024;
pub const FILE_VIEW_DRAFT_UNAVAILABLE_REASON: &str =
    "File presentations are unavailable while drafting an inline review note • using raw diff";

/// Top-level Workdeck commands that native extensions may not shadow.
///
/// This list is owned by the versioned extension API so the handshake validator and the CLI
/// parser enforce one policy without introducing a host-to-CLI dependency.
pub const BUILT_IN_CLI_COMMAND_NAMES: &[&str] = &[
    "diff",
    "show",
    "patch",
    "pager",
    "difftool",
    "stash",
    "session",
    "markup",
    "skill",
    "extension",
    "ext",
    "update",
    "install",
    "daemon",
    "mcp",
    "help",
    "version",
    "migrate",
    "status",
    "files",
    "changes",
    "search",
    "config",
    "events",
    "import",
    "doctor",
    "export",
    "issue",
    "agent",
    "project",
    "cycle",
    "label",
];

/// Hunk's public lowercase-kebab grammar for extension-owned top-level commands.
#[must_use]
pub fn is_valid_extension_cli_command_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(first) if first.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[must_use]
pub fn is_reserved_extension_cli_command_name(name: &str) -> bool {
    BUILT_IN_CLI_COMMAND_NAMES.contains(&name)
}

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
        "extension id {0:?} must start with an ASCII letter or digit and contain only ASCII letters, digits, dots, dashes, or underscores"
    )]
    InvalidId(String),
    #[error("extension id {0:?} is reserved by Workdeck")]
    ReservedId(String),
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
        self.validate_identity()?;
        self.validate_api_compatibility()?;
        self.validate_executable()?;
        Ok(())
    }

    pub fn validate_identity(&self) -> Result<(), ManifestError> {
        let mut id = self.id.bytes();
        if !matches!(id.next(), Some(first) if first.is_ascii_alphanumeric())
            || !id.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        {
            return Err(ManifestError::InvalidId(self.id.clone()));
        }
        if matches!(self.id.as_str(), "workdeck" | "git" | "jj" | "sl") {
            return Err(ManifestError::ReservedId(self.id.clone()));
        }
        Ok(())
    }

    pub fn validate_api_compatibility(&self) -> Result<(), ManifestError> {
        if self.api_version != API_VERSION {
            return Err(ManifestError::ApiVersion {
                found: self.api_version,
                expected: API_VERSION,
            });
        }
        Ok(())
    }

    pub fn validate_executable(&self) -> Result<(), ManifestError> {
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
    StatusLine,
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
    /// Session working directory supplied by the host extension context.
    pub cwd: PathBuf,
    pub granted_capabilities: Vec<Capability>,
    /// Merged `[extension.<id>]` settings when the manifest requested `configuration`.
    #[serde(default)]
    pub config: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub extension_api_version: u32,
    pub extension_version: String,
    #[serde(default)]
    pub registrations: Vec<Registration>,
}

/// Lifecycle event names exposed by Hunk's public `on(event, handler)` contract.
pub const LIFECYCLE_EVENT_NAMES: &[&str] = &[
    "startup",
    "changeset_loaded",
    "command_executed",
    "selection_changed",
    "file_viewed",
    "hunk_viewed",
    "filter_changed",
    "theme_changed",
    "layout_changed",
    "watch_reload_pending",
    "note_created",
    "note_edited",
    "note_changed",
    "session_reload",
    "shutdown",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Registration {
    SessionOptions(SessionOptionsRegistration),
    Command(CommandRegistration),
    CliCommand(CliCommandRegistration),
    Pane(PaneRegistration),
    Theme(ThemeRegistration),
    VcsAdapter(ExtensionVcsAdapterRegistration),
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
    /// Open-name extension-to-extension event-bus subscriptions.
    CustomEventSubscription {
        names: Vec<String>,
    },
    /// Custom event emitted while the extension factory was producing its handshake.
    PendingCustomEvent {
        name: String,
        #[serde(default)]
        payload: Value,
    },
}

impl Registration {
    pub fn key(&self) -> String {
        match self {
            Self::SessionOptions(_) => "session-options".into(),
            Self::Command(value) => format!("command:{}", value.id),
            Self::CliCommand(value) => format!("cli-command:{}", value.name),
            Self::Pane(value) => format!("pane:{}", value.id),
            Self::Theme(value) => format!("theme:{}", value.id),
            Self::VcsAdapter(value) => format!("vcs-adapter:{}", value.id),
            Self::ChangesetTransform { id } => format!("changeset-transform:{id}"),
            Self::FileView { id, .. } => format!("file-view:{id}"),
            Self::FileLanguage(value) => format!("file-language:{}", value.matcher.key()),
            Self::KeyboardMode(value) => format!("keyboard-mode:{}", value.id),
            Self::LineHighlighter { id } => format!("line-highlighter:{id}"),
            Self::EventSubscription { names } => format!("event-subscription:{}", names.join(",")),
            Self::CustomEventSubscription { names } => {
                format!("custom-event-subscription:{}", names.join(","))
            }
            Self::PendingCustomEvent { name, .. } => format!("pending-custom-event:{name}"),
        }
    }

    pub fn required_capability(&self) -> Capability {
        match self {
            Self::SessionOptions(_) => Capability::Configuration,
            Self::Command(_) => Capability::Commands,
            Self::CliCommand(_) => Capability::CliCommands,
            Self::Pane(_) => Capability::Panes,
            Self::Theme(_) => Capability::Themes,
            Self::VcsAdapter(_) => Capability::VcsAdapters,
            Self::ChangesetTransform { .. } => Capability::ChangesetTransforms,
            Self::FileView { .. } => Capability::FileViews,
            Self::FileLanguage(_) => Capability::FileLanguages,
            Self::KeyboardMode(_) => Capability::KeyboardModes,
            Self::LineHighlighter { .. } => Capability::LineHighlighters,
            Self::EventSubscription { .. } => Capability::Events,
            Self::CustomEventSubscription { .. } => Capability::Events,
            Self::PendingCustomEvent { .. } => Capability::Events,
        }
    }
}

/// Host-level behavior requested for the review session loading an extension.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOptionsRegistration {
    #[serde(default)]
    pub view_preferences: Option<ViewPreferencesPolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ViewPreferencesPolicy {
    Default,
    Transient,
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

/// Lazy request from an extension CLI handler for the next host-stdin chunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliStdinReadRequest {
    pub request_id: u64,
    pub read_id: u64,
    #[serde(default = "default_cli_stdin_chunk_bytes")]
    pub max_bytes: usize,
}

const fn default_cli_stdin_chunk_bytes() -> usize {
    8 * 1024
}

/// Host response to one extension CLI stdin read request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CliStdinChunk {
    pub request_id: u64,
    pub read_id: u64,
    #[serde(default)]
    pub bytes: Vec<u8>,
    #[serde(default)]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanePlacement {
    #[default]
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PaneRegistration {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
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
    /// A host-rendered, one-line controlled input inside an extension pane.
    Input {
        id: String,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default)]
        focused: bool,
    },
    /// Resolve one side of the pane request's opted-in current-line painter.
    CurrentLine {
        side: ExtensionFileSide,
        width: u16,
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

/// Frozen file-presentation state exposed to one native command callback.
///
/// Hunk's in-process controls answer `isActive` and `isModeActive` synchronously. A native
/// subprocess cannot borrow the live review controller, so the host sends the equivalent
/// extension-local view ids with every invocation and validates returned actions against the
/// current generation again.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFileViewContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_view_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_mode_id: Option<String>,
}

impl ExtensionFileViewContext {
    #[must_use]
    pub fn is_active(&self, view_id: &str) -> bool {
        self.active_view_id.as_deref() == Some(view_id)
    }

    #[must_use]
    pub fn is_mode_active(&self, view_id: &str) -> bool {
        self.active_mode_id.as_deref() == Some(view_id)
    }
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
    pub changeset: ExtensionChangeset,
    /// Session working directory from Hunk's shared `ExtensionContext`.
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformResponse {
    pub changeset: ExtensionChangeset,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notifications: Vec<TransformNotification>,
}

/// Notifications emitted during a transform; IDs remain host-owned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransformNotification {
    pub message: String,
    #[serde(rename = "type")]
    pub notification_type: ExtensionNotifyType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRenderRequest {
    pub pane_id: String,
    pub snapshot: ReviewSnapshot,
    pub placement: PanePlacement,
    pub width: u16,
    pub height: u16,
    pub theme: ExtensionPaintTheme,
    /// Filtered files visible to the mounted review pane, in review order.
    #[serde(default)]
    pub files: Vec<ExtensionDiffFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_file_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_hunk_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_line: Option<ExtensionCurrentLinePaint>,
    #[serde(default)]
    pub keybindings: ExtensionResolvedKeybindings,
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
    pub current_line: Option<ExtensionCurrentLinePaint>,
}

/// Address and declarative renderer for the selected review row.
///
/// Native extensions cannot receive a callable Ratatui object across stdio. Calling `render`
/// returns a host-owned view node that is resolved only while painting the pane that received
/// this snapshot, preserving Hunk's synchronous side/width choice without exposing renderer data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionCurrentLinePaint {
    pub side: ExtensionFileSide,
    pub line: u32,
}

impl ExtensionCurrentLinePaint {
    #[must_use]
    pub const fn render(self, side: ExtensionFileSide, width: u16) -> ViewNode {
        ViewNode::CurrentLine { side, width }
    }
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

/// One controlled value change from a focused input in an extension pane.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneInputInvocation {
    pub pane_id: String,
    pub input_id: String,
    pub value: String,
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
    /// Synchronous `ctx.fileViews` state for views owned by this command's extension.
    #[serde(default)]
    pub file_views: ExtensionFileViewContext,
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
    TogglePane {
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
    /// Select one registered presentation, or raw diff when `id` is absent.
    SelectFileView {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
    },
    /// Atomically select an interactive file view for the selected file and give it input.
    EnterFileViewMode {
        id: String,
    },
    /// Leave the one native file-view mode currently active in the review.
    ExitFileViewMode,
    /// Invalidate one file-view layout. Omitting `file_id` invalidates every file using it.
    RefreshFileView {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
    },
    /// Invalidate one line highlighter for every file or one current terminal file id.
    RefreshLineHighlights {
        id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_id: Option<String>,
    },
    /// Ask the host to resolve one document side while the invoking review generation is live.
    RequestWorkspaceRead {
        request_id: String,
        file_id: String,
        side: ExtensionFileSide,
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
    /// Set or replace one of this extension's persistent items on the status row.
    SetStatusItem(ExtensionStatusItem),
    /// Clear one of this extension's persistent items on the status row.
    ClearStatusItem {
        id: String,
    },
    /// Ask the user for one line of text inline on the status row; the host
    /// resolves it with [`ExtensionPromptLineCompletion`].
    RequestPromptLine {
        request_id: String,
        #[serde(flatten)]
        options: ExtensionPromptLineOptions,
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
    /// Session working directory from Hunk's shared `ExtensionContext`.
    pub cwd: PathBuf,
    #[serde(default)]
    pub commands: ExtensionCommandAvailability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyboardModeKeyRequest {
    pub mode_id: String,
    pub key: ExtensionKeyEvent,
    pub snapshot: ReviewSnapshot,
    /// Session working directory from Hunk's shared `ExtensionContext`.
    pub cwd: PathBuf,
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

/// Atomic result of one native file-view lifecycle callback.
///
/// A callback may synchronously request a host action and then fail. Keeping the
/// contained failure beside those actions preserves Hunk's ordering: the host
/// applies the actions first, then retires only the activation that actually
/// failed. A replacement mode therefore keeps its independent lifecycle.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewModeLifecycleExecution {
    #[serde(default)]
    pub actions: Vec<ExtensionHostAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<String>,
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

/// Generation-checked result of one host-mediated reviewed-document read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionWorkspaceReadCompletion {
    pub request_id: String,
    #[serde(default)]
    pub value: Option<String>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionReviewNoteChangeKind {
    Created,
    Updated,
    Removed,
}

/// One saved-note change emitted only within a single review generation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionReviewNoteChange {
    pub kind: ExtensionReviewNoteChangeKind,
    pub note: ExtensionReviewSnapshotNote,
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
    let mut input_ids = BTreeSet::new();
    let mut focused_inputs = 0_usize;
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
            ViewNode::Input {
                id,
                value,
                placeholder,
                focused,
            } => {
                if id.trim().is_empty() || id.len() > 1_024 {
                    return Err("view input ids must be 1..=1024 bytes".into());
                }
                if id.contains(['\r', '\n']) {
                    return Err("view input ids must be one line".into());
                }
                if !input_ids.insert(id.as_str()) {
                    return Err(format!("duplicate view input id {id:?}"));
                }
                if value.len() > MAX_PANE_INPUT_BYTES {
                    return Err(format!(
                        "view input value exceeds {MAX_PANE_INPUT_BYTES} bytes"
                    ));
                }
                if value.contains(['\r', '\n']) {
                    return Err("view input values must be one line".into());
                }
                if placeholder
                    .as_ref()
                    .is_some_and(|value| value.len() > 4 * 1_024)
                {
                    return Err("view input placeholder exceeds 4096 bytes".into());
                }
                if placeholder
                    .as_ref()
                    .is_some_and(|value| value.contains(['\r', '\n']))
                {
                    return Err("view input placeholders must be one line".into());
                }
                text_bytes = text_bytes
                    .saturating_add(value.len())
                    .saturating_add(placeholder.as_ref().map_or(0, String::len));
                focused_inputs = focused_inputs.saturating_add(usize::from(*focused));
                if focused_inputs > 1 {
                    return Err("view contains more than one focused input".into());
                }
            }
            ViewNode::CurrentLine { width, .. } => {
                if *width == 0 {
                    return Err("current-line paint width must be positive".into());
                }
            }
            ViewNode::Divider | ViewNode::Empty => {}
        }
        if text_bytes > MAX_MESSAGE_BYTES {
            return Err(format!("view text exceeds {MAX_MESSAGE_BYTES} bytes"));
        }
    }
    Ok(())
}

/// Restrict current-line paint nodes to an opted-in pane and its exact host rectangle.
pub fn validate_current_line_view_nodes(
    root: &ViewNode,
    current_line_available: bool,
    pane_width: u16,
) -> Result<(), String> {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match node {
            ViewNode::CurrentLine { width, .. } => {
                if !current_line_available {
                    return Err(
                        "current-line paint requires an available opted-in pane context".into(),
                    );
                }
                if *width > pane_width {
                    return Err(format!(
                        "current-line paint width {width} exceeds pane width {pane_width}"
                    ));
                }
            }
            ViewNode::Row { children, .. } | ViewNode::Column { children, .. } => {
                pending.extend(children);
            }
            ViewNode::List { items, .. } => pending.extend(items),
            ViewNode::Action { child, .. } => pending.push(child),
            ViewNode::Text { .. }
            | ViewNode::Input { .. }
            | ViewNode::Divider
            | ViewNode::Empty => {}
        }
    }
    Ok(())
}

/// Report whether a declarative tree contains pane-only input state.
#[must_use]
pub fn view_contains_input(root: &ViewNode) -> bool {
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        match node {
            ViewNode::Input { .. } => return true,
            ViewNode::Row { children, .. } | ViewNode::Column { children, .. } => {
                pending.extend(children);
            }
            ViewNode::List { items, .. } => pending.extend(items),
            ViewNode::Action { child, .. } => pending.push(child),
            ViewNode::Text { .. }
            | ViewNode::CurrentLine { .. }
            | ViewNode::Divider
            | ViewNode::Empty => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::Changeset;

    #[test]
    fn extension_cli_names_share_hunks_grammar_and_cannot_shadow_workdeck() {
        for valid in ["lint", "review-export", "x1", "cli--tools-"] {
            assert!(is_valid_extension_cli_command_name(valid), "{valid}");
        }
        for invalid in ["", "1lint", "Lint", "lint_me", "lint/me"] {
            assert!(!is_valid_extension_cli_command_name(invalid), "{invalid}");
        }
        for reserved in ["diff", "session", "ext", "help", "issue"] {
            assert!(is_reserved_extension_cli_command_name(reserved));
        }
        assert!(!is_reserved_extension_cli_command_name("greptile"));
    }

    #[test]
    fn session_options_reject_unknown_view_preference_policies() {
        let error = serde_json::from_value::<HandshakeResponse>(serde_json::json!({
            "extension_api_version": API_VERSION,
            "extension_version": "1.0.0",
            "registrations": [{
                "kind": "session-options",
                "view_preferences": "forever"
            }]
        }))
        .unwrap_err();
        assert!(error.to_string().contains("unknown variant"));
    }
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
    fn file_view_selection_actions_distinguish_registered_views_from_raw_diff() {
        let selected = ExtensionHostAction::SelectFileView {
            id: Some("preview".into()),
        };
        assert_eq!(
            serde_json::to_value(&selected).unwrap(),
            serde_json::json!({ "kind": "select-file-view", "id": "preview" })
        );
        let raw = ExtensionHostAction::SelectFileView { id: None };
        assert_eq!(
            serde_json::to_value(&raw).unwrap(),
            serde_json::json!({ "kind": "select-file-view" })
        );
        assert_eq!(
            serde_json::from_value::<ExtensionHostAction>(
                serde_json::json!({ "kind": "select-file-view", "id": "preview" })
            )
            .unwrap(),
            selected
        );
    }

    #[test]
    fn file_view_lifecycle_preserves_actions_that_precede_a_contained_failure() {
        let execution = FileViewModeLifecycleExecution {
            actions: vec![ExtensionHostAction::EnterFileViewMode {
                id: "probe:replacement".into(),
            }],
            failure: Some("entry exploded".into()),
        };
        let encoded = serde_json::to_value(&execution).unwrap();
        assert_eq!(
            encoded,
            serde_json::json!({
                "actions": [{ "kind": "enter-file-view-mode", "id": "probe:replacement" }],
                "failure": "entry exploded"
            })
        );
        assert_eq!(
            serde_json::from_value::<FileViewModeLifecycleExecution>(encoded).unwrap(),
            execution
        );
        assert_eq!(
            serde_json::from_value::<FileViewModeLifecycleExecution>(
                serde_json::json!({ "actions": [] })
            )
            .unwrap()
            .failure,
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
    fn manifest_reserves_product_and_bundled_vcs_namespaces() {
        for id in ["workdeck", "git", "jj", "sl"] {
            let manifest = ExtensionManifest {
                id: id.into(),
                name: "Reserved".into(),
                version: "1.0.0".into(),
                api_version: API_VERSION,
                executable: "extension".into(),
                capabilities: Vec::new(),
                description: None,
            };
            assert!(matches!(
                manifest.validate(),
                Err(ManifestError::ReservedId(actual)) if actual == id
            ));
        }
    }

    #[test]
    fn manifest_ids_start_with_a_namespace_character_and_keep_native_dotted_ids() {
        let mut manifest = ExtensionManifest {
            id: "example.review-tools".into(),
            name: "Review tools".into(),
            version: "1.0.0".into(),
            api_version: API_VERSION,
            executable: PathBuf::from("review-tools"),
            capabilities: Vec::new(),
            description: None,
        };
        manifest.validate().unwrap();

        manifest.id = "Upper.Tools".into();
        manifest.validate().unwrap();

        for invalid in ["-leading", "_leading", ".leading", "has:colon"] {
            manifest.id = invalid.into();
            assert!(matches!(
                manifest.validate(),
                Err(ManifestError::InvalidId(id)) if id == invalid
            ));
        }
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
                cwd: PathBuf::from("/workspace"),
                granted_capabilities: vec![Capability::Commands],
                config: serde_json::json!({ "threshold": 3 }),
            },
        )
        .unwrap();
        let encoded = serde_json::to_string(&request).unwrap();
        assert!(!encoded.contains('\n'));
        assert_eq!(
            serde_json::from_str::<JsonRpcRequest>(&encoded).unwrap(),
            request
        );
        assert_eq!(request.params["config"]["threshold"], 3);
        assert_eq!(request.params["cwd"], "/workspace");
    }

    #[test]
    fn cli_stdin_lease_messages_preserve_binary_chunks_and_read_identity() {
        let request = CliStdinReadRequest {
            request_id: 9,
            read_id: 3,
            max_bytes: MAX_CLI_STDIN_CHUNK_BYTES,
        };
        assert_eq!(
            serde_json::from_value::<CliStdinReadRequest>(serde_json::to_value(&request).unwrap())
                .unwrap(),
            request
        );
        let chunk = CliStdinChunk {
            request_id: 9,
            read_id: 3,
            bytes: vec![0, 10, 255],
            done: false,
            error: None,
        };
        assert_eq!(
            serde_json::from_value::<CliStdinChunk>(serde_json::to_value(&chunk).unwrap()).unwrap(),
            chunk
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

        let commands = ExtensionCommandAvailability {
            enabled: vec!["workdeck.review.nextHunk".into()],
        };
        let lifecycle = KeyboardModeLifecycleRequest {
            mode_id: "normal".into(),
            snapshot: ReviewSnapshot {
                generation: 1,
                changeset: Changeset {
                    id: "review".into(),
                    source_label: "working tree".into(),
                    title: "Review".into(),
                    summary: None,
                    agent_summary: None,
                    source: workdeck_core::ChangesetSource::WorkingTree { staged: false },
                    files: Vec::new(),
                },
                selection: workdeck_core::ReviewSelection::default(),
            },
            cwd: PathBuf::from("/repo"),
            commands: commands.clone(),
        };
        let key = KeyboardModeKeyRequest {
            mode_id: "normal".into(),
            key: ExtensionKeyEvent {
                name: "j".into(),
                sequence: "j".into(),
                ..ExtensionKeyEvent::default()
            },
            snapshot: lifecycle.snapshot.clone(),
            cwd: lifecycle.cwd.clone(),
            commands,
        };
        assert_eq!(serde_json::to_value(&lifecycle).unwrap()["cwd"], "/repo");
        assert_eq!(serde_json::to_value(&key).unwrap()["cwd"], "/repo");
        assert_eq!(
            serde_json::from_value::<KeyboardModeLifecycleRequest>(
                serde_json::to_value(&lifecycle).unwrap()
            )
            .unwrap(),
            lifecycle
        );
        assert_eq!(
            serde_json::from_value::<KeyboardModeKeyRequest>(serde_json::to_value(&key).unwrap())
                .unwrap(),
            key
        );
    }

    #[test]
    fn transform_protocol_exposes_only_the_public_changeset_and_opaque_metadata_contract() {
        let request = TransformRequest {
            transform_id: "filter".into(),
            changeset: ExtensionChangeset {
                id: "review".into(),
                source_label: "working tree".into(),
                title: "Review".into(),
                summary: None,
                agent_summary: None,
                files: Vec::new(),
            },
            cwd: PathBuf::from("/repo"),
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["cwd"], "/repo");
        assert_eq!(value["changeset"]["sourceLabel"], "working tree");
        assert!(value["changeset"].get("source").is_none());
        assert_eq!(
            serde_json::from_value::<TransformRequest>(value).unwrap(),
            request
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
    fn pane_inputs_round_trip_as_bounded_unique_one_line_controlled_values() {
        let input = ViewNode::Input {
            id: "prompt".into(),
            value: "j?界".into(),
            placeholder: Some("type here".into()),
            focused: true,
        };
        assert!(validate_view(&input).is_ok());
        assert!(view_contains_input(&ViewNode::Column {
            children: vec![ViewNode::Empty, input.clone()],
            gap: 0,
        }));
        assert_eq!(
            serde_json::to_value(&input).unwrap(),
            serde_json::json!({
                "type": "input",
                "id": "prompt",
                "value": "j?界",
                "placeholder": "type here",
                "focused": true
            })
        );
        assert_eq!(
            serde_json::from_value::<ViewNode>(serde_json::to_value(&input).unwrap()).unwrap(),
            input
        );

        let invocation = PaneInputInvocation {
            pane_id: "bottom".into(),
            input_id: "prompt".into(),
            value: "j?".into(),
            snapshot: ReviewSnapshot {
                generation: 0,
                changeset: Changeset {
                    id: "pane-input".into(),
                    source_label: "test".into(),
                    title: "Pane input".into(),
                    summary: None,
                    agent_summary: None,
                    source: workdeck_core::ChangesetSource::Patch {
                        label: "test".into(),
                    },
                    files: Vec::new(),
                },
                selection: workdeck_core::ReviewSelection::default(),
            },
            cwd: PathBuf::from("/tmp/review"),
            review: None,
            open_panes: vec!["example:bottom".into()],
        };
        let encoded = serde_json::to_value(&invocation).unwrap();
        assert_eq!(encoded["paneId"], "bottom");
        assert_eq!(encoded["inputId"], "prompt");
        assert_eq!(encoded["openPanes"][0], "example:bottom");
        assert_eq!(
            serde_json::from_value::<PaneInputInvocation>(encoded).unwrap(),
            invocation
        );
    }

    #[test]
    fn pane_input_validation_rejects_ambiguous_or_non_line_editor_state() {
        let duplicate = ViewNode::Row {
            children: vec![
                ViewNode::Input {
                    id: "same".into(),
                    value: String::new(),
                    placeholder: None,
                    focused: false,
                },
                ViewNode::Input {
                    id: "same".into(),
                    value: String::new(),
                    placeholder: None,
                    focused: false,
                },
            ],
            gap: 1,
        };
        assert!(validate_view(&duplicate).unwrap_err().contains("duplicate"));

        let focused = |id: &str| ViewNode::Input {
            id: id.into(),
            value: String::new(),
            placeholder: None,
            focused: true,
        };
        assert!(
            validate_view(&ViewNode::Column {
                children: vec![focused("one"), focused("two")],
                gap: 0,
            })
            .unwrap_err()
            .contains("more than one focused")
        );
        for invalid in [
            ViewNode::Input {
                id: "bad\nid".into(),
                value: String::new(),
                placeholder: None,
                focused: true,
            },
            ViewNode::Input {
                id: "value".into(),
                value: "two\nlines".into(),
                placeholder: None,
                focused: true,
            },
            ViewNode::Input {
                id: "placeholder".into(),
                value: String::new(),
                placeholder: Some("two\rline".into()),
                focused: true,
            },
            ViewNode::Input {
                id: "large".into(),
                value: "x".repeat(MAX_PANE_INPUT_BYTES + 1),
                placeholder: None,
                focused: true,
            },
        ] {
            assert!(validate_view(&invalid).is_err());
        }
    }

    #[test]
    fn line_highlight_refresh_action_round_trips_optional_file_scope() {
        let execution = CommandExecution {
            actions: vec![
                ExtensionHostAction::RefreshLineHighlights {
                    id: "matches".into(),
                    file_id: None,
                },
                ExtensionHostAction::RefreshLineHighlights {
                    id: "search:matches".into(),
                    file_id: Some("file:1".into()),
                },
            ],
        };
        let encoded = serde_json::to_value(&execution).unwrap();
        assert_eq!(encoded["actions"][0]["kind"], "refresh-line-highlights");
        assert!(encoded["actions"][0].get("file_id").is_none());
        assert_eq!(encoded["actions"][1]["file_id"], "file:1");
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
            current_line: Some(ExtensionCurrentLinePaint {
                side: ExtensionFileSide::New,
                line: 41,
            }),
        };
        let value = serde_json::to_value(&request).unwrap();
        assert_eq!(value["paneId"], "detail");
        assert_eq!(value["selectedFileId"], "alpha");
        assert_eq!(value["selectedHunkIndex"], 2);
        assert_eq!(value["currentLine"]["side"], "new");
        assert_eq!(value["currentLine"]["line"], 41);
        let rendered = request
            .current_line
            .unwrap()
            .render(ExtensionFileSide::Old, 24);
        assert_eq!(
            rendered,
            ViewNode::CurrentLine {
                side: ExtensionFileSide::Old,
                width: 24,
            }
        );
        assert!(validate_view(&rendered).is_ok());
        assert!(validate_current_line_view_nodes(&rendered, true, 24).is_ok());
        assert!(validate_current_line_view_nodes(&rendered, false, 24).is_err());
        assert!(validate_current_line_view_nodes(&rendered, true, 23).is_err());
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
            serde_json::to_value(ExtensionHostAction::RequestWorkspaceRead {
                request_id: "read-1".into(),
                file_id: "alpha".into(),
                side: ExtensionFileSide::New,
            })
            .unwrap(),
            serde_json::json!({
                "kind": "request-workspace-read",
                "request_id": "read-1",
                "file_id": "alpha",
                "side": "new"
            })
        );
        assert_eq!(
            serde_json::to_value(ExtensionWorkspaceReadCompletion {
                request_id: "read-1".into(),
                value: None,
            })
            .unwrap(),
            serde_json::json!({ "requestId": "read-1", "value": null })
        );
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
                    source_label: "stdin".into(),
                    title: "Review".into(),
                    summary: None,
                    agent_summary: None,
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
            file_views: ExtensionFileViewContext {
                active_view_id: Some("outline".into()),
                active_mode_id: Some("outline".into()),
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
        assert_eq!(encoded["file_views"]["activeViewId"], "outline");
        assert_eq!(encoded["file_views"]["activeModeId"], "outline");
        assert_eq!(encoded["selection"], serde_json::json!({}));
        assert_eq!(
            encoded["commands"]["enabled"],
            serde_json::json!(["workdeck.review.nextHunk", "workdeck.review.next-hunk"])
        );
        assert!(invocation.commands.is_enabled("workdeck.review.nextHunk"));
        assert!(invocation.file_views.is_active("outline"));
        assert!(invocation.file_views.is_mode_active("outline"));
        assert!(!invocation.file_views.is_active("other"));
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
            serde_json::to_value(ExtensionHostAction::TogglePane {
                id: "summary".into()
            })
            .unwrap(),
            serde_json::json!({ "kind": "toggle-pane", "id": "summary" })
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

    #[test]
    fn pinned_public_api_oracle_covers_every_export_and_source_byte_once() {
        const EXPECTED_EXPORTS: &[&str] = &[
            "AgentAnnotation",
            "AgentFileContext",
            "ChangesetTransform",
            "CustomSyntaxColorsConfig",
            "CustomSyntaxScopesConfig",
            "CustomThemeConfig",
            "ExtensionChangeset",
            "ExtensionCliCommand",
            "ExtensionCliCommandContext",
            "ExtensionCliCommandHandler",
            "ExtensionCliCommandResult",
            "ExtensionCliDelegateResult",
            "ExtensionCliExitResult",
            "ExtensionCliWriter",
            "ExtensionCommand",
            "ExtensionCommandContext",
            "ExtensionCommandControls",
            "ExtensionCommandExecutionOptions",
            "ExtensionCommandHandler",
            "ExtensionConfirmOptions",
            "ExtensionContext",
            "ExtensionCurrentLinePaint",
            "ExtensionCustomEventHandler",
            "ExtensionDialogs",
            "ExtensionDiffFile",
            "ExtensionDiffHunk",
            "ExtensionEventBus",
            "ExtensionEventContext",
            "ExtensionEventHandler",
            "ExtensionEventName",
            "ExtensionEventPayloads",
            "ExtensionFactory",
            "ExtensionFileChangeRange",
            "ExtensionFileLanguageMatcher",
            "ExtensionFileSide",
            "ExtensionFileView",
            "ExtensionFileViewControls",
            "ExtensionFileViewInput",
            "ExtensionFileViewLayout",
            "ExtensionFileViewMode",
            "ExtensionFileViewModeContext",
            "ExtensionFileViewModeKeyResult",
            "ExtensionFileViewRow",
            "ExtensionFileViewRowComponentProps",
            "ExtensionFileViewSourceRange",
            "ExtensionFileViewSpan",
            "ExtensionHorizontalPane",
            "ExtensionInputOptions",
            "ExtensionKeyEvent",
            "ExtensionKeyboardMode",
            "ExtensionKeyboardModeContext",
            "ExtensionKeyboardModeControls",
            "ExtensionKeyboardModeKeyResult",
            "ExtensionLayoutMode",
            "ExtensionLineHighlight",
            "ExtensionLineHighlightControls",
            "ExtensionLineHighlightInput",
            "ExtensionLineHighlightTone",
            "ExtensionLineHighlighter",
            "ExtensionNoteChangeKind",
            "ExtensionNotifyType",
            "ExtensionPaintTheme",
            "ExtensionPane",
            "ExtensionPaneActions",
            "ExtensionPaneAvailabilityContext",
            "ExtensionPaneComponent",
            "ExtensionPaneControls",
            "ExtensionPaneKeybindings",
            "ExtensionPanePlacement",
            "ExtensionPaneProps",
            "ExtensionPaneSize",
            "ExtensionPaneTheme",
            "ExtensionResolvedLayout",
            "ExtensionReviewControls",
            "ExtensionReviewNavigation",
            "ExtensionReviewNote",
            "ExtensionReviewSelection",
            "ExtensionReviewSnapshot",
            "ExtensionReviewSnapshotFile",
            "ExtensionReviewSnapshotLineAddress",
            "ExtensionReviewSnapshotNote",
            "ExtensionReviewSnapshotNoteAnchor",
            "ExtensionSelectOptions",
            "ExtensionSessionOptions",
            "ExtensionSidebarActions",
            "ExtensionSidebarComponent",
            "ExtensionSidebarControls",
            "ExtensionSidebarKeybindings",
            "ExtensionSidebarPlacement",
            "ExtensionSidebarTheme",
            "ExtensionSidebarView",
            "ExtensionSidebarViewProps",
            "ExtensionThemeConfig",
            "ExtensionVcsAdapter",
            "ExtensionVcsDetection",
            "ExtensionVcsDiffInput",
            "ExtensionVcsDirectoryEntriesWatchTarget",
            "ExtensionVcsDirectoryTreeWatchTarget",
            "ExtensionVcsExtraFile",
            "ExtensionVcsExtraPatchFile",
            "ExtensionVcsFileChangeType",
            "ExtensionVcsFileSide",
            "ExtensionVcsFileSourceReader",
            "ExtensionVcsFileSourceRequest",
            "ExtensionVcsFileSourceResult",
            "ExtensionVcsFileSourceTooLarge",
            "ExtensionVcsFileStats",
            "ExtensionVcsLoadContext",
            "ExtensionVcsOperation",
            "ExtensionVcsOperations",
            "ExtensionVcsPatchResult",
            "ExtensionVcsRangeEndpoints",
            "ExtensionVcsReviewOptions",
            "ExtensionVcsShowInput",
            "ExtensionVcsSkippedFile",
            "ExtensionVcsSkippedFileReason",
            "ExtensionVcsStashShowInput",
            "ExtensionVcsWatchPlan",
            "ExtensionVcsWatchTarget",
            "ExtensionVcsWatchTargetSource",
            "ExtensionVerticalPane",
            "ExtensionWorkspace",
            "ExtensionWorkspaceWriteRequest",
            "ExtensionWorkspaceWriteResult",
            "HUNK_CORE_VCS_DETECTION_PRIORITY",
            "HUNK_DEFAULT_VCS_DETECTION_PRIORITY",
            "HUNK_EXTENSION_API_VERSION",
            "HUNK_EXTENSION_USER_ERROR_NAME",
            "HUNK_VCS_DETECTION_BASELINE_PRIORITY",
            "HunkExtensionAPI",
            "HunkExtensionApiVersion",
            "HunkExtensionUserError",
            "HunkExtensionUserErrorOptions",
            "NamedCustomThemeConfig",
            "SessionReloadReason",
        ];

        let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let oracle_path = workspace.join("port/hunk/oracles/extension-api-contract.json");
        let oracle: Value = serde_json::from_slice(&std::fs::read(&oracle_path).unwrap()).unwrap();
        assert_eq!(
            oracle["baselines"],
            serde_json::json!([
                {
                    "commit": "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
                    "types_blob": "345d298206d9110ab3c6ad4ec4cae933ddd02bf5",
                    "types_sha256": "4dcb1fa0d8b71673ccdad9282e7d65140f16e6eec05cd8c826dd3242d55dbaef",
                    "types_bytes": 87576,
                    "types_lines": 2053,
                    "types_exports": 135,
                    "index_blob": "dbc86e2fe74d3edc344bbff1fd4c234d11dc0f02",
                    "index_sha256": "63558f4c3995067c3ff2b613af07738d33d3d6e8005423cc1560a346920891e0",
                    "index_bytes": 4941,
                    "index_lines": 165,
                    "index_exports": 139
                },
                {
                    "commit": "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
                    "types_blob": "c5134664a92bb6e933a165869ae816e467fc2283",
                    "types_sha256": "b3427a40e5379b0a0c78d4ef3a497c74706a7191c55f0777a429fbb87391437b",
                    "types_bytes": 82417,
                    "types_lines": 1924,
                    "types_exports": 125,
                    "index_blob": "e7038a7fad9df6b23b20eaee83c0c6de7b6805f5",
                    "index_sha256": "e109a24f72f3a5d19a9cdfecf071e300039ff58e55e7d1d58ec05f4c6efe4e1a",
                    "index_bytes": 4662,
                    "index_lines": 155,
                    "index_exports": 129
                }
            ])
        );

        let sections = oracle["types_sections"].as_array().unwrap();
        assert_eq!(sections.len(), 17);
        let mut next_byte = 0;
        let mut next_line = 1;
        let mut exports = Vec::new();
        for section in sections {
            assert_eq!(section["byte_start"].as_u64().unwrap(), next_byte);
            assert_eq!(section["line_start"].as_u64().unwrap(), next_line);
            next_byte = section["byte_end"].as_u64().unwrap();
            next_line = section["line_end"].as_u64().unwrap().saturating_add(1);
            assert!(!section["native_surfaces"].as_array().unwrap().is_empty());
            assert!(!section["adaptation"].as_str().unwrap().is_empty());
            for evidence in section["evidence"].as_array().unwrap() {
                let evidence = evidence.as_str().unwrap();
                assert!(
                    workspace.join(evidence).exists(),
                    "missing evidence {evidence}"
                );
            }
            exports.extend(
                section["exports"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|name| name.as_str().unwrap().to_owned()),
            );
        }
        assert_eq!(next_byte, 87_576);
        assert_eq!(next_line, 2_054);
        assert_eq!(exports.len(), 135);
        exports.sort();
        assert_eq!(
            exports.iter().map(String::as_str).collect::<Vec<_>>(),
            EXPECTED_EXPORTS
        );
        assert_eq!(exports.iter().collect::<BTreeSet<_>>().len(), exports.len());
        assert_eq!(
            oracle["index_contract"]["key_exports"],
            serde_json::json!([
                "ParsedKeyChord",
                "matchesKey",
                "matchesKeyChord",
                "parseKeyChord"
            ])
        );
        assert_eq!(oracle["index_contract"]["total_exports"], 139);
        for evidence in oracle["index_contract"]["native_surfaces"]
            .as_array()
            .unwrap()
        {
            let evidence = evidence.as_str().unwrap();
            assert!(
                workspace.join(evidence).exists(),
                "missing evidence {evidence}"
            );
        }
    }
}
