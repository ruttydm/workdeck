use super::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Instant;
use workdeck_core::{ReviewNoteSource, ReviewSide};

use crate::{
    BrokerCommandOutcome, DaemonSessionSocket, NativeSessionBrokerClientSocket,
    RegisterSessionOptions, RegisterSessionResult, SESSION_BROKER_HOST_ENV,
    SESSION_BROKER_PORT_ENV, SESSION_BROKER_REGISTRATION_VERSION, SessionBrokerCallerClient,
    SessionBrokerCallerClientOptions, SessionBrokerClientCredential,
    SessionBrokerClientHttpRequest, SessionBrokerClientHttpResponse, SessionBrokerConnection,
    SessionBrokerConnectionOptions, SessionBrokerDaemonVerifier,
    SessionBrokerProducerAuthentication, SessionBrokerSignedRequestInit,
    SessionBrokerSocketCloseEvent, SessionBrokerSocketLike, SessionBrokerStateError,
    SessionFileSummary, SessionLiveCommentSummary, SessionReviewFile, SessionReviewHunk,
    SessionReviewNoteSummary, SharedDaemonSessionSocket, UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV,
    WORKDECK_REVIEW_CAPABILITY_HEADER, WorkdeckSessionCliClient, WorkdeckSessionCommandInput,
    WorkdeckSessionCommandResult, WorkdeckSessionInfo, WorkdeckSessionInputKind,
    WorkdeckSessionRegistration, WorkdeckSessionSnapshot, WorkdeckSessionState,
    create_workdeck_session_protocol_parsers, load_or_create_workdeck_session_broker_credentials,
};

const PORT: u16 = 7_000;

#[derive(Debug)]
struct NativeHttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl NativeHttpResponse {
    fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap()
    }
}

fn send_native_http(
    port: u16,
    method: &str,
    path: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
) -> Result<NativeHttpResponse, String> {
    let timeout = Duration::from_secs(3);
    let mut stream = TcpStream::connect(("127.0.0.1", port)).map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    let has_host = headers.keys().any(|name| name.eq_ignore_ascii_case("host"));
    let mut encoded = format!("{method} {path} HTTP/1.1\r\n").into_bytes();
    if !has_host {
        encoded.extend_from_slice(format!("Host: 127.0.0.1:{port}\r\n").as_bytes());
    }
    for (name, value) in headers {
        encoded.extend_from_slice(name.as_bytes());
        encoded.extend_from_slice(b": ");
        encoded.extend_from_slice(value.as_bytes());
        encoded.extend_from_slice(b"\r\n");
    }
    encoded.extend_from_slice(
        format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    );
    encoded.extend_from_slice(body);
    let write_result = stream.write_all(&encoded);
    let _ = stream.shutdown(Shutdown::Write);
    let mut response = Vec::new();
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            Err(error)
                if error.kind() == std::io::ErrorKind::ConnectionReset && !response.is_empty() =>
            {
                break;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
    if response.is_empty() {
        return Err(write_result
            .err()
            .map_or_else(|| "empty HTTP response".into(), |error| error.to_string()));
    }
    let boundary = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "malformed HTTP response".to_owned())?;
    let head = std::str::from_utf8(&response[..boundary]).map_err(|error| error.to_string())?;
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_ascii_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| "malformed HTTP status".to_owned())?;
    let headers = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_owned()))
        .collect();
    Ok(NativeHttpResponse {
        status,
        headers,
        body: response[boundary + 4..].to_vec(),
    })
}

#[derive(Debug)]
struct TestHttpTransport;

impl crate::SessionBrokerClientHttpTransport for TestHttpTransport {
    fn send(
        &self,
        request: SessionBrokerClientHttpRequest,
    ) -> Result<SessionBrokerClientHttpResponse, String> {
        let url = Url::parse(&request.url).map_err(|error| error.to_string())?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| "missing request port".to_owned())?;
        let mut path = url.path().to_owned();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        let response = send_native_http(
            port,
            &request.method,
            &path,
            &request.headers,
            &request.body,
        )?;
        Ok(SessionBrokerClientHttpResponse {
            status: response.status,
            headers: response.headers,
            body: Some(Box::new(std::io::Cursor::new(response.body))),
        })
    }
}

fn signed_caller(env: &BTreeMap<String, String>, port: u16) -> SessionBrokerCallerClient {
    let credentials = load_or_create_workdeck_session_broker_credentials(env, None).unwrap();
    SessionBrokerCallerClient::new(SessionBrokerCallerClientOptions::native(
        WORKDECK_SESSION_BROKER_APP_ID,
        WORKDECK_SESSION_BROKER_APP_REVISION,
        format!("http://127.0.0.1:{port}"),
        SessionBrokerClientCredential {
            grant: credentials.caller.grant,
            private_key: credentials.caller.private_key,
        },
        SessionBrokerDaemonVerifier {
            key_id: credentials.daemon_identity.key_id,
            public_key: credentials.daemon_public_key,
        },
        Arc::new(TestHttpTransport),
    ))
}

fn wait_until(label: &str, timeout: Duration, predicate: impl Fn() -> bool) {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if predicate() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for {label}");
}

fn request(headers: &[(&str, &str)]) -> SessionBrokerHttpRequest {
    SessionBrokerHttpRequest {
        method: "GET".into(),
        url: format!("http://127.0.0.1:{PORT}/"),
        headers: headers
            .iter()
            .map(|(name, value)| ((*name).into(), (*value).into()))
            .collect::<BTreeMap<_, _>>(),
        body: Vec::new(),
    }
}

fn api_request(body: Value) -> SessionBrokerHttpRequest {
    SessionBrokerHttpRequest::json(format!("http://127.0.0.1:{PORT}/session-api"), &body)
}

#[derive(Default)]
struct ApiSocket {
    state: Mutex<Weak<WorkdeckSessionBrokerState>>,
    own_socket: Mutex<Option<Weak<dyn DaemonSessionSocket>>>,
    messages: Mutex<Vec<Value>>,
    failures: Mutex<BTreeMap<String, String>>,
}

impl ApiSocket {
    fn bind(self: &Arc<Self>, state: &Arc<WorkdeckSessionBrokerState>) {
        *self.state.lock().unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(state);
        let socket: SharedDaemonSessionSocket = Arc::clone(self) as SharedDaemonSessionSocket;
        *self
            .own_socket
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(Arc::downgrade(&socket));
    }

    fn shared(self: &Arc<Self>) -> SharedDaemonSessionSocket {
        Arc::clone(self) as SharedDaemonSessionSocket
    }

    fn messages(&self) -> Vec<Value> {
        self.messages
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone()
    }

    fn fail(&self, command: &str, message: &str) {
        self.failures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(command.into(), message.into());
    }
}

