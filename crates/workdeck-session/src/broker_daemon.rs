//! Runtime-neutral authenticated session-broker daemon composition.

use std::collections::BTreeMap;
use std::future::Future;
use std::marker::PhantomData;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;
use url::Url;

use crate::{
    AuthenticatedCallerRequest, AuthenticatedProducerHello, BrokerCapacityCode,
    BrokerCommandOutcome, BrokerProtocolError, CallerOperation, CallerRequestAuthenticationInput,
    CallerRequestAuthenticator, CallerResponseSigningInput, DEFAULT_SESSION_BROKER_API_PATH,
    DEFAULT_SESSION_BROKER_CAPABILITIES_PATH, DEFAULT_SESSION_BROKER_HEALTH_PATH,
    DEFAULT_SESSION_BROKER_SOCKET_PATH, DaemonSessionSocket, HandleCommandResult,
    MarkSessionSeenResult, ProducerOperation, ProducerPrincipal, RegisterSessionOptions,
    RegisterSessionResult, ResourceBudget, SessionBroker, SessionBrokerAuditDecision,
    SessionBrokerAuditEvent, SessionBrokerAuditHook, SessionBrokerAuditOperation,
    SessionBrokerAuditOutcome, SessionBrokerAuthenticatedResponse,
    SessionBrokerAuthenticationFailureCode, SessionBrokerAuthorizationContext,
    SessionBrokerAuthorizer, SessionBrokerCancellation, SessionBrokerCapabilities,
    SessionBrokerController, SessionBrokerHelloAuthenticator, SessionBrokerHttpPaths,
    SessionBrokerLimitOptions, SessionBrokerLimits, SessionBrokerStateError,
    SharedDaemonSessionSocket, SignedBrokerAppContract, StructuralCommandOutcome,
    StructuralSessionBrokerDaemonRequest, StructuralSessionClientMessage, UpdateSnapshotResult,
    caller_principal_allows, canonicalize_json, is_valid_broker_app_id, is_valid_broker_revision,
    merge_session_broker_limits, parse_session_broker_json_bytes, producer_principal_allows,
};

const DEFAULT_STALE_SESSION_TTL_MS: u64 = 45_000;
const DEFAULT_STALE_SESSION_SWEEP_INTERVAL_MS: u64 = 15_000;
const DEFAULT_IDLE_TIMEOUT_MS: u64 = 60_000;
const INCOMPATIBLE_PAYLOAD_CLOSE_CODE: u16 = 1008;

type BrokerDaemonContract<Info, State, CommandInput, CommandResult> =
    fn() -> (Info, State, CommandInput, CommandResult);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Invalid session broker daemon configuration: {0}")]
pub struct SessionBrokerDaemonConfigError(pub String);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionBrokerDaemonPathOptions {
    pub health: Option<String>,
    pub socket: Option<String>,
    pub api: Option<String>,
    pub capabilities: Option<String>,
}

/// The facts the app maps from its own session view into one admin status entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerAdminSessionFacts {
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub pid: u64,
}

/// Configure the revision-tolerant admin scope (`status` and `stop`).
///
/// The authenticator is a second instance built with the admin scope version in place of the
/// app revision, sharing the daemon identity and on-disk credentials; its caller sessions are
/// unknown to the main authenticator, so an admin caller can never reach the session API.
pub struct SessionBrokerDaemonAdminOptions<ListedSession> {
    /// One authenticator serving both the admin hello and its caller requests, built with the
    /// admin scope version in place of the app revision.
    pub authenticator: Arc<crate::SessionBrokerAuthenticator>,
    pub paths: Option<crate::SessionBrokerAdminPaths>,
    /// Human-readable app build version reported beside the app revision.
    pub app_version: String,
    /// Map the app's own session view to the frozen v1 session entry.
    pub describe_session:
        Arc<dyn Fn(&ListedSession) -> SessionBrokerAdminSessionFacts + Send + Sync>,
}

impl<ListedSession> std::fmt::Debug for SessionBrokerDaemonAdminOptions<ListedSession> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerDaemonAdminOptions")
            .field("paths", &self.paths)
            .field("app_version", &self.app_version)
            .finish_non_exhaustive()
    }
}

pub struct SessionBrokerDaemonOptions<
    Info,
    State,
    CommandInput,
    CommandResult,
    Controller = SessionBroker<Info, State, CommandInput, CommandResult>,
> where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    pub broker: Arc<Controller>,
    pub capabilities: Option<SessionBrokerCapabilities>,
    pub paths: SessionBrokerDaemonPathOptions,
    pub expose_http_api: bool,
    pub caller_authenticator: Option<Arc<dyn CallerRequestAuthenticator>>,
    pub hello_authenticator: Option<Arc<dyn SessionBrokerHelloAuthenticator>>,
    pub admin: Option<SessionBrokerDaemonAdminOptions<Controller::ListedSession>>,
    pub producer_endpoint: Option<String>,
    pub authorizer: Option<Arc<dyn SessionBrokerAuthorizer>>,
    pub audit: Option<Arc<dyn SessionBrokerAuditHook>>,
    pub app_id: Option<String>,
    pub app_revision: Option<u64>,
    pub idle_timeout_ms: Option<u64>,
    pub stale_session_ttl_ms: Option<u64>,
    pub stale_session_sweep_interval_ms: Option<u64>,
    pub limit_options: SessionBrokerLimitOptions,
    contract: PhantomData<BrokerDaemonContract<Info, State, CommandInput, CommandResult>>,
}

impl<Info, State, CommandInput, CommandResult, Controller>
    SessionBrokerDaemonOptions<Info, State, CommandInput, CommandResult, Controller>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    #[must_use]
    pub fn new(broker: Arc<Controller>) -> Self {
        Self {
            broker,
            capabilities: None,
            paths: SessionBrokerDaemonPathOptions::default(),
            expose_http_api: false,
            caller_authenticator: None,
            hello_authenticator: None,
            admin: None,
            producer_endpoint: None,
            authorizer: None,
            audit: None,
            app_id: None,
            app_revision: None,
            idle_timeout_ms: None,
            stale_session_ttl_ms: None,
            stale_session_sweep_interval_ms: None,
            limit_options: SessionBrokerLimitOptions::default(),
            contract: PhantomData,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerHttpRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl SessionBrokerHttpRequest {
    #[must_use]
    pub fn get(url: impl Into<String>) -> Self {
        Self {
            method: "GET".into(),
            url: url.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    #[must_use]
    pub fn json(url: impl Into<String>, value: &Value) -> Self {
        Self {
            method: "POST".into(),
            url: url.into(),
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: serde_json::to_vec(value).expect("JSON values always serialize"),
        }
    }

    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl SessionBrokerHttpResponse {
    #[must_use]
    pub fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: serde_json::to_vec(value).expect("JSON values always serialize"),
        }
    }

    #[must_use]
    pub fn empty(status: u16) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: Vec::new(),
        }
    }

    pub fn json_body(&self) -> Result<Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }
}

/// WebSocket peer boundary owned by the native listener rather than the daemon engine.
pub trait SessionBrokerDaemonPeer: Send + Sync {
    fn send(&self, data: &str) -> Result<(), String>;
    fn close(&self, code: Option<u16>, reason: Option<&str>);
    fn mark_authenticated(&self);
}

pub type SharedSessionBrokerDaemonPeer = Arc<dyn SessionBrokerDaemonPeer>;

#[derive(Debug, Clone, PartialEq)]
pub struct SessionBrokerAuthenticatedControlFacts {
    pub operation: CallerOperation,
    pub session_id: Option<String>,
    pub command: Option<String>,
    pub command_version: Option<u64>,
    pub target_specific: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionBrokerAuthenticatedControlResult {
    pub body: Value,
    pub status: u16,
}

type PayloadTooLargeResponse = Arc<dyn Fn() -> SessionBrokerHttpResponse + Send + Sync>;
type ResolveFailureTarget = Arc<dyn Fn(&[u8]) -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct SessionBrokerBoundedControlOptions {
    pub max_body_bytes: Option<u64>,
    pub payload_too_large: Option<PayloadTooLargeResponse>,
}

#[derive(Clone, Default)]
pub struct SessionBrokerAuthenticatedControlOptions {
    pub authentication_failure_operation: Option<CallerOperation>,
    pub resolve_failure_target_specific: Option<ResolveFailureTarget>,
}

pub struct SessionBrokerDaemon<
    Info,
    State,
    CommandInput,
    CommandResult,
    Controller = SessionBroker<Info, State, CommandInput, CommandResult>,
> where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    inner: Arc<DaemonInner<Info, State, CommandInput, CommandResult, Controller>>,
}

impl<Info, State, CommandInput, CommandResult, Controller> Clone
    for SessionBrokerDaemon<Info, State, CommandInput, CommandResult, Controller>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

struct DaemonInner<Info, State, CommandInput, CommandResult, Controller>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    broker: Arc<Controller>,
    paths: SessionBrokerHttpPaths,
    capabilities: SessionBrokerCapabilities,
    limits: SessionBrokerLimits,
    app_id: String,
    app_revision: u64,
    caller_authenticator: Option<Arc<dyn CallerRequestAuthenticator>>,
    hello_authenticator: Option<Arc<dyn SessionBrokerHelloAuthenticator>>,
    admin: Option<DaemonAdminInner<Controller::ListedSession>>,
    producer_endpoint: Option<String>,
    authorizer: Option<Arc<dyn SessionBrokerAuthorizer>>,
    audit: Option<Arc<dyn SessionBrokerAuditHook>>,
    started_at_ms: u64,
    idle_timeout_ms: u64,
    stale_session_ttl_ms: u64,
    stale_session_sweep_interval_ms: u64,
    last_activity_at: AtomicU64,
    shutting_down: AtomicBool,
    stopped: (Mutex<bool>, Condvar),
    http_control_budget: ResourceBudget,
    http_body_budget: ResourceBudget,
    next_connection_id: AtomicU64,
    producers: Mutex<ProducerState>,
    contract: PhantomData<BrokerDaemonContract<Info, State, CommandInput, CommandResult>>,
}

#[derive(Default)]
struct ProducerState {
    authentication: BTreeMap<usize, ProducerAuthenticationState>,
    sockets: BTreeMap<usize, SharedDaemonSessionSocket>,
    owners: BTreeMap<String, ProducerOwner>,
    reconnects: BTreeMap<String, ProducerReconnect>,
}

enum ProducerAuthenticationState {
    Challenged(u64),
    Authenticated {
        authority: Arc<AuthenticatedProducerHello>,
        broker_socket: SharedDaemonSessionSocket,
        session_id: Option<String>,
    },
}

struct DaemonAdminInner<ListedSession> {
    authenticator: Arc<crate::SessionBrokerAuthenticator>,
    paths: crate::SessionBrokerAdminPaths,
    app_version: String,
    describe_session: Arc<dyn Fn(&ListedSession) -> SessionBrokerAdminSessionFacts + Send + Sync>,
}

#[derive(Clone)]
struct ProducerOwner {
    peer: SharedSessionBrokerDaemonPeer,
    broker_socket: SharedDaemonSessionSocket,
    principal: ProducerPrincipal,
    /// The app revision this producer presented in its hello.
    app_revision: Option<u64>,
}

#[derive(Clone)]
struct ProducerReconnect {
    principal: ProducerPrincipal,
    disconnected_at: u64,
}

struct DaemonPeerSocket {
    peer: SharedSessionBrokerDaemonPeer,
    authority: Option<Arc<AuthenticatedProducerHello>>,
}

impl DaemonSessionSocket for DaemonPeerSocket {
    fn send(&self, data: &str) -> Result<bool, SessionBrokerStateError> {
        if let Some(authority) = &self.authority
            && !producer_authority_is_active(authority)
        {
            self.peer.close(
                Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                Some("Session producer authority expired."),
            );
            return Err(SessionBrokerStateError::message(
                "Session producer authority expired.",
            ));
        }
        self.peer
            .send(data)
            .map(|()| true)
            .map_err(SessionBrokerStateError::message)
    }
}

