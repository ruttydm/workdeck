//! Native HTTP and WebSocket transport for the runtime-neutral session broker daemon.
//!
//! This is the Rust replacement for Hunk's Bun and Node adapters. The listener owns transport
//! admission, byte budgets, WebSocket framing, response bounding, and lifecycle ordering; the
//! daemon remains the authority for protocol, authentication, authorization, and session state.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, Wake, Waker};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;
use tungstenite::error::Error as WebSocketError;
use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Message, WebSocketConfig};

use crate::{
    BoundedHttpBody, BrokerBody, BrokerCapacityCode, BrokerHttpResponse, BudgetReservation,
    ResourceBudget, SessionBroker, SessionBrokerController, SessionBrokerDaemon,
    SessionBrokerDaemonPeer, SessionBrokerHttpRequest, SessionBrokerHttpResponse,
    SessionBrokerLimits, SharedSessionBrokerDaemonPeer, bound_http_response,
};

const MAX_HTTP_HEAD_BYTES: usize = 64 * 1_024;
const ACCEPT_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SOCKET_POLL_INTERVAL: Duration = Duration::from_millis(10);
const SOCKET_IO_TIMEOUT: Duration = Duration::from_secs(5);

pub type NativeSessionBrokerResponseFuture =
    Pin<Box<dyn Future<Output = Option<BrokerHttpResponse>> + Send + 'static>>;

pub type NativeSessionBrokerHttpHandler = Arc<
    dyn Fn(
            SessionBrokerHttpRequest,
            NativeSessionBrokerAddress,
        ) -> NativeSessionBrokerResponseFuture
        + Send
        + Sync,
>;

pub type NativeSessionBrokerServeErrorFormatter =
    Arc<dyn Fn(&io::Error, &NativeSessionBrokerAddress) -> String + Send + Sync>;

pub type NativeSessionBrokerMessageHandler = Arc<
    dyn Fn(SharedSessionBrokerDaemonPeer, Value) -> Result<(), NativeSessionBrokerMessageError>
        + Send
        + Sync,
>;

type DaemonHttpHandler =
    Arc<dyn Fn(&SessionBrokerHttpRequest) -> Option<SessionBrokerHttpResponse> + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeSessionBrokerAddress {
    pub hostname: String,
    pub port: u16,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("{0}")]
pub struct NativeSessionBrokerServeError(pub String);

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum NativeSessionBrokerMessageError {
    #[error("session broker message handling reached transport capacity")]
    Capacity,
    #[error("session broker message handling failed: {0}")]
    Failure(String),
}

/// Runtime-specific edge behavior retained for adapter conformance. Workdeck defaults to the Bun
/// surface used by Hunk itself; the Node profile remains available to execute that package's exact
/// wrong-path upgrade behavior without shipping a JavaScript runtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NativeSessionBrokerAdapterSemantics {
    #[default]
    Bun,
    Node,
}

pub struct ServeSessionBrokerDaemonOptions<
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
    pub daemon: SessionBrokerDaemon<Info, State, CommandInput, CommandResult, Controller>,
    pub hostname: String,
    pub port: u16,
    pub handle_request: Option<NativeSessionBrokerHttpHandler>,
    pub not_found: Option<NativeSessionBrokerHttpHandler>,
    pub format_serve_error: Option<NativeSessionBrokerServeErrorFormatter>,
    pub adapter_semantics: NativeSessionBrokerAdapterSemantics,
    pub allow_remote: bool,
    /// Test and embedding seam matching the runtime adapters' contained handler-failure behavior.
    /// Production callers normally leave this unset so the daemon handles every socket message.
    pub message_handler: Option<NativeSessionBrokerMessageHandler>,
}

impl<Info, State, CommandInput, CommandResult, Controller>
    ServeSessionBrokerDaemonOptions<Info, State, CommandInput, CommandResult, Controller>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    #[must_use]
    pub fn new(
        daemon: SessionBrokerDaemon<Info, State, CommandInput, CommandResult, Controller>,
        hostname: impl Into<String>,
        port: u16,
    ) -> Self {
        Self {
            daemon,
            hostname: hostname.into(),
            port,
            handle_request: None,
            not_found: None,
            format_serve_error: None,
            adapter_semantics: NativeSessionBrokerAdapterSemantics::default(),
            allow_remote: false,
            message_handler: None,
        }
    }
}

pub struct RunningSessionBrokerDaemon {
    inner: Arc<RunningInner>,
}

impl RunningSessionBrokerDaemon {
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.inner.socket_address
    }

    /// Begin semantic shutdown before closing transports. This operation is idempotent and does
    /// not wait for active custom HTTP handlers; use `wait_stopped` for the completion barrier.
    pub fn stop(&self) {
        (self.inner.shutdown_daemon)();
        self.inner.begin_transport_stop();
    }

    #[must_use]
    pub fn wait_stopped(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut lifecycle = self
            .inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        while !lifecycle.finished {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let waited = self
                .inner
                .lifecycle_changed
                .wait_timeout(lifecycle, remaining)
                .unwrap_or_else(|error| error.into_inner());
            lifecycle = waited.0;
            if waited.1.timed_out() && !lifecycle.finished {
                return false;
            }
        }
        true
    }

    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.inner
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .finished
    }
}

impl Drop for RunningSessionBrokerDaemon {
    fn drop(&mut self) {
        self.stop();
        let _ = self.wait_stopped(Duration::from_secs(5));
    }
}

struct TransportHandlers {
    request: DaemonHttpHandler,
    message: NativeSessionBrokerMessageHandler,
    close: Arc<dyn Fn(&SharedSessionBrokerDaemonPeer) + Send + Sync>,
}

