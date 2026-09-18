//! One-line daemon-side diagnosis of rejected session wire payloads.
//!
//! A rejection today closes the producer socket with a fixed reason and nothing else, which
//! turns a daemon/client version skew into a bisect. The description names the rejecting
//! parser and its top-level key path and never includes payload contents.

use serde_json::Value;

use crate::{
    SessionWireRejection, diagnose_workdeck_session_registration,
    diagnose_workdeck_session_snapshot,
};

pub const SESSION_WIRE_DEBUG_ENV: &str = "WORKDECK_DEBUG";

/// Which app-owned payload one rejection describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionWirePayloadKind {
    Registration,
    Snapshot,
}

impl SessionWirePayloadKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Registration => "registration",
            Self::Snapshot => "snapshot",
        }
    }
}

/// Read the session id from an unparsed payload defensively; it may be absent or malformed.
#[must_use]
pub fn read_session_wire_session_id(input: &Value) -> Option<String> {
    let session_id = input.get("sessionId")?.as_str()?;
    (1..=128)
        .contains(&session_id.len())
        .then(|| session_id.to_owned())
}

fn diagnose(kind: SessionWirePayloadKind, input: &Value) -> Option<SessionWireRejection> {
    match kind {
        SessionWirePayloadKind::Registration => diagnose_workdeck_session_registration(input),
        SessionWirePayloadKind::Snapshot => diagnose_workdeck_session_snapshot(input),
    }
}

/// Describe one rejected payload as a single log line without reflecting its contents.
#[must_use]
pub fn describe_session_wire_rejection(
    kind: SessionWirePayloadKind,
    input: &Value,
    session_id: Option<&str>,
) -> String {
    let rejection = diagnose(kind, input);
    let origin = session_id
        .filter(|id| !id.is_empty())
        .map_or_else(String::new, |id| format!(" from session {id}"));
    match rejection {
        None => format!(
            "rejected {}{}: the payload parses; the broker refused it for another reason",
            kind.as_str(),
            origin
        ),
        Some(rejection) => {
            let location = if rejection.path.is_empty() {
                " at the envelope".to_owned()
            } else {
                format!(" at {}", rejection.path)
            };
            format!(
                "rejected {}{}: {} returned null{}",
                kind.as_str(),
                origin,
                rejection.parser,
                location
            )
        }
    }
}

/// Log one rejected payload to the daemon's stderr when `WORKDECK_DEBUG=1`.
pub fn report_session_wire_rejection(
    kind: SessionWirePayloadKind,
    input: &Value,
    session_id: Option<&str>,
) {
    let debug = std::env::var(SESSION_WIRE_DEBUG_ENV).ok();
    report_session_wire_rejection_with(debug.as_deref(), kind, input, session_id, |line| {
        eprintln!("{line}");
    });
}

