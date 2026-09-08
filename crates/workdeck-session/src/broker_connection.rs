//! Runtime-neutral producer websocket connection with bounded FIFO command execution.

use std::collections::{BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use crate::{
    BrokerCapacityCode, BrokerProtocolFailureCode, ProducerGrant, ResourceBudget,
    SessionBrokerClientCredential, SessionBrokerConnectionCloseDirective, SessionBrokerCrypto,
    SessionBrokerDaemonVerifier, SessionBrokerHelloChallengeRequest,
    SessionBrokerHelloClientOptions, SessionBrokerLimitOptions, SessionBrokerLimits,
    SessionBrokerProtocolParsers, SessionBrokerSocketCloseEvent, SessionBrokerSocketLike,
    SessionBrokerSocketMessageEvent, SessionRegistration, SessionServerMessage, SessionSnapshot,
    answer_session_broker_hello_challenge, create_session_broker_hello_request,
    parse_session_broker_hello_challenge, parse_session_broker_json_text,
    resolve_session_broker_limits, verify_producer_hello_ack,
};

const DEFAULT_RECONNECT_DELAY_MS: u64 = 3_000;
const DEFAULT_HEARTBEAT_INTERVAL_MS: u64 = 10_000;
const DEFAULT_SOCKET_OPEN_STATE: u16 = 1;
const PRODUCER_COMMAND_OVERHEAD_BYTES: u64 = 128;

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionBrokerConnectionError {
    #[error("Session broker socket operation failed: {0}")]
    Socket(String),
    #[error("Session broker connection protocol validation failed: {0}")]
    Protocol(&'static str),
    #[error("Invalid session broker connection limits: {0}")]
    Limits(String),
}

pub trait SessionBrokerConnectionBridge<Input, ResultValue>: Send + Sync + 'static {
    fn dispatch_command(
        &self,
        message: SessionServerMessage<String, Input>,
    ) -> Result<ResultValue, String>;

    /// A validated successful result was accepted by the current socket's send
    /// queue. This is not a peer acknowledgement. Lifecycle owners may use it
    /// to order local shutdown after the reply, rather than after dispatch alone.
    fn command_result_queued(&self, _request_id: &str) {}
}

impl<Input, ResultValue, F> SessionBrokerConnectionBridge<Input, ResultValue> for F
where
    F: Fn(SessionServerMessage<String, Input>) -> Result<ResultValue, String>
        + Send
        + Sync
        + 'static,
{
    fn dispatch_command(
        &self,
        message: SessionServerMessage<String, Input>,
    ) -> Result<ResultValue, String> {
        self(message)
    }
}

#[derive(Clone)]
pub struct SessionBrokerProducerAuthentication {
    pub app_id: String,
    pub app_revision: u32,
    pub credential: SessionBrokerClientCredential<ProducerGrant>,
    pub daemon: SessionBrokerDaemonVerifier,
    pub crypto: Arc<dyn SessionBrokerCrypto>,
}

impl std::fmt::Debug for SessionBrokerProducerAuthentication {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerProducerAuthentication")
            .field("app_id", &self.app_id)
            .field("app_revision", &self.app_revision)
            .field("credential", &self.credential)
            .field("daemon", &self.daemon)
            .finish_non_exhaustive()
    }
}

impl SessionBrokerProducerAuthentication {
    #[must_use]
    pub fn native(
        app_id: impl Into<String>,
        app_revision: u32,
        credential: SessionBrokerClientCredential<ProducerGrant>,
        daemon: SessionBrokerDaemonVerifier,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            app_revision,
            credential,
            daemon,
            crypto: Arc::new(crate::NativeSessionBrokerCrypto),
        }
    }
}

type SocketFactory = Arc<
    dyn Fn(&str) -> Result<Arc<dyn SessionBrokerSocketLike>, SessionBrokerConnectionError>
        + Send
        + Sync,
>;
type CloseResolver = Arc<
    dyn Fn(SessionBrokerSocketCloseEvent) -> SessionBrokerConnectionCloseDirective + Send + Sync,
>;
type ReconnectPreparation = Arc<dyn Fn() -> Result<(), String> + Send + Sync>;
type ConnectionCallback = Arc<dyn Fn() + Send + Sync>;
type WarningCallback = Arc<dyn Fn(&str) + Send + Sync>;

pub struct SessionBrokerConnectionOptions<Info, State, Input, ResultValue> {
    pub url: String,
    pub create_socket: SocketFactory,
    pub registration: SessionRegistration<Info>,
    pub snapshot: SessionSnapshot<State>,
    pub bridge: Option<Arc<dyn SessionBrokerConnectionBridge<Input, ResultValue>>>,
    pub protocol_parsers: Arc<SessionBrokerProtocolParsers<Info, State, Input, ResultValue>>,
    pub producer_authentication: Option<SessionBrokerProducerAuthentication>,
    pub heartbeat_interval_ms: u64,
    pub reconnect_delay_ms: u64,
    pub open_state: u16,
    pub resolve_close: Option<CloseResolver>,
    pub prepare_reconnect: Option<ReconnectPreparation>,
    pub on_connected: Option<ConnectionCallback>,
    pub on_warning: Option<WarningCallback>,
    pub limit_options: SessionBrokerLimitOptions,
}

impl<Info, State, Input, ResultValue>
    SessionBrokerConnectionOptions<Info, State, Input, ResultValue>
{
    pub fn new(
        url: impl Into<String>,
        create_socket: SocketFactory,
        registration: SessionRegistration<Info>,
        snapshot: SessionSnapshot<State>,
        protocol_parsers: Arc<SessionBrokerProtocolParsers<Info, State, Input, ResultValue>>,
    ) -> Self {
        Self {
            url: url.into(),
            create_socket,
            registration,
            snapshot,
            bridge: None,
            protocol_parsers,
            producer_authentication: None,
            heartbeat_interval_ms: DEFAULT_HEARTBEAT_INTERVAL_MS,
            reconnect_delay_ms: DEFAULT_RECONNECT_DELAY_MS,
            open_state: DEFAULT_SOCKET_OPEN_STATE,
            resolve_close: None,
            prepare_reconnect: None,
            on_connected: None,
            on_warning: None,
            limit_options: SessionBrokerLimitOptions::default(),
        }
    }
}

#[derive(Clone)]
struct CommandReservation {
    count: crate::BudgetReservation,
    bytes: crate::BudgetReservation,
}

impl CommandReservation {
    fn release(&self) {
        self.count.release();
        self.bytes.release();
    }
}

struct QueuedProducerCommand<Input> {
    socket: Arc<dyn SessionBrokerSocketLike>,
    message: SessionServerMessage<String, Input>,
    reservation: CommandReservation,
}

enum ProducerHelloState {
    Initiated {
        request: SessionBrokerHelloChallengeRequest,
    },
    Answering,
    Pending(Box<crate::PendingSessionBrokerHello<ProducerGrant>>),
}

struct ConnectionState<Info, State, Input, ResultValue> {
    socket: Option<Arc<dyn SessionBrokerSocketLike>>,
    active_socket: Option<Arc<dyn SessionBrokerSocketLike>>,
    bridge: Option<Arc<dyn SessionBrokerConnectionBridge<Input, ResultValue>>>,
    queued: VecDeque<QueuedProducerCommand<Input>>,
    executing: Vec<(u64, CommandReservation)>,
    next_execution_id: u64,
    draining: bool,
    reconnect_scheduled: bool,
    stopped: bool,
    registration: SessionRegistration<Info>,
    snapshot: SessionSnapshot<State>,
    lifecycle_epoch: u64,
    heartbeat_epoch: u64,
    producer_hello: Option<(Arc<dyn SessionBrokerSocketLike>, ProducerHelloState)>,
}

struct SessionBrokerConnectionInner<Info, State, Input, ResultValue> {
    options: SessionBrokerConnectionOptions<Info, State, Input, ResultValue>,
    limits: SessionBrokerLimits,
    queued_count_budget: ResourceBudget,
    queued_byte_budget: ResourceBudget,
    state: Mutex<ConnectionState<Info, State, Input, ResultValue>>,
}

pub struct SessionBrokerConnection<Info, State, Input, ResultValue> {
    inner: Arc<SessionBrokerConnectionInner<Info, State, Input, ResultValue>>,
    pub limits: SessionBrokerLimits,
}

impl<Info, State, Input, ResultValue> Clone
    for SessionBrokerConnection<Info, State, Input, ResultValue>
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            limits: self.limits,
        }
    }
}

