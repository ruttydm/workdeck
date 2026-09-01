//! Strict JSON contract for the local session daemon's action surface.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use workdeck_core::ReviewSide;

use crate::{
    AppliedCommentBatchResult, AppliedCommentResult, AppliedHighlightResult, ClearedCommentsResult,
    ClearedHighlightsResult, ListedSession, NavigatedSelectionResult, ReloadedSessionResult,
    RemovedCommentResult, SelectedSessionContext, SessionDaemonAction, SessionDaemonCapabilities,
    SessionLineHighlightTone, SessionLiveCommentSummary, SessionReview, SessionReviewNoteSummary,
    SessionSelector, WORKDECK_SESSION_API_VERSION, WORKDECK_SESSION_DAEMON_VERSION,
};

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonLayoutMode {
    Auto,
    Split,
    Stack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonCursorLine {
    Row,
    Number,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DaemonSidebarVisibility {
    Visible(bool),
    Auto(DaemonSidebarAuto),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DaemonSidebarAuto {
    #[serde(rename = "auto")]
    Auto,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonCommonOptions {
    pub mode: Option<DaemonLayoutMode>,
    pub cursor_line: Option<DaemonCursorLine>,
    pub vcs: Option<String>,
    pub theme: Option<String>,
    pub agent_context: Option<String>,
    pub pager: Option<bool>,
    pub watch: Option<bool>,
    pub experimental: Option<bool>,
    pub fast: Option<bool>,
    pub exclude_untracked: Option<bool>,
    pub line_numbers: Option<bool>,
    pub tab_width: Option<u64>,
    pub file_gap: Option<u64>,
    pub hunk_gap: Option<u64>,
    pub wrap_lines: Option<bool>,
    pub hunk_headers: Option<bool>,
    pub menu_bar: Option<bool>,
    pub sidebar: Option<DaemonSidebarVisibility>,
    pub agent_notes: Option<bool>,
    pub copy_decorations: Option<bool>,
    pub prompt_save_view_preferences: Option<bool>,
    pub transparent_background: Option<bool>,
    pub color_moved: Option<bool>,
    pub extensions: Option<bool>,
    pub extension_paths: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonRangeEndpoints {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum DaemonCliInput {
    #[serde(rename = "vcs")]
    Vcs {
        #[serde(default)]
        range: Option<String>,
        #[serde(default)]
        range_endpoints: Option<DaemonRangeEndpoints>,
        staged: bool,
        #[serde(default)]
        pathspecs: Option<Vec<String>>,
        options: DaemonCommonOptions,
    },
    #[serde(rename = "show")]
    Show {
        #[serde(default, rename = "ref")]
        reference: Option<String>,
        #[serde(default)]
        pathspecs: Option<Vec<String>>,
        options: DaemonCommonOptions,
    },
    #[serde(rename = "stash-show")]
    StashShow {
        #[serde(default, rename = "ref")]
        reference: Option<String>,
        options: DaemonCommonOptions,
    },
    #[serde(rename = "diff")]
    Diff {
        left: String,
        right: String,
        options: DaemonCommonOptions,
    },
    #[serde(rename = "patch")]
    Patch {
        #[serde(default)]
        file: Option<String>,
        #[serde(default)]
        text: Option<String>,
        options: DaemonCommonOptions,
    },
    #[serde(rename = "difftool")]
    Difftool {
        left: String,
        right: String,
        #[serde(default)]
        path: Option<String>,
        options: DaemonCommonOptions,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DaemonCommentApplyItem {
    pub file_path: String,
    pub hunk_number: Option<u64>,
    pub side: Option<ReviewSide>,
    pub line: Option<u64>,
    pub summary: String,
    pub rationale: Option<String>,
    pub markup: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonRevealMode {
    None,
    First,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonCommentListType {
    Live,
    All,
    Ai,
    Agent,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DaemonCommentDirection {
    Next,
    Prev,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum SessionDaemonRequest {
    #[serde(rename = "list")]
    List,
    #[serde(rename = "get")]
    Get { selector: SessionSelector },
    #[serde(rename = "context")]
    Context { selector: SessionSelector },
    #[serde(rename = "review")]
    Review {
        selector: SessionSelector,
        #[serde(default)]
        include_patch: Option<bool>,
        #[serde(default)]
        include_notes: Option<bool>,
    },
    #[serde(rename = "navigate")]
    Navigate {
        selector: SessionSelector,
        #[serde(default)]
        file_path: Option<String>,
        #[serde(default)]
        hunk_number: Option<u64>,
        #[serde(default)]
        side: Option<ReviewSide>,
        #[serde(default)]
        line: Option<u64>,
        #[serde(default)]
        comment_direction: Option<DaemonCommentDirection>,
        #[serde(default)]
        comment_id: Option<String>,
    },
    #[serde(rename = "reload")]
    Reload {
        selector: SessionSelector,
        next_input: DaemonCliInput,
        #[serde(default)]
        source_path: Option<String>,
    },
    #[serde(rename = "comment-add")]
    CommentAdd {
        selector: SessionSelector,
        file_path: String,
        side: ReviewSide,
        line: u64,
        summary: String,
        #[serde(default)]
        rationale: Option<String>,
        #[serde(default)]
        markup: Option<String>,
        #[serde(default)]
        author: Option<String>,
        reveal: bool,
    },
    #[serde(rename = "comment-apply")]
    CommentApply {
        selector: SessionSelector,
        comments: Vec<DaemonCommentApplyItem>,
        reveal_mode: DaemonRevealMode,
    },
    #[serde(rename = "comment-list")]
    CommentList {
        selector: SessionSelector,
        #[serde(default)]
        file_path: Option<String>,
        #[serde(default, rename = "type")]
        list_type: Option<DaemonCommentListType>,
    },
    #[serde(rename = "comment-rm")]
    CommentRm {
        selector: SessionSelector,
        comment_id: String,
    },
    #[serde(rename = "comment-clear")]
    CommentClear {
        selector: SessionSelector,
        #[serde(default)]
        file_path: Option<String>,
        #[serde(default)]
        include_user: Option<bool>,
    },
    #[serde(rename = "highlight-add")]
    HighlightAdd {
        selector: SessionSelector,
        file_path: String,
        side: ReviewSide,
        line: u64,
        start: u64,
        end: u64,
        #[serde(default)]
        tone: Option<SessionLineHighlightTone>,
        reveal: bool,
    },
    #[serde(rename = "highlight-clear")]
    HighlightClear {
        selector: SessionSelector,
        #[serde(default)]
        file_path: Option<String>,
    },
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionDaemonProtocolError {
    #[error("Invalid session API request: {0}")]
    InvalidRequest(String),
    #[error("Invalid Workdeck session daemon response for {action}.")]
    InvalidResponse { action: String },
}

fn valid_common_options(options: &DaemonCommonOptions) -> bool {
    options
        .tab_width
        .is_none_or(|value| value > 0 && value <= MAX_SAFE_INTEGER)
        && options
            .file_gap
            .is_none_or(|value| value <= MAX_SAFE_INTEGER)
        && options
            .hunk_gap
            .is_none_or(|value| value <= MAX_SAFE_INTEGER)
}

fn valid_cli_input(input: &DaemonCliInput) -> bool {
    let options = match input {
        DaemonCliInput::Vcs {
            range,
            range_endpoints,
            options,
            ..
        } => {
            if range.is_some() && range_endpoints.is_some() {
                return false;
            }
            if range_endpoints
                .as_ref()
                .is_some_and(|range| range.from.is_empty() || range.to.is_empty())
            {
                return false;
            }
            options
        }
        DaemonCliInput::Show { options, .. }
        | DaemonCliInput::StashShow { options, .. }
        | DaemonCliInput::Diff { options, .. }
        | DaemonCliInput::Patch { options, .. }
        | DaemonCliInput::Difftool { options, .. } => options,
    };
    valid_common_options(options)
}

fn valid_positive(value: Option<u64>) -> bool {
    value.is_none_or(|value| value > 0 && value <= MAX_SAFE_INTEGER)
}

fn valid_request(request: &SessionDaemonRequest) -> Result<(), &'static str> {
    match request {
        SessionDaemonRequest::Navigate {
            hunk_number,
            line,
            comment_id,
            ..
        } => {
            if !valid_positive(*hunk_number) {
                return Err("hunkNumber");
            }
            if !valid_positive(*line) {
                return Err("line");
            }
            if comment_id.as_ref().is_some_and(String::is_empty) {
                return Err("commentId");
            }
        }
        SessionDaemonRequest::Reload { next_input, .. } if !valid_cli_input(next_input) => {
            return Err("nextInput");
        }
        SessionDaemonRequest::CommentAdd { line, .. } if *line == 0 || *line > MAX_SAFE_INTEGER => {
            return Err("line");
        }
        SessionDaemonRequest::CommentApply { comments, .. }
            if comments.iter().any(|comment| {
                !valid_positive(comment.hunk_number) || !valid_positive(comment.line)
            }) =>
        {
            return Err("comments");
        }
        SessionDaemonRequest::HighlightAdd {
            line, start, end, ..
        } => {
            if *line == 0 || *line > MAX_SAFE_INTEGER {
                return Err("line");
            }
            if *start > MAX_SAFE_INTEGER {
                return Err("start");
            }
            if *end == 0 || *end > MAX_SAFE_INTEGER {
                return Err("end");
            }
        }
        _ => {}
    }
    Ok(())
}

/// Parse one exact capability document; mismatches are treated as an incompatible daemon.
#[must_use]
pub fn parse_session_daemon_capabilities(value: &Value) -> Option<SessionDaemonCapabilities> {
    let capabilities = serde_json::from_value::<SessionDaemonCapabilities>(value.clone()).ok()?;
    (capabilities.version == WORKDECK_SESSION_API_VERSION
        && capabilities.daemon_version == WORKDECK_SESSION_DAEMON_VERSION)
        .then_some(capabilities)
}

pub fn parse_session_daemon_request(
    value: &Value,
) -> Result<SessionDaemonRequest, SessionDaemonProtocolError> {
    if let Some(field) = obvious_request_issue(value) {
        return Err(SessionDaemonProtocolError::InvalidRequest(field.into()));
    }
    let request =
        serde_json::from_value::<SessionDaemonRequest>(value.clone()).map_err(|error| {
            SessionDaemonProtocolError::InvalidRequest(
                error
                    .to_string()
                    .split(" at line")
                    .next()
                    .unwrap_or("invalid request")
                    .to_owned(),
            )
        })?;
    valid_request(&request)
        .map_err(|field| SessionDaemonProtocolError::InvalidRequest(field.into()))?;
    Ok(request)
}

fn obvious_request_issue(value: &Value) -> Option<&'static str> {
    let object = value.as_object()?;
    match object.get("action")?.as_str()? {
        "navigate" => object
            .get("hunkNumber")
            .filter(|value| !positive(value))
            .map(|_| "hunkNumber"),
        "comment-add" => {
            if object
                .get("side")
                .is_some_and(|value| !matches!(value.as_str(), Some("old" | "new")))
            {
                Some("side")
            } else {
                object
                    .get("line")
                    .filter(|value| !positive(value))
                    .map(|_| "line")
            }
        }
        "highlight-add" => {
            for (field, predicate) in [
                ("line", positive as fn(&Value) -> bool),
                ("start", nonnegative),
                ("end", positive),
            ] {
                if object.get(field).is_some_and(|value| !predicate(value)) {
                    return Some(field);
                }
            }
            object
                .get("tone")
                .filter(|value| {
                    !matches!(
                        value.as_str(),
                        Some("match" | "current" | "info" | "warning" | "error" | "dim")
                    )
                })
                .map(|_| "tone")
        }
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionCommentSummary {
    Live(SessionLiveCommentSummary),
    Review(SessionReviewNoteSummary),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SessionDaemonResponse {
    List {
        sessions: Vec<ListedSession>,
    },
    Get {
        session: Box<ListedSession>,
    },
    Context {
        context: Box<SelectedSessionContext>,
    },
    Review {
        review: Box<SessionReview>,
    },
    Navigate {
        result: NavigatedSelectionResult,
    },
    Reload {
        result: ReloadedSessionResult,
    },
    CommentAdd {
        result: AppliedCommentResult,
    },
    CommentApply {
        result: AppliedCommentBatchResult,
    },
    CommentList {
        comments: Vec<SessionCommentSummary>,
    },
    CommentRm {
        result: RemovedCommentResult,
    },
    CommentClear {
        result: ClearedCommentsResult,
    },
    HighlightAdd {
        result: AppliedHighlightResult,
    },
    HighlightClear {
        result: ClearedHighlightsResult,
    },
}

fn exact_object<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Option<&'a serde_json::Map<String, Value>> {
    let object = value.as_object()?;
    if object.len()
        != required.len()
            + optional
                .iter()
                .filter(|key| object.contains_key(**key))
                .count()
        || !required.iter().all(|key| object.contains_key(*key))
        || object
            .keys()
            .any(|key| !required.contains(&key.as_str()) && !optional.contains(&key.as_str()))
    {
        return None;
    }
    Some(object)
}

fn nonnegative(value: &Value) -> bool {
    value
        .as_u64()
        .is_some_and(|value| value <= MAX_SAFE_INTEGER)
}

fn positive(value: &Value) -> bool {
    value
        .as_u64()
        .is_some_and(|value| value > 0 && value <= MAX_SAFE_INTEGER)
}

fn line_range(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|range| range.len() == 2 && range.iter().all(nonnegative))
}

fn input_kind(value: &Value) -> bool {
    matches!(
        value.as_str(),
        Some("vcs" | "show" | "stash-show" | "diff" | "patch" | "difftool")
    )
}

fn experimental_features(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|features| features.iter().all(|feature| feature == "stml"))
}

fn terminal_location(value: &Value) -> bool {
    exact_object(
        value,
        &["source"],
        &[
            "tty",
            "windowId",
            "tabId",
            "paneId",
            "terminalId",
            "sessionId",
        ],
    )
    .is_some_and(|object| object.values().all(Value::is_string))
}

fn terminal(value: &Value) -> bool {
    exact_object(value, &["locations"], &["program"]).is_some_and(|object| {
        object.get("program").is_none_or(Value::is_string)
            && object["locations"]
                .as_array()
                .is_some_and(|locations| locations.iter().all(terminal_location))
    })
}

fn file_summary(value: &Value) -> bool {
    exact_object(
        value,
        &["id", "path", "additions", "deletions", "hunkCount"],
        &["previousPath"],
    )
    .is_some_and(|object| {
        object["id"].is_string()
            && object["path"].is_string()
            && object.get("previousPath").is_none_or(Value::is_string)
            && nonnegative(&object["additions"])
            && nonnegative(&object["deletions"])
            && nonnegative(&object["hunkCount"])
    })
}

fn review_hunk(value: &Value) -> bool {
    exact_object(value, &["index", "header"], &["oldRange", "newRange"]).is_some_and(|object| {
        nonnegative(&object["index"])
            && object["header"].is_string()
            && object.get("oldRange").is_none_or(line_range)
            && object.get("newRange").is_none_or(line_range)
    })
}

fn selected_hunk(value: &Value) -> bool {
    exact_object(value, &["index"], &["oldRange", "newRange"]).is_some_and(|object| {
        nonnegative(&object["index"])
            && object.get("oldRange").is_none_or(line_range)
            && object.get("newRange").is_none_or(line_range)
    })
}

fn review_file(value: &Value) -> bool {
    let Some(object) = exact_object(
        value,
        &["id", "path", "additions", "deletions", "hunkCount", "hunks"],
        &["previousPath", "patch"],
    ) else {
        return false;
    };
    file_summary(&Value::Object(
        object
            .iter()
            .filter(|(key, _)| key.as_str() != "hunks" && key.as_str() != "patch")
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect(),
    )) && object.get("patch").is_none_or(Value::is_string)
        && object["hunks"]
            .as_array()
            .is_some_and(|hunks| hunks.iter().all(review_hunk))
}

fn live_comment(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "commentId",
            "filePath",
            "hunkIndex",
            "side",
            "line",
            "summary",
            "createdAt",
        ],
        &["rationale", "author"],
    )
    .is_some_and(|object| {
        object["commentId"].is_string()
            && object["filePath"].is_string()
            && nonnegative(&object["hunkIndex"])
            && matches!(object["side"].as_str(), Some("old" | "new"))
            && positive(&object["line"])
            && object["summary"].is_string()
            && object["createdAt"].is_string()
            && object.get("rationale").is_none_or(Value::is_string)
            && object.get("author").is_none_or(Value::is_string)
    })
}

fn review_note(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "noteId",
            "source",
            "filePath",
            "body",
            "createdAt",
            "editable",
        ],
        &[
            "parentId",
            "hunkIndex",
            "oldRange",
            "newRange",
            "title",
            "author",
            "updatedAt",
        ],
    )
    .is_some_and(|object| {
        object["noteId"].is_string()
            && matches!(object["source"].as_str(), Some("ai" | "agent" | "user"))
            && object["filePath"].is_string()
            && object["body"].is_string()
            && object["createdAt"].is_string()
            && object["editable"].is_boolean()
            && object.get("parentId").is_none_or(Value::is_string)
            && object.get("hunkIndex").is_none_or(nonnegative)
            && object.get("oldRange").is_none_or(line_range)
            && object.get("newRange").is_none_or(line_range)
            && object.get("title").is_none_or(Value::is_string)
            && object.get("author").is_none_or(Value::is_string)
            && object.get("updatedAt").is_none_or(Value::is_string)
    })
}

fn review_publication(value: &Value) -> bool {
    exact_object(value, &["generation", "stateRevision"], &[]).is_some_and(|object| {
        object["generation"].is_string() && nonnegative(&object["stateRevision"])
    })
}

fn snapshot(value: &Value) -> bool {
    let Some(object) = exact_object(value, &["updatedAt", "state"], &[]) else {
        return false;
    };
    if !object["updatedAt"].is_string() {
        return false;
    }
    exact_object(
        &object["state"],
        &[
            "selectedHunkIndex",
            "showAgentNotes",
            "liveCommentCount",
            "liveComments",
        ],
        &[
            "selectedFileId",
            "selectedFilePath",
            "selectedHunkOldRange",
            "selectedHunkNewRange",
            "noteMarkupWidth",
            "reviewNoteCount",
            "reviewNotes",
            "reviewPublication",
        ],
    )
    .is_some_and(|state| {
        nonnegative(&state["selectedHunkIndex"])
            && state["showAgentNotes"].is_boolean()
            && nonnegative(&state["liveCommentCount"])
            && state["liveComments"]
                .as_array()
                .is_some_and(|comments| comments.iter().all(live_comment))
            && state.get("selectedFileId").is_none_or(Value::is_string)
            && state.get("selectedFilePath").is_none_or(Value::is_string)
            && state.get("selectedHunkOldRange").is_none_or(line_range)
            && state.get("selectedHunkNewRange").is_none_or(line_range)
            && state.get("noteMarkupWidth").is_none_or(nonnegative)
            && state.get("reviewNoteCount").is_none_or(nonnegative)
            && state.get("reviewNotes").is_none_or(|notes| {
                notes
                    .as_array()
                    .is_some_and(|notes| notes.iter().all(review_note))
            })
            && state
                .get("reviewPublication")
                .is_none_or(review_publication)
    })
}

fn listed_session(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "sessionId",
            "pid",
            "cwd",
            "launchedAt",
            "inputKind",
            "title",
            "sourceLabel",
            "fileCount",
            "files",
            "snapshot",
        ],
        &["repoRoot", "terminal", "experimentalFeatures"],
    )
    .is_some_and(|object| {
        object["sessionId"].is_string()
            && positive(&object["pid"])
            && object["cwd"].is_string()
            && object["launchedAt"].is_string()
            && input_kind(&object["inputKind"])
            && object["title"].is_string()
            && object["sourceLabel"].is_string()
            && nonnegative(&object["fileCount"])
            && object["files"]
                .as_array()
                .is_some_and(|files| files.iter().all(file_summary))
            && snapshot(&object["snapshot"])
            && object.get("repoRoot").is_none_or(Value::is_string)
            && object.get("terminal").is_none_or(terminal)
            && object
                .get("experimentalFeatures")
                .is_none_or(experimental_features)
    })
}

fn selected_context(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "sessionId",
            "title",
            "sourceLabel",
            "inputKind",
            "selectedFile",
            "selectedHunk",
            "showAgentNotes",
            "liveCommentCount",
        ],
        &["cwd", "repoRoot", "experimentalFeatures", "noteMarkupWidth"],
    )
    .is_some_and(|object| {
        object["sessionId"].is_string()
            && object["title"].is_string()
            && object["sourceLabel"].is_string()
            && input_kind(&object["inputKind"])
            && (object["selectedFile"].is_null() || file_summary(&object["selectedFile"]))
            && (object["selectedHunk"].is_null() || selected_hunk(&object["selectedHunk"]))
            && object["showAgentNotes"].is_boolean()
            && nonnegative(&object["liveCommentCount"])
            && object.get("cwd").is_none_or(Value::is_string)
            && object.get("repoRoot").is_none_or(Value::is_string)
            && object
                .get("experimentalFeatures")
                .is_none_or(experimental_features)
            && object.get("noteMarkupWidth").is_none_or(nonnegative)
    })
}

fn session_review(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "sessionId",
            "title",
            "sourceLabel",
            "inputKind",
            "selectedFile",
            "selectedHunk",
            "showAgentNotes",
            "liveCommentCount",
            "files",
        ],
        &[
            "cwd",
            "repoRoot",
            "experimentalFeatures",
            "reviewNoteCount",
            "reviewNotes",
        ],
    )
    .is_some_and(|object| {
        object["sessionId"].is_string()
            && object["title"].is_string()
            && object["sourceLabel"].is_string()
            && input_kind(&object["inputKind"])
            && (object["selectedFile"].is_null() || review_file(&object["selectedFile"]))
            && (object["selectedHunk"].is_null() || review_hunk(&object["selectedHunk"]))
            && object["showAgentNotes"].is_boolean()
            && nonnegative(&object["liveCommentCount"])
            && object["files"]
                .as_array()
                .is_some_and(|files| files.iter().all(review_file))
            && object.get("cwd").is_none_or(Value::is_string)
            && object.get("repoRoot").is_none_or(Value::is_string)
            && object
                .get("experimentalFeatures")
                .is_none_or(experimental_features)
            && object.get("reviewNoteCount").is_none_or(nonnegative)
            && object.get("reviewNotes").is_none_or(|notes| {
                notes
                    .as_array()
                    .is_some_and(|notes| notes.iter().all(review_note))
            })
    })
}

fn applied_comment(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "commentId",
            "fileId",
            "filePath",
            "hunkIndex",
            "side",
            "line",
        ],
        &["markupWidth", "markupNotes"],
    )
    .is_some_and(|object| {
        object["commentId"].is_string()
            && object["fileId"].is_string()
            && object["filePath"].is_string()
            && nonnegative(&object["hunkIndex"])
            && matches!(object["side"].as_str(), Some("old" | "new"))
            && positive(&object["line"])
            && object.get("markupWidth").is_none_or(nonnegative)
            && object.get("markupNotes").is_none_or(|notes| {
                notes
                    .as_array()
                    .is_some_and(|notes| notes.iter().all(Value::is_string))
            })
    })
}

