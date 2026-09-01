//! Raw-session facade over the generic broker state machine.

use std::convert::Infallible;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;

use crate::{
    BrokerCommandOutcome, BrokerSessionCommentFilter, BrokerSessionReviewOptions,
    DispatchSessionCommand, HandleCommandResult, MarkSessionSeenResult, PendingCommandResult,
    RegisterSessionOptions, RegisterSessionResult, SelectableSession, SessionBrokerEntry,
    SessionBrokerLimitOptions, SessionBrokerLimits, SessionBrokerListedSession,
    SessionBrokerProtocolParsers, SessionBrokerState, SessionBrokerStateError,
    SessionBrokerViewAdapter, SessionRegistration, SessionSelector, SessionSnapshot,
    SharedDaemonSessionSocket, UpdateSnapshotResult,
};

type SessionDescription<Info, State> =
    Arc<dyn Fn(&SessionRegistration<Info>, &SessionSnapshot<State>) -> String + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBrokerRecord<Info, State> {
    pub session_id: String,
    pub cwd: String,
    pub repo_root: Option<String>,
    pub title: String,
    pub connected_at: String,
    pub last_seen_at: String,
    pub registration: SessionRegistration<Info>,
    pub snapshot: SessionSnapshot<State>,
}

impl<Info: Clone, State: Clone> SessionBrokerListedSession for SessionBrokerRecord<Info, State> {
    fn selectable_session(&self) -> SelectableSession {
        SelectableSession {
            session_id: self.session_id.clone(),
            cwd: PathBuf::from(&self.cwd),
            repo_root: self.repo_root.as_deref().map(PathBuf::from),
        }
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn snapshot_updated_at(&self) -> &str {
        &self.snapshot.updated_at
    }
}

pub struct SessionBrokerOptions<Info, State, CommandInput, CommandResult> {
    pub protocol_parsers:
        Arc<SessionBrokerProtocolParsers<Info, State, CommandInput, CommandResult>>,
    pub limit_options: SessionBrokerLimitOptions,
    pub describe_session: Option<SessionDescription<Info, State>>,
}

struct RawSessionAdapter<Info, State, CommandInput, CommandResult> {
    parsers: Arc<SessionBrokerProtocolParsers<Info, State, CommandInput, CommandResult>>,
    describe: SessionDescription<Info, State>,
}

impl<Info, State, CommandInput, CommandResult>
    RawSessionAdapter<Info, State, CommandInput, CommandResult>
where
    Info: Clone + Serialize,
    State: Clone,
{
    fn build_record(
        &self,
        entry: &SessionBrokerEntry<Info, State>,
    ) -> SessionBrokerRecord<Info, State> {
        SessionBrokerRecord {
            session_id: entry.registration.session_id.clone(),
            cwd: entry.registration.cwd.clone(),
            repo_root: entry.registration.repo_root.clone(),
            title: (self.describe)(&entry.registration, &entry.snapshot),
            connected_at: entry.connected_at.clone(),
            last_seen_at: entry.last_seen_at.clone(),
            registration: entry.registration.clone(),
            snapshot: entry.snapshot.clone(),
        }
    }
}

impl<Info, State, CommandInput, CommandResult> SessionBrokerViewAdapter
    for RawSessionAdapter<Info, State, CommandInput, CommandResult>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
{
    type Info = Info;
    type State = State;
    type CommandInput = CommandInput;
    type CommandResult = CommandResult;
    type ListedSession = SessionBrokerRecord<Info, State>;
    type SelectedContext = SessionBrokerRecord<Info, State>;
    type SessionReview = SessionBrokerRecord<Info, State>;
    type SessionCommentSummary = Infallible;

    fn parse_registration(&self, value: &Value) -> Option<SessionRegistration<Info>> {
        catch_unwind(AssertUnwindSafe(|| self.parsers.parse_registration(value)))
            .ok()
            .and_then(Result::ok)
    }

    fn parse_snapshot(&self, value: &Value) -> Option<SessionSnapshot<State>> {
        catch_unwind(AssertUnwindSafe(|| self.parsers.parse_snapshot(value)))
            .ok()
            .and_then(Result::ok)
    }

    fn parse_command_input(
        &self,
        command: &str,
        version: u64,
        value: &Value,
    ) -> Option<CommandInput> {
        self.parsers
            .parse_command_input(command, version, value)
            .ok()
    }

    fn parse_command_result(
        &self,
        command: &str,
        version: u64,
        value: &Value,
    ) -> Option<CommandResult> {
        self.parsers
            .parse_command_result(command, version, value)
            .ok()
    }

    fn build_listed_session(&self, entry: &SessionBrokerEntry<Info, State>) -> Self::ListedSession {
        self.build_record(entry)
    }

    fn build_selected_context(&self, session: &Self::ListedSession) -> Self::SelectedContext {
        session.clone()
    }

    fn build_session_review(
        &self,
        entry: &SessionBrokerEntry<Info, State>,
        _options: BrokerSessionReviewOptions,
    ) -> Self::SessionReview {
        self.build_record(entry)
    }

    fn list_comments(
        &self,
        _session: &Self::ListedSession,
        _filter: BrokerSessionCommentFilter,
    ) -> Vec<Self::SessionCommentSummary> {
        Vec::new()
    }
}

pub struct SessionBroker<Info, State, CommandInput, CommandResult>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
{
    pub protocol_parsers:
        Arc<SessionBrokerProtocolParsers<Info, State, CommandInput, CommandResult>>,
    state: SessionBrokerState<RawSessionAdapter<Info, State, CommandInput, CommandResult>>,
}

impl<Info, State, CommandInput, CommandResult>
    SessionBroker<Info, State, CommandInput, CommandResult>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
{
    pub fn new(
        options: SessionBrokerOptions<Info, State, CommandInput, CommandResult>,
    ) -> Result<Self, crate::BrokerLimitError> {
        let describe = options.describe_session.unwrap_or_else(|| {
            Arc::new(|registration, _snapshot| {
                serde_json::to_value(&registration.info)
                    .ok()
                    .and_then(|info| info.get("title")?.as_str().map(str::to_owned))
                    .filter(|title| !title.is_empty())
                    .unwrap_or_else(|| registration.session_id.clone())
            })
        });
        let parsers = Arc::clone(&options.protocol_parsers);
        let state = SessionBrokerState::new(
            RawSessionAdapter {
                parsers: Arc::clone(&parsers),
                describe,
            },
            &options.limit_options,
        )?;
        Ok(Self {
            protocol_parsers: parsers,
            state,
        })
    }

    #[must_use]
    pub fn limits(&self) -> SessionBrokerLimits {
        self.state.limits()
    }

    #[must_use]
    pub fn list_sessions(&self) -> Vec<SessionBrokerRecord<Info, State>> {
        self.state.list_sessions()
    }

    pub fn get_session(
        &self,
        selector: &SessionSelector,
    ) -> Result<SessionBrokerRecord<Info, State>, SessionBrokerStateError> {
        self.state.get_session(selector)
    }

    pub fn resolve_session_id(
        &self,
        selector: &SessionSelector,
    ) -> Result<String, SessionBrokerStateError> {
        Ok(self.get_session(selector)?.session_id)
    }

    #[must_use]
    pub fn session_ids(&self) -> Vec<String> {
        self.list_sessions()
            .into_iter()
            .map(|session| session.session_id)
            .collect()
    }

    #[must_use]
    pub fn session_count(&self) -> usize {
        self.state.session_count()
    }

    #[must_use]
    pub fn pending_command_count(&self) -> usize {
        self.state.pending_command_count()
    }

    pub fn register_session(
        &self,
        socket: SharedDaemonSessionSocket,
        registration: &Value,
        snapshot: &Value,
        options: RegisterSessionOptions,
    ) -> RegisterSessionResult {
        self.state
            .register_session(socket, registration, snapshot, options)
    }

    pub fn update_snapshot(
        &self,
        socket: &SharedDaemonSessionSocket,
        session_id: &str,
        snapshot: &Value,
    ) -> UpdateSnapshotResult {
        self.state.update_snapshot(socket, session_id, snapshot)
    }

    pub fn mark_session_seen(
        &self,
        socket: &SharedDaemonSessionSocket,
        session_id: &str,
    ) -> MarkSessionSeenResult {
        self.state.mark_session_seen(socket, session_id)
    }

    pub fn unregister_connection(&self, socket: &SharedDaemonSessionSocket) {
        self.state.unregister_socket(socket);
    }

    pub fn prune_stale_sessions(&self, ttl_ms: u64, now_ms: Option<i64>) -> usize {
        self.state.prune_stale_sessions(ttl_ms, now_ms)
    }

    pub fn dispatch_command(
        &self,
        request: DispatchSessionCommand,
    ) -> Result<PendingCommandResult<CommandResult>, SessionBrokerStateError> {
        self.state.dispatch_command(request)
    }

    pub fn handle_command_result(
        &self,
        socket: &SharedDaemonSessionSocket,
        request_id: &str,
        outcome: BrokerCommandOutcome,
    ) -> HandleCommandResult {
        self.state
            .handle_command_result(socket, request_id, outcome)
    }

    pub fn shutdown(&self, error: Option<SessionBrokerStateError>) {
        self.state.shutdown(error);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde::{Deserialize, Serialize};
    use serde_json::json;

    use super::*;
    use crate::{
        DaemonSessionSocket, SESSION_BROKER_REGISTRATION_VERSION, SessionBrokerAppParserRegistry,
        SessionBrokerCommandParsers, create_session_broker_protocol_parsers,
        parse_session_registration_envelope, parse_session_snapshot_envelope,
    };

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestInfo {
        title: String,
        files: Vec<String>,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestState {
        selected_index: u64,
        note_count: u64,
    }

    #[derive(Default)]
    struct TestSocket {
        sent: Mutex<Vec<String>>,
    }

    impl DaemonSessionSocket for TestSocket {
        fn send(&self, data: &str) -> Result<bool, SessionBrokerStateError> {
            self.sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(data.into());
            Ok(true)
        }
    }

    type TestBroker = SessionBroker<TestInfo, TestState, Value, Value>;

    fn parsers() -> Arc<SessionBrokerProtocolParsers<TestInfo, TestState, Value, Value>> {
        Arc::new(
            create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry {
                broker_revision: None,
                app_revision: 1,
                features: Vec::new(),
                parse_registration: Arc::new(|value| {
                    parse_session_registration_envelope(value, |info| {
                        serde_json::from_value(info.clone()).ok()
                    })
                }),
                parse_snapshot: Arc::new(|value| {
                    parse_session_snapshot_envelope(value, |state| {
                        serde_json::from_value(state.clone()).ok()
                    })
                }),
                commands: ["annotate", "reload_view"]
                    .into_iter()
                    .map(|command| SessionBrokerCommandParsers {
                        command: command.into(),
                        version: 1,
                        parse_input: Arc::new(|value| value.as_object().map(|_| value.clone())),
                        parse_result: Arc::new(|value| value.as_object().map(|_| value.clone())),
                    })
                    .collect(),
            })
            .unwrap(),
        )
    }

    fn broker_with_parsers(
        protocol_parsers: Arc<SessionBrokerProtocolParsers<TestInfo, TestState, Value, Value>>,
    ) -> TestBroker {
        SessionBroker::new(SessionBrokerOptions {
            protocol_parsers,
            limit_options: SessionBrokerLimitOptions::default(),
            describe_session: None,
        })
        .unwrap()
    }

    fn broker() -> TestBroker {
        broker_with_parsers(parsers())
    }

    fn registration() -> Value {
        json!({
            "registrationVersion": SESSION_BROKER_REGISTRATION_VERSION,
            "sessionId": "session-1",
            "pid": 123,
            "cwd": "/repo",
            "repoRoot": "/repo",
            "launchedAt": "2026-04-15T00:00:00.000Z",
            "info": {
                "title": "repo working tree",
                "files": ["src/example.ts"],
            },
        })
    }

    fn snapshot(selected_index: u64, note_count: u64) -> Value {
        json!({
            "updatedAt": "2026-04-15T00:00:00.000Z",
            "state": { "selectedIndex": selected_index, "noteCount": note_count },
        })
    }

    #[test]
    fn exposes_the_exact_parser_registry_owning_state_contracts() {
        let parsers = parsers();
        let broker = broker_with_parsers(Arc::clone(&parsers));
        assert!(Arc::ptr_eq(&broker.protocol_parsers, &parsers));
    }

    #[test]
    fn stores_raw_registrations_and_snapshots_without_projection_adapter() {
        let broker = broker();
        let socket: SharedDaemonSessionSocket = Arc::new(TestSocket::default());
        assert_eq!(
            broker.register_session(
                socket,
                &registration(),
                &snapshot(0, 0),
                RegisterSessionOptions::default(),
            ),
            RegisterSessionResult::Registered
        );
        let sessions = broker.list_sessions();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(session.session_id, "session-1");
        assert_eq!(session.cwd, "/repo");
        assert_eq!(session.repo_root.as_deref(), Some("/repo"));
        assert_eq!(session.title, "repo working tree");
        assert!(!session.connected_at.is_empty());
        assert!(!session.last_seen_at.is_empty());
        assert_eq!(session.registration.info.files, ["src/example.ts"]);
        assert_eq!(session.snapshot.state.selected_index, 0);
    }

    #[test]
    fn rejects_incompatible_registration_with_shared_envelope_parser() {
        let broker = broker();
        let socket: SharedDaemonSessionSocket = Arc::new(TestSocket::default());
        let mut invalid = registration();
        invalid["registrationVersion"] = json!(0);
        assert_eq!(
            broker.register_session(
                socket,
                &invalid,
                &snapshot(0, 0),
                RegisterSessionOptions::default(),
            ),
            RegisterSessionResult::Invalid
        );
        assert!(broker.list_sessions().is_empty());
    }

    #[test]
    fn dispatches_raw_command_and_resolves_result() {
        let broker = broker();
        let concrete = Arc::new(TestSocket::default());
        let socket: SharedDaemonSessionSocket = concrete.clone();
        assert_eq!(
            broker.register_session(
                Arc::clone(&socket),
                &registration(),
                &snapshot(0, 0),
                RegisterSessionOptions::default(),
            ),
            RegisterSessionResult::Registered
        );
        let pending = broker
            .dispatch_command(DispatchSessionCommand::new(
                SessionSelector {
                    session_id: Some("session-1".into()),
                    ..SessionSelector::default()
                },
                "annotate",
                json!({"filePath": "src/example.ts", "summary": "Review note"}),
                "Timed out waiting for annotate.",
            ))
            .unwrap();
        let outgoing: Value = serde_json::from_str(
            &concrete
                .sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())[0],
        )
        .unwrap();
        assert_eq!(outgoing["command"], "annotate");
        assert_eq!(
            broker.handle_command_result(
                &socket,
                outgoing["requestId"].as_str().unwrap(),
                BrokerCommandOutcome::Success(json!({"ok": true})),
            ),
            HandleCommandResult::Handled
        );
        assert_eq!(pending.receive().unwrap(), json!({"ok": true}));
    }

    #[test]
    fn delegates_snapshot_liveness_selection_unregistration_and_shutdown() {
        let broker = broker();
        let socket: SharedDaemonSessionSocket = Arc::new(TestSocket::default());
        assert_eq!(
            broker.register_session(
                Arc::clone(&socket),
                &registration(),
                &snapshot(0, 0),
                RegisterSessionOptions::default(),
            ),
            RegisterSessionResult::Registered
        );
        assert_eq!(broker.session_ids(), ["session-1"]);
        assert_eq!(broker.session_count(), 1);
        let selector = SessionSelector {
            session_id: Some("session-1".into()),
            ..SessionSelector::default()
        };
        assert_eq!(broker.resolve_session_id(&selector).unwrap(), "session-1");
        assert_eq!(
            broker.update_snapshot(&socket, "session-1", &snapshot(2, 3)),
            UpdateSnapshotResult::Updated
        );
        assert_eq!(
            broker
                .get_session(&selector)
                .unwrap()
                .snapshot
                .state
                .note_count,
            3
        );
        assert_eq!(
            broker.mark_session_seen(&socket, "session-1"),
            MarkSessionSeenResult::Seen
        );
        broker.unregister_connection(&socket);
        assert_eq!(broker.session_count(), 0);

        let next_socket: SharedDaemonSessionSocket = Arc::new(TestSocket::default());
        assert_eq!(
            broker.register_session(
                Arc::clone(&next_socket),
                &registration(),
                &snapshot(0, 0),
                RegisterSessionOptions::default(),
            ),
            RegisterSessionResult::Registered
        );
        broker.shutdown(None);
        assert_eq!(broker.session_count(), 0);
        assert_eq!(
            broker.register_session(
                next_socket,
                &registration(),
                &snapshot(0, 0),
                RegisterSessionOptions::default(),
            ),
            RegisterSessionResult::Shutdown
        );
    }
}