struct NativeRuntime {
    address: NativeSessionBrokerAddress,
    limits: SessionBrokerLimits,
    socket_path: String,
    requires_authentication: bool,
    adapter_semantics: NativeSessionBrokerAdapterSemantics,
    custom_request: Option<NativeSessionBrokerHttpHandler>,
    not_found: Option<NativeSessionBrokerHttpHandler>,
    handlers: TransportHandlers,
    inbound_budget: ResourceBudget,
    outbound_budget: ResourceBudget,
    unauthenticated_budget: ResourceBudget,
    response_budget: ResourceBudget,
    running: Arc<RunningInner>,
}

struct RunningInner {
    socket_address: SocketAddr,
    stopping: AtomicBool,
    next_connection_id: AtomicU64,
    lifecycle: Mutex<TransportLifecycle>,
    lifecycle_changed: Condvar,
    shutdown_daemon: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
struct TransportLifecycle {
    connections: BTreeMap<u64, ActiveConnection>,
    peers: BTreeMap<u64, Arc<NativePeer>>,
    listener_exited: bool,
    finished: bool,
}

struct ActiveConnection {
    socket: TcpStream,
    websocket: bool,
}

impl RunningInner {
    fn register_connection(&self, socket: &TcpStream) -> io::Result<u64> {
        let id = self.next_connection_id.fetch_add(1, Ordering::AcqRel);
        self.lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .connections
            .insert(
                id,
                ActiveConnection {
                    socket: socket.try_clone()?,
                    websocket: false,
                },
            );
        Ok(id)
    }

    fn mark_websocket(&self, id: u64, peer: Arc<NativePeer>) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(connection) = lifecycle.connections.get_mut(&id) {
            connection.websocket = true;
        }
        lifecycle.peers.insert(id, peer);
    }

    fn finish_connection(&self, id: u64) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.connections.remove(&id);
        lifecycle.peers.remove(&id);
        self.lifecycle_changed.notify_all();
    }

    fn listener_exited(&self) {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        lifecycle.listener_exited = true;
        while !lifecycle.connections.is_empty() {
            lifecycle = self
                .lifecycle_changed
                .wait(lifecycle)
                .unwrap_or_else(|error| error.into_inner());
        }
        lifecycle.finished = true;
        self.lifecycle_changed.notify_all();
    }

    fn begin_transport_stop(&self) {
        if self.stopping.swap(true, Ordering::AcqRel) {
            return;
        }
        let (peers, http_sockets) = {
            let lifecycle = self
                .lifecycle
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (
                lifecycle.peers.values().cloned().collect::<Vec<_>>(),
                lifecycle
                    .connections
                    .values()
                    .filter(|connection| !connection.websocket)
                    .filter_map(|connection| connection.socket.try_clone().ok())
                    .collect::<Vec<_>>(),
            )
        };
        for peer in peers {
            peer.close(Some(1001), Some("Session broker shutting down."));
        }
        for socket in http_sockets {
            let _ = socket.shutdown(Shutdown::Both);
        }
        self.lifecycle_changed.notify_all();
    }
}

struct ConnectionGuard {
    running: Arc<RunningInner>,
    id: u64,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.running.finish_connection(self.id);
    }
}

struct OutboundMessage {
    text: String,
    reservation: BudgetReservation,
}

struct PeerQueue {
    messages: VecDeque<OutboundMessage>,
    bytes: u64,
    close: Option<(u16, String)>,
}

struct NativePeer {
    queue: Mutex<PeerQueue>,
    max_outbound_bytes: u64,
    outbound_budget: ResourceBudget,
    admission: Mutex<Option<BudgetReservation>>,
    retired: AtomicBool,
}

impl NativePeer {
    fn new(
        max_outbound_bytes: u64,
        outbound_budget: ResourceBudget,
        admission: BudgetReservation,
    ) -> Self {
        Self {
            queue: Mutex::new(PeerQueue {
                messages: VecDeque::new(),
                bytes: 0,
                close: None,
            }),
            max_outbound_bytes,
            outbound_budget,
            admission: Mutex::new(Some(admission)),
            retired: AtomicBool::new(false),
        }
    }

    fn take_message(&self) -> Option<OutboundMessage> {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        queue.messages.pop_front()
    }

    fn complete_message(&self, bytes: u64) {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        queue.bytes = queue.bytes.saturating_sub(bytes);
    }

    fn take_close(&self) -> Option<(u16, String)> {
        self.queue
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .close
            .take()
    }

    fn clear_outbound(&self) {
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        for message in queue.messages.drain(..) {
            message.reservation.release();
        }
        queue.bytes = 0;
    }

    fn release_admission(&self) {
        if let Some(reservation) = self
            .admission
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            reservation.release();
        }
    }

    fn authentication_pending(&self) -> bool {
        self.admission
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_some()
    }

    fn retire(&self) {
        self.retired.store(true, Ordering::Release);
        self.release_admission();
        self.clear_outbound();
    }
}

impl SessionBrokerDaemonPeer for NativePeer {
    fn send(&self, data: &str) -> Result<(), String> {
        let bytes = u64::try_from(data.len()).unwrap_or(u64::MAX);
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        if self.retired.load(Ordering::Acquire) {
            return Err("closed".into());
        }
        if queue.close.is_some() || bytes > self.max_outbound_bytes.saturating_sub(queue.bytes) {
            queue.close = Some((1013, "Session broker outbound pressure exceeded.".into()));
            return Err("busy".into());
        }
        let Some(reservation) = self.outbound_budget.try_reserve(bytes) else {
            queue.close = Some((1013, "Session broker outbound pressure exceeded.".into()));
            return Err("busy".into());
        };
        queue.bytes += bytes;
        queue.messages.push_back(OutboundMessage {
            text: data.into(),
            reservation,
        });
        Ok(())
    }