impl DaemonSessionSocket for ApiSocket {
    fn send(&self, data: &str) -> Result<bool, SessionBrokerStateError> {
        let message = serde_json::from_str::<Value>(data)
            .map_err(|error| SessionBrokerStateError::message(error.to_string()))?;
        self.messages
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(message.clone());
        let command = message["command"]
            .as_str()
            .ok_or_else(|| SessionBrokerStateError::message("missing command"))?
            .to_owned();
        let request_id = message["requestId"]
            .as_str()
            .ok_or_else(|| SessionBrokerStateError::message("missing request id"))?
            .to_owned();
        let failure = self
            .failures
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&command)
            .cloned();
        let result = command_result(&command, &message);
        let state = self
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clone();
        let socket = self
            .own_socket
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .cloned()
            .ok_or_else(|| SessionBrokerStateError::message("socket is not bound"))?;
        std::thread::spawn(move || {
            if let (Some(state), Some(socket)) = (state.upgrade(), socket.upgrade()) {
                state.handle_command_result(
                    &socket,
                    &request_id,
                    failure.map_or_else(
                        || BrokerCommandOutcome::Success(result),
                        BrokerCommandOutcome::Failure,
                    ),
                );
            }
        });
        Ok(true)
    }
}

fn command_result(command: &str, message: &Value) -> Value {
    match command {
        "navigate_to_hunk" => json!({
            "fileId": "file-1", "filePath": "src/example.rs", "hunkIndex": 0
        }),
        "reload_session" => json!({
            "sessionId": "s-1", "inputKind": "vcs", "title": "review",
            "sourceLabel": message["input"]["sourcePath"].as_str().unwrap_or("working tree"),
            "fileCount": 1, "selectedHunkIndex": 0
        }),
        "comment" => json!({
            "commentId": "comment-2", "fileId": "file-1", "filePath": "src/example.rs",
            "hunkIndex": 0, "side": "new", "line": 1
        }),
        "comment_batch" => json!({"applied": [
            {
                "commentId": "comment-1", "fileId": "file-1",
                "filePath": "src/example.rs", "hunkIndex": 0,
                "side": "new", "line": 2
            },
            {
                "commentId": "comment-2", "fileId": "file-1",
                "filePath": "src/example.rs", "hunkIndex": 1,
                "side": "new", "line": 13
            }
        ]}),
        "remove_comment" => {
            json!({"commentId": "comment-1", "removed": true, "remainingCommentCount": 0})
        }
        "clear_comments" => json!({"removedCount": 1, "remainingCommentCount": 0}),
        "highlight" => json!({
            "fileId": "file-1", "filePath": "src/example.rs", "hunkIndex": 0,
            "side": "new", "line": 1, "start": 0, "end": 1, "tone": "info",
            "fileMarkCount": 1
        }),
        "clear_highlights" => json!({"removedCount": 1, "remainingCount": 0}),
        other => panic!("unsupported API fixture command {other}"),
    }
}

fn api_state() -> (Arc<WorkdeckSessionBrokerState>, Arc<ApiSocket>) {
    let state = Arc::new(WorkdeckSessionBrokerState::default());
    let socket = Arc::new(ApiSocket::default());
    socket.bind(&state);
    let file = SessionReviewFile {
        summary: SessionFileSummary {
            id: "file-1".into(),
            path: "src/example.rs".into(),
            previous_path: None,
            additions: 1,
            deletions: 1,
            hunk_count: 1,
        },
        patch: Some("@@ -1 +1 @@\n-old\n+new\n".into()),
        hunks: vec![SessionReviewHunk {
            index: 0,
            header: "@@ -1 +1 @@".into(),
            old_range: Some([1, 1]),
            new_range: Some([1, 1]),
        }],
    };
    let registration = WorkdeckSessionRegistration {
        registration_version: SESSION_BROKER_REGISTRATION_VERSION,
        session_id: "s-1".into(),
        pid: 123,
        cwd: "/repo".into(),
        repo_root: Some("/repo".into()),
        launched_at: "2026-08-25T00:00:00.000Z".into(),
        terminal: None,
        info: WorkdeckSessionInfo {
            input_kind: WorkdeckSessionInputKind::Vcs,
            title: "review".into(),
            source_label: "working tree".into(),
            experimental_features: Some(Vec::new()),
            files: vec![file],
            review_catalog: None,
            review_capability_digest: None,
        },
    };
    let snapshot = WorkdeckSessionSnapshot {
        updated_at: "2026-08-25T00:00:00.000Z".into(),
        state: WorkdeckSessionState {
            selected_file_id: Some("file-1".into()),
            selected_file_path: Some("src/example.rs".into()),
            selected_hunk_index: 0,
            selected_hunk_old_range: Some([1, 1]),
            selected_hunk_new_range: Some([1, 1]),
            show_agent_notes: true,
            note_markup_width: None,
            live_comment_count: 1,
            live_comments: vec![SessionLiveCommentSummary {
                comment_id: "comment-1".into(),
                file_path: "src/example.rs".into(),
                hunk_index: 0,
                side: ReviewSide::Old,
                line: 17,
                summary: "Inspect this line".into(),
                rationale: None,
                author: None,
                created_at: "2026-08-25T00:00:00.000Z".into(),
            }],
            review_note_count: Some(1),
            review_notes: Some(vec![SessionReviewNoteSummary {
                note_id: "note-1".into(),
                parent_id: None,
                source: ReviewNoteSource::Agent,
                file_path: "src/example.rs".into(),
                hunk_index: Some(0),
                old_range: Some([1, 1]),
                new_range: Some([1, 1]),
                body: "Review note".into(),
                title: None,
                author: None,
                created_at: "2026-08-25T00:00:00.000Z".into(),
                updated_at: None,
                editable: false,
            }]),
            review_publication: None,
        },
    };
    assert_eq!(
        state.register_session(
            socket.shared(),
            &serde_json::to_value(registration).unwrap(),
            &serde_json::to_value(snapshot).unwrap(),
            RegisterSessionOptions::default(),
        ),
        RegisterSessionResult::Registered
    );
    (state, socket)
}

fn handle(state: &WorkdeckSessionBrokerState, body: Value) -> SessionBrokerHttpResponse {
    handle_session_api_request(state, &api_request(body), None, None)
}

