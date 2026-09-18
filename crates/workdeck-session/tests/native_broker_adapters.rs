//! Executable parity corpus for Hunk's Bun and Node session-broker adapters.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tungstenite::protocol::Message;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{WebSocket, connect};
use workdeck_session::*;

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
type TestSocket = WebSocket<MaybeTlsStream<TcpStream>>;
type ValueParser<T> = Arc<dyn Fn(&Value) -> Option<T> + Send + Sync>;

fn adapter_corpus() -> Value {
    serde_json::from_str(include_str!(
        "fixtures/session_broker_adapter_conformance.json"
    ))
    .unwrap()
}

fn parsers(
    parse_snapshot: Option<ValueParser<SessionSnapshot<TestState>>>,
) -> Arc<SessionBrokerProtocolParsers<TestInfo, TestState, Value, Value>> {
    let parse_snapshot = parse_snapshot.unwrap_or_else(|| {
        Arc::new(|value| {
            parse_session_snapshot_envelope(value, |state| {
                serde_json::from_value(state.clone()).ok()
            })
        })
    });
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
            parse_snapshot,
            commands: vec![SessionBrokerCommandParsers {
                command: "annotate".into(),
                version: 1,
                parse_input: Arc::new(|value| {
                    value
                        .get("summary")
                        .and_then(Value::as_str)
                        .map(|summary| json!({"summary": summary}))
                }),
                parse_result: Arc::new(|value| {
                    (value.get("applied") == Some(&Value::Bool(true)))
                        .then(|| json!({"applied": true}))
                }),
            }],
        })
        .unwrap(),
    )
}

fn broker_with_parsers(
    parsers: Arc<SessionBrokerProtocolParsers<TestInfo, TestState, Value, Value>>,
) -> Arc<TestBroker> {
    Arc::new(
        SessionBroker::new(SessionBrokerOptions {
            protocol_parsers: parsers,
            limit_options: SessionBrokerLimitOptions::default(),
            describe_session: None,
        })
        .unwrap(),
    )
}

fn daemon_with(broker: Arc<TestBroker>, patch: SessionBrokerLimitPatch) -> TestDaemon {
    let mut options = SessionBrokerDaemonOptions::new(broker);
    options.limit_options.limits = patch;
    SessionBrokerDaemon::new(options).unwrap()
}