impl<Info, State, CommandInput, CommandResult, Controller>
    SessionBrokerDaemon<Info, State, CommandInput, CommandResult, Controller>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    pub fn new(
        options: SessionBrokerDaemonOptions<Info, State, CommandInput, CommandResult, Controller>,
    ) -> Result<Self, SessionBrokerDaemonConfigError> {
        let broker_limits = options.broker.limits();
        let limits = merge_session_broker_limits(broker_limits, &options.limit_options)
            .map_err(|error| SessionBrokerDaemonConfigError(error.to_string()))?;
        if !same_broker_state_limits(&limits, &broker_limits) {
            return Err(SessionBrokerDaemonConfigError(
                "Session broker state limits must be configured on the broker controller before daemon composition."
                    .into(),
            ));
        }
        let explicit_app_id_is_valid = options
            .app_id
            .as_deref()
            .is_some_and(is_valid_broker_app_id);
        let explicit_app_revision_is_valid =
            options.app_revision.is_some_and(is_valid_broker_revision);
        let app_id = options.app_id.unwrap_or_else(|| "session-broker".into());
        let app_revision = options.broker.protocol_parsers().app_revision;
        if options
            .app_revision
            .is_some_and(|revision| revision != app_revision)
        {
            return Err(SessionBrokerDaemonConfigError(
                "Session broker app revision does not match its parser registry.".into(),
            ));
        }
        if options.producer_endpoint.is_some() && options.hello_authenticator.is_none() {
            return Err(SessionBrokerDaemonConfigError(
                "Authenticated producer transport requires a hello authenticator.".into(),
            ));
        }
        let admin = options
            .admin
            .map(|admin| {
                if options.authorizer.is_none() {
                    // The admin scope exists so an operator's existing caller credential can
                    // inspect and retire a daemon; authorization stays mandatory even for it.
                    return Err(SessionBrokerDaemonConfigError(
                        "The session broker admin scope requires an authorizer.".into(),
                    ));
                }
                Ok(DaemonAdminInner {
                    paths: admin
                        .paths
                        .unwrap_or_else(crate::default_session_broker_admin_paths),
                    authenticator: admin.authenticator,
                    app_version: admin.app_version,
                    describe_session: admin.describe_session,
                })
            })
            .transpose()?;
        let expose = options.expose_http_api
            && explicit_app_id_is_valid
            && explicit_app_revision_is_valid
            && options.caller_authenticator.is_some()
            && options.authorizer.is_some();
        let paths = SessionBrokerHttpPaths {
            health: options
                .paths
                .health
                .unwrap_or_else(|| DEFAULT_SESSION_BROKER_HEALTH_PATH.into()),
            socket: options
                .paths
                .socket
                .unwrap_or_else(|| DEFAULT_SESSION_BROKER_SOCKET_PATH.into()),
            api: expose.then(|| {
                options
                    .paths
                    .api
                    .unwrap_or_else(|| DEFAULT_SESSION_BROKER_API_PATH.into())
            }),
            capabilities: expose.then(|| {
                options
                    .paths
                    .capabilities
                    .unwrap_or_else(|| DEFAULT_SESSION_BROKER_CAPABILITIES_PATH.into())
            }),
        };
        let now = now_ms();
        let inner = Arc::new(DaemonInner {
            broker: options.broker,
            paths,
            capabilities: options.capabilities.unwrap_or(SessionBrokerCapabilities {
                version: 1,
                name: None,
                features: None,
                extra: BTreeMap::new(),
            }),
            limits,
            app_id,
            app_revision,
            caller_authenticator: options.caller_authenticator,
            hello_authenticator: options.hello_authenticator,
            admin,
            producer_endpoint: options.producer_endpoint,
            authorizer: options.authorizer,
            audit: options.audit,
            started_at_ms: now,
            idle_timeout_ms: options.idle_timeout_ms.unwrap_or(DEFAULT_IDLE_TIMEOUT_MS),
            stale_session_ttl_ms: options
                .stale_session_ttl_ms
                .unwrap_or(DEFAULT_STALE_SESSION_TTL_MS),
            stale_session_sweep_interval_ms: options
                .stale_session_sweep_interval_ms
                .unwrap_or(DEFAULT_STALE_SESSION_SWEEP_INTERVAL_MS),
            last_activity_at: AtomicU64::new(now),
            shutting_down: AtomicBool::new(false),
            stopped: (Mutex::new(false), Condvar::new()),
            http_control_budget: ResourceBudget::with_code(
                limits.max_concurrent_http_controls,
                "maxConcurrentHttpControls",
                BrokerCapacityCode::Busy,
            ),
            http_body_budget: ResourceBudget::new(
                limits.max_in_flight_http_body_bytes,
                "maxInFlightHttpBodyBytes",
            ),
            next_connection_id: AtomicU64::new(1),
            producers: Mutex::new(ProducerState::default()),
            contract: PhantomData,
        });
        start_lifecycle(&inner);
        Ok(Self { inner })
    }

    #[must_use]
    pub fn paths(&self) -> SessionBrokerHttpPaths {
        self.inner.paths.clone()
    }

    #[must_use]
    pub fn limits(&self) -> SessionBrokerLimits {
        self.inner.limits
    }

    #[must_use]
    pub fn list_sessions(&self) -> Vec<Controller::ListedSession> {
        self.inner.broker.list_sessions()
    }

    pub fn get_session(
        &self,
        selector: &crate::SessionSelector,
    ) -> Result<Controller::ListedSession, SessionBrokerStateError> {
        self.inner.broker.get_session(selector)
    }

    #[must_use]
    pub fn get_health(&self) -> crate::SessionBrokerHealth {
        let uptime_ms = now_ms().saturating_sub(self.inner.started_at_ms);
        let started = chrono::DateTime::from_timestamp_millis(
            i64::try_from(self.inner.started_at_ms).unwrap_or(i64::MAX),
        )
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        crate::SessionBrokerHealth {
            ok: true,
            pid: u64::from(std::process::id()),
            sessions: self.inner.broker.session_count() as u64,
            pending_commands: self.inner.broker.pending_command_count() as u64,
            started_at: started,
            uptime_ms,
            stale_session_ttl_ms: self.inner.stale_session_ttl_ms,
            paths: self.inner.paths.clone(),
        }
    }

    #[must_use]
    pub fn matches_socket_path(&self, pathname: &str) -> bool {
        pathname == self.inner.paths.socket
    }

    #[must_use]
    pub fn requires_producer_authentication(&self) -> bool {
        self.inner.producer_endpoint.is_some()
    }

    pub fn wait_stopped(&self, timeout: Duration) -> bool {
        let (lock, ready) = &self.inner.stopped;
        let stopped = lock.lock().unwrap_or_else(|error| error.into_inner());
        if *stopped {
            return true;
        }
        *ready
            .wait_timeout_while(stopped, timeout, |stopped| !*stopped)
            .unwrap_or_else(|error| error.into_inner())
            .0
    }

    pub fn shutdown(&self, error: Option<SessionBrokerStateError>) {
        self.shutdown_with_options(error, None);
    }

    /// Begin graceful shutdown. When a close reason is given, attached producers are closed
    /// with it before broker state is torn down so their windows can tell a restart from a
    /// crash.
    pub fn shutdown_with_options(
        &self,
        error: Option<SessionBrokerStateError>,
        producer_close_reason: Option<&str>,
    ) {
        if self.inner.shutting_down.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(reason) = producer_close_reason {
            let owners = {
                self.inner
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .owners
                    .clone()
            };
            for owner in owners.values() {
                // A transport that already failed cannot block shutdown.
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    owner.peer.close(Some(1001), Some(reason))
                }));
            }
        }
        self.inner.broker.shutdown(Some(error.unwrap_or_else(|| {
            SessionBrokerStateError::message("The session broker daemon shut down.")
        })));
        {
            let mut state = self
                .inner
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            state.authentication.clear();
            state.owners.clear();
            state.reconnects.clear();
        }
        if let Some(authenticator) = &self.inner.caller_authenticator {
            authenticator.clear_authentication();
        }
        let (lock, ready) = &self.inner.stopped;
        *lock.lock().unwrap_or_else(|error| error.into_inner()) = true;
        ready.notify_all();
    }

    pub fn handle_connection_message(&self, peer: SharedSessionBrokerDaemonPeer, message: Value) {
        if self.inner.shutting_down.load(Ordering::Acquire) {
            peer.close(Some(1001), Some("Session broker shutting down."));
            return;
        }
        if message.as_str().is_some_and(|message| {
            u64::try_from(message.len()).unwrap_or(u64::MAX)
                > self.inner.limits.max_ws_message_bytes
        }) {
            peer.close(
                Some(1009),
                Some("Session broker message exceeded its limit."),
            );
            return;
        }
        if self.inner.producer_endpoint.is_some() {
            let peer_id = peer_id(&peer);
            let authenticated = self
                .inner
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .authentication
                .get(&peer_id)
                .and_then(|state| match state {
                    ProducerAuthenticationState::Authenticated { authority, .. } => {
                        Some(Arc::clone(authority))
                    }
                    ProducerAuthenticationState::Challenged(_) => None,
                });
            if let Some(authority) = authenticated {
                if !producer_authority_is_active(&authority) {
                    peer.close(
                        Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                        Some("Session producer authority expired."),
                    );
                    return;
                }
                self.handle_authenticated_connection_message(peer, message);
            } else {
                self.handle_producer_hello_message(peer, message);
            }
            return;
        }
        self.handle_authenticated_connection_message(peer, message);
    }

    pub fn handle_connection_close(&self, peer: &SharedSessionBrokerDaemonPeer) {
        let id = peer_id(peer);
        let (session_id, _principal, socket) = {
            let mut state = self
                .inner
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let auth = state.authentication.remove(&id);
            let socket = state.sockets.remove(&id);
            let tuple = match auth {
                Some(ProducerAuthenticationState::Authenticated {
                    authority,
                    session_id,
                    broker_socket,
                }) => {
                    let active = producer_authority_is_active(&authority);
                    (
                        session_id,
                        Some(authority.ack.principal.clone()),
                        active,
                        Some(broker_socket),
                    )
                }
                _ => (None, None, false, socket),
            };
            if let (Some(session_id), Some(principal), true) = (&tuple.0, &tuple.1, tuple.2)
                && state
                    .owners
                    .get(session_id)
                    .is_some_and(|owner| peer_id(&owner.peer) == id)
            {
                state.owners.remove(session_id);
                let now = now_ms();
                let ttl = self.inner.stale_session_ttl_ms;
                state
                    .reconnects
                    .retain(|_, entry| now.saturating_sub(entry.disconnected_at) < ttl);
                if state.reconnects.len() >= self.inner.limits.max_sessions as usize
                    && let Some(oldest) = state.reconnects.keys().next().cloned()
                {
                    state.reconnects.remove(&oldest);
                }
                state.reconnects.insert(
                    session_id.clone(),
                    ProducerReconnect {
                        principal: principal.clone(),
                        disconnected_at: now,
                    },
                );
            }
            (tuple.0, tuple.1, tuple.3)
        };
        if let Some(socket) = socket {
            self.inner.broker.unregister_connection(&socket);
        }
        if self.inner.producer_endpoint.is_none() || session_id.is_some() {
            self.note_activity();
        }
    }

    pub fn handle_request(
        &self,
        request: &SessionBrokerHttpRequest,
    ) -> Option<SessionBrokerHttpResponse> {
        let path = Url::parse(&request.url).ok()?.path().to_owned();
        if self.inner.admin.is_some()
            && let Some(response) = self.handle_admin_request(request, &path)
        {
            return Some(response);
        }
        if matches!(
            path.as_str(),
            "/session-auth/challenge" | "/session-auth/proof"
        ) {
            if request.method != "POST"
                || !has_json_content_type(request)
                || self.inner.hello_authenticator.is_none()
            {
                return Some(json_error(
                    "Session broker authentication requires an upgraded client.",
                    401,
                ));
            }
            return Some(self.handle_bounded_control(
                request,
                SessionBrokerBoundedControlOptions::default(),
                |body| {
                    let authenticator = self
                        .inner
                        .hello_authenticator
                        .as_ref()
                        .expect("route requires authenticator");
                    let result = parse_session_broker_json_bytes(body)
                        .map_err(|_| auth_required_error())
                        .and_then(|value| {
                            if path.ends_with("/challenge") {
                                authenticator
                                    .issue_hello_challenge(value, &request.url)
                                    .and_then(|challenge| {
                                        serde_json::to_value(challenge)
                                            .map_err(|_| auth_required_error())
                                    })
                            } else {
                                authenticator.complete_caller_hello_proof(value).and_then(
                                    |session| {
                                        serde_json::to_value(session)
                                            .map_err(|_| auth_required_error())
                                    },
                                )
                            }
                        });
                    match result {
                        Ok(value) => SessionBrokerHttpResponse::json(200, &value),
                        Err(error) => SessionBrokerHttpResponse::json(
                            401,
                            &json!({"error": error.code.as_str()}),
                        ),
                    }
                },
            ));
        }
        if path == self.inner.paths.health {
            if self
                .inner
                .broker
                .prune_stale_sessions(self.inner.stale_session_ttl_ms, None)
                > 0
            {
                self.note_activity();
            }
            self.reconcile_producer_owners();
            return Some(SessionBrokerHttpResponse::json(200, &json!({"ok": true})));
        }
        if self.inner.paths.capabilities.as_deref() == Some(path.as_str()) {
            return Some(self.handle_capabilities_request(request));
        }
        if self.inner.paths.api.as_deref() == Some(path.as_str()) {
            self.note_activity();
            return Some(self.handle_api_request(request));
        }
        None
    }

    pub fn handle_bounded_control<F>(
        &self,
        request: &SessionBrokerHttpRequest,
        options: SessionBrokerBoundedControlOptions,
        handler: F,
    ) -> SessionBrokerHttpResponse
    where
        F: FnOnce(&[u8]) -> SessionBrokerHttpResponse,
    {
        let Ok(_control) = self.inner.http_control_budget.reserve(1) else {
            return capacity_response("busy", "maxConcurrentHttpControls", 503);
        };
        self.handle_bounded_body(request, options, handler)
    }

    fn handle_bounded_body<F>(
        &self,
        request: &SessionBrokerHttpRequest,
        options: SessionBrokerBoundedControlOptions,
        handler: F,
    ) -> SessionBrokerHttpResponse
    where
        F: FnOnce(&[u8]) -> SessionBrokerHttpResponse,
    {
        let max = options
            .max_body_bytes
            .unwrap_or(self.inner.limits.max_http_body_bytes)
            .min(self.inner.limits.max_http_body_bytes);
        if invalid_content_length(request) {
            return json_error("Invalid Content-Length.", 400);
        }
        if u64::try_from(request.body.len()).unwrap_or(u64::MAX) > max {
            if let Some(response) = options.payload_too_large {
                return response();
            }
            return capacity_response("capacity-exceeded", "maxHttpBodyBytes", 413);
        }
        let Ok(_body) = self
            .inner
            .http_body_budget
            .reserve(request.body.len() as u64)
        else {
            return capacity_response("capacity-exceeded", "maxInFlightHttpBodyBytes", 503);
        };
        handler(&request.body)
    }

    pub fn handle_authenticated_control<Resolve, Handle>(
        &self,
        request: &SessionBrokerHttpRequest,
        control_options: SessionBrokerAuthenticatedControlOptions,
        resolve: Resolve,
        handle: Handle,
    ) -> SessionBrokerHttpResponse
    where
        Resolve: FnOnce(&[u8]) -> Result<SessionBrokerAuthenticatedControlFacts, ()>,
        Handle: FnOnce(
            &[u8],
            &SessionBrokerAuthenticatedControlFacts,
        ) -> SessionBrokerAuthenticatedControlResult,
    {
        self.handle_bounded_control(
            request,
            SessionBrokerBoundedControlOptions::default(),
            |body| {
                let authenticated = match self.authenticate_request(
                    request,
                    body,
                    control_options
                        .authentication_failure_operation
                        .map(SessionBrokerAuditOperation::Caller)
                        .unwrap_or(SessionBrokerAuditOperation::Unknown),
                ) {
                    Ok(authenticated) => authenticated,
                    Err(response) => return response,
                };
                let facts = catch_unwind(AssertUnwindSafe(|| resolve(body)));
                let Ok(Ok(facts)) = facts else {
                    let target_specific = control_options
                        .resolve_failure_target_specific
                        .as_ref()
                        .and_then(|resolve| catch_unwind(AssertUnwindSafe(|| resolve(body))).ok())
                        .unwrap_or(false);
                    return self.authenticated_response(
                        &authenticated,
                        json!({"error": "protocol-validation-failed"}),
                        400,
                        target_specific,
                    );
                };
                if !self.authorize(request, &authenticated, &facts) {
                    return self.authenticated_response(
                        &authenticated,
                        json!({"error": "authorization-denied"}),
                        403,
                        facts
                            .target_specific
                            .unwrap_or(facts.operation != CallerOperation::List),
                    );
                }
                if let Err(response) = self.reject_inactive_request(&authenticated) {
                    return response;
                }
                let Ok(result) = catch_unwind(AssertUnwindSafe(|| handle(body, &facts))) else {
                    return self.authenticated_response(
                        &authenticated,
                        json!({"error": "session-control-failed"}),
                        400,
                        facts
                            .target_specific
                            .unwrap_or(facts.operation != CallerOperation::List),
                    );
                };
                self.authenticated_response(
                    &authenticated,
                    result.body,
                    result.status,
                    facts
                        .target_specific
                        .unwrap_or(facts.operation != CallerOperation::List),
                )
            },
        )
    }

    fn handle_producer_hello_message(&self, peer: SharedSessionBrokerDaemonPeer, message: Value) {
        let Ok(value) = parse_text_json(&message) else {
            self.reject_producer_hello(&peer);
            return;
        };
        let id = peer_id(&peer);
        let current = self
            .inner
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .authentication
            .get(&id)
            .and_then(|state| match state {
                ProducerAuthenticationState::Challenged(token) => Some(*token),
                ProducerAuthenticationState::Authenticated { .. } => None,
            });
        if current.is_none() {
            let Ok(payload) = exact_hello_envelope(&value, "hello-init", "hello") else {
                self.reject_producer_hello(&peer);
                return;
            };
            let token = self.inner.next_connection_id.fetch_add(1, Ordering::AcqRel);
            self.inner
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .authentication
                .insert(id, ProducerAuthenticationState::Challenged(token));
            let daemon = Self {
                inner: Arc::clone(&self.inner),
            };
            thread::spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| {
                    daemon
                        .inner
                        .hello_authenticator
                        .as_ref()
                        .expect("producer endpoint requires authenticator")
                        .issue_hello_challenge(
                            payload,
                            daemon
                                .inner
                                .producer_endpoint
                                .as_deref()
                                .expect("producer endpoint configured"),
                        )
                }))
                .unwrap_or_else(|_| Err(auth_required_error()));
                if daemon.inner.shutting_down.load(Ordering::Acquire) {
                    return;
                }
                let still_current = daemon
                    .inner
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .authentication
                    .get(&id)
                    .is_some_and(|state| {
                        matches!(state, ProducerAuthenticationState::Challenged(value) if *value == token)
                    });
                if !still_current {
                    return;
                }
                match result.and_then(|challenge| {
                    serde_json::to_string(&json!({
                        "type": "hello-challenge",
                        "challenge": challenge,
                    }))
                    .map_err(|_| auth_required_error())
                }) {
                    Ok(encoded)
                        if catch_unwind(AssertUnwindSafe(|| peer.send(&encoded)))
                            .is_ok_and(|result| result.is_ok()) => {}
                    _ => daemon.reject_producer_hello(&peer),
                }
            });
            return;
        }
        let token = current.expect("challenged state checked");
        let Ok(payload) = exact_hello_envelope(&value, "hello-proof", "proof") else {
            self.reject_producer_hello(&peer);
            return;
        };
        let daemon = Self {
            inner: Arc::clone(&self.inner),
        };
        thread::spawn(move || {
            let connection_id = format!(
                "b_{:032x}_0",
                daemon
                    .inner
                    .next_connection_id
                    .fetch_add(1, Ordering::AcqRel)
            );
            let result = catch_unwind(AssertUnwindSafe(|| {
                daemon
                    .inner
                    .hello_authenticator
                    .as_ref()
                    .expect("producer endpoint requires authenticator")
                    .complete_producer_hello_proof(payload, &connection_id)
            }))
            .unwrap_or_else(|_| Err(auth_required_error()));
            if daemon.inner.shutting_down.load(Ordering::Acquire) {
                return;
            }
            let Ok(authority) = result else {
                daemon.reject_producer_hello(&peer);
                return;
            };
            if !producer_authority_is_active(&authority) {
                daemon.reject_producer_hello(&peer);
                return;
            }
            let authority = Arc::new(authority);
            let broker_socket: SharedDaemonSessionSocket = Arc::new(DaemonPeerSocket {
                peer: Arc::clone(&peer),
                authority: Some(Arc::clone(&authority)),
            });
            {
                let mut state = daemon
                    .inner
                    .producers
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if !state.authentication.get(&id).is_some_and(|state| {
                    matches!(state, ProducerAuthenticationState::Challenged(value) if *value == token)
                }) {
                    return;
                }
                state.sockets.insert(id, Arc::clone(&broker_socket));
                state.authentication.insert(
                    id,
                    ProducerAuthenticationState::Authenticated {
                        authority: Arc::clone(&authority),
                        broker_socket,
                        session_id: None,
                    },
                );
            }
            let encoded =
                serde_json::to_string(&json!({"type": "hello-ack", "ack": authority.ack}));
            if encoded.is_err()
                || catch_unwind(AssertUnwindSafe(|| peer.send(&encoded.unwrap_or_default())))
                    .map_or(true, |result| result.is_err())
            {
                daemon.reject_producer_hello(&peer);
            }
        });
    }

    fn reject_producer_hello(&self, peer: &SharedSessionBrokerDaemonPeer) {
        let id = peer_id(peer);
        self.inner
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .authentication
            .remove(&id);
        peer.close(
            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
            Some("Session broker authentication required; upgrade Workdeck."),
        );
    }

    fn handle_authenticated_connection_message(
        &self,
        peer: SharedSessionBrokerDaemonPeer,
        message: Value,
    ) {
        let parsed = parse_text_json(&message).and_then(|value| {
            self.inner
                .broker
                .protocol_parsers()
                .parse_client_message(&value)
        });
        let Ok(parsed) = parsed else {
            peer.close(
                Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                Some("Malformed session broker protocol."),
            );
            return;
        };
        let id = peer_id(&peer);
        let socket = self.socket_for(&peer);
        match parsed {
            StructuralSessionClientMessage::Register {
                registration,
                snapshot,
            } => {
                let Some(session_id) = registration
                    .as_object()
                    .and_then(|record| record.get("sessionId"))
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                else {
                    peer.close(
                        Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                        Some("Incompatible session registration."),
                    );
                    return;
                };
                self.prune_producer_reconnects(now_ms());
                let (principal, current_session, displaced, reconnect, app_revision) = {
                    let state = self
                        .inner
                        .producers
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    let (principal, current_session, app_revision) = state
                        .authentication
                        .get(&id)
                        .and_then(|auth| match auth {
                            ProducerAuthenticationState::Authenticated {
                                authority,
                                session_id,
                                ..
                            } => Some((
                                authority.ack.principal.clone(),
                                session_id.clone(),
                                Some(u64::from(authority.ack.app_revision)),
                            )),
                            ProducerAuthenticationState::Challenged(_) => None,
                        })
                        .map_or((None, None, None), |(principal, session, revision)| {
                            (Some(principal), session, revision)
                        });
                    let displaced = state
                        .owners
                        .get(&session_id)
                        .filter(|owner| peer_id(&owner.peer) != id)
                        .cloned();
                    let reconnect = displaced
                        .as_ref()
                        .map(|owner| owner.principal.clone())
                        .or_else(|| {
                            state
                                .reconnects
                                .get(&session_id)
                                .map(|entry| entry.principal.clone())
                        });
                    (
                        principal,
                        current_session,
                        displaced,
                        reconnect,
                        app_revision,
                    )
                };
                if let Some(principal) = &principal {
                    let operation = if reconnect.is_some() {
                        ProducerOperation::Reconnect
                    } else {
                        ProducerOperation::Register
                    };
                    if current_session
                        .as_deref()
                        .is_some_and(|current| current != session_id)
                        || reconnect
                            .as_ref()
                            .is_some_and(|prior| !same_producer_binding(principal, prior))
                        || !producer_principal_allows(
                            principal,
                            &self.inner.app_id,
                            operation,
                            Some(&session_id),
                        )
                    {
                        peer.close(
                            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                            Some("Session producer scope rejected."),
                        );
                        return;
                    }
                }
                let result = self.inner.broker.register_session(
                    Arc::clone(&socket),
                    &registration,
                    &snapshot,
                    RegisterSessionOptions {
                        replace_owner: displaced.is_some(),
                    },
                );
                match result {
                    RegisterSessionResult::Invalid => {
                        peer.close(
                            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                            Some("Incompatible session registration."),
                        );
                        return;
                    }
                    RegisterSessionResult::AlreadyConnected => {
                        peer.close(
                            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                            Some("Session registration rejected."),
                        );
                        return;
                    }
                    RegisterSessionResult::CapacityExceeded => {
                        peer.close(Some(1013), Some("Session broker capacity exceeded."));
                        return;
                    }
                    RegisterSessionResult::Shutdown => {
                        peer.close(Some(1001), Some("Session broker shutting down."));
                        return;
                    }
                    RegisterSessionResult::Registered => {}
                }
                if let Some(principal) = principal {
                    let mut state = self
                        .inner
                        .producers
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    if let Some(displaced) = &displaced {
                        state.authentication.remove(&peer_id(&displaced.peer));
                    }
                    if let Some(ProducerAuthenticationState::Authenticated {
                        session_id: current,
                        ..
                    }) = state.authentication.get_mut(&id)
                    {
                        *current = Some(session_id.clone());
                    }
                    state.owners.insert(
                        session_id.clone(),
                        ProducerOwner {
                            peer: Arc::clone(&peer),
                            broker_socket: Arc::clone(&socket),
                            principal,
                            app_revision,
                        },
                    );
                    state.reconnects.remove(&session_id);
                }
                peer.mark_authenticated();
                if let Some(displaced) = displaced {
                    displaced
                        .peer
                        .close(Some(1000), Some("Session owner reconnected."));
                }
                self.note_activity();
            }
            StructuralSessionClientMessage::Snapshot {
                session_id,
                snapshot,
            } => {
                if self.producer_session_mismatch(id, &session_id) {
                    peer.close(
                        Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                        Some("Session producer scope rejected."),
                    );
                    return;
                }
                match self
                    .inner
                    .broker
                    .update_snapshot(&socket, &session_id, &snapshot)
                {
                    UpdateSnapshotResult::NotOwner => {
                        peer.close(
                            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                            Some("Session ownership rejected."),
                        );
                        return;
                    }
                    UpdateSnapshotResult::Invalid => {
                        peer.close(
                            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                            Some("Incompatible session snapshot."),
                        );
                        return;
                    }
                    UpdateSnapshotResult::CapacityExceeded => {
                        peer.close(Some(1013), Some("Session broker capacity exceeded."));
                        return;
                    }
                    UpdateSnapshotResult::Updated => {}
                }
                self.note_activity();
            }
            StructuralSessionClientMessage::Heartbeat { session_id } => {
                if self.producer_session_mismatch(id, &session_id) {
                    peer.close(
                        Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                        Some("Session producer scope rejected."),
                    );
                    return;
                }
                if self.inner.broker.mark_session_seen(&socket, &session_id)
                    == MarkSessionSeenResult::NotOwner
                {
                    peer.close(
                        Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                        Some("Session ownership rejected."),
                    );
                    return;
                }
                self.note_activity();
            }
            StructuralSessionClientMessage::CommandResult {
                request_id,
                outcome,
            } => {
                let outcome = match outcome {
                    StructuralCommandOutcome::Success { result } => {
                        BrokerCommandOutcome::Success(result)
                    }
                    StructuralCommandOutcome::Failure { error } => {
                        BrokerCommandOutcome::Failure(error)
                    }
                };
                match self
                    .inner
                    .broker
                    .handle_command_result(&socket, &request_id, outcome)
                {
                    HandleCommandResult::NotOwner => {
                        peer.close(
                            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                            Some("Command ownership rejected."),
                        );
                    }
                    HandleCommandResult::Invalid => {
                        peer.close(
                            Some(INCOMPATIBLE_PAYLOAD_CLOSE_CODE),
                            Some("Malformed command result."),
                        );
                    }
                    HandleCommandResult::Handled => self.note_activity(),
                    HandleCommandResult::NotFound => {}
                }
            }
        }
    }

    fn socket_for(&self, peer: &SharedSessionBrokerDaemonPeer) -> SharedDaemonSessionSocket {
        let id = peer_id(peer);
        let mut state = self
            .inner
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(socket) = state.sockets.get(&id) {
            return Arc::clone(socket);
        }
        let socket: SharedDaemonSessionSocket = Arc::new(DaemonPeerSocket {
            peer: Arc::clone(peer),
            authority: None,
        });
        state.sockets.insert(id, Arc::clone(&socket));
        socket
    }

    fn producer_session_mismatch(&self, id: usize, session_id: &str) -> bool {
        if self.inner.producer_endpoint.is_none() {
            return false;
        }
        self.inner
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .authentication
            .get(&id)
            .and_then(|state| match state {
                ProducerAuthenticationState::Authenticated { session_id, .. } => {
                    session_id.as_deref()
                }
                ProducerAuthenticationState::Challenged(_) => None,
            })
            != Some(session_id)
    }

    fn handle_capabilities_request(
        &self,
        request: &SessionBrokerHttpRequest,
    ) -> SessionBrokerHttpResponse {
        let Ok(_control) = self.inner.http_control_budget.reserve(1) else {
            return capacity_response("busy", "maxConcurrentHttpControls", 503);
        };
        if bodyless_transport_error(request) {
            return json_error(
                "Broker capabilities requests must not include a transport body.",
                400,
            );
        }
        if request.method != "GET" {
            return json_error("Broker capabilities requests must use GET.", 405);
        }
        if !request.body.is_empty() {
            return json_error("Broker capabilities requests must not include a body.", 400);
        }
        let authenticated = match self.authenticate_request(
            request,
            &request.body,
            SessionBrokerAuditOperation::Caller(CallerOperation::Diagnostics),
        ) {
            Ok(value) => value,
            Err(response) => return response,
        };
        let facts = SessionBrokerAuthenticatedControlFacts {
            operation: CallerOperation::Diagnostics,
            session_id: None,
            command: None,
            command_version: None,
            target_specific: Some(false),
        };
        if !self.authorize(request, &authenticated, &facts) {
            return self.authenticated_response(
                &authenticated,
                json!({"error": "authorization-denied"}),
                403,
                false,
            );
        }
        if let Err(response) = self.reject_inactive_request(&authenticated) {
            return response;
        }
        self.note_activity();
        self.authenticated_response(
            &authenticated,
            serde_json::to_value(&self.inner.capabilities).unwrap_or(Value::Null),
            200,
            false,
        )
    }

    fn handle_api_request(&self, request: &SessionBrokerHttpRequest) -> SessionBrokerHttpResponse {
        let Ok(_control) = self.inner.http_control_budget.reserve(1) else {
            return capacity_response("busy", "maxConcurrentHttpControls", 503);
        };
        if request.method != "POST" {
            return json_error("Broker API requests must use POST.", 405);
        }
        if !has_json_content_type(request) {
            return json_error("Expected Content-Type application/json.", 415);
        }
        self.handle_bounded_body(
            request,
            SessionBrokerBoundedControlOptions::default(),
            |body| {
                let authenticated = match self.authenticate_request(
                    request,
                    body,
                    SessionBrokerAuditOperation::Unknown,
                ) {
                    Ok(value) => value,
                    Err(response) => return response,
                };
                let input = match parse_session_broker_json_bytes(body).and_then(|value| {
                    self.inner
                        .broker
                        .protocol_parsers()
                        .parse_daemon_request(&value)
                }) {
                    Ok(input) => input,
                    Err(error) => {
                        return self.authenticated_response(
                            &authenticated,
                            protocol_error(Some(error)),
                            400,
                            false,
                        );
                    }
                };
                let (operation, selector, command, command_version, target_specific) = match &input
                {
                    StructuralSessionBrokerDaemonRequest::List => {
                        (CallerOperation::List, None, None, None, false)
                    }
                    StructuralSessionBrokerDaemonRequest::Get { selector } => {
                        (CallerOperation::Get, Some(selector), None, None, true)
                    }
                    StructuralSessionBrokerDaemonRequest::Dispatch {
                        selector,
                        command,
                        command_version,
                        ..
                    } => (
                        CallerOperation::Dispatch,
                        Some(selector),
                        Some(command.as_str()),
                        Some(*command_version),
                        true,
                    ),
                };
                let session_id = match selector {
                    Some(selector) => match self.inner.broker.resolve_session_id(selector) {
                        Ok(value) => Some(value),
                        Err(error) => {
                            return self.authenticated_response(
                                &authenticated,
                                json!({"error": error.to_string()}),
                                400,
                                true,
                            );
                        }
                    },
                    None => None,
                };
                let facts = SessionBrokerAuthenticatedControlFacts {
                    operation,
                    session_id: session_id.clone(),
                    command: command.map(str::to_owned),
                    command_version,
                    target_specific: Some(target_specific),
                };
                if !self.authorize(request, &authenticated, &facts) {
                    return self.authenticated_response(
                        &authenticated,
                        json!({"error": "authorization-denied"}),
                        403,
                        target_specific,
                    );
                }
                if let Err(response) = self.reject_inactive_request(&authenticated) {
                    return response;
                }
                let response = match input {
                    StructuralSessionBrokerDaemonRequest::List => {
                        serde_json::to_value(json!({"sessions": self.inner.broker.list_sessions()}))
                            .map_err(|error| SessionBrokerStateError::message(error.to_string()))
                    }
                    StructuralSessionBrokerDaemonRequest::Get { .. } => self
                        .inner
                        .broker
                        .get_session(&crate::SessionSelector {
                            session_id,
                            session_path: None,
                            repo_root: None,
                            repo_boundary: None,
                        })
                        .and_then(|session| {
                            serde_json::to_value(json!({"session": session})).map_err(|error| {
                                SessionBrokerStateError::message(error.to_string())
                            })
                        }),
                    StructuralSessionBrokerDaemonRequest::Dispatch {
                        command,
                        command_version,
                        input,
                        timeout_ms,
                        timeout_message,
                        ..
                    } => self
                        .inner
                        .broker
                        .dispatch_command(crate::DispatchSessionCommand {
                            selector: crate::SessionSelector {
                                session_id,
                                session_path: None,
                                repo_root: None,
                                repo_boundary: None,
                            },
                            command: command.clone(),
                            command_version,
                            input,
                            timeout_message: timeout_message.unwrap_or_else(|| {
                                format!("Timed out waiting for the session to handle {command}.")
                            }),
                            timeout_ms,
                        })
                        .and_then(|pending| pending.receive())
                        .and_then(|result| {
                            serde_json::to_value(json!({"result": result})).map_err(|error| {
                                SessionBrokerStateError::message(error.to_string())
                            })
                        }),
                };
                match response {
                    Ok(body) => {
                        self.authenticated_response(&authenticated, body, 200, target_specific)
                    }
                    Err(error) => {
                        let (body, status) = if let Some(capacity) = error.capacity {
                            (
                                json!({
                                    "error": capacity.code.as_str(),
                                    "resource": capacity.resource,
                                }),
                                503,
                            )
                        } else if let Some(protocol) = error.protocol {
                            (protocol_error(Some(protocol)), 400)
                        } else {
                            (json!({"error": error.to_string()}), 400)
                        };
                        self.authenticated_response(&authenticated, body, status, target_specific)
                    }
                }
            },
        )
    }

    /// Route the admin hello and control paths; `None` for every other path.
    fn handle_admin_request(
        &self,
        request: &SessionBrokerHttpRequest,
        pathname: &str,
    ) -> Option<SessionBrokerHttpResponse> {
        let admin = self.inner.admin.as_ref()?;
        if pathname == admin.paths.challenge || pathname == admin.paths.proof {
            if request.method != "POST" || !has_json_content_type(request) {
                return Some(json_error(
                    "Session broker admin authentication requires a JSON POST.",
                    401,
                ));
            }
            let authenticator = Arc::clone(&admin.authenticator);
            let path = pathname.to_owned();
            return Some(self.handle_bounded_control(
                request,
                SessionBrokerBoundedControlOptions::default(),
                |body| {
                    let result = parse_session_broker_json_bytes(body)
                        .map_err(|_| auth_required_error())
                        .and_then(|value| {
                            if path.ends_with("/challenge") {
                                authenticator
                                    .issue_hello_challenge(value, &request.url)
                                    .and_then(|challenge| {
                                        serde_json::to_value(challenge)
                                            .map_err(|_| auth_required_error())
                                    })
                            } else {
                                authenticator.complete_caller_hello_proof(value).and_then(
                                    |session| {
                                        serde_json::to_value(session)
                                            .map_err(|_| auth_required_error())
                                    },
                                )
                            }
                        });
                    match result {
                        Ok(value) => SessionBrokerHttpResponse::json(200, &value),
                        Err(error) => SessionBrokerHttpResponse::json(
                            401,
                            &json!({"error": error.code.as_str()}),
                        ),
                    }
                },
            ));
        }
        if pathname != admin.paths.control {
            return None;
        }
        if request.method != "POST" {
            return Some(json_error("Admin requests must use POST.", 405));
        }
        if !has_json_content_type(request) {
            return Some(json_error("Expected Content-Type application/json.", 415));
        }
        Some(self.handle_bounded_control(
            request,
            SessionBrokerBoundedControlOptions::default(),
            |body| {
                let authenticated = match self.authenticate_request_with(
                    request,
                    body,
                    SessionBrokerAuditOperation::Caller(CallerOperation::Diagnostics),
                    Some(Arc::clone(&admin.authenticator) as Arc<dyn CallerRequestAuthenticator>),
                ) {
                    Ok(authenticated) => authenticated,
                    Err(response) => return response,
                };
                let input = match parse_session_broker_json_bytes(body)
                    .and_then(|value| crate::parse_session_broker_admin_request(&value))
                {
                    Ok(input) => input,
                    Err(error) => {
                        return self.authenticated_response(
                            &authenticated,
                            protocol_error(Some(error)),
                            400,
                            false,
                        );
                    }
                };
                // Both actions are authorized as diagnostics: the scope exists so an
                // operator's existing caller credential can inspect and retire a daemon it
                // cannot otherwise talk to.
                let facts = SessionBrokerAuthenticatedControlFacts {
                    operation: CallerOperation::Diagnostics,
                    session_id: None,
                    command: None,
                    command_version: None,
                    target_specific: Some(false),
                };
                if !self.authorize(request, &authenticated, &facts) {
                    return self.authenticated_response(
                        &authenticated,
                        json!({"error": "authorization-denied"}),
                        403,
                        false,
                    );
                }
                if let Err(response) = self.reject_inactive_request(&authenticated) {
                    return response;
                }
                // Deliberately not activity. `status` is a read-only diagnostic, and the client
                // that needs it most is a newer window waiting for an incompatible incumbent to
                // go quiescent. Counting it would keep that incumbent alive for as long as the
                // window keeps asking, which is the opposite of what the window is waiting for.
                // `stop` shuts the daemon down anyway.
                match input {
                    crate::SessionBrokerAdminRequest::Status => self.authenticated_response(
                        &authenticated,
                        serde_json::to_value(self.admin_status()).unwrap_or(Value::Null),
                        200,
                        false,
                    ),
                    crate::SessionBrokerAdminRequest::Stop => {
                        let response = self.authenticated_response(
                            &authenticated,
                            serde_json::to_value(crate::SessionBrokerAdminStopResultV1 {
                                admin_scope_version: crate::SESSION_BROKER_ADMIN_SCOPE_VERSION,
                                stopping: true,
                            })
                            .unwrap_or(Value::Null),
                            200,
                            false,
                        );
                        // Let the signed acknowledgement leave before producers are closed and
                        // the listener stops.
                        let daemon = Self {
                            inner: Arc::clone(&self.inner),
                        };
                        thread::spawn(move || {
                            thread::sleep(Duration::from_millis(50));
                            daemon.shutdown_with_options(
                                Some(SessionBrokerStateError::message(
                                    "The session broker daemon is restarting.",
                                )),
                                Some(crate::SESSION_BROKER_ADMIN_STOP_CLOSE_REASON),
                            );
                        });
                        response
                    }
                }
            },
        ))
    }

    /// Build the frozen v1 admin status body from daemon facts and the app's session view.
    fn admin_status(&self) -> crate::SessionBrokerAdminStatusV1 {
        let admin = self
            .inner
            .admin
            .as_ref()
            .expect("admin status is only built when the scope is configured");
        let sessions = self.inner.broker.list_sessions();
        let owner_revisions = {
            let producers = self
                .inner
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            producers
                .owners
                .iter()
                .map(|(session_id, owner)| (session_id.clone(), owner.app_revision))
                .collect::<BTreeMap<String, Option<u64>>>()
        };
        let entries = sessions
            .into_iter()
            .map(|session| {
                let described = (admin.describe_session)(&session);
                // A session that registered necessarily matched the daemon's revision in its
                // hello; the recorded value is preferred, and the daemon's own revision stands
                // in for a retained session whose transport has since disconnected.
                let client_daemon_version = owner_revisions
                    .get(&described.session_id)
                    .copied()
                    .flatten()
                    .unwrap_or(self.inner.app_revision);
                crate::SessionBrokerAdminSessionV1 {
                    session_id: described.session_id,
                    title: described.title,
                    cwd: described.cwd,
                    pid: described.pid,
                    client_daemon_version,
                }
            })
            .collect::<Vec<_>>();
        let uptime_ms = now_ms().saturating_sub(self.inner.started_at_ms);
        crate::SessionBrokerAdminStatusV1 {
            admin_scope_version: crate::SESSION_BROKER_ADMIN_SCOPE_VERSION,
            daemon_version: self.inner.app_revision,
            app_version: admin.app_version.clone(),
            pid: u64::from(std::process::id()),
            started_at: chrono::DateTime::from_timestamp_millis(
                i64::try_from(self.inner.started_at_ms).unwrap_or(i64::MAX),
            )
            .unwrap_or_default()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            uptime_ms,
            sessions: entries,
        }
    }

    fn authenticate_request(
        &self,
        request: &SessionBrokerHttpRequest,
        body: &[u8],
        operation: SessionBrokerAuditOperation,
    ) -> Result<AuthenticatedCallerRequest, SessionBrokerHttpResponse> {
        self.authenticate_request_with(request, body, operation, None)
    }

    /// Authenticate one control request, optionally against the admin scope's authenticator.
    fn authenticate_request_with(
        &self,
        request: &SessionBrokerHttpRequest,
        body: &[u8],
        operation: SessionBrokerAuditOperation,
        authenticator: Option<Arc<dyn CallerRequestAuthenticator>>,
    ) -> Result<AuthenticatedCallerRequest, SessionBrokerHttpResponse> {
        let request_id = request
            .header("x-session-broker-request-id")
            .map(str::to_owned);
        let Some(authenticator) = authenticator.or_else(|| self.inner.caller_authenticator.clone())
        else {
            return Err(json_error("Broker control is unavailable.", 404));
        };
        if self.inner.authorizer.is_none() {
            return Err(json_error("Broker control is unavailable.", 404));
        }
        let authentication = catch_unwind(AssertUnwindSafe(|| {
            authenticator.authenticate_request(&CallerRequestAuthenticationInput {
                method: request.method.clone(),
                url: request.url.clone(),
                headers: request.headers.clone(),
                body: body.to_vec(),
            })
        }))
        .unwrap_or_else(|_| Err(auth_required_error()));
        match authentication {
            Ok(authenticated) if crate::is_valid_broker_identifier(&authenticated.request_id) => {
                Ok(authenticated)
            }
            Ok(_) => Err(self.authentication_failure(
                operation,
                request_id,
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            )),
            Err(error) => Err(self.authentication_failure(operation, request_id, error.code)),
        }
    }

    fn authentication_failure(
        &self,
        operation: SessionBrokerAuditOperation,
        request_id: Option<String>,
        code: SessionBrokerAuthenticationFailureCode,
    ) -> SessionBrokerHttpResponse {
        self.emit_audit(SessionBrokerAuditEvent {
            app_id: self.inner.app_id.clone(),
            principal_id: None,
            key_id: None,
            session_id: None,
            operation,
            command: None,
            command_version: None,
            request_id,
            decision: SessionBrokerAuditDecision::Deny,
            outcome: SessionBrokerAuditOutcome::AuthenticationFailed,
            timestamp: now_ms(),
        });
        SessionBrokerHttpResponse::json(
            401,
            &json!({"error": "authentication-failed", "code": code.as_str()}),
        )
    }

    fn authorize(
        &self,
        _request: &SessionBrokerHttpRequest,
        authenticated: &AuthenticatedCallerRequest,
        facts: &SessionBrokerAuthenticatedControlFacts,
    ) -> bool {
        let grant_allows = caller_principal_allows(
            &authenticated.principal,
            &self.inner.app_id,
            facts.operation,
            facts.session_id.as_deref(),
            facts.command.as_deref().zip(facts.command_version),
        );
        let allowed = grant_allows
            && self.inner.authorizer.as_ref().is_some_and(|authorizer| {
                let context = SessionBrokerAuthorizationContext {
                    principal: authenticated.principal.clone(),
                    operation: facts.operation,
                    session_id: facts.session_id.clone(),
                    command: facts.command.clone(),
                    command_version: facts.command_version,
                    request_id: Some(authenticated.request_id.clone()),
                    cancellation: SessionBrokerCancellation::default(),
                };
                catch_unwind(AssertUnwindSafe(|| {
                    block_on(authorizer.authorize(&context))
                }))
                .unwrap_or(false)
            });
        self.emit_audit(SessionBrokerAuditEvent {
            app_id: self.inner.app_id.clone(),
            principal_id: Some(authenticated.principal.principal_id.clone()),
            key_id: Some(authenticated.principal.key_id.clone()),
            session_id: facts.session_id.clone(),
            operation: SessionBrokerAuditOperation::Caller(facts.operation),
            command: facts.command.clone(),
            command_version: facts.command_version,
            request_id: Some(authenticated.request_id.clone()),
            decision: if allowed {
                SessionBrokerAuditDecision::Allow
            } else {
                SessionBrokerAuditDecision::Deny
            },
            outcome: if allowed {
                SessionBrokerAuditOutcome::Authenticated
            } else {
                SessionBrokerAuditOutcome::AuthorizationFailed
            },
            timestamp: now_ms(),
        });
        allowed
    }

    fn reject_inactive_request(
        &self,
        authenticated: &AuthenticatedCallerRequest,
    ) -> Result<(), SessionBrokerHttpResponse> {
        catch_unwind(AssertUnwindSafe(|| authenticated.assert_active()))
            .unwrap_or_else(|_| Err(auth_required_error()))
            .map_err(|error| {
                SessionBrokerHttpResponse::json(
                    401,
                    &json!({
                        "error": "authentication-failed",
                        "code": error.code.as_str(),
                    }),
                )
            })
    }

    fn authenticated_response(
        &self,
        authenticated: &AuthenticatedCallerRequest,
        mut body: Value,
        mut status: u16,
        target_specific: bool,
    ) -> SessionBrokerHttpResponse {
        let target_contract = target_specific.then(|| SignedBrokerAppContract {
            app_revision: u32::try_from(self.inner.app_revision).unwrap_or(u32::MAX),
            features: Vec::new(),
        });
        if canonicalize_json(&body)
            .map(|encoded| encoded.len() as u64 > self.inner.limits.max_http_response_bytes)
            .unwrap_or(true)
        {
            body = json!({"error": "capacity-exceeded", "resource": "maxHttpResponseBytes"});
            status = 503;
        }
        let sign = |body: &Value, status| {
            authenticated.sign_response(&CallerResponseSigningInput {
                http_status: status,
                body: body.clone(),
                app_contract: target_contract.clone(),
            })
        };
        let Ok(mut authentication) = sign(&body, status) else {
            return SessionBrokerHttpResponse::empty(503);
        };
        let mut envelope = SessionBrokerAuthenticatedResponse {
            body: body.clone(),
            authentication: authentication.clone(),
        };
        let mut serialized = canonical_response(&envelope);
        if serialized.as_ref().map_or(true, |value| {
            value.len() as u64 > self.inner.limits.max_http_response_bytes
        }) {
            body = json!({"error": "capacity-exceeded", "resource": "maxHttpResponseBytes"});
            status = 503;
            let Ok(signed) = sign(&body, status) else {
                return SessionBrokerHttpResponse::empty(503);
            };
            authentication = signed;
            envelope = SessionBrokerAuthenticatedResponse {
                body,
                authentication,
            };
            serialized = canonical_response(&envelope);
        }
        let Ok(body) = serialized else {
            return SessionBrokerHttpResponse::empty(503);
        };
        if body.len() as u64 > self.inner.limits.max_http_response_bytes {
            return SessionBrokerHttpResponse::empty(503);
        }
        SessionBrokerHttpResponse {
            status,
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body,
        }
    }

    fn emit_audit(&self, event: SessionBrokerAuditEvent) {
        if let Some(audit) = &self.inner.audit {
            let _ = catch_unwind(AssertUnwindSafe(|| block_on(audit.record(&event))));
        }
    }

    fn note_activity(&self) {
        self.inner
            .last_activity_at
            .store(now_ms(), Ordering::Release);
    }

    fn prune_producer_reconnects(&self, now: u64) {
        let ttl = self.inner.stale_session_ttl_ms;
        self.inner
            .producers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reconnects
            .retain(|_, value| now.saturating_sub(value.disconnected_at) < ttl);
    }

    fn reconcile_producer_owners(&self) {
        let live = self.inner.broker.session_ids();
        let retired = {
            let mut state = self
                .inner
                .producers
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let session_ids = state.owners.keys().cloned().collect::<Vec<_>>();
            let mut retired = Vec::new();
            for session_id in session_ids {
                let Some(owner) = state.owners.get(&session_id).cloned() else {
                    continue;
                };
                let session_live = live.contains(&session_id);
                let authority_active =
                    state
                        .authentication
                        .get(&peer_id(&owner.peer))
                        .is_some_and(|authentication| match authentication {
                            ProducerAuthenticationState::Authenticated { authority, .. } => {
                                producer_authority_is_active(authority)
                            }
                            ProducerAuthenticationState::Challenged(_) => false,
                        });
                if session_live && authority_active {
                    continue;
                }
                state.owners.remove(&session_id);
                state.authentication.remove(&peer_id(&owner.peer));
                if !session_live && authority_active {
                    state.reconnects.insert(
                        session_id.clone(),
                        ProducerReconnect {
                            principal: owner.principal.clone(),
                            disconnected_at: now_ms(),
                        },
                    );
                }
                retired.push(owner);
            }
            retired
        };
        for owner in retired {
            self.inner
                .broker
                .unregister_connection(&owner.broker_socket);
            owner
                .peer
                .close(Some(1000), Some("Session producer authority retired."));
        }
    }
}

