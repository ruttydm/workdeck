use super::*;

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;

use crate::{
    BrokerGrant, NativeSessionBrokerAdapterSemantics, ProducerGrant,
    SESSION_BROKER_REGISTRATION_VERSION, ServeSessionBrokerDaemonOptions, SessionBroker,
    SessionBrokerAuthenticator, SessionBrokerAuthenticatorOptions,
    SessionBrokerAuthorityCredential, SessionBrokerDaemon, SessionBrokerDaemonIdentity,
    SessionBrokerDaemonOptions, SessionBrokerLimitOptions, SessionBrokerOptions,
    SessionBrokerSocketCloseEvent, WorkdeckSessionInfo, WorkdeckSessionInputKind,
    WorkdeckSessionState, create_workdeck_session_protocol_parsers, serve_session_broker_daemon,
};

fn registration(id: &str) -> WorkdeckSessionRegistration {
    WorkdeckSessionRegistration {
        registration_version: SESSION_BROKER_REGISTRATION_VERSION,
        session_id: id.into(),
        pid: u64::from(std::process::id()),
        cwd: "/repo".into(),
        repo_root: Some("/repo".into()),
        launched_at: "2026-01-01T00:00:00.000Z".into(),
        terminal: None,
        info: WorkdeckSessionInfo {
            input_kind: WorkdeckSessionInputKind::Diff,
            title: "before.rs ↔ after.rs".into(),
            source_label: "before.rs -> after.rs".into(),
            experimental_features: Some(Vec::new()),
            files: Vec::new(),
            review_catalog: None,
            review_capability_digest: None,
        },
    }
}

fn snapshot(index: u64) -> WorkdeckSessionSnapshot {
    WorkdeckSessionSnapshot {
        updated_at: format!("2026-01-01T00:00:00.{index:03}Z"),
        state: WorkdeckSessionState {
            selected_file_id: None,
            selected_file_path: Some("after.rs".into()),
            selected_hunk_index: index,
            selected_hunk_old_range: None,
            selected_hunk_new_range: None,
            show_agent_notes: true,
            note_markup_width: None,
            live_comment_count: 0,
            live_comments: Vec::new(),
            review_note_count: Some(0),
            review_notes: Some(Vec::new()),
            review_publication: None,
        },
    }
}

fn config() -> ResolvedSessionBrokerConfig {
    ResolvedSessionBrokerConfig {
        host: "127.0.0.1".into(),
        port: 47_657,
        http_origin: "http://127.0.0.1:47657".into(),
        ws_origin: "ws://127.0.0.1:47657".into(),
    }
}

fn wait_attempt(attempt: &SessionBrokerStartup) {
    assert!(
        attempt.wait_timeout(Duration::from_secs(2)),
        "startup attempt {} did not settle",
        attempt.id()
    );
}

fn wait_until(predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        thread::sleep(Duration::from_millis(2));
    }
    panic!("client fixture timed out");
}

#[derive(Default)]
struct GateState {
    result: Option<Result<(), String>>,
}

#[derive(Default)]
struct Gate {
    state: (Mutex<GateState>, Condvar),
}

impl Gate {
    fn wait(&self) -> Result<(), String> {
        let state = self
            .state
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.state
            .1
            .wait_while(state, |state| state.result.is_none())
            .unwrap_or_else(|error| error.into_inner())
            .result
            .clone()
            .unwrap()
    }

    fn resolve(&self) {
        self.set(Ok(()));
    }

    fn reject(&self, message: &str) {
        self.set(Err(message.into()));
    }

    fn set(&self, result: Result<(), String>) {
        self.state
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .result = Some(result);
        self.state.1.notify_all();
    }
}

struct ManualTask {
    due_ms: u64,
    callback: Option<Box<dyn FnOnce() + Send>>,
}

#[derive(Default)]
struct ManualSchedulerState {
    now_ms: u64,
    next_id: u64,
    tasks: BTreeMap<u64, ManualTask>,
}

#[derive(Default)]
struct ManualScheduler {
    state: Arc<Mutex<ManualSchedulerState>>,
}

struct ManualHandle {
    state: Weak<Mutex<ManualSchedulerState>>,
    id: u64,
}

impl SessionBrokerRetryHandle for ManualHandle {
    fn cancel(&self) {
        if let Some(state) = self.state.upgrade() {
            state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .tasks
                .remove(&self.id);
        }
    }
}

impl SessionBrokerRetryScheduler for ManualScheduler {
    fn schedule(
        &self,
        delay: Duration,
        callback: Box<dyn FnOnce() + Send>,
    ) -> Arc<dyn SessionBrokerRetryHandle> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.next_id += 1;
        let id = state.next_id;
        let due_ms = state
            .now_ms
            .saturating_add(delay.as_millis().try_into().unwrap_or(u64::MAX));
        state.tasks.insert(
            id,
            ManualTask {
                due_ms,
                callback: Some(callback),
            },
        );
        Arc::new(ManualHandle {
            state: Arc::downgrade(&self.state),
            id,
        })
    }
}

