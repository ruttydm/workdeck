//! Fixed Workdeck parser registry shared by the daemon and producer connections.

use std::sync::Arc;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{
    ApplyReviewActionToolInput, ClearCommentsToolInput, ClearHighlightsToolInput,
    CommentBatchToolInput, CommentToolInput, HighlightToolInput, NavigateToHunkToolInput,
    ParserRegistryError, ReadReviewResourceToolInput, ReloadSessionToolInput,
    RemoveCommentToolInput, SessionBrokerAppParserRegistry, SessionBrokerCommandParsers,
    SessionBrokerProtocolParsers, SessionDaemonAction, SessionDaemonResponse,
    WORKDECK_REVIEW_PROTOCOL_VERSION, WORKDECK_SESSION_DAEMON_VERSION,
    WorkdeckReviewActionResultV1, WorkdeckReviewFailureV1, WorkdeckReviewParseResult,
    WorkdeckReviewResourceReadResultV1, WorkdeckSessionCommandResult, WorkdeckSessionInfo,
    WorkdeckSessionRegistration, WorkdeckSessionSnapshot, WorkdeckSessionState,
    create_session_broker_protocol_parsers, parse_daemon_cli_input, parse_session_daemon_response,
    parse_workdeck_review_action_envelope, parse_workdeck_review_resource_read_envelope,
    parse_workdeck_session_registration, parse_workdeck_session_snapshot,
};

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
const SELECTOR_FIELDS: [&str; 4] = ["sessionId", "sessionPath", "repoRoot", "repoBoundary"];

/// Strictly parsed input for one registered Workdeck broker command.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(untagged)]
pub enum WorkdeckSessionCommandInput {
    QuitSession(crate::QuitSessionToolInput),
    Comment(CommentToolInput),
    CommentBatch(CommentBatchToolInput),
    NavigateToHunk(NavigateToHunkToolInput),
    ReloadSession(ReloadSessionToolInput),
    RemoveComment(RemoveCommentToolInput),
    ClearComments(ClearCommentsToolInput),
    ReadReviewResource(ReadReviewResourceToolInput),
    ApplyReviewAction(ApplyReviewActionToolInput),
    Highlight(HighlightToolInput),
    ClearHighlights(ClearHighlightsToolInput),
}

pub type WorkdeckSessionProtocolParsers = SessionBrokerProtocolParsers<
    WorkdeckSessionInfo,
    WorkdeckSessionState,
    WorkdeckSessionCommandInput,
    WorkdeckSessionCommandResult,
>;

fn exact<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Option<&'a Map<String, Value>> {
    crate::parse_exact_broker_record(value, required, optional).ok()
}

fn exact_with_selectors<'a>(
    value: &'a Value,
    required: &[&str],
    optional: &[&str],
) -> Option<&'a Map<String, Value>> {
    let all_optional = optional
        .iter()
        .copied()
        .chain(SELECTOR_FIELDS)
        .collect::<Vec<_>>();
    exact(value, required, &all_optional)
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