fn registration(session_id: &str) -> Value {
    json!({
        "registrationVersion": SESSION_BROKER_REGISTRATION_VERSION,
        "sessionId": session_id,
        "pid": u64::from(std::process::id()),
        "cwd": "/repo",
        "repoRoot": "/repo",
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

fn register_text(session_id: &str) -> String {
    serde_json::to_string(&json!({
        "type": "register",
        "registration": registration(session_id),
        "snapshot": snapshot(0),
    }))
    .unwrap()
}

fn start(daemon: TestDaemon) -> RunningSessionBrokerDaemon {
    serve_session_broker_daemon(ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0))
        .unwrap()
}

fn open_socket(address: SocketAddr) -> TestSocket {
    let (mut socket, _) = connect(format!("ws://{address}/session")).unwrap();
    if let MaybeTlsStream::Plain(stream) = socket.get_mut() {
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .unwrap();
    }
    socket
}

fn close_code(socket: &mut TestSocket) -> u16 {
    loop {
        match socket.read() {
            Ok(Message::Close(Some(frame))) => return frame.code.into(),
            Ok(Message::Close(None)) => return 1005,
            Ok(_) => {}
            Err(error) => panic!("expected a WebSocket close frame: {error}"),
        }
    }
}

fn wait_until(label: &str, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !predicate() {
        assert!(Instant::now() < deadline, "timed out waiting for {label}");
        thread::sleep(Duration::from_millis(10));
    }
}

struct HttpResult {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn http_request(address: SocketAddr, method: &str, path: &str, body: &[u8]) -> HttpResult {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    write!(
        socket,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    socket.write_all(body).unwrap();
    socket.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).unwrap();
    parse_http_response(&response)
}

fn parse_http_response(response: &[u8]) -> HttpResult {
    let head_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
        .expect("HTTP response head");
    let head = std::str::from_utf8(&response[..head_end]).unwrap();
    let mut lines = head[..head.len() - 4].split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect();
    HttpResult {
        status,
        headers,
        body: response[head_end..].to_vec(),
    }
}

fn raw_http(address: SocketAddr, request: &[u8]) -> HttpResult {
    let mut socket = TcpStream::connect(address).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    socket.write_all(request).unwrap();
    socket.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    if let Err(error) = socket.read_to_end(&mut response) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        assert!(
            !response.is_empty(),
            "connection reset before HTTP response"
        );
    }
    parse_http_response(&response)
}

fn raw_upgrade_status(address: SocketAddr) -> u16 {
    let mut socket = TcpStream::connect(address).unwrap();
    write!(
        socket,
        "GET /session HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGVzdC1zZXNzaW9uLWtleQ==\r\n\r\n"
    )
    .unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut response = Vec::new();
    if let Err(error) = socket.read_to_end(&mut response) {
        assert_eq!(error.kind(), std::io::ErrorKind::ConnectionReset);
        assert!(
            !response.is_empty(),
            "upgrade reset before an HTTP response"
        );
    }
    parse_http_response(&response).status
}

fn raw_websocket(address: SocketAddr) -> TcpStream {
    let mut socket = TcpStream::connect(address).unwrap();
    write!(
        socket,
        "GET /session HTTP/1.1\r\nHost: {address}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGVzdC1zZXNzaW9uLWtleQ==\r\n\r\n"
    )
    .unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut response = Vec::new();
    while !response.ends_with(b"\r\n\r\n") {
        let mut byte = [0_u8; 1];
        socket.read_exact(&mut byte).unwrap();
        response.push(byte[0]);
    }
    assert_eq!(parse_http_response(&response).status, 101);
    socket
}

fn raw_close_code(socket: &mut TcpStream) -> u16 {
    let mut prefix = [0_u8; 2];
    socket.read_exact(&mut prefix).unwrap();
    assert_eq!(prefix[0] & 0x0f, 8, "expected a close frame");
    assert_eq!(prefix[1] & 0x80, 0, "server frames must not be masked");
    let length = usize::from(prefix[1] & 0x7f);
    assert!((2..126).contains(&length));
    let mut payload = vec![0_u8; length];
    socket.read_exact(&mut payload).unwrap();
    u16::from_be_bytes([payload[0], payload[1]])
}

fn text_response(text: &str) -> BrokerHttpResponse {
    BrokerHttpResponse {
        status: 200,
        status_text: "OK".into(),
        headers: BTreeMap::from([("content-type".into(), "text/plain;charset=UTF-8".into())]),
        body: Some(BoundedHttpBody::Streaming(Box::new(std::io::Cursor::new(
            text.as_bytes().to_vec(),
        )))),
    }
}

fn empty_response(status: u16) -> BrokerHttpResponse {
    BrokerHttpResponse {
        status,
        status_text: String::new(),
        headers: BTreeMap::new(),
        body: None,
    }
}

fn handler(
    callback: impl Fn(SessionBrokerHttpRequest) -> Option<BrokerHttpResponse> + Send + Sync + 'static,
) -> NativeSessionBrokerHttpHandler {
    let callback = Arc::new(callback);
    Arc::new(move |request, _address| {
        let result = callback(request);
        Box::pin(async move { result })
    })
}

fn stop(server: &RunningSessionBrokerDaemon) {
    server.stop();
    assert!(server.wait_stopped(Duration::from_secs(2)));
}

#[test]
fn closes_binary_and_oversized_messages_per_shared_corpus() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(
        broker,
        SessionBrokerLimitPatch {
            max_ws_message_bytes: Some(8),
            ..SessionBrokerLimitPatch::default()
        },
    );
    let server = start(daemon);

    let mut binary = open_socket(server.address());
    binary.send(Message::Binary(vec![1].into())).unwrap();
    assert_eq!(
        close_code(&mut binary),
        adapter_corpus()["textOnly"]["binaryCloseCode"]
            .as_u64()
            .unwrap() as u16
    );

    let mut oversized = open_socket(server.address());
    oversized.send(Message::Text("123456789".into())).unwrap();
    assert_eq!(
        close_code(&mut oversized),
        adapter_corpus()["inbound"]["oversizedCloseCode"]
            .as_u64()
            .unwrap() as u16
    );
    stop(&server);
}

#[test]
fn closes_malformed_utf8_text_with_protocol_code_1007() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let server = start(daemon);
    let mut socket = raw_websocket(server.address());
    let mask = [1_u8, 2, 3, 4];
    let invalid = [0xc0_u8, 0xaf];
    socket
        .write_all(&[
            0x81,
            0x82,
            mask[0],
            mask[1],
            mask[2],
            mask[3],
            invalid[0] ^ mask[0],
            invalid[1] ^ mask[1],
        ])
        .unwrap();
    assert_eq!(raw_close_code(&mut socket), 1007);
    stop(&server);
}

