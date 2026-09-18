//! Revision-tolerant daemon admin scope: protocol, signed client, and local probe.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    BrokerProtocolError, BrokerProtocolFailureCode, BrokerProtocolResult, BrokerStringOptions,
    CallerGrant, ResolvedSessionBrokerConfig, SessionBrokerCallerCancellation,
    SessionBrokerCallerClient, SessionBrokerCallerClientError, SessionBrokerCallerClientOptions,
    SessionBrokerClientAuthenticationError, SessionBrokerClientCredential,
    SessionBrokerDaemonVerifier, SessionBrokerSignedRequestInit, WORKDECK_SESSION_BROKER_APP_ID,
    WORKDECK_SESSION_DAEMON_HTTP_TIMEOUT_MS, WorkdeckSessionBrokerCredentials,
    load_or_create_workdeck_session_broker_credentials, parse_broker_revision,
    parse_broker_safe_integer, parse_broker_string, parse_exact_broker_record,
    with_session_daemon_http_timeout,
};

/// Fixed app contract of the admin scope; it stands in for the app revision in the hello.
pub const SESSION_BROKER_ADMIN_SCOPE_VERSION: u32 = 1;

/// Admin HTTP paths, separate from the session API's namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerAdminPaths {
    pub challenge: String,
    pub proof: String,
    pub control: String,
}

/// Default admin paths; a daemon that predates the scope simply never serves them.
pub const SESSION_BROKER_ADMIN_CHALLENGE_PATH: &str = "/session-admin/challenge";
pub const SESSION_BROKER_ADMIN_PROOF_PATH: &str = "/session-admin/proof";
pub const SESSION_BROKER_ADMIN_CONTROL_PATH: &str = "/session-admin";

#[must_use]
pub fn default_session_broker_admin_paths() -> SessionBrokerAdminPaths {
    SessionBrokerAdminPaths {
        challenge: SESSION_BROKER_ADMIN_CHALLENGE_PATH.into(),
        proof: SESSION_BROKER_ADMIN_PROOF_PATH.into(),
        control: SESSION_BROKER_ADMIN_CONTROL_PATH.into(),
    }
}

/// Close reason attached producers see when an admin `stop` retires the daemon.
pub const SESSION_BROKER_ADMIN_STOP_CLOSE_REASON: &str = "Session daemon restarting.";

/// The two actions a caller from a different app revision may perform.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBrokerAdminRequest {
    Status,
    Stop,
}

/// One attached session as the frozen v1 status reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBrokerAdminSessionV1 {
    pub session_id: String,
    pub title: String,
    pub cwd: String,
    pub pid: u64,
    /// The app revision the session's producer presented in its hello.
    pub client_daemon_version: u64,
}

/// The daemon's identity and attached sessions as the frozen v1 status reports them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBrokerAdminStatusV1 {
    pub admin_scope_version: u32,
    /// The daemon's app revision — the value the ordinary hello requires an exact match on.
    pub daemon_version: u64,
    /// The app's human-readable build version.
    pub app_version: String,
    pub pid: u64,
    pub started_at: String,
    pub uptime_ms: u64,
    pub sessions: Vec<SessionBrokerAdminSessionV1>,
}

/// Acknowledgement of an accepted admin `stop`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBrokerAdminStopResultV1 {
    pub admin_scope_version: u32,
    pub stopping: bool,
}

/// Parse one exact admin request body.
pub fn parse_session_broker_admin_request(
    value: &Value,
) -> BrokerProtocolResult<SessionBrokerAdminRequest> {
    let record = parse_exact_broker_record(value, &["action"], &[])?;
    match record["action"].as_str() {
        Some("status") => Ok(SessionBrokerAdminRequest::Status),
        Some("stop") => Ok(SessionBrokerAdminRequest::Stop),
        _ => Err(BrokerProtocolError {
            code: BrokerProtocolFailureCode::InvalidDiscriminant,
        }),
    }
}