fn applied_comment_batch(value: &Value) -> bool {
    exact_object(value, &["applied"], &[]).is_some_and(|object| {
        object["applied"]
            .as_array()
            .is_some_and(|applied| applied.iter().all(applied_comment))
    })
}

fn navigated_selection(value: &Value) -> bool {
    exact_object(
        value,
        &["fileId", "filePath", "hunkIndex"],
        &["selectedHunk", "revealed", "side", "line"],
    )
    .is_some_and(|object| {
        object["fileId"].is_string()
            && object["filePath"].is_string()
            && nonnegative(&object["hunkIndex"])
            && object.get("selectedHunk").is_none_or(selected_hunk)
            && object
                .get("revealed")
                .is_none_or(|value| matches!(value.as_str(), Some("line" | "hunk")))
            && object
                .get("side")
                .is_none_or(|value| matches!(value.as_str(), Some("old" | "new")))
            && object.get("line").is_none_or(positive)
    })
}

fn reloaded_session(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "sessionId",
            "inputKind",
            "title",
            "sourceLabel",
            "fileCount",
            "selectedHunkIndex",
        ],
        &["selectedFilePath"],
    )
    .is_some_and(|object| {
        object["sessionId"].is_string()
            && input_kind(&object["inputKind"])
            && object["title"].is_string()
            && object["sourceLabel"].is_string()
            && nonnegative(&object["fileCount"])
            && nonnegative(&object["selectedHunkIndex"])
            && object.get("selectedFilePath").is_none_or(Value::is_string)
    })
}

