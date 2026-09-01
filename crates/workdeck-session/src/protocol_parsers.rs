//! Authoritative strict parsers for broker WebSocket, bridge, and HTTP envelopes.

use crate::{
    BrokerProtocolError, BrokerProtocolFailureCode, BrokerStringOptions, SessionRegistration,
    SessionSelector, SessionServerMessage, SessionSnapshot, fail_broker_protocol,
    parse_broker_app_payload, parse_broker_identifier, parse_broker_revision,
    parse_broker_selector, parse_broker_string, parse_broker_timeout, parse_exact_broker_record,
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

type RuntimeParser<T> = Arc<dyn Fn(&Value) -> Option<T> + Send + Sync>;

#[derive(Clone)]
pub struct SessionBrokerCommandParsers<Input, ResultValue> {
    pub command: String,
    pub version: u64,
    pub parse_input: RuntimeParser<Input>,
    pub parse_result: RuntimeParser<ResultValue>,
}

pub struct SessionBrokerAppParserRegistry<Info, State, Input, ResultValue> {
    pub broker_revision: Option<u32>,
    pub app_revision: u64,
    pub features: Vec<String>,
    pub parse_registration: RuntimeParser<SessionRegistration<Info>>,
    pub parse_snapshot: RuntimeParser<SessionSnapshot<State>>,
    pub commands: Vec<SessionBrokerCommandParsers<Input, ResultValue>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParserRegistryError;

impl fmt::Display for ParserRegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Invalid session broker parser registry.")
    }
}

impl std::error::Error for ParserRegistryError {}

