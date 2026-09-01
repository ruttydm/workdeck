//! Workdeck producer client lifecycle layered over the runtime-neutral broker connection.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::Duration;

use thiserror::Error;

use crate::{
    EnsureSessionBrokerAvailableOptions, NativeSessionBrokerClientSocket,
    ResolvedSessionBrokerConfig, SESSION_BROKER_SOCKET_PATH, SessionBrokerClientCredential,
    SessionBrokerConnection, SessionBrokerConnectionBridge, SessionBrokerConnectionCloseDirective,
    SessionBrokerConnectionError, SessionBrokerConnectionOptions, SessionBrokerDaemonVerifier,
    SessionBrokerProducerAuthentication, SessionBrokerSocketCloseEvent,
    WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE, WORKDECK_SESSION_BROKER_APP_ID,
    WORKDECK_SESSION_BROKER_APP_REVISION, WorkdeckSessionBrokerCredentials,
    WorkdeckSessionCommandInput, WorkdeckSessionCommandResult, WorkdeckSessionRegistration,
    WorkdeckSessionSnapshot, create_workdeck_session_protocol_parsers,
    ensure_session_broker_available, is_session_broker_healthy,
    load_or_create_workdeck_session_broker_credentials, read_session_broker_launch_fingerprint,
    resolve_session_broker_config,
};

pub const WORKDECK_MCP_DISABLE_ENV: &str = "WORKDECK_MCP_DISABLE";
pub const SESSION_CLIENT_DAEMON_STARTUP_TIMEOUT: Duration = Duration::from_secs(3);
pub const SESSION_CLIENT_RECONNECT_DELAY: Duration = Duration::from_secs(3);
pub const SESSION_CLIENT_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
pub const INCOMPATIBLE_SESSION_CLOSE_CODE: u16 = 1008;

const QUIESCENT_REFUSAL_REASONS: [&str; 2] = [
    "Session broker authentication required; upgrade Workdeck.",
    "Malformed session broker protocol.",
];

pub type WorkdeckSessionAppBridge =
    dyn SessionBrokerConnectionBridge<WorkdeckSessionCommandInput, WorkdeckSessionCommandResult>;

