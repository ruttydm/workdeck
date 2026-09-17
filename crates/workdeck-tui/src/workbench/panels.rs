//! Read-only repository panel contracts supplied by the composition root.
//!
//! A provider is bound to one repository. Requests contain only relative
//! locations and typed targets; the provider validates them before I/O.

use std::{fmt::Debug, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PanelPage {
    Changes,
    Git,
    Files,
    Agents,
    Search,
}

impl PanelPage {
    pub const fn title(self) -> &'static str {
        match self {
            Self::Changes => "Changes",
            Self::Git => "Git",
            Self::Files => "Files",
            Self::Agents => "Agents",
            Self::Search => "Search",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryPanelSource {
    pub root: PathBuf,
    /// Stable identity chosen by the bound provider, including its repository.
    pub identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRequest {
    pub page: PanelPage,
    /// Repository-relative directory; empty means the repository root.
    pub directory: String,
    pub query: String,
    pub limit: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelTarget {
    Change { path: String, staged: bool },
    File { path: String, line: Option<u32> },
    Directory { path: String },
    GitSummary,
    Commit { reference: String },
    Branch { reference: String },
    Stash { reference: String },
    Tag { reference: String },
    Remote { name: String },
    AgentSession { id: String },
    Issue { id: String },
    Initiative { id: String },
    Project { id: String },
    Milestone { id: String },
    Cycle { id: String },
    Target { id: String },
    Label { id: String },
}

impl PanelTarget {
    pub(super) fn reference(&self) -> Option<&str> {
        match self {
            Self::Change { path, .. } | Self::File { path, .. } | Self::Directory { path } => {
                Some(path)
            }
            Self::Commit { reference }
            | Self::Branch { reference }
            | Self::Stash { reference }
            | Self::Tag { reference } => Some(reference),
            Self::Remote { name } => Some(name),
            Self::AgentSession { id }
            | Self::Issue { id }
            | Self::Initiative { id }
            | Self::Project { id }
            | Self::Milestone { id }
            | Self::Cycle { id }
            | Self::Target { id }
            | Self::Label { id } => Some(id),
            Self::GitSummary => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PanelChangeStats {
    pub additions: usize,
    pub deletions: usize,
    pub staged: bool,
    pub unstaged: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelEntry {
    pub id: String,
    pub label: String,
    pub detail: String,
    /// Git section or change-directory group; an empty string omits grouping.
    pub section: String,
    pub target: PanelTarget,
    pub changes: Option<PanelChangeStats>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelSnapshot {
    pub page: PanelPage,
    pub title: String,
    pub summary: String,
    pub entries: Vec<PanelEntry>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PanelPreviewKind {
    #[default]
    Text,
    Markdown,
    Source,
    Diff,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelPreview {
    pub title: String,
    pub body: String,
    pub kind: PanelPreviewKind,
    pub truncated: bool,
    pub binary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelError {
    pub message: String,
    pub hint: Option<String>,
}

impl PanelError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            hint: None,
        }
    }
}

pub trait RepositoryPanelProvider: Debug + Send + Sync {
    /// Pure identity accessor. The value must remain stable for this instance.
    fn source(&self) -> RepositoryPanelSource;
    fn load(&self, request: &PanelRequest) -> Result<PanelSnapshot, PanelError>;
    fn preview(&self, target: &PanelTarget) -> Result<PanelPreview, PanelError>;
    /// Normalized `keys.base` binding for the Git page; `None` disables cycling.
    fn git_base_key(&self) -> Option<String> {
        None
    }
    /// Rotate the session-local Git comparison base and name the next branch.
    fn cycle_git_base_branch(&self) -> Result<String, PanelError> {
        Err(PanelError::new(
            "This repository panel cannot cycle its comparison base",
        ))
    }
}
