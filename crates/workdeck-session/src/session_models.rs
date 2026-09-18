//! App-owned session registrations, snapshots, tool inputs, and command results.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use workdeck_core::{ReviewNoteSource, ReviewSide};
use workdeck_review::ReviewPublicationAddress;

use crate::{
    SessionRegistration, SessionSelector, SessionServerMessage, SessionSnapshot,
    SessionTerminalMetadata, WorkdeckReviewActionEnvelopeV1, WorkdeckReviewActionResultV1,
    WorkdeckReviewResourceCatalogV1, WorkdeckReviewResourceReadEnvelopeV1,
    WorkdeckReviewResourceReadResultV1,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkdeckSessionInputKind {
    Vcs,
    Diff,
    Show,
    StashShow,
    Patch,
    Difftool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkdeckExperimentalFeature {
    Stml,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionLineHighlightTone {
    Match,
    Current,
    Info,
    Warning,
    Error,
    Dim,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionReloadReason {
    Watch,
    Daemon,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFileSummary {
    pub id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub additions: u64,
    pub deletions: u64,
    pub hunk_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReviewHunk {
    pub index: u64,
    pub header: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReviewFile {
    #[serde(flatten)]
    pub summary: SessionFileSummary,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<String>,
    pub hunks: Vec<SessionReviewHunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedHunkSummary {
    pub index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u64; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckSessionInfo {
    pub input_kind: WorkdeckSessionInputKind,
    pub title: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_features: Option<Vec<WorkdeckExperimentalFeature>>,
    pub files: Vec<SessionReviewFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_catalog: Option<WorkdeckReviewResourceCatalogV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_capability_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckSessionState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_file_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_file_path: Option<String>,
    pub selected_hunk_index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_hunk_old_range: Option<[u64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_hunk_new_range: Option<[u64; 2]>,
    pub show_agent_notes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_markup_width: Option<u64>,
    pub live_comment_count: u64,
    pub live_comments: Vec<SessionLiveCommentSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_note_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<Vec<SessionReviewNoteSummary>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_publication: Option<ReviewPublicationAddress>,
}

pub type WorkdeckSessionRegistration = SessionRegistration<WorkdeckSessionInfo>;
pub type WorkdeckSessionSnapshot = SessionSnapshot<WorkdeckSessionState>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentTargetInput {
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<ReviewSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    #[serde(flatten)]
    pub target: CommentTargetInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<bool>,
}

pub type CommentBatchItemInput = CommentTargetInput;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentBatchToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    pub comments: Vec<CommentBatchItemInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal_mode: Option<CommentBatchRevealMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentBatchRevealMode {
    None,
    First,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigateToHunkToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<ReviewSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment_direction: Option<CommentDirection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CommentDirection {
    Next,
    Prev,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadSessionToolInput<NextInput = Value> {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    pub next_input: NextInput,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveCommentToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    pub comment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearCommentsToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_user: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadReviewResourceToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    #[serde(flatten)]
    pub review: WorkdeckReviewResourceReadEnvelopeV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyReviewActionToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    #[serde(flatten)]
    pub review: WorkdeckReviewActionEnvelopeV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HighlightToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    pub file_path: String,
    pub side: ReviewSide,
    pub line: u64,
    pub start: u64,
    pub end: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tone: Option<SessionLineHighlightTone>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reveal: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearHighlightsToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QuitSessionToolInput {
    #[serde(flatten)]
    pub target_session: SessionSelector,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuitSessionResult {
    pub quitting: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLiveCommentSummary {
    pub comment_id: String,
    pub file_path: String,
    pub hunk_index: u64,
    pub side: ReviewSide,
    pub line: u64,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReviewNoteSummary {
    pub note_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub source: ReviewNoteSource,
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_index: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u64; 2]>,
    pub body: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub editable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedCommentResult {
    pub comment_id: String,
    pub file_id: String,
    pub file_path: String,
    pub hunk_index: u64,
    pub side: ReviewSide,
    pub line: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup_width: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup_notes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppliedCommentBatchResult {
    pub applied: Vec<AppliedCommentResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RevealedTarget {
    Line,
    Hunk,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigatedSelectionResult {
    pub file_id: String,
    pub file_path: String,
    pub hunk_index: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_hunk: Option<SelectedHunkSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed: Option<RevealedTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub side: Option<ReviewSide>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedHighlightResult {
    pub file_id: String,
    pub file_path: String,
    pub hunk_index: u64,
    pub side: ReviewSide,
    pub line: u64,
    pub start: u64,
    pub end: u64,
    pub tone: SessionLineHighlightTone,
    pub file_mark_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed: Option<RevealedTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearedHighlightsResult {
    pub removed_count: u64,
    pub remaining_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemovedCommentResult {
    pub comment_id: String,
    pub removed: bool,
    pub remaining_comment_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<ReviewNoteSource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearedCommentsResult {
    pub removed_count: u64,
    pub remaining_comment_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub include_user: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_live_comment_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed_user_note_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_live_comment_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remaining_user_note_count: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadSessionOptions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_app: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<SessionReloadReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reload_extensions: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadedSessionResult {
    pub session_id: String,
    pub input_kind: WorkdeckSessionInputKind,
    pub title: String,
    pub source_label: String,
    pub file_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_file_path: Option<String>,
    pub selected_hunk_index: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListedSession {
    pub session_id: String,
    pub pid: u64,
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub launched_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal: Option<SessionTerminalMetadata>,
    pub input_kind: WorkdeckSessionInputKind,
    pub title: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_features: Option<Vec<WorkdeckExperimentalFeature>>,
    pub file_count: u64,
    pub files: Vec<SessionFileSummary>,
    pub snapshot: WorkdeckSessionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedSessionContext {
    pub session_id: String,
    pub title: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub input_kind: WorkdeckSessionInputKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_features: Option<Vec<WorkdeckExperimentalFeature>>,
    pub selected_file: Option<SessionFileSummary>,
    pub selected_hunk: Option<SelectedHunkSummary>,
    pub show_agent_notes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note_markup_width: Option<u64>,
    pub live_comment_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionReview {
    pub session_id: String,
    pub title: String,
    pub source_label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub input_kind: WorkdeckSessionInputKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub experimental_features: Option<Vec<WorkdeckExperimentalFeature>>,
    pub selected_file: Option<SessionReviewFile>,
    pub selected_hunk: Option<SessionReviewHunk>,
    pub show_agent_notes: bool,
    pub live_comment_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_note_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_notes: Option<Vec<SessionReviewNoteSummary>>,
    pub files: Vec<SessionReviewFile>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkdeckSessionCommandResult {
    QuitSession(QuitSessionResult),
    AppliedComment(AppliedCommentResult),
    AppliedCommentBatch(AppliedCommentBatchResult),
    NavigatedSelection(NavigatedSelectionResult),
    RemovedComment(RemovedCommentResult),
    ClearedComments(ClearedCommentsResult),
    ReloadedSession(ReloadedSessionResult),
    ReviewAction(WorkdeckReviewActionResultV1),
    ReviewResource(WorkdeckReviewResourceReadResultV1),
    AppliedHighlight(AppliedHighlightResult),
    ClearedHighlights(ClearedHighlightsResult),
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkdeckSessionServerMessage {
    QuitSession(SessionServerMessage<String, QuitSessionToolInput>),
    Comment(SessionServerMessage<String, CommentToolInput>),
    CommentBatch(SessionServerMessage<String, CommentBatchToolInput>),
    NavigateToHunk(SessionServerMessage<String, NavigateToHunkToolInput>),
    ReloadSession(SessionServerMessage<String, ReloadSessionToolInput>),
    RemoveComment(SessionServerMessage<String, RemoveCommentToolInput>),
    ClearComments(SessionServerMessage<String, ClearCommentsToolInput>),
    ReadReviewResource(SessionServerMessage<String, ReadReviewResourceToolInput>),
    ApplyReviewAction(SessionServerMessage<String, ApplyReviewActionToolInput>),
    Highlight(SessionServerMessage<String, HighlightToolInput>),
    ClearHighlights(SessionServerMessage<String, ClearHighlightsToolInput>),
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;

    #[test]
    fn app_info_and_state_round_trip_legacy_optional_and_review_fields() {
        let value = json!({
            "inputKind": "diff",
            "title": "Working tree",
            "sourceLabel": "git diff",
            "experimentalFeatures": ["stml"],
            "files": [{
                "id": "runtime-1",
                "path": "src/main.rs",
                "additions": 3,
                "deletions": 1,
                "hunkCount": 1,
                "hunks": [{"index": 0, "header": "@@ -1 +1 @@"}]
            }],
            "reviewCapabilityDigest": "a".repeat(64),
        });
        let parsed: WorkdeckSessionInfo = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);

        let state = json!({
            "selectedHunkIndex": 0,
            "showAgentNotes": true,
            "liveCommentCount": 0,
            "liveComments": [],
            "reviewPublication": {"generation": "generation:p1:2", "stateRevision": 4}
        });
        let parsed: WorkdeckSessionState = serde_json::from_value(state.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), state);
    }

    #[test]
    fn flattened_tool_inputs_keep_session_selection_and_command_fields_on_one_wire_object() {
        let comment = CommentToolInput {
            target_session: SessionSelector {
                session_id: Some("session-1".into()),
                ..SessionSelector::default()
            },
            target: CommentTargetInput {
                file_path: "src/main.rs".into(),
                hunk_index: Some(0),
                side: None,
                line: None,
                summary: "Check this".into(),
                rationale: None,
                markup: None,
                author: None,
            },
            reveal: Some(true),
        };
        assert_eq!(
            serde_json::to_value(comment).unwrap(),
            json!({
                "sessionId": "session-1",
                "filePath": "src/main.rs",
                "hunkIndex": 0,
                "summary": "Check this",
                "reveal": true,
            })
        );

        let clear: ClearHighlightsToolInput = serde_json::from_value(json!({
            "repoRoot": "/repo",
            "filePath": "src/main.rs"
        }))
        .unwrap();
        assert_eq!(clear.target_session.repo_root, Some(PathBuf::from("/repo")));
    }

    #[test]
    fn result_models_preserve_ranges_markup_and_highlight_coordinates() {
        let value = json!({
            "commentId": "comment-1",
            "fileId": "runtime-1",
            "filePath": "src/main.rs",
            "hunkIndex": 0,
            "side": "new",
            "line": 4,
            "markupWidth": 80,
            "markupNotes": ["unsupported tag"]
        });
        let result: AppliedCommentResult = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), value);

        let highlight = json!({
            "fileId": "runtime-1",
            "filePath": "src/main.rs",
            "hunkIndex": 0,
            "side": "new",
            "line": 4,
            "start": 2,
            "end": 7,
            "tone": "warning",
            "fileMarkCount": 2,
            "revealed": "line"
        });
        let result: AppliedHighlightResult = serde_json::from_value(highlight.clone()).unwrap();
        assert_eq!(serde_json::to_value(result).unwrap(), highlight);
    }
}