/// Identify only known compatibility refusals emitted before producer authentication.
#[must_use]
pub fn is_quiescent_upgrade_refusal(event: &SessionBrokerSocketCloseEvent) -> bool {
    event.authenticated == Some(false)
        && event.code == INCOMPATIBLE_SESSION_CLOSE_CODE
        && QUIESCENT_REFUSAL_REASONS.contains(&event.reason.as_str())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionBrokerClientTiming {
    pub daemon_startup_timeout: Duration,
    pub reconnect_delay: Duration,
}

impl Default for SessionBrokerClientTiming {
    fn default() -> Self {
        Self {
            daemon_startup_timeout: SESSION_CLIENT_DAEMON_STARTUP_TIMEOUT,
            reconnect_delay: SESSION_CLIENT_RECONNECT_DELAY,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionBrokerClientError {
    #[error("{0}")]
    Runtime(String),
    #[error(transparent)]
    Connection(#[from] SessionBrokerConnectionError),
}

pub trait WorkdeckSessionClientConnection: Send + Sync {
    fn start(&self) -> Result<(), SessionBrokerClientError>;
    fn stop(&self);
    fn set_bridge(&self, bridge: Option<Arc<WorkdeckSessionAppBridge>>);
    fn replace_session(
        &self,
        registration: WorkdeckSessionRegistration,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError>;
    fn update_snapshot(
        &self,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError>;
}

type NativeWorkdeckConnection = SessionBrokerConnection<
    crate::WorkdeckSessionInfo,
    crate::WorkdeckSessionState,
    WorkdeckSessionCommandInput,
    WorkdeckSessionCommandResult,
>;

impl WorkdeckSessionClientConnection for NativeWorkdeckConnection {
    fn start(&self) -> Result<(), SessionBrokerClientError> {
        SessionBrokerConnection::start(self).map_err(Into::into)
    }

    fn stop(&self) {
        SessionBrokerConnection::stop(self);
    }

    fn set_bridge(&self, bridge: Option<Arc<WorkdeckSessionAppBridge>>) {
        SessionBrokerConnection::set_bridge(self, bridge);
    }

    fn replace_session(
        &self,
        registration: WorkdeckSessionRegistration,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError> {
        SessionBrokerConnection::replace_session(self, registration, snapshot).map_err(Into::into)
    }

    fn update_snapshot(
        &self,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError> {
        SessionBrokerConnection::update_snapshot(self, snapshot).map_err(Into::into)
    }
}

pub type ClientConnectionFactory = Arc<
    dyn Fn(
            SessionBrokerClientConnectionSpec,
        ) -> Result<Arc<dyn WorkdeckSessionClientConnection>, SessionBrokerClientError>
        + Send
        + Sync,
>;

#[derive(Clone)]
pub struct SessionBrokerClientConnectionSpec {
    pub config: ResolvedSessionBrokerConfig,
    pub registration: WorkdeckSessionRegistration,
    pub snapshot: WorkdeckSessionSnapshot,
    pub bridge: Option<Arc<WorkdeckSessionAppBridge>>,
    pub credentials: Arc<WorkdeckSessionBrokerCredentials>,
    pub reconnect_delay: Duration,
    pub prepare_reconnect: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    pub resolve_close: Arc<
        dyn Fn(SessionBrokerSocketCloseEvent) -> SessionBrokerConnectionCloseDirective
            + Send
            + Sync,
    >,
    pub on_connected: Arc<dyn Fn() + Send + Sync>,
    pub on_warning: Arc<dyn Fn(&str) + Send + Sync>,
}

impl std::fmt::Debug for SessionBrokerClientConnectionSpec {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerClientConnectionSpec")
            .field("config", &self.config)
            .field("registration", &self.registration)
            .field("snapshot", &self.snapshot)
            .field("reconnect_delay", &self.reconnect_delay)
            .finish_non_exhaustive()
    }
}

pub trait SessionBrokerRetryHandle: Send + Sync {
    fn cancel(&self);
}

pub trait SessionBrokerRetryScheduler: Send + Sync {
    fn schedule(
        &self,
        delay: Duration,
        callback: Box<dyn FnOnce() + Send>,
    ) -> Arc<dyn SessionBrokerRetryHandle>;
}

#[derive(Debug, Default)]
pub struct ThreadSessionBrokerRetryScheduler;

#[derive(Default)]
struct ThreadRetryState {
    cancelled: bool,
}

struct ThreadRetryHandle {
    state: (Mutex<ThreadRetryState>, Condvar),
}

#[derive(Default)]
struct DeferredRetryHandleState {
    cancelled: bool,
    inner: Option<Arc<dyn SessionBrokerRetryHandle>>,
}

#[derive(Default)]
struct DeferredRetryHandle {
    state: Mutex<DeferredRetryHandleState>,
}

impl DeferredRetryHandle {
    fn attach(&self, handle: Arc<dyn SessionBrokerRetryHandle>) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if state.cancelled {
            drop(state);
            handle.cancel();
        } else {
            state.inner = Some(handle);
        }
    }
}

impl SessionBrokerRetryHandle for DeferredRetryHandle {
    fn cancel(&self) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        state.cancelled = true;
        if let Some(handle) = state.inner.take() {
            drop(state);
            handle.cancel();
        }
    }
}

impl SessionBrokerRetryHandle for ThreadRetryHandle {
    fn cancel(&self) {
        let mut state = self
            .state
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.cancelled = true;
        self.state.1.notify_all();
    }
}

impl SessionBrokerRetryScheduler for ThreadSessionBrokerRetryScheduler {
    fn schedule(
        &self,
        delay: Duration,
        callback: Box<dyn FnOnce() + Send>,
    ) -> Arc<dyn SessionBrokerRetryHandle> {
        let handle = Arc::new(ThreadRetryHandle {
            state: (Mutex::new(ThreadRetryState::default()), Condvar::new()),
        });
        let worker = Arc::clone(&handle);
        let callback = Arc::new(Mutex::new(Some(callback)));
        let worker_callback = Arc::clone(&callback);
        if thread::Builder::new()
            .name("workdeck-session-startup-retry".into())
            .spawn(move || {
                let state = worker
                    .state
                    .0
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let state = worker
                    .state
                    .1
                    .wait_timeout_while(state, delay, |state| !state.cancelled)
                    .unwrap_or_else(|error| error.into_inner())
                    .0;
                if !state.cancelled {
                    drop(state);
                    if let Some(callback) = worker_callback
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take()
                    {
                        callback();
                    }
                }
            })
            .is_err()
            && let Some(callback) = callback
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
        {
            callback();
        }
        handle
    }
}

pub type SessionBrokerStartupOverride =
    Arc<dyn Fn(&WorkdeckSessionBrokerClient) -> Result<(), SessionBrokerClientError> + Send + Sync>;
pub type SessionBrokerDaemonEnsurer = Arc<
    dyn Fn(&ResolvedSessionBrokerConfig, Duration) -> Result<(), SessionBrokerClientError>
        + Send
        + Sync,
>;
pub type SessionBrokerLaunchFingerprintReader =
    Arc<dyn Fn(&ResolvedSessionBrokerConfig) -> Option<String> + Send + Sync>;

#[derive(Clone)]
pub struct SessionBrokerClientRuntime {
    pub disabled: Arc<dyn Fn() -> bool + Send + Sync>,
    pub resolve_config: Arc<
        dyn Fn() -> Result<ResolvedSessionBrokerConfig, SessionBrokerClientError> + Send + Sync,
    >,
    pub ensure_daemon: SessionBrokerDaemonEnsurer,
    pub load_credentials: Arc<
        dyn Fn() -> Result<Arc<WorkdeckSessionBrokerCredentials>, SessionBrokerClientError>
            + Send
            + Sync,
    >,
    pub is_healthy: Arc<dyn Fn(&ResolvedSessionBrokerConfig) -> bool + Send + Sync>,
    pub read_launch_fingerprint: SessionBrokerLaunchFingerprintReader,
    pub create_connection: ClientConnectionFactory,
    pub retry_scheduler: Arc<dyn SessionBrokerRetryScheduler>,
    pub warning_sink: Arc<dyn Fn(&str) + Send + Sync>,
    pub startup_override: Option<SessionBrokerStartupOverride>,
}

impl std::fmt::Debug for SessionBrokerClientRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerClientRuntime")
            .finish_non_exhaustive()
    }
}

impl Default for SessionBrokerClientRuntime {
    fn default() -> Self {
        Self::production()
    }
}

impl SessionBrokerClientRuntime {
    #[must_use]
    pub fn production() -> Self {
        Self {
            disabled: Arc::new(|| std::env::var(WORKDECK_MCP_DISABLE_ENV).as_deref() == Ok("1")),
            resolve_config: Arc::new(|| {
                resolve_session_broker_config(&std::env::vars().collect())
                    .map_err(|error| SessionBrokerClientError::Runtime(error.to_string()))
            }),
            ensure_daemon: Arc::new(|config, timeout| {
                let env = std::env::vars().collect::<BTreeMap<_, _>>();
                let mut options = EnsureSessionBrokerAvailableOptions::from_environment(env)
                    .map_err(|error| SessionBrokerClientError::Runtime(error.to_string()))?;
                options.config = config.clone();
                options.timeout = timeout;
                ensure_session_broker_available(&options)
                    .map_err(|error| SessionBrokerClientError::Runtime(error.to_string()))
            }),
            load_credentials: Arc::new(|| {
                load_or_create_workdeck_session_broker_credentials(
                    &std::env::vars().collect(),
                    None,
                )
                .map(Arc::new)
                .map_err(|error| SessionBrokerClientError::Runtime(error.to_string()))
            }),
            is_healthy: Arc::new(|config| {
                is_session_broker_healthy(config, Duration::from_millis(500))
            }),
            read_launch_fingerprint: Arc::new(|config| {
                read_session_broker_launch_fingerprint(config, &std::env::vars().collect())
            }),
            create_connection: Arc::new(create_native_client_connection),
            retry_scheduler: Arc::new(ThreadSessionBrokerRetryScheduler),
            warning_sink: Arc::new(|message| eprintln!("{message}")),
            startup_override: None,
        }
    }
}

#[derive(Clone)]
pub struct SessionBrokerStartup {
    id: u64,
    completion: Arc<(Mutex<bool>, Condvar)>,
}

impl std::fmt::Debug for SessionBrokerStartup {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerStartup")
            .field("id", &self.id)
            .field("complete", &self.is_complete())
            .finish()
    }
}

impl SessionBrokerStartup {
    fn pending(id: u64) -> Self {
        Self {
            id,
            completion: Arc::new((Mutex::new(false), Condvar::new())),
        }
    }

    fn completed() -> Self {
        let completion = Self::pending(0);
        completion.finish();
        completion
    }

    fn finish(&self) {
        let mut complete = self
            .completion
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *complete = true;
        self.completion.1.notify_all();
    }

    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub fn is_complete(&self) -> bool {
        *self
            .completion
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
    }

    pub fn wait(&self) {
        let complete = self
            .completion
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        drop(
            self.completion
                .1
                .wait_while(complete, |complete| !*complete)
                .unwrap_or_else(|error| error.into_inner()),
        );
    }

    #[must_use]
    pub fn wait_timeout(&self, timeout: Duration) -> bool {
        let complete = self
            .completion
            .0
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        *self
            .completion
            .1
            .wait_timeout_while(complete, timeout, |complete| !*complete)
            .unwrap_or_else(|error| error.into_inner())
            .0
    }
}

#[derive(Clone)]
struct ScheduledStartupRetry {
    id: u64,
    handle: Arc<dyn SessionBrokerRetryHandle>,
}

enum StartupLifecycleState {
    Idle,
    Attempting {
        attempt: SessionBrokerStartup,
        retry: Option<ScheduledStartupRetry>,
    },
    Waiting {
        retry: ScheduledStartupRetry,
    },
    Stopped,
}

struct SessionBrokerClientState {
    registration: WorkdeckSessionRegistration,
    snapshot: WorkdeckSessionSnapshot,
    bridge: Option<Arc<WorkdeckSessionAppBridge>>,
    connection: Option<Arc<dyn WorkdeckSessionClientConnection>>,
    startup: StartupLifecycleState,
    next_attempt_id: u64,
    next_retry_id: u64,
    last_connection_warning: Option<String>,
    credentials: Option<Arc<WorkdeckSessionBrokerCredentials>>,
    waiting_for_incumbent_exit: bool,
    incumbent_launch_fingerprint: Option<String>,
}

struct SessionBrokerClientInner {
    state: Mutex<SessionBrokerClientState>,
    timing: SessionBrokerClientTiming,
    runtime: SessionBrokerClientRuntime,
}

#[derive(Clone)]
pub struct WorkdeckSessionBrokerClient {
    inner: Arc<SessionBrokerClientInner>,
}

impl std::fmt::Debug for WorkdeckSessionBrokerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WorkdeckSessionBrokerClient")
            .field("registration", &self.get_registration())
            .finish_non_exhaustive()
    }
}

impl WorkdeckSessionBrokerClient {
    #[must_use]
    pub fn new(
        registration: WorkdeckSessionRegistration,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Self {
        Self::with_runtime(
            registration,
            snapshot,
            SessionBrokerClientTiming::default(),
            SessionBrokerClientRuntime::production(),
        )
    }

    #[must_use]
    pub fn with_runtime(
        registration: WorkdeckSessionRegistration,
        snapshot: WorkdeckSessionSnapshot,
        timing: SessionBrokerClientTiming,
        runtime: SessionBrokerClientRuntime,
    ) -> Self {
        Self {
            inner: Arc::new(SessionBrokerClientInner {
                state: Mutex::new(SessionBrokerClientState {
                    registration,
                    snapshot,
                    bridge: None,
                    connection: None,
                    startup: StartupLifecycleState::Idle,
                    next_attempt_id: 1,
                    next_retry_id: 1,
                    last_connection_warning: None,
                    credentials: None,
                    waiting_for_incumbent_exit: false,
                    incumbent_launch_fingerprint: None,
                }),
                timing,
                runtime,
            }),
        }
    }

    /// Start or join one startup attempt. Failures warn and resolve after owning one retry.
    #[must_use]
    pub fn start(&self) -> SessionBrokerStartup {
        if (self.inner.runtime.disabled)() {
            return SessionBrokerStartup::completed();
        }
        let attempt = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let StartupLifecycleState::Attempting { attempt, .. } = &state.startup {
                return attempt.clone();
            }
            if matches!(state.startup, StartupLifecycleState::Stopped) {
                return SessionBrokerStartup::completed();
            }
            let retry = match &state.startup {
                StartupLifecycleState::Waiting { retry } => Some(retry.clone()),
                StartupLifecycleState::Idle => None,
                StartupLifecycleState::Attempting { .. } | StartupLifecycleState::Stopped => {
                    unreachable!("attempting and stopped states returned above")
                }
            };
            self.begin_attempt_locked(&mut state, retry)
        };
        self.spawn_attempt(attempt.clone());
        attempt
    }

    pub fn stop(&self) {
        let connection = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            match &state.startup {
                StartupLifecycleState::Attempting {
                    retry: Some(retry), ..
                }
                | StartupLifecycleState::Waiting { retry } => retry.handle.cancel(),
                StartupLifecycleState::Idle
                | StartupLifecycleState::Attempting { retry: None, .. }
                | StartupLifecycleState::Stopped => {}
            }
            state.startup = StartupLifecycleState::Stopped;
            state.connection.take()
        };
        if let Some(connection) = connection {
            connection.stop();
        }
    }

    #[must_use]
    pub fn get_registration(&self) -> WorkdeckSessionRegistration {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .registration
            .clone()
    }

    pub fn replace_session(
        &self,
        registration: WorkdeckSessionRegistration,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError> {
        let connection = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .connection
            .clone();
        if let Some(connection) = connection {
            connection.replace_session(registration.clone(), snapshot.clone())?;
        }
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.registration = registration;
        state.snapshot = snapshot;
        Ok(())
    }

    pub fn set_bridge(&self, bridge: Option<Arc<WorkdeckSessionAppBridge>>) {
        let connection = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.bridge.clone_from(&bridge);
            state.connection.clone()
        };
        if let Some(connection) = connection {
            connection.set_bridge(bridge);
        }
    }

    pub fn update_snapshot(
        &self,
        snapshot: WorkdeckSessionSnapshot,
    ) -> Result<(), SessionBrokerClientError> {
        let connection = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.snapshot = snapshot.clone();
            state.connection.clone()
        };
        if let Some(connection) = connection {
            connection.update_snapshot(snapshot)?;
        }
        Ok(())
    }

    fn begin_attempt_locked(
        &self,
        state: &mut SessionBrokerClientState,
        retry: Option<ScheduledStartupRetry>,
    ) -> SessionBrokerStartup {
        let id = state.next_attempt_id;
        state.next_attempt_id = state.next_attempt_id.wrapping_add(1).max(1);
        let attempt = SessionBrokerStartup::pending(id);
        state.startup = StartupLifecycleState::Attempting {
            attempt: attempt.clone(),
            retry,
        };
        attempt
    }

    fn spawn_attempt(&self, attempt: SessionBrokerStartup) {
        let client = self.clone();
        let fallback = attempt.clone();
        if thread::Builder::new()
            .name("workdeck-session-client-startup".into())
            .spawn(move || {
                let result = client.inner.runtime.startup_override.as_ref().map_or_else(
                    || client.ensure_daemon_and_connect(),
                    |start| start(&client),
                );
                if let Err(error) = result {
                    client.handle_startup_failure(attempt.id(), error.to_string());
                }
                client.handle_startup_settlement(attempt.id());
                attempt.finish();
            })
            .is_err()
        {
            self.handle_startup_failure(
                fallback.id(),
                "Could not start session client worker.".into(),
            );
            self.handle_startup_settlement(fallback.id());
            fallback.finish();
        }
    }

    fn ensure_daemon_and_connect(&self) -> Result<(), SessionBrokerClientError> {
        let config = (self.inner.runtime.resolve_config)()?;
        (self.inner.runtime.ensure_daemon)(&config, self.inner.timing.daemon_startup_timeout)?;
        let has_credentials = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .credentials
            .is_some();
        if !has_credentials {
            let credentials = (self.inner.runtime.load_credentials)()?;
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.credentials.is_none() {
                state.credentials = Some(credentials);
            }
        }
        self.connect(&config)
    }

    fn connect(
        &self,
        config: &ResolvedSessionBrokerConfig,
    ) -> Result<(), SessionBrokerClientError> {
        let (registration, snapshot, bridge, credentials) = {
            let state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if matches!(state.startup, StartupLifecycleState::Stopped) || state.connection.is_some()
            {
                return Ok(());
            }
            let Some(credentials) = state.credentials.clone() else {
                return Ok(());
            };
            (
                state.registration.clone(),
                state.snapshot.clone(),
                state.bridge.clone(),
                credentials,
            )
        };
        let prepare_client = self.clone();
        let prepare_config = config.clone();
        let close_client = self.clone();
        let close_config = config.clone();
        let connected_client = self.clone();
        let warning_client = self.clone();
        let connection =
            (self.inner.runtime.create_connection)(SessionBrokerClientConnectionSpec {
                config: config.clone(),
                registration,
                snapshot,
                bridge,
                credentials,
                reconnect_delay: self.inner.timing.reconnect_delay,
                prepare_reconnect: Arc::new(move || {
                    prepare_client
                        .prepare_reconnect(&prepare_config)
                        .map_err(|error| error.to_string())
                }),
                resolve_close: Arc::new(move |event| {
                    close_client.resolve_close(&close_config, event)
                }),
                on_connected: Arc::new(move || connected_client.on_connected()),
                on_warning: Arc::new(move |message| warning_client.warn_unavailable(message)),
            })?;
        let published = {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if matches!(state.startup, StartupLifecycleState::Stopped) || state.connection.is_some()
            {
                false
            } else {
                state.connection = Some(Arc::clone(&connection));
                true
            }
        };
        if !published {
            connection.stop();
            return Ok(());
        }
        if let Err(error) = connection.start() {
            let _ = catch_unwind(AssertUnwindSafe(|| connection.stop()));
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state
                .connection
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &connection))
            {
                state.connection = None;
            }
            return Err(error);
        }
        Ok(())
    }

    fn prepare_reconnect(
        &self,
        config: &ResolvedSessionBrokerConfig,
    ) -> Result<(), SessionBrokerClientError> {
        let incumbent = {
            let state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state
                .waiting_for_incumbent_exit
                .then(|| state.incumbent_launch_fingerprint.clone())
        };
        if let Some(incumbent) = incumbent {
            if (self.inner.runtime.is_healthy)(config)
                && (self.inner.runtime.read_launch_fingerprint)(config) == incumbent
            {
                return Err(SessionBrokerClientError::Runtime(
                    WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE.into(),
                ));
            }
            self.inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .waiting_for_incumbent_exit = false;
        }
        (self.inner.runtime.ensure_daemon)(config, self.inner.timing.daemon_startup_timeout)
    }

    fn resolve_close(
        &self,
        config: &ResolvedSessionBrokerConfig,
        event: SessionBrokerSocketCloseEvent,
    ) -> SessionBrokerConnectionCloseDirective {
        let refusal = is_quiescent_upgrade_refusal(&event);
        if refusal {
            let fingerprint = (self.inner.runtime.read_launch_fingerprint)(config);
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.waiting_for_incumbent_exit = true;
            state.incumbent_launch_fingerprint = fingerprint;
        }
        SessionBrokerConnectionCloseDirective {
            reconnect: Some(true),
            warning: refusal.then(|| WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE.into()),
        }
    }

    fn on_connected(&self) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.waiting_for_incumbent_exit = false;
        state.incumbent_launch_fingerprint = None;
        state.last_connection_warning = None;
    }

    fn handle_startup_failure(&self, attempt_id: u64, message: String) {
        let needs_retry = {
            let state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            matches!(
                &state.startup,
                StartupLifecycleState::Attempting { attempt, retry: None }
                    if attempt.id() == attempt_id
            )
        };
        if needs_retry {
            let retry_id = {
                let mut state = self
                    .inner
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                let id = state.next_retry_id;
                state.next_retry_id = state.next_retry_id.wrapping_add(1).max(1);
                id
            };
            let deferred = Arc::new(DeferredRetryHandle::default());
            let retry = ScheduledStartupRetry {
                id: retry_id,
                handle: deferred.clone(),
            };
            let installed = {
                let mut state = self
                    .inner
                    .state
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if matches!(
                    &state.startup,
                    StartupLifecycleState::Attempting { attempt, retry: None }
                        if attempt.id() == attempt_id
                ) {
                    if let StartupLifecycleState::Attempting { retry: slot, .. } =
                        &mut state.startup
                    {
                        *slot = Some(retry.clone());
                    }
                    true
                } else {
                    false
                }
            };
            if !installed {
                retry.handle.cancel();
                return;
            }
            // Publish retry ownership before the scheduler can fire. This preserves the source
            // runtime's run-to-completion timer semantics even for zero-delay native schedulers.
            let weak = Arc::downgrade(&self.inner);
            let handle = self.inner.runtime.retry_scheduler.schedule(
                self.inner.timing.reconnect_delay,
                Box::new(move || handle_retry_deadline(weak, retry_id)),
            );
            deferred.attach(handle);
        }
        let current = {
            let state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            matches!(
                &state.startup,
                StartupLifecycleState::Attempting { attempt, .. }
                    if attempt.id() == attempt_id
            )
        };
        if current {
            self.warn_unavailable(&message);
        }
    }

    fn handle_startup_settlement(&self, attempt_id: u64) {
        let mut state = self
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let replacement = match &state.startup {
            StartupLifecycleState::Attempting { attempt, retry } if attempt.id() == attempt_id => {
                retry.clone().map_or(StartupLifecycleState::Idle, |retry| {
                    StartupLifecycleState::Waiting { retry }
                })
            }
            _ => return,
        };
        state.startup = replacement;
    }

    fn warn_unavailable(&self, message: &str) {
        let rendered = format!("[session:broker] {message}");
        {
            let mut state = self
                .inner
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if state.last_connection_warning.as_deref() == Some(message) {
                return;
            }
            state.last_connection_warning = Some(message.into());
        }
        (self.inner.runtime.warning_sink)(&rendered);
    }
}