struct RejectingHello;

fn authentication_error() -> SessionBrokerAuthenticationError {
    SessionBrokerAuthenticationError {
        code: SessionBrokerAuthenticationFailureCode::AuthenticationRequired,
    }
}

impl SessionBrokerHelloAuthenticator for RejectingHello {
    fn issue_hello_challenge(
        &self,
        _request: Value,
        _listener_endpoint: &str,
    ) -> Result<SessionBrokerHelloChallenge, SessionBrokerAuthenticationError> {
        Err(authentication_error())
    }

    fn complete_caller_hello_proof(
        &self,
        _proof: Value,
    ) -> Result<AuthenticatedCallerSession, SessionBrokerAuthenticationError> {
        Err(authentication_error())
    }

    fn complete_producer_hello_proof(
        &self,
        _proof: Value,
        _connection_id: &str,
    ) -> Result<AuthenticatedProducerHello, SessionBrokerAuthenticationError> {
        Err(authentication_error())
    }
}

fn authenticated_daemon(max_sockets: u64, handshake_ms: u64) -> TestDaemon {
    let broker = broker_with_parsers(parsers(None));
    let mut options = SessionBrokerDaemonOptions::new(broker);
    options.limit_options.limits = SessionBrokerLimitPatch {
        max_unauthenticated_sockets: Some(max_sockets),
        max_handshake_duration_ms: Some(handshake_ms),
        ..SessionBrokerLimitPatch::default()
    };
    options.hello_authenticator = Some(Arc::new(RejectingHello));
    options.producer_endpoint = Some("ws://127.0.0.1/session".into());
    SessionBrokerDaemon::new(options).unwrap()
}

#[test]
fn returns_shared_http_status_when_socket_admission_is_full_and_releases_on_close() {
    let server = start(authenticated_daemon(1, 1_000));
    let mut first = open_socket(server.address());
    assert_eq!(
        raw_upgrade_status(server.address()),
        adapter_corpus()["inbound"]["admissionHttpStatus"]
            .as_u64()
            .unwrap() as u16
    );
    first.close(None).unwrap();
    let _ = first.read();
    thread::sleep(Duration::from_millis(30));
    let mut after_release = open_socket(server.address());
    after_release.close(None).unwrap();
    stop(&server);
}

#[test]
fn contains_handler_failures_and_closes_the_affected_peer() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.message_handler = Some(Arc::new(|_peer, message| match message.as_str() {
        Some("capacity") => Err(NativeSessionBrokerMessageError::Capacity),
        _ => Err(NativeSessionBrokerMessageError::Failure(
            "unexpected".into(),
        )),
    }));
    let server = serve_session_broker_daemon(options).unwrap();
    let mut capacity = open_socket(server.address());
    capacity.send(Message::Text("capacity".into())).unwrap();
    assert_eq!(close_code(&mut capacity), 1013);
    let mut unexpected = open_socket(server.address());
    unexpected.send(Message::Text("unexpected".into())).unwrap();
    assert_eq!(close_code(&mut unexpected), 1011);
    stop(&server);
}

#[test]
fn accepts_a_websocket_upgrade_after_tcp_connect_without_early_header_bytes() {
    let broker = broker_with_parsers(parsers(None));
    let server = start(daemon_with(broker, SessionBrokerLimitPatch::default()));
    let stream = TcpStream::connect(server.address()).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    // Accept and dispatch the connection before any HTTP bytes arrive. A
    // nonblocking accepted socket must not mistake WouldBlock for disconnect.
    thread::sleep(Duration::from_millis(100));
    let (mut socket, _) =
        tungstenite::client(format!("ws://{}/session", server.address()), stream).unwrap();
    socket
        .send(Message::Text(register_text("delayed-headers").into()))
        .unwrap();
    socket.close(None).unwrap();
    stop(&server);
}