impl<Info, State, Input, ResultValue> SessionBrokerConnection<Info, State, Input, ResultValue>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    Input: Clone + Serialize + Send + Sync + 'static,
    ResultValue: Clone + Serialize + Send + 'static,
{
    pub fn new(
        options: SessionBrokerConnectionOptions<Info, State, Input, ResultValue>,
    ) -> Result<Self, SessionBrokerConnectionError> {
        let limits = resolve_session_broker_limits(&options.limit_options)
            .map_err(|error| SessionBrokerConnectionError::Limits(error.to_string()))?;
        let inner = Arc::new(SessionBrokerConnectionInner {
            queued_count_budget: ResourceBudget::with_code(
                limits.max_pre_bridge_commands,
                "maxPreBridgeCommands",
                BrokerCapacityCode::QueueFull,
            ),
            queued_byte_budget: ResourceBudget::with_code(
                limits.max_queued_command_bytes,
                "maxQueuedCommandBytes",
                BrokerCapacityCode::QueueFull,
            ),
            state: Mutex::new(ConnectionState {
                socket: None,
                active_socket: None,
                bridge: options.bridge.clone(),
                queued: VecDeque::new(),
                executing: Vec::new(),
                next_execution_id: 1,
                draining: false,
                reconnect_scheduled: false,
                stopped: false,
                registration: options.registration.clone(),
                snapshot: options.snapshot.clone(),
                lifecycle_epoch: 0,
                heartbeat_epoch: 0,
                producer_hello: None,
            }),
            options,
            limits,
        });
        Ok(Self { inner, limits })
    }

    pub fn start(&self) -> Result<(), SessionBrokerConnectionError> {
        self.inner.connect()
    }

    pub fn stop(&self) {
        self.inner.stop();
    }

    #[must_use]
    pub fn registration(&self) -> SessionRegistration<Info> {
        self.inner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .registration
            .clone()
    }

    pub fn set_bridge(
        &self,
        bridge: Option<Arc<dyn SessionBrokerConnectionBridge<Input, ResultValue>>>,
    ) {
        self.inner.set_bridge(bridge);
    }

    pub fn replace_session(
        &self,
        registration: SessionRegistration<Info>,
        snapshot: SessionSnapshot<State>,
    ) -> Result<(), SessionBrokerConnectionError> {
        self.inner.replace_session(registration, snapshot)
    }

    pub fn update_snapshot(
        &self,
        snapshot: SessionSnapshot<State>,
    ) -> Result<(), SessionBrokerConnectionError> {
        self.inner.update_snapshot(snapshot)
    }
}

pub fn create_session_broker_connection<Info, State, Input, ResultValue>(
    options: SessionBrokerConnectionOptions<Info, State, Input, ResultValue>,
) -> Result<SessionBrokerConnection<Info, State, Input, ResultValue>, SessionBrokerConnectionError>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    Input: Clone + Serialize + Send + Sync + 'static,
    ResultValue: Clone + Serialize + Send + 'static,
{
    SessionBrokerConnection::new(options)
}