impl ManualScheduler {
    fn advance(&self, duration: Duration) {
        let target = {
            let state = self.state.lock().unwrap_or_else(|error| error.into_inner());
            state
                .now_ms
                .saturating_add(duration.as_millis().try_into().unwrap_or(u64::MAX))
        };
        loop {
            let callback = {
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                let next = state
                    .tasks
                    .iter()
                    .filter(|(_, task)| task.due_ms <= target)
                    .min_by_key(|(id, task)| (task.due_ms, **id))
                    .map(|(id, _)| *id);
                let Some(id) = next else {
                    state.now_ms = target;
                    return;
                };
                let mut task = state.tasks.remove(&id).unwrap();
                state.now_ms = task.due_ms;
                task.callback.take()
            };
            if let Some(callback) = callback {
                callback();
            }
        }
    }

    fn pending(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .tasks
            .len()
    }
}

struct NoopRetryHandle;

impl SessionBrokerRetryHandle for NoopRetryHandle {
    fn cancel(&self) {}
}

struct EagerScheduler;

impl SessionBrokerRetryScheduler for EagerScheduler {
    fn schedule(
        &self,
        _delay: Duration,
        callback: Box<dyn FnOnce() + Send>,
    ) -> Arc<dyn SessionBrokerRetryHandle> {
        callback();
        Arc::new(NoopRetryHandle)
    }
}

type StartHook = Arc<dyn Fn() -> Result<(), SessionBrokerClientError> + Send + Sync>;

#[derive(Default)]
struct TestConnection {
    starts: AtomicUsize,
    stops: AtomicUsize,
    snapshots: Mutex<Vec<WorkdeckSessionSnapshot>>,
    replacements: Mutex<Vec<WorkdeckSessionRegistration>>,
    start_hook: Mutex<Option<StartHook>>,
    start_error: Mutex<Option<String>>,
    replace_error: Mutex<Option<String>>,
    panic_on_stop: AtomicBool,
}

impl WorkdeckSessionClientConnection for TestConnection {
    fn start(&self) -> Result<(), SessionBrokerClientError> {
        self.starts.fetch_add(1, Ordering::SeqCst);
        if let Some(hook) = self
            .start_hook
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
        {
            hook()?;
        }
        if let Some(error) = self
            .start_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
        {
            return Err(SessionBrokerClientError::Runtime(error));
        }
        Ok(())
    }

    fn stop(&self) {
        self.stops.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.panic_on_stop.load(Ordering::SeqCst),
            "cleanup exploded"
        );
    }

    fn set_bridge(&self, _bridge: Option<Arc<WorkdeckSessionAppBridge>>) {}

    fn replace_session(
        &self,
        registration: WorkdeckSessionRegistration,
        _snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError> {
        if let Some(error) = self
            .replace_error
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
        {
            return Err(SessionBrokerClientError::Runtime(error));
        }
        self.replacements
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(registration);
        Ok(())
    }

    fn update_snapshot(
        &self,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError> {
        self.snapshots
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(snapshot);
        Ok(())
    }
}

fn connection_factory(
    queue: Arc<Mutex<VecDeque<Arc<TestConnection>>>>,
    specs: Arc<Mutex<Vec<SessionBrokerClientConnectionSpec>>>,
) -> ClientConnectionFactory {
    Arc::new(move |spec| {
        specs
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(spec);
        let connection = queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .pop_front()
            .unwrap_or_else(|| Arc::new(TestConnection::default()));
        Ok(connection)
    })
}

fn runtime(
    scheduler: Arc<dyn SessionBrokerRetryScheduler>,
    warnings: Arc<Mutex<Vec<String>>>,
) -> SessionBrokerClientRuntime {
    let root = Arc::new(tempfile::tempdir().unwrap());
    let credentials = Arc::new(Mutex::new(None));
    SessionBrokerClientRuntime {
        disabled: Arc::new(|| false),
        resolve_config: Arc::new(|| Ok(config())),
        ensure_daemon: Arc::new(|_, _| Ok(())),
        load_credentials: Arc::new(move || {
            let mut cache = credentials
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(credentials) = cache.as_ref() {
                return Ok(Arc::clone(credentials));
            }
            let env = BTreeMap::from([(
                "XDG_RUNTIME_DIR".into(),
                root.path().to_string_lossy().into_owned(),
            )]);
            let loaded = Arc::new(
                load_or_create_workdeck_session_broker_credentials(&env, Some(100)).unwrap(),
            );
            *cache = Some(Arc::clone(&loaded));
            Ok(loaded)
        }),
        is_healthy: Arc::new(|_| false),
        read_launch_fingerprint: Arc::new(|_| None),
        create_connection: Arc::new(|_| {
            Err(SessionBrokerClientError::Runtime(
                "unexpected connection".into(),
            ))
        }),
        retry_scheduler: scheduler,
        warning_sink: Arc::new(move |message| {
            warnings
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(message.into());
        }),
        startup_override: None,
    }
}

fn client_with_override(
    scheduler: Arc<dyn SessionBrokerRetryScheduler>,
    warnings: Arc<Mutex<Vec<String>>>,
    start: SessionBrokerStartupOverride,
    reconnect_delay: Duration,
) -> WorkdeckSessionBrokerClient {
    let mut runtime = runtime(scheduler, warnings);
    runtime.startup_override = Some(start);
    WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming {
            reconnect_delay,
            ..SessionBrokerClientTiming::default()
        },
        runtime,
    )
}

