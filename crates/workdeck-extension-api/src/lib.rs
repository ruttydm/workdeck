//! Workdeck native extension API v1.
//!
//! Third-party extensions are executables speaking JSON-RPC 2.0 over newline-delimited stdio.
//! The host retains terminal ownership: extensions return declarative views and actions rather
//! than terminal escape sequences or Ratatui widgets.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;
use workdeck_core::{Changeset, ReviewSide, ReviewSnapshot};

pub use workdeck_core::{WORKDECK_EXTENSION_USER_ERROR_NAME, WorkdeckExtensionUserError};

pub const API_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 2_000;
pub const MAX_VIEW_NODES: usize = 10_000;
pub const MAX_VIEW_DEPTH: usize = 64;

/// Frozen, method-free keyboard snapshot passed across the extension boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandshakeResponse {
    pub extension_api_version: u32,
    pub extension_version: String,
    #[serde(default)]
    pub registrations: Vec<Registration>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Registration {
    Command(CommandRegistration),
    CliCommand(CliCommandRegistration),
    Pane(PaneRegistration),
    Theme(ThemeRegistration),
    VcsAdapter { id: String, markers: Vec<String> },
    ChangesetTransform { id: String },
    FileView { id: String, priority: i32 },
    FileLanguage(FileLanguageRegistration),
    KeyboardMode { id: String },
    LineHighlighter { id: String },
    EventSubscription { names: Vec<String> },
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
            Self::KeyboardMode { id } => format!("keyboard-mode:{id}"),
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
            Self::KeyboardMode { .. } => Capability::KeyboardModes,
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
pub struct CliCommandRegistration {
    pub name: String,
    pub summary: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PanePlacement {
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRegistration {
    pub id: String,
    pub title: String,
    pub placement: PanePlacement,
    pub preferred_size: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThemeRegistration {
    pub id: String,
    pub base: Option<String>,
    #[serde(default)]
    pub colors: std::collections::BTreeMap<String, String>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewEvent {
    pub name: String,
    pub snapshot: ReviewSnapshot,
    #[serde(default)]
    pub payload: Value,
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaneRenderResponse {
    pub content: ViewNode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