#[test]
fn accepts_websocket_message_exactly_at_configured_byte_ceiling() {
    let broker = broker_with_parsers(parsers(None));
    let message = register_text("session-1");
    let daemon = daemon_with(
        Arc::clone(&broker),
        SessionBrokerLimitPatch {
            max_ws_message_bytes: Some(message.len() as u64),
            ..SessionBrokerLimitPatch::default()
        },
    );
    let server = start(daemon);
    let mut socket = open_socket(server.address());
    socket.send(Message::Text(message.into())).unwrap();
    wait_until("exact-ceiling registration", || broker.session_count() == 1);
    socket.close(None).unwrap();
    stop(&server);
}

#[test]
fn closes_outbound_aggregate_pressure_and_releases_capacity_for_reconnect() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(
        Arc::clone(&broker),
        SessionBrokerLimitPatch {
            max_outbound_bytes_total: Some(8),
            ..SessionBrokerLimitPatch::default()
        },
    );
    let server = start(daemon);
    let mut socket = open_socket(server.address());
    socket
        .send(Message::Text(register_text("session-1").into()))
        .unwrap();
    wait_until("pressure registration", || broker.session_count() == 1);
    let pending = broker
        .dispatch_command(DispatchSessionCommand::new(
            SessionSelector {
                session_id: Some("session-1".into()),
                ..SessionSelector::default()
            },
            "annotate",
            json!({"summary": "pressure"}),
            "timeout",
        ))
        .unwrap();
    assert_eq!(
        close_code(&mut socket),
        adapter_corpus()["outbound"]["pressureCloseCode"]
            .as_u64()
            .unwrap() as u16
    );
    assert!(pending.receive().is_err());
    wait_until("pressure cleanup", || broker.session_count() == 0);
    let mut after_release = open_socket(server.address());
    after_release.close(None).unwrap();
    stop(&server);
}

#[test]
fn manual_stop_retires_peer_and_rejects_late_message_delivery() {
    let snapshot_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::clone(&snapshot_calls);
    let parse_snapshot: ValueParser<SessionSnapshot<TestState>> = Arc::new(move |value| {
        calls.fetch_add(1, Ordering::AcqRel);
        parse_session_snapshot_envelope(value, |state| serde_json::from_value(state.clone()).ok())
    });
    let broker = broker_with_parsers(parsers(Some(parse_snapshot)));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let server = start(daemon);
    let mut socket = open_socket(server.address());
    socket
        .send(Message::Text(register_text("session-1").into()))
        .unwrap();
    wait_until("initial snapshot parse", || {
        snapshot_calls.load(Ordering::Acquire) == 1
    });
    server.stop();
    let late = serde_json::to_string(&json!({
        "type": "snapshot",
        "sessionId": "session-1",
        "snapshot": snapshot(1),
    }))
    .unwrap();
    let _ = socket.send(Message::Text(late.into()));
    assert!(server.wait_stopped(Duration::from_secs(2)));
    assert_eq!(snapshot_calls.load(Ordering::Acquire), 1);
}

#[test]
fn waits_for_active_custom_http_handler_before_stopped_settles() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let gate = Arc::new((Mutex::new((false, false)), Condvar::new()));
    let handler_gate = Arc::clone(&gate);
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.handle_request = Some(handler(move |_request| {
        let (lock, changed) = &*handler_gate;
        let mut state = lock.lock().unwrap();
        state.0 = true;
        changed.notify_all();
        while !state.1 {
            state = changed.wait(state).unwrap();
        }
        Some(text_response("done"))
    }));
    let server = serve_session_broker_daemon(options).unwrap();
    let address = server.address();
    let request = thread::spawn(move || http_request(address, "GET", "/deferred", b""));
    {
        let (lock, changed) = &*gate;
        let mut state = lock.lock().unwrap();
        while !state.0 {
            state = changed.wait(state).unwrap();
        }
    }
    server.stop();
    assert!(!server.wait_stopped(Duration::from_millis(30)));
    assert!(connect(format!("ws://{}/session", server.address())).is_err());
    {
        let (lock, changed) = &*gate;
        lock.lock().unwrap().1 = true;
        changed.notify_all();
    }
    let _ = request.join().unwrap();
    assert!(server.wait_stopped(Duration::from_secs(2)));
}

