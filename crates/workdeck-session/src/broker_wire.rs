//! Shared broker envelopes and strict parsers for application-owned registration and state.

use crate::{
    BrokerProtocolResult, BrokerStringOptions, parse_broker_app_payload, parse_broker_identifier,
    parse_broker_safe_integer, parse_broker_string, parse_exact_broker_record,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// Version live broker registration separately from Workdeck's public session CLI API.
pub const SESSION_BROKER_REGISTRATION_VERSION: u64 = 2;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub type SessionTargetInput = crate::SessionSelector;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTerminalLocation {
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tty: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pane_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTerminalMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    pub locations: Vec<SessionTerminalLocation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRegistration<Info = Value> {
    pub registration_version: u64,
    pub session_id: String,
    pub pid: u64,
    pub cwd: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo_root: Option<String>,
    pub launched_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub terminal: Option<SessionTerminalMetadata>,
    pub info: Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSnapshot<State = Value> {
    pub updated_at: String,
    pub state: State,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionClientMessage<Info = Value, State = Value, ResultValue = Value> {
    Register {
        registration: SessionRegistration<Info>,
        snapshot: SessionSnapshot<State>,
    },
    Snapshot {
        session_id: String,
        snapshot: SessionSnapshot<State>,
    },
    Heartbeat {
        session_id: String,
    },
    CommandResult {
        request_id: String,
        outcome: SessionCommandOutcome<ResultValue>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommandOutcome<ResultValue> {
    Success { result: ResultValue },
    Failure { error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionServerMessage<CommandName = String, Input = Value> {
    pub request_id: String,
    pub command: CommandName,
    pub command_version: Option<u64>,
    pub input: Input,
}

#[must_use]
pub fn broker_as_record(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

#[must_use]
pub fn parse_required_broker_string(value: &Value) -> Option<String> {
    parse_broker_string(value, BrokerStringOptions::default())
        .ok()
        .map(str::to_owned)
}

pub fn parse_optional_broker_string(value: Option<&Value>) -> BrokerProtocolResult<Option<String>> {
    value
        .map(|value| parse_broker_string(value, BrokerStringOptions::default()))
        .transpose()
        .map(|value| value.map(str::to_owned))
}

#[must_use]
pub fn parse_nonnegative_broker_integer(value: &Value) -> Option<u64> {
    parse_broker_safe_integer(value, 0, MAX_SAFE_INTEGER).ok()
}

#[must_use]
pub fn parse_positive_broker_integer(value: &Value) -> Option<u64> {
    parse_broker_safe_integer(value, 1, MAX_SAFE_INTEGER).ok()
}

fn parse_session_terminal_location(value: &Value) -> BrokerProtocolResult<SessionTerminalLocation> {
    let record = parse_exact_broker_record(
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
    )?;
    Ok(SessionTerminalLocation {
        source: parse_broker_string(&record["source"], BrokerStringOptions::default())?.into(),
        tty: parse_optional_broker_string(record.get("tty"))?,
        window_id: parse_optional_broker_string(record.get("windowId"))?,
        tab_id: parse_optional_broker_string(record.get("tabId"))?,
        pane_id: parse_optional_broker_string(record.get("paneId"))?,
        terminal_id: parse_optional_broker_string(record.get("terminalId"))?,
        session_id: parse_optional_broker_string(record.get("sessionId"))?,
    })
}

fn parse_session_terminal_metadata(value: &Value) -> BrokerProtocolResult<SessionTerminalMetadata> {
    let record = parse_exact_broker_record(value, &["locations"], &["program"])?;
    let locations = record["locations"]
        .as_array()
        .ok_or(crate::BrokerProtocolError {
            code: crate::BrokerProtocolFailureCode::InvalidField,
        })?;
    Ok(SessionTerminalMetadata {
        program: parse_optional_broker_string(record.get("program"))?,
        locations: locations
            .iter()
            .map(parse_session_terminal_location)
            .collect::<BrokerProtocolResult<Vec<_>>>()?,
    })
}

#[must_use]
pub fn parse_session_registration_envelope<Info>(
    value: &Value,
    parse_info: impl FnOnce(&Value) -> Option<Info>,
) -> Option<SessionRegistration<Info>> {
    let parse = || -> BrokerProtocolResult<SessionRegistration<Info>> {
        let record = parse_exact_broker_record(
            value,
            &[
                "registrationVersion",
                "sessionId",
                "pid",
                "cwd",
                "launchedAt",
                "info",
            ],
            &["repoRoot", "terminal"],
        )?;
        let registration_version =
            parse_broker_safe_integer(&record["registrationVersion"], 1, MAX_SAFE_INTEGER)?;
        if registration_version != SESSION_BROKER_REGISTRATION_VERSION {
            return Err(crate::BrokerProtocolError {
                code: crate::BrokerProtocolFailureCode::InvalidContract,
            });
        }
        Ok(SessionRegistration {
            registration_version,
            session_id: parse_broker_identifier(&record["sessionId"])?.into(),
            pid: parse_broker_safe_integer(&record["pid"], 1, MAX_SAFE_INTEGER)?,
            cwd: parse_broker_string(&record["cwd"], BrokerStringOptions::default())?.into(),
            repo_root: parse_optional_broker_string(record.get("repoRoot"))?,
            launched_at: parse_broker_string(
                &record["launchedAt"],
                BrokerStringOptions::default(),
            )?
            .into(),
            terminal: record
                .get("terminal")
                .map(parse_session_terminal_metadata)
                .transpose()?,
            info: parse_broker_app_payload(parse_info, &record["info"])?,
        })
    };
    parse().ok()
}

#[must_use]
pub fn parse_session_snapshot_envelope<State>(
    value: &Value,
    parse_state: impl FnOnce(&Value) -> Option<State>,
) -> Option<SessionSnapshot<State>> {
    let parse = || -> BrokerProtocolResult<SessionSnapshot<State>> {
        let record = parse_exact_broker_record(value, &["updatedAt", "state"], &[])?;
        Ok(SessionSnapshot {
            updated_at: parse_broker_string(&record["updatedAt"], BrokerStringOptions::default())?
                .into(),
            state: parse_broker_app_payload(parse_state, &record["state"])?,
        })
    };
    parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_registration() -> Value {
        json!({
            "registrationVersion": SESSION_BROKER_REGISTRATION_VERSION,
            "sessionId": "session-1",
            "pid": 123,
            "cwd": "/repo",
            "launchedAt": "2026-03-22T00:00:00.000Z",
            "info": { "ok": true },
        })
    }

    fn object_info(value: &Value) -> Option<Value> {
        value.is_object().then(|| value.clone())
    }

    #[test]
    fn registration_requires_the_current_websocket_version() {
        let mut registration = valid_registration();
        registration["registrationVersion"] = (SESSION_BROKER_REGISTRATION_VERSION - 1).into();
        assert!(parse_session_registration_envelope(&registration, object_info).is_none());
    }

    #[test]
    fn rejects_nonrecords_unknown_keys_malformed_optionals_and_panicking_parsers() {
        let valid = valid_registration();
        let mut extra = valid.clone();
        extra["extra"] = true.into();
        let mut null_root = valid.clone();
        null_root["repoRoot"] = Value::Null;
        let mut terminal_extra = valid.clone();
        terminal_extra["terminal"] = json!({ "locations": [], "extra": true });
        let mut huge_pid = valid.clone();
        huge_pid["pid"] = 9_007_199_254_740_992_u64.into();
        let mut bad_session = valid.clone();
        bad_session["sessionId"] = "bad id!".into();
        for value in [
            Value::Null,
            json!([]),
            extra,
            null_root,
            terminal_extra,
            huge_pid,
            bad_session,
        ] {
            assert!(parse_session_registration_envelope(&value, object_info).is_none());
        }
        assert!(
            parse_session_registration_envelope::<Value>(&valid, |_| {
                panic!("parser internals")
            })
            .is_none()
        );
        assert!(
            parse_session_snapshot_envelope(
                &json!({ "updatedAt": "now", "state": {}, "extra": true }),
                object_info,
            )
            .is_none()
        );
    }

    #[test]
    fn accepts_terminal_native_identifiers_as_bounded_strings() {
        let mut registration = valid_registration();
        registration["terminal"] = json!({
            "locations": [{ "source": "iterm2", "sessionId": "w1t2p3:ABCDEF" }],
        });
        let parsed = parse_session_registration_envelope(&registration, object_info).unwrap();
        assert_eq!(
            parsed.terminal.unwrap().locations,
            vec![SessionTerminalLocation {
                source: "iterm2".into(),
                session_id: Some("w1t2p3:ABCDEF".into()),
                ..SessionTerminalLocation::default()
            }]
        );
    }

    #[test]
    fn rejects_prototype_shaped_names_as_unknown_own_keys() {
        for key in ["constructor", "toString", "__proto__"] {
            let mut registration = valid_registration();
            registration[key] = "unexpected".into();
            assert!(parse_session_registration_envelope(&registration, object_info).is_none());
        }
        let mut registration = valid_registration();
        registration["terminal"] = json!({ "locations": [], "toString": "unexpected" });
        assert!(parse_session_registration_envelope(&registration, object_info).is_none());
    }

    #[test]
    fn snapshot_parsing_delegates_opaque_app_state_validation() {
        #[derive(Debug, PartialEq, Eq)]
        struct ReviewState {
            mode: String,
            selected: u64,
        }
        let snapshot = parse_session_snapshot_envelope(
            &json!({
                "updatedAt": "2026-03-22T00:00:00.000Z",
                "state": { "mode": "review", "selected": 2 },
            }),
            |value| {
                let record = value.as_object()?;
                Some(ReviewState {
                    mode: record.get("mode")?.as_str()?.into(),
                    selected: record.get("selected")?.as_u64()?,
                })
                .filter(|state| state.mode == "review")
            },
        )
        .unwrap();
        assert_eq!(snapshot.updated_at, "2026-03-22T00:00:00.000Z");
        assert_eq!(
            snapshot.state,
            ReviewState {
                mode: "review".into(),
                selected: 2,
            }
        );
    }
}