fn start_lifecycle<Info, State, CommandInput, CommandResult, Controller>(
    inner: &Arc<DaemonInner<Info, State, CommandInput, CommandResult, Controller>>,
) where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    let weak = Arc::downgrade(inner);
    thread::spawn(move || lifecycle_loop(weak));
}

fn lifecycle_loop<Info, State, CommandInput, CommandResult, Controller>(
    weak: Weak<DaemonInner<Info, State, CommandInput, CommandResult, Controller>>,
) where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    let mut last_sweep = now_ms();
    loop {
        thread::sleep(Duration::from_millis(2));
        let Some(inner) = weak.upgrade() else {
            return;
        };
        if inner.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let now = now_ms();
        if now.saturating_sub(last_sweep) >= inner.stale_session_sweep_interval_ms {
            last_sweep = now;
            if inner
                .broker
                .prune_stale_sessions(inner.stale_session_ttl_ms, Some(now as i64))
                > 0
            {
                inner.last_activity_at.store(now, Ordering::Release);
            }
            SessionBrokerDaemon {
                inner: Arc::clone(&inner),
            }
            .reconcile_producer_owners();
        }
        if inner.idle_timeout_ms > 0
            && inner.broker.session_count() == 0
            && inner.broker.pending_command_count() == 0
            && now.saturating_sub(inner.last_activity_at.load(Ordering::Acquire))
                >= inner.idle_timeout_ms
        {
            SessionBrokerDaemon { inner }.shutdown(None);
            return;
        }
    }
}