/// Debug-gated write of one rejection line; injectable for tests.
pub fn report_session_wire_rejection_with(
    debug: Option<&str>,
    kind: SessionWirePayloadKind,
    input: &Value,
    session_id: Option<&str>,
    write: impl FnMut(&str),
) {
    if debug != Some("1") {
        return;
    }
    let mut write = write;
    write(&format!(
        "[session:daemon] {}",
        describe_session_wire_rejection(kind, input, session_id)
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SESSION_BROKER_REGISTRATION_VERSION;
    use serde_json::json;

    fn valid_registration() -> Value {
        json!({
            "registrationVersion": SESSION_BROKER_REGISTRATION_VERSION,
            "sessionId": "session-1",
            "pid": 123,
            "cwd": "/repo",
            "launchedAt": "2026-03-22T00:00:00.000Z",
            "info": {
                "inputKind": "vcs",
                "title": "repo working tree",
                "sourceLabel": "/repo",
                "files": [{
                    "id": "file-1",
                    "path": "src/example.ts",
                    "additions": 1,
                    "deletions": 0,
                    "hunks": [{"index": 0, "header": "@@ -1 +1 @@"}],
                }],
            },
        })
    }

    fn valid_snapshot() -> Value {
        json!({
            "updatedAt": "2026-03-22T00:00:00.000Z",
            "state": {
                "selectedHunkIndex": 0,
                "showAgentNotes": true,
                "liveComments": [],
            },
        })
    }

    #[test]
    fn a_valid_registration_and_snapshot_report_no_rejection() {
        assert_eq!(
            diagnose_workdeck_session_registration(&valid_registration()),
            None
        );
        assert_eq!(diagnose_workdeck_session_snapshot(&valid_snapshot()), None);
    }

    #[test]
    fn names_the_innermost_parser_with_an_indexed_path() {
        let mut registration = valid_registration();
        registration["info"]["files"][0]["hunks"] = json!([
            {"index": 0, "header": "@@"},
            {"index": 1, "header": "@@"},
        ]);
        registration["info"]["files"][0]["hunks"][1]["index"] = json!(-1);
        assert_eq!(
            diagnose_workdeck_session_registration(&registration),
            Some(SessionWireRejection {
                parser: "parse_session_review_hunk",
                path: "info.files[0].hunks[1]".into(),
            })
        );
    }

    #[test]
    fn names_the_info_parser_for_an_unknown_top_level_info_key() {
        let mut registration = valid_registration();
        registration["info"]["surprise"] = json!(true);
        assert_eq!(
            diagnose_workdeck_session_registration(&registration),
            Some(SessionWireRejection {
                parser: "parse_workdeck_session_info",
                path: "info".into(),
            })
        );
    }

    #[test]
    fn names_the_envelope_parser_when_the_shared_envelope_itself_is_malformed() {
        let mut registration = valid_registration();
        registration["registrationVersion"] = json!(0);
        assert_eq!(
            diagnose_workdeck_session_registration(&registration),
            Some(SessionWireRejection {
                parser: "parse_session_registration_envelope",
                path: String::new(),
            })
        );
        assert_eq!(
            diagnose_workdeck_session_snapshot(&json!({"state": {}})),
            Some(SessionWireRejection {
                parser: "parse_session_snapshot_envelope",
                path: String::new(),
            })
        );
    }

    #[test]
    fn names_the_live_comment_parser_inside_a_snapshot() {
        let mut snapshot = valid_snapshot();
        snapshot["state"]["liveComments"] = json!([{
            "commentId": "c1",
            "filePath": "src/example.ts",
            "hunkIndex": 0,
            "side": "sideways",
            "line": 1,
            "summary": "x",
            "createdAt": "2026-03-22T00:00:00.000Z",
        }]);
        assert_eq!(
            diagnose_workdeck_session_snapshot(&snapshot),
            Some(SessionWireRejection {
                parser: "parse_session_live_comment",
                path: "state.liveComments[0]".into(),
            })
        );
    }

    #[test]
    fn describes_a_rejection_with_the_session_id_and_never_the_payload() {
        let mut registration = valid_registration();
        registration["sessionId"] = json!("abcdef12-rest");
        registration["info"]["surprise"] = json!("secret-value");
        let description = describe_session_wire_rejection(
            SessionWirePayloadKind::Registration,
            &registration,
            read_session_wire_session_id(&registration).as_deref(),
        );
        assert_eq!(
            description,
            "rejected registration from session abcdef12-rest: parse_workdeck_session_info returned null at info"
        );
        assert!(!description.contains("secret-value"));
    }

    #[test]
    fn describes_a_payload_that_parses_but_was_refused_anyway() {
        let description = describe_session_wire_rejection(
            SessionWirePayloadKind::Registration,
            &valid_registration(),
            None,
        );
        assert_eq!(
            description,
            "rejected registration: the payload parses; the broker refused it for another reason"
        );
    }

    #[test]
    fn logs_only_under_workdeck_debug_one() {
        let mut registration = valid_registration();
        registration["info"]["surprise"] = json!(true);
        let lines: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let collect = |line: &str| {
            lines
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(line.into());
        };
        report_session_wire_rejection_with(
            None,
            SessionWirePayloadKind::Registration,
            &registration,
            None,
            collect,
        );
        assert!(
            lines
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
        );
        report_session_wire_rejection_with(
            Some("0"),
            SessionWirePayloadKind::Registration,
            &registration,
            None,
            collect,
        );
        assert!(
            lines
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
        );
        report_session_wire_rejection_with(
            Some("1"),
            SessionWirePayloadKind::Registration,
            &registration,
            read_session_wire_session_id(&registration).as_deref(),
            collect,
        );
        assert_eq!(
            lines
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_slice(),
            [
                "[session:daemon] rejected registration from session session-1: parse_workdeck_session_info returned null at info"
            ]
        );
    }

    #[test]
    fn reads_only_bounded_nonempty_session_ids() {
        assert_eq!(
            read_session_wire_session_id(&json!({"sessionId": "abc"})),
            Some("abc".into())
        );
        assert_eq!(
            read_session_wire_session_id(&json!({"sessionId": ""})),
            None
        );
        assert_eq!(
            read_session_wire_session_id(&json!({"sessionId": "x".repeat(129)})),
            None
        );
        assert_eq!(read_session_wire_session_id(&json!({})), None);
    }
}