#[test]
fn node_adapter_preserves_bodyless_framing_headers_and_waits_for_handlers() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let observed = Arc::new(Mutex::new(Vec::new()));
    let handler_observed = Arc::clone(&observed);
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.adapter_semantics = NativeSessionBrokerAdapterSemantics::Node;
    options.handle_request = Some(handler(move |request| {
        if request.url.ends_with("/capabilities") {
            let framed = request.headers.contains_key("content-length")
                || request.headers.contains_key("transfer-encoding");
            handler_observed.lock().unwrap().push(framed);
            return Some(empty_response(if framed { 400 } else { 200 }));
        }
        None
    }));
    let server = serve_session_broker_daemon(options).unwrap();
    let fixed = http_request(server.address(), "GET", "/capabilities", b"x");
    assert_eq!(fixed.status, 400);
    let chunked = raw_http(
        server.address(),
        format!(
            "GET /capabilities HTTP/1.1\r\nHost: {}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n1\r\nx\r\n0\r\n\r\n",
            server.address()
        )
        .as_bytes(),
    );
    assert_eq!(chunked.status, 400);
    assert_eq!(*observed.lock().unwrap(), vec![true, true]);
    stop(&server);
}

#[test]
fn daemon_idle_shutdown_reuses_the_transport_stop_path() {
    let broker = broker_with_parsers(parsers(None));
    let mut options = SessionBrokerDaemonOptions::new(broker);
    options.idle_timeout_ms = Some(20);
    options.stale_session_sweep_interval_ms = Some(5);
    let server = start(SessionBrokerDaemon::new(options).unwrap());
    assert!(server.wait_stopped(Duration::from_secs(2)));
}

#[test]
fn admits_exactly_configured_number_of_unauthenticated_websocket_peers() {
    let server = start(authenticated_daemon(1, 50));
    let mut first = open_socket(server.address());
    assert_eq!(
        raw_upgrade_status(server.address()),
        adapter_corpus()["inbound"]["admissionHttpStatus"]
            .as_u64()
            .unwrap() as u16
    );
    assert_eq!(close_code(&mut first), 1008);
    let mut after_release = open_socket(server.address());
    after_release.close(None).unwrap();
    stop(&server);
}

struct AllowAuthenticator;

impl CallerRequestAuthenticator for AllowAuthenticator {
    fn authenticate_request(
        &self,
        _input: &CallerRequestAuthenticationInput,
    ) -> Result<AuthenticatedCallerRequest, SessionBrokerAuthenticationError> {
        Ok(AuthenticatedCallerRequest::from_callbacks(
            CallerPrincipal {
                app_id: "test.app".into(),
                principal_id: "test-caller".into(),
                key_id: "test-key".into(),
                grant_id: "test-grant".into(),
                session_id: None,
                operations: vec![CallerOperation::List, CallerOperation::Get],
                commands: Vec::new(),
            },
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
                    body_digest: "test-digest".into(),
                    daemon_key_id: "daemon-key-1".into(),
                    daemon_signature: "test-signature".into(),
                })
            }),
        ))
    }

    fn clear_authentication(&self) {}
}

struct AllowAuthorizer;

impl SessionBrokerAuthorizer for AllowAuthorizer {
    fn authorize<'a>(
        &'a self,
        _context: &'a SessionBrokerAuthorizationContext,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async { true })
    }
}

fn exposed_daemon(broker: Arc<TestBroker>) -> TestDaemon {
    let mut options = SessionBrokerDaemonOptions::new(broker);
    options.capabilities = Some(SessionBrokerCapabilities {
        version: 1,
        name: None,
        features: None,
        extra: BTreeMap::new(),
    });
    options.expose_http_api = true;
    options.app_id = Some("test.app".into());
    options.app_revision = Some(1);
    options.caller_authenticator = Some(Arc::new(AllowAuthenticator));
    options.authorizer = Some(Arc::new(AllowAuthorizer));
    SessionBrokerDaemon::new(options).unwrap()
}

