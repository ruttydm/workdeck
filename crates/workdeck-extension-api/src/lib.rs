//! Workdeck native extension API v1.
//!
//! Third-party extensions are executables speaking JSON-RPC 2.0 over newline-delimited stdio.
//! The host retains terminal ownership: extensions return declarative views and actions rather
//! than terminal escape sequences or Ratatui widgets.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use thiserror::Error;
use workdeck_core::{Changeset, ReviewSide, ReviewSnapshot};

pub const API_VERSION: u32 = 1;
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub const DEFAULT_REQUEST_TIMEOUT_MS: u64 = 2_000;
pub const MAX_VIEW_NODES: usize = 10_000;
pub const MAX_VIEW_DEPTH: usize = 64;

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
}
