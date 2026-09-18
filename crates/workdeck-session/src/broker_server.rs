//! Workdeck-owned session daemon routing above the authenticated broker transport.

use std::collections::BTreeMap;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};
use url::Url;
use workdeck_core::ReviewNoteSource;

use crate::{
    BoundedHttpBody, BrokerBody, BrokerGrant, BrokerHttpResponse, BrowserReviewServer,
    BrowserReviewServerOptions, CallerOperation, ClearCommentsToolInput, ClearHighlightsToolInput,
    CommentBatchItemInput, CommentBatchRevealMode, CommentBatchToolInput, CommentDirection,
    CommentTargetInput, CommentToolInput, DaemonCommentDirection, DaemonCommentListType,
    DaemonRevealMode, DispatchSessionCommand, HighlightToolInput,
    MAX_WORKDECK_REVIEW_ENVELOPE_BYTES, NativeSessionBrokerHttpHandler, NavigateToHunkToolInput,
    ReloadSessionToolInput, RemoveCommentToolInput, RunningSessionBrokerDaemon,
    ServeSessionBrokerDaemonOptions, SessionBrokerAuthenticatedControlFacts,
    SessionBrokerAuthenticatedControlOptions, SessionBrokerAuthenticator,
    SessionBrokerAuthenticatorOptions, SessionBrokerAuthorityCredential,
    SessionBrokerAuthorizationContext, SessionBrokerAuthorizer, SessionBrokerBoundedControlOptions,
    SessionBrokerCapabilities, SessionBrokerDaemon, SessionBrokerDaemonIdentity,
    SessionBrokerDaemonOptions, SessionBrokerHttpRequest, SessionBrokerHttpResponse,
    SessionBrokerLimitOptions, SessionBrokerStateError, SessionCommentSummary, SessionDaemonAction,
    SessionDaemonCapabilities, SessionDaemonRequest, SessionDaemonResponse, SessionNoteFilter,
    SessionSelector, WORKDECK_SESSION_API_PATH, WORKDECK_SESSION_API_VERSION,
    WORKDECK_SESSION_BROKER_APP_ID, WORKDECK_SESSION_CAPABILITIES_PATH,
    WORKDECK_SESSION_DAEMON_VERSION, WorkdeckSessionBrokerCredentials, WorkdeckSessionBrokerError,
    WorkdeckSessionBrokerState, WorkdeckSessionCommandInput, WorkdeckSessionCommandResult,
    WorkdeckSessionInfo, WorkdeckSessionState, encode_base64_url, list_workdeck_session_notes,
    load_or_create_workdeck_session_broker_credentials, parse_session_daemon_request,
    resolve_session_broker_config, serve_session_broker_daemon,
};

pub const DEFAULT_STALE_SESSION_TTL_MS: u64 = 45_000;
pub const DEFAULT_STALE_SESSION_SWEEP_INTERVAL_MS: u64 = 15_000;
pub const DEFAULT_SESSION_DAEMON_IDLE_TIMEOUT_MS: u64 = 60_000;