fn bounded_string(value: &Value, minimum: usize, maximum: usize) -> bool {
    value.as_str().is_some_and(|value| {
        let length = utf16_len(value);
        length >= minimum && length <= maximum
    })
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

fn side(value: &Value) -> bool {
    matches!(value.as_str(), Some("old" | "new"))
}

fn optional_matches(
    record: &Map<String, Value>,
    key: &str,
    predicate: impl FnOnce(&Value) -> bool,
) -> bool {
    record.get(key).is_none_or(predicate)
}

fn valid_selector_fields(record: &Map<String, Value>) -> bool {
    optional_matches(record, "sessionId", |value| bounded_string(value, 1, 128))
        && ["sessionPath", "repoRoot", "repoBoundary"]
            .iter()
            .all(|key| optional_matches(record, key, |value| bounded_string(value, 1, 4_096)))
}

fn valid_optional_text(record: &Map<String, Value>, key: &str) -> bool {
    optional_matches(record, key, |value| bounded_string(value, 1, 4_096))
}

fn decode<T: DeserializeOwned>(value: &Value) -> Option<T> {
    serde_json::from_value(value.clone()).ok()
}

fn valid_comment_item(value: &Value) -> bool {
    exact(
        value,
        &["filePath", "summary"],
        &["hunkIndex", "side", "line", "rationale", "markup", "author"],
    )
    .is_some_and(|record| {
        bounded_string(&record["filePath"], 1, 4_096)
            && bounded_string(&record["summary"], 1, 4_096)
            && optional_matches(record, "hunkIndex", nonnegative)
            && optional_matches(record, "side", side)
            && optional_matches(record, "line", positive)
            && ["rationale", "markup", "author"]
                .iter()
                .all(|key| valid_optional_text(record, key))
    })
}

fn review_envelope_value(record: &Map<String, Value>, keys: &[&str]) -> Value {
    Value::Object(
        keys.iter()
            .filter_map(|key| {
                record
                    .get(*key)
                    .map(|value| ((*key).to_owned(), value.clone()))
            })
            .collect(),
    )
}

fn parse_command_input(command: &str, value: &Value) -> Option<WorkdeckSessionCommandInput> {
    match command {
        "quit_session" => {
            let record = exact_with_selectors(value, &[], &[])?;
            valid_selector_fields(record)
                .then(|| decode(value).map(WorkdeckSessionCommandInput::QuitSession))?
        }
        "comment" => {
            let record = exact_with_selectors(
                value,
                &["filePath", "summary"],
                &[
                    "hunkIndex",
                    "side",
                    "line",
                    "rationale",
                    "markup",
                    "author",
                    "reveal",
                ],
            )?;
            (valid_selector_fields(record)
                && bounded_string(&record["filePath"], 1, 4_096)
                && bounded_string(&record["summary"], 1, 4_096)
                && optional_matches(record, "hunkIndex", nonnegative)
                && optional_matches(record, "side", side)
                && optional_matches(record, "line", positive)
                && ["rationale", "markup", "author"]
                    .iter()
                    .all(|key| valid_optional_text(record, key))
                && optional_matches(record, "reveal", Value::is_boolean))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::Comment))?
        }
        "comment_batch" => {
            let record = exact_with_selectors(value, &["comments"], &["revealMode"])?;
            (valid_selector_fields(record)
                && record["comments"]
                    .as_array()
                    .is_some_and(|comments| comments.iter().all(valid_comment_item))
                && optional_matches(record, "revealMode", |value| {
                    matches!(value.as_str(), Some("none" | "first"))
                }))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::CommentBatch))?
        }
        "navigate_to_hunk" => {
            let record = exact_with_selectors(
                value,
                &[],
                &["filePath", "hunkIndex", "side", "line", "commentDirection"],
            )?;
            (valid_selector_fields(record)
                && valid_optional_text(record, "filePath")
                && optional_matches(record, "hunkIndex", nonnegative)
                && optional_matches(record, "side", side)
                && optional_matches(record, "line", positive)
                && optional_matches(record, "commentDirection", |value| {
                    matches!(value.as_str(), Some("next" | "prev"))
                }))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::NavigateToHunk))?
        }
        "reload_session" => {
            let record = exact_with_selectors(value, &["nextInput"], &["sourcePath"])?;
            (valid_selector_fields(record)
                && parse_daemon_cli_input(&record["nextInput"]).is_some()
                && valid_optional_text(record, "sourcePath"))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::ReloadSession))?
        }
        "remove_comment" => {
            let record = exact_with_selectors(value, &["commentId"], &[])?;
            (valid_selector_fields(record) && bounded_string(&record["commentId"], 1, 128))
                .then(|| decode(value).map(WorkdeckSessionCommandInput::RemoveComment))?
        }
        "clear_comments" => {
            let record = exact_with_selectors(value, &[], &["filePath", "includeUser"])?;
            (valid_selector_fields(record)
                && valid_optional_text(record, "filePath")
                && optional_matches(record, "includeUser", Value::is_boolean))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::ClearComments))?
        }
        "read_review_resource" => {
            let record =
                exact_with_selectors(value, &["protocolVersion", "actor", "request"], &[])?;
            let envelope = review_envelope_value(record, &["protocolVersion", "actor", "request"]);
            (valid_selector_fields(record)
                && matches!(
                    parse_workdeck_review_resource_read_envelope(&envelope),
                    WorkdeckReviewParseResult::Parsed(_)
                ))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::ReadReviewResource))?
        }
        "apply_review_action" => {
            let record = exact_with_selectors(
                value,
                &["protocolVersion", "generation", "actor", "action"],
                &["expectedStateRevision"],
            )?;
            let envelope = review_envelope_value(
                record,
                &[
                    "protocolVersion",
                    "generation",
                    "expectedStateRevision",
                    "actor",
                    "action",
                ],
            );
            (valid_selector_fields(record)
                && matches!(
                    parse_workdeck_review_action_envelope(&envelope),
                    WorkdeckReviewParseResult::Parsed(_)
                ))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::ApplyReviewAction))?
        }
        "highlight" => {
            let record = exact_with_selectors(
                value,
                &["filePath", "side", "line", "start", "end"],
                &["tone", "reveal"],
            )?;
            (valid_selector_fields(record)
                && bounded_string(&record["filePath"], 1, 4_096)
                && side(&record["side"])
                && positive(&record["line"])
                && nonnegative(&record["start"])
                && positive(&record["end"])
                && optional_matches(record, "tone", |value| {
                    matches!(
                        value.as_str(),
                        Some("match" | "current" | "info" | "warning" | "error" | "dim")
                    )
                })
                && optional_matches(record, "reveal", Value::is_boolean))
            .then(|| decode(value).map(WorkdeckSessionCommandInput::Highlight))?
        }
        "clear_highlights" => {
            let record = exact_with_selectors(value, &[], &["filePath"])?;
            (valid_selector_fields(record) && valid_optional_text(record, "filePath"))
                .then(|| decode(value).map(WorkdeckSessionCommandInput::ClearHighlights))?
        }
        _ => None,
    }
}

