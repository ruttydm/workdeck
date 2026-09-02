use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ViewNode;
use workdeck_core::AgentFileContext;

/// One parsed hunk summarized without exposing renderer-specific metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionDiffHunk {
    pub index: usize,
    pub header: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionDiffStats {
    pub additions: usize,
    pub deletions: usize,
}

/// Provider-neutral reviewed file exposed to native extensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionDiffFile {
    pub id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub patch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub stats: ExtensionDiffStats,
    pub change_type: String,
    #[serde(default)]
    pub stats_truncated: bool,
    pub hunks: Vec<ExtensionDiffHunk>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<AgentFileContext>,
    #[serde(default)]
    pub is_untracked: bool,
    #[serde(default)]
    pub is_binary: bool,
    #[serde(default)]
    pub is_too_large: bool,
}

/// A side of a reviewed source document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionFileSide {
    Old,
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSelectionLine {
    pub side: ExtensionFileSide,
    pub line: u32,
}

/// Immutable provider-neutral selection captured for one native command invocation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionReviewSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<ExtensionDiffFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_index: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_line: Option<ExtensionReviewSelectionLine>,
}

/// One added or removed source-line range, inclusive on both ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFileChangeRange {
    pub hunk_index: usize,
    pub kind: ExtensionFileChangeKind,
    pub range: [usize; 2],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionFileChangeKind {
    Added,
    Removed,
}

/// One exact-source range associated with a host-owned file-view row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionFileViewSourceRange {
    pub side: ExtensionFileSide,
    /// Inclusive, one-based source line range.
    pub range: [usize; 2],
}

/// A generic semantic color mapped to the active terminal theme at paint time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExtensionFileViewTone {
    Muted,
    Accent,
    AccentMuted,
    Syntax,
    Added,
    Removed,
}

/// Theme-independent terminal emphasis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExtensionTextAttribute {
    Bold,
    Italic,
    Underline,
    Strikethrough,
}

/// One symbolic run in a host-rendered file-view row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionFileViewSpan {
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<ExtensionFileViewTone>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<ExtensionTextAttribute>,
}

/// A bounded custom row described without transferring renderer ownership.
///
/// Hunk's in-process extension API accepted a React render callback. Workdeck's
/// native subprocess protocol carries the same fixed-height fallback contract
/// as a declarative view tree which the host validates and paints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFileViewRowComponent {
    pub height: usize,
    pub content: ViewNode,
    /// Alternate declarative paint tree used while this row's owning hunk is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_content: Option<ViewNode>,
    /// Alternate declarative paint tree selected by ephemeral host-owned row state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expanded_content: Option<ViewNode>,
    /// Expanded paint tree used while this row's owning hunk is selected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_expanded_content: Option<ViewNode>,
    /// Cooperatively consume an un-dragged left-button mouse-up inside this row.
    #[serde(default)]
    pub toggle_expanded_on_left_mouse_up: bool,
    /// Optional prefixes applied to the first painted line from current hunk selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selection_prefix: Option<ExtensionFileViewSelectionPrefix>,
}

/// Paint-only selection marker for a fixed-height declarative component row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtensionFileViewSelectionPrefix {
    pub selected: String,
    pub unselected: String,
}

/// A row in a host-owned, terminal-safe file-view layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFileViewRow {
    pub id: String,
    pub spans: Vec<ExtensionFileViewSpan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_ranges: Vec<ExtensionFileViewSourceRange>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<ExtensionFileViewRowComponent>,
}

/// Inclusive row extents corresponding positionally to one source hunk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFileViewHunkRows {
    pub start_row: usize,
    pub end_row: usize,
}

/// The deterministic symbolic layout returned by a native file-view extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtensionFileViewLayout {
    pub rows: Vec<ExtensionFileViewRow>,
    pub hunk_rows: Vec<ExtensionFileViewHunkRows>,
}

/// One host request asking a registered native view whether it accepts a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewMatchRequest {
    pub view_id: String,
    pub file: ExtensionDiffFile,
}

/// Immutable input for one native file-view layout calculation.
///
/// Exact source documents cross the subprocess boundary as owned snapshots. This
/// keeps extensions unable to race subsequent reloads while preserving Hunk's
/// `readDocument(side)` semantics without a nested, re-entrant RPC exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewLayoutRequest {
    pub view_id: String,
    pub file: ExtensionDiffFile,
    pub width: usize,
    pub changes: Vec<ExtensionFileChangeRange>,
    pub documents: BTreeMap<ExtensionFileSide, Option<String>>,
    #[serde(default)]
    pub aborted: bool,
}

/// Host-validated layout plus terminal row measurements retained for painting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidatedFileViewLayout {
    pub layout: ExtensionFileViewLayout,
    pub row_heights: Vec<usize>,
}

/// Identify one host-rendered file-presentation row failure for warning attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileViewRowFailure {
    pub extension_id: String,
    pub view_id: String,
    pub file_id: String,
    pub file_path: String,
    pub row_id: String,
    pub layout_generation: u64,
    pub message: String,
}