#[test]
fn only_exact_pre_authentication_compatibility_closes_are_quiescent_refusals() {
    let reason = "Session broker authentication required; upgrade Workdeck.";
    assert!(is_quiescent_upgrade_refusal(
        &SessionBrokerSocketCloseEvent {
            code: 1008,
            reason: reason.into(),
            authenticated: Some(false),
        }
    ));
    assert!(is_quiescent_upgrade_refusal(
        &SessionBrokerSocketCloseEvent {
            code: 1008,
            reason: "Malformed session broker protocol.".into(),
            authenticated: Some(false),
        }
    ));
    for event in [
        SessionBrokerSocketCloseEvent {
            code: 1008,
            reason: reason.into(),
            authenticated: Some(true),
        },
        SessionBrokerSocketCloseEvent {
            code: 1006,
            reason: reason.into(),
            authenticated: Some(false),
        },
        SessionBrokerSocketCloseEvent {
            code: 1008,
            reason: "Session broker authentication failed.".into(),
            authenticated: Some(false),
        },
    ] {
        assert!(!is_quiescent_upgrade_refusal(&event));
    }
}

#[test]
fn keeps_previous_registration_when_live_connection_rejects_replacement() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming::default(),
        runtime(Arc::new(ManualScheduler::default()), warnings),
    );
    let connection = Arc::new(TestConnection::default());
    *connection.replace_error.lock().unwrap() = Some("connection exploded".into());
    client.inner.state.lock().unwrap().connection = Some(connection);
    let error = client
        .replace_session(registration("replacement-session"), snapshot(1))
        .unwrap_err();
    assert_eq!(error.to_string(), "connection exploded");
    assert_eq!(client.get_registration().session_id, "session-a");
}

#[test]
fn logs_one_actionable_warning_for_non_loopback_config_without_opt_in() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = runtime(Arc::new(ManualScheduler::default()), Arc::clone(&warnings));
    runtime.resolve_config = Arc::new(|| {
        resolve_session_broker_config(&BTreeMap::from([
            ("WORKDECK_MCP_HOST".into(), "0.0.0.0".into()),
            ("WORKDECK_MCP_PORT".into(), "47657".into()),
        ]))
        .map_err(|error| SessionBrokerClientError::Runtime(error.to_string()))
    });
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming::default(),
        runtime,
    );
    wait_attempt(&client.start());
    let messages = warnings.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("refuses to bind 0.0.0.0:47657"));
    assert!(messages[0].contains("WORKDECK_MCP_UNSAFE_ALLOW_REMOTE=1"));
    drop(messages);
    client.stop();
}

#[test]
fn retains_no_legacy_pid_based_incompatible_daemon_replacement_path() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let fingerprint = Arc::new(Mutex::new(Some("generation-a".into())));
    let mut runtime = runtime(Arc::new(ManualScheduler::default()), warnings);
    let fingerprint_read = Arc::clone(&fingerprint);
    runtime.read_launch_fingerprint = Arc::new(move |_| fingerprint_read.lock().unwrap().clone());
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming::default(),
        runtime,
    );
    client.resolve_close(
        &config(),
        SessionBrokerSocketCloseEvent {
            code: 1008,
            reason: "Malformed session broker protocol.".into(),
            authenticated: Some(false),
        },
    );
    let state = client.inner.state.lock().unwrap();
    assert!(state.waiting_for_incumbent_exit);
    assert_eq!(
        state.incumbent_launch_fingerprint.as_deref(),
        Some("generation-a")
    );
}