impl<Info, State, Input, ResultValue> SessionBrokerConnectionInner<Info, State, Input, ResultValue>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    Input: Clone + Serialize + Send + Sync + 'static,
    ResultValue: Clone + Serialize + Send + 'static,
{
    fn socket_is(
        candidate: &Option<Arc<dyn SessionBrokerSocketLike>>,
        socket: &Arc<dyn SessionBrokerSocketLike>,
    ) -> bool {
        candidate
            .as_ref()
            .is_some_and(|candidate| Arc::ptr_eq(candidate, socket))
    }

    fn connect(self: &Arc<Self>) -> Result<(), SessionBrokerConnectionError> {
        {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.stopped || state.socket.is_some() {
                return Ok(());
            }
        }
        let socket = (self.options.create_socket)(&self.options.url)?;
        let epoch = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.stopped || state.socket.is_some() {
                drop(state);
                socket.close(None, None);
                return Ok(());
            }
            state.lifecycle_epoch = state.lifecycle_epoch.wrapping_add(1);
            state.socket = Some(Arc::clone(&socket));
            state.active_socket = None;
            state.producer_hello = None;
            state.lifecycle_epoch
        };
        self.install_socket_handlers(&socket);
        if self.options.producer_authentication.is_some() {
            self.spawn_handshake_timeout(Arc::clone(&socket), epoch);
        }
        Ok(())
    }

    fn install_socket_handlers(self: &Arc<Self>, socket: &Arc<dyn SessionBrokerSocketLike>) {
        let weak = Arc::downgrade(self);
        let opened = Arc::clone(socket);
        socket.set_on_open(Some(Arc::new(move || {
            if let Some(inner) = weak.upgrade() {
                inner.on_open(&opened);
            }
        })));

        let weak = Arc::downgrade(self);
        let messaged = Arc::clone(socket);
        socket.set_on_message(Some(Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_message(&messaged, event);
            }
        })));

        let weak = Arc::downgrade(self);
        let closed = Arc::clone(socket);
        socket.set_on_close(Some(Arc::new(move |event| {
            if let Some(inner) = weak.upgrade() {
                inner.on_close(&closed, event);
            }
        })));

        let weak = Arc::downgrade(self);
        let errored = Arc::clone(socket);
        socket.set_on_error(Some(Arc::new(move || {
            if weak.upgrade().is_some() {
                errored.close(None, None);
            }
        })));
    }

    fn stop(&self) {
        let socket = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.stopped = true;
            state.lifecycle_epoch = state.lifecycle_epoch.wrapping_add(1);
            state.heartbeat_epoch = state.heartbeat_epoch.wrapping_add(1);
            state.reconnect_scheduled = false;
            for queued in state.queued.drain(..) {
                queued.reservation.release();
            }
            for (_, reservation) in state.executing.drain(..) {
                reservation.release();
            }
            state.active_socket = None;
            state.producer_hello = None;
            state.socket.take()
        };
        if let Some(socket) = socket {
            socket.close(None, None);
        }
    }

    fn set_bridge(
        self: &Arc<Self>,
        bridge: Option<Arc<dyn SessionBrokerConnectionBridge<Input, ResultValue>>>,
    ) {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .bridge = bridge;
        self.maybe_start_drain();
    }

    fn replace_session(
        &self,
        registration: SessionRegistration<Info>,
        snapshot: SessionSnapshot<State>,
    ) -> Result<(), SessionBrokerConnectionError> {
        let old_session_id = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .registration
            .session_id
            .clone();
        if self.options.producer_authentication.is_some()
            && registration.session_id != old_session_id
        {
            return Err(SessionBrokerConnectionError::Protocol(
                "invalid-app-payload",
            ));
        }
        self.send_value(json!({
            "type": "register",
            "registration": registration,
            "snapshot": snapshot,
        }))?;
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.registration = registration;
        state.snapshot = snapshot;
        Ok(())
    }

    fn update_snapshot(
        &self,
        snapshot: SessionSnapshot<State>,
    ) -> Result<(), SessionBrokerConnectionError> {
        let session_id = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.snapshot = snapshot.clone();
            state.registration.session_id.clone()
        };
        self.send_value(json!({
            "type": "snapshot",
            "sessionId": session_id,
            "snapshot": snapshot,
        }))
    }

    fn on_open(self: &Arc<Self>, socket: &Arc<dyn SessionBrokerSocketLike>) {
        let current = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            !state.stopped && Self::socket_is(&state.socket, socket)
        };
        if !current {
            return;
        }
        if let Some(authentication) = &self.options.producer_authentication {
            let hello_options = SessionBrokerHelloClientOptions {
                app_id: authentication.app_id.clone(),
                app_revision: authentication.app_revision,
                endpoint: self.options.url.clone(),
                credential: authentication.credential.clone(),
                daemon: authentication.daemon.clone(),
                crypto: Arc::clone(&authentication.crypto),
            };
            let Ok(request) = create_session_broker_hello_request(&hello_options) else {
                socket.close(Some(1008), Some("Session broker authentication failed."));
                return;
            };
            self.state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .producer_hello = Some((
                Arc::clone(socket),
                ProducerHelloState::Initiated {
                    request: request.clone(),
                },
            ));
            if socket
                .send(&json!({"type": "hello-init", "hello": request}).to_string())
                .is_err()
            {
                socket.close(Some(1008), Some("Session broker authentication failed."));
            }
        } else {
            self.activate_socket(socket);
        }
    }

    fn activate_socket(self: &Arc<Self>, socket: &Arc<dyn SessionBrokerSocketLike>) {
        let (registration, snapshot, heartbeat_epoch) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !Self::socket_is(&state.socket, socket)
                || Self::socket_is(&state.active_socket, socket)
            {
                return;
            }
            state.active_socket = Some(Arc::clone(socket));
            state.producer_hello = None;
            state.heartbeat_epoch = state.heartbeat_epoch.wrapping_add(1);
            (
                state.registration.clone(),
                state.snapshot.clone(),
                state.heartbeat_epoch,
            )
        };
        self.spawn_heartbeat(Arc::clone(socket), heartbeat_epoch);
        if let Some(callback) = &self.options.on_connected {
            callback();
        }
        if self
            .send_to_socket(
                socket,
                json!({
                    "type": "register",
                    "registration": registration,
                    "snapshot": snapshot,
                }),
            )
            .is_err()
        {
            socket.close(None, None);
            return;
        }
        self.maybe_start_drain();
    }

    fn on_message(
        self: &Arc<Self>,
        socket: &Arc<dyn SessionBrokerSocketLike>,
        event: SessionBrokerSocketMessageEvent,
    ) {
        let (current, authenticated) = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            (
                !state.stopped && Self::socket_is(&state.socket, socket),
                Self::socket_is(&state.active_socket, socket),
            )
        };
        if !current {
            return;
        }
        let Some(message) = event.data.as_str() else {
            socket.close(
                Some(1003),
                Some("Session broker accepts text messages only."),
            );
            return;
        };
        if u64::try_from(message.len()).unwrap_or(u64::MAX) > self.limits.max_ws_message_bytes {
            socket.close(
                Some(1009),
                Some("Message exceeds the session broker size limit."),
            );
            return;
        }
        if self.options.producer_authentication.is_some() && !authenticated {
            self.handle_producer_hello(socket, message);
            return;
        }
        let parsed = (|| {
            let raw = parse_session_broker_json_text(&Value::String(message.into()))?;
            let input_bytes = raw
                .get("input")
                .ok_or(crate::BrokerProtocolError {
                    code: BrokerProtocolFailureCode::InvalidAppPayload,
                })
                .and_then(command_value_bytes)?;
            if input_bytes > self.limits.max_command_input_bytes {
                return Err(crate::BrokerProtocolError {
                    code: BrokerProtocolFailureCode::InvalidAppPayload,
                });
            }
            let parsed = self.options.protocol_parsers.parse_server_message(&raw)?;
            if command_value_bytes(&parsed.input)? > self.limits.max_command_input_bytes {
                return Err(crate::BrokerProtocolError {
                    code: BrokerProtocolFailureCode::InvalidAppPayload,
                });
            }
            Ok(parsed)
        })();
        match parsed {
            Ok(message) => self.handle_server_message(socket, message),
            Err(_) => socket.close(Some(1008), Some("Malformed session broker command.")),
        }
    }

    fn handle_producer_hello(
        self: &Arc<Self>,
        socket: &Arc<dyn SessionBrokerSocketLike>,
        message: &str,
    ) {
        if u64::try_from(message.len()).unwrap_or(u64::MAX) > self.limits.max_ws_message_bytes {
            socket.close(
                Some(1009),
                Some("Session broker authentication message exceeded its limit."),
            );
            return;
        }
        let result = (|| {
            let value =
                parse_session_broker_json_text(&Value::String(message.into())).map_err(|_| ())?;
            let progress = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some((hello_socket, progress)) = state.producer_hello.as_mut() else {
                    return Err(());
                };
                if !Arc::ptr_eq(hello_socket, socket) {
                    return Err(());
                }
                std::mem::replace(progress, ProducerHelloState::Answering)
            };
            match progress {
                ProducerHelloState::Initiated { request } => {
                    let record = exact_record(&value, &["type", "challenge"])?;
                    if record.get("type").and_then(Value::as_str) != Some("hello-challenge") {
                        return Err(());
                    }
                    let challenge =
                        parse_session_broker_hello_challenge(record.get("challenge").ok_or(())?)
                            .map_err(|_| ())?;
                    let authentication = self.options.producer_authentication.as_ref().ok_or(())?;
                    let pending = answer_session_broker_hello_challenge(
                        &SessionBrokerHelloClientOptions {
                            app_id: authentication.app_id.clone(),
                            app_revision: authentication.app_revision,
                            endpoint: self.options.url.clone(),
                            credential: authentication.credential.clone(),
                            daemon: authentication.daemon.clone(),
                            crypto: Arc::clone(&authentication.crypto),
                        },
                        &request,
                        &challenge,
                    )
                    .map_err(|_| ())?;
                    let current = {
                        let mut state = self
                            .state
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        if !Self::socket_is(&state.socket, socket)
                            || socket.ready_state() != self.options.open_state
                        {
                            return Ok(());
                        }
                        state.producer_hello = Some((
                            Arc::clone(socket),
                            ProducerHelloState::Pending(Box::new(pending.clone())),
                        ));
                        true
                    };
                    if current {
                        socket
                            .send(
                                &json!({"type": "hello-proof", "proof": pending.proof}).to_string(),
                            )
                            .map_err(|_| ())?;
                    }
                }
                ProducerHelloState::Pending(pending) => {
                    let record = exact_record(&value, &["type", "ack"])?;
                    if record.get("type").and_then(Value::as_str) != Some("hello-ack") {
                        return Err(());
                    }
                    verify_producer_hello_ack(&pending, record.get("ack").ok_or(())?)
                        .map_err(|_| ())?;
                    self.activate_socket(socket);
                }
                ProducerHelloState::Answering => return Err(()),
            }
            Ok(())
        })();
        if result.is_err() {
            socket.close(Some(1008), Some("Session broker authentication failed."));
        }
    }

    fn on_close(
        self: &Arc<Self>,
        socket: &Arc<dyn SessionBrokerSocketLike>,
        event: SessionBrokerSocketCloseEvent,
    ) {
        let (was_authenticated, stopped) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let authenticated = Self::socket_is(&state.active_socket, socket);
            if Self::socket_is(&state.socket, socket) {
                state.socket = None;
                state.active_socket = None;
                state.producer_hello = None;
                state.heartbeat_epoch = state.heartbeat_epoch.wrapping_add(1);
            }
            let mut retained = VecDeque::new();
            while let Some(queued) = state.queued.pop_front() {
                if Arc::ptr_eq(&queued.socket, socket) {
                    queued.reservation.release();
                } else {
                    retained.push_back(queued);
                }
            }
            state.queued = retained;
            (authenticated, state.stopped)
        };
        if stopped {
            return;
        }
        let directive = self.options.resolve_close.as_ref().map_or(
            SessionBrokerConnectionCloseDirective {
                reconnect: Some(true),
                warning: None,
            },
            |resolve| {
                resolve(SessionBrokerSocketCloseEvent {
                    authenticated: Some(was_authenticated),
                    ..event
                })
            },
        );
        if let Some(warning) = directive.warning.as_deref()
            && let Some(callback) = &self.options.on_warning
        {
            callback(warning);
        }
        if directive.reconnect != Some(false) {
            self.schedule_reconnect(self.options.reconnect_delay_ms);
        }
    }

    fn schedule_reconnect(self: &Arc<Self>, delay_ms: u64) {
        let epoch = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.reconnect_scheduled || state.stopped {
                return;
            }
            state.reconnect_scheduled = true;
            state.lifecycle_epoch
        };
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(delay_ms));
            let Some(inner) = weak.upgrade() else {
                return;
            };
            {
                let mut state = inner
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state.reconnect_scheduled = false;
                if state.stopped || state.lifecycle_epoch != epoch {
                    return;
                }
            }
            let prepared = inner
                .options
                .prepare_reconnect
                .as_ref()
                .map_or(Ok(()), |prepare| prepare());
            let stopped = inner
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .stopped;
            if stopped {
                return;
            }
            match prepared {
                Ok(()) => {
                    let _ = inner.connect();
                }
                Err(error) => {
                    if let Some(callback) = &inner.options.on_warning {
                        callback(&error);
                    }
                    inner.schedule_reconnect(inner.options.reconnect_delay_ms);
                }
            }
        });
    }

    fn spawn_handshake_timeout(
        self: &Arc<Self>,
        socket: Arc<dyn SessionBrokerSocketLike>,
        epoch: u64,
    ) {
        let weak = Arc::downgrade(self);
        let timeout = self.limits.max_handshake_duration_ms;
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(timeout));
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let should_close = {
                let state = inner
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                !state.stopped
                    && state.lifecycle_epoch == epoch
                    && Self::socket_is(&state.socket, &socket)
                    && !Self::socket_is(&state.active_socket, &socket)
            };
            if should_close {
                socket.close(Some(1008), Some("Session broker authentication timed out."));
            }
        });
    }

    fn spawn_heartbeat(
        self: &Arc<Self>,
        socket: Arc<dyn SessionBrokerSocketLike>,
        heartbeat_epoch: u64,
    ) {
        let weak = Arc::downgrade(self);
        let interval = self.options.heartbeat_interval_ms;
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(interval));
                let Some(inner) = weak.upgrade() else {
                    return;
                };
                let session_id = {
                    let state = inner
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if state.stopped
                        || state.heartbeat_epoch != heartbeat_epoch
                        || !Self::socket_is(&state.active_socket, &socket)
                    {
                        return;
                    }
                    state.registration.session_id.clone()
                };
                if inner
                    .send_to_socket(
                        &socket,
                        json!({"type": "heartbeat", "sessionId": session_id}),
                    )
                    .is_err()
                {
                    socket.close(None, None);
                    return;
                }
            }
        });
    }

    fn send_value(&self, value: Value) -> Result<(), SessionBrokerConnectionError> {
        let socket = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_socket
            .clone();
        if let Some(socket) = socket {
            self.send_to_socket(&socket, value)?;
        }
        Ok(())
    }

    fn send_to_socket(
        &self,
        socket: &Arc<dyn SessionBrokerSocketLike>,
        value: Value,
    ) -> Result<bool, SessionBrokerConnectionError> {
        let current = {
            let state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            Self::socket_is(&state.socket, socket)
                && Self::socket_is(&state.active_socket, socket)
                && socket.ready_state() == self.options.open_state
        };
        if !current {
            return Ok(false);
        }
        socket
            .send(&value.to_string())
            .map(|()| true)
            .map_err(SessionBrokerConnectionError::Socket)
    }

    fn handle_server_message(
        self: &Arc<Self>,
        socket: &Arc<dyn SessionBrokerSocketLike>,
        message: SessionServerMessage<String, Input>,
    ) {
        let count = match self.queued_count_budget.reserve(1) {
            Ok(reservation) => reservation,
            Err(_) => {
                self.reject_queue_pressure(socket, &message.request_id);
                return;
            }
        };
        let bytes = command_value_bytes(&message)
            .ok()
            .and_then(|bytes| bytes.checked_add(PRODUCER_COMMAND_OVERHEAD_BYTES))
            .and_then(|bytes| self.queued_byte_budget.reserve(bytes).ok());
        let Some(bytes) = bytes else {
            count.release();
            self.reject_queue_pressure(socket, &message.request_id);
            return;
        };
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .queued
            .push_back(QueuedProducerCommand {
                socket: Arc::clone(socket),
                message,
                reservation: CommandReservation { count, bytes },
            });
        self.maybe_start_drain();
    }

    fn reject_queue_pressure(&self, socket: &Arc<dyn SessionBrokerSocketLike>, request_id: &str) {
        if self
            .send_to_socket(
                socket,
                json!({
                    "type": "command-result",
                    "requestId": request_id,
                    "ok": false,
                    "error": "queue-full",
                }),
            )
            .is_err()
        {
            socket.close(Some(1013), Some("Session broker queue pressure exceeded."));
        }
    }

    fn maybe_start_drain(self: &Arc<Self>) {
        let should_start = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if state.draining
                || state.bridge.is_none()
                || state.socket.is_none()
                || state.queued.is_empty()
            {
                false
            } else {
                state.draining = true;
                true
            }
        };
        if !should_start {
            return;
        }
        let weak = Arc::downgrade(self);
        std::thread::spawn(move || {
            if let Some(inner) = weak.upgrade() {
                inner.drain_commands();
            }
        });
    }

    fn drain_commands(self: &Arc<Self>) {
        loop {
            let next = {
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let Some(bridge) = state.bridge.clone() else {
                    state.draining = false;
                    return;
                };
                let Some(current_socket) = state.socket.clone() else {
                    state.draining = false;
                    return;
                };
                let index = state
                    .queued
                    .iter()
                    .position(|entry| Arc::ptr_eq(&entry.socket, &current_socket));
                let Some(index) = index else {
                    state.draining = false;
                    return;
                };
                let entry = state.queued.remove(index).expect("queued index exists");
                if !Self::socket_is(&state.socket, &entry.socket)
                    || entry.socket.ready_state() != self.options.open_state
                {
                    entry.reservation.release();
                    state.draining = false;
                    return;
                }
                let execution_id = state.next_execution_id;
                state.next_execution_id = state.next_execution_id.wrapping_add(1);
                state
                    .executing
                    .push((execution_id, entry.reservation.clone()));
                Some((bridge, entry, execution_id))
            };
            let Some((bridge, entry, execution_id)) = next else {
                return;
            };
            self.execute_server_message(&bridge, &entry.socket, &entry.message);
            entry.reservation.release();
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            state.executing.retain(|(id, _)| *id != execution_id);
        }
    }

    fn execute_server_message(
        &self,
        bridge: &Arc<dyn SessionBrokerConnectionBridge<Input, ResultValue>>,
        socket: &Arc<dyn SessionBrokerSocketLike>,
        message: &SessionServerMessage<String, Input>,
    ) {
        let dispatched = catch_unwind(AssertUnwindSafe(|| {
            bridge.dispatch_command(message.clone())
        }))
        .unwrap_or_else(|_| Err("Unknown broker connection error.".into()));
        match dispatched {
            Ok(raw) => {
                let parsed = (|| {
                    if command_value_bytes(&raw)? > self.limits.max_command_result_bytes {
                        return Err(crate::BrokerProtocolError {
                            code: BrokerProtocolFailureCode::InvalidAppPayload,
                        });
                    }
                    let raw_value =
                        serde_json::to_value(&raw).map_err(|_| crate::BrokerProtocolError {
                            code: BrokerProtocolFailureCode::InvalidAppPayload,
                        })?;
                    let parsed = self.options.protocol_parsers.parse_command_result(
                        &message.command,
                        message.command_version.unwrap_or(1),
                        &raw_value,
                    )?;
                    if command_value_bytes(&parsed)? > self.limits.max_command_result_bytes {
                        return Err(crate::BrokerProtocolError {
                            code: BrokerProtocolFailureCode::InvalidAppPayload,
                        });
                    }
                    serde_json::to_value(parsed).map_err(|_| crate::BrokerProtocolError {
                        code: BrokerProtocolFailureCode::InvalidAppPayload,
                    })
                })();
                match parsed {
                    Ok(result) => {
                        let sent = self.send_to_socket(
                            socket,
                            json!({
                                "type": "command-result",
                                "requestId": message.request_id,
                                "ok": true,
                                "result": result,
                            }),
                        );
                        if matches!(sent, Ok(true)) {
                            // A lifecycle callback must not unwind out of the
                            // FIFO worker and strand its remaining reservations.
                            let _ = catch_unwind(AssertUnwindSafe(|| {
                                bridge.command_result_queued(&message.request_id);
                            }));
                        }
                    }
                    Err(_) => {
                        socket.close(Some(1008), Some("Malformed session broker command result."))
                    }
                }
            }
            Err(error) => {
                let _ = self.send_to_socket(
                    socket,
                    json!({
                        "type": "command-result",
                        "requestId": message.request_id,
                        "ok": false,
                        "error": if error.is_empty() {
                            "Unknown broker connection error."
                        } else {
                            &error
                        },
                    }),
                );
            }
        }
    }
}