fn assert_generic_daemon_api_and_websocket_path(
    adapter_semantics: NativeSessionBrokerAdapterSemantics,
) {
    let broker = broker_with_parsers(parsers(None));
    let mut options =
        ServeSessionBrokerDaemonOptions::new(exposed_daemon(Arc::clone(&broker)), "127.0.0.1", 0);
    options.adapter_semantics = adapter_semantics;
    let server = serve_session_broker_daemon(options).unwrap();
    let health = http_request(server.address(), "GET", "/health", b"");
    assert_eq!(health.status, 200);
    assert_eq!(
        serde_json::from_slice::<Value>(&health.body).unwrap(),
        json!({"ok": true})
    );
    let mut socket = open_socket(server.address());
    socket
        .send(Message::Text(register_text("session-1").into()))
        .unwrap();
    wait_until("session registration", || broker.session_count() == 1);
    let list = http_request(
        server.address(),
        "POST",
        "/broker",
        serde_json::to_string(&json!({"action": "list"}))
            .unwrap()
            .as_bytes(),
    );
    assert_eq!(list.status, 200);
    let list: Value = serde_json::from_slice(&list.body).unwrap();
    assert_eq!(list["body"]["sessions"][0]["sessionId"], "session-1");
    let get = http_request(
        server.address(),
        "POST",
        "/broker",
        serde_json::to_string(&json!({
            "action": "get",
            "selector": {"sessionId": "session-1"},
        }))
        .unwrap()
        .as_bytes(),
    );
    assert_eq!(get.status, 200);
    let get: Value = serde_json::from_slice(&get.body).unwrap();
    assert_eq!(
        get["body"]["session"]["snapshot"]["state"]["selectedIndex"],
        0
    );
    socket.close(None).unwrap();
    stop(&server);
}

#[test]
fn serves_generic_daemon_api_and_websocket_path_through_bun_parity_surface() {
    assert_generic_daemon_api_and_websocket_path(NativeSessionBrokerAdapterSemantics::Bun);
}

#[test]
fn serves_custom_http_response_exactly_at_byte_ceiling() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(
        broker,
        SessionBrokerLimitPatch {
            max_http_response_bytes: Some(4),
            ..SessionBrokerLimitPatch::default()
        },
    );
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.handle_request = Some(handler(|_| Some(text_response("1234"))));
    let server = serve_session_broker_daemon(options).unwrap();
    let response = http_request(server.address(), "GET", "/exact", b"");
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"1234");
    stop(&server);
}

#[test]
fn releases_bounded_response_capacity_when_head_suppresses_body() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(
        broker,
        SessionBrokerLimitPatch {
            max_http_response_bytes: Some(4),
            max_in_flight_http_response_bytes: Some(8),
            ..SessionBrokerLimitPatch::default()
        },
    );
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.handle_request = Some(handler(|_| Some(text_response("1234"))));
    let server = serve_session_broker_daemon(options).unwrap();
    let head = http_request(server.address(), "HEAD", "/head", b"");
    assert_eq!(head.status, 200);
    assert!(head.body.is_empty());
    assert_eq!(
        head.headers.get("content-length").map(String::as_str),
        Some("4")
    );
    let after = http_request(server.address(), "GET", "/after-head", b"");
    assert_eq!(after.status, 200);
    assert_eq!(after.body, b"1234");
    stop(&server);
}

#[test]
fn falls_back_to_empty_503_when_capacity_envelope_exceeds_response_cap() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(
        broker,
        SessionBrokerLimitPatch {
            max_http_response_bytes: Some(1),
            ..SessionBrokerLimitPatch::default()
        },
    );
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.handle_request = Some(handler(|_| Some(text_response("too large"))));
    let server = serve_session_broker_daemon(options).unwrap();
    let response = http_request(server.address(), "GET", "/large", b"");
    assert_eq!(response.status, 503);
    assert!(response.body.is_empty());
    stop(&server);
}

#[test]
fn custom_request_handlers_override_generic_routes() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.handle_request = Some(handler(|request| {
        request
            .url
            .ends_with("/health")
            .then(|| BrokerHttpResponse {
                status: 200,
                status_text: "OK".into(),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: Some(BoundedHttpBody::Streaming(Box::new(std::io::Cursor::new(
                    br#"{"ok":true,"overridden":true}"#.to_vec(),
                )))),
            })
    }));
    let server = serve_session_broker_daemon(options).unwrap();
    let response = http_request(server.address(), "GET", "/health", b"");
    assert_eq!(
        serde_json::from_slice::<Value>(&response.body).unwrap(),
        json!({"ok": true, "overridden": true})
    );
    stop(&server);
}

#[test]
fn custom_not_found_handler_runs_only_after_shared_routes() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.not_found = Some(handler(|_| Some(text_response("custom missing"))));
    let server = serve_session_broker_daemon(options).unwrap();
    let missing = http_request(server.address(), "GET", "/missing", b"");
    assert_eq!(missing.status, 200);
    assert_eq!(missing.body, b"custom missing");
    let health = http_request(server.address(), "GET", "/health", b"");
    assert_eq!(health.status, 200);
    assert_ne!(health.body, b"custom missing");
    stop(&server);
}