#[test]
fn logs_one_warning_per_older_window_and_waits_for_incumbent_generation_change() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let fingerprint = Arc::new(Mutex::new(Some("generation-a".into())));
    let healthy = Arc::new(AtomicBool::new(true));
    let mut clients = Vec::new();
    for id in ["session-a", "session-b"] {
        let mut runtime = runtime(Arc::new(ManualScheduler::default()), Arc::clone(&warnings));
        let fingerprint_read = Arc::clone(&fingerprint);
        let healthy_read = Arc::clone(&healthy);
        runtime.read_launch_fingerprint =
            Arc::new(move |_| fingerprint_read.lock().unwrap().clone());
        runtime.is_healthy = Arc::new(move |_| healthy_read.load(Ordering::SeqCst));
        let client = WorkdeckSessionBrokerClient::with_runtime(
            registration(id),
            snapshot(0),
            SessionBrokerClientTiming::default(),
            runtime,
        );
        let directive = client.resolve_close(
            &config(),
            SessionBrokerSocketCloseEvent {
                code: 1008,
                reason: "Session broker authentication required; upgrade Workdeck.".into(),
                authenticated: Some(false),
            },
        );
        client.warn_unavailable(directive.warning.as_deref().unwrap());
        assert_eq!(
            client.prepare_reconnect(&config()).unwrap_err().to_string(),
            WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE
        );
        clients.push(client);
    }
    assert_eq!(warnings.lock().unwrap().len(), 2);
    assert!(clients.iter().all(|client| {
        client
            .inner
            .state
            .lock()
            .unwrap()
            .waiting_for_incumbent_exit
    }));
}

#[test]
fn authenticates_after_successor_is_healthy_before_waiter_observes_absence() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let fingerprint = Arc::new(Mutex::new(Some("generation-a".into())));
    let ensured = Arc::new(AtomicUsize::new(0));
    let mut runtime = runtime(Arc::new(ManualScheduler::default()), warnings);
    runtime.is_healthy = Arc::new(|_| true);
    let fingerprint_read = Arc::clone(&fingerprint);
    runtime.read_launch_fingerprint = Arc::new(move |_| fingerprint_read.lock().unwrap().clone());
    let ensure_count = Arc::clone(&ensured);
    runtime.ensure_daemon = Arc::new(move |_, _| {
        ensure_count.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming::default(),
        runtime,
    );
    client.resolve_close(
        &config(),
        SessionBrokerSocketCloseEvent {
            code: 1008,
            reason: "Malformed session broker protocol.".into(),
            authenticated: Some(false),
        },
    );
    *fingerprint.lock().unwrap() = Some("generation-b".into());
    client.prepare_reconnect(&config()).unwrap();
    assert_eq!(ensured.load(Ordering::SeqCst), 1);
    assert!(
        !client
            .inner
            .state
            .lock()
            .unwrap()
            .waiting_for_incumbent_exit
    );
}

#[test]
fn waits_out_incompatible_incumbent_and_reuses_one_connection_for_successors() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let fingerprint = Arc::new(Mutex::new(Some("generation-a".into())));
    let healthy = Arc::new(AtomicBool::new(true));
    let ensured = Arc::new(AtomicUsize::new(0));
    let specs = Arc::new(Mutex::new(Vec::new()));
    let connection = Arc::new(TestConnection::default());
    let queue = Arc::new(Mutex::new(VecDeque::from([Arc::clone(&connection)])));
    let mut runtime = runtime(Arc::new(ManualScheduler::default()), Arc::clone(&warnings));
    runtime.create_connection = connection_factory(queue, Arc::clone(&specs));
    runtime.is_healthy = {
        let healthy = Arc::clone(&healthy);
        Arc::new(move |_| healthy.load(Ordering::SeqCst))
    };
    runtime.read_launch_fingerprint = {
        let fingerprint = Arc::clone(&fingerprint);
        Arc::new(move |_| fingerprint.lock().unwrap().clone())
    };
    runtime.ensure_daemon = {
        let ensured = Arc::clone(&ensured);
        Arc::new(move |_, _| {
            ensured.fetch_add(1, Ordering::SeqCst);
            Ok(())
        })
    };
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming {
            reconnect_delay: Duration::from_millis(10),
            ..SessionBrokerClientTiming::default()
        },
        runtime,
    );
    wait_attempt(&client.start());
    let spec = specs.lock().unwrap()[0].clone();
    let directive = (spec.resolve_close)(SessionBrokerSocketCloseEvent {
        code: 1008,
        reason: "Malformed session broker protocol.".into(),
        authenticated: Some(false),
    });
    (spec.on_warning)(directive.warning.as_deref().unwrap());
    assert!((spec.prepare_reconnect)().is_err());
    *fingerprint.lock().unwrap() = Some("generation-b".into());
    (spec.prepare_reconnect)().unwrap();
    (spec.on_connected)();
    healthy.store(false, Ordering::SeqCst);
    (spec.prepare_reconnect)().unwrap();
    assert_eq!(specs.lock().unwrap().len(), 1);
    assert_eq!(connection.starts.load(Ordering::SeqCst), 1);
    assert!(ensured.load(Ordering::SeqCst) >= 3);
    client.stop();
}

