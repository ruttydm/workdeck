//! Typed, authenticated client for Workdeck's broker-backed live-session API.

use std::collections::BTreeMap;
use std::env;
use std::io::{Cursor, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::Value;
use thiserror::Error;
use url::Url;
use workdeck_core::ReviewSide;

use crate::{
    AppliedCommentBatchResult, AppliedCommentResult, AppliedHighlightResult, CallerGrant,
    ClearedCommentsResult, ClearedHighlightsResult, DaemonCliInput, DaemonCommentApplyItem,
    DaemonCommentDirection, DaemonCommentListType, DaemonRevealMode, ListedSession,
    NavigatedSelectionResult, ReloadedSessionResult, RemovedCommentResult, SelectedSessionContext,
    SessionBrokerCallerCancellation, SessionBrokerCallerClient, SessionBrokerCallerClientOptions,
    SessionBrokerCallerResponse, SessionBrokerClientCredential, SessionBrokerClientHttpRequest,
    SessionBrokerClientHttpResponse, SessionBrokerClientHttpTransport, SessionBrokerDaemonVerifier,
    SessionBrokerSignedRequestInit, SessionCommentSummary, SessionDaemonAction,
    SessionDaemonCapabilities, SessionDaemonRequest, SessionDaemonResponse,
    SessionLineHighlightTone, SessionReview, SessionSelector, WORKDECK_SESSION_API_PATH,
    WORKDECK_SESSION_BROKER_APP_ID, WORKDECK_SESSION_BROKER_APP_REVISION,
    WORKDECK_SESSION_CAPABILITIES_PATH, WORKDECK_SESSION_DAEMON_HTTP_TIMEOUT_MS,
    load_or_create_workdeck_session_broker_credentials, parse_session_daemon_capabilities,
    parse_session_daemon_response, resolve_session_broker_config,
};

const HTTP_HEADER_LIMIT: usize = 64 * 1024;

/// Transport seam retained for deterministic CLI-client parity tests and alternate native hosts.
pub trait WorkdeckSessionCliCallerTransport: Send + Sync + 'static {
    fn request(
        &self,
        path: &str,
        init: SessionBrokerSignedRequestInit,
    ) -> Result<WorkdeckSessionCliHttpResponse, WorkdeckSessionCliClientError>;
}

impl<F> WorkdeckSessionCliCallerTransport for F
where
    F: Fn(
            &str,
            SessionBrokerSignedRequestInit,
        ) -> Result<WorkdeckSessionCliHttpResponse, WorkdeckSessionCliClientError>
        + Send
        + Sync
        + 'static,
{
    fn request(
        &self,
        path: &str,
        init: SessionBrokerSignedRequestInit,
    ) -> Result<WorkdeckSessionCliHttpResponse, WorkdeckSessionCliClientError> {
        self(path, init)
    }
}

/// Raw application response after the broker envelope has been authenticated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkdeckSessionCliHttpResponse {
    pub status: u16,
    pub status_text: String,
    pub body: Vec<u8>,
}

