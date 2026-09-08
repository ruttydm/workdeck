//! Bounded HTTP compatibility probes for the local session daemon.

use crate::{ResolvedSessionBrokerConfig, WORKDECK_SESSION_DAEMON_VERSION};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use thiserror::Error;

pub const WORKDECK_SESSION_API_PATH: &str = "/session-api";
pub const WORKDECK_SESSION_CAPABILITIES_PATH: &str = "/session-api/capabilities";
pub const WORKDECK_SESSION_API_VERSION: u32 = 1;
pub const WORKDECK_SESSION_DAEMON_HTTP_TIMEOUT_MS: u64 = 5_000;
pub const WORKDECK_DAEMON_UPGRADE_WAIT_MESSAGE: &str = "An older or incompatible Workdeck session daemon is running. Close older Workdeck windows; this window will reconnect automatically.";

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionDaemonHttpError {
    #[error("Timed out waiting for the Workdeck session daemon to {operation}.")]
    Timeout { operation: String, timeout_ms: u64 },
    #[error("session daemon HTTP deadline expired")]
    Deadline,
    #[error("session daemon HTTP request failed: {0}")]
    Request(String),
}

impl SessionDaemonHttpError {
    #[must_use]
    pub fn details(&self) -> Vec<String> {
        match self {
            Self::Timeout {
                timeout_ms,
                operation: _,
            } => vec![
                format!("The daemon did not respond within {timeout_ms}ms."),
                "Run \"workdeck daemon serve\" or open a Workdeck window, then retry.".into(),
            ],
            Self::Deadline | Self::Request(_) => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonHttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

/// Apply one outer deadline even when the operation itself ignores cancellation.
pub fn with_session_daemon_http_timeout<T, F>(
    operation: impl Into<String>,
    timeout: Duration,
    task: F,
) -> Result<T, SessionDaemonHttpError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, SessionDaemonHttpError> + Send + 'static,
{
    let operation = operation.into();
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::spawn(move || {
        let _ = sender.send(task());
    });
    match receiver.recv_timeout(timeout) {
        Ok(Err(SessionDaemonHttpError::Deadline)) | Err(_) => {
            Err(SessionDaemonHttpError::Timeout {
                operation,
                timeout_ms: timeout.as_millis().try_into().unwrap_or(u64::MAX),
            })
        }
        Ok(result) => result,
    }
}

/// GET one daemon endpoint and retain the outer deadline through body parsing.
pub fn request_session_daemon_http<T, F>(
    config: &ResolvedSessionBrokerConfig,
    path: &str,
    operation: &str,
    timeout: Duration,
    parse: F,
) -> Result<T, SessionDaemonHttpError>
where
    T: Send + 'static,
    F: FnOnce(DaemonHttpResponse) -> Result<T, SessionDaemonHttpError> + Send + 'static,
{
    let config = config.clone();
    let path = path.to_owned();
    with_session_daemon_http_timeout(operation, timeout, move || {
        parse(http_get(&config, &path, timeout)?)
    })
}

fn http_get(
    config: &ResolvedSessionBrokerConfig,
    path: &str,
    timeout: Duration,
) -> Result<DaemonHttpResponse, SessionDaemonHttpError> {
    let port = u16::try_from(config.port).map_err(|_| {
        SessionDaemonHttpError::Request(format!("invalid daemon port {}", config.port))
    })?;
    let address = (config.host.as_str(), port)
        .to_socket_addrs()
        .map_err(|error| SessionDaemonHttpError::Request(error.to_string()))?
        .next()
        .ok_or_else(|| SessionDaemonHttpError::Request("daemon host did not resolve".into()))?;
    let mut stream = TcpStream::connect_timeout(&address, timeout).map_err(http_io_error)?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(http_io_error)?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(http_io_error)?;
    let authority = if config.host.contains(':') && !config.host.starts_with('[') {
        format!("[{}]:{}", config.host, config.port)
    } else {
        format!("{}:{}", config.host, config.port)
    };
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {authority}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    )
    .map_err(http_io_error)?;
    stream.flush().map_err(http_io_error)?;
    let mut encoded = Vec::new();
    let mut limited = stream.take((crate::MAX_ENVELOPE_BYTES + 64 * 1024 + 1) as u64);
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        match limited.read(&mut chunk) {
            Ok(0) => break,
            Ok(bytes) => encoded.extend_from_slice(&chunk[..bytes]),
            Err(error)
                if error.kind() == std::io::ErrorKind::ConnectionReset && !encoded.is_empty() =>
            {
                break;
            }
            Err(error) => return Err(http_io_error(error)),
        }
    }
    parse_http_response(&encoded)
}

fn http_io_error(error: std::io::Error) -> SessionDaemonHttpError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        SessionDaemonHttpError::Deadline
    } else {
        SessionDaemonHttpError::Request(error.to_string())
    }
}