#[test]
fn repeated_start_after_success_retains_one_connection_generation() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let specs = Arc::new(Mutex::new(Vec::new()));
    let connection = Arc::new(TestConnection::default());
    let queue = Arc::new(Mutex::new(VecDeque::from([Arc::clone(&connection)])));
    let mut runtime = runtime(Arc::new(ManualScheduler::default()), warnings);
    runtime.create_connection = connection_factory(queue, Arc::clone(&specs));
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming::default(),
        runtime,
    );
    wait_attempt(&client.start());
    wait_attempt(&client.start());
    assert_eq!(specs.lock().unwrap().len(), 1);
    assert_eq!(connection.starts.load(Ordering::SeqCst), 1);
    client.stop();
}

#[test]
fn concurrent_starts_share_one_settlement_and_attempt() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let gate = Arc::new(Gate::default());
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_gate = Arc::clone(&gate);
    let start_attempts = Arc::clone(&attempts);
    let client = client_with_override(
        Arc::new(ManualScheduler::default()),
        warnings,
        Arc::new(move |_| {
            start_attempts.fetch_add(1, Ordering::SeqCst);
            start_gate.wait().map_err(SessionBrokerClientError::Runtime)
        }),
        Duration::from_millis(30),
    );
    let first = client.start();
    let second = client.start();
    assert_eq!(first.id(), second.id());
    wait_until(|| attempts.load(Ordering::SeqCst) == 1);
    gate.resolve();
    wait_attempt(&first);
    wait_attempt(&second);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    client.stop();
}

#[test]
fn manual_start_during_automatic_retry_keeps_original_deadline() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_attempts = Arc::clone(&attempts);
    let client = client_with_override(
        scheduler.clone(),
        Arc::clone(&warnings),
        Arc::new(move |_| {
            let attempt = start_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                Err(SessionBrokerClientError::Runtime(
                    "startup unavailable".into(),
                ))
            } else {
                Ok(())
            }
        }),
        Duration::from_millis(30),
    );
    wait_attempt(&client.start());
    assert_eq!(scheduler.pending(), 1);
    scheduler.advance(Duration::from_millis(10));
    wait_attempt(&client.start());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert_eq!(scheduler.pending(), 1);
    scheduler.advance(Duration::from_millis(19));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    scheduler.advance(Duration::from_millis(1));
    wait_until(|| attempts.load(Ordering::SeqCst) == 3);
    assert_eq!(scheduler.pending(), 0);
    assert_eq!(warnings.lock().unwrap().len(), 1);
    client.stop();
}

#[test]
fn eager_zero_delay_scheduler_cannot_lose_retry_ownership() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_attempts = Arc::clone(&attempts);
    let client = client_with_override(
        Arc::new(EagerScheduler),
        warnings,
        Arc::new(move |_| {
            let attempt = start_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                Err(SessionBrokerClientError::Runtime(
                    "first unavailable".into(),
                ))
            } else {
                Ok(())
            }
        }),
        Duration::ZERO,
    );
    wait_attempt(&client.start());
    wait_attempt(&client.start());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    client.stop();
}

#[test]
fn retry_deadline_during_active_attempt_is_consumed_and_coalesced() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let gate = Arc::new(Gate::default());
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_attempts = Arc::clone(&attempts);
    let start_gate = Arc::clone(&gate);
    let client = client_with_override(
        scheduler.clone(),
        Arc::clone(&warnings),
        Arc::new(move |_| {
            let attempt = start_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            match attempt {
                1 => Err(SessionBrokerClientError::Runtime(
                    "initial startup unavailable".into(),
                )),
                2 => start_gate.wait().map_err(SessionBrokerClientError::Runtime),
                _ => Ok(()),
            }
        }),
        Duration::from_millis(30),
    );
    wait_attempt(&client.start());
    scheduler.advance(Duration::from_millis(10));
    let manual = client.start();
    wait_until(|| attempts.load(Ordering::SeqCst) == 2);
    scheduler.advance(Duration::from_millis(20));
    assert_eq!(scheduler.pending(), 0);
    let joined = client.start();
    assert_eq!(manual.id(), joined.id());
    gate.reject("active startup unavailable");
    wait_attempt(&manual);
    wait_attempt(&joined);
    assert_eq!(scheduler.pending(), 1);
    assert_eq!(warnings.lock().unwrap().len(), 2);
    client.stop();
}