pub const SUPPORTED_SESSION_ACTIONS: [SessionDaemonAction; 14] = [
    SessionDaemonAction::Quit,
    SessionDaemonAction::List,
    SessionDaemonAction::Get,
    SessionDaemonAction::Context,
    SessionDaemonAction::Review,
    SessionDaemonAction::Navigate,
    SessionDaemonAction::Reload,
    SessionDaemonAction::CommentAdd,
    SessionDaemonAction::CommentApply,
    SessionDaemonAction::CommentList,
    SessionDaemonAction::CommentRm,
    SessionDaemonAction::CommentClear,
    SessionDaemonAction::HighlightAdd,
    SessionDaemonAction::HighlightClear,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHostPort {
    pub host: String,
    pub port: Option<u16>,
}

#[must_use]
pub fn format_daemon_serve_error(message: &str, host: &str, port: u16) -> String {
    let normalized = message.to_lowercase();
    if normalized.contains("eaddrinuse")
        || normalized.contains("address already in use")
        || normalized.contains(&format!("is port {port} in use?"))
    {
        return format!(
            "Session broker daemon could not bind {host}:{port} because the port is already in use. Stop the conflicting process or set WORKDECK_MCP_PORT to a different loopback port."
        );
    }
    format!("Failed to start the session broker daemon on {host}:{port}: {message}")
}

#[must_use]
pub fn session_daemon_capabilities() -> SessionDaemonCapabilities {
    SessionDaemonCapabilities {
        version: WORKDECK_SESSION_API_VERSION,
        daemon_version: WORKDECK_SESSION_DAEMON_VERSION,
        actions: SUPPORTED_SESSION_ACTIONS.to_vec(),
    }
}

#[must_use]
pub fn parse_host_and_port(value: &str) -> Option<ParsedHostPort> {
    let value = value.trim();
    if value.is_empty() || value.contains(',') {
        return None;
    }
    if let Some(rest) = value.strip_prefix('[') {
        let close = rest.find(']')?;
        let host = &rest[..close];
        let suffix = &rest[close + 1..];
        if suffix.is_empty() {
            return Some(ParsedHostPort {
                host: host.into(),
                port: None,
            });
        }
        let port = suffix.strip_prefix(':')?;
        return parse_port(port).map(|port| ParsedHostPort {
            host: host.into(),
            port: Some(port),
        });
    }
    match value.matches(':').count() {
        0 => Some(ParsedHostPort {
            host: value.into(),
            port: None,
        }),
        1 => {
            let (host, port) = value.split_once(':')?;
            (!host.is_empty()).then_some(())?;
            parse_port(port).map(|port| ParsedHostPort {
                host: host.into(),
                port: Some(port),
            })
        }
        _ => None,
    }
}

fn parse_port(value: &str) -> Option<u16> {
    (!value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| value.parse::<u16>().ok().filter(|port| *port > 0))?
}

#[must_use]
pub fn is_allowed_host_port(
    host_port: &ParsedHostPort,
    expected_port: u16,
    allow_remote: bool,
) -> bool {
    (allow_remote || crate::is_loopback_host(&host_port.host))
        && host_port.port.unwrap_or(80) == expected_port
}

#[must_use]
pub fn validate_host_header(
    request: &SessionBrokerHttpRequest,
    expected_port: u16,
    allow_remote: bool,
) -> Option<SessionBrokerHttpResponse> {
    let Some(host) = request.header("host") else {
        return Some(json_error(
            "Expected Host header for the local session broker.",
            400,
        ));
    };
    let allowed = parse_host_and_port(host)
        .as_ref()
        .is_some_and(|host_port| is_allowed_host_port(host_port, expected_port, allow_remote));
    (!allowed).then(|| {
        json_error(
            "Host header is not allowed for the local session broker.",
            403,
        )
    })
}

#[must_use]
pub fn validate_origin_header(
    request: &SessionBrokerHttpRequest,
    expected_port: u16,
    allow_remote: bool,
) -> Option<SessionBrokerHttpResponse> {
    let origin = request.header("origin")?;
    let invalid = || json_error("Origin is not allowed for the local session broker.", 403);
    if origin == "null" || origin.contains(',') {
        return Some(invalid());
    }
    let Ok(url) = Url::parse(origin) else {
        return Some(invalid());
    };
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || url.origin().ascii_serialization() != origin
    {
        return Some(invalid());
    }
    let default_port = if url.scheme() == "http" { 80 } else { 443 };
    let Some(host) = url.host_str() else {
        return Some(invalid());
    };
    let host_port = ParsedHostPort {
        host: host.into(),
        port: Some(url.port().unwrap_or(default_port)),
    };
    (!is_allowed_host_port(&host_port, expected_port, allow_remote)).then(invalid)
}

fn json_error(message: &str, status: u16) -> SessionBrokerHttpResponse {
    SessionBrokerHttpResponse::json(status, &json!({"error": message}))
}

fn json_response(value: &impl Serialize) -> SessionBrokerHttpResponse {
    match serde_json::to_value(value) {
        Ok(value) => SessionBrokerHttpResponse::json(200, &value),
        Err(_) => json_error("Could not encode the session API response.", 500),
    }
}

fn parse_json_request_bytes(bytes: &[u8]) -> Result<SessionDaemonRequest, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Expected one JSON request body.")?;
    let value = serde_json::from_str::<Value>(text)
        .map_err(|_| "Expected one JSON request body.".to_owned())?;
    parse_session_daemon_request(&value).map_err(|error| error.to_string())
}

fn request_selector(request: &SessionDaemonRequest) -> Option<&SessionSelector> {
    match request {
        SessionDaemonRequest::List => None,
        SessionDaemonRequest::Get { selector }
        | SessionDaemonRequest::Context { selector }
        | SessionDaemonRequest::Review { selector, .. }
        | SessionDaemonRequest::Navigate { selector, .. }
        | SessionDaemonRequest::Reload { selector, .. }
        | SessionDaemonRequest::CommentAdd { selector, .. }
        | SessionDaemonRequest::CommentApply { selector, .. }
        | SessionDaemonRequest::CommentList { selector, .. }
        | SessionDaemonRequest::CommentRm { selector, .. }
        | SessionDaemonRequest::CommentClear { selector, .. }
        | SessionDaemonRequest::HighlightAdd { selector, .. }
        | SessionDaemonRequest::HighlightClear { selector, .. }
        | SessionDaemonRequest::Quit { selector } => Some(selector),
    }
}

fn request_selector_mut(request: &mut SessionDaemonRequest) -> Option<&mut SessionSelector> {
    match request {
        SessionDaemonRequest::List => None,
        SessionDaemonRequest::Get { selector }
        | SessionDaemonRequest::Context { selector }
        | SessionDaemonRequest::Review { selector, .. }
        | SessionDaemonRequest::Navigate { selector, .. }
        | SessionDaemonRequest::Reload { selector, .. }
        | SessionDaemonRequest::CommentAdd { selector, .. }
        | SessionDaemonRequest::CommentApply { selector, .. }
        | SessionDaemonRequest::CommentList { selector, .. }
        | SessionDaemonRequest::CommentRm { selector, .. }
        | SessionDaemonRequest::CommentClear { selector, .. }
        | SessionDaemonRequest::HighlightAdd { selector, .. }
        | SessionDaemonRequest::HighlightClear { selector, .. }
        | SessionDaemonRequest::Quit { selector } => Some(selector),
    }
}