#[test]
fn serves_generic_daemon_api_and_websocket_path_through_node_parity_surface() {
    assert_generic_daemon_api_and_websocket_path(NativeSessionBrokerAdapterSemantics::Node);
}

#[test]
fn retains_bun_and_node_non_upgrade_socket_path_semantics() {
    for (semantics, expected) in [
        (NativeSessionBrokerAdapterSemantics::Bun, 426),
        (NativeSessionBrokerAdapterSemantics::Node, 404),
    ] {
        let broker = broker_with_parsers(parsers(None));
        let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
        let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
        options.adapter_semantics = semantics;
        let server = serve_session_broker_daemon(options).unwrap();
        assert_eq!(
            http_request(server.address(), "GET", "/session", b"").status,
            expected
        );
        stop(&server);
    }
}

#[test]
fn decodes_bounded_chunked_request_bodies_before_custom_dispatch() {
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.handle_request = Some(handler(|request| {
        request.url.ends_with("/echo").then(|| BrokerHttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: BTreeMap::new(),
            body: Some(BoundedHttpBody::Streaming(Box::new(std::io::Cursor::new(
                request.body,
            )))),
        })
    }));
    let server = serve_session_broker_daemon(options).unwrap();
    let mut socket = TcpStream::connect(server.address()).unwrap();
    write!(
        socket,
        "POST /echo HTTP/1.1\r\nHost: {}\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n4\r\n1234\r\n3\r\n567\r\n0\r\nX-Trace: done\r\n\r\n",
        server.address()
    )
    .unwrap();
    socket.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    socket.read_to_end(&mut response).unwrap();
    let response = parse_http_response(&response);
    assert_eq!(response.status, 200);
    assert_eq!(response.body, b"1234567");
    stop(&server);
}

#[test]
fn formats_synchronous_bind_failures_with_the_configured_callback() {
    let occupied = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = occupied.local_addr().unwrap().port();
    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let cleanup = daemon.clone();
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", port);
    options.format_serve_error = Some(Arc::new(|_error, address| {
        format!(
            "custom bind failure on {}:{}",
            address.hostname, address.port
        )
    }));
    let error = serve_session_broker_daemon(options).err().unwrap();
    assert_eq!(
        error.to_string(),
        format!("custom bind failure on 127.0.0.1:{port}")
    );
    cleanup.shutdown(None);
}

#[test]
fn native_ed25519_and_base64url_replace_node_webcrypto_without_runtime_globals() {
    let mut private_der = vec![
        0x30, 0x2e, 0x02, 0x01, 0x00, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x04, 0x22, 0x04,
        0x20,
    ];
    private_der.extend([
        0x9d, 0x61, 0xb1, 0x9d, 0xef, 0xfd, 0x5a, 0x60, 0xba, 0x84, 0x4a, 0xf4, 0x92, 0xec, 0x2c,
        0xc4, 0x44, 0x49, 0xc5, 0x69, 0x7b, 0x32, 0x69, 0x19, 0x70, 0x3b, 0xac, 0x03, 0x1c, 0xae,
        0x7f, 0x60,
    ]);
    let mut public_der = vec![
        0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
    ];
    public_der.extend([
        0xd7, 0x5a, 0x98, 0x01, 0x82, 0xb1, 0x0a, 0xb7, 0xd5, 0x4b, 0xfe, 0xd3, 0xc9, 0x64, 0x07,
        0x3a, 0x0e, 0xe1, 0x72, 0xf3, 0xda, 0xa6, 0x23, 0x25, 0xaf, 0x02, 0x1a, 0x68, 0xf7, 0x07,
        0x51, 0x1a,
    ]);
    let private_key = import_ed25519_private_key(&private_der).unwrap();
    let public_key = import_ed25519_public_key(&public_der).unwrap();
    let message = b"session-broker-node";
    let signature = NativeSessionBrokerCrypto.sign(&private_key, message);
    assert!(NativeSessionBrokerCrypto.verify(&public_key, &signature, message));
    let encoded = encode_base64_url(&signature);
    assert_eq!(decode_base64_url(&encoded), Some(signature));
}