/// Parse one exact v1 admin session entry.
pub fn parse_session_broker_admin_session_v1(
    value: &Value,
) -> BrokerProtocolResult<SessionBrokerAdminSessionV1> {
    let record = parse_exact_broker_record(
        value,
        &["sessionId", "title", "cwd", "pid", "clientDaemonVersion"],
        &[],
    )?;
    Ok(SessionBrokerAdminSessionV1 {
        session_id: parse_broker_string(&record["sessionId"], BrokerStringOptions::default())?
            .into(),
        title: parse_broker_string(
            &record["title"],
            BrokerStringOptions {
                min_bytes: 0,
                ..BrokerStringOptions::default()
            },
        )?
        .into(),
        cwd: parse_broker_string(
            &record["cwd"],
            BrokerStringOptions {
                min_bytes: 0,
                ..BrokerStringOptions::default()
            },
        )?
        .into(),
        pid: parse_broker_safe_integer(&record["pid"], 1, u64::MAX)?,
        client_daemon_version: parse_broker_revision(&record["clientDaemonVersion"])?,
    })
}

/// Parse one exact v1 admin status body; any other scope version is a protocol failure.
pub fn parse_session_broker_admin_status_v1(
    value: &Value,
) -> BrokerProtocolResult<SessionBrokerAdminStatusV1> {
    let record = parse_exact_broker_record(
        value,
        &[
            "adminScopeVersion",
            "daemonVersion",
            "appVersion",
            "pid",
            "startedAt",
            "uptimeMs",
            "sessions",
        ],
        &[],
    )?;
    if parse_broker_revision(&record["adminScopeVersion"])?
        != u64::from(SESSION_BROKER_ADMIN_SCOPE_VERSION)
    {
        return Err(BrokerProtocolError {
            code: BrokerProtocolFailureCode::InvalidDiscriminant,
        });
    }
    let sessions = record["sessions"]
        .as_array()
        .ok_or(BrokerProtocolError {
            code: BrokerProtocolFailureCode::InvalidField,
        })?
        .iter()
        .map(parse_session_broker_admin_session_v1)
        .collect::<BrokerProtocolResult<Vec<_>>>()?;
    Ok(SessionBrokerAdminStatusV1 {
        admin_scope_version: SESSION_BROKER_ADMIN_SCOPE_VERSION,
        daemon_version: parse_broker_revision(&record["daemonVersion"])?,
        app_version: parse_broker_string(&record["appVersion"], BrokerStringOptions::default())?
            .into(),
        pid: parse_broker_safe_integer(&record["pid"], 1, u64::MAX)?,
        started_at: parse_broker_string(&record["startedAt"], BrokerStringOptions::default())?
            .into(),
        uptime_ms: parse_broker_safe_integer(&record["uptimeMs"], 0, u64::MAX)?,
        sessions,
    })
}

/// Parse one exact v1 admin stop acknowledgement.
pub fn parse_session_broker_admin_stop_result_v1(
    value: &Value,
) -> BrokerProtocolResult<SessionBrokerAdminStopResultV1> {
    let record = parse_exact_broker_record(value, &["adminScopeVersion", "stopping"], &[])?;
    if parse_broker_revision(&record["adminScopeVersion"])?
        != u64::from(SESSION_BROKER_ADMIN_SCOPE_VERSION)
        || record["stopping"] != json!(true)
    {
        return Err(BrokerProtocolError {
            code: BrokerProtocolFailureCode::InvalidDiscriminant,
        });
    }
    Ok(SessionBrokerAdminStopResultV1 {
        admin_scope_version: SESSION_BROKER_ADMIN_SCOPE_VERSION,
        stopping: true,
    })
}

fn admin_paths() -> SessionBrokerAdminPaths {
    default_session_broker_admin_paths()
}