fn standard_result(command: &str, value: &Value) -> Option<WorkdeckSessionCommandResult> {
    let action = match command {
        "comment" => SessionDaemonAction::CommentAdd,
        "comment_batch" => SessionDaemonAction::CommentApply,
        "navigate_to_hunk" => SessionDaemonAction::Navigate,
        "reload_session" => SessionDaemonAction::Reload,
        "remove_comment" => SessionDaemonAction::CommentRm,
        "clear_comments" => SessionDaemonAction::CommentClear,
        "highlight" => SessionDaemonAction::HighlightAdd,
        "clear_highlights" => SessionDaemonAction::HighlightClear,
        _ => return None,
    };
    let response = parse_session_daemon_response(action, &json!({"result": value})).ok()?;
    Some(match response {
        SessionDaemonResponse::CommentAdd { result } => {
            WorkdeckSessionCommandResult::AppliedComment(result)
        }
        SessionDaemonResponse::CommentApply { result } => {
            WorkdeckSessionCommandResult::AppliedCommentBatch(result)
        }
        SessionDaemonResponse::Navigate { result } => {
            WorkdeckSessionCommandResult::NavigatedSelection(result)
        }
        SessionDaemonResponse::Reload { result } => {
            WorkdeckSessionCommandResult::ReloadedSession(result)
        }
        SessionDaemonResponse::CommentRm { result } => {
            WorkdeckSessionCommandResult::RemovedComment(result)
        }
        SessionDaemonResponse::CommentClear { result } => {
            WorkdeckSessionCommandResult::ClearedComments(result)
        }
        SessionDaemonResponse::HighlightAdd { result } => {
            WorkdeckSessionCommandResult::AppliedHighlight(result)
        }
        SessionDaemonResponse::HighlightClear { result } => {
            WorkdeckSessionCommandResult::ClearedHighlights(result)
        }
        _ => return None,
    })
}