#[test]
fn node_adapter_consumes_shared_text_binary_oversize_and_pressure_corpus() {
    let broker = broker_with_parsers(parsers(None));
    let mut daemon_options = SessionBrokerDaemonOptions::new(broker);
    daemon_options.limit_options.limits = SessionBrokerLimitPatch {
        max_ws_message_bytes: Some(8),
        max_http_response_bytes: Some(8),
        max_unauthenticated_sockets: Some(1),
        max_handshake_duration_ms: Some(1_000),
        ..SessionBrokerLimitPatch::default()
    };
    daemon_options.hello_authenticator = Some(Arc::new(RejectingHello));
    daemon_options.producer_endpoint = Some("ws://127.0.0.1/session".into());
    let daemon = SessionBrokerDaemon::new(daemon_options).unwrap();
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.adapter_semantics = NativeSessionBrokerAdapterSemantics::Node;
    options.message_handler = Some(Arc::new(|_, _| Ok(())));
    options.handle_request = Some(handler(|request| {
        request.url.ends_with("/large").then(|| BrokerHttpResponse {
            status: 200,
            status_text: "OK".into(),
            headers: BTreeMap::from([("content-length".into(), "9".into())]),
            body: Some(BoundedHttpBody::Streaming(Box::new(std::io::Cursor::new(
                b"123456789".to_vec(),
            )))),
        })
    }));
    let server = serve_session_broker_daemon(options).unwrap();

    let malformed = raw_http(
        server.address(),
        format!(
            "GET * HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\n\r\n",
            server.address()
        )
        .as_bytes(),
    );
    assert_eq!(malformed.status, 400);
    let bounded = http_request(server.address(), "GET", "/large", b"");
    assert_eq!(bounded.status, 503);
    assert!(bounded.body.is_empty());

    let mut exact = open_socket(server.address());
    exact.send(Message::Text("12345678".into())).unwrap();
    thread::sleep(Duration::from_millis(20));
    assert!(exact.can_write());
    assert_eq!(
        raw_upgrade_status(server.address()),
        adapter_corpus()["inbound"]["admissionHttpStatus"]
            .as_u64()
            .unwrap() as u16
    );
    exact.close(None).unwrap();
    let _ = exact.read();
    thread::sleep(Duration::from_millis(20));

    let mut binary = open_socket(server.address());
    binary.send(Message::Binary(vec![1].into())).unwrap();
    assert_eq!(close_code(&mut binary), 1003);
    let mut oversized = open_socket(server.address());
    oversized.send(Message::Text("123456789".into())).unwrap();
    assert_eq!(close_code(&mut oversized), 1009);
    let mut malformed_utf8 = raw_websocket(server.address());
    malformed_utf8
        .write_all(&[0x81, 0x82, 1, 2, 3, 4, 0xc0 ^ 1, 0xaf ^ 2])
        .unwrap();
    assert_eq!(raw_close_code(&mut malformed_utf8), 1007);
    stop(&server);

    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(
        broker,
        SessionBrokerLimitPatch {
            max_outbound_bytes_per_peer: Some(1),
            ..SessionBrokerLimitPatch::default()
        },
    );
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.adapter_semantics = NativeSessionBrokerAdapterSemantics::Node;
    options.message_handler = Some(Arc::new(|peer, _| {
        peer.send("too large")
            .map_err(|_| NativeSessionBrokerMessageError::Capacity)
    }));
    let outbound_server = serve_session_broker_daemon(options).unwrap();
    let mut outbound = open_socket(outbound_server.address());
    outbound.send(Message::Text("trigger".into())).unwrap();
    assert_eq!(
        close_code(&mut outbound),
        adapter_corpus()["outbound"]["pressureCloseCode"]
            .as_u64()
            .unwrap() as u16
    );
    stop(&outbound_server);

    let broker = broker_with_parsers(parsers(None));
    let daemon = daemon_with(broker, SessionBrokerLimitPatch::default());
    let mut options = ServeSessionBrokerDaemonOptions::new(daemon, "127.0.0.1", 0);
    options.adapter_semantics = NativeSessionBrokerAdapterSemantics::Node;
    options.message_handler = Some(Arc::new(|_, _| panic!("contained handler failure")));
    let handler_server = serve_session_broker_daemon(options).unwrap();
    let mut failed = open_socket(handler_server.address());
    failed.send(Message::Text("trigger".into())).unwrap();
    assert_eq!(close_code(&mut failed), 1011);
    stop(&handler_server);
}