fn handle_retry_deadline(inner: Weak<SessionBrokerClientInner>, retry_id: u64) {
    let Some(inner) = inner.upgrade() else {
        return;
    };
    let client = WorkdeckSessionBrokerClient { inner };
    let start = {
        let mut state = client
            .inner
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match &mut state.startup {
            StartupLifecycleState::Waiting { retry } if retry.id == retry_id => {
                state.startup = StartupLifecycleState::Idle;
                true
            }
            StartupLifecycleState::Attempting { retry, .. }
                if retry.as_ref().is_some_and(|retry| retry.id == retry_id) =>
            {
                *retry = None;
                false
            }
            _ => false,
        }
    };
    if start {
        let _ = client.start();
    }
}

fn create_native_client_connection(
    spec: SessionBrokerClientConnectionSpec,
) -> Result<Arc<dyn WorkdeckSessionClientConnection>, SessionBrokerClientError> {
    let url = format!("{}{}", spec.config.ws_origin, SESSION_BROKER_SOCKET_PATH);
    let mut options = SessionBrokerConnectionOptions::new(
        url,
        Arc::new(|url: &str| NativeSessionBrokerClientSocket::connect(url)),
        spec.registration,
        spec.snapshot,
        Arc::new(
            create_workdeck_session_protocol_parsers()
                .map_err(|error| SessionBrokerClientError::Runtime(error.to_string()))?,
        ),
    );
    options.bridge = spec.bridge;
    options.producer_authentication = Some(SessionBrokerProducerAuthentication::native(
        WORKDECK_SESSION_BROKER_APP_ID,
        WORKDECK_SESSION_BROKER_APP_REVISION,
        SessionBrokerClientCredential {
            grant: spec.credentials.producer.grant.clone(),
            private_key: spec.credentials.producer.private_key.clone(),
        },
        SessionBrokerDaemonVerifier {
            key_id: spec.credentials.daemon_identity.key_id.clone(),
            public_key: spec.credentials.daemon_public_key,
        },
    ));
    options.heartbeat_interval_ms = duration_millis(SESSION_CLIENT_HEARTBEAT_INTERVAL);
    options.reconnect_delay_ms = duration_millis(spec.reconnect_delay);
    options.prepare_reconnect = Some(spec.prepare_reconnect);
    options.resolve_close = Some(spec.resolve_close);
    options.on_connected = Some(spec.on_connected);
    options.on_warning = Some(spec.on_warning);
    Ok(Arc::new(SessionBrokerConnection::new(options)?))
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
