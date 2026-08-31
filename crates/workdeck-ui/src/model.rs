use serde::{Deserialize, Serialize};

use workdeck_api::SearchResultKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Inbox,
    Workspaces,
    Git,
    Search,
    PullRequests,
    Ci,
    Artifacts,
    Changes,
}

impl Area {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Inbox => "Updates",
            Self::Workspaces => "Workspaces",
            Self::Git => "Commits",
            Self::Search => "Search",
            Self::PullRequests => "Pull requests",
            Self::Ci => "CI",
            Self::Artifacts => "Artifacts",
            Self::Changes => "Changes",
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Inbox => "inbox",
            Self::Workspaces => "workspaces",
            Self::Git => "git",
            Self::Search => "search",
            Self::PullRequests => "pull-requests",
            Self::Ci => "ci",
            Self::Artifacts => "artifacts",
            Self::Changes => "changes",
        }
    }

    pub const fn shell_label(self) -> &'static str {
        if self.is_git_workspace() {
            "Git"
        } else {
            self.label()
        }
    }

    pub const fn is_git_workspace(self) -> bool {
        matches!(self, Self::Git | Self::PullRequests)
    }

    pub fn from_id(value: &str) -> Option<Self> {
        Some(match value {
            "inbox" => Self::Inbox,
            "workspaces" => Self::Workspaces,
            "git" => Self::Git,
            "search" => Self::Search,
            "pull-requests" => Self::PullRequests,
            "ci" => Self::Ci,
            "artifacts" => Self::Artifacts,
            "changes" | "review" => Self::Changes,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewLens {
    Diff,
    Split,
    Source,
    Markdown,
    Calls,
    Structure,
    Ast,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchActivation {
    pub kind: SearchResultKind,
    pub id: String,
    pub target: String,
}

impl ReviewLens {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Diff => "Diff",
            Self::Split => "Split",
            Self::Source => "Source",
            Self::Markdown => "Markdown",
            Self::Calls => "Calls",
            Self::Structure => "Structure",
            Self::Ast => "AST",
        }
    }
}