fn removed_comment(value: &Value) -> bool {
    exact_object(
        value,
        &["commentId", "removed", "remainingCommentCount"],
        &["source"],
    )
    .is_some_and(|object| {
        object["commentId"].is_string()
            && object["removed"].is_boolean()
            && nonnegative(&object["remainingCommentCount"])
            && object
                .get("source")
                .is_none_or(|value| matches!(value.as_str(), Some("ai" | "agent" | "user")))
    })
}

fn cleared_comments(value: &Value) -> bool {
    exact_object(
        value,
        &["removedCount", "remainingCommentCount"],
        &[
            "filePath",
            "includeUser",
            "removedLiveCommentCount",
            "removedUserNoteCount",
            "remainingLiveCommentCount",
            "remainingUserNoteCount",
        ],
    )
    .is_some_and(|object| {
        nonnegative(&object["removedCount"])
            && nonnegative(&object["remainingCommentCount"])
            && object.get("filePath").is_none_or(Value::is_string)
            && object.get("includeUser").is_none_or(Value::is_boolean)
            && [
                "removedLiveCommentCount",
                "removedUserNoteCount",
                "remainingLiveCommentCount",
                "remainingUserNoteCount",
            ]
            .iter()
            .all(|key| object.get(*key).is_none_or(nonnegative))
    })
}