    fn close(&self, code: Option<u16>, reason: Option<&str>) {
        if self.retired.load(Ordering::Acquire) {
            return;
        }
        let mut queue = self.queue.lock().unwrap_or_else(|error| error.into_inner());
        if queue.close.is_none() {
            queue.close = Some((
                code.unwrap_or(1000),
                reason.unwrap_or_default().chars().take(123).collect(),
            ));
        }
    }

    fn mark_authenticated(&self) {
        self.release_admission();
    }
}

impl Drop for NativePeer {
    fn drop(&mut self) {
        self.release_admission();
        self.clear_outbound();
    }
}

pub fn serve_session_broker_daemon<Info, State, CommandInput, CommandResult, Controller>(
    options: ServeSessionBrokerDaemonOptions<Info, State, CommandInput, CommandResult, Controller>,
) -> Result<RunningSessionBrokerDaemon, NativeSessionBrokerServeError>
where
    Info: Clone + Serialize + Send + Sync + 'static,
    State: Clone + Serialize + Send + Sync + 'static,
    CommandInput: Serialize + Send + Sync + 'static,
    CommandResult: Clone + Serialize + Send + 'static,
    Controller: SessionBrokerController<Info, State, CommandInput, CommandResult> + 'static,
{
    let requested = NativeSessionBrokerAddress {
        hostname: options.hostname.clone(),
        port: options.port,
    };
    let listener =
        TcpListener::bind((options.hostname.as_str(), options.port)).map_err(|error| {
            format_serve_error(&error, &requested, options.format_serve_error.as_ref())
        })?;
    listener.set_nonblocking(true).map_err(|error| {
        format_serve_error(&error, &requested, options.format_serve_error.as_ref())
    })?;
    let socket_address = listener.local_addr().map_err(|error| {
        format_serve_error(&error, &requested, options.format_serve_error.as_ref())
    })?;
    if !options.allow_remote && !socket_address.ip().is_loopback() {
        let error = io::Error::new(
            io::ErrorKind::PermissionDenied,
            "session broker listeners must bind a loopback address",
        );
        return Err(format_serve_error(
            &error,
            &requested,
            options.format_serve_error.as_ref(),
        ));
    }
    let address = NativeSessionBrokerAddress {
        hostname: options.hostname,
        port: socket_address.port(),
    };
    let daemon = options.daemon;
    let limits = daemon.limits();
    let socket_path = daemon.paths().socket;
    let requires_authentication = daemon.requires_producer_authentication();
    let request_daemon = daemon.clone();
    let request =
        Arc::new(move |value: &SessionBrokerHttpRequest| request_daemon.handle_request(value));
    let message = options.message_handler.unwrap_or_else(|| {
        let message_daemon = daemon.clone();
        Arc::new(move |peer, value| {
            message_daemon.handle_connection_message(peer, value);
            Ok(())
        })
    });
    let close_daemon = daemon.clone();
    let close = Arc::new(move |peer: &SharedSessionBrokerDaemonPeer| {
        close_daemon.handle_connection_close(peer);
    });
    let shutdown_daemon = {
        let daemon = daemon.clone();
        Arc::new(move || daemon.shutdown(None)) as Arc<dyn Fn() + Send + Sync>
    };
    let running = Arc::new(RunningInner {
        socket_address,
        stopping: AtomicBool::new(false),
        next_connection_id: AtomicU64::new(1),
        lifecycle: Mutex::new(TransportLifecycle::default()),
        lifecycle_changed: Condvar::new(),
        shutdown_daemon,
    });
    let runtime = Arc::new(NativeRuntime {
        address,
        limits,
        socket_path,
        requires_authentication,
        adapter_semantics: options.adapter_semantics,
        custom_request: options.handle_request,
        not_found: options.not_found,
        handlers: TransportHandlers {
            request,
            message,
            close,
        },
        inbound_budget: ResourceBudget::new(limits.max_in_flight_ws_bytes, "maxInFlightWsBytes"),
        outbound_budget: ResourceBudget::with_code(
            limits.max_outbound_bytes_total,
            "maxOutboundBytesTotal",
            BrokerCapacityCode::Busy,
        ),
        unauthenticated_budget: ResourceBudget::with_code(
            limits.max_unauthenticated_sockets,
            "maxUnauthenticatedSockets",
            BrokerCapacityCode::Busy,
        ),
        response_budget: ResourceBudget::with_code(
            limits.max_in_flight_http_response_bytes,
            "maxInFlightHttpResponseBytes",
            BrokerCapacityCode::Busy,
        ),
        running: Arc::clone(&running),
    });

    let listener_runtime = Arc::clone(&runtime);
    thread::Builder::new()
        .name("workdeck-session-listener".into())
        .spawn(move || run_listener(listener, listener_runtime))
        .map_err(|error| NativeSessionBrokerServeError(error.to_string()))?;

    let watcher_running = Arc::clone(&running);
    if let Err(error) = thread::Builder::new()
        .name("workdeck-session-daemon-watch".into())
        .spawn(move || {
            while !watcher_running.stopping.load(Ordering::Acquire) {
                if daemon.wait_stopped(Duration::from_millis(100)) {
                    watcher_running.begin_transport_stop();
                    return;
                }
            }
        })
    {
        (running.shutdown_daemon)();
        running.begin_transport_stop();
        return Err(NativeSessionBrokerServeError(error.to_string()));
    }

    Ok(RunningSessionBrokerDaemon { inner: running })
}

