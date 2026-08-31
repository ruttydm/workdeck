use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::path::PathBuf;
use uuid::Uuid;

macro_rules! string_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new() -> Self {
                Self(format!("{}{}", $prefix, Uuid::now_v7()))
            }

            pub fn from_string(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self::from_string(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self::from_string(value)
            }
        }
    };
}

string_id!(ProjectId, "project_");
string_id!(RepositoryId, "repo_");
string_id!(CheckoutId, "checkout_");
string_id!(WorktreeId, "worktree_");
string_id!(ReviewSetId, "review_set_");
string_id!(SnapshotId, "snapshot_");
string_id!(ReviewUnitId, "unit_");
string_id!(ReviewUnitVersionId, "unit_version_");
string_id!(ThreadId, "thread_");
string_id!(ArtifactId, "artifact_");

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProject {
    pub id: ProjectId,
    pub name: String,
    pub description: String,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl WorkspaceProject {
    pub fn new(name: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: ProjectId::new(),
            name: name.into(),
            description: String::new(),
            archived: false,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryRecord {
    pub id: RepositoryId,
    pub project_id: ProjectId,
    pub name: String,
    pub provider: Option<String>,
    pub provider_owner: Option<String>,
    pub provider_name: Option<String>,
    pub normalized_remotes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutRecord {
    pub id: CheckoutId,
    pub repository_id: RepositoryId,
    pub path: PathBuf,
    pub git_common_dir: PathBuf,
    pub available: bool,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeRecord {
    pub id: WorktreeId,
    pub checkout_id: CheckoutId,
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
    pub available: bool,
    pub last_seen_at: DateTime<Utc>,
}

/// The last lightweight, read-only repository scan recorded by Workdeck.
///
/// This cache is deliberately separate from checkpoint snapshots. It makes the
/// global inbox immediately useful at launch while a fresh bounded scan runs in
/// the background, without fetching, reading file contents, or mutating the
/// repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeAttention {
    pub worktree_id: WorktreeId,
    pub change_count: usize,
    pub commit_count: usize,
    pub base_ref: Option<String>,
    /// Stable digest of the live HEAD plus the discovered upstream commit
    /// range. Working-tree edits are intentionally excluded: WIP is visible
    /// in Git but does not create an unread commit update.
    pub fingerprint: String,
    pub truncated: bool,
    pub error: Option<String>,
    pub scanned_at: DateTime<Utc>,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboxDisposition {
    Active,
    Pinned,
    Snoozed,
    Baseline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxPreference {
    pub worktree_id: WorktreeId,
    pub disposition: InboxDisposition,
    pub snoozed_until: Option<DateTime<Utc>>,
    pub baseline_signature: Option<String>,
    pub updated_at: DateTime<Utc>,
}

/// A durable, source-owned read position for Workdeck's activity inbox.
///
/// The key identifies a Git branch/worktree or provider pull request. The
/// revision is deliberately opaque: local Git uses the bounded attention
/// fingerprint while providers use their activity revision. Advancing either
/// source makes the item unread again without inventing a separate review
/// object or writing state into a repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    CommitBranch,
    PullRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityReadCursor {
    pub key: String,
    pub kind: ActivityKind,
    pub revision: String,
    pub read_at: DateTime<Utc>,
}

impl ActivityReadCursor {
    pub fn matches(&self, key: &str, revision: &str) -> bool {
        self.key == key && self.revision == revision
    }
}

impl InboxPreference {
    pub fn is_snoozed(&self, now: DateTime<Utc>) -> bool {
        self.disposition == InboxDisposition::Snoozed
            && self.snoozed_until.is_some_and(|until| until > now)
    }

    pub fn hides_signature(&self, signature: &str) -> bool {
        self.disposition == InboxDisposition::Baseline
            && self.baseline_signature.as_deref() == Some(signature)
    }
}

impl WorktreeAttention {
    pub fn is_dirty(&self) -> bool {
        self.change_count > 0
    }

    pub fn has_reviewable_work(&self) -> bool {
        self.change_count > 0 || self.commit_count > 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewSet {
    pub id: ReviewSetId,
    pub title: String,
    pub description: String,
    pub archived: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl ReviewSet {
    pub fn new(title: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: ReviewSetId::new(),
            title: title.into(),
            description: String::new(),
            archived: false,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ReviewSource {
    LocalWorktree {
        worktree_id: WorktreeId,
        base: Option<String>,
    },
    CommitRange {
        repository_id: RepositoryId,
        base: String,
        head: String,
    },
    PullRequest {
        provider: String,
        repository: String,
        number: u64,
    },
    Markdown {
        repository_id: RepositoryId,
        revision: Option<String>,
        path: PathBuf,
    },
    CiRun {
        provider: String,
        repository: String,
        run_id: String,
    },
    Artifact {
        artifact_id: ArtifactId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewCheckpoint {
    pub id: SnapshotId,
    pub review_set_id: ReviewSetId,
    pub sequence: u64,
    pub created_at: DateTime<Utc>,
    pub sources: Vec<SnapshotSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotSource {
    pub source: ReviewSource,
    pub revision: String,
    pub manifest_hash: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewUnitKind {
    File,
    Symbol,
    Ast,
    MarkdownSection,
    PlanClaim,
    Hunk,
    CiStep,
    LogAnnotation,
    ArtifactRegion,
    Contract,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAnchor {
    pub repository_id: Option<RepositoryId>,
    pub path: Option<PathBuf>,
    pub qualified_name: Option<String>,
    pub start_byte: Option<u64>,
    pub end_byte: Option<u64>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub content_hash: String,
    pub semantic_hash: String,
    pub parent_fingerprint: Option<String>,
    pub context_fingerprint: Option<String>,
}

impl ReviewAnchor {
    pub fn for_text(
        repository_id: Option<RepositoryId>,
        path: Option<PathBuf>,
        qualified_name: Option<String>,
        content: &str,
        semantic_content: &str,
    ) -> Self {
        Self {
            repository_id,
            path,
            qualified_name,
            start_byte: None,
            end_byte: None,
            start_line: None,
            end_line: None,
            content_hash: content_hash(content.as_bytes()),
            semantic_hash: content_hash(semantic_content.as_bytes()),
            parent_fingerprint: None,
            context_fingerprint: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewUnitVersion {
    pub id: ReviewUnitVersionId,
    pub unit_id: ReviewUnitId,
    pub snapshot_id: SnapshotId,
    pub kind: ReviewUnitKind,
    pub title: String,
    pub anchor: ReviewAnchor,
    pub provenance: String,
    pub confidence: AnalysisConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnalysisConfidence {
    Observed,
    LanguageServer,
    SyntaxInferred,
    Textual,
    Unresolved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMarkState {
    Seen,
    Reviewed,
    Accepted,
    Questioned,
    Resolved,
}

impl ReviewMarkState {
    /// Only explicit completion states close a review decision. Seeing or
    /// questioning a unit records useful durable context while keeping it in
    /// the reviewer's active queue.
    pub fn closes_review(self) -> bool {
        matches!(self, Self::Reviewed | Self::Accepted | Self::Resolved)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewMark {
    pub unit_version_id: ReviewUnitVersionId,
    pub state: ReviewMarkState,
    pub reviewer: String,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitTransition {
    Unchanged,
    Moved,
    Rebased,
    FormatOnly,
    Modified,
    New,
    Removed,
    DependencyImpact,
    Ambiguous,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewDelta {
    pub unit_id: ReviewUnitId,
    pub from_version: Option<ReviewUnitVersionId>,
    pub to_version: Option<ReviewUnitVersionId>,
    pub transition: UnitTransition,
    pub carry_review_state: bool,
    pub reason: String,
}

pub fn classify_unit_transition(
    previous: Option<&ReviewUnitVersion>,
    current: Option<&ReviewUnitVersion>,
) -> UnitTransition {
    match (previous, current) {
        (None, Some(_)) => UnitTransition::New,
        (Some(_), None) => UnitTransition::Removed,
        (None, None) => UnitTransition::Ambiguous,
        (Some(previous), Some(current)) => {
            if previous.anchor.content_hash == current.anchor.content_hash {
                if previous.anchor.path == current.anchor.path
                    && previous.anchor.qualified_name == current.anchor.qualified_name
                {
                    UnitTransition::Unchanged
                } else {
                    UnitTransition::Moved
                }
            } else if previous.anchor.semantic_hash == current.anchor.semantic_hash {
                UnitTransition::FormatOnly
            } else {
                UnitTransition::Modified
            }
        }
    }
}

pub fn transition_carries_review_state(transition: UnitTransition) -> bool {
    matches!(
        transition,
        UnitTransition::Unchanged
            | UnitTransition::Moved
            | UnitTransition::Rebased
            | UnitTransition::FormatOnly
            | UnitTransition::DependencyImpact
    )
}

pub fn content_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(path: &str, content: &str, semantic: &str) -> ReviewUnitVersion {
        ReviewUnitVersion {
            id: ReviewUnitVersionId::new(),
            unit_id: ReviewUnitId::new(),
            snapshot_id: SnapshotId::new(),
            kind: ReviewUnitKind::Symbol,
            title: "symbol".to_string(),
            anchor: ReviewAnchor::for_text(
                None,
                Some(path.into()),
                Some("module::symbol".to_string()),
                content,
                semantic,
            ),
            provenance: "test".to_string(),
            confidence: AnalysisConfidence::Observed,
        }
    }

    #[test]
    fn exact_content_at_new_path_is_moved() {
        let previous = version("old.rs", "fn a() {}", "fn a(){}");
        let mut current = version("new.rs", "fn a() {}", "fn a(){}");
        current.unit_id = previous.unit_id.clone();
        assert_eq!(
            classify_unit_transition(Some(&previous), Some(&current)),
            UnitTransition::Moved
        );
    }

    #[test]
    fn semantic_match_is_format_only() {
        let previous = version("a.rs", "fn a(){}", "fn a(){}");
        let mut current = version("a.rs", "fn a() { }", "fn a(){}");
        current.unit_id = previous.unit_id.clone();
        assert_eq!(
            classify_unit_transition(Some(&previous), Some(&current)),
            UnitTransition::FormatOnly
        );
        assert!(transition_carries_review_state(UnitTransition::FormatOnly));
    }

    #[test]
    fn changed_semantics_reopens_review() {
        let previous = version("a.rs", "fn a(){old()}", "fn a(){old()}");
        let current = version("a.rs", "fn a(){new()}", "fn a(){new()}");
        assert_eq!(
            classify_unit_transition(Some(&previous), Some(&current)),
            UnitTransition::Modified
        );
        assert!(!transition_carries_review_state(UnitTransition::Modified));
    }

    #[test]
    fn only_completion_marks_close_a_review_decision() {
        assert!(!ReviewMarkState::Seen.closes_review());
        assert!(!ReviewMarkState::Questioned.closes_review());
        assert!(ReviewMarkState::Reviewed.closes_review());
        assert!(ReviewMarkState::Accepted.closes_review());
        assert!(ReviewMarkState::Resolved.closes_review());
    }
}