pub fn session_api_authorization_facts(
    state: &WorkdeckSessionBrokerState,
    bytes: &[u8],
) -> Result<SessionBrokerAuthenticatedControlFacts, String> {
    let input = parse_json_request_bytes(bytes)?;
    if matches!(input, SessionDaemonRequest::List) {
        return Ok(SessionBrokerAuthenticatedControlFacts {
            operation: CallerOperation::List,
            session_id: None,
            command: None,
            command_version: None,
            target_specific: Some(false),
        });
    }
    let selector = request_selector(&input).expect("non-list requests have selectors");
    let session_id = selector
        .session_id
        .clone()
        .map_or_else(
            || {
                state
                    .get_session(selector)
                    .map(|session| session.session_id)
            },
            Ok,
        )
        .map_err(|error| error.to_string())?;
    let (operation, command) = match input {
        SessionDaemonRequest::Get { .. }
        | SessionDaemonRequest::Context { .. }
        | SessionDaemonRequest::Review { .. }
        | SessionDaemonRequest::CommentList { .. } => (CallerOperation::Get, None),
        SessionDaemonRequest::Navigate { .. } => {
            (CallerOperation::Dispatch, Some("navigate_to_hunk"))
        }
        SessionDaemonRequest::Reload { .. } => (CallerOperation::Dispatch, Some("reload_session")),
        SessionDaemonRequest::Quit { .. } => (CallerOperation::Dispatch, Some("quit_session")),
        SessionDaemonRequest::CommentAdd { .. } => (CallerOperation::Dispatch, Some("comment")),
        SessionDaemonRequest::CommentApply { .. } => {
            (CallerOperation::Dispatch, Some("comment_batch"))
        }
        SessionDaemonRequest::CommentRm { .. } => {
            (CallerOperation::Dispatch, Some("remove_comment"))
        }
        SessionDaemonRequest::CommentClear { .. } => {
            (CallerOperation::Dispatch, Some("clear_comments"))
        }
        SessionDaemonRequest::HighlightAdd { .. } => (CallerOperation::Dispatch, Some("highlight")),
        SessionDaemonRequest::HighlightClear { .. } => {
            (CallerOperation::Dispatch, Some("clear_highlights"))
        }
        SessionDaemonRequest::List => unreachable!("list returned above"),
    };
    Ok(SessionBrokerAuthenticatedControlFacts {
        operation,
        session_id: Some(session_id),
        command: command.map(str::to_owned),
        command_version: command.map(|_| 1),
        target_specific: Some(true),
    })
}

pub fn handle_session_api_request(
    state: &WorkdeckSessionBrokerState,
    request: &SessionBrokerHttpRequest,
    body_bytes: Option<&[u8]>,
    resolved_session_id: Option<&str>,
) -> SessionBrokerHttpResponse {
    if request.method != "POST" {
        return json_error("Session API requests must use POST.", 405);
    }
    if request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .is_none_or(|value| !value.eq_ignore_ascii_case("application/json"))
    {
        return json_error("Expected Content-Type application/json.", 415);
    }
    let bytes = body_bytes.unwrap_or(&request.body);
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > crate::MAX_HTTP_BODY_BYTES {
        return json_error("Session broker HTTP body exceeded the protocol limit.", 413);
    }
    let mut input = match parse_json_request_bytes(bytes) {
        Ok(input) => input,
        Err(error) => return json_error(&error, 400),
    };
    if let Some(session_id) = resolved_session_id
        && let Some(selector) = request_selector_mut(&mut input)
    {
        *selector = SessionSelector {
            session_id: Some(session_id.into()),
            ..SessionSelector::default()
        };
    }
    match dispatch_api_request(state, input) {
        Ok(response) => json_response(&response),
        Err(error) => error_response(error),
    }
}