#[test]
fn failed_manual_attempt_retains_original_retry_without_duplication() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_attempts = Arc::clone(&attempts);
    let client = client_with_override(
        scheduler.clone(),
        Arc::clone(&warnings),
        Arc::new(move |_| {
            let attempt = start_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            match attempt {
                1 => Err(SessionBrokerClientError::Runtime(
                    "initial startup unavailable".into(),
                )),
                2 => Err(SessionBrokerClientError::Runtime(
                    "manual startup unavailable".into(),
                )),
                _ => Ok(()),
            }
        }),
        Duration::from_millis(30),
    );
    wait_attempt(&client.start());
    scheduler.advance(Duration::from_millis(10));
    wait_attempt(&client.start());
    assert_eq!(scheduler.pending(), 1);
    scheduler.advance(Duration::from_millis(19));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    scheduler.advance(Duration::from_millis(1));
    wait_until(|| attempts.load(Ordering::SeqCst) == 3);
    assert_eq!(scheduler.pending(), 0);
    assert_eq!(warnings.lock().unwrap().len(), 2);
    client.stop();
}

#[test]
fn warning_side_stop_is_terminal_and_clears_new_retry() {
    let scheduler = Arc::new(ManualScheduler::default());
    let client_slot = Arc::new(Mutex::new(None::<WorkdeckSessionBrokerClient>));
    let attempts = Arc::new(AtomicUsize::new(0));
    let mut runtime = runtime(scheduler.clone(), Arc::new(Mutex::new(Vec::new())));
    let start_attempts = Arc::clone(&attempts);
    runtime.startup_override = Some(Arc::new(move |_| {
        start_attempts.fetch_add(1, Ordering::SeqCst);
        Err(SessionBrokerClientError::Runtime(
            "startup unavailable".into(),
        ))
    }));
    let stop_slot = Arc::clone(&client_slot);
    runtime.warning_sink = Arc::new(move |_| {
        if let Some(client) = stop_slot.lock().unwrap().clone() {
            client.stop();
        }
    });
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming {
            reconnect_delay: Duration::from_millis(30),
            ..SessionBrokerClientTiming::default()
        },
        runtime,
    );
    *client_slot.lock().unwrap() = Some(client.clone());
    wait_attempt(&client.start());
    assert_eq!(scheduler.pending(), 0);
    scheduler.advance(Duration::from_millis(30));
    wait_attempt(&client.start());
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[test]
fn repeated_stop_clears_retained_retry_and_fences_late_manual_failure() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let gate = Arc::new(Gate::default());
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_attempts = Arc::clone(&attempts);
    let start_gate = Arc::clone(&gate);
    let client = client_with_override(
        scheduler.clone(),
        Arc::clone(&warnings),
        Arc::new(move |_| {
            let attempt = start_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                Err(SessionBrokerClientError::Runtime(
                    "initial startup unavailable".into(),
                ))
            } else {
                start_gate
                    .wait()
                    .map_err(SessionBrokerClientError::Runtime)?;
                Err(SessionBrokerClientError::Runtime(
                    "late manual startup failure".into(),
                ))
            }
        }),
        Duration::from_millis(30),
    );
    wait_attempt(&client.start());
    let manual = client.start();
    wait_until(|| attempts.load(Ordering::SeqCst) == 2);
    client.stop();
    client.stop();
    assert_eq!(scheduler.pending(), 0);
    gate.resolve();
    wait_attempt(&manual);
    assert_eq!(warnings.lock().unwrap().len(), 1);
    wait_attempt(&client.start());
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[test]
fn creates_fresh_connection_after_synchronous_socket_construction_failure() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let specs = Arc::new(Mutex::new(Vec::new()));
    let failed = Arc::new(TestConnection::default());
    *failed.start_error.lock().unwrap() = Some("socket factory exploded".into());
    let succeeding = Arc::new(TestConnection::default());
    let queue = Arc::new(Mutex::new(VecDeque::from([
        Arc::clone(&failed),
        Arc::clone(&succeeding),
    ])));
    let mut runtime = runtime(scheduler, warnings);
    runtime.create_connection = connection_factory(queue, Arc::clone(&specs));
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming {
            reconnect_delay: Duration::from_secs(10),
            ..SessionBrokerClientTiming::default()
        },
        runtime,
    );
    wait_attempt(&client.start());
    wait_attempt(&client.start());
    assert_eq!(specs.lock().unwrap().len(), 2);
    assert_eq!(failed.stops.load(Ordering::SeqCst), 1);
    assert_eq!(succeeding.starts.load(Ordering::SeqCst), 1);
    client.stop();
}