impl WorkdeckSessionCliHttpResponse {
    pub fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            status_text: canonical_status_text(status).into(),
            body: serde_json::to_vec(value).expect("JSON values always serialize"),
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WorkdeckSessionCliClientError {
    #[error("Session broker authentication failed or the daemon identity could not be verified.")]
    Authentication,
    #[error("Timed out waiting for the Workdeck session daemon to {operation}.")]
    Timeout { operation: String, timeout_ms: u64 },
    #[error("Workdeck session daemon request failed: {0}")]
    Request(String),
    #[error("{0}")]
    Remote(String),
    #[error("Invalid Workdeck session daemon capabilities response.")]
    InvalidCapabilities,
    #[error("Invalid Workdeck session daemon response for {action}.")]
    InvalidResponse { action: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReviewCliInput {
    pub selector: SessionSelector,
    pub include_patch: bool,
    pub include_notes: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionNavigateCliInput {
    pub selector: SessionSelector,
    pub file_path: Option<String>,
    pub hunk_number: Option<u64>,
    pub side: Option<ReviewSide>,
    pub line: Option<u64>,
    pub comment_direction: Option<DaemonCommentDirection>,
    pub comment_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionReloadCliInput {
    pub selector: SessionSelector,
    pub next_input: DaemonCliInput,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCommentAddCliInput {
    pub selector: SessionSelector,
    pub file_path: String,
    pub side: ReviewSide,
    pub line: u64,
    pub summary: String,
    pub rationale: Option<String>,
    pub markup: Option<String>,
    pub author: Option<String>,
    pub reveal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCommentApplyCliInput {
    pub selector: SessionSelector,
    pub comments: Vec<DaemonCommentApplyItem>,
    pub reveal_mode: DaemonRevealMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCommentListCliInput {
    pub selector: SessionSelector,
    pub file_path: Option<String>,
    pub list_type: Option<DaemonCommentListType>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCommentRemoveCliInput {
    pub selector: SessionSelector,
    pub comment_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCommentClearCliInput {
    pub selector: SessionSelector,
    pub file_path: Option<String>,
    pub include_user: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHighlightAddCliInput {
    pub selector: SessionSelector,
    pub file_path: String,
    pub side: ReviewSide,
    pub line: u64,
    pub start: u64,
    pub end: u64,
    pub tone: Option<SessionLineHighlightTone>,
    pub reveal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionHighlightClearCliInput {
    pub selector: SessionSelector,
    pub file_path: Option<String>,
}

/// Complete typed surface used by the `workdeck session` command family.
pub trait WorkdeckSessionCliClient {
    fn get_capabilities(
        &self,
    ) -> Result<Option<SessionDaemonCapabilities>, WorkdeckSessionCliClientError>;
    fn list_sessions(&self) -> Result<Vec<ListedSession>, WorkdeckSessionCliClientError>;
    fn get_session(
        &self,
        selector: SessionSelector,
    ) -> Result<ListedSession, WorkdeckSessionCliClientError>;
    fn get_selected_context(
        &self,
        selector: SessionSelector,
    ) -> Result<SelectedSessionContext, WorkdeckSessionCliClientError>;
    fn get_session_review(
        &self,
        input: SessionReviewCliInput,
    ) -> Result<SessionReview, WorkdeckSessionCliClientError>;
    fn navigate_to_hunk(
        &self,
        input: SessionNavigateCliInput,
    ) -> Result<NavigatedSelectionResult, WorkdeckSessionCliClientError>;
    fn reload_session(
        &self,
        input: SessionReloadCliInput,
    ) -> Result<ReloadedSessionResult, WorkdeckSessionCliClientError>;
    fn add_comment(
        &self,
        input: SessionCommentAddCliInput,
    ) -> Result<AppliedCommentResult, WorkdeckSessionCliClientError>;
    fn apply_comments(
        &self,
        input: SessionCommentApplyCliInput,
    ) -> Result<AppliedCommentBatchResult, WorkdeckSessionCliClientError>;
    fn list_comments(
        &self,
        input: SessionCommentListCliInput,
    ) -> Result<Vec<SessionCommentSummary>, WorkdeckSessionCliClientError>;
    fn remove_comment(
        &self,
        input: SessionCommentRemoveCliInput,
    ) -> Result<RemovedCommentResult, WorkdeckSessionCliClientError>;
    fn clear_comments(
        &self,
        input: SessionCommentClearCliInput,
    ) -> Result<ClearedCommentsResult, WorkdeckSessionCliClientError>;
    fn add_highlight(
        &self,
        input: SessionHighlightAddCliInput,
    ) -> Result<AppliedHighlightResult, WorkdeckSessionCliClientError>;
    fn clear_highlights(
        &self,
        input: SessionHighlightClearCliInput,
    ) -> Result<ClearedHighlightsResult, WorkdeckSessionCliClientError>;
}

type CallerResult = Result<Arc<dyn WorkdeckSessionCliCallerTransport>, String>;

struct LazyCaller {
    value: Mutex<Option<CallerResult>>,
    factory: Box<dyn Fn() -> CallerResult + Send + Sync>,
}

impl LazyCaller {
    fn get(&self) -> CallerResult {
        let mut value = self
            .value
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if value.is_none() {
            *value = Some((self.factory)());
        }
        value.as_ref().expect("lazy caller initialized").clone()
    }
}

#[derive(Clone)]
pub struct HttpWorkdeckSessionCliClient {
    timeout: Duration,
    caller: Arc<LazyCaller>,
}

impl std::fmt::Debug for HttpWorkdeckSessionCliClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("HttpWorkdeckSessionCliClient")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl HttpWorkdeckSessionCliClient {
    #[must_use]
    pub fn with_caller(
        timeout: Duration,
        caller: Arc<dyn WorkdeckSessionCliCallerTransport>,
    ) -> Self {
        Self {
            timeout,
            caller: Arc::new(LazyCaller {
                value: Mutex::new(Some(Ok(caller))),
                factory: Box::new(|| Err("injected caller was unexpectedly unavailable".into())),
            }),
        }
    }

    /// Resolve immutable process configuration now and create credentials only on first use.
    pub fn from_environment(
        env: BTreeMap<String, String>,
        timeout: Duration,
    ) -> Result<Self, WorkdeckSessionCliClientError> {
        let config = resolve_session_broker_config(&env)
            .map_err(|error| WorkdeckSessionCliClientError::Request(error.to_string()))?;
        let factory_timeout = timeout;
        Ok(Self {
            timeout,
            caller: Arc::new(LazyCaller {
                value: Mutex::new(None),
                factory: Box::new(move || {
                    let credentials =
                        load_or_create_workdeck_session_broker_credentials(&env, None)
                            .map_err(|error| error.to_string())?;
                    let transport: Arc<dyn SessionBrokerClientHttpTransport> =
                        Arc::new(NativeSessionBrokerHttpTransport::new(factory_timeout));
                    let caller =
                        SessionBrokerCallerClient::new(SessionBrokerCallerClientOptions::native(
                            WORKDECK_SESSION_BROKER_APP_ID,
                            WORKDECK_SESSION_BROKER_APP_REVISION,
                            config.http_origin.clone(),
                            SessionBrokerClientCredential::<CallerGrant> {
                                grant: credentials.caller.grant,
                                private_key: credentials.caller.private_key,
                            },
                            SessionBrokerDaemonVerifier {
                                key_id: credentials.daemon_identity.key_id,
                                public_key: credentials.daemon_public_key,
                            },
                            transport,
                        ));
                    Ok(Arc::new(AuthenticatedCallerTransport(caller))
                        as Arc<dyn WorkdeckSessionCliCallerTransport>)
                }),
            }),
        })
    }

    pub fn from_process_environment() -> Result<Self, WorkdeckSessionCliClientError> {
        Self::from_environment(
            env::vars().collect(),
            Duration::from_millis(WORKDECK_SESSION_DAEMON_HTTP_TIMEOUT_MS),
        )
    }

    fn call_with_timeout<T, F>(
        &self,
        operation: String,
        task: F,
    ) -> Result<T, WorkdeckSessionCliClientError>
    where
        T: Send + 'static,
        F: FnOnce(
                Arc<dyn WorkdeckSessionCliCallerTransport>,
                SessionBrokerCallerCancellation,
            ) -> Result<T, WorkdeckSessionCliClientError>
            + Send
            + 'static,
    {
        let caller = self
            .caller
            .get()
            .map_err(WorkdeckSessionCliClientError::Request)?;
        let cancellation = SessionBrokerCallerCancellation::default();
        let task_cancellation = cancellation.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let _ = sender.send(task(caller, task_cancellation));
        });
        match receiver.recv_timeout(self.timeout) {
            Ok(result) => result,
            Err(_) => {
                cancellation.cancel("session CLI request deadline expired");
                // Give a cooperative transport one bounded cleanup window to observe cancellation
                // and release its waiter before this command returns. The request deadline still
                // decides the public result; an uncooperative transport remains detached.
                let _ = receiver.recv_timeout(Duration::from_millis(100));
                Err(WorkdeckSessionCliClientError::Timeout {
                    operation,
                    timeout_ms: self.timeout.as_millis().try_into().unwrap_or(u64::MAX),
                })
            }
        }
    }

    fn request(
        &self,
        request: SessionDaemonRequest,
    ) -> Result<SessionDaemonResponse, WorkdeckSessionCliClientError> {
        let action = request_action(&request);
        let action_name = action_name(action).to_owned();
        let operation = format!("complete session {action_name}");
        self.call_with_timeout(operation, move |caller, cancellation| {
            let body = serde_json::to_string(&request)
                .map_err(|error| WorkdeckSessionCliClientError::Request(error.to_string()))?;
            let response = caller.request(
                WORKDECK_SESSION_API_PATH,
                SessionBrokerSignedRequestInit {
                    method: Some("POST".into()),
                    headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                    body: Some(body),
                    target_specific: action != SessionDaemonAction::List,
                    cancellation: Some(cancellation),
                },
            )?;
            ensure_success(&response)?;
            let value = serde_json::from_slice::<Value>(&response.body).map_err(|_| {
                WorkdeckSessionCliClientError::InvalidResponse {
                    action: action_name.clone(),
                }
            })?;
            parse_session_daemon_response(action, &value).map_err(|_| {
                WorkdeckSessionCliClientError::InvalidResponse {
                    action: action_name,
                }
            })
        })
    }
}

impl WorkdeckSessionCliClient for HttpWorkdeckSessionCliClient {
    fn get_capabilities(
        &self,
    ) -> Result<Option<SessionDaemonCapabilities>, WorkdeckSessionCliClientError> {
        self.call_with_timeout("report capabilities".into(), |caller, cancellation| {
            let response = caller.request(
                WORKDECK_SESSION_CAPABILITIES_PATH,
                SessionBrokerSignedRequestInit {
                    method: Some("GET".into()),
                    cancellation: Some(cancellation),
                    ..SessionBrokerSignedRequestInit::default()
                },
            )?;
            if !(200..300).contains(&response.status) {
                return Ok(None);
            }
            let value = serde_json::from_slice::<Value>(&response.body)
                .map_err(|_| WorkdeckSessionCliClientError::InvalidCapabilities)?;
            Ok(parse_session_daemon_capabilities(&value))
        })
    }

    fn list_sessions(&self) -> Result<Vec<ListedSession>, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::List)? {
            SessionDaemonResponse::List { sessions } => Ok(sessions),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn get_session(
        &self,
        selector: SessionSelector,
    ) -> Result<ListedSession, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::Get { selector })? {
            SessionDaemonResponse::Get { session } => Ok(*session),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn get_selected_context(
        &self,
        selector: SessionSelector,
    ) -> Result<SelectedSessionContext, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::Context { selector })? {
            SessionDaemonResponse::Context { context } => Ok(*context),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn get_session_review(
        &self,
        input: SessionReviewCliInput,
    ) -> Result<SessionReview, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::Review {
            selector: input.selector,
            include_patch: Some(input.include_patch),
            include_notes: input.include_notes,
        })? {
            SessionDaemonResponse::Review { review } => Ok(*review),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn navigate_to_hunk(
        &self,
        input: SessionNavigateCliInput,
    ) -> Result<NavigatedSelectionResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::Navigate {
            selector: input.selector,
            file_path: input.file_path,
            hunk_number: input.hunk_number,
            side: input.side,
            line: input.line,
            comment_direction: input.comment_direction,
            comment_id: input.comment_id,
        })? {
            SessionDaemonResponse::Navigate { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn reload_session(
        &self,
        input: SessionReloadCliInput,
    ) -> Result<ReloadedSessionResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::Reload {
            selector: input.selector,
            next_input: input.next_input,
            source_path: input.source_path,
        })? {
            SessionDaemonResponse::Reload { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn add_comment(
        &self,
        input: SessionCommentAddCliInput,
    ) -> Result<AppliedCommentResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::CommentAdd {
            selector: input.selector,
            file_path: input.file_path,
            side: input.side,
            line: input.line,
            summary: input.summary,
            rationale: input.rationale,
            markup: input.markup,
            author: input.author,
            reveal: input.reveal,
        })? {
            SessionDaemonResponse::CommentAdd { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn apply_comments(
        &self,
        input: SessionCommentApplyCliInput,
    ) -> Result<AppliedCommentBatchResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::CommentApply {
            selector: input.selector,
            comments: input.comments,
            reveal_mode: input.reveal_mode,
        })? {
            SessionDaemonResponse::CommentApply { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn list_comments(
        &self,
        input: SessionCommentListCliInput,
    ) -> Result<Vec<SessionCommentSummary>, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::CommentList {
            selector: input.selector,
            file_path: input.file_path,
            list_type: input.list_type,
        })? {
            SessionDaemonResponse::CommentList { comments } => Ok(comments),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn remove_comment(
        &self,
        input: SessionCommentRemoveCliInput,
    ) -> Result<RemovedCommentResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::CommentRm {
            selector: input.selector,
            comment_id: input.comment_id,
        })? {
            SessionDaemonResponse::CommentRm { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn clear_comments(
        &self,
        input: SessionCommentClearCliInput,
    ) -> Result<ClearedCommentsResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::CommentClear {
            selector: input.selector,
            file_path: input.file_path,
            include_user: input.include_user,
        })? {
            SessionDaemonResponse::CommentClear { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn add_highlight(
        &self,
        input: SessionHighlightAddCliInput,
    ) -> Result<AppliedHighlightResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::HighlightAdd {
            selector: input.selector,
            file_path: input.file_path,
            side: input.side,
            line: input.line,
            start: input.start,
            end: input.end,
            tone: input.tone,
            reveal: input.reveal,
        })? {
            SessionDaemonResponse::HighlightAdd { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }

    fn clear_highlights(
        &self,
        input: SessionHighlightClearCliInput,
    ) -> Result<ClearedHighlightsResult, WorkdeckSessionCliClientError> {
        match self.request(SessionDaemonRequest::HighlightClear {
            selector: input.selector,
            file_path: input.file_path,
        })? {
            SessionDaemonResponse::HighlightClear { result } => Ok(result),
            _ => unreachable!("action parser returns the requested response variant"),
        }
    }
}

struct AuthenticatedCallerTransport(SessionBrokerCallerClient);

impl WorkdeckSessionCliCallerTransport for AuthenticatedCallerTransport {
    fn request(
        &self,
        path: &str,
        init: SessionBrokerSignedRequestInit,
    ) -> Result<WorkdeckSessionCliHttpResponse, WorkdeckSessionCliClientError> {
        let SessionBrokerCallerResponse { status, body, .. } =
            self.0.request(path, init).map_err(|error| match error {
                crate::SessionBrokerCallerClientError::Authentication(_) => {
                    WorkdeckSessionCliClientError::Authentication
                }
                crate::SessionBrokerCallerClientError::Cancelled(reason) => {
                    WorkdeckSessionCliClientError::Request(reason)
                }
            })?;
        Ok(WorkdeckSessionCliHttpResponse {
            status,
            status_text: canonical_status_text(status).into(),
            body: serde_json::to_vec(&body)
                .map_err(|error| WorkdeckSessionCliClientError::Request(error.to_string()))?,
        })
    }
}

#[derive(Debug, Clone)]
struct NativeSessionBrokerHttpTransport {
    timeout: Duration,
}

impl NativeSessionBrokerHttpTransport {
    const fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl SessionBrokerClientHttpTransport for NativeSessionBrokerHttpTransport {
    fn send(
        &self,
        request: SessionBrokerClientHttpRequest,
    ) -> Result<SessionBrokerClientHttpResponse, String> {
        send_native_http(request, self.timeout)
    }
}

fn send_native_http(
    request: SessionBrokerClientHttpRequest,
    timeout: Duration,
) -> Result<SessionBrokerClientHttpResponse, String> {
    check_cancelled(request.cancellation.as_ref())?;
    let url = Url::parse(&request.url).map_err(|error| error.to_string())?;
    if url.scheme() != "http"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("session broker client accepts only plain local HTTP URLs".into());
    }
    let host = url
        .host_str()
        .ok_or_else(|| "session broker URL has no host".to_owned())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "session broker URL has no port".to_owned())?;
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|error| error.to_string())?
        .next()
        .ok_or_else(|| "session broker host did not resolve".to_owned())?;
    let mut stream =
        TcpStream::connect_timeout(&address, timeout).map_err(|error| error.to_string())?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| error.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_millis(50).min(timeout)))
        .map_err(|error| error.to_string())?;

    let method = request.method.trim();
    if method.is_empty()
        || !method
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
    {
        return Err("invalid HTTP method".into());
    }
    let mut target = url.path().to_owned();
    if target.is_empty() {
        target.push('/');
    }
    if let Some(query) = url.query() {
        target.push('?');
        target.push_str(query);
    }
    let authority = if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    };
    let mut encoded = format!("{method} {target} HTTP/1.1\r\nHost: {authority}\r\n").into_bytes();
    for (name, value) in &request.headers {
        if !valid_header_name(name) || value.contains(['\r', '\n']) {
            return Err("invalid HTTP header".into());
        }
        encoded.extend_from_slice(name.as_bytes());
        encoded.extend_from_slice(b": ");
        encoded.extend_from_slice(value.as_bytes());
        encoded.extend_from_slice(b"\r\n");
    }
    encoded.extend_from_slice(
        format!(
            "Content-Length: {}\r\nConnection: close\r\n\r\n",
            request.body.len()
        )
        .as_bytes(),
    );
    encoded.extend_from_slice(&request.body);
    stream
        .write_all(&encoded)
        .map_err(|error| error.to_string())?;
    stream.flush().map_err(|error| error.to_string())?;

    let deadline = Instant::now() + timeout;
    let mut response = Vec::new();
    let limit = crate::DEFAULT_SESSION_BROKER_LIMITS
        .max_http_response_bytes
        .saturating_add(HTTP_HEADER_LIMIT as u64)
        .saturating_add(1);
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        check_cancelled(request.cancellation.as_ref())?;
        if Instant::now() >= deadline {
            return Err("session broker HTTP deadline expired".into());
        }
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(bytes) => {
                response.extend_from_slice(&chunk[..bytes]);
                if response.len() as u64 >= limit {
                    return Err("session broker HTTP response exceeded its limit".into());
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                ) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    parse_native_http_response(&response)
}

fn parse_native_http_response(encoded: &[u8]) -> Result<SessionBrokerClientHttpResponse, String> {
    let header_end = encoded
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| "incomplete HTTP response headers".to_owned())?;
    if header_end > HTTP_HEADER_LIMIT {
        return Err("session broker HTTP headers exceeded their limit".into());
    }
    let header_text =
        std::str::from_utf8(&encoded[..header_end]).map_err(|error| error.to_string())?;
    let mut lines = header_text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| "missing HTTP status".to_owned())?;
    let mut status_fields = status_line.splitn(3, ' ');
    if status_fields.next() != Some("HTTP/1.1") {
        return Err("invalid HTTP response version".into());
    }
    let status = status_fields
        .next()
        .ok_or_else(|| "missing HTTP status".to_owned())?
        .parse::<u16>()
        .map_err(|error| error.to_string())?;
    let mut headers = BTreeMap::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "malformed HTTP response header".to_owned())?;
        let name = name.trim().to_ascii_lowercase();
        if !valid_header_name(&name) || headers.insert(name, value.trim().to_owned()).is_some() {
            return Err("invalid or duplicate HTTP response header".into());
        }
    }
    let raw_body = &encoded[header_end + 4..];
    let body = if headers
        .get("transfer-encoding")
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
    {
        if headers.contains_key("content-length") {
            return Err("ambiguous HTTP response framing".into());
        }
        decode_http_chunks(raw_body)?
    } else {
        if headers.contains_key("transfer-encoding") {
            return Err("unsupported HTTP response transfer encoding".into());
        }
        if let Some(length) = headers.get("content-length") {
            let length = length.parse::<usize>().map_err(|error| error.to_string())?;
            if raw_body.len() != length {
                return Err("HTTP response content length mismatch".into());
            }
        }
        raw_body.to_vec()
    };
    Ok(SessionBrokerClientHttpResponse {
        status,
        headers,
        body: Some(Box::new(Cursor::new(body))),
    })
}

fn decode_http_chunks(mut encoded: &[u8]) -> Result<Vec<u8>, String> {
    let mut body = Vec::new();
    loop {
        let line_end = encoded
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| "invalid HTTP chunk header".to_owned())?;
        let size_text =
            std::str::from_utf8(&encoded[..line_end]).map_err(|error| error.to_string())?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or_default(), 16)
            .map_err(|error| error.to_string())?;
        encoded = &encoded[line_end + 2..];
        if size == 0 {
            return Ok(body);
        }
        if encoded.len() < size + 2 || &encoded[size..size + 2] != b"\r\n" {
            return Err("incomplete HTTP chunk".into());
        }
        body.extend_from_slice(&encoded[..size]);
        if body.len() as u64 > crate::DEFAULT_SESSION_BROKER_LIMITS.max_http_response_bytes {
            return Err("session broker HTTP body exceeded its limit".into());
        }
        encoded = &encoded[size + 2..];
    }
}

fn check_cancelled(cancellation: Option<&SessionBrokerCallerCancellation>) -> Result<(), String> {
    match cancellation.and_then(SessionBrokerCallerCancellation::reason) {
        Some(reason) => Err(reason),
        None => Ok(()),
    }
}

fn valid_header_name(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte))
}

fn ensure_success(
    response: &WorkdeckSessionCliHttpResponse,
) -> Result<(), WorkdeckSessionCliClientError> {
    if (200..300).contains(&response.status) {
        return Ok(());
    }
    let message = serde_json::from_slice::<Value>(&response.body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .filter(|message| !message.is_empty())
        .or_else(|| (!response.status_text.is_empty()).then(|| response.status_text.clone()))
        .unwrap_or_else(|| "Unknown Workdeck session daemon error.".into());
    Err(WorkdeckSessionCliClientError::Remote(message))
}

const fn canonical_status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        413 => "Payload Too Large",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

const fn request_action(request: &SessionDaemonRequest) -> SessionDaemonAction {
    match request {
        SessionDaemonRequest::List => SessionDaemonAction::List,
        SessionDaemonRequest::Get { .. } => SessionDaemonAction::Get,
        SessionDaemonRequest::Context { .. } => SessionDaemonAction::Context,
        SessionDaemonRequest::Review { .. } => SessionDaemonAction::Review,
        SessionDaemonRequest::Navigate { .. } => SessionDaemonAction::Navigate,
        SessionDaemonRequest::Reload { .. } => SessionDaemonAction::Reload,
        SessionDaemonRequest::CommentAdd { .. } => SessionDaemonAction::CommentAdd,
        SessionDaemonRequest::CommentApply { .. } => SessionDaemonAction::CommentApply,
        SessionDaemonRequest::CommentList { .. } => SessionDaemonAction::CommentList,
        SessionDaemonRequest::CommentRm { .. } => SessionDaemonAction::CommentRm,
        SessionDaemonRequest::CommentClear { .. } => SessionDaemonAction::CommentClear,
        SessionDaemonRequest::HighlightAdd { .. } => SessionDaemonAction::HighlightAdd,
        SessionDaemonRequest::HighlightClear { .. } => SessionDaemonAction::HighlightClear,
        SessionDaemonRequest::Quit { .. } => SessionDaemonAction::Quit,
    }
}

const fn action_name(action: SessionDaemonAction) -> &'static str {
    match action {
        SessionDaemonAction::List => "list",
        SessionDaemonAction::Get => "get",
        SessionDaemonAction::Context => "context",
        SessionDaemonAction::Review => "review",
        SessionDaemonAction::Navigate => "navigate",
        SessionDaemonAction::Reload => "reload",
        SessionDaemonAction::CommentAdd => "comment-add",
        SessionDaemonAction::CommentApply => "comment-apply",
        SessionDaemonAction::CommentList => "comment-list",
        SessionDaemonAction::CommentRm => "comment-rm",
        SessionDaemonAction::CommentClear => "comment-clear",
        SessionDaemonAction::HighlightAdd => "highlight-add",
        SessionDaemonAction::HighlightClear => "highlight-clear",
        SessionDaemonAction::Quit => "quit",
    }
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use serde_json::json;
    use tempfile::TempDir;

    use super::*;
    use crate::{
        DaemonCommonOptions, SessionFileSummary, SessionReviewFile, SessionReviewHunk,
        SessionSnapshot, WORKDECK_SESSION_API_VERSION, WORKDECK_SESSION_DAEMON_VERSION,
        WorkdeckSessionInputKind, WorkdeckSessionState,
    };

    fn selector() -> SessionSelector {
        SessionSelector {
            session_id: Some("session-1".into()),
            ..SessionSelector::default()
        }
    }

    fn listed_session() -> ListedSession {
        ListedSession {
            session_id: "session-1".into(),
            pid: 42,
            cwd: "/repo".into(),
            repo_root: Some("/repo".into()),
            launched_at: "2026-01-01T00:00:00Z".into(),
            terminal: None,
            input_kind: WorkdeckSessionInputKind::Vcs,
            title: "repo working tree".into(),
            source_label: "/repo".into(),
            experimental_features: None,
            file_count: 1,
            files: vec![SessionFileSummary {
                id: "file-1".into(),
                path: "src/app.ts".into(),
                previous_path: None,
                additions: 3,
                deletions: 1,
                hunk_count: 1,
            }],
            snapshot: SessionSnapshot {
                updated_at: "2026-01-01T00:00:00Z".into(),
                state: WorkdeckSessionState {
                    selected_file_id: Some("file-1".into()),
                    selected_file_path: Some("src/app.ts".into()),
                    selected_hunk_index: 0,
                    selected_hunk_old_range: Some([1, 1]),
                    selected_hunk_new_range: Some([1, 2]),
                    show_agent_notes: false,
                    note_markup_width: None,
                    live_comment_count: 0,
                    live_comments: Vec::new(),
                    review_note_count: None,
                    review_notes: None,
                    review_publication: None,
                },
            },
        }
    }

    fn selected_context() -> SelectedSessionContext {
        SelectedSessionContext {
            session_id: "session-1".into(),
            title: "repo working tree".into(),
            source_label: "/repo".into(),
            cwd: Some("/repo".into()),
            repo_root: Some("/repo".into()),
            input_kind: WorkdeckSessionInputKind::Vcs,
            experimental_features: None,
            selected_file: None,
            selected_hunk: None,
            show_agent_notes: false,
            note_markup_width: None,
            live_comment_count: 0,
        }
    }

    fn review() -> SessionReview {
        SessionReview {
            session_id: "session-1".into(),
            title: "repo working tree".into(),
            source_label: "/repo".into(),
            cwd: Some("/repo".into()),
            repo_root: Some("/repo".into()),
            input_kind: WorkdeckSessionInputKind::Vcs,
            experimental_features: None,
            selected_file: None,
            selected_hunk: None,
            show_agent_notes: false,
            live_comment_count: 0,
            review_note_count: None,
            review_notes: None,
            files: Vec::<SessionReviewFile>::new(),
        }
    }

    fn comment() -> AppliedCommentResult {
        AppliedCommentResult {
            comment_id: "comment-1".into(),
            file_id: "file-1".into(),
            file_path: "src/app.ts".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 12,
            markup_width: None,
            markup_notes: None,
        }
    }

    fn response_for(action: &str) -> Value {
        match action {
            "list" => json!({"sessions": [listed_session()]}),
            "get" => json!({"session": listed_session()}),
            "context" => json!({"context": selected_context()}),
            "review" => json!({"review": review()}),
            "navigate" => json!({"result": {
                "fileId": "file-1", "filePath": "src/app.ts", "hunkIndex": 1
            }}),
            "reload" => json!({"result": {
                "sessionId": "session-1", "inputKind": "vcs", "title": "repo working tree",
                "sourceLabel": "/repo", "fileCount": 1, "selectedFilePath": "src/app.ts",
                "selectedHunkIndex": 0
            }}),
            "comment-add" => json!({"result": comment()}),
            "comment-apply" => json!({"result": {"applied": [comment()]}}),
            "comment-list" => json!({"comments": [{
                "commentId": "comment-1", "filePath": "src/app.ts", "hunkIndex": 0,
                "side": "new", "line": 12, "summary": "Check this",
                "createdAt": "2026-01-01T00:00:00Z"
            }]}),
            "comment-rm" => json!({"result": {
                "commentId": "comment-1", "removed": true, "remainingCommentCount": 0
            }}),
            "comment-clear" => json!({"result": {
                "removedCount": 1, "remainingCommentCount": 0, "filePath": "src/app.ts"
            }}),
            "highlight-add" => json!({"result": {
                "fileId": "file-1", "filePath": "src/app.ts", "hunkIndex": 0,
                "side": "new", "line": 12, "start": 2, "end": 9, "tone": "warning",
                "fileMarkCount": 1, "revealed": "line"
            }}),
            "highlight-clear" => json!({"result": {
                "removedCount": 2, "remainingCount": 0, "filePath": "src/app.ts"
            }}),
            _ => panic!("unexpected action {action}"),
        }
    }

    fn mapped_client(
        requests: Arc<Mutex<Vec<(String, SessionBrokerSignedRequestInit)>>>,
    ) -> HttpWorkdeckSessionCliClient {
        let caller = move |path: &str, init: SessionBrokerSignedRequestInit| {
            requests
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push((path.into(), init.clone()));
            if path == WORKDECK_SESSION_CAPABILITIES_PATH {
                return Ok(WorkdeckSessionCliHttpResponse::json(
                    200,
                    &json!({
                        "version": WORKDECK_SESSION_API_VERSION,
                        "daemonVersion": WORKDECK_SESSION_DAEMON_VERSION,
                        "actions": ["list", "get", "context", "review", "navigate", "reload",
                            "comment-add", "comment-apply", "comment-list", "comment-rm",
                            "comment-clear", "highlight-add", "highlight-clear"]
                    }),
                ));
            }
            let value: Value = serde_json::from_str(init.body.as_deref().unwrap()).unwrap();
            let action = value["action"].as_str().unwrap();
            Ok(WorkdeckSessionCliHttpResponse::json(
                200,
                &response_for(action),
            ))
        };
        HttpWorkdeckSessionCliClient::with_caller(Duration::from_secs(1), Arc::new(caller))
    }

    #[test]
    fn maps_every_cli_method_onto_the_typed_daemon_envelope() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let client = mapped_client(Arc::clone(&requests));
        let target = selector();

        assert_eq!(client.get_capabilities().unwrap().unwrap().version, 1);
        assert_eq!(client.list_sessions().unwrap(), [listed_session()]);
        assert_eq!(
            client.get_session(target.clone()).unwrap(),
            listed_session()
        );
        assert_eq!(
            client.get_selected_context(target.clone()).unwrap(),
            selected_context()
        );
        assert_eq!(
            client
                .get_session_review(SessionReviewCliInput {
                    selector: target.clone(),
                    include_patch: true,
                    include_notes: None,
                })
                .unwrap(),
            review()
        );
        assert_eq!(
            client
                .navigate_to_hunk(SessionNavigateCliInput {
                    selector: target.clone(),
                    file_path: Some("src/app.ts".into()),
                    hunk_number: Some(2),
                    side: Some(ReviewSide::New),
                    line: Some(12),
                    comment_direction: Some(DaemonCommentDirection::Next),
                    comment_id: None,
                })
                .unwrap()
                .hunk_index,
            1
        );
        assert_eq!(
            client
                .reload_session(SessionReloadCliInput {
                    selector: target.clone(),
                    next_input: DaemonCliInput::Vcs {
                        range: None,
                        range_endpoints: None,
                        staged: false,
                        pathspecs: None,
                        options: DaemonCommonOptions::default(),
                    },
                    source_path: Some("/repo".into()),
                })
                .unwrap()
                .title,
            "repo working tree"
        );
        assert_eq!(
            client
                .add_comment(SessionCommentAddCliInput {
                    selector: target.clone(),
                    file_path: "src/app.ts".into(),
                    side: ReviewSide::New,
                    line: 12,
                    summary: "Check this".into(),
                    rationale: Some("Preserve mapping".into()),
                    markup: None,
                    author: Some("pi".into()),
                    reveal: true,
                })
                .unwrap(),
            comment()
        );
        assert_eq!(
            client
                .apply_comments(SessionCommentApplyCliInput {
                    selector: target.clone(),
                    comments: vec![DaemonCommentApplyItem {
                        file_path: "src/app.ts".into(),
                        hunk_number: None,
                        side: None,
                        line: None,
                        summary: "Check this".into(),
                        rationale: None,
                        markup: None,
                        author: None,
                    }],
                    reveal_mode: DaemonRevealMode::First,
                })
                .unwrap()
                .applied,
            [comment()]
        );
        assert_eq!(
            client
                .list_comments(SessionCommentListCliInput {
                    selector: target.clone(),
                    file_path: Some("src/app.ts".into()),
                    list_type: None,
                })
                .unwrap()
                .len(),
            1
        );
        assert!(
            client
                .remove_comment(SessionCommentRemoveCliInput {
                    selector: target.clone(),
                    comment_id: "comment-1".into(),
                })
                .unwrap()
                .removed
        );
        assert_eq!(
            client
                .clear_comments(SessionCommentClearCliInput {
                    selector: target.clone(),
                    file_path: Some("src/app.ts".into()),
                    include_user: None,
                })
                .unwrap()
                .removed_count,
            1
        );
        assert_eq!(
            client
                .add_highlight(SessionHighlightAddCliInput {
                    selector: target.clone(),
                    file_path: "src/app.ts".into(),
                    side: ReviewSide::New,
                    line: 12,
                    start: 2,
                    end: 9,
                    tone: Some(SessionLineHighlightTone::Warning),
                    reveal: true,
                })
                .unwrap()
                .file_mark_count,
            1
        );
        assert_eq!(
            client
                .clear_highlights(SessionHighlightClearCliInput {
                    selector: target,
                    file_path: Some("src/app.ts".into()),
                })
                .unwrap()
                .removed_count,
            2
        );

        let requests = requests.lock().unwrap_or_else(|error| error.into_inner());
        assert_eq!(requests.len(), 14);
        assert_eq!(requests[0].0, WORKDECK_SESSION_CAPABILITIES_PATH);
        assert_eq!(requests[0].1.method.as_deref(), Some("GET"));
        assert!(!requests[0].1.target_specific);
        assert!(!requests[1].1.target_specific);
        assert!(requests[2..].iter().all(|(_, init)| init.target_specific));
        let review: Value = serde_json::from_str(requests[4].1.body.as_deref().unwrap()).unwrap();
        assert_eq!(
            review,
            json!({"action": "review", "selector": {"sessionId": "session-1"}, "includePatch": true})
        );
        let reload: Value = serde_json::from_str(requests[6].1.body.as_deref().unwrap()).unwrap();
        assert_eq!(
            reload,
            json!({
                "action": "reload", "selector": {"sessionId": "session-1"},
                "nextInput": {"kind": "vcs", "staged": false, "options": {}},
                "sourcePath": "/repo"
            })
        );
    }

    #[test]
    fn times_out_hung_requests_and_cancels_the_transport_waiter() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let observed = Arc::clone(&cancelled);
        let caller = move |_path: &str, init: SessionBrokerSignedRequestInit| loop {
            if init
                .cancellation
                .as_ref()
                .and_then(SessionBrokerCallerCancellation::reason)
                .is_some()
            {
                observed.store(true, Ordering::Release);
                return Err(WorkdeckSessionCliClientError::Request("cancelled".into()));
            }
            std::thread::yield_now();
        };
        let client =
            HttpWorkdeckSessionCliClient::with_caller(Duration::from_millis(10), Arc::new(caller));
        let error = client.list_sessions().unwrap_err();
        assert_eq!(
            error.to_string(),
            "Timed out waiting for the Workdeck session daemon to complete session list."
        );
        for _ in 0..1_000 {
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            std::thread::yield_now();
        }
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn rejects_partial_and_non_json_success_responses() {
        let calls = Arc::new(AtomicUsize::new(0));
        let caller = {
            let calls = Arc::clone(&calls);
            move |_path: &str, _init: SessionBrokerSignedRequestInit| {
                if calls.fetch_add(1, Ordering::AcqRel) == 0 {
                    Ok(WorkdeckSessionCliHttpResponse::json(
                        200,
                        &json!({"sessions": [{"sessionId": "partial", "unknown": true}]}),
                    ))
                } else {
                    Ok(WorkdeckSessionCliHttpResponse {
                        status: 200,
                        status_text: "OK".into(),
                        body: b"not json".to_vec(),
                    })
                }
            }
        };
        let client =
            HttpWorkdeckSessionCliClient::with_caller(Duration::from_secs(1), Arc::new(caller));
        for _ in 0..2 {
            assert_eq!(
                client.list_sessions().unwrap_err().to_string(),
                "Invalid Workdeck session daemon response for list."
            );
        }
    }

    #[test]
    fn successful_responses_are_deserialized_into_owned_models() {
        let raw = Arc::new(Mutex::new(json!({"sessions": [listed_session()]})));
        let caller = {
            let raw = Arc::clone(&raw);
            move |_path: &str, _init: SessionBrokerSignedRequestInit| {
                Ok(WorkdeckSessionCliHttpResponse::json(
                    200,
                    &raw.lock().unwrap_or_else(|error| error.into_inner()),
                ))
            }
        };
        let client =
            HttpWorkdeckSessionCliClient::with_caller(Duration::from_secs(1), Arc::new(caller));
        let result = client.list_sessions().unwrap();
        raw.lock().unwrap_or_else(|error| error.into_inner())["sessions"] = json!([]);
        assert_eq!(result, [listed_session()]);
    }

    #[test]
    fn reports_json_remote_errors_then_status_text_fallbacks() {
        let calls = Arc::new(AtomicUsize::new(0));
        let caller = {
            let calls = Arc::clone(&calls);
            move |_path: &str, _init: SessionBrokerSignedRequestInit| {
                if calls.fetch_add(1, Ordering::AcqRel) == 0 {
                    Ok(WorkdeckSessionCliHttpResponse {
                        status: 404,
                        status_text: "Not Found".into(),
                        body: serde_json::to_vec(&json!({"error": "No matching session."}))
                            .unwrap(),
                    })
                } else {
                    Ok(WorkdeckSessionCliHttpResponse {
                        status: 500,
                        status_text: "Daemon exploded".into(),
                        body: b"not json".to_vec(),
                    })
                }
            }
        };
        let client =
            HttpWorkdeckSessionCliClient::with_caller(Duration::from_secs(1), Arc::new(caller));
        assert_eq!(
            client.list_sessions().unwrap_err().to_string(),
            "No matching session."
        );
        assert_eq!(
            client.list_sessions().unwrap_err().to_string(),
            "Daemon exploded"
        );
    }

    #[test]
    fn native_response_parser_accepts_fixed_and_chunked_bodies_but_rejects_ambiguity() {
        let mut fixed = parse_native_http_response(
            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nContent-Type: application/json\r\n\r\n{}",
        )
        .unwrap();
        let mut body = Vec::new();
        fixed.body.as_mut().unwrap().read_to_end(&mut body).unwrap();
        assert_eq!(body, b"{}");

        let mut chunked = parse_native_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n{}\r\n0\r\n\r\n",
        )
        .unwrap();
        body.clear();
        chunked
            .body
            .as_mut()
            .unwrap()
            .read_to_end(&mut body)
            .unwrap();
        assert_eq!(body, b"{}");

        let error = match parse_native_http_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nContent-Length: 2\r\n\r\n0\r\n\r\n",
        ) {
            Err(error) => error,
            Ok(_) => panic!("ambiguous framing must be rejected"),
        };
        assert!(error.contains("ambiguous"));
    }

    #[test]
    fn native_transport_sends_one_bounded_http_request_over_loopback() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1_024];
            loop {
                let bytes = stream.read(&mut chunk).unwrap();
                assert_ne!(bytes, 0);
                request.extend_from_slice(&chunk[..bytes]);
                if request.ends_with(b"{}") {
                    break;
                }
            }
            let text = String::from_utf8(request).unwrap();
            assert!(text.starts_with("POST /session-api?probe=1 HTTP/1.1\r\n"));
            assert!(text.contains("content-type: application/json\r\n"));
            assert!(text.contains("Content-Length: 2\r\n"));
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 11\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{\"ok\":true}",
                )
                .unwrap();
        });
        let mut response = send_native_http(
            SessionBrokerClientHttpRequest {
                url: format!("http://{address}/session-api?probe=1"),
                method: "POST".into(),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: b"{}".to_vec(),
                cancellation: None,
            },
            Duration::from_secs(1),
        )
        .unwrap();
        let mut body = Vec::new();
        response
            .body
            .as_mut()
            .unwrap()
            .read_to_end(&mut body)
            .unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(body, br#"{"ok":true}"#);
        server.join().unwrap();
    }

    #[test]
    fn constructing_the_default_client_does_not_create_runtime_state() {
        let root = TempDir::new().unwrap();
        let runtime = root.path().join("runtime");
        std::fs::create_dir(&runtime).unwrap();
        let client = HttpWorkdeckSessionCliClient::from_environment(
            BTreeMap::from([(
                "XDG_RUNTIME_DIR".into(),
                runtime.to_string_lossy().into_owned(),
            )]),
            Duration::from_millis(10),
        )
        .unwrap();
        assert!(!runtime.join("workdeck-mcp").exists());
        drop(client);
        assert!(!runtime.join("workdeck-mcp").exists());
    }

    #[test]
    fn request_models_omit_absent_fields_instead_of_emitting_null() {
        let request = SessionDaemonRequest::CommentAdd {
            selector: selector(),
            file_path: "src/app.ts".into(),
            side: ReviewSide::New,
            line: 12,
            summary: "Check this".into(),
            rationale: None,
            markup: None,
            author: None,
            reveal: false,
        };
        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "action": "comment-add", "selector": {"sessionId": "session-1"},
                "filePath": "src/app.ts", "side": "new", "line": 12,
                "summary": "Check this", "reveal": false
            })
        );
    }

    #[test]
    fn native_http_request_rejects_header_injection_before_writing() {
        assert!(!valid_header_name("bad header"));
        assert!(!valid_header_name("bad\r\nname"));
        assert!(valid_header_name("x-session-broker-signature"));
    }

    #[test]
    fn fixture_types_used_by_the_client_remain_provider_neutral() {
        let file = SessionReviewFile {
            summary: SessionFileSummary {
                id: "file-1".into(),
                path: PathBuf::from("src/app.ts").to_string_lossy().into_owned(),
                previous_path: None,
                additions: 1,
                deletions: 1,
                hunk_count: 1,
            },
            patch: Some("@@ -1 +1 @@".into()),
            hunks: vec![SessionReviewHunk {
                index: 0,
                header: "@@ -1 +1 @@".into(),
                old_range: Some([1, 1]),
                new_range: Some([1, 1]),
            }],
        };
        assert_eq!(file.summary.path, "src/app.ts");
    }
}