fn format_serve_error(
    error: &io::Error,
    address: &NativeSessionBrokerAddress,
    formatter: Option<&NativeSessionBrokerServeErrorFormatter>,
) -> NativeSessionBrokerServeError {
    let message = formatter.map_or_else(
        || {
            format!(
                "Failed to start the session broker server on {}:{}: {error}",
                address.hostname, address.port
            )
        },
        |formatter| formatter(error, address),
    );
    NativeSessionBrokerServeError(message)
}

fn run_listener(listener: TcpListener, runtime: Arc<NativeRuntime>) {
    while !runtime.running.stopping.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((socket, _)) => {
                let Ok(id) = runtime.running.register_connection(&socket) else {
                    let _ = socket.shutdown(Shutdown::Both);
                    continue;
                };
                let connection_runtime = Arc::clone(&runtime);
                if thread::Builder::new()
                    .name(format!("workdeck-session-connection-{id}"))
                    .spawn(move || run_connection(socket, id, connection_runtime))
                    .is_err()
                {
                    runtime.running.finish_connection(id);
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(ACCEPT_POLL_INTERVAL);
            }
            Err(_) => {
                (runtime.running.shutdown_daemon)();
                runtime.running.begin_transport_stop();
                break;
            }
        }
    }
    drop(listener);
    runtime.running.listener_exited();
}

fn run_connection(mut socket: TcpStream, id: u64, runtime: Arc<NativeRuntime>) {
    let _guard = ConnectionGuard {
        running: Arc::clone(&runtime.running),
        id,
    };
    // Accepted sockets can inherit the listener's nonblocking mode (Darwin).
    // Connection workers use bounded blocking I/O; headers need not arrive in
    // the same scheduling slice as accept, and WouldBlock is not an EOF.
    if socket
        .set_nonblocking(false)
        .and_then(|()| socket.set_read_timeout(Some(SOCKET_IO_TIMEOUT)))
        .and_then(|()| socket.set_write_timeout(Some(SOCKET_IO_TIMEOUT)))
        .is_err()
    {
        return;
    }
    let Ok(head) = peek_http_head(&socket) else {
        return;
    };
    let Ok(parsed) = parse_http_head(&head) else {
        let _ = write_empty_response(&mut socket, 400);
        return;
    };
    if is_websocket_upgrade(&parsed.headers) {
        run_websocket(socket, id, runtime, parsed, head.len());
    } else {
        run_http(socket, runtime);
    }
}

#[derive(Debug)]
struct ParsedHttpHead {
    method: String,
    target: String,
    headers: BTreeMap<String, String>,
}

fn peek_http_head(socket: &TcpStream) -> io::Result<Vec<u8>> {
    let deadline = Instant::now() + SOCKET_IO_TIMEOUT;
    let mut buffer = vec![0_u8; MAX_HTTP_HEAD_BYTES];
    loop {
        let read = socket.peek(&mut buffer)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "connection closed before HTTP request headers",
            ));
        }
        if let Some(end) = find_head_end(&buffer[..read]) {
            buffer.truncate(end);
            return Ok(buffer);
        }
        if read == MAX_HTTP_HEAD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP request head exceeded its limit",
            ));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "timed out reading HTTP request head",
            ));
        }
        thread::sleep(Duration::from_millis(2));
    }
}

fn find_head_end(bytes: &[u8]) -> Option<usize> {
    bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
}

fn parse_http_head(bytes: &[u8]) -> Result<ParsedHttpHead, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let mut lines = text[..text.len().saturating_sub(4)].split("\r\n");
    let mut request = lines.next().ok_or(())?.split_whitespace();
    let method = request.next().ok_or(())?.to_owned();
    let target = request.next().ok_or(())?.to_owned();
    if request.next() != Some("HTTP/1.1") || request.next().is_some() || method.is_empty() {
        return Err(());
    }
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or(())?;
        if name.is_empty()
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
        {
            return Err(());
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim().to_owned();
        if !valid_header_value(&value) {
            return Err(());
        }
        headers
            .entry(name)
            .and_modify(|existing: &mut String| {
                existing.push_str(", ");
                existing.push_str(&value);
            })
            .or_insert(value);
    }
    Ok(ParsedHttpHead {
        method,
        target,
        headers,
    })
}

fn is_websocket_upgrade(headers: &BTreeMap<String, String>) -> bool {
    headers
        .get("upgrade")
        .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && headers.get("connection").is_some_and(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        })
}

fn valid_origin_target(target: &str) -> bool {
    target.starts_with('/') && !target.starts_with("//")
}

fn target_path(target: &str) -> &str {
    target.split(['?', '#']).next().unwrap_or(target)
}