fn dispatch_api_request(
    state: &WorkdeckSessionBrokerState,
    input: SessionDaemonRequest,
) -> Result<SessionDaemonResponse, ApiError> {
    match input {
        SessionDaemonRequest::List => Ok(SessionDaemonResponse::List {
            sessions: state.list_sessions(),
        }),
        SessionDaemonRequest::Get { selector } => Ok(SessionDaemonResponse::Get {
            session: Box::new(state.get_session(&selector)?),
        }),
        SessionDaemonRequest::Context { selector } => Ok(SessionDaemonResponse::Context {
            context: Box::new(state.get_selected_context(&selector)?),
        }),
        SessionDaemonRequest::Review {
            selector,
            include_patch,
            include_notes,
        } => Ok(SessionDaemonResponse::Review {
            review: Box::new(state.get_session_review_with_resources(
                &selector,
                crate::BrokerSessionReviewOptions {
                    include_patch: include_patch.unwrap_or(false),
                    include_notes: include_notes.unwrap_or(false),
                },
            )?),
        }),
        SessionDaemonRequest::Navigate {
            selector,
            file_path,
            hunk_number,
            side,
            line,
            comment_direction,
            comment_id,
        } => {
            let command = resolve_navigate_command_input(
                state,
                selector.clone(),
                file_path,
                hunk_number,
                side,
                line,
                comment_direction,
                comment_id,
            )?;
            let result = dispatch_command(
                state,
                selector,
                "navigate_to_hunk",
                &command,
                "Timed out waiting for the session to navigate to the requested hunk.",
                None,
            )?;
            match result {
                WorkdeckSessionCommandResult::NavigatedSelection(result) => {
                    Ok(SessionDaemonResponse::Navigate { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong navigation result.",
                )),
            }
        }
        SessionDaemonRequest::Reload {
            selector,
            next_input,
            source_path,
        } => {
            let command = ReloadSessionToolInput {
                target_session: selector.clone(),
                next_input: serde_json::to_value(next_input)
                    .map_err(|_| ApiError::message("Could not encode the reload input."))?,
                source_path,
            };
            let result = dispatch_command(
                state,
                selector,
                "reload_session",
                &command,
                "Timed out waiting for the session to reload the requested contents.",
                Some(30_000),
            )?;
            match result {
                WorkdeckSessionCommandResult::ReloadedSession(result) => {
                    Ok(SessionDaemonResponse::Reload { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong reload result.",
                )),
            }
        }
        SessionDaemonRequest::CommentAdd {
            selector,
            file_path,
            side,
            line,
            summary,
            rationale,
            markup,
            author,
            reveal,
        } => {
            let command = CommentToolInput {
                target_session: selector.clone(),
                target: CommentTargetInput {
                    file_path,
                    hunk_index: None,
                    side: Some(side),
                    line: Some(line),
                    summary,
                    rationale,
                    markup,
                    author,
                },
                reveal: Some(reveal),
            };
            let result = dispatch_command(
                state,
                selector,
                "comment",
                &command,
                "Timed out waiting for the session to apply the comment.",
                None,
            )?;
            match result {
                WorkdeckSessionCommandResult::AppliedComment(result) => {
                    Ok(SessionDaemonResponse::CommentAdd { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong comment result.",
                )),
            }
        }
        SessionDaemonRequest::CommentApply {
            selector,
            comments,
            reveal_mode,
        } => {
            let comments = comments
                .into_iter()
                .map(|comment| CommentBatchItemInput {
                    file_path: comment.file_path,
                    hunk_index: comment.hunk_number.map(|number| number - 1),
                    side: comment.side,
                    line: comment.line,
                    summary: comment.summary,
                    rationale: comment.rationale,
                    markup: comment.markup,
                    author: comment.author,
                })
                .collect();
            let command = CommentBatchToolInput {
                target_session: selector.clone(),
                comments,
                reveal_mode: Some(match reveal_mode {
                    DaemonRevealMode::None => CommentBatchRevealMode::None,
                    DaemonRevealMode::First => CommentBatchRevealMode::First,
                }),
            };
            let result = dispatch_command(
                state,
                selector,
                "comment_batch",
                &command,
                "Timed out waiting for the session to apply the comment batch.",
                Some(30_000),
            )?;
            match result {
                WorkdeckSessionCommandResult::AppliedCommentBatch(result) => {
                    Ok(SessionDaemonResponse::CommentApply { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong comment batch result.",
                )),
            }
        }
        SessionDaemonRequest::CommentList {
            selector,
            file_path,
            list_type,
        } => {
            let comments = if list_type.is_none_or(|kind| kind == DaemonCommentListType::Live) {
                state
                    .list_comments(&selector, file_path)
                    .map_err(ApiError::from)?
                    .into_iter()
                    .map(SessionCommentSummary::Live)
                    .collect()
            } else {
                let session = state.get_session(&selector)?;
                let source = match list_type {
                    Some(DaemonCommentListType::Ai) => Some(ReviewNoteSource::Ai),
                    Some(DaemonCommentListType::Agent) => Some(ReviewNoteSource::Agent),
                    Some(DaemonCommentListType::User) => Some(ReviewNoteSource::User),
                    Some(DaemonCommentListType::All) | None => None,
                    Some(DaemonCommentListType::Live) => unreachable!("live handled above"),
                };
                list_workdeck_session_notes(
                    &session,
                    SessionNoteFilter {
                        file_path: file_path.as_deref(),
                        source,
                    },
                )
                .into_iter()
                .map(SessionCommentSummary::Review)
                .collect()
            };
            Ok(SessionDaemonResponse::CommentList { comments })
        }
        SessionDaemonRequest::CommentRm {
            selector,
            comment_id,
        } => {
            let command = RemoveCommentToolInput {
                target_session: selector.clone(),
                comment_id,
            };
            let result = dispatch_command(
                state,
                selector,
                "remove_comment",
                &command,
                "Timed out waiting for the session to remove the requested comment.",
                None,
            )?;
            match result {
                WorkdeckSessionCommandResult::RemovedComment(result) => {
                    Ok(SessionDaemonResponse::CommentRm { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong remove result.",
                )),
            }
        }
        SessionDaemonRequest::CommentClear {
            selector,
            file_path,
            include_user,
        } => {
            let command = ClearCommentsToolInput {
                target_session: selector.clone(),
                file_path,
                include_user,
            };
            let result = dispatch_command(
                state,
                selector,
                "clear_comments",
                &command,
                "Timed out waiting for the session to clear the requested comments.",
                None,
            )?;
            match result {
                WorkdeckSessionCommandResult::ClearedComments(result) => {
                    Ok(SessionDaemonResponse::CommentClear { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong clear result.",
                )),
            }
        }
        SessionDaemonRequest::HighlightAdd {
            selector,
            file_path,
            side,
            line,
            start,
            end,
            tone,
            reveal,
        } => {
            let command = HighlightToolInput {
                target_session: selector.clone(),
                file_path,
                side,
                line,
                start,
                end,
                tone,
                reveal: Some(reveal),
            };
            let result = dispatch_command(
                state,
                selector,
                "highlight",
                &command,
                "Timed out waiting for the session to apply the highlight.",
                None,
            )?;
            match result {
                WorkdeckSessionCommandResult::AppliedHighlight(result) => {
                    Ok(SessionDaemonResponse::HighlightAdd { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong highlight result.",
                )),
            }
        }
        SessionDaemonRequest::Quit { selector } => {
            let command = crate::QuitSessionToolInput {
                target_session: selector.clone(),
            };
            let result = dispatch_command(
                state,
                selector,
                "quit_session",
                &command,
                "Timed out waiting for the session to quit.",
                None,
            )?;
            match result {
                WorkdeckSessionCommandResult::QuitSession(result) => {
                    Ok(SessionDaemonResponse::Quit { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong quit result.",
                )),
            }
        }
        SessionDaemonRequest::HighlightClear {
            selector,
            file_path,
        } => {
            let command = ClearHighlightsToolInput {
                target_session: selector.clone(),
                file_path,
            };
            let result = dispatch_command(
                state,
                selector,
                "clear_highlights",
                &command,
                "Timed out waiting for the session to clear the requested highlights.",
                None,
            )?;
            match result {
                WorkdeckSessionCommandResult::ClearedHighlights(result) => {
                    Ok(SessionDaemonResponse::HighlightClear { result })
                }
                _ => Err(ApiError::message(
                    "The session returned the wrong highlight clear result.",
                )),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn resolve_navigate_command_input(
    state: &WorkdeckSessionBrokerState,
    selector: SessionSelector,
    file_path: Option<String>,
    hunk_number: Option<u64>,
    side: Option<workdeck_core::ReviewSide>,
    line: Option<u64>,
    comment_direction: Option<DaemonCommentDirection>,
    comment_id: Option<String>,
) -> Result<NavigateToHunkToolInput, ApiError> {
    if let Some(comment_id) = comment_id {
        if comment_direction.is_some()
            || file_path.is_some()
            || hunk_number.is_some()
            || side.is_some()
            || line.is_some()
        {
            return Err(ApiError::message(
                "navigate commentId cannot be combined with another navigation target.",
            ));
        }
        let comment = state
            .list_comments(&selector, None)?
            .into_iter()
            .find(|comment| comment.comment_id == comment_id)
            .ok_or_else(|| {
                ApiError::message(format!(
                    "No live comment with id \"{comment_id}\" exists in the selected session."
                ))
            })?;
        return Ok(NavigateToHunkToolInput {
            target_session: selector,
            file_path: Some(comment.file_path),
            hunk_index: None,
            side: Some(comment.side),
            line: Some(comment.line),
            comment_direction: None,
        });
    }
    if comment_direction.is_none() && hunk_number.is_none() && (side.is_none() || line.is_none()) {
        return Err(ApiError::message(
            "navigate requires commentId, commentDirection, hunkNumber, or both side and line.",
        ));
    }
    let has_exact_line_target = side.is_some() && line.is_some();
    Ok(NavigateToHunkToolInput {
        target_session: selector,
        file_path,
        hunk_index: (!has_exact_line_target)
            .then(|| hunk_number.map(|number| number - 1))
            .flatten(),
        side,
        line,
        comment_direction: comment_direction.map(|direction| match direction {
            DaemonCommentDirection::Next => CommentDirection::Next,
            DaemonCommentDirection::Prev => CommentDirection::Prev,
        }),
    })
}

fn dispatch_command(
    state: &WorkdeckSessionBrokerState,
    selector: SessionSelector,
    command: &str,
    input: &impl Serialize,
    timeout_message: &str,
    timeout_ms: Option<u64>,
) -> Result<WorkdeckSessionCommandResult, ApiError> {
    let mut request = DispatchSessionCommand::new(
        selector,
        command,
        serde_json::to_value(input)
            .map_err(|_| ApiError::message("Could not encode the session command."))?,
        timeout_message,
    );
    request.timeout_ms = timeout_ms;
    state
        .dispatch_command(request)
        .map_err(ApiError::from)?
        .receive()
        .map_err(ApiError::from)
}

#[derive(Debug)]
struct ApiError {
    message: String,
    capacity: Option<crate::BrokerCapacityError>,
}

impl ApiError {
    fn message(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            capacity: None,
        }
    }
}

impl From<SessionBrokerStateError> for ApiError {
    fn from(error: SessionBrokerStateError) -> Self {
        Self {
            message: error.to_string(),
            capacity: error.capacity,
        }
    }
}

impl From<WorkdeckSessionBrokerError> for ApiError {
    fn from(error: WorkdeckSessionBrokerError) -> Self {
        match error {
            WorkdeckSessionBrokerError::Broker(error) => error.into(),
            error => Self::message(error.to_string()),
        }
    }
}

fn error_response(error: ApiError) -> SessionBrokerHttpResponse {
    if let Some(capacity) = error.capacity {
        return SessionBrokerHttpResponse::json(
            503,
            &json!({"error": capacity.code.as_str(), "resource": capacity.resource}),
        );
    }
    json_error(&error.message, 400)
}

type WorkdeckDaemon = SessionBrokerDaemon<
    WorkdeckSessionInfo,
    WorkdeckSessionState,
    WorkdeckSessionCommandInput,
    WorkdeckSessionCommandResult,
    WorkdeckSessionBrokerState,
>;

struct AllowAllSessionBrokerAuthorizer;

impl SessionBrokerAuthorizer for AllowAllSessionBrokerAuthorizer {
    fn authorize<'a>(
        &'a self,
        _context: &'a SessionBrokerAuthorizationContext,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async { true })
    }
}

pub struct ServeWorkdeckSessionBrokerDaemonOptions {
    pub env: BTreeMap<String, String>,
    pub idle_timeout_ms: Option<u64>,
    pub stale_session_ttl_ms: Option<u64>,
    pub stale_session_sweep_interval_ms: Option<u64>,
    pub state: Option<Arc<WorkdeckSessionBrokerState>>,
    pub credentials: Option<Arc<WorkdeckSessionBrokerCredentials>>,
    pub review_handler: Option<NativeSessionBrokerHttpHandler>,
}

impl Default for ServeWorkdeckSessionBrokerDaemonOptions {
    fn default() -> Self {
        Self {
            env: std::env::vars().collect(),
            idle_timeout_ms: None,
            stale_session_ttl_ms: None,
            stale_session_sweep_interval_ms: None,
            state: None,
            credentials: None,
            review_handler: None,
        }
    }
}

pub struct RunningWorkdeckSessionBrokerDaemon {
    server: RunningSessionBrokerDaemon,
    state: Arc<WorkdeckSessionBrokerState>,
    browser_review: BrowserReviewServer,
}

impl RunningWorkdeckSessionBrokerDaemon {
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.server.address()
    }

    #[must_use]
    pub fn state(&self) -> Arc<WorkdeckSessionBrokerState> {
        Arc::clone(&self.state)
    }

    pub fn stop(&self) {
        self.browser_review.close();
        self.server.stop();
    }

    #[must_use]
    pub fn wait_stopped(&self, timeout: Duration) -> bool {
        self.server.wait_stopped(timeout)
    }

    #[must_use]
    pub fn is_stopped(&self) -> bool {
        self.server.is_stopped()
    }
}

impl Drop for RunningWorkdeckSessionBrokerDaemon {
    fn drop(&mut self) {
        self.browser_review.close();
    }
}

pub fn serve_workdeck_session_broker_daemon(
    options: ServeWorkdeckSessionBrokerDaemonOptions,
) -> Result<RunningWorkdeckSessionBrokerDaemon, String> {
    let config = resolve_session_broker_config(&options.env).map_err(|error| error.to_string())?;
    let port = u16::try_from(config.port)
        .map_err(|_| format!("Session broker port {} is invalid.", config.port))?;
    let http_origin = config.http_origin.clone();
    let ws_origin = config.ws_origin.clone();
    let allow_remote = crate::allows_unsafe_remote_session_broker(&options.env);
    let effective_revision = crate::resolve_workdeck_session_daemon_version(&options.env);
    let state = match options.state {
        Some(state) => state,
        None => {
            let parsers = Arc::new(
                crate::create_workdeck_session_protocol_parsers_with_revision(u64::from(
                    effective_revision,
                ))
                .map_err(|error| error.to_string())?,
            );
            Arc::new(
                WorkdeckSessionBrokerState::with_options_and_parsers(
                    Default::default(),
                    &SessionBrokerLimitOptions::default(),
                    parsers,
                )
                .map_err(|error| error.to_string())?,
            )
        }
    };
    let credentials = match options.credentials {
        Some(credentials) => credentials,
        None => Arc::new(
            load_or_create_workdeck_session_broker_credentials(&options.env, None)
                .map_err(|error| error.to_string())?,
        ),
    };
    let mut generation_bytes = [0_u8; 18];
    getrandom::fill(&mut generation_bytes)
        .map_err(|error| format!("Could not generate a broker generation: {error}"))?;
    let authenticator = Arc::new(
        SessionBrokerAuthenticator::new(SessionBrokerAuthenticatorOptions {
            app_id: WORKDECK_SESSION_BROKER_APP_ID.into(),
            app_revision: effective_revision,
            generation: format!("h_{}_0", encode_base64_url(&generation_bytes)),
            daemon_identity: SessionBrokerDaemonIdentity {
                key_id: credentials.daemon_identity.key_id.clone(),
                private_key: credentials.daemon_identity.private_key.clone(),
            },
            credentials: vec![
                SessionBrokerAuthorityCredential {
                    grant: BrokerGrant::Producer(credentials.producer.grant.clone()),
                    public_key: credentials.producer.public_key,
                },
                SessionBrokerAuthorityCredential {
                    grant: BrokerGrant::Caller(credentials.caller.grant.clone()),
                    public_key: credentials.caller.public_key,
                },
            ],
            crypto: None,
            now: None,
            is_revoked: None,
            challenge_ttl_ms: None,
            caller_session_ttl_ms: Some(30_000),
            max_challenges: None,
            max_challenge_bytes: None,
            max_challenge_transcript_bytes: None,
            max_caller_sessions: None,
            limits: SessionBrokerLimitOptions::default(),
        })
        .map_err(|error| error.to_string())?,
    );
    // The admin scope (`workdeck daemon status` / `restart`) must work from a Workdeck build on a
    // different revision, so it authenticates against the frozen scope version with the same
    // credentials. Its caller sessions live only here and can never satisfy the session API's
    // authenticator.
    let admin_authenticator = Arc::new(
        SessionBrokerAuthenticator::new(SessionBrokerAuthenticatorOptions {
            app_id: WORKDECK_SESSION_BROKER_APP_ID.into(),
            app_revision: crate::SESSION_BROKER_ADMIN_SCOPE_VERSION,
            generation: format!("h_{}_1", encode_base64_url(&generation_bytes)),
            daemon_identity: SessionBrokerDaemonIdentity {
                key_id: credentials.daemon_identity.key_id.clone(),
                private_key: credentials.daemon_identity.private_key.clone(),
            },
            credentials: vec![SessionBrokerAuthorityCredential {
                grant: BrokerGrant::Caller(credentials.caller.grant.clone()),
                public_key: credentials.caller.public_key,
            }],
            crypto: None,
            now: None,
            is_revoked: None,
            challenge_ttl_ms: None,
            caller_session_ttl_ms: Some(30_000),
            max_challenges: None,
            max_challenge_bytes: None,
            max_challenge_transcript_bytes: None,
            max_caller_sessions: None,
            limits: SessionBrokerLimitOptions::default(),
        })
        .map_err(|error| error.to_string())?,
    );
    let mut extra = BTreeMap::new();
    extra.insert(
        "actions".into(),
        serde_json::to_value(SUPPORTED_SESSION_ACTIONS)
            .map_err(|error| format!("Could not encode session actions: {error}"))?,
    );
    let mut daemon_options = SessionBrokerDaemonOptions::new(Arc::clone(&state));
    daemon_options.capabilities = Some(SessionBrokerCapabilities {
        version: u64::from(effective_revision),
        name: Some("workdeck-session-broker".into()),
        features: None,
        extra,
    });
    daemon_options.idle_timeout_ms = Some(
        options
            .idle_timeout_ms
            .unwrap_or(DEFAULT_SESSION_DAEMON_IDLE_TIMEOUT_MS),
    );
    daemon_options.stale_session_ttl_ms = Some(
        options
            .stale_session_ttl_ms
            .unwrap_or(DEFAULT_STALE_SESSION_TTL_MS),
    );
    daemon_options.stale_session_sweep_interval_ms = Some(
        options
            .stale_session_sweep_interval_ms
            .unwrap_or(DEFAULT_STALE_SESSION_SWEEP_INTERVAL_MS),
    );
    daemon_options.app_id = Some(WORKDECK_SESSION_BROKER_APP_ID.into());
    daemon_options.app_revision = Some(u64::from(effective_revision));
    daemon_options.caller_authenticator = Some(authenticator.clone());
    daemon_options.hello_authenticator = Some(authenticator);
    daemon_options.admin = Some(crate::SessionBrokerDaemonAdminOptions {
        authenticator: admin_authenticator,
        paths: None,
        app_version: env!("CARGO_PKG_VERSION").into(),
        describe_session: Arc::new(|session: &crate::ListedSession| {
            crate::SessionBrokerAdminSessionFacts {
                session_id: session.session_id.clone(),
                title: session.title.clone(),
                cwd: session.cwd.clone(),
                pid: session.pid,
            }
        }),
    });
    daemon_options.producer_endpoint = Some(format!(
        "{}{}",
        config.ws_origin,
        crate::SESSION_BROKER_SOCKET_PATH
    ));
    daemon_options.authorizer = Some(Arc::new(AllowAllSessionBrokerAuthorizer));
    daemon_options.paths.socket = Some(crate::SESSION_BROKER_SOCKET_PATH.into());
    let daemon = SessionBrokerDaemon::new(daemon_options).map_err(|error| error.to_string())?;

    let action_daemon = daemon.clone();
    let action_control = Arc::new(
        move |request: &SessionBrokerHttpRequest, handler: crate::BrowserReviewActionHandler| {
            action_daemon.handle_bounded_control(
                request,
                SessionBrokerBoundedControlOptions {
                    max_body_bytes: Some(
                        MAX_WORKDECK_REVIEW_ENVELOPE_BYTES.min(crate::MAX_HTTP_BODY_BYTES),
                    ),
                    payload_too_large: Some(Arc::new(|| {
                        crate::review_failure_http(
                            crate::WorkdeckReviewClientErrorCodeV1::PayloadTooLarge,
                            None,
                            None,
                        )
                    })),
                },
                |body| handler(body),
            )
        },
    );
    let browser_review = BrowserReviewServer::new(
        Arc::clone(&state),
        BrowserReviewServerOptions {
            allow_remote,
            handle_action_control: Some(action_control),
            ..BrowserReviewServerOptions::default()
        },
    );
    let default_review_handler = browser_review.handler();
    let route_daemon = daemon.clone();
    let route_state = Arc::clone(&state);
    let review_handler = Some(options.review_handler.unwrap_or(default_review_handler));
    let handler: NativeSessionBrokerHttpHandler = Arc::new(move |request, address| {
        let daemon = route_daemon.clone();
        let state = Arc::clone(&route_state);
        let review_handler = review_handler.clone();
        Box::pin(async move {
            route_workdeck_daemon_request(
                &daemon,
                &state,
                request,
                address,
                port,
                allow_remote,
                review_handler,
            )
            .await
        })
    });
    let host = config.host.clone();
    let mut serve_options = ServeSessionBrokerDaemonOptions::new(daemon, config.host, port);
    serve_options.allow_remote = allow_remote;
    serve_options.handle_request = Some(handler);
    serve_options.format_serve_error = Some(Arc::new(move |error, _| {
        format_daemon_serve_error(&error.to_string(), &host, port)
    }));
    let server = serve_session_broker_daemon(serve_options).map_err(|error| error.to_string())?;
    println!("Session broker API listening on {http_origin}{WORKDECK_SESSION_API_PATH}");
    println!(
        "Session broker websocket listening on {ws_origin}{}",
        crate::SESSION_BROKER_SOCKET_PATH
    );
    Ok(RunningWorkdeckSessionBrokerDaemon {
        server,
        state,
        browser_review,
    })
}

#[allow(clippy::too_many_arguments)]
async fn route_workdeck_daemon_request(
    daemon: &WorkdeckDaemon,
    state: &Arc<WorkdeckSessionBrokerState>,
    request: SessionBrokerHttpRequest,
    address: crate::NativeSessionBrokerAddress,
    port: u16,
    allow_remote: bool,
    review_handler: Option<NativeSessionBrokerHttpHandler>,
) -> Option<BrokerHttpResponse> {
    if let Some(response) = validate_host_header(&request, port, allow_remote) {
        return Some(native_response(response));
    }
    if let Some(response) = validate_origin_header(&request, port, allow_remote) {
        return Some(native_response(response));
    }
    let path = Url::parse(&request.url).ok()?.path().to_owned();
    if matches!(
        path.as_str(),
        WORKDECK_SESSION_CAPABILITIES_PATH | WORKDECK_SESSION_API_PATH
    ) && request.header("x-session-broker-caller-session").is_none()
    {
        return Some(native_response(SessionBrokerHttpResponse::json(
            401,
            &json!({
                "error": "authentication-required",
                "message": "This Workdeck session client must be upgraded to use automatic signed authentication."
            }),
        )));
    }
    if path == WORKDECK_SESSION_CAPABILITIES_PATH {
        let response = daemon.handle_authenticated_control(
            &request,
            SessionBrokerAuthenticatedControlOptions {
                authentication_failure_operation: Some(CallerOperation::Diagnostics),
                resolve_failure_target_specific: None,
            },
            |_| {
                Ok(SessionBrokerAuthenticatedControlFacts {
                    operation: CallerOperation::Diagnostics,
                    session_id: None,
                    command: None,
                    command_version: None,
                    target_specific: Some(false),
                })
            },
            |body, _| {
                if request.method == "GET" && body.is_empty() {
                    crate::SessionBrokerAuthenticatedControlResult {
                        body: serde_json::to_value(session_daemon_capabilities())
                            .expect("capabilities serialize"),
                        status: 200,
                    }
                } else {
                    crate::SessionBrokerAuthenticatedControlResult {
                        body: json!({"error": "Capabilities require GET with an empty body."}),
                        status: if request.method == "GET" { 400 } else { 405 },
                    }
                }
            },
        );
        return Some(native_response(response));
    }
    if path == WORKDECK_SESSION_API_PATH {
        let failure_state = Arc::clone(state);
        let response = daemon.handle_authenticated_control(
            &request,
            SessionBrokerAuthenticatedControlOptions {
                authentication_failure_operation: None,
                resolve_failure_target_specific: Some(Arc::new(move |body| {
                    parse_json_request_bytes(body)
                        .is_ok_and(|input| !matches!(input, SessionDaemonRequest::List))
                })),
            },
            |body| session_api_authorization_facts(state, body).map_err(|_| ()),
            |body, facts| {
                let response = handle_session_api_request(
                    &failure_state,
                    &request,
                    Some(body),
                    facts.session_id.as_deref(),
                );
                crate::SessionBrokerAuthenticatedControlResult {
                    body: response
                        .json_body()
                        .unwrap_or_else(|_| json!({"error": "session-control-failed"})),
                    status: response.status,
                }
            },
        );
        return Some(native_response(response));
    }
    if let Some(review_handler) = review_handler
        && let Some(response) = review_handler(request.clone(), address).await
    {
        return Some(response);
    }
    if path == crate::LEGACY_MCP_PATH {
        return Some(native_response(json_error(
            "This app no longer exposes agent-facing MCP tools. Use the session CLI instead.",
            410,
        )));
    }
    None
}

fn native_response(response: SessionBrokerHttpResponse) -> BrokerHttpResponse {
    let body = (!response.body.is_empty()).then(|| {
        BoundedHttpBody::Streaming(
            Box::new(std::io::Cursor::new(response.body)) as Box<dyn BrokerBody>
        )
    });
    BrokerHttpResponse {
        status: response.status,
        status_text: String::new(),
        headers: response.headers,
        body,
    }
}

#[cfg(test)]
mod tests;