fn live_server() -> (
    tempfile::TempDir,
    BTreeMap<String, String>,
    RunningWorkdeckSessionBrokerDaemon,
) {
    let root = tempfile::tempdir().unwrap();
    let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = probe.local_addr().unwrap().port();
    drop(probe);
    let env = BTreeMap::from([
        (
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        ),
        (SESSION_BROKER_HOST_ENV.into(), "127.0.0.1".into()),
        (SESSION_BROKER_PORT_ENV.into(), port.to_string()),
    ]);
    let server = serve_workdeck_session_broker_daemon(ServeWorkdeckSessionBrokerDaemonOptions {
        env: env.clone(),
        idle_timeout_ms: Some(0),
        ..ServeWorkdeckSessionBrokerDaemonOptions::default()
    })
    .unwrap();
    (root, env, server)
}

fn reserve_loopback_port() -> u16 {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.local_addr().unwrap().port()
}

fn live_server_with_timing(
    idle_timeout_ms: u64,
    stale_session_ttl_ms: u64,
    stale_session_sweep_interval_ms: u64,
) -> (
    tempfile::TempDir,
    BTreeMap<String, String>,
    RunningWorkdeckSessionBrokerDaemon,
) {
    let root = tempfile::tempdir().unwrap();
    let port = reserve_loopback_port();
    let env = BTreeMap::from([
        (
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        ),
        (SESSION_BROKER_HOST_ENV.into(), "127.0.0.1".into()),
        (SESSION_BROKER_PORT_ENV.into(), port.to_string()),
    ]);
    let server = serve_workdeck_session_broker_daemon(ServeWorkdeckSessionBrokerDaemonOptions {
        env: env.clone(),
        idle_timeout_ms: Some(idle_timeout_ms),
        stale_session_ttl_ms: Some(stale_session_ttl_ms),
        stale_session_sweep_interval_ms: Some(stale_session_sweep_interval_ms),
        ..ServeWorkdeckSessionBrokerDaemonOptions::default()
    })
    .unwrap();
    (root, env, server)
}

fn live_server_with_state(
    state: Arc<WorkdeckSessionBrokerState>,
) -> (
    tempfile::TempDir,
    BTreeMap<String, String>,
    RunningWorkdeckSessionBrokerDaemon,
) {
    let root = tempfile::tempdir().unwrap();
    let port = reserve_loopback_port();
    let env = BTreeMap::from([
        (
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        ),
        (SESSION_BROKER_HOST_ENV.into(), "127.0.0.1".into()),
        (SESSION_BROKER_PORT_ENV.into(), port.to_string()),
    ]);
    let server = serve_workdeck_session_broker_daemon(ServeWorkdeckSessionBrokerDaemonOptions {
        env: env.clone(),
        idle_timeout_ms: Some(0),
        state: Some(state),
        ..ServeWorkdeckSessionBrokerDaemonOptions::default()
    })
    .unwrap();
    (root, env, server)
}

fn signed_session_request(
    caller: &SessionBrokerCallerClient,
    body: Value,
) -> crate::SessionBrokerCallerResponse {
    caller
        .request(
            WORKDECK_SESSION_API_PATH,
            SessionBrokerSignedRequestInit {
                method: Some("POST".into()),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: Some(serde_json::to_string(&body).unwrap()),
                target_specific: body["action"] != "list",
                ..SessionBrokerSignedRequestInit::default()
            },
        )
        .unwrap()
}

fn raw_websocket_handshake(port: u16, extra_headers: &[(&str, &str)]) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let mut request = format!(
        "GET {} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n",
        crate::SESSION_BROKER_SOCKET_PATH
    );
    for (name, value) in extra_headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    let mut byte = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        let read = stream.read(&mut byte).unwrap();
        if read == 0 {
            break;
        }
        response.push(byte[0]);
    }
    String::from_utf8(response).unwrap()
}

type NativeProducerConnection = SessionBrokerConnection<
    WorkdeckSessionInfo,
    WorkdeckSessionState,
    WorkdeckSessionCommandInput,
    WorkdeckSessionCommandResult,
>;

fn producer_registration(session_id: &str) -> WorkdeckSessionRegistration {
    WorkdeckSessionRegistration {
        registration_version: SESSION_BROKER_REGISTRATION_VERSION,
        session_id: session_id.into(),
        pid: u64::from(std::process::id()),
        cwd: "/repo".into(),
        repo_root: Some("/repo".into()),
        launched_at: "2026-03-24T00:00:00.000Z".into(),
        terminal: None,
        info: WorkdeckSessionInfo {
            input_kind: WorkdeckSessionInputKind::Vcs,
            title: "repo diff".into(),
            source_label: "/repo".into(),
            experimental_features: Some(Vec::new()),
            files: Vec::new(),
            review_catalog: None,
            review_capability_digest: None,
        },
    }
}

fn producer_snapshot() -> WorkdeckSessionSnapshot {
    WorkdeckSessionSnapshot {
        updated_at: "2026-03-24T00:00:00.000Z".into(),
        state: WorkdeckSessionState {
            selected_file_id: None,
            selected_file_path: None,
            selected_hunk_index: 0,
            selected_hunk_old_range: None,
            selected_hunk_new_range: None,
            show_agent_notes: false,
            note_markup_width: None,
            live_comment_count: 0,
            live_comments: Vec::new(),
            review_note_count: Some(0),
            review_notes: Some(Vec::new()),
            review_publication: None,
        },
    }
}

fn start_native_producer(
    env: &BTreeMap<String, String>,
    port: u16,
    session_id: &str,
) -> NativeProducerConnection {
    let credentials = load_or_create_workdeck_session_broker_credentials(env, None).unwrap();
    let mut options = SessionBrokerConnectionOptions::new(
        format!("ws://127.0.0.1:{port}{}", crate::SESSION_BROKER_SOCKET_PATH),
        Arc::new(|url: &str| NativeSessionBrokerClientSocket::connect(url)),
        producer_registration(session_id),
        producer_snapshot(),
        Arc::new(create_workdeck_session_protocol_parsers().unwrap()),
    );
    options.producer_authentication = Some(SessionBrokerProducerAuthentication::native(
        WORKDECK_SESSION_BROKER_APP_ID,
        WORKDECK_SESSION_BROKER_APP_REVISION,
        SessionBrokerClientCredential {
            grant: credentials.producer.grant,
            private_key: credentials.producer.private_key,
        },
        SessionBrokerDaemonVerifier {
            key_id: credentials.daemon_identity.key_id,
            public_key: credentials.daemon_public_key,
        },
    ));
    let connection = SessionBrokerConnection::new(options).unwrap();
    connection.start().unwrap();
    connection
}

struct SocketObservation {
    opened: AtomicBool,
    closes: Mutex<Vec<SessionBrokerSocketCloseEvent>>,
    changed: Condvar,
}