fn run_websocket(
    mut socket: TcpStream,
    id: u64,
    runtime: Arc<NativeRuntime>,
    parsed: ParsedHttpHead,
    request_head_bytes: usize,
) {
    if !valid_origin_target(&parsed.target) {
        let _ = consume_bytes(&mut socket, request_head_bytes);
        let _ = write_empty_response(&mut socket, 400);
        return;
    }
    if let Some(handler) = runtime.custom_request.as_ref() {
        let request = SessionBrokerHttpRequest {
            method: parsed.method.clone(),
            url: format!(
                "http://{}:{}{}",
                runtime.address.hostname, runtime.address.port, parsed.target
            ),
            headers: parsed.headers.clone(),
            body: Vec::new(),
        };
        let response = match catch_unwind(AssertUnwindSafe(|| {
            block_on_response(handler(request.clone(), runtime.address.clone()))
        })) {
            Ok(response) => response,
            Err(_) => return,
        };
        if let Some(response) = response {
            let _ = consume_bytes(&mut socket, request_head_bytes);
            let _ = write_bounded_response(&mut socket, &request.method, response, &runtime);
            return;
        }
    }
    if parsed.method != "GET" || target_path(&parsed.target) != runtime.socket_path {
        if runtime.adapter_semantics == NativeSessionBrokerAdapterSemantics::Bun {
            run_http(socket, runtime);
        }
        return;
    }
    if runtime.running.stopping.load(Ordering::Acquire) {
        let _ = consume_bytes(&mut socket, request_head_bytes);
        let _ = write_empty_response(&mut socket, 503);
        return;
    }
    let Some(admission) = runtime.unauthenticated_budget.try_reserve(1) else {
        let _ = consume_bytes(&mut socket, request_head_bytes);
        let _ = write_empty_response(&mut socket, 503);
        return;
    };
    let mut config = WebSocketConfig::default();
    let native_message_limit = runtime.limits.max_ws_message_bytes.max(1);
    config.max_message_size = usize::try_from(native_message_limit).ok();
    config.max_frame_size = usize::try_from(native_message_limit).ok();
    let websocket = tungstenite::accept_with_config(socket, Some(config));
    let mut websocket = match websocket {
        Ok(websocket) => websocket,
        Err(_) => {
            admission.release();
            return;
        }
    };
    let _ = websocket
        .get_mut()
        .set_read_timeout(Some(SOCKET_POLL_INTERVAL));
    let peer = Arc::new(NativePeer::new(
        runtime.limits.max_outbound_bytes_per_peer,
        runtime.outbound_budget.clone(),
        admission,
    ));
    let shared_peer: SharedSessionBrokerDaemonPeer = peer.clone();
    runtime.running.mark_websocket(id, Arc::clone(&peer));
    if !runtime.requires_authentication {
        peer.mark_authenticated();
    }
    let opened = Instant::now();
    loop {
        if runtime.running.stopping.load(Ordering::Acquire) {
            peer.close(Some(1001), Some("Session broker shutting down."));
        }
        if runtime.requires_authentication
            && peer.authentication_pending()
            && opened.elapsed() >= Duration::from_millis(runtime.limits.max_handshake_duration_ms)
        {
            peer.close(Some(1008), Some("Session broker authentication timed out."));
        }
        if let Some((code, reason)) = peer.take_close() {
            peer.clear_outbound();
            let _ = websocket.close(Some(CloseFrame {
                code: CloseCode::from(code),
                reason: reason.into(),
            }));
            break;
        }
        while let Some(message) = peer.take_message() {
            let bytes = message.reservation.amount();
            let result = websocket.send(Message::Text(message.text.into()));
            message.reservation.release();
            peer.complete_message(bytes);
            if result.is_err() {
                peer.close(Some(1013), Some("Session broker outbound delivery failed."));
                break;
            }
        }
        match websocket.read() {
            Ok(Message::Text(text)) => {
                handle_text_message(&runtime, &peer, &shared_peer, text.as_str());
            }
            Ok(Message::Binary(_)) => peer.close(
                Some(1003),
                Some("Session broker accepts text messages only."),
            ),
            Ok(Message::Close(_)) => break,
            Ok(Message::Ping(_) | Message::Pong(_) | Message::Frame(_)) => {}
            Err(WebSocketError::Io(error))
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(WebSocketError::ConnectionClosed | WebSocketError::AlreadyClosed) => break,
            Err(WebSocketError::Capacity(_)) => peer.close(
                Some(1009),
                Some("Message exceeds the session broker size limit."),
            ),
            Err(WebSocketError::Utf8(_)) => {
                peer.close(Some(1007), Some("Malformed UTF-8 session broker message."))
            }
            Err(_) => break,
        }
    }
    peer.retire();
    let _ = catch_unwind(AssertUnwindSafe(|| (runtime.handlers.close)(&shared_peer)));
}

fn consume_bytes(socket: &mut TcpStream, mut remaining: usize) -> io::Result<()> {
    let mut buffer = [0_u8; 4 * 1_024];
    while remaining > 0 {
        let amount = remaining.min(buffer.len());
        let read = socket.read(&mut buffer[..amount])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "socket closed while consuming request head",
            ));
        }
        remaining -= read;
    }
    Ok(())
}

fn handle_text_message(
    runtime: &NativeRuntime,
    peer: &Arc<NativePeer>,
    shared_peer: &SharedSessionBrokerDaemonPeer,
    text: &str,
) {
    let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
    if bytes > runtime.limits.max_ws_message_bytes {
        peer.close(
            Some(1009),
            Some("Message exceeds the session broker size limit."),
        );
        return;
    }
    let Some(reservation) = runtime.inbound_budget.try_reserve(bytes) else {
        peer.close(
            Some(1013),
            Some("Session broker inbound pressure exceeded."),
        );
        return;
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        (runtime.handlers.message)(Arc::clone(shared_peer), Value::String(text.into()))
    }));
    reservation.release();
    match result {
        Ok(Ok(())) => {}
        Ok(Err(NativeSessionBrokerMessageError::Capacity)) => {
            peer.close(Some(1013), Some("Session broker message handling failed."))
        }
        Ok(Err(NativeSessionBrokerMessageError::Failure(_))) | Err(_) => {
            peer.close(Some(1011), Some("Session broker message handling failed."))
        }
    }
}