fn applied_highlight(value: &Value) -> bool {
    exact_object(
        value,
        &[
            "fileId",
            "filePath",
            "hunkIndex",
            "side",
            "line",
            "start",
            "end",
            "tone",
            "fileMarkCount",
        ],
        &["revealed"],
    )
    .is_some_and(|object| {
        object["fileId"].is_string()
            && object["filePath"].is_string()
            && nonnegative(&object["hunkIndex"])
            && matches!(object["side"].as_str(), Some("old" | "new"))
            && positive(&object["line"])
            && nonnegative(&object["start"])
            && positive(&object["end"])
            && matches!(
                object["tone"].as_str(),
                Some("match" | "current" | "info" | "warning" | "error" | "dim")
            )
            && nonnegative(&object["fileMarkCount"])
            && object
                .get("revealed")
                .is_none_or(|value| matches!(value.as_str(), Some("line" | "hunk")))
    })
}

fn cleared_highlights(value: &Value) -> bool {
    exact_object(value, &["removedCount", "remainingCount"], &["filePath"]).is_some_and(|object| {
        nonnegative(&object["removedCount"])
            && nonnegative(&object["remainingCount"])
            && object.get("filePath").is_none_or(Value::is_string)
    })
}

fn valid_response(action: SessionDaemonAction, value: &Value) -> bool {
    match action {
        SessionDaemonAction::List => {
            exact_object(value, &["sessions"], &[]).is_some_and(|object| {
                object["sessions"]
                    .as_array()
                    .is_some_and(|sessions| sessions.iter().all(listed_session))
            })
        }
        SessionDaemonAction::Get => exact_object(value, &["session"], &[])
            .is_some_and(|object| listed_session(&object["session"])),
        SessionDaemonAction::Context => exact_object(value, &["context"], &[])
            .is_some_and(|object| selected_context(&object["context"])),
        SessionDaemonAction::Review => exact_object(value, &["review"], &[])
            .is_some_and(|object| session_review(&object["review"])),
        SessionDaemonAction::Navigate => exact_object(value, &["result"], &[])
            .is_some_and(|object| navigated_selection(&object["result"])),
        SessionDaemonAction::Reload => exact_object(value, &["result"], &[])
            .is_some_and(|object| reloaded_session(&object["result"])),
        SessionDaemonAction::CommentAdd => exact_object(value, &["result"], &[])
            .is_some_and(|object| applied_comment(&object["result"])),
        SessionDaemonAction::CommentApply => exact_object(value, &["result"], &[])
            .is_some_and(|object| applied_comment_batch(&object["result"])),
        SessionDaemonAction::CommentList => {
            exact_object(value, &["comments"], &[]).is_some_and(|object| {
                object["comments"].as_array().is_some_and(|comments| {
                    comments
                        .iter()
                        .all(|value| live_comment(value) || review_note(value))
                })
            })
        }
        SessionDaemonAction::CommentRm => exact_object(value, &["result"], &[])
            .is_some_and(|object| removed_comment(&object["result"])),
        SessionDaemonAction::CommentClear => exact_object(value, &["result"], &[])
            .is_some_and(|object| cleared_comments(&object["result"])),
        SessionDaemonAction::HighlightAdd => exact_object(value, &["result"], &[])
            .is_some_and(|object| applied_highlight(&object["result"])),
        SessionDaemonAction::HighlightClear => exact_object(value, &["result"], &[])
            .is_some_and(|object| cleared_highlights(&object["result"])),
    }
}