fn open_unauthenticated_socket(
    port: u16,
) -> (Arc<dyn SessionBrokerSocketLike>, Arc<SocketObservation>) {
    let socket = NativeSessionBrokerClientSocket::connect(format!(
        "ws://127.0.0.1:{port}{}",
        crate::SESSION_BROKER_SOCKET_PATH
    ))
    .unwrap();
    let observed = Arc::new(SocketObservation {
        opened: AtomicBool::new(false),
        closes: Mutex::new(Vec::new()),
        changed: Condvar::new(),
    });
    let open_observed = Arc::clone(&observed);
    socket.set_on_open(Some(Arc::new(move || {
        open_observed.opened.store(true, Ordering::Release);
        open_observed.changed.notify_all();
    })));
    socket.set_on_message(Some(Arc::new(|_| {})));
    let close_observed = Arc::clone(&observed);
    socket.set_on_close(Some(Arc::new(move |event| {
        close_observed
            .closes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .push(event);
        close_observed.changed.notify_all();
    })));
    socket.set_on_error(Some(Arc::new(|| {})));
    wait_until("websocket open", Duration::from_secs(2), || {
        observed.opened.load(Ordering::Acquire)
    });
    (socket, observed)
}

#[test]
fn maps_address_in_use_failures_to_a_port_conflict_hint() {
    let error = format_daemon_serve_error("listen EADDRINUSE", "127.0.0.1", PORT);
    assert!(error.contains("already in use"));
    assert!(error.contains(&format!("127.0.0.1:{PORT}")));
}

#[test]
fn falls_back_to_a_generic_start_failure_for_other_errors() {
    let error = format_daemon_serve_error("boom", "127.0.0.1", PORT);
    assert!(error.contains("Failed to start the session broker daemon"));
    assert!(error.contains("boom"));
}

#[test]
fn stringifies_non_error_start_failures() {
    assert!(format_daemon_serve_error("plain string", "127.0.0.1", PORT).contains("plain string"));
}

#[test]
fn parse_host_returns_none_for_empty_input() {
    assert_eq!(parse_host_and_port("   "), None);
}

#[test]
fn parse_host_accepts_a_bare_host_with_no_port() {
    assert_eq!(
        parse_host_and_port("127.0.0.1"),
        Some(ParsedHostPort {
            host: "127.0.0.1".into(),
            port: None,
        })
    );
}

#[test]
fn parse_host_accepts_host_and_port() {
    assert_eq!(
        parse_host_and_port("127.0.0.1:7000"),
        Some(ParsedHostPort {
            host: "127.0.0.1".into(),
            port: Some(PORT),
        })
    );
}

#[test]
fn parse_host_rejects_a_non_numeric_port() {
    assert_eq!(parse_host_and_port("127.0.0.1:abc"), None);
}

#[test]
fn parse_host_accepts_a_bracketed_ipv6_literal_with_port() {
    assert_eq!(
        parse_host_and_port("[::1]:7000"),
        Some(ParsedHostPort {
            host: "::1".into(),
            port: Some(PORT),
        })
    );
}

#[test]
fn parse_host_accepts_a_bracketed_ipv6_literal_without_port() {
    assert_eq!(
        parse_host_and_port("[::1]"),
        Some(ParsedHostPort {
            host: "::1".into(),
            port: None,
        })
    );
}

#[test]
fn parse_host_rejects_an_unterminated_bracket() {
    assert_eq!(parse_host_and_port("[::1"), None);
}

#[test]
fn parse_host_rejects_bracketed_trailing_junk_that_is_not_a_port() {
    assert_eq!(parse_host_and_port("[::1]x"), None);
}

#[test]
fn parse_host_rejects_a_bracketed_host_with_a_zero_port() {
    assert_eq!(parse_host_and_port("[::1]:0"), None);
}

#[test]
fn parse_host_rejects_ambiguous_unbracketed_ipv6_authorities() {
    assert_eq!(parse_host_and_port("::1"), None);
}

#[test]
fn parse_host_rejects_multiple_comma_separated_authorities() {
    assert_eq!(parse_host_and_port("a,b"), None);
}

#[test]
fn host_allowlist_accepts_a_loopback_host_on_the_expected_port() {
    let loopback = ParsedHostPort {
        host: "127.0.0.1".into(),
        port: Some(PORT),
    };
    assert!(is_allowed_host_port(&loopback, PORT, false));
}

#[test]
fn host_allowlist_defaults_a_missing_port_to_80_and_rejects_it() {
    assert!(!is_allowed_host_port(
        &ParsedHostPort {
            port: None,
            host: "127.0.0.1".into(),
        },
        PORT,
        false
    ));
}

#[test]
fn host_allowlist_rejects_a_remote_host_unless_remote_is_allowed() {
    let remote = ParsedHostPort {
        host: "10.0.0.5".into(),
        port: Some(PORT),
    };
    assert!(!is_allowed_host_port(&remote, PORT, false));
    assert!(is_allowed_host_port(&remote, PORT, true));
}

#[test]
fn host_header_validation_rejects_a_request_with_no_host() {
    assert_eq!(
        validate_host_header(&request(&[]), PORT, false)
            .unwrap()
            .status,
        400
    );
}

#[test]
fn host_header_validation_rejects_a_disallowed_endpoint() {
    assert_eq!(
        validate_host_header(&request(&[("host", "evil.com:7000")]), PORT, false)
            .unwrap()
            .status,
        403
    );
}

#[test]
fn host_header_validation_accepts_loopback_on_the_expected_port() {
    assert!(validate_host_header(&request(&[("host", "127.0.0.1:7000")]), PORT, false).is_none());
}

#[test]
fn origin_validation_allows_a_request_with_no_origin() {
    assert!(validate_origin_header(&request(&[]), PORT, false).is_none());
}

fn assert_origin_refused(origin: &str) {
    assert_eq!(
        validate_origin_header(&request(&[("origin", origin)]), PORT, false)
            .unwrap()
            .status,
        403,
        "accepted {origin}"
    );
}

#[test]
fn origin_validation_rejects_a_malformed_value() {
    assert_origin_refused("not a url");
}

#[test]
fn origin_validation_rejects_a_non_http_scheme() {
    assert_origin_refused("file://localhost");
}

#[test]
fn origin_validation_rejects_a_cross_origin_browser_request() {
    assert_origin_refused("http://evil.com");
}

#[test]
fn origin_validation_accepts_loopback_on_the_expected_port() {
    assert!(
        validate_origin_header(
            &request(&[("origin", "http://127.0.0.1:7000")]),
            PORT,
            false
        )
        .is_none()
    );
}

#[test]
fn origin_validation_rejects_opaque_credentialed_and_non_origin_urls() {
    for origin in [
        "null",
        "http://127.0.0.1:7000/path",
        "http://user@127.0.0.1:7000",
        "http://127.0.0.1:7000?query",
    ] {
        assert_eq!(
            validate_origin_header(&request(&[("origin", origin)]), PORT, false)
                .unwrap()
                .status,
            403,
            "accepted {origin}"
        );
    }
}