fn run_http(mut socket: TcpStream, runtime: Arc<NativeRuntime>) {
    if runtime.running.stopping.load(Ordering::Acquire) {
        let _ = write_empty_response(&mut socket, 503);
        return;
    }
    let request = match read_http_request(&mut socket, &runtime) {
        Ok(request) => request,
        Err(ReadHttpRequestError::PayloadTooLarge(remaining)) => {
            let response = daemon_http_response(SessionBrokerHttpResponse::json(
                413,
                &json!({"error": "capacity-exceeded", "resource": "maxHttpBodyBytes"}),
            ));
            let _ = write_bounded_response(&mut socket, "POST", response, &runtime);
            drain_oversized_body(&mut socket, remaining, runtime.limits.max_http_body_bytes);
            return;
        }
        Err(ReadHttpRequestError::Malformed) => {
            let _ = write_empty_response(&mut socket, 400);
            return;
        }
        Err(ReadHttpRequestError::Io) => return,
    };
    let callback_address = runtime.address.clone();
    let custom = match runtime.custom_request.as_ref() {
        Some(handler) => match catch_unwind(AssertUnwindSafe(|| {
            block_on_response(handler(request.clone(), callback_address.clone()))
        })) {
            Ok(response) => response,
            Err(_) => return,
        },
        None => None,
    };
    let daemon_response = if custom.is_some() {
        None
    } else {
        match catch_unwind(AssertUnwindSafe(|| (runtime.handlers.request)(&request))) {
            Ok(response) => response.map(daemon_http_response),
            Err(_) => return,
        }
    };
    let mut response = custom.or(daemon_response).or_else(|| {
        (runtime.adapter_semantics == NativeSessionBrokerAdapterSemantics::Bun
            && request_path(&request.url).as_deref() == Some(runtime.socket_path.as_str()))
        .then(expected_websocket_upgrade)
    });
    if response.is_none()
        && let Some(handler) = runtime.not_found.as_ref()
    {
        response = match catch_unwind(AssertUnwindSafe(|| {
            block_on_response(handler(request.clone(), callback_address))
        })) {
            Ok(response) => response,
            Err(_) => return,
        };
    }
    let response = response.unwrap_or_else(default_not_found);
    let _ = write_bounded_response(&mut socket, &request.method, response, &runtime);
}

#[derive(Debug)]
enum ReadHttpRequestError {
    PayloadTooLarge(u64),
    Malformed,
    Io,
}

impl From<io::Error> for ReadHttpRequestError {
    fn from(error: io::Error) -> Self {
        let _ = error;
        Self::Io
    }
}