fn invalid_response(action: SessionDaemonAction) -> SessionDaemonProtocolError {
    SessionDaemonProtocolError::InvalidResponse {
        action: serde_json::to_value(action)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "unknown".into()),
    }
}

fn decode_response<T: DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

pub fn parse_session_daemon_response(
    action: SessionDaemonAction,
    value: &Value,
) -> Result<SessionDaemonResponse, SessionDaemonProtocolError> {
    if !valid_response(action, value) {
        return Err(invalid_response(action));
    }
    let response = match action {
        SessionDaemonAction::List => SessionDaemonResponse::List {
            sessions: decode_response(&value["sessions"])
                .ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::Get => SessionDaemonResponse::Get {
            session: Box::new(
                decode_response(&value["session"]).ok_or_else(|| invalid_response(action))?,
            ),
        },
        SessionDaemonAction::Context => SessionDaemonResponse::Context {
            context: Box::new(
                decode_response(&value["context"]).ok_or_else(|| invalid_response(action))?,
            ),
        },
        SessionDaemonAction::Review => SessionDaemonResponse::Review {
            review: Box::new(
                decode_response(&value["review"]).ok_or_else(|| invalid_response(action))?,
            ),
        },
        SessionDaemonAction::Navigate => SessionDaemonResponse::Navigate {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::Reload => SessionDaemonResponse::Reload {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::CommentAdd => SessionDaemonResponse::CommentAdd {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::CommentApply => SessionDaemonResponse::CommentApply {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::CommentList => SessionDaemonResponse::CommentList {
            comments: decode_response(&value["comments"])
                .ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::CommentRm => SessionDaemonResponse::CommentRm {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::CommentClear => SessionDaemonResponse::CommentClear {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::HighlightAdd => SessionDaemonResponse::HighlightAdd {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
        SessionDaemonAction::HighlightClear => SessionDaemonResponse::HighlightClear {
            result: decode_response(&value["result"]).ok_or_else(|| invalid_response(action))?,
        },
    };
    Ok(response)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn selector() -> Value {
        json!({"sessionId": "s-1"})
    }

    #[test]
    fn daemon_revision_is_the_structured_two_endpoint_revision() {
        assert_eq!(WORKDECK_SESSION_DAEMON_VERSION, 12);
    }

    #[test]
    fn capabilities_are_exact_versioned_and_action_typed() {
        let valid = json!({
            "version": WORKDECK_SESSION_API_VERSION,
            "daemonVersion": WORKDECK_SESSION_DAEMON_VERSION,
            "actions": ["list", "get"]
        });
        assert_eq!(
            serde_json::to_value(parse_session_daemon_capabilities(&valid).unwrap()).unwrap(),
            valid
        );
        for invalid in [
            Value::Null,
            json!([]),
            json!({
                "version": WORKDECK_SESSION_API_VERSION,
                "daemonVersion": WORKDECK_SESSION_DAEMON_VERSION,
                "actions": ["unknown"]
            }),
            json!({
                "version": WORKDECK_SESSION_API_VERSION,
                "daemonVersion": WORKDECK_SESSION_DAEMON_VERSION,
                "actions": ["list"],
                "extra": true
            }),
        ] {
            assert_eq!(parse_session_daemon_capabilities(&invalid), None);
        }
    }

    #[test]
    fn every_wire_shaped_action_payload_is_accepted() {
        let requests = vec![
            json!({"action": "list"}),
            json!({"action": "get", "selector": selector()}),
            json!({"action": "context", "selector": {"repoRoot": "/repo/nested", "repoBoundary": "/repo"}}),
            json!({"action": "review", "selector": selector()}),
            json!({"action": "review", "selector": selector(), "includePatch": true, "includeNotes": true}),
            json!({"action": "navigate", "selector": selector(), "hunkNumber": 2}),
            json!({"action": "navigate", "selector": selector(), "filePath": "a.ts", "side": "new", "line": 12}),
            json!({"action": "navigate", "selector": selector(), "commentDirection": "next"}),
            json!({"action": "navigate", "selector": selector(), "commentId": "comment-1"}),
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "show", "ref": "HEAD~1", "options": {}}}),
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "vcs", "rangeEndpoints": {"from": "main", "to": "feature"}, "staged": false, "options": {}}}),
            json!({"action": "comment-add", "selector": selector(), "filePath": "a.ts", "side": "new", "line": 1, "summary": "note", "reveal": false}),
            json!({"action": "comment-apply", "selector": selector(), "comments": [{"filePath": "a.ts", "summary": "note", "hunkNumber": 2}], "revealMode": "first"}),
            json!({"action": "comment-list", "selector": selector(), "type": "user"}),
            json!({"action": "comment-rm", "selector": selector(), "commentId": "c-1"}),
            json!({"action": "comment-clear", "selector": selector(), "includeUser": true}),
            json!({"action": "highlight-add", "selector": selector(), "filePath": "a.ts", "side": "new", "line": 12, "start": 0, "end": 8, "tone": "warning", "reveal": true}),
            json!({"action": "highlight-add", "selector": {"repoRoot": "/repo"}, "filePath": "a.ts", "side": "old", "line": 3, "start": 4, "end": 9, "reveal": false}),
            json!({"action": "highlight-clear", "selector": selector(), "filePath": "a.ts"}),
            json!({"action": "highlight-clear", "selector": selector()}),
        ];
        for request in requests {
            assert!(parse_session_daemon_request(&request).is_ok(), "{request}");
        }
    }

    #[test]
    fn navigation_response_accepts_zero_based_ranges() {
        let value = json!({
            "result": {
                "fileId": "file-1",
                "filePath": "new-file.ts",
                "hunkIndex": 0,
                "selectedHunk": {"index": 0, "oldRange": [0, 0], "newRange": [0, 4]}
            }
        });
        let parsed = parse_session_daemon_response(SessionDaemonAction::Navigate, &value).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), value);
    }

    #[test]
    fn malformed_action_responses_have_one_stable_error() {
        for (action, value) in [
            (
                SessionDaemonAction::List,
                json!({"sessions": "not-an-array"}),
            ),
            (
                SessionDaemonAction::Get,
                json!({"session": {"sessionId": "partial"}}),
            ),
            (
                SessionDaemonAction::Context,
                json!({"context": {"sessionId": "partial"}}),
            ),
            (
                SessionDaemonAction::Review,
                json!({"review": {"files": []}}),
            ),
            (
                SessionDaemonAction::Navigate,
                json!({"result": {"fileId": "file-1", "filePath": "a.ts", "hunkIndex": -1}}),
            ),
            (
                SessionDaemonAction::CommentList,
                json!({"comments": [{"commentId": "partial"}]}),
            ),
            (
                SessionDaemonAction::HighlightClear,
                json!({"result": {"removedCount": "two", "remainingCount": 0}}),
            ),
        ] {
            let error = parse_session_daemon_response(action, &value).unwrap_err();
            assert!(
                error
                    .to_string()
                    .starts_with("Invalid Workdeck session daemon response for")
            );
        }
        let extra = json!({
            "result": {"fileId": "file-1", "filePath": "a.ts", "hunkIndex": 0, "unknown": true}
        });
        assert!(parse_session_daemon_response(SessionDaemonAction::Navigate, &extra).is_err());
    }

    #[test]
    fn malformed_highlight_fields_name_the_rejected_coordinate_or_tone() {
        for (field, value) in [("start", json!(-1)), ("tone", json!("loud"))] {
            let mut request = json!({
                "action": "highlight-add",
                "selector": selector(),
                "filePath": "a.ts",
                "side": "new",
                "line": 12,
                "start": 0,
                "end": 8,
                "reveal": false
            });
            request[field] = value;
            assert!(
                parse_session_daemon_request(&request)
                    .unwrap_err()
                    .to_string()
                    .contains(field)
            );
        }
    }

    #[test]
    fn unknown_wrongly_typed_and_extra_request_fields_are_rejected() {
        assert!(parse_session_daemon_request(&json!({"action": "self-destruct"})).is_err());
        for value in [
            json!({"action": "navigate", "selector": selector(), "hunkNumber": "2"}),
            json!({"action": "comment-rm", "selector": selector(), "commentId": "c-1", "extra": true}),
            json!({"action": "comment-add", "selector": selector(), "filePath": "a.ts", "side": "sideways", "line": 1, "summary": "note", "reveal": false}),
        ] {
            assert!(parse_session_daemon_request(&value).is_err());
        }
    }

    #[test]
    fn deterministic_nested_command_mutations_are_rejected() {
        let malformed = vec![
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "vcs", "staged": false, "options": {"tabWidth": 0}}}),
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "patch", "options": {}, "unknown": true}}),
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "vcs", "rangeEndpoints": {"from": "main"}, "staged": false, "options": {}}}),
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "vcs", "rangeEndpoints": {"from": "", "to": "feature"}, "staged": false, "options": {}}}),
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "vcs", "rangeEndpoints": {"from": "main", "to": "feature", "injected": true}, "staged": false, "options": {}}}),
            json!({"action": "reload", "selector": selector(), "nextInput": {"kind": "vcs", "range": "main..feature", "rangeEndpoints": {"from": "main", "to": "feature"}, "staged": false, "options": {}}}),
            json!({"action": "comment-apply", "selector": selector(), "comments": [{"filePath": "a.ts", "summary": "note", "hunkNumber": 0}], "revealMode": "first"}),
        ];
        for value in malformed {
            assert!(parse_session_daemon_request(&value).is_err(), "{value}");
        }
        for index in 0..8 {
            let selector = if index % 2 == 0 {
                json!({"sessionId": index})
            } else {
                json!({"sessionId": "s-1", "extra": index})
            };
            assert!(
                parse_session_daemon_request(&json!({
                    "action": "navigate",
                    "selector": selector,
                    "hunkNumber": index + 1,
                }))
                .is_err()
            );
        }
    }

    #[test]
    fn nonobjects_and_missing_required_fields_are_rejected() {
        for value in [
            json!("list"),
            Value::Null,
            json!({"action": "comment-rm", "selector": selector()}),
            json!({"action": "reload", "selector": selector()}),
        ] {
            assert!(parse_session_daemon_request(&value).is_err());
        }
    }
}