#[derive(Debug, Clone, PartialEq)]
pub enum StructuralSessionClientMessage<ResultValue = Value> {
    Register {
        registration: Value,
        snapshot: Value,
    },
    Snapshot {
        session_id: String,
        snapshot: Value,
    },
    Heartbeat {
        session_id: String,
    },
    CommandResult {
        request_id: String,
        outcome: StructuralCommandOutcome<ResultValue>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructuralCommandOutcome<ResultValue> {
    Success { result: ResultValue },
    Failure { error: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum StructuralSessionBrokerDaemonRequest<CommandName = String> {
    List,
    Get {
        selector: SessionSelector,
    },
    Dispatch {
        selector: SessionSelector,
        command: CommandName,
        command_version: u64,
        input: Value,
        timeout_ms: Option<u64>,
        timeout_message: Option<String>,
    },
}

pub fn parse_session_broker_json_bytes(body: &[u8]) -> Result<Value, BrokerProtocolError> {
    if body.starts_with(&[0xef, 0xbb, 0xbf]) {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidJson);
    }
    serde_json::from_slice(body).map_err(|_| BrokerProtocolError {
        code: BrokerProtocolFailureCode::InvalidJson,
    })
}

pub fn parse_session_broker_json_text(message: &Value) -> Result<Value, BrokerProtocolError> {
    let Some(message) = message.as_str() else {
        return fail_broker_protocol(BrokerProtocolFailureCode::InvalidJson);
    };
    serde_json::from_str(message).map_err(|_| BrokerProtocolError {
        code: BrokerProtocolFailureCode::InvalidJson,
    })
}

pub struct SessionBrokerProtocolParsers<Info, State, Input, ResultValue> {
    pub broker_revision: u32,
    pub app_revision: u64,
    pub features: Vec<String>,
    parse_registration: RuntimeParser<SessionRegistration<Info>>,
    parse_snapshot: RuntimeParser<SessionSnapshot<State>>,
    commands: BTreeMap<(String, u64), SessionBrokerCommandParsers<Input, ResultValue>>,
}

impl<Info, State, Input, ResultValue>
    SessionBrokerProtocolParsers<Info, State, Input, ResultValue>
{
    pub fn new(
        registry: SessionBrokerAppParserRegistry<Info, State, Input, ResultValue>,
    ) -> Result<Self, ParserRegistryError> {
        if registry
            .broker_revision
            .is_some_and(|revision| revision != 1)
            || !crate::is_valid_broker_revision(registry.app_revision)
            || !registry.features.is_empty()
        {
            return Err(ParserRegistryError);
        }
        let mut commands = BTreeMap::new();
        for descriptor in registry.commands {
            if !crate::is_valid_broker_identifier(&descriptor.command)
                || !crate::is_valid_broker_revision(descriptor.version)
                || commands
                    .insert((descriptor.command.clone(), descriptor.version), descriptor)
                    .is_some()
            {
                return Err(ParserRegistryError);
            }
        }
        Ok(Self {
            broker_revision: 1,
            app_revision: registry.app_revision,
            features: Vec::new(),
            parse_registration: registry.parse_registration,
            parse_snapshot: registry.parse_snapshot,
            commands,
        })
    }

    pub fn parse_registration(
        &self,
        value: &Value,
    ) -> Result<SessionRegistration<Info>, BrokerProtocolError> {
        parse_broker_app_payload(|value| (self.parse_registration)(value), value)
    }

    pub fn parse_snapshot(
        &self,
        value: &Value,
    ) -> Result<SessionSnapshot<State>, BrokerProtocolError> {
        parse_broker_app_payload(|value| (self.parse_snapshot)(value), value)
    }

    pub fn parse_command_input(
        &self,
        command: &str,
        version: u64,
        value: &Value,
    ) -> Result<Input, BrokerProtocolError> {
        let descriptor = self.lookup_command(command, version)?;
        parse_broker_app_payload(|value| (descriptor.parse_input)(value), value)
    }

    pub fn parse_command_result(
        &self,
        command: &str,
        version: u64,
        value: &Value,
    ) -> Result<ResultValue, BrokerProtocolError> {
        let descriptor = self.lookup_command(command, version)?;
        parse_broker_app_payload(|value| (descriptor.parse_result)(value), value)
    }

    pub fn parse_client_message(
        &self,
        value: &Value,
    ) -> Result<StructuralSessionClientMessage<Value>, BrokerProtocolError> {
        let base = parse_exact_broker_record(
            value,
            &["type"],
            &[
                "registration",
                "snapshot",
                "sessionId",
                "requestId",
                "ok",
                "result",
                "error",
            ],
        )?;
        let Some(discriminant) = base["type"].as_str() else {
            return fail_broker_protocol(BrokerProtocolFailureCode::InvalidDiscriminant);
        };
        match discriminant {
            "register" => {
                let record =
                    parse_exact_broker_record(value, &["type", "registration", "snapshot"], &[])?;
                Ok(StructuralSessionClientMessage::Register {
                    registration: record["registration"].clone(),
                    snapshot: record["snapshot"].clone(),
                })
            }
            "snapshot" => {
                let record =
                    parse_exact_broker_record(value, &["type", "sessionId", "snapshot"], &[])?;
                Ok(StructuralSessionClientMessage::Snapshot {
                    session_id: parse_broker_identifier(&record["sessionId"])?.into(),
                    snapshot: record["snapshot"].clone(),
                })
            }
            "heartbeat" => {
                let record = parse_exact_broker_record(value, &["type", "sessionId"], &[])?;
                Ok(StructuralSessionClientMessage::Heartbeat {
                    session_id: parse_broker_identifier(&record["sessionId"])?.into(),
                })
            }
            "command-result" => {
                let common = parse_exact_broker_record(
                    value,
                    &["type", "requestId", "ok"],
                    &["result", "error"],
                )?;
                let request_id = parse_broker_identifier(&common["requestId"])?.to_owned();
                let outcome = match common["ok"].as_bool() {
                    Some(true) => {
                        let record = parse_exact_broker_record(
                            value,
                            &["type", "requestId", "ok", "result"],
                            &[],
                        )?;
                        StructuralCommandOutcome::Success {
                            result: record["result"].clone(),
                        }
                    }
                    Some(false) => {
                        let record = parse_exact_broker_record(
                            value,
                            &["type", "requestId", "ok", "error"],
                            &[],
                        )?;
                        StructuralCommandOutcome::Failure {
                            error: parse_broker_string(
                                &record["error"],
                                BrokerStringOptions {
                                    min_bytes: 1,
                                    max_bytes: 1_024,
                                },
                            )?
                            .into(),
                        }
                    }
                    None => return fail_broker_protocol(BrokerProtocolFailureCode::InvalidField),
                };
                Ok(StructuralSessionClientMessage::CommandResult {
                    request_id,
                    outcome,
                })
            }
            _ => fail_broker_protocol(BrokerProtocolFailureCode::InvalidDiscriminant),
        }
    }

    pub fn parse_server_message(
        &self,
        value: &Value,
    ) -> Result<SessionServerMessage<String, Input>, BrokerProtocolError> {
        let record = parse_exact_broker_record(
            value,
            &["type", "requestId", "command", "input"],
            &["commandVersion"],
        )?;
        if record["type"] != "command" {
            return fail_broker_protocol(BrokerProtocolFailureCode::InvalidDiscriminant);
        }
        let request_id = parse_broker_identifier(&record["requestId"])?.to_owned();
        let command = parse_broker_identifier(&record["command"])?.to_owned();
        let command_version = record
            .get("commandVersion")
            .map(parse_broker_revision)
            .transpose()?
            .unwrap_or(1);
        let input = self.parse_command_input(&command, command_version, &record["input"])?;
        Ok(SessionServerMessage {
            request_id,
            command,
            command_version: Some(command_version),
            input,
        })
    }

    pub fn parse_daemon_request(
        &self,
        value: &Value,
    ) -> Result<StructuralSessionBrokerDaemonRequest, BrokerProtocolError> {
        let base = parse_exact_broker_record(
            value,
            &["action"],
            &[
                "selector",
                "command",
                "commandVersion",
                "input",
                "timeoutMs",
                "timeoutMessage",
            ],
        )?;
        match base["action"].as_str() {
            Some("list") => {
                parse_exact_broker_record(value, &["action"], &[])?;
                Ok(StructuralSessionBrokerDaemonRequest::List)
            }
            Some("get") => {
                let record = parse_exact_broker_record(value, &["action", "selector"], &[])?;
                Ok(StructuralSessionBrokerDaemonRequest::Get {
                    selector: parse_broker_selector(&record["selector"])?,
                })
            }
            Some("dispatch") => {
                let record = parse_exact_broker_record(
                    value,
                    &["action", "selector", "command", "input"],
                    &["commandVersion", "timeoutMs", "timeoutMessage"],
                )?;
                let command = parse_broker_identifier(&record["command"])?.to_owned();
                let command_version = record
                    .get("commandVersion")
                    .map(parse_broker_revision)
                    .transpose()?
                    .unwrap_or(1);
                let timeout_ms = record
                    .get("timeoutMs")
                    .map(parse_broker_timeout)
                    .transpose()?;
                let timeout_message = record
                    .get("timeoutMessage")
                    .map(|value| {
                        parse_broker_string(
                            value,
                            BrokerStringOptions {
                                min_bytes: 1,
                                max_bytes: 1_024,
                            },
                        )
                        .map(str::to_owned)
                    })
                    .transpose()?;
                Ok(StructuralSessionBrokerDaemonRequest::Dispatch {
                    selector: parse_broker_selector(&record["selector"])?,
                    command,
                    command_version,
                    input: record["input"].clone(),
                    timeout_ms,
                    timeout_message,
                })
            }
            _ => fail_broker_protocol(BrokerProtocolFailureCode::InvalidDiscriminant),
        }
    }

    fn lookup_command(
        &self,
        command: &str,
        version: u64,
    ) -> Result<&SessionBrokerCommandParsers<Input, ResultValue>, BrokerProtocolError> {
        self.commands
            .get(&(command.to_owned(), version))
            .ok_or(BrokerProtocolError {
                code: BrokerProtocolFailureCode::UnknownCommand,
            })
    }
}

pub fn create_session_broker_protocol_parsers<Info, State, Input, ResultValue>(
    registry: SessionBrokerAppParserRegistry<Info, State, Input, ResultValue>,
) -> Result<SessionBrokerProtocolParsers<Info, State, Input, ResultValue>, ParserRegistryError> {
    SessionBrokerProtocolParsers::new(registry)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SESSION_BROKER_REGISTRATION_VERSION;
    use serde_json::json;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestInfo {
        title: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestState {
        selected: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestInput {
        summary: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct TestResult {
        applied: bool,
    }

    fn exact_object(value: &Value, keys: &[&str]) -> bool {
        value.as_object().is_some_and(|record| {
            record.len() == keys.len() && keys.iter().all(|key| record.contains_key(*key))
        })
    }

    fn parsers() -> SessionBrokerProtocolParsers<TestInfo, TestState, TestInput, TestResult> {
        create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry {
            broker_revision: None,
            app_revision: 7,
            features: vec![],
            parse_registration: Arc::new(|value| {
                if !exact_object(
                    value,
                    &[
                        "registrationVersion",
                        "sessionId",
                        "pid",
                        "cwd",
                        "launchedAt",
                        "info",
                    ],
                ) {
                    return None;
                }
                Some(SessionRegistration {
                    registration_version: value["registrationVersion"].as_u64()?,
                    session_id: value["sessionId"].as_str()?.into(),
                    pid: value["pid"].as_u64()?,
                    cwd: value["cwd"].as_str()?.into(),
                    repo_root: None,
                    launched_at: value["launchedAt"].as_str()?.into(),
                    terminal: None,
                    info: TestInfo {
                        title: value["info"]["title"].as_str()?.into(),
                    },
                })
            }),
            parse_snapshot: Arc::new(|value| {
                if !exact_object(value, &["updatedAt", "state"]) {
                    return None;
                }
                Some(SessionSnapshot {
                    updated_at: value["updatedAt"].as_str()?.into(),
                    state: TestState {
                        selected: value["state"]["selected"].as_u64()?,
                    },
                })
            }),
            commands: vec![SessionBrokerCommandParsers {
                command: "annotate".into(),
                version: 2,
                parse_input: Arc::new(|value| {
                    if !exact_object(value, &["summary"]) {
                        return None;
                    }
                    Some(TestInput {
                        summary: value["summary"].as_str()?.into(),
                    })
                }),
                parse_result: Arc::new(|value| {
                    (exact_object(value, &["applied"]) && value["applied"] == true)
                        .then_some(TestResult { applied: true })
                }),
            }],
        })
        .unwrap()
    }

    #[test]
    fn type_locks_complete_outputs_to_public_rust_unions() {
        let parsers = parsers();
        assert_eq!(
            parsers
                .parse_client_message(&json!({ "type": "heartbeat", "sessionId": "session-1" }))
                .unwrap(),
            StructuralSessionClientMessage::Heartbeat {
                session_id: "session-1".into()
            }
        );
        let server = parsers
            .parse_server_message(&json!({
                "type": "command",
                "requestId": "request-1",
                "command": "annotate",
                "commandVersion": 2,
                "input": { "summary": "note" },
            }))
            .unwrap();
        assert_eq!(server.command, "annotate");
        assert!(matches!(
            parsers
                .parse_daemon_request(&json!({ "action": "list" }))
                .unwrap(),
            StructuralSessionBrokerDaemonRequest::List
        ));
    }

    #[test]
    fn runs_deterministic_malformed_corpus_through_all_envelopes() {
        let parsers = parsers();
        let malformed = vec![
            Value::Null,
            json!(false),
            json!(1.5),
            json!("message"),
            json!([]),
            json!({}),
            json!({ "extra": 1 }),
            json!({ "type": "command", "requestId": "request-1", "command": "annotate", "input": null }),
            json!({ "type": "command", "requestId": "request-1", "command": "annotate", "commandVersion": 0, "input": { "summary": "note" } }),
            json!({ "type": "command", "requestId": "request-1", "command": "annotate", "commandVersion": 2, "input": { "summary": "note", "extra": true } }),
            json!({ "type": "command", "requestId": "bad id!", "command": "annotate", "commandVersion": 2, "input": { "summary": "note" } }),
            json!({ "type": "register", "registration": null, "snapshot": {}, "extra": true }),
            json!({ "action": "get", "selector": { "sessionId": "session-1", "nested": true } }),
            json!({ "action": "list", "selector": {} }),
        ];
        for value in malformed {
            assert!(parsers.parse_server_message(&value).is_err());
            assert!(parsers.parse_client_message(&value).is_err());
            assert!(parsers.parse_daemon_request(&value).is_err());
        }
    }

    #[test]
    fn rejects_unsupported_controls_and_enforces_result_contracts() {
        let parsers = parsers();
        for control in [
            json!({ "deadline": 1 }),
            json!({ "idempotencyKey": "request-key-1" }),
        ] {
            let mut dispatch = json!({
                "action": "dispatch",
                "selector": { "sessionId": "session-1" },
                "command": "annotate",
                "commandVersion": 2,
                "input": { "summary": "note" },
            });
            dispatch
                .as_object_mut()
                .unwrap()
                .extend(control.as_object().unwrap().clone());
            assert_eq!(
                parsers.parse_daemon_request(&dispatch).unwrap_err().code,
                BrokerProtocolFailureCode::InvalidKeys
            );
        }
        assert_eq!(
            parsers
                .parse_command_result("annotate", 2, &json!({ "applied": false }))
                .unwrap_err()
                .code,
            BrokerProtocolFailureCode::InvalidAppPayload
        );
        assert_eq!(
            parsers
                .parse_command_result("annotate", 1, &json!({ "applied": true }))
                .unwrap_err()
                .code,
            BrokerProtocolFailureCode::UnknownCommand
        );
    }

    #[test]
    fn rejects_dangerous_own_keys_and_leaves_app_payloads_opaque() {
        let parsers = parsers();
        for key in ["__proto__", "constructor", "toString"] {
            let mut heartbeat = json!({ "type": "heartbeat", "sessionId": "session-1" });
            heartbeat[key] = true.into();
            assert_eq!(
                parsers.parse_client_message(&heartbeat).unwrap_err().code,
                BrokerProtocolFailureCode::InvalidKeys
            );
        }
        let registration = json!({ "malformedForApp": true });
        let snapshot = json!({ "transformedLater": true });
        assert_eq!(
            parsers
                .parse_client_message(&json!({
                    "type": "register",
                    "registration": registration.clone(),
                    "snapshot": snapshot.clone(),
                }))
                .unwrap(),
            StructuralSessionClientMessage::Register {
                registration,
                snapshot
            }
        );
    }

    #[test]
    fn normalizes_parser_panics_without_exposing_messages() {
        let throwing = create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry::<
            Value,
            Value,
            Value,
            Value,
        > {
            broker_revision: None,
            app_revision: 1,
            features: vec![],
            parse_registration: Arc::new(|_| panic!("registration secret")),
            parse_snapshot: Arc::new(|_| None),
            commands: vec![],
        })
        .unwrap();
        let error = throwing.parse_registration(&json!({})).unwrap_err();
        assert_eq!(error.code, BrokerProtocolFailureCode::AppParserFailed);
        assert!(!error.to_string().contains("registration secret"));
    }

    #[test]
    fn validates_registry_revision_features_and_duplicate_commands() {
        let descriptor = || SessionBrokerCommandParsers {
            command: "annotate".into(),
            version: 1,
            parse_input: Arc::new(|value: &Value| Some(value.clone())),
            parse_result: Arc::new(|value: &Value| Some(value.clone())),
        };
        let registry = |commands| SessionBrokerAppParserRegistry {
            broker_revision: Some(1),
            app_revision: 1,
            features: vec![],
            parse_registration: Arc::new(|_| None::<SessionRegistration<Value>>),
            parse_snapshot: Arc::new(|_| None::<SessionSnapshot<Value>>),
            commands,
        };
        assert!(
            create_session_broker_protocol_parsers(registry(vec![descriptor(), descriptor()]))
                .is_err()
        );
        let mut invalid = registry(vec![]);
        invalid.broker_revision = Some(2);
        assert!(create_session_broker_protocol_parsers(invalid).is_err());
    }

    #[test]
    fn strictly_parses_json_bytes_text_bom_and_utf8() {
        assert_eq!(
            parse_session_broker_json_bytes(br#"{"ok":true}"#).unwrap(),
            json!({ "ok": true })
        );
        assert_eq!(
            parse_session_broker_json_text(&json!(r#"{"ok":true}"#)).unwrap(),
            json!({ "ok": true })
        );
        assert_eq!(
            parse_session_broker_json_bytes(&[0xef, 0xbb, 0xbf, b'{', b'}'])
                .unwrap_err()
                .code,
            BrokerProtocolFailureCode::InvalidJson
        );
        assert!(parse_session_broker_json_bytes(&[0xc0, 0xaf]).is_err());
        assert!(parse_session_broker_json_text(&json!({})).is_err());
    }

    #[test]
    fn parses_registered_app_envelopes() {
        let parsers = parsers();
        let registration = parsers
            .parse_registration(&json!({
                "registrationVersion": SESSION_BROKER_REGISTRATION_VERSION,
                "sessionId": "session-1",
                "pid": 123,
                "cwd": "/repo",
                "launchedAt": "now",
                "info": { "title": "Review" },
            }))
            .unwrap();
        assert_eq!(registration.info.title, "Review");
        let snapshot = parsers
            .parse_snapshot(&json!({ "updatedAt": "now", "state": { "selected": 2 } }))
            .unwrap();
        assert_eq!(snapshot.state.selected, 2);
    }
}