#[test]
fn capabilities_advertise_every_workdeck_session_action_once() {
    let capabilities = session_daemon_capabilities();
    assert_eq!(capabilities.version, WORKDECK_SESSION_API_VERSION);
    assert_eq!(capabilities.daemon_version, WORKDECK_SESSION_DAEMON_VERSION);
    assert_eq!(capabilities.actions, SUPPORTED_SESSION_ACTIONS);
}

#[test]
fn session_api_rejects_non_post_methods() {
    let (state, _) = api_state();
    let request = SessionBrokerHttpRequest::get(format!("http://127.0.0.1:{PORT}/session-api"));
    assert_eq!(
        handle_session_api_request(&state, &request, None, None).status,
        405
    );
}

#[test]
fn session_api_requires_a_json_content_type() {
    let (state, _) = api_state();
    let request = SessionBrokerHttpRequest {
        method: "POST".into(),
        url: format!("http://127.0.0.1:{PORT}/session-api"),
        headers: BTreeMap::new(),
        body: b"{}".to_vec(),
    };
    assert_eq!(
        handle_session_api_request(&state, &request, None, None).status,
        415
    );
}

#[test]
fn session_api_returns_400_for_unparseable_json() {
    let (state, _) = api_state();
    let request = SessionBrokerHttpRequest {
        method: "POST".into(),
        url: format!("http://127.0.0.1:{PORT}/session-api"),
        headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
        body: b"{ not json".to_vec(),
    };
    assert_eq!(
        handle_session_api_request(&state, &request, None, None).status,
        400
    );
}

#[test]
fn session_api_rejects_malformed_nested_bodies_before_dispatch() {
    let (state, socket) = api_state();
    for malformed in [
        json!({"action":"get","selector":{"sessionId":"s-1","extra":true}}),
        json!({
            "action":"reload","selector":{"sessionId":"s-1"},
            "nextInput":{"kind":"vcs","staged":false,"options":{"tabWidth":0}}
        }),
        json!({
            "action":"comment-apply","selector":{"sessionId":"s-1"},
            "comments":[{"filePath":"a.ts","summary":"note","hunkNumber":0}],
            "revealMode":"first"
        }),
    ] {
        assert_eq!(handle(&state, malformed).status, 400);
    }
    assert!(socket.messages().is_empty());
}

#[test]
fn session_api_routes_list_get_context_and_review() {
    let (state, _) = api_state();
    for body in [
        json!({"action":"list"}),
        json!({"action":"get","selector":{"sessionId":"s-1"}}),
        json!({"action":"context","selector":{"sessionId":"s-1"}}),
        json!({"action":"review","selector":{"sessionId":"s-1"}}),
    ] {
        assert_eq!(handle(&state, body).status, 200);
    }
}

#[test]
fn session_api_rejects_navigation_without_a_hunk_or_line_target() {
    let (state, _) = api_state();
    let response = handle(
        &state,
        json!({"action":"navigate","selector":{"sessionId":"s-1"}}),
    );
    assert_eq!(response.status, 400);
    assert!(
        response.json_body().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("navigate")
    );
}

#[test]
fn session_api_converts_one_based_hunk_numbers_before_navigation_dispatch() {
    let (state, socket) = api_state();
    let response = handle(
        &state,
        json!({"action":"navigate","selector":{"sessionId":"s-1"},"hunkNumber":2}),
    );
    assert_eq!(response.status, 200);
    assert_eq!(socket.messages()[0]["input"]["hunkIndex"], 1);
}

#[test]
fn session_api_prefers_exact_line_coordinates_over_a_hunk_number() {
    let (state, socket) = api_state();
    let response = handle(
        &state,
        json!({
            "action":"navigate","selector":{"sessionId":"s-1"},
            "filePath":"src/example.rs","hunkNumber":3,"side":"new","line":17
        }),
    );
    assert_eq!(response.status, 200);
    let input = &socket.messages()[0]["input"];
    assert_eq!(input["filePath"], "src/example.rs");
    assert!(input.get("hunkIndex").is_none());
    assert_eq!(input["side"], "new");
    assert_eq!(input["line"], 17);
}

#[test]
fn session_api_resolves_comment_ids_to_exact_navigation_coordinates() {
    let (state, socket) = api_state();
    let response = handle(
        &state,
        json!({
            "action":"navigate","selector":{"sessionId":"s-1"},"commentId":"comment-1"
        }),
    );
    assert_eq!(response.status, 200);
    let input = &socket.messages()[0]["input"];
    assert_eq!(input["filePath"], "src/example.rs");
    assert_eq!(input["side"], "old");
    assert_eq!(input["line"], 17);
}

#[test]
fn session_api_rejects_navigation_to_an_unknown_comment_id() {
    let (state, _) = api_state();
    let response = handle(
        &state,
        json!({
            "action":"navigate","selector":{"sessionId":"s-1"},"commentId":"missing-comment"
        }),
    );
    assert_eq!(response.status, 400);
    assert!(
        response.json_body().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("missing-comment")
    );
}

#[test]
fn session_api_rejects_comment_ids_combined_with_other_navigation_targets() {
    let (state, _) = api_state();
    let response = handle(
        &state,
        json!({
            "action":"navigate","selector":{"sessionId":"s-1"},
            "commentId":"comment-1","hunkNumber":2
        }),
    );
    assert_eq!(response.status, 400);
    assert!(
        response.json_body().unwrap()["error"]
            .as_str()
            .unwrap()
            .contains("cannot be combined")
    );
}

#[test]
fn session_api_dispatches_reload_comment_remove_and_clear_commands() {
    let (state, socket) = api_state();
    for body in [
        json!({
            "action":"reload","selector":{"sessionId":"s-1"},
            "nextInput":{"kind":"show","ref":"HEAD~1","options":{}}
        }),
        json!({
            "action":"comment-add","selector":{"sessionId":"s-1"},
            "filePath":"src/example.rs","side":"new","line":1,"summary":"note","reveal":false
        }),
        json!({"action":"comment-rm","selector":{"sessionId":"s-1"},"commentId":"comment-1"}),
        json!({"action":"comment-clear","selector":{"sessionId":"s-1"}}),
    ] {
        assert_eq!(handle(&state, body).status, 200);
    }
    assert_eq!(socket.messages().len(), 4);
}

#[test]
fn session_api_serves_live_comments_from_broker_state() {
    let (state, socket) = api_state();
    let response = handle(
        &state,
        json!({"action":"comment-list","selector":{"sessionId":"s-1"}}),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        response.json_body().unwrap()["comments"][0]["commentId"],
        "comment-1"
    );
    assert!(socket.messages().is_empty());
}