fn read_http_request(
    socket: &mut TcpStream,
    runtime: &NativeRuntime,
) -> Result<SessionBrokerHttpRequest, ReadHttpRequestError> {
    let mut bytes = Vec::with_capacity(4 * 1_024);
    let mut chunk = [0_u8; 4 * 1_024];
    let head_end = loop {
        let read = socket.read(&mut chunk)?;
        if read == 0 {
            return Err(ReadHttpRequestError::Malformed);
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = find_head_end(&bytes) {
            break end;
        }
        if bytes.len() >= MAX_HTTP_HEAD_BYTES {
            return Err(ReadHttpRequestError::Malformed);
        }
    };
    let parsed =
        parse_http_head(&bytes[..head_end]).map_err(|()| ReadHttpRequestError::Malformed)?;
    if !valid_origin_target(&parsed.target) {
        return Err(ReadHttpRequestError::Malformed);
    }
    let chunked = parsed
        .headers
        .get("transfer-encoding")
        .map(|value| value.eq_ignore_ascii_case("chunked"))
        .unwrap_or(false);
    if parsed.headers.contains_key("transfer-encoding") && !chunked {
        return Err(ReadHttpRequestError::Malformed);
    }
    if chunked && parsed.headers.contains_key("content-length") {
        return Err(ReadHttpRequestError::Malformed);
    }
    let content_length = match parsed.headers.get("content-length") {
        Some(value)
            if !value.is_empty()
                && value.bytes().all(|byte| byte.is_ascii_digit())
                && (value == "0" || !value.starts_with('0')) =>
        {
            value
                .parse::<u64>()
                .map_err(|_| ReadHttpRequestError::PayloadTooLarge(0))?
        }
        Some(_) => return Err(ReadHttpRequestError::Malformed),
        None => 0,
    };
    if content_length > runtime.limits.max_http_body_bytes {
        let buffered = u64::try_from(bytes.len().saturating_sub(head_end)).unwrap_or(u64::MAX);
        return Err(ReadHttpRequestError::PayloadTooLarge(
            content_length.saturating_sub(buffered),
        ));
    }
    let body = if chunked {
        read_chunked_body(
            socket,
            bytes[head_end..].to_vec(),
            runtime.limits.max_http_body_bytes,
        )?
    } else {
        let body_length = usize::try_from(content_length)
            .map_err(|_| ReadHttpRequestError::PayloadTooLarge(0))?;
        let body_end = head_end
            .checked_add(body_length)
            .ok_or(ReadHttpRequestError::PayloadTooLarge(0))?;
        while bytes.len() < body_end {
            let read = socket.read(&mut chunk)?;
            if read == 0 {
                return Err(ReadHttpRequestError::Malformed);
            }
            bytes.extend_from_slice(&chunk[..read]);
        }
        bytes[head_end..body_end].to_vec()
    };
    let url = format!(
        "http://{}:{}{}",
        runtime.address.hostname, runtime.address.port, parsed.target
    );
    Ok(SessionBrokerHttpRequest {
        method: parsed.method,
        url,
        headers: parsed.headers,
        body,
    })
}

fn read_chunked_body(
    socket: &mut TcpStream,
    buffered: Vec<u8>,
    max_bytes: u64,
) -> Result<Vec<u8>, ReadHttpRequestError> {
    let mut reader = BufReader::new(std::io::Cursor::new(buffered).chain(socket));
    let mut body = Vec::new();
    loop {
        let mut size_line = Vec::new();
        if reader.read_until(b'\n', &mut size_line)? == 0
            || size_line.len() > MAX_HTTP_HEAD_BYTES
            || !size_line.ends_with(b"\r\n")
        {
            return Err(ReadHttpRequestError::Malformed);
        }
        size_line.truncate(size_line.len() - 2);
        let size_token = size_line
            .split(|byte| *byte == b';')
            .next()
            .unwrap_or_default();
        let size_text =
            std::str::from_utf8(size_token).map_err(|_| ReadHttpRequestError::Malformed)?;
        if size_text.is_empty() || !size_text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(ReadHttpRequestError::Malformed);
        }
        let size = u64::from_str_radix(size_text, 16)
            .map_err(|_| ReadHttpRequestError::PayloadTooLarge(0))?;
        if size == 0 {
            let mut trailer_bytes = 0_usize;
            loop {
                let mut trailer = Vec::new();
                if reader.read_until(b'\n', &mut trailer)? == 0 || !trailer.ends_with(b"\r\n") {
                    return Err(ReadHttpRequestError::Malformed);
                }
                trailer_bytes = trailer_bytes.saturating_add(trailer.len());
                if trailer_bytes > MAX_HTTP_HEAD_BYTES {
                    return Err(ReadHttpRequestError::Malformed);
                }
                if trailer == b"\r\n" {
                    return Ok(body);
                }
                let trailer = std::str::from_utf8(&trailer[..trailer.len() - 2])
                    .map_err(|_| ReadHttpRequestError::Malformed)?;
                let (name, value) = trailer
                    .split_once(':')
                    .ok_or(ReadHttpRequestError::Malformed)?;
                if !valid_header_name(name) || !valid_header_value(value.trim()) {
                    return Err(ReadHttpRequestError::Malformed);
                }
            }
        }
        let current = u64::try_from(body.len()).unwrap_or(u64::MAX);
        if size > max_bytes.saturating_sub(current) {
            return Err(ReadHttpRequestError::PayloadTooLarge(0));
        }
        let size = usize::try_from(size).map_err(|_| ReadHttpRequestError::PayloadTooLarge(0))?;
        let start = body.len();
        let end = start
            .checked_add(size)
            .ok_or(ReadHttpRequestError::PayloadTooLarge(0))?;
        body.resize(end, 0);
        reader.read_exact(&mut body[start..])?;
        let mut ending = [0_u8; 2];
        reader.read_exact(&mut ending)?;
        if ending != *b"\r\n" {
            return Err(ReadHttpRequestError::Malformed);
        }
    }
}

fn daemon_http_response(response: SessionBrokerHttpResponse) -> BrokerHttpResponse {
    let body = (!response.body.is_empty()).then(|| {
        BoundedHttpBody::Streaming(
            Box::new(std::io::Cursor::new(response.body)) as Box<dyn BrokerBody>
        )
    });
    BrokerHttpResponse {
        status: response.status,
        status_text: status_text(response.status).into(),
        headers: response.headers,
        body,
    }
}

fn default_not_found() -> BrokerHttpResponse {
    BrokerHttpResponse {
        status: 404,
        status_text: "Not Found".into(),
        headers: BTreeMap::from([("content-type".into(), "text/plain;charset=UTF-8".into())]),
        body: Some(BoundedHttpBody::Streaming(Box::new(std::io::Cursor::new(
            b"Not found.".to_vec(),
        )))),
    }
}

fn expected_websocket_upgrade() -> BrokerHttpResponse {
    BrokerHttpResponse {
        status: 426,
        status_text: "Upgrade Required".into(),
        headers: BTreeMap::from([("content-type".into(), "text/plain;charset=UTF-8".into())]),
        body: Some(BoundedHttpBody::Streaming(Box::new(std::io::Cursor::new(
            b"Expected websocket upgrade.".to_vec(),
        )))),
    }
}

fn request_path(url: &str) -> Option<String> {
    url::Url::parse(url).ok().map(|url| url.path().to_owned())
}

fn write_bounded_response(
    socket: &mut TcpStream,
    method: &str,
    response: BrokerHttpResponse,
    runtime: &NativeRuntime,
) -> Result<(), io::Error> {
    let mut response = bound_http_response(
        response,
        runtime.limits.max_http_response_bytes,
        Some(&runtime.response_budget),
    )
    .map_err(io::Error::other)?;
    if method == "HEAD"
        && let Some(body) = &mut response.body
    {
        let _ = body.cancel();
        response.body = None;
    }
    let reason = if response.status_text.is_empty() {
        status_text(response.status)
    } else {
        &response.status_text
    };
    if !(200..=599).contains(&response.status)
        || reason.contains(['\r', '\n'])
        || response
            .headers
            .iter()
            .any(|(name, value)| !valid_header_name(name) || !valid_header_value(value))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid HTTP response metadata",
        ));
    }
    let mut head = format!("HTTP/1.1 {} {}\r\n", response.status, reason);
    response.headers.insert("connection".into(), "close".into());
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    socket.write_all(head.as_bytes())?;
    let streaming = response
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .is_some_and(|(_, value)| value.to_ascii_lowercase().starts_with("text/event-stream"));
    if let Some(body) = response.body {
        if streaming {
            write_streaming_body(socket, body, &runtime.running)?;
        } else {
            socket.write_all(&body.read_all()?)?;
        }
    }
    socket.flush()
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn valid_header_value(value: &str) -> bool {
    !value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0))
}