fn valid_review_failure(value: &Value) -> bool {
    exact(value, &["ok", "code", "message", "currentGeneration"], &[]).is_some_and(|record| {
        record["ok"] == false
            && matches!(
                record["code"].as_str(),
                Some(
                    "unknown-resource"
                        | "resource-unavailable"
                        | "resource-too-large"
                        | "resource-integrity"
                        | "invalid-range"
                        | "stale-generation"
                        | "invalid-request"
                        | "file-not-found"
                        | "hunk-not-found"
                        | "gap-not-found"
                        | "draft-missing"
                        | "note-not-found"
                        | "missing-fact"
                )
            )
            && record["message"].is_string()
            && record["currentGeneration"].is_string()
    })
}

fn parse_review_action_result(value: &Value) -> Option<WorkdeckSessionCommandResult> {
    if valid_review_failure(value) {
        let failure = decode::<WorkdeckReviewFailureV1>(value)?;
        return Some(WorkdeckSessionCommandResult::ReviewAction(
            WorkdeckReviewActionResultV1::Failed(failure),
        ));
    }
    let record = exact(value, &["ok", "generation", "stateRevision"], &[])?;
    if record["ok"] != true
        || !record["generation"].is_string()
        || !nonnegative(&record["stateRevision"])
    {
        return None;
    }
    decode(value).map(WorkdeckSessionCommandResult::ReviewAction)
}

fn parse_review_resource_result(value: &Value) -> Option<WorkdeckSessionCommandResult> {
    if valid_review_failure(value) {
        let failure = decode::<WorkdeckReviewFailureV1>(value)?;
        return Some(WorkdeckSessionCommandResult::ReviewResource(
            WorkdeckReviewResourceReadResultV1::Failed(failure),
        ));
    }
    let record = exact(value, &["ok", "chunk"], &[])?;
    let chunk = exact(
        &record["chunk"],
        &[
            "generation",
            "resourceId",
            "offset",
            "byteLength",
            "encoding",
            "data",
            "contentDigest",
            "contentSize",
            "eof",
        ],
        &[],
    )?;
    if record["ok"] != true
        || !["generation", "resourceId", "data", "contentDigest"]
            .iter()
            .all(|key| chunk[*key].is_string())
        || chunk["encoding"] != "base64"
        || !["offset", "byteLength", "contentSize"]
            .iter()
            .all(|key| nonnegative(&chunk[*key]))
        || !chunk["eof"].is_boolean()
    {
        return None;
    }
    decode(value).map(WorkdeckSessionCommandResult::ReviewResource)
}

fn parse_command_result(command: &str, value: &Value) -> Option<WorkdeckSessionCommandResult> {
    match command {
        "quit_session" => {
            let record = exact(value, &["quitting"], &[])?;
            (record["quitting"] == true)
                .then(|| decode(value).map(WorkdeckSessionCommandResult::QuitSession))?
        }
        "read_review_resource" => parse_review_resource_result(value),
        "apply_review_action" => parse_review_action_result(value),
        _ => standard_result(command, value),
    }
}

fn command_descriptor(
    command: &'static str,
) -> SessionBrokerCommandParsers<WorkdeckSessionCommandInput, WorkdeckSessionCommandResult> {
    SessionBrokerCommandParsers {
        command: command.into(),
        version: u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION),
        parse_input: Arc::new(move |value| parse_command_input(command, value)),
        parse_result: Arc::new(move |value| parse_command_result(command, value)),
    }
}