#[test]
fn session_api_serves_review_notes_from_the_session_snapshot() {
    let (state, _) = api_state();
    let response = handle(
        &state,
        json!({"action":"comment-list","selector":{"sessionId":"s-1"},"type":"all"}),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        response.json_body().unwrap()["comments"][0]["noteId"],
        "note-1"
    );
}

#[test]
fn session_api_returns_400_when_a_dispatched_command_rejects() {
    let (state, socket) = api_state();
    socket.fail("remove_comment", "session timed out");
    let response = handle(
        &state,
        json!({"action":"comment-rm","selector":{"sessionId":"s-1"},"commentId":"comment-1"}),
    );
    assert_eq!(response.status, 400);
    assert_eq!(response.json_body().unwrap()["error"], "session timed out");
}

#[test]
fn native_server_authenticates_capabilities_and_session_list_over_one_state() {
    let (_root, env, server) = live_server();
    let client =
        crate::HttpWorkdeckSessionCliClient::from_environment(env, Duration::from_secs(2)).unwrap();
    assert_eq!(
        client.get_capabilities().unwrap(),
        Some(session_daemon_capabilities())
    );
    assert!(client.list_sessions().unwrap().is_empty());
    server.stop();
    assert!(server.wait_stopped(Duration::from_secs(2)));
}

#[test]
fn native_server_refuses_non_loopback_binding_unless_explicitly_allowed() {
    let root = tempfile::tempdir().unwrap();
    let port = reserve_loopback_port();
    let mut env = BTreeMap::from([
        (
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        ),
        (SESSION_BROKER_HOST_ENV.into(), "0.0.0.0".into()),
        (SESSION_BROKER_PORT_ENV.into(), port.to_string()),
    ]);
    let error = serve_workdeck_session_broker_daemon(ServeWorkdeckSessionBrokerDaemonOptions {
        env: env.clone(),
        ..ServeWorkdeckSessionBrokerDaemonOptions::default()
    })
    .err()
    .expect("remote binding should be refused");
    assert!(error.contains("local-only by default"));

    env.insert(UNSAFE_ALLOW_REMOTE_SESSION_BROKER_ENV.into(), "1".into());
    let server = serve_workdeck_session_broker_daemon(ServeWorkdeckSessionBrokerDaemonOptions {
        env,
        idle_timeout_ms: Some(0),
        ..ServeWorkdeckSessionBrokerDaemonOptions::default()
    })
    .unwrap();
    server.stop();
    assert!(server.wait_stopped(Duration::from_secs(2)));
}

#[test]
fn native_server_reports_a_clear_port_conflict_error() {
    let root = tempfile::tempdir().unwrap();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let env = BTreeMap::from([
        (
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        ),
        (SESSION_BROKER_HOST_ENV.into(), "127.0.0.1".into()),
        (SESSION_BROKER_PORT_ENV.into(), port.to_string()),
    ]);
    let error = serve_workdeck_session_broker_daemon(ServeWorkdeckSessionBrokerDaemonOptions {
        env,
        ..ServeWorkdeckSessionBrokerDaemonOptions::default()
    })
    .err()
    .expect("occupied port should fail");
    assert!(error.contains("port is already in use"));
    assert!(error.contains(&format!("127.0.0.1:{port}")));
    drop(listener);
}

#[test]
fn native_server_exposes_only_workdeck_session_endpoints_and_tombstones_mcp() {
    let (_root, env, server) = live_server();
    let port = server.address().port();
    let health = send_native_http(port, "GET", "/health", &BTreeMap::new(), b"").unwrap();
    assert_eq!(health.status, 200);
    assert_eq!(health.json(), json!({"ok": true}));

    let generic_capabilities =
        send_native_http(port, "GET", "/broker/capabilities", &BTreeMap::new(), b"").unwrap();
    assert_eq!(generic_capabilities.status, 404);
    let generic_broker = send_native_http(
        port,
        "POST",
        "/broker",
        &BTreeMap::from([("content-type".into(), "application/json".into())]),
        br#"{"action":"list"}"#,
    )
    .unwrap();
    assert_eq!(generic_broker.status, 404);

    let capabilities = signed_caller(&env, port)
        .request(
            WORKDECK_SESSION_CAPABILITIES_PATH,
            SessionBrokerSignedRequestInit {
                method: Some("GET".into()),
                ..SessionBrokerSignedRequestInit::default()
            },
        )
        .unwrap();
    assert_eq!(capabilities.status, 200);
    assert_eq!(capabilities.body["version"], WORKDECK_SESSION_API_VERSION);
    assert_eq!(
        capabilities.body["daemonVersion"],
        WORKDECK_SESSION_DAEMON_VERSION
    );
    assert_eq!(
        serde_json::from_value::<Vec<SessionDaemonAction>>(capabilities.body["actions"].clone())
            .unwrap(),
        SUPPORTED_SESSION_ACTIONS
    );

    let legacy = send_native_http(
        port,
        "POST",
        crate::LEGACY_MCP_PATH,
        &BTreeMap::from([("content-type".into(), "application/json".into())]),
        b"{}",
    )
    .unwrap();
    assert_eq!(legacy.status, 410);
    assert_eq!(
        legacy.json()["error"],
        "This app no longer exposes agent-facing MCP tools. Use the session CLI instead."
    );
    server.stop();
}

#[test]
fn native_server_keeps_caller_and_browser_review_authority_independent() {
    let (_root, env, server) = live_server();
    let port = server.address().port();
    let signed_review = signed_caller(&env, port).request(
        "/review-api/missing/publication",
        SessionBrokerSignedRequestInit {
            method: Some("GET".into()),
            ..SessionBrokerSignedRequestInit::default()
        },
    );
    assert!(signed_review.is_err());

    let generic_only = send_native_http(
        port,
        "GET",
        "/review-api/missing/publication",
        &BTreeMap::from([(
            "x-session-broker-caller-session".into(),
            "generic-only".into(),
        )]),
        b"",
    )
    .unwrap();
    assert_eq!(generic_only.status, 401);

    let review_only = send_native_http(
        port,
        "POST",
        WORKDECK_SESSION_API_PATH,
        &BTreeMap::from([
            ("content-type".into(), "application/json".into()),
            (
                WORKDECK_REVIEW_CAPABILITY_HEADER.into(),
                "review-only-capability".into(),
            ),
        ]),
        br#"{"action":"list"}"#,
    )
    .unwrap();
    assert_eq!(review_only.status, 401);
    server.stop();
}