/// Calls the daemon's revision-tolerant admin scope with the ordinary caller credential.
///
/// The hello proposes `SESSION_BROKER_ADMIN_SCOPE_VERSION` in place of the app revision, so the
/// same signed handshake works against any daemon that exposes the scope regardless of which app
/// revision either side was built with. A daemon that predates the scope answers the hello with
/// a refusal, which surfaces as `SessionBrokerClientAuthenticationError`.
#[derive(Debug, Clone)]
pub struct SessionBrokerAdminClient {
    caller: SessionBrokerCallerClient,
    control_path: String,
}

/// Error surface of the admin client: an authentication refusal, or a failed control call.
#[derive(Debug, thiserror::Error, Clone, PartialEq, Eq)]
pub enum SessionBrokerAdminClientError {
    /// The daemon answered but refused the admin hello: it predates the scope.
    #[error(transparent)]
    Authentication(#[from] SessionBrokerClientAuthenticationError),
    /// The control call failed or its response was not the frozen schema.
    #[error("Session broker admin {action} failed{detail}")]
    Control {
        action: &'static str,
        detail: String,
    },
}

impl SessionBrokerAdminClient {
    #[must_use]
    pub fn new(
        config: &ResolvedSessionBrokerConfig,
        credentials: &WorkdeckSessionBrokerCredentials,
        timeout: Duration,
    ) -> Self {
        let paths = admin_paths();
        let transport: Arc<dyn crate::SessionBrokerClientHttpTransport> =
            Arc::new(crate::NativeSessionBrokerHttpTransport::new(timeout));
        let mut options = SessionBrokerCallerClientOptions::native(
            WORKDECK_SESSION_BROKER_APP_ID,
            SESSION_BROKER_ADMIN_SCOPE_VERSION,
            config.http_origin.clone(),
            SessionBrokerClientCredential::<CallerGrant> {
                grant: credentials.caller.grant.clone(),
                private_key: credentials.caller.private_key.clone(),
            },
            SessionBrokerDaemonVerifier {
                key_id: credentials.daemon_identity.key_id.clone(),
                public_key: credentials.daemon_public_key,
            },
            transport,
        );
        // The admin hello lives on its own paths so its caller sessions can never satisfy the
        // session API's authenticator, and vice versa.
        options.challenge_path = paths.challenge.clone();
        options.proof_path = paths.proof.clone();
        Self {
            caller: SessionBrokerCallerClient::new(options),
            control_path: paths.control,
        }
    }

    /// Report the daemon's identity and attached sessions.
    pub fn status(&self) -> Result<SessionBrokerAdminStatusV1, SessionBrokerAdminClientError> {
        let body = self.control(SessionBrokerAdminRequest::Status)?;
        parse_session_broker_admin_status_v1(&body).map_err(|error| {
            SessionBrokerAdminClientError::Control {
                action: "status",
                detail: format!(": {error:?}"),
            }
        })
    }

    /// Ask the daemon to shut down gracefully; attached producers are closed with a restart reason.
    pub fn stop(&self) -> Result<(), SessionBrokerAdminClientError> {
        let body = self.control(SessionBrokerAdminRequest::Stop)?;
        parse_session_broker_admin_stop_result_v1(&body)
            .map(|_| ())
            .map_err(|error| SessionBrokerAdminClientError::Control {
                action: "stop",
                detail: format!(": {error:?}"),
            })
    }