fn peer_id(peer: &SharedSessionBrokerDaemonPeer) -> usize {
    Arc::as_ptr(peer) as *const () as usize
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn same_producer_binding(left: &ProducerPrincipal, right: &ProducerPrincipal) -> bool {
    left.app_id == right.app_id
        && left.principal_id == right.principal_id
        && left.key_id == right.key_id
        && left.grant_id == right.grant_id
        && left.session_id == right.session_id
}

fn producer_authority_is_active(authority: &AuthenticatedProducerHello) -> bool {
    catch_unwind(AssertUnwindSafe(|| authority.assert_active())).is_ok_and(|result| result.is_ok())
}

fn same_broker_state_limits(left: &SessionBrokerLimits, right: &SessionBrokerLimits) -> bool {
    left.max_sessions == right.max_sessions
        && left.max_commands_per_session == right.max_commands_per_session
        && left.max_commands_total == right.max_commands_total
        && left.max_command_input_bytes == right.max_command_input_bytes
        && left.max_command_result_bytes == right.max_command_result_bytes
        && left.max_queued_command_bytes == right.max_queued_command_bytes
        && left.max_retained_session_bytes == right.max_retained_session_bytes
        && left.max_retained_bytes == right.max_retained_bytes
        && left.default_command_timeout_ms == right.default_command_timeout_ms
        && left.max_command_timeout_ms == right.max_command_timeout_ms
}

fn parse_text_json(message: &Value) -> Result<Value, BrokerProtocolError> {
    crate::parse_session_broker_json_text(message)
}

fn exact_hello_envelope(
    value: &Value,
    kind: &str,
    key: &str,
) -> Result<Value, BrokerProtocolError> {
    let Some(record) = value.as_object() else {
        return Err(BrokerProtocolError {
            code: crate::BrokerProtocolFailureCode::InvalidRecord,
        });
    };
    if record.len() != 2
        || record.get("type").and_then(Value::as_str) != Some(kind)
        || !record.contains_key(key)
    {
        return Err(BrokerProtocolError {
            code: crate::BrokerProtocolFailureCode::InvalidKeys,
        });
    }
    Ok(record[key].clone())
}

fn has_json_content_type(request: &SessionBrokerHttpRequest) -> bool {
    request.header("content-type").is_some_and(|value| {
        value
            .split(';')
            .next()
            .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    })
}

fn invalid_content_length(request: &SessionBrokerHttpRequest) -> bool {
    request.header("content-length").is_some_and(|value| {
        value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
            || value.parse::<u64>().map_or(true, |declared| {
                declared != u64::try_from(request.body.len()).unwrap_or(u64::MAX)
            })
    })
}

fn bodyless_transport_error(request: &SessionBrokerHttpRequest) -> bool {
    matches!(request.method.as_str(), "GET" | "HEAD")
        && (request.header("transfer-encoding").is_some()
            || request
                .header("content-length")
                .is_some_and(|value| value != "0"))
}

fn json_error(message: &str, status: u16) -> SessionBrokerHttpResponse {
    SessionBrokerHttpResponse::json(status, &json!({"error": message}))
}

fn capacity_response(code: &str, resource: &str, status: u16) -> SessionBrokerHttpResponse {
    SessionBrokerHttpResponse::json(status, &json!({"error": code, "resource": resource}))
}

fn protocol_error(error: Option<BrokerProtocolError>) -> Value {
    json!({
        "error": "protocol-validation-failed",
        "code": error.map_or("invalid-app-payload", |error| error.code.as_str()),
    })
}

fn auth_required_error() -> crate::SessionBrokerAuthenticationError {
    crate::SessionBrokerAuthenticationError {
        code: SessionBrokerAuthenticationFailureCode::AuthenticationRequired,
    }
}

fn canonical_response(
    response: &SessionBrokerAuthenticatedResponse<Value>,
) -> Result<Vec<u8>, serde_json::Error> {
    let value = serde_json::to_value(response)?;
    Ok(canonicalize_json(&value)
        .map_err(|error| serde_json::Error::io(std::io::Error::other(error)))?
        .into_bytes())
}

struct ThreadWaker(thread::Thread);

impl Wake for ThreadWaker {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<T>(mut future: Pin<Box<dyn Future<Output = T> + Send + '_>>) -> T {
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park(),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        BrokerCommandScope, CallerPrincipal, SessionBrokerAppParserRegistry,
        SessionBrokerCommandParsers, SessionBrokerOptions, SessionBrokerProducerHelloAck,
        SessionBrokerProtocolParsers, SessionBrokerResponseAuthentication, SessionRegistration,
        SessionSnapshot, create_session_broker_protocol_parsers,
        parse_session_registration_envelope, parse_session_snapshot_envelope,
    };

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestInfo {
        title: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct TestState {
        selected_index: u64,
    }

    type TestBroker = SessionBroker<TestInfo, TestState, Value, Value>;
    type TestDaemon = SessionBrokerDaemon<TestInfo, TestState, Value, Value>;

    fn standard_parsers() -> Arc<SessionBrokerProtocolParsers<TestInfo, TestState, Value, Value>> {
        parsers_with_callbacks(
            Arc::new(|value| {
                parse_session_registration_envelope(value, |info| {
                    serde_json::from_value(info.clone()).ok()
                })
            }),
            Arc::new(|value| {
                parse_session_snapshot_envelope(value, |state| {
                    serde_json::from_value(state.clone()).ok()
                })
            }),
            Arc::new(|value| {
                value
                    .as_object()
                    .and_then(|record| record.get("summary")?.as_str())
                    .map(|summary| json!({"summary": summary}))
            }),
            Arc::new(|value| {
                (value.as_object().is_some_and(|record| {
                    record.len() == 1 && record.get("applied") == Some(&Value::Bool(true))
                }))
                .then(|| json!({"applied": true}))
            }),
        )
    }

    type ValueParser<T> = Arc<dyn Fn(&Value) -> Option<T> + Send + Sync>;

    fn parsers_with_callbacks(
        parse_registration: ValueParser<SessionRegistration<TestInfo>>,
        parse_snapshot: ValueParser<SessionSnapshot<TestState>>,
        parse_input: ValueParser<Value>,
        parse_result: ValueParser<Value>,
    ) -> Arc<SessionBrokerProtocolParsers<TestInfo, TestState, Value, Value>> {
        Arc::new(
            create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry {
                broker_revision: None,
                app_revision: 1,
                features: Vec::new(),
                parse_registration,
                parse_snapshot,
                commands: [1_u64, 2]
                    .into_iter()
                    .map(|version| SessionBrokerCommandParsers {
                        command: "annotate".into(),
                        version,
                        parse_input: Arc::clone(&parse_input),
                        parse_result: Arc::clone(&parse_result),
                    })
                    .collect(),
            })
            .unwrap(),
        )
    }

    fn broker() -> Arc<TestBroker> {
        Arc::new(
            SessionBroker::new(SessionBrokerOptions {
                protocol_parsers: standard_parsers(),
                limit_options: SessionBrokerLimitOptions::default(),
                describe_session: None,
            })
            .unwrap(),
        )
    }

    fn registration(session_id: &str) -> Value {
        json!({
            "registrationVersion": crate::SESSION_BROKER_REGISTRATION_VERSION,
            "sessionId": session_id,
            "pid": 123,
            "cwd": format!("/{session_id}"),
            "repoRoot": format!("/{session_id}"),
            "launchedAt": "2026-04-15T00:00:00.000Z",
            "info": {"title": "repo working tree"},
        })
    }

    fn snapshot(selected_index: u64) -> Value {
        json!({
            "updatedAt": "2026-04-15T00:00:00.000Z",
            "state": {"selectedIndex": selected_index},
        })
    }

    fn register_message(session_id: &str) -> Value {
        Value::String(
            serde_json::to_string(&json!({
                "type": "register",
                "registration": registration(session_id),
                "snapshot": snapshot(0),
            }))
            .unwrap(),
        )
    }

    #[derive(Default)]
    struct TestPeer {
        sent: Mutex<Vec<String>>,
        closed: Mutex<Option<(Option<u16>, Option<String>)>>,
        authenticated: AtomicBool,
    }

    impl TestPeer {
        fn shared(self: &Arc<Self>) -> SharedSessionBrokerDaemonPeer {
            Arc::clone(self) as SharedSessionBrokerDaemonPeer
        }

        fn sent(&self) -> Vec<String> {
            self.sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }

        fn closed(&self) -> Option<(Option<u16>, Option<String>)> {
            self.closed
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }
    }

    impl SessionBrokerDaemonPeer for TestPeer {
        fn send(&self, data: &str) -> Result<(), String> {
            self.sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(data.into());
            Ok(())
        }

        fn close(&self, code: Option<u16>, reason: Option<&str>) {
            *self
                .closed
                .lock()
                .unwrap_or_else(|error| error.into_inner()) =
                Some((code, reason.map(str::to_owned)));
        }

        fn mark_authenticated(&self) {
            self.authenticated.store(true, Ordering::Release);
        }
    }

    type TestCallerAuthentication = Arc<
        dyn Fn(
                &CallerRequestAuthenticationInput,
            )
                -> Result<AuthenticatedCallerRequest, crate::SessionBrokerAuthenticationError>
            + Send
            + Sync,
    >;

    struct TestCallerAuthenticator {
        callback: TestCallerAuthentication,
        clears: AtomicU64,
    }

    impl CallerRequestAuthenticator for TestCallerAuthenticator {
        fn authenticate_request(
            &self,
            input: &CallerRequestAuthenticationInput,
        ) -> Result<AuthenticatedCallerRequest, crate::SessionBrokerAuthenticationError> {
            (self.callback)(input)
        }

        fn clear_authentication(&self) {
            self.clears.fetch_add(1, Ordering::AcqRel);
        }
    }

    struct TestAuthorizer {
        callback: Arc<dyn Fn(&SessionBrokerAuthorizationContext) -> bool + Send + Sync>,
    }

    impl SessionBrokerAuthorizer for TestAuthorizer {
        fn authorize<'a>(
            &'a self,
            context: &'a SessionBrokerAuthorizationContext,
        ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
            let value = (self.callback)(context);
            Box::pin(async move { value })
        }
    }

    struct TestAudit {
        operations: Arc<Mutex<Vec<SessionBrokerAuditOperation>>>,
    }

    impl SessionBrokerAuditHook for TestAudit {
        fn record<'a>(
            &'a self,
            event: &'a SessionBrokerAuditEvent,
        ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
            self.operations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(event.operation);
            Box::pin(async {})
        }
    }

    fn full_caller_principal() -> CallerPrincipal {
        CallerPrincipal {
            app_id: "session-broker".into(),
            principal_id: "test-caller".into(),
            key_id: "test-key".into(),
            grant_id: "test-grant".into(),
            session_id: None,
            operations: vec![
                CallerOperation::List,
                CallerOperation::Get,
                CallerOperation::Dispatch,
                CallerOperation::Diagnostics,
                CallerOperation::Shutdown,
            ],
            commands: vec![
                BrokerCommandScope {
                    name: "annotate".into(),
                    version: 1,
                },
                BrokerCommandScope {
                    name: "annotate".into(),
                    version: 2,
                },
            ],
        }
    }

    fn authenticated_request(principal: CallerPrincipal) -> AuthenticatedCallerRequest {
        AuthenticatedCallerRequest::from_callbacks(
            principal,
            "request-1",
            Arc::new(|| Ok(())),
            Arc::new(|input| {
                Ok(SessionBrokerResponseAuthentication {
                    generation: "generation-1".into(),
                    broker_revision: 1,
                    app_contract: input.app_contract.as_ref().map(Into::into),
                    caller_session_id: "caller-session-1".into(),
                    request_id: "request-1".into(),
                    sequence: "1".into(),
                    http_status: input.http_status,
                    body_digest: "test-body-digest".into(),
                    daemon_key_id: "daemon-key-1".into(),
                    daemon_signature: "test-signature".into(),
                })
            }),
        )
    }

    fn allow_http(options: &mut SessionBrokerDaemonOptions<TestInfo, TestState, Value, Value>) {
        options.expose_http_api = true;
        options.app_id = Some("session-broker".into());
        options.app_revision = Some(1);
        options.caller_authenticator = Some(Arc::new(TestCallerAuthenticator {
            callback: Arc::new(|_| Ok(authenticated_request(full_caller_principal()))),
            clears: AtomicU64::new(0),
        }));
        options.authorizer = Some(Arc::new(TestAuthorizer {
            callback: Arc::new(|_| true),
        }));
    }

    fn daemon_with_http() -> TestDaemon {
        let mut options = SessionBrokerDaemonOptions::new(broker());
        allow_http(&mut options);
        TestDaemon::new(options).unwrap()
    }

    fn authenticated_body(response: &SessionBrokerHttpResponse) -> Value {
        response.json_body().unwrap()["body"].clone()
    }

    fn wait_until(predicate: impl Fn() -> bool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !predicate() {
            assert!(std::time::Instant::now() < deadline, "condition timed out");
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn closed_reason(peer: &TestPeer) -> String {
        peer.closed()
            .and_then(|(_, reason)| reason)
            .unwrap_or_default()
    }

    #[test]
    fn closes_late_producer_messages_without_parsing_or_recreating_state_after_shutdown() {
        let parses = Arc::new(AtomicU64::new(0));
        let counter = Arc::clone(&parses);
        let parsers = parsers_with_callbacks(
            Arc::new(move |value| {
                counter.fetch_add(1, Ordering::AcqRel);
                parse_session_registration_envelope(value, |info| {
                    serde_json::from_value(info.clone()).ok()
                })
            }),
            Arc::new(|value| {
                parse_session_snapshot_envelope(value, |state| {
                    serde_json::from_value(state.clone()).ok()
                })
            }),
            Arc::new(|value| Some(value.clone())),
            Arc::new(|value| Some(value.clone())),
        );
        let broker = Arc::new(
            SessionBroker::new(SessionBrokerOptions {
                protocol_parsers: parsers,
                limit_options: SessionBrokerLimitOptions::default(),
                describe_session: None,
            })
            .unwrap(),
        );
        let daemon = TestDaemon::new(SessionBrokerDaemonOptions::new(Arc::clone(&broker))).unwrap();
        let peer = Arc::new(TestPeer::default());
        daemon.shutdown(None);
        daemon.shutdown(None);
        daemon.handle_connection_message(peer.shared(), register_message("session-1"));
        assert_eq!(parses.load(Ordering::Acquire), 0);
        assert!(broker.list_sessions().is_empty());
        assert_eq!(
            peer.closed(),
            Some((Some(1001), Some("Session broker shutting down.".into())))
        );
    }

    type TestChallengeIssuer = Arc<
        dyn Fn(
                Value,
                &str,
            ) -> Result<
                crate::SessionBrokerHelloChallenge,
                crate::SessionBrokerAuthenticationError,
            > + Send
            + Sync,
    >;
    type TestProducerAuthenticator = Arc<
        dyn Fn(
                Value,
                &str,
            )
                -> Result<AuthenticatedProducerHello, crate::SessionBrokerAuthenticationError>
            + Send
            + Sync,
    >;

    struct TestHelloAuthenticator {
        issue: TestChallengeIssuer,
        producer: TestProducerAuthenticator,
    }

    impl SessionBrokerHelloAuthenticator for TestHelloAuthenticator {
        fn issue_hello_challenge(
            &self,
            request: Value,
            listener_endpoint: &str,
        ) -> Result<crate::SessionBrokerHelloChallenge, crate::SessionBrokerAuthenticationError>
        {
            (self.issue)(request, listener_endpoint)
        }

        fn complete_caller_hello_proof(
            &self,
            _proof: Value,
        ) -> Result<crate::AuthenticatedCallerSession, crate::SessionBrokerAuthenticationError>
        {
            Err(auth_required_error())
        }

        fn complete_producer_hello_proof(
            &self,
            proof: Value,
            connection_id: &str,
        ) -> Result<AuthenticatedProducerHello, crate::SessionBrokerAuthenticationError> {
            (self.producer)(proof, connection_id)
        }
    }

    fn challenge() -> crate::SessionBrokerHelloChallenge {
        crate::SessionBrokerHelloChallenge {
            challenge_id: "challenge-1".into(),
            generation: "generation-1".into(),
            responder_nonce: "responder-1".into(),
            expires_at: u64::MAX,
            daemon_key_id: "daemon-key-1".into(),
            daemon_signature: "signature-1".into(),
        }
    }

    fn producer_authority(
        principal: ProducerPrincipal,
        active: Arc<dyn Fn() -> bool + Send + Sync>,
        connection_id: &str,
    ) -> AuthenticatedProducerHello {
        AuthenticatedProducerHello::from_assertion(
            SessionBrokerProducerHelloAck {
                principal,
                connection_id: connection_id.into(),
                broker_revision: 1,
                app_revision: 1,
                features: Vec::new(),
                hello_transcript_hash: "transcript-1".into(),
                daemon_key_id: "daemon-key-1".into(),
                daemon_signature: "signature-1".into(),
            },
            Arc::new(move || active().then_some(()).ok_or_else(auth_required_error)),
        )
    }

    fn producer_options(
        authenticator: Arc<dyn SessionBrokerHelloAuthenticator>,
    ) -> SessionBrokerDaemonOptions<TestInfo, TestState, Value, Value> {
        let mut options = SessionBrokerDaemonOptions::new(broker());
        options.app_id = Some("dev.example".into());
        options.app_revision = Some(1);
        options.producer_endpoint = Some("ws://broker.test/session".into());
        options.hello_authenticator = Some(authenticator);
        options
    }

    #[test]
    fn does_not_send_a_deferred_producer_challenge_after_shutdown() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let blocked = Arc::clone(&gate);
        let auth = Arc::new(TestHelloAuthenticator {
            issue: Arc::new(move |_, _| {
                let (lock, ready) = &*blocked;
                let mut released = lock.lock().unwrap_or_else(|error| error.into_inner());
                while !*released {
                    released = ready
                        .wait(released)
                        .unwrap_or_else(|error| error.into_inner());
                }
                Ok(challenge())
            }),
            producer: Arc::new(|_, _| Err(auth_required_error())),
        });
        let daemon = TestDaemon::new(producer_options(auth)).unwrap();
        let peer = Arc::new(TestPeer::default());
        daemon.handle_connection_message(
            peer.shared(),
            Value::String(r#"{"type":"hello-init","hello":{}}"#.into()),
        );
        thread::sleep(Duration::from_millis(5));
        daemon.shutdown(None);
        let (lock, ready) = &*gate;
        *lock.lock().unwrap_or_else(|error| error.into_inner()) = true;
        ready.notify_all();
        thread::sleep(Duration::from_millis(5));
        assert!(peer.sent().is_empty());
        assert!(daemon.list_sessions().is_empty());
    }

    #[test]
    fn serves_health_and_raw_list_get_requests_when_http_api_is_enabled() {
        let daemon = daemon_with_http();
        let peer = Arc::new(TestPeer::default());
        daemon.handle_connection_message(peer.shared(), register_message("session-1"));
        assert_eq!(
            daemon
                .handle_request(&SessionBrokerHttpRequest::get("http://broker.test/health"))
                .unwrap()
                .status,
            200
        );
        assert_eq!(
            daemon
                .handle_request(&SessionBrokerHttpRequest::get(
                    "http://broker.test/broker/capabilities",
                ))
                .unwrap()
                .status,
            200
        );
        let list = daemon
            .handle_request(&SessionBrokerHttpRequest::json(
                "http://broker.test/broker",
                &json!({"action": "list"}),
            ))
            .unwrap();
        assert_eq!(
            authenticated_body(&list)["sessions"][0]["sessionId"],
            "session-1"
        );
        let get = daemon
            .handle_request(&SessionBrokerHttpRequest::json(
                "http://broker.test/broker",
                &json!({"action": "get", "selector": {"sessionId": "session-1"}}),
            ))
            .unwrap();
        assert_eq!(
            authenticated_body(&get)["session"]["snapshot"]["state"]["selectedIndex"],
            0
        );
        daemon.shutdown(None);
    }

    #[test]
    fn refuses_expose_http_api_without_both_explicit_authenticator_and_authorizer() {
        let mut first = SessionBrokerDaemonOptions::new(broker());
        first.expose_http_api = true;
        first.app_id = Some("session-broker".into());
        let mut second = SessionBrokerDaemonOptions::new(broker());
        second.expose_http_api = true;
        second.app_id = Some("session-broker".into());
        second.caller_authenticator = Some(Arc::new(TestCallerAuthenticator {
            callback: Arc::new(|_| Ok(authenticated_request(full_caller_principal()))),
            clears: AtomicU64::new(0),
        }));
        for daemon in [
            TestDaemon::new(first).unwrap(),
            TestDaemon::new(second).unwrap(),
        ] {
            assert_eq!(daemon.paths().api, None);
            assert!(
                daemon
                    .handle_request(&SessionBrokerHttpRequest::json(
                        "http://broker.test/broker",
                        &json!({"action": "list"}),
                    ))
                    .is_none()
            );
            daemon.shutdown(None);
        }
    }

    #[test]
    fn requires_get_with_empty_body_for_authenticated_capabilities() {
        let daemon = daemon_with_http();
        let mut request = SessionBrokerHttpRequest::get("http://broker.test/broker/capabilities");
        request.method = "POST".into();
        request.body = b"unsigned bytes".to_vec();
        assert_eq!(daemon.handle_request(&request).unwrap().status, 405);
        daemon.shutdown(None);
    }

    #[test]
    fn rejects_transport_bodies_on_bodyless_capabilities_before_authentication() {
        let calls = Arc::new(AtomicU64::new(0));
        let count = Arc::clone(&calls);
        let mut options = SessionBrokerDaemonOptions::new(broker());
        allow_http(&mut options);
        options.caller_authenticator = Some(Arc::new(TestCallerAuthenticator {
            callback: Arc::new(move |_| {
                count.fetch_add(1, Ordering::AcqRel);
                Ok(authenticated_request(full_caller_principal()))
            }),
            clears: AtomicU64::new(0),
        }));
        let daemon = TestDaemon::new(options).unwrap();
        for (method, name, value) in [
            ("GET", "content-length", "1"),
            ("GET", "content-length", "01"),
            ("GET", "transfer-encoding", "chunked"),
            ("HEAD", "content-length", "1"),
        ] {
            let mut request =
                SessionBrokerHttpRequest::get("http://broker.test/broker/capabilities");
            request.method = method.into();
            request.headers.insert(name.into(), value.into());
            let response = daemon.handle_request(&request).unwrap();
            assert_eq!(response.status, 400);
            assert_eq!(
                response.json_body().unwrap(),
                json!({"error": "Broker capabilities requests must not include a transport body."})
            );
        }
        assert_eq!(calls.load(Ordering::Acquire), 0);
        daemon.shutdown(None);
    }

    #[test]
    fn does_not_expose_raw_broker_http_api_by_default() {
        let daemon = TestDaemon::new(SessionBrokerDaemonOptions::new(broker())).unwrap();
        assert!(
            daemon
                .handle_request(&SessionBrokerHttpRequest::get(
                    "http://broker.test/broker/capabilities"
                ))
                .is_none()
        );
        assert!(
            daemon
                .handle_request(&SessionBrokerHttpRequest::json(
                    "http://broker.test/broker",
                    &json!({"action": "list"})
                ))
                .is_none()
        );
        assert_eq!(
            daemon
                .handle_request(&SessionBrokerHttpRequest::get("http://broker.test/health"))
                .unwrap()
                .status,
            200
        );
        assert_eq!(daemon.paths().api, None);
        daemon.shutdown(None);
    }

    #[test]
    fn requires_json_content_type_for_raw_broker_api_posts() {
        let daemon = daemon_with_http();
        let mut request =
            SessionBrokerHttpRequest::json("http://broker.test/broker", &json!({"action": "list"}));
        request
            .headers
            .insert("content-type".into(), "text/plain".into());
        let response = daemon.handle_request(&request).unwrap();
        assert_eq!(response.status, 415);
        assert_eq!(
            response.json_body().unwrap(),
            json!({"error": "Expected Content-Type application/json."})
        );
        daemon.shutdown(None);
    }

    #[test]
    fn authenticates_exact_bytes_before_rejecting_bom_and_malformed_utf8() {
        let bodies = Arc::new(Mutex::new(Vec::<Vec<u8>>::new()));
        let seen = Arc::clone(&bodies);
        let mut options = SessionBrokerDaemonOptions::new(broker());
        allow_http(&mut options);
        options.caller_authenticator = Some(Arc::new(TestCallerAuthenticator {
            callback: Arc::new(move |input| {
                seen.lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .push(input.body.clone());
                Ok(authenticated_request(full_caller_principal()))
            }),
            clears: AtomicU64::new(0),
        }));
        let daemon = TestDaemon::new(options).unwrap();
        let malformed = vec![
            [vec![0xef, 0xbb, 0xbf], br#"{"action":"list"}"#.to_vec()].concat(),
            vec![0x7b, 0x22, 0x78, 0x22, 0x3a, 0xc0, 0xaf, 0x7d],
        ];
        for body in &malformed {
            let request = SessionBrokerHttpRequest {
                method: "POST".into(),
                url: "http://broker.test/broker".into(),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: body.clone(),
            };
            assert_eq!(daemon.handle_request(&request).unwrap().status, 400);
        }
        assert_eq!(
            *bodies.lock().unwrap_or_else(|error| error.into_inner()),
            malformed
        );
        daemon.shutdown(None);
    }

    #[test]
    fn rejects_raw_broker_api_bodies_above_size_limit() {
        let daemon = daemon_with_http();
        let request = SessionBrokerHttpRequest {
            method: "POST".into(),
            url: "http://broker.test/broker".into(),
            headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
            body: vec![b'x'; 4 * 1024 * 1024 + 1],
        };
        let response = daemon.handle_request(&request).unwrap();
        assert_eq!(response.status, 413);
        assert_eq!(
            response.json_body().unwrap(),
            json!({"error": "capacity-exceeded", "resource": "maxHttpBodyBytes"})
        );
        daemon.shutdown(None);
    }

    #[test]
    fn dispatches_one_raw_command_through_broker_api() {
        let daemon = daemon_with_http();
        let peer = Arc::new(TestPeer::default());
        daemon.handle_connection_message(peer.shared(), register_message("session-1"));
        let worker = {
            let daemon = daemon.clone();
            thread::spawn(move || {
                daemon.handle_request(&SessionBrokerHttpRequest::json(
                "http://broker.test/broker",
                &json!({"action": "dispatch", "selector": {"sessionId": "session-1"}, "command": "annotate", "commandVersion": 2, "input": {"summary": "Review note"}}),
            )).unwrap()
            })
        };
        wait_until(|| !peer.sent().is_empty());
        let outgoing: Value = serde_json::from_str(peer.sent().last().unwrap()).unwrap();
        assert_eq!(outgoing["commandVersion"], 2);
        daemon.handle_connection_message(peer.shared(), Value::String(serde_json::to_string(&json!({
            "type": "command-result", "requestId": outgoing["requestId"], "ok": true, "result": {"applied": true}
        })).unwrap()));
        let response = worker.join().unwrap();
        assert_eq!(
            authenticated_body(&response),
            json!({"result": {"applied": true}})
        );
        daemon.shutdown(None);
    }

    #[test]
    fn rejects_unsupported_dispatch_controls_before_delivery() {
        let daemon = daemon_with_http();
        let peer = Arc::new(TestPeer::default());
        daemon.handle_connection_message(peer.shared(), register_message("session-1"));
        for control in [
            json!({"deadline": 1}),
            json!({"idempotencyKey": "request-key-1"}),
        ] {
            let mut body = json!({"action": "dispatch", "selector": {"sessionId": "session-1"}, "command": "annotate", "input": {"summary": "Review note"}});
            body.as_object_mut()
                .unwrap()
                .extend(control.as_object().unwrap().clone());
            let response = daemon
                .handle_request(&SessionBrokerHttpRequest::json(
                    "http://broker.test/broker",
                    &body,
                ))
                .unwrap();
            assert_eq!(response.status, 400);
            assert_eq!(
                authenticated_body(&response),
                json!({"error": "protocol-validation-failed", "code": "invalid-keys"})
            );
        }
        assert!(peer.sent().is_empty());
        daemon.shutdown(None);
    }

    #[test]
    fn executes_each_app_parser_once_and_forwards_transformed_input() {
        let calls = Arc::new(Mutex::new([0_u64; 4]));
        let registration_calls = Arc::clone(&calls);
        let snapshot_calls = Arc::clone(&calls);
        let input_calls = Arc::clone(&calls);
        let result_calls = Arc::clone(&calls);
        let parsers = parsers_with_callbacks(
            Arc::new(move |value| {
                registration_calls.lock().unwrap()[0] += 1;
                parse_session_registration_envelope(value, |info| {
                    serde_json::from_value(info.clone()).ok()
                })
            }),
            Arc::new(move |value| {
                snapshot_calls.lock().unwrap()[1] += 1;
                parse_session_snapshot_envelope(value, |state| {
                    serde_json::from_value(state.clone()).ok()
                })
            }),
            Arc::new(move |value| {
                input_calls.lock().unwrap()[2] += 1;
                value["summary"]
                    .as_str()
                    .map(|summary| json!({"summary": summary.to_uppercase()}))
            }),
            Arc::new(move |value| {
                result_calls.lock().unwrap()[3] += 1;
                (value == &json!({"applied": true})).then(|| value.clone())
            }),
        );
        let custom_broker = Arc::new(
            SessionBroker::new(SessionBrokerOptions {
                protocol_parsers: parsers,
                limit_options: SessionBrokerLimitOptions::default(),
                describe_session: None,
            })
            .unwrap(),
        );
        let mut options = SessionBrokerDaemonOptions::new(custom_broker);
        allow_http(&mut options);
        let daemon = TestDaemon::new(options).unwrap();
        let peer = Arc::new(TestPeer::default());
        daemon.handle_connection_message(peer.shared(), register_message("session-1"));
        assert_eq!(*calls.lock().unwrap(), [1, 1, 0, 0]);
        daemon.handle_connection_message(
            peer.shared(),
            Value::String(
                serde_json::to_string(
                    &json!({"type": "snapshot", "sessionId": "session-1", "snapshot": snapshot(1)}),
                )
                .unwrap(),
            ),
        );
        assert_eq!(calls.lock().unwrap()[1], 2);
        let worker = {
            let daemon = daemon.clone();
            thread::spawn(move || {
                daemon.handle_request(&SessionBrokerHttpRequest::json("http://broker.test/broker", &json!({"action": "dispatch", "selector": {"sessionId": "session-1"}, "command": "annotate", "input": {"summary": "review note"}}))).unwrap()
            })
        };
        wait_until(|| !peer.sent().is_empty());
        let outgoing: Value = serde_json::from_str(peer.sent().last().unwrap()).unwrap();
        assert_eq!(outgoing["input"], json!({"summary": "REVIEW NOTE"}));
        daemon.handle_connection_message(peer.shared(), Value::String(serde_json::to_string(&json!({"type": "command-result", "requestId": outgoing["requestId"], "ok": true, "result": {"applied": true}})).unwrap()));
        assert_eq!(
            authenticated_body(&worker.join().unwrap()),
            json!({"result": {"applied": true}})
        );
        assert_eq!(*calls.lock().unwrap(), [1, 2, 1, 1]);
        daemon.shutdown(None);
    }

    #[test]
    fn closes_snapshot_assertions_from_unregistered_peers() {
        let daemon = TestDaemon::new(SessionBrokerDaemonOptions::new(broker())).unwrap();
        let peer = Arc::new(TestPeer::default());
        daemon.handle_connection_message(peer.shared(), Value::String(serde_json::to_string(&json!({"type": "snapshot", "sessionId": "missing-session", "snapshot": snapshot(0)})).unwrap()));
        assert_eq!(
            peer.closed(),
            Some((Some(1008), Some("Session ownership rejected.".into())))
        );
        daemon.shutdown(None);
    }

    #[test]
    fn rejects_producer_hello_wrappers_with_unknown_or_dangerous_keys() {
        let calls = Arc::new(AtomicU64::new(0));
        let count = Arc::clone(&calls);
        let auth = Arc::new(TestHelloAuthenticator {
            issue: Arc::new(move |_, _| {
                count.fetch_add(1, Ordering::AcqRel);
                Ok(challenge())
            }),
            producer: Arc::new(|_, _| Err(auth_required_error())),
        });
        let daemon = TestDaemon::new(producer_options(auth)).unwrap();
        for message in [
            r#"{"type":"hello-init","hello":{},"extra":true}"#,
            r#"{"type":"hello-init","hello":{},"__proto__":{}}"#,
        ] {
            let peer = Arc::new(TestPeer::default());
            daemon.handle_connection_message(peer.shared(), Value::String(message.into()));
            assert!(closed_reason(&peer).contains("authentication required"));
        }
        assert_eq!(calls.load(Ordering::Acquire), 0);
        daemon.shutdown(None);
    }

    #[test]
    fn pre_registration_authentication_failures_do_not_postpone_idle_shutdown() {
        let auth = Arc::new(TestHelloAuthenticator {
            issue: Arc::new(|_, _| Err(auth_required_error())),
            producer: Arc::new(|_, _| Err(auth_required_error())),
        });
        let mut options = producer_options(auth);
        options.idle_timeout_ms = Some(50);
        let daemon = TestDaemon::new(options).unwrap();
        let activity = daemon.inner.last_activity_at.load(Ordering::Acquire);
        thread::sleep(Duration::from_millis(10));
        let peer = Arc::new(TestPeer::default());
        daemon.handle_connection_message(
            peer.shared(),
            Value::String(r#"{"type":"hello-init","hello":{}}"#.into()),
        );
        wait_until(|| peer.closed().is_some());
        let shared = peer.shared();
        daemon.handle_connection_close(&shared);
        assert_eq!(
            daemon.inner.last_activity_at.load(Ordering::Acquire),
            activity
        );
        assert!(daemon.wait_stopped(Duration::from_millis(100)));
    }

    #[derive(Clone)]
    struct MutableProducer {
        operations: Arc<Mutex<Vec<ProducerOperation>>>,
        principal_id: Arc<Mutex<String>>,
        active: Arc<AtomicBool>,
        active_checks: Arc<AtomicU64>,
    }

    impl MutableProducer {
        fn principal(&self) -> ProducerPrincipal {
            ProducerPrincipal {
                app_id: "dev.example".into(),
                principal_id: self
                    .principal_id
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone(),
                key_id: "producer-key-1".into(),
                grant_id: "producer-grant-1".into(),
                session_id: None,
                scopes: self
                    .operations
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone(),
            }
        }

        fn authority(&self, connection_id: &str) -> AuthenticatedProducerHello {
            let checks = Arc::clone(&self.active_checks);
            let active = Arc::clone(&self.active);
            producer_authority(
                self.principal(),
                Arc::new(move || {
                    checks.fetch_add(1, Ordering::AcqRel);
                    active.load(Ordering::Acquire)
                }),
                connection_id,
            )
        }
    }

    fn mutable_producer_auth(config: MutableProducer) -> Arc<dyn SessionBrokerHelloAuthenticator> {
        Arc::new(TestHelloAuthenticator {
            issue: Arc::new(|_, _| Ok(challenge())),
            producer: Arc::new(move |_, connection_id| Ok(config.authority(connection_id))),
        })
    }

    fn authenticate_producer(daemon: &TestDaemon, peer: &Arc<TestPeer>) {
        daemon.handle_connection_message(
            peer.shared(),
            Value::String(r#"{"type":"hello-init","hello":{}}"#.into()),
        );
        wait_until(|| !peer.sent().is_empty() || peer.closed().is_some());
        daemon.handle_connection_message(
            peer.shared(),
            Value::String(r#"{"type":"hello-proof","proof":{}}"#.into()),
        );
        wait_until(|| peer.sent().len() >= 2 || peer.closed().is_some());
    }

    #[test]
    fn requires_reconnect_scope_and_rechecks_retained_producer_authority() {
        let config = MutableProducer {
            operations: Arc::new(Mutex::new(vec![ProducerOperation::Register])),
            principal_id: Arc::new(Mutex::new("producer-1".into())),
            active: Arc::new(AtomicBool::new(true)),
            active_checks: Arc::new(AtomicU64::new(0)),
        };
        let broker = broker();
        let mut options = producer_options(mutable_producer_auth(config.clone()));
        options.broker = Arc::clone(&broker);
        let daemon = TestDaemon::new(options).unwrap();
        let first = Arc::new(TestPeer::default());
        authenticate_producer(&daemon, &first);
        assert!(!first.authenticated.load(Ordering::Acquire));
        daemon.handle_connection_message(first.shared(), register_message("session-1"));
        assert!(first.authenticated.load(Ordering::Acquire));

        let denied = Arc::new(TestPeer::default());
        authenticate_producer(&daemon, &denied);
        daemon.handle_connection_message(denied.shared(), register_message("session-1"));
        assert!(closed_reason(&denied).contains("scope rejected"));
        assert!(first.closed().is_none());

        *config.operations.lock().unwrap() = vec![ProducerOperation::Reconnect];
        let replacement = Arc::new(TestPeer::default());
        authenticate_producer(&daemon, &replacement);
        daemon.handle_connection_message(replacement.shared(), register_message("session-1"));
        assert!(closed_reason(&first).contains("owner reconnected"));
        assert_eq!(daemon.list_sessions().len(), 1);

        daemon.handle_connection_message(first.shared(), register_message("session-1"));
        assert!(closed_reason(&first).contains("authentication required"));

        let before = config.active_checks.load(Ordering::Acquire);
        let sent_before = replacement.sent().len();
        config.active.store(false, Ordering::Release);
        let pending = broker
            .dispatch_command(crate::DispatchSessionCommand::new(
                crate::SessionSelector {
                    session_id: Some("session-1".into()),
                    ..crate::SessionSelector::default()
                },
                "annotate",
                json!({"summary": "must stay private"}),
                "timed out",
            ))
            .unwrap();
        let error = pending.receive().unwrap_err();
        assert!(error.to_string().contains("authority expired"));
        assert_eq!(replacement.sent().len(), sent_before);
        assert!(closed_reason(&replacement).contains("authority expired"));

        daemon.handle_connection_message(
            replacement.shared(),
            Value::String(r#"{"type":"heartbeat","sessionId":"session-1"}"#.into()),
        );
        daemon.handle_connection_message(
            replacement.shared(),
            Value::String(
                serde_json::to_string(&json!({
                    "type": "snapshot",
                    "sessionId": "session-1",
                    "snapshot": snapshot(1),
                }))
                .unwrap(),
            ),
        );
        assert_eq!(config.active_checks.load(Ordering::Acquire), before + 3);
        assert_eq!(
            daemon
                .get_session(&crate::SessionSelector {
                    session_id: Some("session-1".into()),
                    ..crate::SessionSelector::default()
                })
                .unwrap()
                .snapshot
                .state
                .selected_index,
            0
        );
        daemon.shutdown(None);
    }

    #[test]
    fn retains_active_reconnect_ownership_when_reconciliation_prunes_stale_session() {
        let config = MutableProducer {
            operations: Arc::new(Mutex::new(vec![
                ProducerOperation::Register,
                ProducerOperation::Reconnect,
            ])),
            principal_id: Arc::new(Mutex::new("producer-1".into())),
            active: Arc::new(AtomicBool::new(true)),
            active_checks: Arc::new(AtomicU64::new(0)),
        };
        let broker = broker();
        let mut options = producer_options(mutable_producer_auth(config.clone()));
        options.broker = Arc::clone(&broker);
        let daemon = TestDaemon::new(options).unwrap();
        let owner = Arc::new(TestPeer::default());
        authenticate_producer(&daemon, &owner);
        daemon.handle_connection_message(owner.shared(), register_message("session-1"));
        let stale_at = now_ms() + 2_000;
        assert_eq!(broker.prune_stale_sessions(1_000, Some(stale_at as i64)), 1);
        daemon.reconcile_producer_owners();
        assert!(closed_reason(&owner).contains("authority retired"));

        *config.principal_id.lock().unwrap() = "different-producer".into();
        *config.operations.lock().unwrap() = vec![ProducerOperation::Reconnect];
        let different = Arc::new(TestPeer::default());
        authenticate_producer(&daemon, &different);
        daemon.handle_connection_message(different.shared(), register_message("session-1"));
        assert!(closed_reason(&different).contains("scope rejected"));

        *config.principal_id.lock().unwrap() = "producer-1".into();
        *config.operations.lock().unwrap() = vec![ProducerOperation::Register];
        let register_only = Arc::new(TestPeer::default());
        authenticate_producer(&daemon, &register_only);
        daemon.handle_connection_message(register_only.shared(), register_message("session-1"));
        assert!(closed_reason(&register_only).contains("scope rejected"));

        *config.operations.lock().unwrap() = vec![ProducerOperation::Reconnect];
        let replacement = Arc::new(TestPeer::default());
        authenticate_producer(&daemon, &replacement);
        daemon.handle_connection_message(replacement.shared(), register_message("session-1"));
        assert!(replacement.closed().is_none());
        assert_eq!(daemon.list_sessions().len(), 1);

        config.active.store(false, Ordering::Release);
        assert_eq!(
            broker.prune_stale_sessions(1_000, Some(stale_at as i64 + 1)),
            1
        );
        daemon.reconcile_producer_owners();
        assert!(daemon.inner.producers.lock().unwrap().reconnects.is_empty());
        daemon.shutdown(None);
    }

    #[test]
    fn rejects_duplicate_live_registration_without_retiring_owner() {
        let daemon = TestDaemon::new(SessionBrokerDaemonOptions::new(broker())).unwrap();
        let owner = Arc::new(TestPeer::default());
        let duplicate = Arc::new(TestPeer::default());
        daemon.handle_connection_message(owner.shared(), register_message("session-1"));
        daemon.handle_connection_message(duplicate.shared(), register_message("session-1"));
        assert_eq!(
            duplicate.closed(),
            Some((Some(1008), Some("Session registration rejected.".into())))
        );
        let shared = duplicate.shared();
        daemon.handle_connection_close(&shared);
        assert_eq!(daemon.list_sessions().len(), 1);
        daemon.shutdown(None);
    }

    #[test]
    fn rejects_cross_peer_snapshot_heartbeat_and_result_authority() {
        let daemon = daemon_with_http();
        let owner = Arc::new(TestPeer::default());
        let snapshot_peer = Arc::new(TestPeer::default());
        let heartbeat_peer = Arc::new(TestPeer::default());
        let result_peer = Arc::new(TestPeer::default());
        for (peer, session_id) in [
            (&owner, "session-1"),
            (&snapshot_peer, "session-2"),
            (&heartbeat_peer, "session-3"),
            (&result_peer, "session-4"),
        ] {
            daemon.handle_connection_message(peer.shared(), register_message(session_id));
        }
        daemon.handle_connection_message(
            snapshot_peer.shared(),
            Value::String(
                serde_json::to_string(
                    &json!({"type": "snapshot", "sessionId": "session-1", "snapshot": snapshot(1)}),
                )
                .unwrap(),
            ),
        );
        assert_eq!(closed_reason(&snapshot_peer), "Session ownership rejected.");
        let seen = daemon
            .get_session(&crate::SessionSelector {
                session_id: Some("session-1".into()),
                ..crate::SessionSelector::default()
            })
            .unwrap()
            .last_seen_at;
        daemon.handle_connection_message(
            heartbeat_peer.shared(),
            Value::String(r#"{"type":"heartbeat","sessionId":"session-1"}"#.into()),
        );
        assert_eq!(
            closed_reason(&heartbeat_peer),
            "Session ownership rejected."
        );
        assert_eq!(
            daemon
                .get_session(&crate::SessionSelector {
                    session_id: Some("session-1".into()),
                    ..crate::SessionSelector::default()
                })
                .unwrap()
                .last_seen_at,
            seen
        );

        let worker = {
            let daemon = daemon.clone();
            thread::spawn(move || {
                daemon.handle_request(&SessionBrokerHttpRequest::json("http://broker.test/broker", &json!({"action": "dispatch", "selector": {"sessionId": "session-1"}, "command": "annotate", "input": {"summary": "Review note"}}))).unwrap()
            })
        };
        wait_until(|| !owner.sent().is_empty());
        let outgoing: Value = serde_json::from_str(owner.sent().last().unwrap()).unwrap();
        daemon.handle_connection_message(result_peer.shared(), Value::String(serde_json::to_string(&json!({"type": "command-result", "requestId": outgoing["requestId"], "ok": true, "result": {"applied": "forged"}})).unwrap()));
        assert_eq!(closed_reason(&result_peer), "Command ownership rejected.");
        assert_eq!(daemon.get_health().pending_commands, 1);
        daemon.handle_connection_message(owner.shared(), Value::String(serde_json::to_string(&json!({"type": "command-result", "requestId": outgoing["requestId"], "ok": true, "result": {"applied": true}})).unwrap()));
        assert_eq!(
            authenticated_body(&worker.join().unwrap()),
            json!({"result": {"applied": true}})
        );
        daemon.shutdown(None);
    }

    #[test]
    fn closes_malformed_results_without_resolving_pending_or_leaking_parser_details() {
        let daemon = daemon_with_http();
        let owner = Arc::new(TestPeer::default());
        daemon.handle_connection_message(owner.shared(), register_message("session-1"));
        let worker = {
            let daemon = daemon.clone();
            thread::spawn(move || {
                daemon.handle_request(&SessionBrokerHttpRequest::json("http://broker.test/broker", &json!({"action": "dispatch", "selector": {"sessionId": "session-1"}, "command": "annotate", "input": {"summary": "note"}}))).unwrap()
            })
        };
        wait_until(|| !owner.sent().is_empty());
        let outgoing: Value = serde_json::from_str(owner.sent().last().unwrap()).unwrap();
        daemon.handle_connection_message(owner.shared(), Value::String(serde_json::to_string(&json!({"type": "command-result", "requestId": outgoing["requestId"], "ok": true, "result": {"applied": false, "parserStack": "secret"}})).unwrap()));
        assert_eq!(closed_reason(&owner), "Malformed command result.");
        assert_eq!(daemon.get_health().pending_commands, 1);
        let shared = owner.shared();
        daemon.handle_connection_close(&shared);
        let response = worker.join().unwrap();
        assert!(!String::from_utf8_lossy(&response.body).contains("parserStack"));
        assert_eq!(daemon.get_health().pending_commands, 0);
        daemon.shutdown(None);
    }

    #[test]
    fn preserves_prior_registration_when_replacement_parser_rejects() {
        let daemon = TestDaemon::new(SessionBrokerDaemonOptions::new(broker())).unwrap();
        let owner = Arc::new(TestPeer::default());
        daemon.handle_connection_message(owner.shared(), register_message("session-1"));
        let mut bad = registration("session-1");
        bad.as_object_mut()
            .unwrap()
            .insert("unexpected".into(), Value::Bool(true));
        daemon.handle_connection_message(
            owner.shared(),
            Value::String(
                serde_json::to_string(
                    &json!({"type": "register", "registration": bad, "snapshot": snapshot(0)}),
                )
                .unwrap(),
            ),
        );
        assert_eq!(closed_reason(&owner), "Incompatible session registration.");
        assert_eq!(daemon.list_sessions().len(), 1);
        daemon.shutdown(None);
    }

    #[test]
    fn requires_operation_command_and_app_authorization_before_control() {
        let calls = Arc::new(AtomicU64::new(0));
        let principal = CallerPrincipal {
            app_id: "session-broker".into(),
            principal_id: "limited-caller".into(),
            key_id: "limited-key".into(),
            grant_id: "limited-grant".into(),
            session_id: None,
            operations: vec![CallerOperation::List, CallerOperation::Dispatch],
            commands: vec![BrokerCommandScope {
                name: "allowed".into(),
                version: 1,
            }],
        };
        let mut options = SessionBrokerDaemonOptions::new(broker());
        options.expose_http_api = true;
        options.app_id = Some("session-broker".into());
        options.app_revision = Some(1);
        options.caller_authenticator = Some(Arc::new(TestCallerAuthenticator {
            callback: Arc::new(move |_| Ok(authenticated_request(principal.clone()))),
            clears: AtomicU64::new(0),
        }));
        let count = Arc::clone(&calls);
        options.authorizer = Some(Arc::new(TestAuthorizer {
            callback: Arc::new(move |_| {
                count.fetch_add(1, Ordering::AcqRel);
                true
            }),
        }));
        let daemon = TestDaemon::new(options).unwrap();
        let owner = Arc::new(TestPeer::default());
        daemon.handle_connection_message(owner.shared(), register_message("session-1"));
        let post = |body: Value| {
            daemon
                .handle_request(&SessionBrokerHttpRequest::json(
                    "http://broker.test/broker",
                    &body,
                ))
                .unwrap()
        };
        assert_eq!(
            post(json!({"action": "get", "selector": {"sessionId": "session-1"}})).status,
            403
        );
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert_eq!(post(json!({"action": "dispatch", "selector": {"sessionId": "session-1"}, "command": "forbidden", "input": {}})).status, 403);
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert_eq!(post(json!({"action": "dispatch", "selector": {"sessionId": "session-1"}, "command": "allowed", "commandVersion": 0, "input": {}})).status, 400);
        assert_eq!(calls.load(Ordering::Acquire), 0);
        assert_eq!(post(json!({"action": "list"})).status, 200);
        assert_eq!(calls.load(Ordering::Acquire), 1);
        daemon.shutdown(None);
    }

    #[test]
    fn returns_redacted_authentication_failures_without_app_authorization() {
        let authorized = Arc::new(AtomicBool::new(false));
        let operations = Arc::new(Mutex::new(Vec::new()));
        let mut options = SessionBrokerDaemonOptions::new(broker());
        options.expose_http_api = true;
        options.app_id = Some("session-broker".into());
        options.app_revision = Some(1);
        options.caller_authenticator = Some(Arc::new(TestCallerAuthenticator {
            callback: Arc::new(|_| {
                Err(crate::SessionBrokerAuthenticationError {
                    code: SessionBrokerAuthenticationFailureCode::InvalidSignature,
                })
            }),
            clears: AtomicU64::new(0),
        }));
        let did_authorize = Arc::clone(&authorized);
        options.authorizer = Some(Arc::new(TestAuthorizer {
            callback: Arc::new(move |_| {
                did_authorize.store(true, Ordering::Release);
                true
            }),
        }));
        options.audit = Some(Arc::new(TestAudit {
            operations: Arc::clone(&operations),
        }));
        let daemon = TestDaemon::new(options).unwrap();
        let response = daemon
            .handle_request(&SessionBrokerHttpRequest::json(
                "http://broker.test/broker",
                &json!({"action": "list"}),
            ))
            .unwrap();
        assert_eq!(response.status, 401);
        assert_eq!(
            response.json_body().unwrap(),
            json!({"error": "authentication-failed", "code": "invalid-signature"})
        );
        assert_eq!(
            daemon
                .handle_request(&SessionBrokerHttpRequest::get(
                    "http://broker.test/broker/capabilities"
                ))
                .unwrap()
                .status,
            401
        );
        let custom = daemon.handle_authenticated_control(
            &SessionBrokerHttpRequest::get("http://broker.test/custom"),
            SessionBrokerAuthenticatedControlOptions {
                authentication_failure_operation: Some(CallerOperation::Shutdown),
                resolve_failure_target_specific: None,
            },
            |_| {
                Ok(SessionBrokerAuthenticatedControlFacts {
                    operation: CallerOperation::Shutdown,
                    session_id: None,
                    command: None,
                    command_version: None,
                    target_specific: None,
                })
            },
            |_, _| SessionBrokerAuthenticatedControlResult {
                body: json!({"ok": true}),
                status: 200,
            },
        );
        assert_eq!(custom.status, 401);
        assert!(!authorized.load(Ordering::Acquire));
        assert_eq!(
            *operations.lock().unwrap(),
            vec![
                SessionBrokerAuditOperation::Unknown,
                SessionBrokerAuditOperation::Caller(CallerOperation::Diagnostics),
                SessionBrokerAuditOperation::Caller(CallerOperation::Shutdown),
            ]
        );
        daemon.shutdown(None);
    }

    #[test]
    fn rejects_daemon_state_limits_not_configured_on_broker_controller() {
        let mut options = SessionBrokerDaemonOptions::new(broker());
        options.limit_options.limits.max_sessions = Some(0);
        let error = TestDaemon::new(options).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("state limits must be configured")
        );
    }

    #[test]
    fn returns_busy_before_admitting_more_than_configured_controls() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let started = Arc::new(AtomicBool::new(false));
        let blocked = Arc::clone(&gate);
        let entered = Arc::clone(&started);
        let mut options = SessionBrokerDaemonOptions::new(broker());
        allow_http(&mut options);
        options.limit_options.limits.max_concurrent_http_controls = Some(1);
        options.caller_authenticator = Some(Arc::new(TestCallerAuthenticator {
            callback: Arc::new(move |_| {
                entered.store(true, Ordering::Release);
                let (lock, ready) = &*blocked;
                let mut released = lock.lock().unwrap_or_else(|error| error.into_inner());
                while !*released {
                    released = ready
                        .wait(released)
                        .unwrap_or_else(|error| error.into_inner());
                }
                Ok(authenticated_request(full_caller_principal()))
            }),
            clears: AtomicU64::new(0),
        }));
        let daemon = TestDaemon::new(options).unwrap();
        let first = {
            let daemon = daemon.clone();
            thread::spawn(move || {
                daemon
                    .handle_request(&SessionBrokerHttpRequest::json(
                        "http://broker.test/broker",
                        &json!({"action": "list"}),
                    ))
                    .unwrap()
            })
        };
        wait_until(|| started.load(Ordering::Acquire));
        let overflow = daemon
            .handle_request(&SessionBrokerHttpRequest::json(
                "http://broker.test/broker",
                &json!({"action": "list"}),
            ))
            .unwrap();
        assert_eq!(overflow.status, 503);
        assert_eq!(
            overflow.json_body().unwrap(),
            json!({"error": "busy", "resource": "maxConcurrentHttpControls"})
        );
        let (lock, ready) = &*gate;
        *lock.lock().unwrap() = true;
        ready.notify_all();
        assert_eq!(first.join().unwrap().status, 200);
        daemon.shutdown(None);
    }

    #[test]
    fn lower_route_body_ceiling_releases_its_reservation() {
        let mut options = SessionBrokerDaemonOptions::new(broker());
        options.limit_options.limits.max_http_body_bytes = Some(4);
        options.limit_options.limits.max_in_flight_http_body_bytes = Some(8);
        let daemon = TestDaemon::new(options).unwrap();
        let handled = Arc::new(AtomicU64::new(0));
        let invoke = |body: &[u8]| {
            let request = SessionBrokerHttpRequest {
                method: "POST".into(),
                url: "http://broker.test/custom".into(),
                headers: BTreeMap::new(),
                body: body.to_vec(),
            };
            let count = Arc::clone(&handled);
            daemon.handle_bounded_control(
                &request,
                SessionBrokerBoundedControlOptions {
                    max_body_bytes: Some(4),
                    payload_too_large: Some(Arc::new(|| SessionBrokerHttpResponse::empty(413))),
                },
                move |bytes| {
                    count.fetch_add(1, Ordering::AcqRel);
                    SessionBrokerHttpResponse {
                        status: 200,
                        headers: BTreeMap::new(),
                        body: bytes.to_vec(),
                    }
                },
            )
        };
        assert_eq!(invoke(b"1234").body, b"1234");
        assert_eq!(invoke(b"12345").status, 413);
        assert_eq!(invoke(b"1234").status, 200);
        assert_eq!(handled.load(Ordering::Acquire), 2);
        daemon.shutdown(None);
    }

    #[test]
    fn requests_shutdown_after_idle_timeout_when_no_sessions_remain() {
        let mut options = SessionBrokerDaemonOptions::new(broker());
        options.idle_timeout_ms = Some(20);
        options.stale_session_sweep_interval_ms = Some(10);
        let daemon = TestDaemon::new(options).unwrap();
        assert!(daemon.wait_stopped(Duration::from_millis(100)));
    }
}