fn parse_http_response(encoded: &[u8]) -> Result<DaemonHttpResponse, SessionDaemonHttpError> {
    let header_end = encoded
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| {
            SessionDaemonHttpError::Request("incomplete HTTP response headers".into())
        })?;
    let headers = std::str::from_utf8(&encoded[..header_end])
        .map_err(|error| SessionDaemonHttpError::Request(error.to_string()))?;
    let mut lines = headers.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .ok_or_else(|| SessionDaemonHttpError::Request("invalid HTTP status line".into()))?;
    let chunked = lines.any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding")
                && value
                    .split(',')
                    .any(|encoding| encoding.trim().eq_ignore_ascii_case("chunked"))
        })
    });
    let raw_body = &encoded[header_end + 4..];
    let body = if chunked {
        decode_chunked_body(raw_body)?
    } else {
        raw_body.to_vec()
    };
    if body.len() > crate::MAX_ENVELOPE_BYTES {
        return Err(SessionDaemonHttpError::Request(
            "session daemon HTTP body exceeded the protocol limit".into(),
        ));
    }
    Ok(DaemonHttpResponse { status, body })
}

fn decode_chunked_body(mut encoded: &[u8]) -> Result<Vec<u8>, SessionDaemonHttpError> {
    let mut body = Vec::new();
    loop {
        let line_end = encoded
            .windows(2)
            .position(|window| window == b"\r\n")
            .ok_or_else(|| SessionDaemonHttpError::Request("invalid chunk header".into()))?;
        let size_text = std::str::from_utf8(&encoded[..line_end])
            .map_err(|error| SessionDaemonHttpError::Request(error.to_string()))?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or_default(), 16)
            .map_err(|error| SessionDaemonHttpError::Request(error.to_string()))?;
        encoded = &encoded[line_end + 2..];
        if size == 0 {
            break;
        }
        if encoded.len() < size + 2 || &encoded[size..size + 2] != b"\r\n" {
            return Err(SessionDaemonHttpError::Request(
                "incomplete chunked HTTP body".into(),
            ));
        }
        body.extend_from_slice(&encoded[..size]);
        if body.len() > crate::MAX_ENVELOPE_BYTES {
            return Err(SessionDaemonHttpError::Request(
                "session daemon HTTP body exceeded the protocol limit".into(),
            ));
        }
        encoded = &encoded[size + 2..];
    }
    Ok(body)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionDaemonAction {
    Quit,
    List,
    Get,
    Context,
    Review,
    Navigate,
    Reload,
    CommentAdd,
    CommentApply,
    CommentList,
    CommentRm,
    CommentClear,
    HighlightAdd,
    HighlightClear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDaemonCapabilities {
    pub version: u32,
    pub daemon_version: u32,
    pub actions: Vec<SessionDaemonAction>,
}

/// Return `None` for old, malformed, or unhealthy daemons so the caller can refresh them.
pub fn read_workdeck_session_daemon_capabilities(
    config: &ResolvedSessionBrokerConfig,
    timeout: Duration,
) -> Result<Option<SessionDaemonCapabilities>, SessionDaemonHttpError> {
    request_session_daemon_http(
        config,
        WORKDECK_SESSION_CAPABILITIES_PATH,
        "report capabilities",
        timeout,
        |response| {
            if response.status == 404
                || response.status == 410
                || !(200..300).contains(&response.status)
            {
                return Ok(None);
            }
            let Ok(capabilities) =
                serde_json::from_slice::<SessionDaemonCapabilities>(&response.body)
            else {
                return Ok(None);
            };
            if capabilities.version != WORKDECK_SESSION_API_VERSION
                || capabilities.daemon_version != WORKDECK_SESSION_DAEMON_VERSION
            {
                return Ok(None);
            }
            Ok(Some(capabilities))
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    fn server(status: u16, body: Option<&str>, delay: Duration) -> ResolvedSessionBrokerConfig {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let body = body.map(str::to_owned);
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let bytes = stream.read(&mut chunk).unwrap();
                if bytes == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..bytes]);
            }
            thread::sleep(delay);
            if let Some(body) = body {
                let reason = if status == 200 { "OK" } else { "Error" };
                write!(
                    stream,
                    "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        ResolvedSessionBrokerConfig {
            host: "127.0.0.1".into(),
            port: u32::from(port),
            http_origin: format!("http://127.0.0.1:{port}"),
            ws_origin: format!("ws://127.0.0.1:{port}"),
        }
    }

    #[test]
    fn timeout_wrapper_retires_tasks_that_ignore_the_deadline() {
        let start = Instant::now();
        let error = with_session_daemon_http_timeout(
            "finish a stubborn request",
            Duration::from_millis(10),
            || {
                thread::sleep(Duration::from_millis(100));
                Ok("late")
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains(
            "Timed out waiting for the Workdeck session daemon to finish a stubborn request."
        ));
        assert!(start.elapsed() < Duration::from_millis(80));
    }

    #[test]
    fn request_timeout_remains_active_during_body_parsing() {
        let config = server(200, Some("ok"), Duration::ZERO);
        let error = request_session_daemon_http(
            &config,
            WORKDECK_SESSION_API_PATH,
            "parse a stuck body",
            Duration::from_millis(10),
            |_| {
                thread::sleep(Duration::from_millis(100));
                Ok("late")
            },
        )
        .unwrap_err();
        assert!(
            error.to_string().contains(
                "Timed out waiting for the Workdeck session daemon to parse a stuck body."
            )
        );
    }

    #[test]
    fn capabilities_time_out_on_a_hung_daemon() {
        let config = server(200, None, Duration::from_millis(100));
        let error = read_workdeck_session_daemon_capabilities(&config, Duration::from_millis(10))
            .unwrap_err();
        assert!(error.to_string().contains("report capabilities"));
    }

    #[test]
    fn capabilities_return_none_for_non_ok_responses() {
        let config = server(500, Some(r#"{"error":"boom"}"#), Duration::ZERO);
        assert_eq!(
            read_workdeck_session_daemon_capabilities(&config, Duration::from_secs(1)).unwrap(),
            None
        );
    }

    #[test]
    fn capabilities_require_the_compatibility_version_field() {
        let config = server(
            200,
            Some(r#"{"version":1,"actions":["list"]}"#),
            Duration::ZERO,
        );
        assert_eq!(
            read_workdeck_session_daemon_capabilities(&config, Duration::from_secs(1)).unwrap(),
            None
        );
    }

    #[test]
    fn capabilities_reject_previous_daemon_wire_revisions() {
        let config = server(
            200,
            Some(r#"{"version":1,"daemonVersion":11,"actions":["comment-add","comment-apply"]}"#),
            Duration::ZERO,
        );
        assert_eq!(
            read_workdeck_session_daemon_capabilities(&config, Duration::from_secs(1)).unwrap(),
            None
        );
    }

    #[test]
    fn capabilities_accept_only_matching_api_and_daemon_versions() {
        let config = server(
            200,
            Some(r#"{"version":1,"daemonVersion":12,"actions":["list","get"]}"#),
            Duration::ZERO,
        );
        assert_eq!(
            read_workdeck_session_daemon_capabilities(&config, Duration::from_secs(1)).unwrap(),
            Some(SessionDaemonCapabilities {
                version: 1,
                daemon_version: 12,
                actions: vec![SessionDaemonAction::List, SessionDaemonAction::Get],
            })
        );
    }
}