    fn control(
        &self,
        request: SessionBrokerAdminRequest,
    ) -> Result<Value, SessionBrokerAdminClientError> {
        let action = match request {
            SessionBrokerAdminRequest::Status => "status",
            SessionBrokerAdminRequest::Stop => "stop",
        };
        let body = match request {
            SessionBrokerAdminRequest::Status => json!({"action": "status"}),
            SessionBrokerAdminRequest::Stop => json!({"action": "stop"}),
        };
        let response = self.caller.request(
            &self.control_path,
            SessionBrokerSignedRequestInit {
                method: Some("POST".into()),
                headers: BTreeMap::from([("content-type".into(), "application/json".into())]),
                body: Some(body.to_string()),
                target_specific: false,
                cancellation: Some(SessionBrokerCallerCancellation::default()),
            },
        )?;
        if !(200..300).contains(&response.status) {
            let code = response.body.get("error").and_then(Value::as_str);
            return Err(SessionBrokerAdminClientError::Control {
                action,
                detail: code.map_or_else(|| ".".into(), |code| format!(": {code}.")),
            });
        }
        Ok(response.body)
    }
}

/// What the local daemon answered about its admin scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkdeckDaemonAdminProbe {
    Status(SessionBrokerAdminStatusV1),
    /// The listener answered but refused the admin hello: a daemon from before the scope existed.
    Unsupported,
    /// Nothing answered at the daemon origin.
    Unavailable,
}

/// Result of asking the daemon to stop through the admin scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkdeckDaemonStopRequest {
    Stopping,
    Unsupported,
    Unavailable,
}

/// Marker the probe/stop wrappers thread through the outer deadline as a refused admin hello.
const ADMIN_AUTH_REFUSAL_MARKER: &str = "session-broker-admin-authentication-refused";

fn admin_request_error(error: SessionBrokerAdminClientError) -> crate::SessionDaemonHttpError {
    crate::SessionDaemonHttpError::Request(match error {
        SessionBrokerAdminClientError::Authentication(_) => ADMIN_AUTH_REFUSAL_MARKER.to_owned(),
        SessionBrokerAdminClientError::Control { action, detail } => {
            format!("admin {action} failed{detail}")
        }
    })
}

/// Load this process's on-disk caller credential and bind the admin client to the config.
pub fn create_workdeck_session_daemon_admin_client(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<SessionBrokerAdminClient, String> {
    let credentials = load_or_create_workdeck_session_broker_credentials(env, None)
        .map_err(|error| error.to_string())?;
    Ok(SessionBrokerAdminClient::new(config, &credentials, timeout))
}

/// What a failed admin call means: an older daemon, or nothing listening at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdminCallFailure {
    Unsupported,
    Unavailable,
}

fn classify_admin_call_failure(error: &crate::SessionDaemonHttpError) -> AdminCallFailure {
    match error {
        crate::SessionDaemonHttpError::Request(message)
            if message.contains(ADMIN_AUTH_REFUSAL_MARKER) =>
        {
            AdminCallFailure::Unsupported
        }
        _ => AdminCallFailure::Unavailable,
    }
}