#[test]
fn preserves_reentrant_connection_replacement_and_original_start_failure() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let specs = Arc::new(Mutex::new(Vec::new()));
    let failed = Arc::new(TestConnection::default());
    failed.panic_on_stop.store(true, Ordering::SeqCst);
    *failed.start_error.lock().unwrap() = Some("socket construction".into());
    let replacement = Arc::new(TestConnection::default());
    let queue = Arc::new(Mutex::new(VecDeque::from([Arc::clone(&failed)])));
    let mut runtime = runtime(Arc::new(ManualScheduler::default()), warnings);
    runtime.create_connection = connection_factory(queue, specs);
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming::default(),
        runtime,
    );
    let credentials = (client.inner.runtime.load_credentials)().unwrap();
    client.inner.state.lock().unwrap().credentials = Some(credentials);
    let client_for_start = client.clone();
    let replacement_for_start = Arc::clone(&replacement);
    *failed.start_hook.lock().unwrap() = Some(Arc::new(move || {
        client_for_start.inner.state.lock().unwrap().connection =
            Some(replacement_for_start.clone());
        Ok(())
    }));
    let error = client.connect(&config()).unwrap_err();
    assert_eq!(error.to_string(), "socket construction");
    assert_eq!(failed.stops.load(Ordering::SeqCst), 1);
    client.update_snapshot(snapshot(2)).unwrap();
    assert_eq!(replacement.snapshots.lock().unwrap().len(), 1);
    client.stop();
    assert_eq!(replacement.stops.load(Ordering::SeqCst), 1);
}

fn assert_stop_fences_late_startup(reject: bool) {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let gate = Arc::new(Gate::default());
    let factories = Arc::new(AtomicUsize::new(0));
    let start_gate = Arc::clone(&gate);
    let mut runtime = runtime(scheduler.clone(), Arc::clone(&warnings));
    runtime.startup_override = Some(Arc::new(move |client| {
        start_gate
            .wait()
            .map_err(SessionBrokerClientError::Runtime)?;
        if reject {
            Err(SessionBrokerClientError::Runtime(
                "late startup failure".into(),
            ))
        } else {
            client.connect(&config())
        }
    }));
    let factory_count = Arc::clone(&factories);
    runtime.create_connection = Arc::new(move |_| {
        factory_count.fetch_add(1, Ordering::SeqCst);
        Ok(Arc::new(TestConnection::default()))
    });
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-a"),
        snapshot(0),
        SessionBrokerClientTiming {
            reconnect_delay: Duration::from_millis(30),
            ..SessionBrokerClientTiming::default()
        },
        runtime,
    );
    let attempt = client.start();
    client.stop();
    gate.resolve();
    wait_attempt(&attempt);
    assert!(warnings.lock().unwrap().is_empty());
    assert_eq!(factories.load(Ordering::SeqCst), 0);
    assert_eq!(scheduler.pending(), 0);
}

#[test]
fn stop_fences_late_resolved_startup_without_mutation() {
    assert_stop_fences_late_startup(false);
}

#[test]
fn stop_fences_late_rejected_startup_without_warning_or_retry() {
    assert_stop_fences_late_startup(true);
}

#[test]
fn automatic_retry_runs_complete_startup_cycle_and_recovers() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_attempts = Arc::clone(&attempts);
    let client = client_with_override(
        scheduler.clone(),
        Arc::clone(&warnings),
        Arc::new(move |_| {
            let attempt = start_attempts.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                Err(SessionBrokerClientError::Runtime(
                    "incumbent incompatible".into(),
                ))
            } else {
                Ok(())
            }
        }),
        Duration::from_millis(10),
    );
    wait_attempt(&client.start());
    scheduler.advance(Duration::from_millis(10));
    wait_until(|| attempts.load(Ordering::SeqCst) == 2);
    assert_eq!(
        warnings.lock().unwrap().as_slice(),
        ["[session:broker] incumbent incompatible"]
    );
    client.stop();
}