/// Build the immutable Workdeck application registry used on every broker boundary.
pub fn create_workdeck_session_protocol_parsers()
-> Result<WorkdeckSessionProtocolParsers, ParserRegistryError> {
    create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry {
        broker_revision: None,
        app_revision: u64::from(WORKDECK_SESSION_DAEMON_VERSION),
        features: Vec::new(),
        parse_registration: Arc::new(|value| -> Option<WorkdeckSessionRegistration> {
            parse_workdeck_session_registration(value)
        }),
        parse_snapshot: Arc::new(|value| -> Option<WorkdeckSessionSnapshot> {
            parse_workdeck_session_snapshot(value)
        }),
        commands: [
            "quit_session",
            "comment",
            "comment_batch",
            "navigate_to_hunk",
            "reload_session",
            "remove_comment",
            "clear_comments",
            "read_review_resource",
            "apply_review_action",
            "highlight",
            "clear_highlights",
        ]
        .into_iter()
        .map(command_descriptor)
        .collect(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::{
        BrokerProtocolFailureCode, SessionLineHighlightTone, SessionSelector,
        WORKDECK_REVIEW_PROTOCOL_VERSION,
    };

    use super::*;

    fn parsers() -> WorkdeckSessionProtocolParsers {
        create_workdeck_session_protocol_parsers().unwrap()
    }

    #[test]
    fn parsed_command_inputs_serialize_as_the_inner_wire_shape() {
        let input = WorkdeckSessionCommandInput::ClearHighlights(ClearHighlightsToolInput {
            target_session: SessionSelector {
                session_id: Some("session-1".into()),
                ..SessionSelector::default()
            },
            file_path: Some("src/main.rs".into()),
        });
        assert_eq!(
            serde_json::to_value(input).unwrap(),
            json!({"sessionId": "session-1", "filePath": "src/main.rs"})
        );
    }

    #[test]
    fn native_quit_accepts_only_selectors_and_an_explicit_quitting_result() {
        let parsers = parsers();
        for value in [
            json!({}),
            json!({"sessionId": "session-1"}),
            json!({"repoRoot": "/repo"}),
        ] {
            let parsed = parsers
                .parse_command_input("quit_session", 1, &value)
                .unwrap();
            assert!(matches!(
                parsed,
                WorkdeckSessionCommandInput::QuitSession(_)
            ));
            assert_eq!(serde_json::to_value(parsed).unwrap(), value);
        }
        for invalid in [
            json!({"sessionId": 7}),
            json!({"force": true}),
            json!({"filePath": "file"}),
        ] {
            assert!(
                parsers
                    .parse_command_input("quit_session", 1, &invalid)
                    .is_err()
            );
        }
        let result = parsers
            .parse_command_result("quit_session", 1, &json!({"quitting": true}))
            .unwrap();
        assert!(matches!(
            result,
            WorkdeckSessionCommandResult::QuitSession(_)
        ));
        for invalid in [
            json!({}),
            json!({"quitting": false}),
            json!({"quitting": "true"}),
            json!({"quitting": true, "extra": 1}),
        ] {
            assert!(
                parsers
                    .parse_command_result("quit_session", 1, &invalid)
                    .is_err()
            );
        }
        assert!(
            parsers
                .parse_command_input("quit_session", 2, &json!({}))
                .is_err()
        );
    }

    #[test]
    fn accepts_dim_line_highlight_input_and_result() {
        let parsers = parsers();
        let input = json!({
            "filePath": "src/App.tsx",
            "side": "new",
            "line": 42,
            "start": 6,
            "end": 19,
            "tone": "dim",
            "reveal": true,
        });
        let parsed = parsers
            .parse_command_input(
                "highlight",
                u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION),
                &input,
            )
            .unwrap();
        assert!(matches!(
            parsed,
            WorkdeckSessionCommandInput::Highlight(HighlightToolInput {
                tone: Some(SessionLineHighlightTone::Dim),
                ..
            })
        ));

        let result = json!({
            "fileId": "file-1",
            "filePath": "src/App.tsx",
            "hunkIndex": 0,
            "side": "new",
            "line": 42,
            "start": 6,
            "end": 19,
            "tone": "dim",
            "fileMarkCount": 1,
            "revealed": "line",
        });
        let parsed = parsers
            .parse_command_result(
                "highlight",
                u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION),
                &result,
            )
            .unwrap();
        assert!(matches!(
            parsed,
            WorkdeckSessionCommandResult::AppliedHighlight(result)
                if result.tone == SessionLineHighlightTone::Dim
        ));
    }

    #[test]
    fn registers_and_parses_every_command_input() {
        let version = u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION);
        let cases = [
            (
                "comment",
                json!({"filePath": "src/a.rs", "summary": "Check"}),
            ),
            ("comment_batch", json!({"comments": []})),
            ("navigate_to_hunk", json!({})),
            (
                "reload_session",
                json!({"nextInput": {"kind": "patch", "options": {}}}),
            ),
            ("remove_comment", json!({"commentId": "comment-1"})),
            ("clear_comments", json!({})),
            (
                "read_review_resource",
                json!({
                    "protocolVersion": WORKDECK_REVIEW_PROTOCOL_VERSION,
                    "actor": {"clientId": "client-1", "kind": "agent"},
                    "request": {
                        "generation": "generation:p1:3",
                        "resourceId": "resource:patch:file:0123456789abcdef",
                        "offset": 0,
                        "length": 1024,
                    },
                }),
            ),
            (
                "apply_review_action",
                json!({
                    "protocolVersion": WORKDECK_REVIEW_PROTOCOL_VERSION,
                    "generation": "generation:p1:3",
                    "actor": {"clientId": "client-1", "kind": "agent"},
                    "action": {"type": "notes/set-visibility", "visible": true},
                }),
            ),
            (
                "highlight",
                json!({"filePath": "src/a.rs", "side": "old", "line": 1, "start": 0, "end": 1}),
            ),
            ("clear_highlights", json!({})),
        ];
        let parsers = parsers();
        for (command, input) in cases {
            assert!(
                parsers
                    .parse_command_input(command, version, &input)
                    .is_ok(),
                "{command} rejected {input}"
            );
        }
    }

    #[test]
    fn rejects_unknown_fields_bad_selectors_coordinates_and_reload_inputs() {
        let parsers = parsers();
        let version = u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION);
        for (command, input) in [
            ("comment", json!({"filePath": "", "summary": "Check"})),
            (
                "comment",
                json!({"filePath": "src/a", "summary": "Check", "future": true}),
            ),
            ("navigate_to_hunk", json!({"sessionId": ""})),
            (
                "highlight",
                json!({"filePath": "src/a", "side": "new", "line": 0, "start": 0, "end": 1}),
            ),
            (
                "reload_session",
                json!({"nextInput": {"kind": "vcs", "range": "a", "rangeEndpoints": {"from": "a", "to": "b"}, "staged": false, "options": {}}}),
            ),
        ] {
            assert_eq!(
                parsers
                    .parse_command_input(command, version, &input)
                    .unwrap_err()
                    .code,
                BrokerProtocolFailureCode::InvalidAppPayload
            );
        }
    }

    #[test]
    fn review_results_keep_the_exact_baseline_failure_vocabulary() {
        let parsers = parsers();
        let version = u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION);
        let failure = json!({
            "ok": false,
            "code": "note-not-found",
            "message": "Missing note",
            "currentGeneration": "generation:p1:3",
        });
        assert!(
            parsers
                .parse_command_result("apply_review_action", version, &failure)
                .is_ok()
        );
        let mut unsupported = failure;
        unsupported["code"] = json!("draft-active");
        assert_eq!(
            parsers
                .parse_command_result("apply_review_action", version, &unsupported)
                .unwrap_err()
                .code,
            BrokerProtocolFailureCode::InvalidAppPayload
        );

        let resource = json!({
            "ok": true,
            "chunk": {
                "generation": "generation:p1:3",
                "resourceId": "resource:patch:file:0123456789abcdef",
                "offset": 0,
                "byteLength": 3,
                "encoding": "base64",
                "data": "YWJj",
                "contentDigest": "digest",
                "contentSize": 3,
                "eof": true,
            }
        });
        assert!(
            parsers
                .parse_command_result("read_review_resource", version, &resource)
                .is_ok()
        );
    }

    #[test]
    fn registry_uses_the_workdeck_daemon_revision_and_exact_command_version() {
        let parsers = parsers();
        assert_eq!(
            parsers.app_revision,
            u64::from(WORKDECK_SESSION_DAEMON_VERSION)
        );
        assert!(parsers.features.is_empty());
        assert_eq!(
            parsers
                .parse_command_input("clear_comments", 2, &json!({}))
                .unwrap_err()
                .code,
            BrokerProtocolFailureCode::UnknownCommand
        );
    }
}