#[test]
fn native_server_rejects_non_loopback_and_wrong_port_host_headers() {
    let (_root, _env, server) = live_server();
    let port = server.address().port();
    for host in [format!("attacker.example:{port}"), "127.0.0.1".into()] {
        let response = send_native_http(
            port,
            "GET",
            "/health",
            &BTreeMap::from([("host".into(), host)]),
            b"",
        )
        .unwrap();
        assert_eq!(response.status, 403);
        assert_eq!(
            response.json(),
            json!({"error": "Host header is not allowed for the local session broker."})
        );
    }
    server.stop();
}

#[test]
fn native_server_rejects_non_local_origins_for_http_and_websocket() {
    let (_root, _env, server) = live_server();
    let port = server.address().port();
    let response = send_native_http(
        port,
        "GET",
        WORKDECK_SESSION_CAPABILITIES_PATH,
        &BTreeMap::from([("origin".into(), "https://attacker.example".into())]),
        b"",
    )
    .unwrap();
    assert_eq!(response.status, 403);
    assert_eq!(
        response.json(),
        json!({"error": "Origin is not allowed for the local session broker."})
    );
    let handshake = raw_websocket_handshake(port, &[("Origin", "https://attacker.example")]);
    assert!(handshake.starts_with("HTTP/1.1 403"), "{handshake}");
    server.stop();
}

#[test]
fn native_server_requires_get_with_empty_body_for_signed_capabilities() {
    let (_root, env, server) = live_server();
    let response = signed_caller(&env, server.address().port())
        .request(
            WORKDECK_SESSION_CAPABILITIES_PATH,
            SessionBrokerSignedRequestInit {
                method: Some("POST".into()),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: Some("{}".into()),
                ..SessionBrokerSignedRequestInit::default()
            },
        )
        .unwrap();
    assert_eq!(response.status, 405);
    assert_eq!(
        response.body,
        json!({"error": "Capabilities require GET with an empty body."})
    );
    server.stop();
}