fn command_value_bytes(value: &impl Serialize) -> Result<u64, crate::BrokerProtocolError> {
    serde_json::to_vec(value)
        .ok()
        .and_then(|bytes| u64::try_from(bytes.len()).ok())
        .ok_or(crate::BrokerProtocolError {
            code: BrokerProtocolFailureCode::InvalidAppPayload,
        })
}

fn exact_record<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, Value>, ()> {
    let record = value.as_object().ok_or(())?;
    let expected = keys.iter().copied().collect::<BTreeSet<_>>();
    if record.len() != expected.len()
        || record.keys().any(|key| {
            matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                || !expected.contains(key.as_str())
        })
    {
        return Err(());
    }
    Ok(record)
}

#[cfg(test)]
mod tests {
    use std::sync::Condvar;
    use std::sync::atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering};
    use std::time::Instant;

    use ed25519_dalek::SigningKey;
    use serde::{Deserialize, Serialize};

    use super::*;
    use crate::{
        ProducerOperation, SESSION_BROKER_REGISTRATION_VERSION, SESSION_BROKER_SIGNATURE_ALGORITHM,
        SessionBrokerAppParserRegistry, SessionBrokerAuthenticator,
        SessionBrokerAuthenticatorOptions, SessionBrokerAuthorityCredential,
        SessionBrokerCommandParsers, SessionBrokerDaemonIdentity, SessionBrokerSocketCloseHandler,
        SessionBrokerSocketErrorHandler, SessionBrokerSocketMessageHandler,
        SessionBrokerSocketOpenHandler, create_session_broker_protocol_parsers,
    };

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TestInfo {
        title: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TestState {
        selected_index: u64,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestInput {
        summary: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestResult {
        ok: bool,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct CloseRecord {
        code: Option<u16>,
        reason: Option<String>,
    }

    #[derive(Default)]
    struct TestSocketHandlers {
        open: Option<SessionBrokerSocketOpenHandler>,
        message: Option<SessionBrokerSocketMessageHandler>,
        close: Option<SessionBrokerSocketCloseHandler>,
        error: Option<SessionBrokerSocketErrorHandler>,
    }

    #[derive(Default)]
    struct TestSocket {
        ready_state: AtomicU16,
        sent: Mutex<Vec<String>>,
        throw_on_send: AtomicBool,
        last_close: Mutex<Option<CloseRecord>>,
        handlers: Mutex<TestSocketHandlers>,
    }

    impl TestSocket {
        fn emit_open(&self) {
            self.ready_state.store(1, Ordering::Release);
            let handler = self
                .handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .open
                .clone();
            if let Some(handler) = handler {
                handler();
            }
        }

        fn emit_message(&self, data: Value) {
            let handler = self
                .handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .message
                .clone();
            if let Some(handler) = handler {
                handler(SessionBrokerSocketMessageEvent { data });
            }
        }

        fn emit_text(&self, data: impl Into<String>) {
            self.emit_message(Value::String(data.into()));
        }

        fn emit_close(&self, code: u16, reason: &str) {
            self.ready_state.store(3, Ordering::Release);
            let handler = self
                .handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .close
                .clone();
            if let Some(handler) = handler {
                handler(SessionBrokerSocketCloseEvent {
                    code,
                    reason: reason.into(),
                    authenticated: None,
                });
            }
        }

        fn set_ready(&self, ready: u16) {
            self.ready_state.store(ready, Ordering::Release);
        }

        fn sent_values(&self) -> Vec<Value> {
            self.sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
                .map(|value| serde_json::from_str(value).unwrap())
                .collect()
        }

        fn close_record(&self) -> Option<CloseRecord> {
            self.last_close
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl SessionBrokerSocketLike for TestSocket {
        fn ready_state(&self) -> u16 {
            self.ready_state.load(Ordering::Acquire)
        }

        fn send(&self, data: &str) -> Result<(), String> {
            if self.throw_on_send.load(Ordering::Acquire) {
                return Err("socket exploded".into());
            }
            self.sent
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(data.into());
            Ok(())
        }

        fn close(&self, code: Option<u16>, reason: Option<&str>) {
            *self
                .last_close
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(CloseRecord {
                code,
                reason: reason.map(str::to_owned),
            });
            self.emit_close(code.unwrap_or(1000), reason.unwrap_or(""));
        }

        fn set_on_open(&self, handler: Option<SessionBrokerSocketOpenHandler>) {
            self.handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .open = handler;
        }

        fn set_on_message(&self, handler: Option<SessionBrokerSocketMessageHandler>) {
            self.handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .message = handler;
        }

        fn set_on_close(&self, handler: Option<SessionBrokerSocketCloseHandler>) {
            self.handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .close = handler;
        }

        fn set_on_error(&self, handler: Option<SessionBrokerSocketErrorHandler>) {
            self.handlers
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .error = handler;
        }
    }

    type TestParsers = SessionBrokerProtocolParsers<TestInfo, TestState, TestInput, TestResult>;
    type TestConnection = SessionBrokerConnection<TestInfo, TestState, TestInput, TestResult>;

    fn parsers() -> Arc<TestParsers> {
        Arc::new(
            create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry {
                broker_revision: None,
                app_revision: 1,
                features: Vec::new(),
                parse_registration: Arc::new(|value| serde_json::from_value(value.clone()).ok()),
                parse_snapshot: Arc::new(|value| serde_json::from_value(value.clone()).ok()),
                commands: vec![SessionBrokerCommandParsers {
                    command: "annotate".into(),
                    version: 1,
                    parse_input: Arc::new(|value| {
                        let record = value.as_object()?;
                        (record.len() == 1).then_some(())?;
                        Some(TestInput {
                            summary: record.get("summary")?.as_str()?.into(),
                        })
                    }),
                    parse_result: Arc::new(|value| {
                        let record = value.as_object()?;
                        (record.len() == 1 && record.get("ok") == Some(&Value::Bool(true)))
                            .then_some(TestResult { ok: true })
                    }),
                }],
            })
            .unwrap(),
        )
    }

    fn registration() -> SessionRegistration<TestInfo> {
        SessionRegistration {
            registration_version: SESSION_BROKER_REGISTRATION_VERSION,
            session_id: "session-1".into(),
            pid: 123,
            cwd: "/repo".into(),
            repo_root: None,
            launched_at: "2026-04-15T00:00:00.000Z".into(),
            terminal: None,
            info: TestInfo {
                title: "repo working tree".into(),
            },
        }
    }

    fn snapshot(index: u64) -> SessionSnapshot<TestState> {
        SessionSnapshot {
            updated_at: format!("2026-04-15T00:00:{index:02}.000Z"),
            state: TestState {
                selected_index: index,
            },
        }
    }

    fn socket_factory(socket: Arc<TestSocket>) -> SocketFactory {
        Arc::new(move |_| {
            let socket: Arc<dyn SessionBrokerSocketLike> = socket.clone();
            Ok(socket)
        })
    }

    fn collecting_factory(sockets: Arc<Mutex<Vec<Arc<TestSocket>>>>) -> SocketFactory {
        Arc::new(move |_| {
            let socket = Arc::new(TestSocket::default());
            sockets
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(Arc::clone(&socket));
            let socket: Arc<dyn SessionBrokerSocketLike> = socket;
            Ok(socket)
        })
    }

    fn options(
        socket: Arc<TestSocket>,
    ) -> SessionBrokerConnectionOptions<TestInfo, TestState, TestInput, TestResult> {
        SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            socket_factory(socket),
            registration(),
            snapshot(0),
            parsers(),
        )
    }

    fn command(request_id: &str, summary: &str) -> String {
        json!({
            "type": "command",
            "requestId": request_id,
            "command": "annotate",
            "input": {"summary": summary},
        })
        .to_string()
    }

    fn wait_until(condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !condition() {
            assert!(Instant::now() < deadline, "condition did not settle");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn now_millis() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }

    #[derive(Default)]
    struct Gate {
        open: Mutex<bool>,
        changed: Condvar,
    }

    impl Gate {
        fn wait(&self) {
            let mut open = self
                .open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            while !*open {
                open = self
                    .changed
                    .wait(open)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        }

        fn open(&self) {
            *self
                .open
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
            self.changed.notify_all();
        }
    }

    #[test]
    fn registers_on_open_and_sends_snapshot_updates() {
        let socket = Arc::new(TestSocket::default());
        let connection = TestConnection::new(options(Arc::clone(&socket))).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        assert_eq!(socket.sent_values()[0]["type"], "register");

        connection.update_snapshot(snapshot(1)).unwrap();
        let messages = socket.sent_values();
        assert_eq!(messages[1]["type"], "snapshot");
        assert_eq!(messages[1]["snapshot"]["state"]["selectedIndex"], 1);
        connection.stop();
    }

    #[test]
    fn preserves_socket_factory_failure_and_permits_later_start() {
        let socket = Arc::new(TestSocket::default());
        let attempts = Arc::new(AtomicUsize::new(0));
        let factory: SocketFactory = Arc::new({
            let socket = Arc::clone(&socket);
            let attempts = Arc::clone(&attempts);
            move |_| {
                if attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                    return Err(SessionBrokerConnectionError::Socket(
                        "socket factory".into(),
                    ));
                }
                let socket: Arc<dyn SessionBrokerSocketLike> = socket.clone();
                Ok(socket)
            }
        });
        let connection = TestConnection::new(SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            factory,
            registration(),
            snapshot(0),
            parsers(),
        ))
        .unwrap();
        assert_eq!(
            connection.start(),
            Err(SessionBrokerConnectionError::Socket(
                "socket factory".into()
            ))
        );
        connection.start().unwrap();
        socket.emit_open();
        assert_eq!(attempts.load(Ordering::Acquire), 2);
        assert_eq!(socket.sent_values()[0]["type"], "register");
        connection.stop();
    }

    #[test]
    fn withholds_registration_and_replacements_until_producer_authentication() {
        let socket = Arc::new(TestSocket::default());
        let now = now_millis();
        let key = SigningKey::from_bytes(&[61; 32]);
        let grant = ProducerGrant {
            base: crate::BrokerGrantBase {
                app_id: "dev.example".into(),
                principal_id: "producer-1".into(),
                key_id: "producer-key-1".into(),
                grant_id: "producer-grant-1".into(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: now.saturating_sub(1_000),
                expires_at: now.saturating_add(60_000),
                revocation_id: "producer-revocation-1".into(),
                may_delegate: false,
                session_id: None,
            },
            operations: vec![ProducerOperation::Register, ProducerOperation::Reconnect],
        };
        let mut options = options(Arc::clone(&socket));
        options.producer_authentication = Some(SessionBrokerProducerAuthentication::native(
            "dev.example",
            1,
            SessionBrokerClientCredential {
                grant,
                private_key: key.clone(),
            },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: key.verifying_key(),
            },
        ));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        connection.update_snapshot(snapshot(2)).unwrap();
        connection
            .replace_session(registration(), snapshot(0))
            .unwrap();
        let messages = socket.sent_values();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["type"], "hello-init");
        connection.stop();
    }

    #[test]
    fn keeps_previous_registration_when_replacement_send_fails() {
        let socket = Arc::new(TestSocket::default());
        let connection = TestConnection::new(options(Arc::clone(&socket))).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        socket.throw_on_send.store(true, Ordering::Release);
        let mut replacement = registration();
        replacement.session_id = "session-2".into();
        assert_eq!(
            connection.replace_session(replacement, snapshot(2)),
            Err(SessionBrokerConnectionError::Socket(
                "socket exploded".into()
            ))
        );
        assert_eq!(connection.registration().session_id, "session-1");
        connection.stop();
    }

    #[test]
    fn queues_commands_until_bridge_is_ready() {
        let socket = Arc::new(TestSocket::default());
        let connection = TestConnection::new(options(Arc::clone(&socket))).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        socket.emit_text(command("request-1", "Review note"));
        connection.set_bridge(Some(Arc::new(|_| Ok(TestResult { ok: true }))));
        wait_until(|| {
            socket
                .sent_values()
                .iter()
                .any(|message| message["type"] == "command-result")
        });
        assert!(
            socket
                .sent_values()
                .iter()
                .any(|message| message["type"] == "command-result" && message["ok"] == true)
        );
        connection.stop();
    }

    #[test]
    fn result_queue_notification_requires_a_successful_current_socket_send() {
        struct Bridge {
            socket: Arc<TestSocket>,
            dispatched: Arc<AtomicUsize>,
            notified: Arc<AtomicUsize>,
            mode: u8,
        }
        impl SessionBrokerConnectionBridge<TestInput, TestResult> for Bridge {
            fn dispatch_command(
                &self,
                _: SessionServerMessage<String, TestInput>,
            ) -> Result<TestResult, String> {
                match self.mode {
                    1 => self.socket.throw_on_send.store(true, Ordering::Release),
                    2 => self.socket.set_ready(3),
                    _ => {}
                }
                self.dispatched.fetch_add(1, Ordering::Release);
                if self.mode == 3 {
                    Err("command refused".into())
                } else {
                    Ok(TestResult { ok: true })
                }
            }
            fn command_result_queued(&self, request_id: &str) {
                assert!(
                    self.socket
                        .sent_values()
                        .iter()
                        .any(|value| value["type"] == "command-result"
                            && value["requestId"] == request_id
                            && value["ok"] == true)
                );
                self.notified.fetch_add(1, Ordering::Release);
            }
        }
        for mode in 0..4 {
            let socket = Arc::new(TestSocket::default());
            let connection = TestConnection::new(options(Arc::clone(&socket))).unwrap();
            connection.start().unwrap();
            socket.emit_open();
            let dispatched = Arc::new(AtomicUsize::new(0));
            let notified = Arc::new(AtomicUsize::new(0));
            connection.set_bridge(Some(Arc::new(Bridge {
                socket: Arc::clone(&socket),
                dispatched: Arc::clone(&dispatched),
                notified: Arc::clone(&notified),
                mode,
            })));
            socket.emit_text(command("quit-result", "fixture"));
            wait_until(|| {
                dispatched.load(Ordering::Acquire) == 1 && {
                    let state = connection.inner.state.lock().unwrap();
                    !state.draining && state.executing.is_empty()
                }
            });
            assert_eq!(
                notified.load(Ordering::Acquire),
                usize::from(mode == 0),
                "mode {mode}"
            );
            connection.stop();
        }
    }

    #[test]
    fn parses_and_transforms_each_command_once() {
        let input_calls = Arc::new(AtomicUsize::new(0));
        let result_calls = Arc::new(AtomicUsize::new(0));
        let transforming = Arc::new(
            create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry {
                broker_revision: None,
                app_revision: 1,
                features: Vec::new(),
                parse_registration: Arc::new(|value| serde_json::from_value(value.clone()).ok()),
                parse_snapshot: Arc::new(|value| serde_json::from_value(value.clone()).ok()),
                commands: vec![SessionBrokerCommandParsers {
                    command: "annotate".into(),
                    version: 1,
                    parse_input: Arc::new({
                        let input_calls = Arc::clone(&input_calls);
                        move |value| {
                            input_calls.fetch_add(1, Ordering::AcqRel);
                            Some(TestInput {
                                summary: value["summary"].as_str()?.to_uppercase(),
                            })
                        }
                    }),
                    parse_result: Arc::new({
                        let result_calls = Arc::clone(&result_calls);
                        move |value| {
                            result_calls.fetch_add(1, Ordering::AcqRel);
                            (value["ok"] == true).then_some(TestResult { ok: true })
                        }
                    }),
                }],
            })
            .unwrap(),
        );
        let socket = Arc::new(TestSocket::default());
        let bridged = Arc::new(Mutex::new(None));
        let mut options = options(Arc::clone(&socket));
        options.protocol_parsers = transforming;
        options.bridge = Some(Arc::new({
            let bridged = Arc::clone(&bridged);
            move |message: SessionServerMessage<String, TestInput>| {
                *bridged
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(message.input.summary);
                Ok(TestResult { ok: true })
            }
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        socket.emit_text(command("request-1", "review note"));
        wait_until(|| result_calls.load(Ordering::Acquire) == 1);
        assert_eq!(
            bridged
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_deref(),
            Some("REVIEW NOTE")
        );
        assert_eq!(input_calls.load(Ordering::Acquire), 1);
        assert_eq!(result_calls.load(Ordering::Acquire), 1);
        connection.stop();
    }

    #[test]
    fn closes_malformed_commands_without_bridge_dispatch() {
        let socket = Arc::new(TestSocket::default());
        let dispatched = Arc::new(AtomicUsize::new(0));
        let mut options = options(Arc::clone(&socket));
        options.reconnect_delay_ms = 10_000;
        options.bridge = Some(Arc::new({
            let dispatched = Arc::clone(&dispatched);
            move |_| {
                dispatched.fetch_add(1, Ordering::AcqRel);
                Ok(TestResult { ok: true })
            }
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        for message in [
            Value::Null,
            json!([]),
            json!({"type": "command", "requestId": "request-1", "command": "unknown", "input": {}}),
            json!({"type": "command", "requestId": "request-1", "command": "annotate", "input": {"summary": "note", "extra": true}}),
        ] {
            socket.set_ready(1);
            socket.emit_text(message.to_string());
            assert_eq!(
                socket.close_record(),
                Some(CloseRecord {
                    code: Some(1008),
                    reason: Some("Malformed session broker command.".into())
                })
            );
        }
        assert_eq!(dispatched.load(Ordering::Acquire), 0);
        connection.stop();
    }

    #[test]
    fn enforces_text_framing_and_exact_message_ceiling_before_parsing() {
        let max_bytes = 512_u64;
        let input_calls = Arc::new(AtomicUsize::new(0));
        let bridge_calls = Arc::new(AtomicUsize::new(0));
        let bounded = Arc::new(
            create_session_broker_protocol_parsers(SessionBrokerAppParserRegistry {
                broker_revision: None,
                app_revision: 1,
                features: Vec::new(),
                parse_registration: Arc::new(|value| serde_json::from_value(value.clone()).ok()),
                parse_snapshot: Arc::new(|value| serde_json::from_value(value.clone()).ok()),
                commands: vec![SessionBrokerCommandParsers {
                    command: "annotate".into(),
                    version: 1,
                    parse_input: Arc::new({
                        let input_calls = Arc::clone(&input_calls);
                        move |value| {
                            input_calls.fetch_add(1, Ordering::AcqRel);
                            serde_json::from_value(value.clone()).ok()
                        }
                    }),
                    parse_result: Arc::new(|_| Some(TestResult { ok: true })),
                }],
            })
            .unwrap(),
        );
        let make_connection = |socket: Arc<TestSocket>| {
            let mut options = options(socket);
            options.protocol_parsers = Arc::clone(&bounded);
            options.bridge = Some(Arc::new({
                let bridge_calls = Arc::clone(&bridge_calls);
                move |_| {
                    bridge_calls.fetch_add(1, Ordering::AcqRel);
                    Ok(TestResult { ok: true })
                }
            }));
            options.limit_options.limits = crate::SessionBrokerLimitPatch {
                max_ws_message_bytes: Some(max_bytes),
                ..crate::SessionBrokerLimitPatch::default()
            };
            options.reconnect_delay_ms = 10_000;
            TestConnection::new(options).unwrap()
        };
        let socket = Arc::new(TestSocket::default());
        let connection = make_connection(Arc::clone(&socket));
        connection.start().unwrap();
        socket.emit_open();
        let empty = command("request-exact", "");
        let exact = command(
            "request-exact",
            &"x".repeat(usize::try_from(max_bytes).unwrap() - empty.len()),
        );
        assert_eq!(exact.len(), usize::try_from(max_bytes).unwrap());
        socket.emit_text(exact.clone());
        wait_until(|| bridge_calls.load(Ordering::Acquire) == 1);
        connection.stop();

        let rejected = |data: Value| {
            let socket = Arc::new(TestSocket::default());
            let connection = make_connection(Arc::clone(&socket));
            connection.start().unwrap();
            socket.emit_open();
            socket.emit_message(data);
            let close = socket.close_record();
            connection.stop();
            close
        };
        assert_eq!(
            rejected(Value::String(format!("{exact} "))).unwrap().code,
            Some(1009)
        );
        assert_eq!(
            rejected(Value::String("{".repeat(513))).unwrap().code,
            Some(1009)
        );
        assert_eq!(
            rejected(Value::String(
                json!({"type": "command", "requestId": "request-extra", "command": "annotate", "input": {"summary": "not parsed"}, "extra": true}).to_string()
            ))
            .unwrap()
            .code,
            Some(1008)
        );
        assert_eq!(rejected(json!([123, 125])).unwrap().code, Some(1003));
        assert_eq!(input_calls.load(Ordering::Acquire), 1);
        assert_eq!(bridge_calls.load(Ordering::Acquire), 1);
    }

    #[test]
    fn ignores_late_socket_callback_after_stop() {
        let socket = Arc::new(TestSocket::default());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut options = options(Arc::clone(&socket));
        options.bridge = Some(Arc::new({
            let calls = Arc::clone(&calls);
            move |_| {
                calls.fetch_add(1, Ordering::AcqRel);
                Ok(TestResult { ok: true })
            }
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        connection.stop();
        socket.emit_text(command("request-late", "late"));
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(calls.load(Ordering::Acquire), 0);
    }

    #[test]
    fn does_not_migrate_late_result_to_replacement_socket() {
        let sockets = Arc::new(Mutex::new(Vec::<Arc<TestSocket>>::new()));
        let factory: SocketFactory = Arc::new({
            let sockets = Arc::clone(&sockets);
            move |_| {
                let socket = Arc::new(TestSocket::default());
                sockets.lock().unwrap().push(Arc::clone(&socket));
                let socket: Arc<dyn SessionBrokerSocketLike> = socket;
                Ok(socket)
            }
        });
        let gate = Arc::new(Gate::default());
        let mut options = SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            factory,
            registration(),
            snapshot(0),
            parsers(),
        );
        options.reconnect_delay_ms = 1;
        options.bridge = Some(Arc::new({
            let gate = Arc::clone(&gate);
            move |_| {
                gate.wait();
                Ok(TestResult { ok: true })
            }
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        let first = Arc::clone(&sockets.lock().unwrap()[0]);
        first.emit_open();
        first.emit_text(command("request-1", "Review note"));
        first.emit_close(1000, "");
        wait_until(|| sockets.lock().unwrap().len() == 2);
        let second = Arc::clone(&sockets.lock().unwrap()[1]);
        second.emit_open();
        gate.open();
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(
            first
                .sent_values()
                .iter()
                .map(|value| value["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["register"]
        );
        assert_eq!(
            second
                .sent_values()
                .iter()
                .map(|value| value["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["register"]
        );
        connection.stop();
    }

    #[test]
    fn discards_queued_commands_when_source_socket_disconnects() {
        let sockets = Arc::new(Mutex::new(Vec::<Arc<TestSocket>>::new()));
        let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut options = SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            collecting_factory(Arc::clone(&sockets)),
            registration(),
            snapshot(0),
            parsers(),
        );
        options.reconnect_delay_ms = 1;
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        let first = Arc::clone(&sockets.lock().unwrap()[0]);
        first.emit_open();
        first.emit_text(command("request-old", "Old review note"));
        first.emit_close(1000, "");
        wait_until(|| sockets.lock().unwrap().len() == 2);
        let second = Arc::clone(&sockets.lock().unwrap()[1]);
        second.emit_open();
        connection.set_bridge(Some(Arc::new({
            let dispatched = Arc::clone(&dispatched);
            move |message: SessionServerMessage<String, TestInput>| {
                dispatched.lock().unwrap().push(message.request_id);
                Ok(TestResult { ok: true })
            }
        })));
        std::thread::sleep(Duration::from_millis(10));
        assert!(dispatched.lock().unwrap().is_empty());
        assert_eq!(
            second
                .sent_values()
                .iter()
                .map(|message| message["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["register"]
        );
        connection.stop();
    }

    #[test]
    fn stops_queued_batch_when_source_socket_disconnects() {
        let socket = Arc::new(TestSocket::default());
        let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
        let gate = Arc::new(Gate::default());
        let mut options = options(Arc::clone(&socket));
        options.reconnect_delay_ms = 1_000;
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        socket.emit_text(command("request-1", "Review note"));
        socket.emit_text(command("request-2", "Review note"));
        connection.set_bridge(Some(Arc::new({
            let dispatched = Arc::clone(&dispatched);
            let gate = Arc::clone(&gate);
            move |message: SessionServerMessage<String, TestInput>| {
                dispatched
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(message.request_id.clone());
                if message.request_id == "request-1" {
                    gate.wait();
                }
                Ok(TestResult { ok: true })
            }
        })));
        wait_until(|| !dispatched.lock().unwrap().is_empty());
        socket.emit_close(1000, "");
        gate.open();
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(&*dispatched.lock().unwrap(), &["request-1"]);
        connection.stop();
    }

    #[test]
    fn keeps_32_missing_bridge_commands_fifo_and_rejects_33rd() {
        let socket = Arc::new(TestSocket::default());
        let dispatched = Arc::new(Mutex::new(Vec::<String>::new()));
        let connection = TestConnection::new(options(Arc::clone(&socket))).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        for index in 1..=33 {
            socket.emit_text(command(
                &format!("request-{index}"),
                &format!("note-{index}"),
            ));
        }
        let overflow = socket.sent_values().pop().unwrap();
        assert_eq!(overflow["requestId"], "request-33");
        assert_eq!(overflow["error"], "queue-full");
        connection.set_bridge(Some(Arc::new({
            let dispatched = Arc::clone(&dispatched);
            move |message: SessionServerMessage<String, TestInput>| {
                dispatched.lock().unwrap().push(message.request_id);
                Ok(TestResult { ok: true })
            }
        })));
        wait_until(|| dispatched.lock().unwrap().len() == 32);
        assert_eq!(
            *dispatched.lock().unwrap(),
            (1..=32)
                .map(|index| format!("request-{index}"))
                .collect::<Vec<_>>()
        );
        connection.stop();
    }

    #[test]
    fn counts_hung_bridge_and_queued_commands_in_same_budget() {
        let socket = Arc::new(TestSocket::default());
        let gate = Arc::new(Gate::default());
        let mut options = options(Arc::clone(&socket));
        options.bridge = Some(Arc::new({
            let gate = Arc::clone(&gate);
            move |_| {
                gate.wait();
                Ok(TestResult { ok: true })
            }
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        for index in 1..=33 {
            socket.emit_text(command(
                &format!("request-{index}"),
                &format!("note-{index}"),
            ));
        }
        wait_until(|| {
            socket.sent_values().iter().any(|message| {
                message["requestId"] == "request-33" && message["error"] == "queue-full"
            })
        });
        gate.open();
        connection.stop();
    }

    #[test]
    fn retains_hung_reservation_across_disconnect_and_reconnect() {
        let sockets = Arc::new(Mutex::new(Vec::<Arc<TestSocket>>::new()));
        let gate = Arc::new(Gate::default());
        let started = Arc::new(AtomicBool::new(false));
        let mut options = SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            collecting_factory(Arc::clone(&sockets)),
            registration(),
            snapshot(0),
            parsers(),
        );
        options.reconnect_delay_ms = 1;
        options.limit_options.limits.max_pre_bridge_commands = Some(1);
        options.bridge = Some(Arc::new({
            let gate = Arc::clone(&gate);
            let started = Arc::clone(&started);
            move |_| {
                started.store(true, Ordering::Release);
                gate.wait();
                Ok(TestResult { ok: true })
            }
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        let first = Arc::clone(&sockets.lock().unwrap()[0]);
        first.emit_open();
        first.emit_text(command("request-hung", "hung"));
        wait_until(|| started.load(Ordering::Acquire));
        assert_eq!(connection.inner.queued_count_budget.used(), 1);
        first.emit_close(1000, "");
        wait_until(|| sockets.lock().unwrap().len() == 2);
        let second = Arc::clone(&sockets.lock().unwrap()[1]);
        second.emit_open();
        second.emit_text(command("request-new", "new"));
        let last = second.sent_values().pop().unwrap();
        assert_eq!(last["requestId"], "request-new");
        assert_eq!(last["error"], "queue-full");
        gate.open();
        connection.stop();
    }

    #[test]
    fn serializes_bridge_execution_in_arrival_order() {
        let socket = Arc::new(TestSocket::default());
        let started = Arc::new(Mutex::new(Vec::<String>::new()));
        let gate = Arc::new(Gate::default());
        let mut options = options(Arc::clone(&socket));
        options.bridge = Some(Arc::new({
            let started = Arc::clone(&started);
            let gate = Arc::clone(&gate);
            move |message: SessionServerMessage<String, TestInput>| {
                started.lock().unwrap().push(message.request_id.clone());
                if message.request_id == "request-1" {
                    gate.wait();
                }
                Ok(TestResult { ok: true })
            }
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        socket.emit_text(command("request-1", "request-1"));
        socket.emit_text(command("request-2", "request-2"));
        wait_until(|| !started.lock().unwrap().is_empty());
        assert_eq!(&*started.lock().unwrap(), &["request-1"]);
        gate.open();
        wait_until(|| started.lock().unwrap().len() == 2);
        assert_eq!(&*started.lock().unwrap(), &["request-1", "request-2"]);
        connection.stop();
    }

    #[test]
    fn rejects_producer_hello_wrappers_with_unknown_or_dangerous_keys() {
        let socket = Arc::new(TestSocket::default());
        let key = SigningKey::from_bytes(&[62; 32]);
        let now = now_millis();
        let grant = ProducerGrant {
            base: crate::BrokerGrantBase {
                app_id: "dev.example".into(),
                principal_id: "producer-1".into(),
                key_id: "producer-key-1".into(),
                grant_id: "producer-grant-1".into(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: now.saturating_sub(1_000),
                expires_at: now.saturating_add(60_000),
                revocation_id: "producer-revocation-1".into(),
                may_delegate: false,
                session_id: None,
            },
            operations: vec![ProducerOperation::Register],
        };
        let mut options = options(Arc::clone(&socket));
        options.reconnect_delay_ms = 10_000;
        options.producer_authentication = Some(SessionBrokerProducerAuthentication::native(
            "dev.example",
            1,
            SessionBrokerClientCredential {
                grant,
                private_key: key.clone(),
            },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: key.verifying_key(),
            },
        ));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        for message in [
            r#"{"type":"hello-challenge","challenge":{},"extra":true}"#,
            r#"{"type":"hello-challenge","challenge":{},"__proto__":{}}"#,
        ] {
            socket.set_ready(1);
            socket.emit_text(message);
            assert_eq!(
                socket.close_record(),
                Some(CloseRecord {
                    code: Some(1008),
                    reason: Some("Session broker authentication failed.".into()),
                })
            );
        }
        connection.stop();
    }

    #[test]
    fn prepares_reconnect_once_per_attempt_and_stops_after_awaited_preparation() {
        let sockets = Arc::new(Mutex::new(Vec::<Arc<TestSocket>>::new()));
        let warnings = Arc::new(Mutex::new(Vec::<String>::new()));
        let attempts = Arc::new(AtomicUsize::new(0));
        let third_started = Arc::new(AtomicBool::new(false));
        let third_gate = Arc::new(Gate::default());
        let mut options = SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            collecting_factory(Arc::clone(&sockets)),
            registration(),
            snapshot(0),
            parsers(),
        );
        options.reconnect_delay_ms = 1;
        options.prepare_reconnect = Some(Arc::new({
            let attempts = Arc::clone(&attempts);
            let third_started = Arc::clone(&third_started);
            let third_gate = Arc::clone(&third_gate);
            move || {
                let attempt = attempts.fetch_add(1, Ordering::AcqRel) + 1;
                if attempt == 1 {
                    return Err("incumbent still alive".into());
                }
                if attempt >= 3 {
                    third_started.store(true, Ordering::Release);
                    third_gate.wait();
                }
                Ok(())
            }
        }));
        options.on_warning = Some(Arc::new({
            let warnings = Arc::clone(&warnings);
            move |warning| warnings.lock().unwrap().push(warning.into())
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        let first = Arc::clone(&sockets.lock().unwrap()[0]);
        first.emit_open();
        first.emit_close(1000, "");
        wait_until(|| sockets.lock().unwrap().len() == 2);
        assert_eq!(attempts.load(Ordering::Acquire), 2);
        assert_eq!(&*warnings.lock().unwrap(), &["incumbent still alive"]);

        let second = Arc::clone(&sockets.lock().unwrap()[1]);
        second.emit_open();
        second.emit_close(1000, "");
        wait_until(|| third_started.load(Ordering::Acquire));
        connection.stop();
        third_gate.open();
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(sockets.lock().unwrap().len(), 2);
    }

    #[test]
    fn explicit_start_creates_fresh_generation_after_no_reconnect_close() {
        let sockets = Arc::new(Mutex::new(Vec::<Arc<TestSocket>>::new()));
        let mut options = SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            collecting_factory(Arc::clone(&sockets)),
            registration(),
            snapshot(0),
            parsers(),
        );
        options.resolve_close = Some(Arc::new(|_| SessionBrokerConnectionCloseDirective {
            reconnect: Some(false),
            warning: None,
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        let first = Arc::clone(&sockets.lock().unwrap()[0]);
        first.emit_open();
        first.emit_close(1000, "complete");
        std::thread::sleep(Duration::from_millis(5));
        assert_eq!(sockets.lock().unwrap().len(), 1);
        connection.start().unwrap();
        let second = Arc::clone(&sockets.lock().unwrap()[1]);
        second.emit_open();
        assert_eq!(sockets.lock().unwrap().len(), 2);
        assert_eq!(first.sent_values()[0]["type"], "register");
        assert_eq!(second.sent_values()[0]["type"], "register");
        connection.stop();
    }

    fn assert_stop_fences_late_preparation(reject: bool) {
        let sockets = Arc::new(Mutex::new(Vec::<Arc<TestSocket>>::new()));
        let warnings = Arc::new(Mutex::new(Vec::<String>::new()));
        let started = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(Gate::default());
        let mut options = SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            collecting_factory(Arc::clone(&sockets)),
            registration(),
            snapshot(0),
            parsers(),
        );
        options.reconnect_delay_ms = 1;
        options.prepare_reconnect = Some(Arc::new({
            let started = Arc::clone(&started);
            let gate = Arc::clone(&gate);
            move || {
                started.store(true, Ordering::Release);
                gate.wait();
                if reject {
                    Err("late preparation failure".into())
                } else {
                    Ok(())
                }
            }
        }));
        options.on_warning = Some(Arc::new({
            let warnings = Arc::clone(&warnings);
            move |warning| warnings.lock().unwrap().push(warning.into())
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        let first = Arc::clone(&sockets.lock().unwrap()[0]);
        first.emit_open();
        first.emit_close(1000, "");
        wait_until(|| started.load(Ordering::Acquire));
        connection.stop();
        gate.open();
        std::thread::sleep(Duration::from_millis(10));
        assert!(warnings.lock().unwrap().is_empty());
        assert_eq!(sockets.lock().unwrap().len(), 1);
    }

    #[test]
    fn stop_fences_late_successful_reconnect_preparation() {
        assert_stop_fences_late_preparation(false);
    }

    #[test]
    fn stop_fences_late_failed_reconnect_preparation() {
        assert_stop_fences_late_preparation(true);
    }

    #[test]
    fn reconnects_unless_close_directive_disables_it() {
        let sockets = Arc::new(Mutex::new(Vec::<Arc<TestSocket>>::new()));
        let warnings = Arc::new(Mutex::new(Vec::<String>::new()));
        let mut options = SessionBrokerConnectionOptions::new(
            "ws://broker.test/session",
            collecting_factory(Arc::clone(&sockets)),
            registration(),
            snapshot(0),
            parsers(),
        );
        options.reconnect_delay_ms = 5;
        options.resolve_close = Some(Arc::new(|event| {
            if event.reason == "stop" {
                SessionBrokerConnectionCloseDirective {
                    reconnect: Some(false),
                    warning: Some("Stopped reconnecting.".into()),
                }
            } else {
                SessionBrokerConnectionCloseDirective {
                    reconnect: Some(true),
                    warning: None,
                }
            }
        }));
        options.on_warning = Some(Arc::new({
            let warnings = Arc::clone(&warnings);
            move |warning| warnings.lock().unwrap().push(warning.into())
        }));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        let first = Arc::clone(&sockets.lock().unwrap()[0]);
        first.emit_open();
        first.emit_close(1008, "retry");
        wait_until(|| sockets.lock().unwrap().len() == 2);
        let second = Arc::clone(&sockets.lock().unwrap()[1]);
        second.emit_close(1008, "stop");
        std::thread::sleep(Duration::from_millis(15));
        assert_eq!(&*warnings.lock().unwrap(), &["Stopped reconnecting."]);
        assert_eq!(sockets.lock().unwrap().len(), 2);
        connection.stop();
    }

    #[test]
    fn completes_signed_producer_handshake_before_registration() {
        let socket = Arc::new(TestSocket::default());
        let daemon = SigningKey::from_bytes(&[70; 32]);
        let producer = SigningKey::from_bytes(&[71; 32]);
        let now = now_millis();
        let grant = ProducerGrant {
            base: crate::BrokerGrantBase {
                app_id: "dev.example".into(),
                principal_id: "producer-1".into(),
                key_id: "producer-key-1".into(),
                grant_id: "producer-grant-1".into(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: now.saturating_sub(1_000),
                expires_at: now.saturating_add(60_000),
                revocation_id: "producer-revocation-1".into(),
                may_delegate: false,
                session_id: None,
            },
            operations: vec![ProducerOperation::Register, ProducerOperation::Reconnect],
        };
        let authenticator = SessionBrokerAuthenticator::new(SessionBrokerAuthenticatorOptions {
            app_id: "dev.example".into(),
            app_revision: 1,
            generation: "generation-1".into(),
            daemon_identity: SessionBrokerDaemonIdentity {
                key_id: "daemon-key-1".into(),
                private_key: daemon.clone(),
            },
            credentials: vec![SessionBrokerAuthorityCredential {
                grant: crate::BrokerGrant::Producer(grant.clone()),
                public_key: producer.verifying_key(),
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
        let connected = Arc::new(AtomicUsize::new(0));
        let mut options = options(Arc::clone(&socket));
        options.on_connected = Some(Arc::new({
            let connected = Arc::clone(&connected);
            move || {
                connected.fetch_add(1, Ordering::AcqRel);
            }
        }));
        options.producer_authentication = Some(SessionBrokerProducerAuthentication::native(
            "dev.example",
            1,
            SessionBrokerClientCredential {
                grant,
                private_key: producer,
            },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: daemon.verifying_key(),
            },
        ));
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        let init = socket.sent_values()[0].clone();
        let challenge = authenticator
            .issue_challenge(init["hello"].clone(), "ws://broker.test/session")
            .unwrap();
        socket.emit_text(json!({"type": "hello-challenge", "challenge": challenge}).to_string());
        let proof = socket.sent_values()[1]["proof"].clone();
        let authenticated = authenticator
            .complete_producer_hello(proof, "connection-1")
            .unwrap();
        socket.emit_text(json!({"type": "hello-ack", "ack": authenticated.ack}).to_string());
        let messages = socket.sent_values();
        assert_eq!(
            messages
                .iter()
                .map(|message| message["type"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["hello-init", "hello-proof", "register"]
        );
        assert_eq!(connected.load(Ordering::Acquire), 1);
        connection.stop();
    }

    #[test]
    fn closes_incomplete_producer_handshake_at_deadline() {
        let socket = Arc::new(TestSocket::default());
        let key = SigningKey::from_bytes(&[72; 32]);
        let now = now_millis();
        let grant = ProducerGrant {
            base: crate::BrokerGrantBase {
                app_id: "dev.example".into(),
                principal_id: "producer-1".into(),
                key_id: "producer-key-1".into(),
                grant_id: "producer-grant-1".into(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: now.saturating_sub(1_000),
                expires_at: now.saturating_add(60_000),
                revocation_id: "producer-revocation-1".into(),
                may_delegate: false,
                session_id: None,
            },
            operations: vec![ProducerOperation::Register],
        };
        let mut options = options(Arc::clone(&socket));
        options.producer_authentication = Some(SessionBrokerProducerAuthentication::native(
            "dev.example",
            1,
            SessionBrokerClientCredential {
                grant,
                private_key: key.clone(),
            },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: key.verifying_key(),
            },
        ));
        options.limit_options.limits.max_handshake_duration_ms = Some(5);
        options.limit_options.limits.challenge_ttl_ms = Some(5);
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        wait_until(|| socket.close_record().is_some());
        assert_eq!(
            socket.close_record(),
            Some(CloseRecord {
                code: Some(1008),
                reason: Some("Session broker authentication timed out.".into()),
            })
        );
        connection.stop();
    }

    #[test]
    fn emits_heartbeats_only_for_active_generation() {
        let socket = Arc::new(TestSocket::default());
        let mut options = options(Arc::clone(&socket));
        options.heartbeat_interval_ms = 5;
        let connection = TestConnection::new(options).unwrap();
        connection.start().unwrap();
        socket.emit_open();
        wait_until(|| {
            socket
                .sent_values()
                .iter()
                .any(|message| message["type"] == "heartbeat")
        });
        let count_before_stop = socket.sent_values().len();
        connection.stop();
        std::thread::sleep(Duration::from_millis(12));
        assert_eq!(socket.sent_values().len(), count_before_stop);
    }
}