fn write_streaming_body(
    socket: &mut TcpStream,
    mut body: BoundedHttpBody,
    running: &RunningInner,
) -> Result<(), io::Error> {
    let mut buffer = [0_u8; 16 * 1_024];
    loop {
        if running.stopping.load(Ordering::Acquire) {
            let _ = body.cancel();
            return Ok(());
        }
        let read = match &mut body {
            BoundedHttpBody::Streaming(body) => body.read(&mut buffer)?,
            BoundedHttpBody::Retained(body) => {
                let bytes = body.take();
                socket.write_all(&bytes)?;
                return Ok(());
            }
        };
        if read == 0 {
            return Ok(());
        }
        socket.write_all(&buffer[..read])?;
        socket.flush()?;
    }
}

/// Let a well-behaved client finish the bounded over-limit upload after the 413 has been sent.
/// This avoids a TCP reset erasing the small JSON failure response while still placing a hard
/// ceiling on bytes and time spent draining a hostile declaration.
fn drain_oversized_body(socket: &mut TcpStream, remaining: u64, configured_limit: u64) {
    if remaining == 0 {
        return;
    }
    let cap = configured_limit
        .saturating_add(configured_limit / 2)
        .max(64 * 1_024);
    let mut left = remaining.min(cap);
    let _ = socket.set_read_timeout(Some(Duration::from_millis(50)));
    let mut buffer = [0_u8; 16 * 1_024];
    while left > 0 {
        let wanted = usize::try_from(left)
            .unwrap_or(usize::MAX)
            .min(buffer.len());
        match socket.read(&mut buffer[..wanted]) {
            Ok(0) => break,
            Ok(read) => left = left.saturating_sub(u64::try_from(read).unwrap_or(u64::MAX)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                ) =>
            {
                break;
            }
            Err(_) => break,
        }
    }
}

fn write_empty_response(socket: &mut TcpStream, status: u16) -> io::Result<()> {
    write!(
        socket,
        "HTTP/1.1 {status} {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
        status_text(status)
    )?;
    socket.flush()
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        415 => "Unsupported Media Type",
        426 => "Upgrade Required",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Unknown",
    }
}

fn block_on_response(future: NativeSessionBrokerResponseFuture) -> Option<BrokerHttpResponse> {
    struct ThreadWaker(thread::Thread);
    impl Wake for ThreadWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    let mut future = future;
    let waker = Waker::from(Arc::new(ThreadWaker(thread::current())));
    let mut context = Context::from_waker(&waker);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => thread::park_timeout(Duration::from_millis(10)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_header_probe_releases_its_connection_without_waiting_for_timeout() {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        drop(client);
        let (send, receive) = std::sync::mpsc::channel();
        let worker = thread::spawn(move || {
            let result = peek_http_head(&server);
            let _ = send.send(result.map_err(|error| error.kind()));
        });
        assert_eq!(
            receive.recv_timeout(Duration::from_secs(1)).unwrap(),
            Err(io::ErrorKind::UnexpectedEof)
        );
        worker.join().unwrap();
    }

    #[test]
    fn parses_strict_origin_form_http_heads() {
        let parsed = parse_http_head(
            b"GET /session?x=1 HTTP/1.1\r\nHost: localhost\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
        )
        .unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(target_path(&parsed.target), "/session");
        assert!(valid_origin_target(&parsed.target));
        assert!(is_websocket_upgrade(&parsed.headers));
        assert!(!valid_origin_target("//attacker.example/session"));
        assert!(parse_http_head(b"GET / HTTP/1.0\r\n\r\n").is_err());
    }

    #[test]
    fn peer_enforces_per_peer_and_aggregate_outbound_budgets() {
        let aggregate = ResourceBudget::with_code(8, "total", BrokerCapacityCode::Busy);
        let admission = ResourceBudget::new(1, "admission").reserve(1).unwrap();
        let peer = NativePeer::new(8, aggregate.clone(), admission);
        peer.send("12345678").unwrap();
        assert_eq!(aggregate.used(), 8);
        assert_eq!(peer.send("x"), Err("busy".into()));
        assert_eq!(peer.take_close().unwrap().0, 1013);
        peer.clear_outbound();
        assert_eq!(aggregate.used(), 0);
    }

    #[test]
    fn authentication_admission_is_released_idempotently() {
        let budget = ResourceBudget::new(1, "admission");
        let peer = NativePeer::new(
            8,
            ResourceBudget::new(8, "outbound"),
            budget.reserve(1).unwrap(),
        );
        assert_eq!(budget.used(), 1);
        peer.mark_authenticated();
        peer.mark_authenticated();
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn retired_peer_rejects_late_sends_without_retaining_capacity() {
        let admission_budget = ResourceBudget::new(1, "admission");
        let outbound_budget = ResourceBudget::new(8, "outbound");
        let peer = NativePeer::new(
            8,
            outbound_budget.clone(),
            admission_budget.reserve(1).unwrap(),
        );
        peer.send("1234").unwrap();
        peer.retire();
        assert_eq!(peer.send("late"), Err("closed".into()));
        assert_eq!(admission_budget.used(), 0);
        assert_eq!(outbound_budget.used(), 0);
    }
}