#[test]
fn native_server_requires_json_content_type_for_signed_session_posts() {
    let (_root, env, server) = live_server();
    let response = signed_caller(&env, server.address().port())
        .request(
            WORKDECK_SESSION_API_PATH,
            SessionBrokerSignedRequestInit {
                method: Some("POST".into()),
                headers: BTreeMap::from([("content-type".into(), "text/plain".into())]),
                body: Some(r#"{"action":"list"}"#.into()),
                ..SessionBrokerSignedRequestInit::default()
            },
        )
        .unwrap();
    assert_eq!(response.status, 415);
    assert_eq!(
        response.body,
        json!({"error": "Expected Content-Type application/json."})
    );
    server.stop();
}

#[test]
fn native_server_rejects_session_api_bodies_over_the_limit() {
    let (_root, _env, server) = live_server();
    let port = server.address().port();
    let body = serde_json::to_vec(&json!({
        "action": "list",
        "filler": "x".repeat(5 * 1024 * 1024)
    }))
    .unwrap();
    let response = send_native_http(
        port,
        "POST",
        WORKDECK_SESSION_API_PATH,
        &BTreeMap::from([
            ("content-type".into(), "application/json".into()),
            (
                "x-session-broker-caller-session".into(),
                "oversized-test-session".into(),
            ),
        ]),
        &body,
    )
    .unwrap();
    assert_eq!(response.status, 413);
    assert_eq!(response.json()["error"], "capacity-exceeded");
    assert_eq!(response.json()["resource"], "maxHttpBodyBytes");
    server.stop();
}

#[test]
fn native_server_closes_snapshot_assertions_from_unauthenticated_peers() {
    let (_root, _env, server) = live_server_with_timing(250, 500, 25);
    let (socket, observed) = open_unauthenticated_socket(server.address().port());
    socket
        .send(
            &serde_json::to_string(&json!({
                "type": "snapshot",
                "sessionId": "missing-session",
                "snapshot": producer_snapshot(),
            }))
            .unwrap(),
        )
        .unwrap();
    wait_until(
        "unauthenticated websocket close",
        Duration::from_secs(2),
        || {
            !observed
                .closes
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_empty()
        },
    );
    let closes = observed
        .closes
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    assert_eq!(closes[0].code, 1008);
    assert_eq!(
        closes[0].reason,
        "Session broker authentication required; upgrade Workdeck."
    );
    drop(closes);
    socket.close(None, None);
    server.stop();
}

#[test]
fn native_server_ignores_incompatible_registration_without_poisoning_the_session_list() {
    let (_root, env, server) = live_server_with_timing(250, 500, 25);
    let port = server.address().port();
    let (bad_socket, observed) = open_unauthenticated_socket(port);
    let mut registration = serde_json::to_value(producer_registration("stale-session")).unwrap();
    registration["registrationVersion"] = json!(0);
    bad_socket
        .send(
            &serde_json::to_string(&json!({
                "type": "register",
                "registration": registration,
                "snapshot": producer_snapshot(),
            }))
            .unwrap(),
        )
        .unwrap();
    wait_until("incompatible socket close", Duration::from_secs(2), || {
        !observed
            .closes
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .is_empty()
    });
    let caller = signed_caller(&env, port);
    let empty = caller
        .request(
            WORKDECK_SESSION_API_PATH,
            SessionBrokerSignedRequestInit {
                method: Some("POST".into()),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: Some(r#"{"action":"list"}"#.into()),
                ..SessionBrokerSignedRequestInit::default()
            },
        )
        .unwrap();
    assert_eq!(empty.status, 200);
    assert_eq!(empty.body["sessions"], json!([]));

    let good = start_native_producer(&env, port, "session-good");
    wait_until("good session registration", Duration::from_secs(2), || {
        server.state().session_count() == 1
    });
    let listed = caller
        .request(
            WORKDECK_SESSION_API_PATH,
            SessionBrokerSignedRequestInit {
                method: Some("POST".into()),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: Some(r#"{"action":"list"}"#.into()),
                ..SessionBrokerSignedRequestInit::default()
            },
        )
        .unwrap();
    assert_eq!(listed.status, 200);
    assert_eq!(listed.body["sessions"][0]["sessionId"], "session-good");
    good.stop();
    bad_socket.close(None, None);
    server.stop();
}

#[test]
fn native_server_stays_alive_while_one_live_session_remains_registered() {
    let (_root, env, server) = live_server_with_timing(1_500, 5_000, 100);
    let port = server.address().port();
    let producer = start_native_producer(&env, port, "session-1");
    wait_until("session registration", Duration::from_secs(5), || {
        server.state().session_count() == 1
    });
    std::thread::sleep(Duration::from_millis(1_800));
    let health = send_native_http(port, "GET", "/health", &BTreeMap::new(), b"").unwrap();
    assert_eq!(health.status, 200);
    assert_eq!(health.json(), json!({"ok": true}));
    producer.stop();
    server.stop();
}

#[test]
fn native_server_shuts_down_after_the_last_live_session_disconnects() {
    let (_root, env, server) = live_server_with_timing(1_500, 5_000, 100);
    let port = server.address().port();
    let producer = start_native_producer(&env, port, "session-1");
    wait_until("session registration", Duration::from_secs(5), || {
        server.state().session_count() == 1
    });
    producer.stop();
    wait_until("session disconnect", Duration::from_secs(5), || {
        server.state().session_count() == 0
    });
    assert!(server.wait_stopped(Duration::from_secs(4)));
}

#[test]
fn native_server_shuts_down_after_stale_pruning_leaves_no_live_sessions() {
    let (_root, env, server) = live_server_with_timing(1_500, 300, 50);
    let port = server.address().port();
    let producer = start_native_producer(&env, port, "session-1");
    wait_until("session registration", Duration::from_secs(5), || {
        server.state().session_count() == 1
    });
    assert!(server.wait_stopped(Duration::from_secs(4)));
    producer.stop();
}

#[test]
fn native_server_forwards_review_options_through_the_session_api() {
    let (state, _socket) = api_state();
    let (_root, env, server) = live_server_with_state(state);
    let response = signed_session_request(
        &signed_caller(&env, server.address().port()),
        json!({
            "action": "review",
            "selector": {"sessionId": "s-1"},
            "includePatch": true,
            "includeNotes": true,
        }),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        response.body["review"]["files"][0]["path"],
        "src/example.rs"
    );
    assert_eq!(
        response.body["review"]["files"][0]["patch"],
        "@@ -1 +1 @@\n-old\n+new\n"
    );
    assert_eq!(
        response.body["review"]["reviewNotes"][0]["noteId"],
        "note-1"
    );
    server.stop();
}

#[test]
fn native_server_forwards_reload_source_path_and_structured_endpoints() {
    let (state, socket) = api_state();
    let (_root, env, server) = live_server_with_state(state);
    let response = signed_session_request(
        &signed_caller(&env, server.address().port()),
        json!({
            "action": "reload",
            "selector": {"sessionId": "s-1"},
            "sourcePath": "/tmp/source-repo",
            "nextInput": {
                "kind": "vcs",
                "rangeEndpoints": {"from": "main", "to": "feature"},
                "staged": false,
                "options": {},
            },
        }),
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.body["result"]["sessionId"], "s-1");
    assert_eq!(response.body["result"]["sourceLabel"], "/tmp/source-repo");
    let messages = socket.messages();
    let reload = messages
        .iter()
        .find(|message| message["command"] == "reload_session")
        .unwrap();
    assert_eq!(reload["input"]["sourcePath"], "/tmp/source-repo");
    assert_eq!(reload["input"]["nextInput"]["kind"], "vcs");
    assert_eq!(
        reload["input"]["nextInput"]["rangeEndpoints"],
        json!({"from": "main", "to": "feature"})
    );
    assert_eq!(reload["input"]["nextInput"]["staged"], false);
    assert_eq!(reload["input"]["nextInput"]["options"], json!({}));
    server.stop();
}

#[test]
fn native_server_serves_review_notes_through_the_session_api() {
    let (state, socket) = api_state();
    let mut snapshot = producer_snapshot();
    snapshot.state.review_note_count = Some(2);
    snapshot.state.review_notes = Some(vec![
        SessionReviewNoteSummary {
            note_id: "user:1".into(),
            parent_id: None,
            source: ReviewNoteSource::User,
            file_path: "src/example.rs".into(),
            hunk_index: Some(0),
            old_range: None,
            new_range: None,
            body: "Human note".into(),
            title: None,
            author: None,
            created_at: "2026-05-10T00:00:00.000Z".into(),
            updated_at: None,
            editable: true,
        },
        SessionReviewNoteSummary {
            note_id: "agent:1".into(),
            parent_id: None,
            source: ReviewNoteSource::Agent,
            file_path: "src/other.rs".into(),
            hunk_index: None,
            old_range: None,
            new_range: None,
            body: "Agent note".into(),
            title: None,
            author: None,
            created_at: "2026-05-10T00:00:00.000Z".into(),
            updated_at: None,
            editable: false,
        },
    ]);
    assert_eq!(
        state.update_snapshot(
            &socket.shared(),
            "s-1",
            &serde_json::to_value(snapshot).unwrap(),
        ),
        crate::UpdateSnapshotResult::Updated
    );
    let (_root, env, server) = live_server_with_state(state);
    let response = signed_session_request(
        &signed_caller(&env, server.address().port()),
        json!({
            "action": "comment-list",
            "selector": {"sessionId": "s-1"},
            "type": "user",
        }),
    );
    assert_eq!(response.status, 200);
    assert_eq!(response.body["comments"].as_array().unwrap().len(), 1);
    assert_eq!(response.body["comments"][0]["noteId"], "user:1");
    assert_eq!(response.body["comments"][0]["body"], "Human note");
    server.stop();
}

#[test]
fn native_server_forwards_comment_batches_through_the_session_api() {
    let (state, socket) = api_state();
    let (_root, env, server) = live_server_with_state(state);
    let response = signed_session_request(
        &signed_caller(&env, server.address().port()),
        json!({
            "action": "comment-apply",
            "selector": {"sessionId": "s-1"},
            "revealMode": "none",
            "comments": [
                {
                    "filePath": "src/example.rs",
                    "hunkNumber": 1,
                    "summary": "First",
                    "author": "Pi",
                },
                {
                    "filePath": "src/example.rs",
                    "hunkNumber": 2,
                    "summary": "Second",
                    "rationale": "Applied together.",
                    "author": "Pi",
                },
            ],
        }),
    );
    assert_eq!(response.status, 200);
    assert_eq!(
        response.body["result"]["applied"][0]["commentId"],
        "comment-1"
    );
    assert_eq!(response.body["result"]["applied"][0]["hunkIndex"], 0);
    assert_eq!(
        response.body["result"]["applied"][1]["commentId"],
        "comment-2"
    );
    assert_eq!(response.body["result"]["applied"][1]["hunkIndex"], 1);
    let messages = socket.messages();
    let batch = messages
        .iter()
        .find(|message| message["command"] == "comment_batch")
        .unwrap();
    assert_eq!(batch["input"]["revealMode"], "none");
    assert_eq!(batch["input"]["comments"][0]["hunkIndex"], 0);
    assert_eq!(batch["input"]["comments"][0]["summary"], "First");
    assert_eq!(batch["input"]["comments"][0]["author"], "Pi");
    assert_eq!(batch["input"]["comments"][1]["hunkIndex"], 1);
    assert_eq!(
        batch["input"]["comments"][1]["rationale"],
        "Applied together."
    );
    server.stop();
}