/// Read the daemon's admin status, tolerating both an older daemon and an absent one.
pub fn probe_workdeck_session_daemon_admin_status(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> WorkdeckDaemonAdminProbe {
    let config = config.clone();
    let env = env.clone();
    let probe = with_session_daemon_http_timeout("report its status", timeout, move || {
        let client = create_workdeck_session_daemon_admin_client(&config, &env, timeout)
            .map_err(crate::SessionDaemonHttpError::Request)?;
        client.status().map_err(admin_request_error)
    });
    match probe {
        Ok(status) => WorkdeckDaemonAdminProbe::Status(status),
        Err(error) => match classify_admin_call_failure(&error) {
            AdminCallFailure::Unsupported => WorkdeckDaemonAdminProbe::Unsupported,
            AdminCallFailure::Unavailable => WorkdeckDaemonAdminProbe::Unavailable,
        },
    }
}

/// Ask the daemon to stop; `Unsupported` for a daemon that predates the admin scope.
pub fn request_workdeck_session_daemon_stop(
    config: &ResolvedSessionBrokerConfig,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> WorkdeckDaemonStopRequest {
    let config = config.clone();
    let env = env.clone();
    let stop =
        with_session_daemon_http_timeout("acknowledge the stop request", timeout, move || {
            let client = create_workdeck_session_daemon_admin_client(&config, &env, timeout)
                .map_err(crate::SessionDaemonHttpError::Request)?;
            client.stop().map_err(admin_request_error)
        });
    match stop {
        Ok(()) => WorkdeckDaemonStopRequest::Stopping,
        Err(error) => match classify_admin_call_failure(&error) {
            AdminCallFailure::Unsupported => WorkdeckDaemonStopRequest::Unsupported,
            AdminCallFailure::Unavailable => WorkdeckDaemonStopRequest::Unavailable,
        },
    }
}

/// The default admin probe timeout shared by status and restart paths.
pub const fn default_admin_probe_timeout() -> Duration {
    Duration::from_millis(WORKDECK_SESSION_DAEMON_HTTP_TIMEOUT_MS)
}

impl From<SessionBrokerCallerClientError> for SessionBrokerAdminClientError {
    fn from(error: SessionBrokerCallerClientError) -> Self {
        match error {
            SessionBrokerCallerClientError::Authentication(inner) => {
                SessionBrokerAdminClientError::Authentication(inner)
            }
            SessionBrokerCallerClientError::Cancelled(reason) => {
                SessionBrokerAdminClientError::Control {
                    action: "request",
                    detail: format!(": {reason}"),
                }
            }
            SessionBrokerCallerClientError::Transport(reason) => {
                SessionBrokerAdminClientError::Control {
                    action: "request",
                    detail: format!(": {reason}"),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn status_body() -> Value {
        json!({
            "adminScopeVersion": SESSION_BROKER_ADMIN_SCOPE_VERSION,
            "daemonVersion": 12,
            "appVersion": "0.22.0",
            "pid": 4242,
            "startedAt": "2026-09-08T09:42:00.000Z",
            "uptimeMs": 1_000,
            "sessions": [{
                "sessionId": "session-1",
                "title": "review 1",
                "cwd": "/repo",
                "pid": 100,
                "clientDaemonVersion": 12,
            }],
        })
    }

    #[test]
    fn parses_the_two_admin_actions_exactly() {
        assert_eq!(
            parse_session_broker_admin_request(&json!({"action": "status"})).unwrap(),
            SessionBrokerAdminRequest::Status
        );
        assert_eq!(
            parse_session_broker_admin_request(&json!({"action": "stop"})).unwrap(),
            SessionBrokerAdminRequest::Stop
        );
        for invalid in [
            json!({"action": "restart"}),
            json!({"action": "status", "extra": 1}),
            json!("status"),
        ] {
            assert!(parse_session_broker_admin_request(&invalid).is_err());
        }
    }

    #[test]
    fn parses_the_frozen_v1_status_and_stop_shapes_exactly() {
        let status = parse_session_broker_admin_status_v1(&status_body()).unwrap();
        assert_eq!(status.daemon_version, 12);
        assert_eq!(status.app_version, "0.22.0");
        assert_eq!(status.sessions.len(), 1);
        assert_eq!(status.sessions[0].client_daemon_version, 12);
        assert_eq!(
            parse_session_broker_admin_stop_result_v1(
                &json!({"adminScopeVersion": 1, "stopping": true})
            )
            .unwrap(),
            SessionBrokerAdminStopResultV1 {
                admin_scope_version: 1,
                stopping: true,
            }
        );
    }

    #[test]
    fn rejects_other_scope_versions_and_shape_drift() {
        let mut other_version = status_body();
        other_version["adminScopeVersion"] = json!(2);
        assert!(parse_session_broker_admin_status_v1(&other_version).is_err());
        let mut stopping_false = json!({"adminScopeVersion": 1, "stopping": true});
        stopping_false["stopping"] = json!(false);
        assert!(parse_session_broker_admin_stop_result_v1(&stopping_false).is_err());
        let mut unknown_session_key = status_body();
        unknown_session_key["sessions"][0]["surprise"] = json!(true);
        assert!(parse_session_broker_admin_status_v1(&unknown_session_key).is_err());
    }

    #[test]
    fn the_admin_scope_version_is_frozen_at_one() {
        assert_eq!(SESSION_BROKER_ADMIN_SCOPE_VERSION, 1);
        assert_eq!(
            SESSION_BROKER_ADMIN_STOP_CLOSE_REASON,
            "Session daemon restarting."
        );
    }

    #[test]
    fn a_daemon_without_the_scope_refuses_the_admin_hello_as_unsupported() {
        use crate::{
            BrokerGrant, ProducerGrant, ServeSessionBrokerDaemonOptions, SessionBroker,
            SessionBrokerAuthenticator, SessionBrokerAuthenticatorOptions,
            SessionBrokerAuthorityCredential, SessionBrokerDaemonIdentity,
            SessionBrokerDaemonOptions, SessionBrokerLimitOptions, SessionBrokerOptions,
            WORKDECK_SESSION_BROKER_APP_ID, WORKDECK_SESSION_BROKER_APP_REVISION,
            create_workdeck_session_protocol_parsers,
            load_or_create_workdeck_session_broker_credentials, serve_session_broker_daemon,
        };
        use std::collections::BTreeMap;
        use std::sync::Arc;

        let root = tempfile::tempdir().unwrap();
        let env = BTreeMap::from([(
            "XDG_RUNTIME_DIR".into(),
            root.path().to_string_lossy().into_owned(),
        )]);
        let credentials = load_or_create_workdeck_session_broker_credentials(&env, None).unwrap();
        let probe = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let authenticator = Arc::new(
            SessionBrokerAuthenticator::new(SessionBrokerAuthenticatorOptions {
                app_id: WORKDECK_SESSION_BROKER_APP_ID.into(),
                app_revision: WORKDECK_SESSION_BROKER_APP_REVISION,
                generation: "generation-1".into(),
                daemon_identity: SessionBrokerDaemonIdentity {
                    key_id: credentials.daemon_identity.key_id.clone(),
                    private_key: credentials.daemon_identity.private_key.clone(),
                },
                credentials: vec![
                    SessionBrokerAuthorityCredential {
                        grant: BrokerGrant::Producer(ProducerGrant {
                            base: credentials.producer.grant.base.clone(),
                            operations: credentials.producer.grant.operations.clone(),
                        }),
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
                caller_session_ttl_ms: None,
                max_challenges: None,
                max_challenge_bytes: None,
                max_challenge_transcript_bytes: None,
                max_caller_sessions: None,
                limits: SessionBrokerLimitOptions::default(),
            })
            .unwrap(),
        );
        // A daemon configured exactly like one from before the admin scope existed.
        let broker = Arc::new(
            SessionBroker::new(SessionBrokerOptions {
                protocol_parsers: Arc::new(create_workdeck_session_protocol_parsers().unwrap()),
                limit_options: SessionBrokerLimitOptions::default(),
                describe_session: None,
            })
            .unwrap(),
        );
        let mut daemon_options = SessionBrokerDaemonOptions::new(broker);
        daemon_options.app_id = Some(WORKDECK_SESSION_BROKER_APP_ID.into());
        daemon_options.app_revision = Some(u64::from(WORKDECK_SESSION_BROKER_APP_REVISION));
        daemon_options.hello_authenticator = Some(authenticator);
        daemon_options.idle_timeout_ms = Some(0);
        let daemon = crate::SessionBrokerDaemon::new(daemon_options).unwrap();
        let server = serve_session_broker_daemon(ServeSessionBrokerDaemonOptions::new(
            daemon,
            "127.0.0.1",
            port,
        ))
        .unwrap();

        let config = ResolvedSessionBrokerConfig {
            host: "127.0.0.1".into(),
            port: u32::from(port),
            http_origin: format!("http://127.0.0.1:{port}"),
            ws_origin: format!("ws://127.0.0.1:{port}"),
        };
        assert_eq!(
            probe_workdeck_session_daemon_admin_status(&config, &env, Duration::from_secs(2)),
            WorkdeckDaemonAdminProbe::Unsupported
        );
        server.stop();
        assert!(server.wait_stopped(Duration::from_secs(2)));
    }
}