#[test]
fn start_after_terminal_stop_does_no_daemon_work() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let start_attempts = Arc::clone(&attempts);
    let client = client_with_override(
        Arc::new(ManualScheduler::default()),
        warnings,
        Arc::new(move |_| {
            start_attempts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Duration::from_millis(10),
    );
    client.stop();
    wait_attempt(&client.start());
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[test]
fn conflicting_listener_warning_is_actionable_and_deduplicated() {
    let warnings = Arc::new(Mutex::new(Vec::new()));
    let scheduler = Arc::new(ManualScheduler::default());
    let client = client_with_override(
        scheduler,
        Arc::clone(&warnings),
        Arc::new(move |_| {
            Err(SessionBrokerClientError::Runtime(
                "Workdeck session daemon port 127.0.0.1:47657 is already in use by another process. Stop the conflicting process or set WORKDECK_MCP_PORT to a different loopback port.".into(),
            ))
        }),
        Duration::from_secs(10),
    );
    wait_attempt(&client.start());
    wait_attempt(&client.start());
    let messages = warnings.lock().unwrap();
    assert_eq!(messages.len(), 1);
    assert!(messages[0].contains("port 127.0.0.1:47657 is already in use"));
    assert!(messages[0].contains("WORKDECK_MCP_PORT"));
    drop(messages);
    client.stop();
}

#[test]
fn native_client_authenticates_and_registers_with_native_daemon() {
    let root = tempfile::tempdir().unwrap();
    let env = BTreeMap::from([(
        "XDG_RUNTIME_DIR".into(),
        root.path().to_string_lossy().into_owned(),
    )]);
    let credentials =
        Arc::new(load_or_create_workdeck_session_broker_credentials(&env, None).unwrap());
    let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let endpoint = format!("ws://127.0.0.1:{port}{SESSION_BROKER_SOCKET_PATH}");
    let authenticator = SessionBrokerAuthenticator::new(SessionBrokerAuthenticatorOptions {
        app_id: WORKDECK_SESSION_BROKER_APP_ID.into(),
        app_revision: WORKDECK_SESSION_BROKER_APP_REVISION,
        generation: "generation-1".into(),
        daemon_identity: SessionBrokerDaemonIdentity {
            key_id: credentials.daemon_identity.key_id.clone(),
            private_key: credentials.daemon_identity.private_key.clone(),
        },
        credentials: vec![SessionBrokerAuthorityCredential {
            grant: BrokerGrant::Producer(ProducerGrant {
                base: credentials.producer.grant.base.clone(),
                operations: credentials.producer.grant.operations.clone(),
            }),
            public_key: credentials.producer.public_key,
        }],
        crypto: None,
        now: None,
        is_revoked: None,
        challenge_ttl_ms: None,
        caller_session_ttl_ms: None,
        max_challenges: None,
        max_challenge_bytes: None,
        max_challenge_transcript_bytes: None,
        max_caller_sessions: None,
        limits: SessionBrokerLimitOptions::default(),
    })
    .unwrap();
    let parsers = Arc::new(create_workdeck_session_protocol_parsers().unwrap());
    let broker = Arc::new(
        SessionBroker::new(SessionBrokerOptions {
            protocol_parsers: parsers,
            limit_options: SessionBrokerLimitOptions::default(),
            describe_session: None,
        })
        .unwrap(),
    );
    let mut daemon_options = SessionBrokerDaemonOptions::new(Arc::clone(&broker));
    daemon_options.app_id = Some(WORKDECK_SESSION_BROKER_APP_ID.into());
    daemon_options.app_revision = Some(u64::from(WORKDECK_SESSION_BROKER_APP_REVISION));
    daemon_options.producer_endpoint = Some(endpoint);
    daemon_options.hello_authenticator = Some(Arc::new(authenticator));
    daemon_options.idle_timeout_ms = Some(0);
    let daemon = SessionBrokerDaemon::new(daemon_options).unwrap();
    let mut serve_options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", port);
    serve_options.adapter_semantics = NativeSessionBrokerAdapterSemantics::Bun;
    let server = serve_session_broker_daemon(serve_options).unwrap();

    let warnings = Arc::new(Mutex::new(Vec::new()));
    let mut runtime = runtime(
        Arc::new(ThreadSessionBrokerRetryScheduler),
        Arc::clone(&warnings),
    );
    let live_config = ResolvedSessionBrokerConfig {
        host: "127.0.0.1".into(),
        port: u32::from(port),
        http_origin: format!("http://127.0.0.1:{port}"),
        ws_origin: format!("ws://127.0.0.1:{port}"),
    };
    runtime.resolve_config = Arc::new(move || Ok(live_config.clone()));
    runtime.ensure_daemon = Arc::new(|_, _| Ok(()));
    let client_credentials = Arc::clone(&credentials);
    runtime.load_credentials = Arc::new(move || Ok(Arc::clone(&client_credentials)));
    runtime.create_connection = Arc::new(create_native_client_connection);
    let client = WorkdeckSessionBrokerClient::with_runtime(
        registration("session-native"),
        snapshot(0),
        SessionBrokerClientTiming {
            reconnect_delay: Duration::from_millis(20),
            ..SessionBrokerClientTiming::default()
        },
        runtime,
    );
    wait_attempt(&client.start());
    wait_until(|| broker.list_sessions().len() == 1);
    assert_eq!(broker.list_sessions()[0].session_id, "session-native");
    assert!(warnings.lock().unwrap().is_empty());
    client.stop();
    server.stop();
    assert!(server.wait_stopped(Duration::from_secs(2)));
}
