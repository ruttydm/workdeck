//! Runtime-neutral signed caller HTTP client for the local session broker.

use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Map, Value};
use thiserror::Error;
use url::Url;

use crate::{
    BrokerBody, BrokerCommandScope, BrokerHelloAckBinding, BrokerHelloAckTranscriptInput,
    BrokerHelloProposal, BrokerResponseTranscriptInput, CallerGrant, CallerOperation,
    CallerRequestTranscriptInput, CallerSequenceAllocator, DEFAULT_SESSION_BROKER_LIMITS,
    PendingSessionBrokerHello, SESSION_BROKER_PROTOCOL_REVISION,
    SessionBrokerAuthenticatedResponse, SessionBrokerClientAuthenticationError,
    SessionBrokerClientCredential, SessionBrokerCrypto, SessionBrokerDaemonVerifier,
    SessionBrokerHelloClientOptions, SignedBrokerAppContract,
    answer_session_broker_hello_challenge, build_broker_hello_ack_transcript,
    build_broker_response_transcript, build_caller_request_transcript, canonical_http_target,
    canonical_json_bytes, create_session_broker_hello_request, decode_base64_url,
    encode_base64_url, is_valid_broker_identifier, parse_session_broker_hello_challenge,
    read_request_bytes_with_limit,
};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SessionBrokerCallerClientError {
    /// The daemon refused the signed hello; distinct from never answering at all.
    #[error(transparent)]
    Authentication(#[from] SessionBrokerClientAuthenticationError),
    /// The transport never delivered the request: nothing answered at the daemon origin.
    #[error("session broker caller transport failed: {0}")]
    Transport(String),
    #[error("{0}")]
    Cancelled(String),
}

fn authentication_error() -> SessionBrokerCallerClientError {
    SessionBrokerClientAuthenticationError.into()
}

impl SessionBrokerCallerClientError {
    fn transport(reason: String) -> Self {
        Self::Transport(reason)
    }
}

/// Per-waiter cancellation. Cancelling one clone does not cancel the shared negotiation itself.
#[derive(Debug, Clone, Default)]
pub struct SessionBrokerCallerCancellation(Arc<Mutex<Option<String>>>);

impl PartialEq for SessionBrokerCallerCancellation {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for SessionBrokerCallerCancellation {}

impl SessionBrokerCallerCancellation {
    pub fn cancel(&self, reason: impl Into<String>) {
        *self
            .0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(reason.into());
    }

    #[must_use]
    pub fn reason(&self) -> Option<String> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionBrokerClientHttpRequest {
    pub url: String,
    pub method: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
    pub cancellation: Option<SessionBrokerCallerCancellation>,
}

pub struct SessionBrokerClientHttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Option<Box<dyn BrokerBody>>,
}

impl SessionBrokerClientHttpResponse {
    #[must_use]
    pub fn json(status: u16, value: &Value) -> Self {
        let body = serde_json::to_vec(value).expect("JSON values always serialize");
        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "application/json".into());
        headers.insert("content-length".into(), body.len().to_string());
        Self {
            status,
            headers,
            body: Some(Box::new(std::io::Cursor::new(body))),
        }
    }
}

pub trait SessionBrokerClientHttpTransport: Send + Sync + 'static {
    fn send(
        &self,
        request: SessionBrokerClientHttpRequest,
    ) -> Result<SessionBrokerClientHttpResponse, String>;
}

impl<F> SessionBrokerClientHttpTransport for F
where
    F: Fn(SessionBrokerClientHttpRequest) -> Result<SessionBrokerClientHttpResponse, String>
        + Send
        + Sync
        + 'static,
{
    fn send(
        &self,
        request: SessionBrokerClientHttpRequest,
    ) -> Result<SessionBrokerClientHttpResponse, String> {
        self(request)
    }
}

#[derive(Clone)]
pub struct SessionBrokerCallerClientOptions {
    pub app_id: String,
    pub app_revision: u32,
    pub origin: String,
    pub credential: SessionBrokerClientCredential<CallerGrant>,
    pub daemon: SessionBrokerDaemonVerifier,
    pub transport: Arc<dyn SessionBrokerClientHttpTransport>,
    pub crypto: Arc<dyn SessionBrokerCrypto>,
    pub challenge_path: String,
    pub proof_path: String,
    pub max_response_bytes: u64,
}

impl std::fmt::Debug for SessionBrokerCallerClientOptions {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerCallerClientOptions")
            .field("app_id", &self.app_id)
            .field("app_revision", &self.app_revision)
            .field("origin", &self.origin)
            .field("credential", &self.credential)
            .field("daemon", &self.daemon)
            .field("challenge_path", &self.challenge_path)
            .field("proof_path", &self.proof_path)
            .field("max_response_bytes", &self.max_response_bytes)
            .finish_non_exhaustive()
    }
}

impl SessionBrokerCallerClientOptions {
    #[must_use]
    pub fn native(
        app_id: impl Into<String>,
        app_revision: u32,
        origin: impl Into<String>,
        credential: SessionBrokerClientCredential<CallerGrant>,
        daemon: SessionBrokerDaemonVerifier,
        transport: Arc<dyn SessionBrokerClientHttpTransport>,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            app_revision,
            origin: origin.into(),
            credential,
            daemon,
            transport,
            crypto: Arc::new(crate::NativeSessionBrokerCrypto),
            challenge_path: "/session-auth/challenge".into(),
            proof_path: "/session-auth/proof".into(),
            max_response_bytes: DEFAULT_SESSION_BROKER_LIMITS.max_http_response_bytes,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SessionBrokerSignedRequestInit {
    pub method: Option<String>,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub target_specific: bool,
    pub cancellation: Option<SessionBrokerCallerCancellation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionBrokerCallerResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Value,
}

#[derive(Debug, Clone)]
struct VerifiedCallerSession {
    caller_session_id: String,
    expires_at: f64,
    initial_sequence: String,
    pending: PendingSessionBrokerHello<CallerGrant>,
}

#[derive(Debug)]
struct InstalledCallerSession {
    verified: VerifiedCallerSession,
    sequence: CallerSequenceAllocator,
}

#[derive(Debug, Default)]
struct CallerClientState {
    authentication_epoch: u64,
    session: Option<InstalledCallerSession>,
    negotiation: Option<Arc<NegotiationTicket>>,
}

#[derive(Debug)]
struct NegotiationTicket {
    epoch: u64,
    result: Mutex<Option<Result<(), SessionBrokerCallerClientError>>>,
    ready: Condvar,
}

impl NegotiationTicket {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn finish(&self, result: Result<(), SessionBrokerCallerClientError>) {
        *self
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
        self.ready.notify_all();
    }

    fn wait(
        &self,
        cancellation: Option<&SessionBrokerCallerCancellation>,
    ) -> Result<(), SessionBrokerCallerClientError> {
        let mut result = self
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(reason) = cancellation.and_then(SessionBrokerCallerCancellation::reason) {
                return Err(SessionBrokerCallerClientError::Cancelled(reason));
            }
            if let Some(result) = result.as_ref() {
                return result.clone();
            }
            let waited = self
                .ready
                .wait_timeout(result, Duration::from_millis(5))
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            result = waited.0;
        }
    }
}

struct SessionBrokerCallerClientInner {
    options: SessionBrokerCallerClientOptions,
    state: Mutex<CallerClientState>,
}

#[derive(Clone)]
pub struct SessionBrokerCallerClient(Arc<SessionBrokerCallerClientInner>);

impl std::fmt::Debug for SessionBrokerCallerClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerCallerClient")
            .field("options", &self.0.options)
            .finish_non_exhaustive()
    }
}

impl SessionBrokerCallerClient {
    #[must_use]
    pub fn new(options: SessionBrokerCallerClientOptions) -> Self {
        Self(Arc::new(SessionBrokerCallerClientInner {
            options,
            state: Mutex::new(CallerClientState::default()),
        }))
    }

    /// Issue one signed request, renegotiating once after restart, expiry, or replay rejection.
    pub fn request(
        &self,
        path: &str,
        init: SessionBrokerSignedRequestInit,
    ) -> Result<SessionBrokerCallerResponse, SessionBrokerCallerClientError> {
        self.0.request(path, init)
    }

    pub fn clear(&self) {
        self.0.clear();
    }
}

impl SessionBrokerCallerClientInner {
    fn clear_locked(state: &mut CallerClientState) {
        state.authentication_epoch = state.authentication_epoch.wrapping_add(1);
        state.session = None;
        state.negotiation = None;
    }

    fn clear(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::clear_locked(&mut state);
    }

    fn request(
        self: &Arc<Self>,
        path: &str,
        init: SessionBrokerSignedRequestInit,
    ) -> Result<SessionBrokerCallerResponse, SessionBrokerCallerClientError> {
        for attempt in 0..2 {
            let requires_negotiation = {
                let state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                state
                    .session
                    .as_ref()
                    .is_none_or(|session| now_millis() as f64 >= session.verified.expires_at)
            };
            if requires_negotiation {
                self.ensure_negotiated(init.cancellation.as_ref())?;
            }
            let (attempted_session_id, attempted_epoch) = {
                let state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let session = state.session.as_ref().ok_or_else(authentication_error)?;
                (
                    session.verified.caller_session_id.clone(),
                    state.authentication_epoch,
                )
            };
            match self.signed_request(path, &init)? {
                Some(response) => return Ok(response),
                None if attempt == 0 => {
                    let mut state = self
                        .state
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let still_owns_epoch = state.authentication_epoch == attempted_epoch
                        && state.session.as_ref().is_some_and(|session| {
                            session.verified.caller_session_id == attempted_session_id
                        });
                    if still_owns_epoch {
                        Self::clear_locked(&mut state);
                    }
                }
                None => return Err(authentication_error()),
            }
        }
        Err(authentication_error())
    }

    fn ensure_negotiated(
        self: &Arc<Self>,
        cancellation: Option<&SessionBrokerCallerCancellation>,
    ) -> Result<(), SessionBrokerCallerClientError> {
        if let Some(reason) = cancellation.and_then(SessionBrokerCallerCancellation::reason) {
            return Err(SessionBrokerCallerClientError::Cancelled(reason));
        }
        let (ticket, start) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(ticket) = &state.negotiation {
                (Arc::clone(ticket), false)
            } else {
                let ticket = Arc::new(NegotiationTicket::new(state.authentication_epoch));
                state.negotiation = Some(Arc::clone(&ticket));
                (ticket, true)
            }
        };
        if start {
            let inner = Arc::clone(self);
            let worker_ticket = Arc::clone(&ticket);
            let spawned = std::thread::Builder::new()
                .name("workdeck-broker-auth".into())
                .spawn(move || {
                    let negotiated = catch_unwind(AssertUnwindSafe(|| inner.negotiate()))
                        .unwrap_or_else(|_| Err(authentication_error()));
                    let result = inner.install_negotiated(&worker_ticket, negotiated);
                    worker_ticket.finish(result);
                });
            if spawned.is_err() {
                let error = authentication_error();
                let mut state = self
                    .state
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if state
                    .negotiation
                    .as_ref()
                    .is_some_and(|current| Arc::ptr_eq(current, &ticket))
                {
                    state.negotiation = None;
                }
                drop(state);
                ticket.finish(Err(error.clone()));
                return Err(error);
            }
        }
        ticket.wait(cancellation)
    }

    fn install_negotiated(
        &self,
        ticket: &Arc<NegotiationTicket>,
        negotiated: Result<VerifiedCallerSession, SessionBrokerCallerClientError>,
    ) -> Result<(), SessionBrokerCallerClientError> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let is_current = state.authentication_epoch == ticket.epoch
            && state
                .negotiation
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, ticket));
        let result = match negotiated {
            Ok(verified) if is_current => {
                let installed = verified
                    .initial_sequence
                    .parse::<u64>()
                    .ok()
                    .and_then(|initial| CallerSequenceAllocator::new(initial).ok())
                    .map(|sequence| InstalledCallerSession { verified, sequence });
                if let Some(installed) = installed {
                    state.session = Some(installed);
                    Ok(())
                } else {
                    Err(authentication_error())
                }
            }
            Ok(_) => Err(authentication_error()),
            Err(error) => Err(error),
        };
        if state
            .negotiation
            .as_ref()
            .is_some_and(|current| Arc::ptr_eq(current, ticket))
        {
            state.negotiation = None;
        }
        result
    }

    fn negotiate(&self) -> Result<VerifiedCallerSession, SessionBrokerCallerClientError> {
        let endpoint = format!("{}{}", self.options.origin, self.options.challenge_path);
        let hello_options = SessionBrokerHelloClientOptions {
            app_id: self.options.app_id.clone(),
            app_revision: self.options.app_revision,
            endpoint: endpoint.clone(),
            credential: self.options.credential.clone(),
            daemon: self.options.daemon.clone(),
            crypto: Arc::clone(&self.options.crypto),
        };
        let request = create_session_broker_hello_request(&hello_options)?;
        let challenge_response = self.send_json(
            &endpoint,
            &serde_json::to_value(&request).map_err(|_| authentication_error())?,
        )?;
        if !(200..300).contains(&challenge_response.status) {
            return Err(authentication_error());
        }
        let challenge_value = self.read_bounded_response_json(challenge_response)?;
        let challenge = parse_session_broker_hello_challenge(&challenge_value)?;
        let pending = answer_session_broker_hello_challenge(&hello_options, &request, &challenge)?;
        let proof_url = format!("{}{}", self.options.origin, self.options.proof_path);
        let proof_response = self.send_json(
            &proof_url,
            &serde_json::to_value(&pending.proof).map_err(|_| authentication_error())?,
        )?;
        if !(200..300).contains(&proof_response.status) {
            return Err(authentication_error());
        }
        let session_value = self.read_bounded_response_json(proof_response)?;
        self.verify_caller_ack(pending, &session_value)
    }

    fn send_json(
        &self,
        url: &str,
        value: &Value,
    ) -> Result<SessionBrokerClientHttpResponse, SessionBrokerCallerClientError> {
        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "application/json".into());
        self.options
            .transport
            .send(SessionBrokerClientHttpRequest {
                url: url.into(),
                method: "POST".into(),
                headers,
                body: serde_json::to_vec(value).map_err(|_| authentication_error())?,
                cancellation: None,
            })
            .map_err(SessionBrokerCallerClientError::transport)
    }

    fn read_bounded_response_json(
        &self,
        mut response: SessionBrokerClientHttpResponse,
    ) -> Result<Value, SessionBrokerCallerClientError> {
        let declared = response
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .map(|(_, value)| value.clone());
        let body = response
            .body
            .as_deref_mut()
            .ok_or_else(authentication_error)?;
        let bytes = read_request_bytes_with_limit(
            Some(body),
            declared.as_deref(),
            self.options.max_response_bytes,
        )
        .map_err(|_| authentication_error())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| authentication_error())?;
        serde_json::from_str(text).map_err(|_| authentication_error())
    }

    fn verify_caller_ack(
        &self,
        pending: PendingSessionBrokerHello<CallerGrant>,
        value: &Value,
    ) -> Result<VerifiedCallerSession, SessionBrokerCallerClientError> {
        let record = exact_record(
            value,
            &[
                "callerSessionId",
                "principal",
                "expiresAt",
                "initialSequence",
                "brokerRevision",
                "appRevision",
                "features",
                "helloTranscriptHash",
                "daemonKeyId",
                "daemonSignature",
            ],
        )?;
        let grant = &self.options.credential.grant;
        let principal_keys = if grant.base.session_id.is_some() {
            vec![
                "kind",
                "appId",
                "principalId",
                "keyId",
                "grantId",
                "operations",
                "commands",
                "sessionId",
            ]
        } else {
            vec![
                "kind",
                "appId",
                "principalId",
                "keyId",
                "grantId",
                "operations",
                "commands",
            ]
        };
        let principal = exact_record(
            record.get("principal").ok_or_else(authentication_error)?,
            &principal_keys,
        )?;
        let operations = parse_caller_operations(
            principal
                .get("operations")
                .ok_or_else(authentication_error)?,
        )?;
        let commands =
            parse_command_scopes(principal.get("commands").ok_or_else(authentication_error)?)?;
        let session_id = principal.get("sessionId").and_then(Value::as_str);
        let caller_session_id = required_string(record, "callerSessionId")?;
        let initial_sequence = required_string(record, "initialSequence")?;
        let hello_transcript_hash = required_string(record, "helloTranscriptHash")?;
        let daemon_key_id = required_string(record, "daemonKeyId")?;
        let daemon_signature = required_string(record, "daemonSignature")?;
        let expires_at = record
            .get("expiresAt")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .ok_or_else(authentication_error)?;
        let features = record
            .get("features")
            .and_then(Value::as_array)
            .ok_or_else(authentication_error)?;
        if required_string(principal, "kind")? != "caller"
            || required_string(principal, "appId")? != grant.base.app_id
            || required_string(principal, "principalId")? != grant.base.principal_id
            || required_string(principal, "keyId")? != grant.base.key_id
            || required_string(principal, "grantId")? != grant.base.grant_id
            || session_id != grant.base.session_id.as_deref()
            || operations != grant.operations
            || commands != grant.commands
            || daemon_key_id != self.options.daemon.key_id
            || hello_transcript_hash != pending.transcript_hash
            || required_u32(record, "brokerRevision")? != SESSION_BROKER_PROTOCOL_REVISION
            || required_u32(record, "appRevision")? != self.options.app_revision
            || !features.is_empty()
            || initial_sequence != "1"
            || !is_valid_broker_identifier(caller_session_id)
        {
            return Err(authentication_error());
        }
        let signature = decode_base64_url(daemon_signature).ok_or_else(authentication_error)?;
        let transcript = build_broker_hello_ack_transcript(&BrokerHelloAckTranscriptInput {
            app_id: self.options.app_id.clone(),
            generation: pending.challenge.generation.clone(),
            key_id: grant.base.key_id.clone(),
            grant_id: grant.base.grant_id.clone(),
            hello_transcript_hash: pending.transcript_hash.clone(),
            selection: BrokerHelloProposal {
                broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
                app_revision: self.options.app_revision,
                features: Vec::new(),
            },
            binding: BrokerHelloAckBinding::Caller {
                caller_session_id: caller_session_id.into(),
                initial_sequence: initial_sequence.into(),
            },
        })
        .map_err(|_| authentication_error())?;
        if !self
            .options
            .crypto
            .verify(&self.options.daemon.public_key, &signature, &transcript)
        {
            return Err(authentication_error());
        }
        Ok(VerifiedCallerSession {
            caller_session_id: caller_session_id.into(),
            expires_at,
            initial_sequence: initial_sequence.into(),
            pending,
        })
    }

    fn signed_request(
        &self,
        path: &str,
        init: &SessionBrokerSignedRequestInit,
    ) -> Result<Option<SessionBrokerCallerResponse>, SessionBrokerCallerClientError> {
        if let Some(reason) = init
            .cancellation
            .as_ref()
            .and_then(SessionBrokerCallerCancellation::reason)
        {
            return Err(SessionBrokerCallerClientError::Cancelled(reason));
        }
        let (session, sequence) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let session = state.session.as_mut().ok_or_else(authentication_error)?;
            let sequence = session
                .sequence
                .allocate()
                .ok_or_else(authentication_error)?;
            (session.verified.clone(), sequence)
        };
        let method = init.method.as_deref().unwrap_or("GET").to_uppercase();
        let body = init.body.as_deref().unwrap_or("").as_bytes().to_vec();
        let origin = Url::parse(&self.options.origin).map_err(|_| authentication_error())?;
        let url = origin.join(path).map_err(|_| authentication_error())?;
        if url.origin() != origin.origin()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(authentication_error());
        }
        let request_id = random_id(self.options.crypto.as_ref())?;
        let body_digest = encode_base64_url(&self.options.crypto.sha256(&body));
        let signature = encode_base64_url(
            &self.options.crypto.sign(
                &self.options.credential.private_key,
                &build_caller_request_transcript(&CallerRequestTranscriptInput {
                    app_id: self.options.app_id.clone(),
                    generation: session.pending.challenge.generation.clone(),
                    caller_session_id: session.caller_session_id.clone(),
                    key_id: self.options.credential.grant.base.key_id.clone(),
                    grant_id: self.options.credential.grant.base.grant_id.clone(),
                    hello_transcript_hash: session.pending.transcript_hash.clone(),
                    method: method.clone(),
                    target: canonical_http_target(&url).map_err(|_| authentication_error())?,
                    body_digest,
                    request_id: request_id.clone(),
                    sequence: sequence.clone(),
                })
                .map_err(|_| authentication_error())?,
            ),
        );
        let mut headers = init.headers.clone();
        set_header(
            &mut headers,
            "x-session-broker-caller-session",
            &session.caller_session_id,
        );
        set_header(&mut headers, "x-session-broker-request-id", &request_id);
        set_header(&mut headers, "x-session-broker-sequence", &sequence);
        set_header(&mut headers, "x-session-broker-signature", &signature);
        let response = self
            .options
            .transport
            .send(SessionBrokerClientHttpRequest {
                url: url.into(),
                method,
                headers,
                body,
                cancellation: init.cancellation.clone(),
            })
            .map_err(|_| authentication_error())?;
        let status = response.status;
        let envelope = match self.read_bounded_response_json(response).and_then(|value| {
            self.verify_response(
                &value,
                status,
                &session.caller_session_id,
                &request_id,
                &sequence,
                &session.pending.challenge.generation,
                init.target_specific,
            )
        }) {
            Ok(envelope) => envelope,
            Err(_) if status == 401 => return Ok(None),
            Err(_) => return Err(authentication_error()),
        };
        let mut headers = BTreeMap::new();
        headers.insert("content-type".into(), "application/json".into());
        Ok(Some(SessionBrokerCallerResponse {
            status,
            headers,
            body: envelope.body,
        }))
    }

    #[allow(clippy::too_many_arguments)]
    fn verify_response(
        &self,
        value: &Value,
        status: u16,
        caller_session_id: &str,
        request_id: &str,
        sequence: &str,
        generation: &str,
        target_specific: bool,
    ) -> Result<SessionBrokerAuthenticatedResponse<Value>, SessionBrokerCallerClientError> {
        let envelope = exact_record(value, &["body", "authentication"])?;
        let mut authentication_keys = vec![
            "generation",
            "brokerRevision",
            "callerSessionId",
            "requestId",
            "sequence",
            "httpStatus",
            "bodyDigest",
            "daemonKeyId",
            "daemonSignature",
        ];
        if target_specific {
            authentication_keys.push("appContract");
        }
        let authentication_value = envelope
            .get("authentication")
            .ok_or_else(authentication_error)?;
        exact_record(authentication_value, &authentication_keys)?;
        if target_specific {
            let authentication = authentication_value
                .as_object()
                .ok_or_else(authentication_error)?;
            exact_record(
                authentication
                    .get("appContract")
                    .ok_or_else(authentication_error)?,
                &["appRevision", "features"],
            )?;
        }
        let parsed: SessionBrokerAuthenticatedResponse<Value> =
            serde_json::from_value(value.clone()).map_err(|_| authentication_error())?;
        let auth = &parsed.authentication;
        if auth.generation != generation
            || auth.caller_session_id != caller_session_id
            || auth.request_id != request_id
            || auth.sequence != sequence
            || auth.http_status != status
            || auth.broker_revision != SESSION_BROKER_PROTOCOL_REVISION
            || auth.daemon_key_id != self.options.daemon.key_id
            || target_specific != auth.app_contract.is_some()
            || auth.app_contract.as_ref().is_some_and(|contract| {
                contract.app_revision != self.options.app_revision || !contract.features.is_empty()
            })
        {
            return Err(authentication_error());
        }
        let body_digest = encode_base64_url(
            &self
                .options
                .crypto
                .sha256(&canonical_json_bytes(&parsed.body).map_err(|_| authentication_error())?),
        );
        if body_digest != auth.body_digest {
            return Err(authentication_error());
        }
        let signature =
            decode_base64_url(&auth.daemon_signature).ok_or_else(authentication_error)?;
        let transcript = build_broker_response_transcript(&BrokerResponseTranscriptInput {
            app_id: self.options.app_id.clone(),
            generation: generation.into(),
            broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
            caller_session_id: caller_session_id.into(),
            request_id: request_id.into(),
            sequence: sequence.into(),
            http_status: status,
            body_digest,
            app_contract: auth
                .app_contract
                .as_ref()
                .map(|contract| SignedBrokerAppContract {
                    app_revision: contract.app_revision,
                    features: contract.features.clone(),
                }),
        })
        .map_err(|_| authentication_error())?;
        if !self
            .options
            .crypto
            .verify(&self.options.daemon.public_key, &signature, &transcript)
        {
            return Err(authentication_error());
        }
        Ok(parsed)
    }
}

fn exact_record<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a Map<String, Value>, SessionBrokerCallerClientError> {
    let record = value.as_object().ok_or_else(authentication_error)?;
    let expected = keys.iter().copied().collect::<BTreeSet<_>>();
    if record.len() != expected.len()
        || record.keys().any(|key| {
            matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                || !expected.contains(key.as_str())
        })
    {
        return Err(authentication_error());
    }
    Ok(record)
}

fn required_string<'a>(
    record: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, SessionBrokerCallerClientError> {
    record
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(authentication_error)
}

fn required_u32(
    record: &Map<String, Value>,
    key: &str,
) -> Result<u32, SessionBrokerCallerClientError> {
    record
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(authentication_error)
}

fn parse_caller_operations(
    value: &Value,
) -> Result<Vec<CallerOperation>, SessionBrokerCallerClientError> {
    value
        .as_array()
        .ok_or_else(authentication_error)?
        .iter()
        .map(|value| match value.as_str() {
            Some("list") => Ok(CallerOperation::List),
            Some("get") => Ok(CallerOperation::Get),
            Some("dispatch") => Ok(CallerOperation::Dispatch),
            Some("diagnostics") => Ok(CallerOperation::Diagnostics),
            Some("shutdown") => Ok(CallerOperation::Shutdown),
            Some("capability-issue") => Ok(CallerOperation::CapabilityIssue),
            _ => Err(authentication_error()),
        })
        .collect()
}

fn parse_command_scopes(
    value: &Value,
) -> Result<Vec<BrokerCommandScope>, SessionBrokerCallerClientError> {
    value
        .as_array()
        .ok_or_else(authentication_error)?
        .iter()
        .map(|value| {
            let record = exact_record(value, &["name", "version"])?;
            Ok(BrokerCommandScope {
                name: required_string(record, "name")?.into(),
                version: record
                    .get("version")
                    .and_then(Value::as_u64)
                    .ok_or_else(authentication_error)?,
            })
        })
        .collect()
}

fn random_id(crypto: &dyn SessionBrokerCrypto) -> Result<String, SessionBrokerCallerClientError> {
    let bytes = crypto
        .random_bytes(24)
        .map_err(|_| authentication_error())?;
    if bytes.len() != 24 {
        return Err(authentication_error());
    }
    Ok(format!("b_{}_0", encode_base64_url(&bytes)))
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn set_header(headers: &mut BTreeMap<String, String>, name: &str, value: &str) {
    if let Some(existing) = headers
        .keys()
        .find(|existing| existing.eq_ignore_ascii_case(name))
        .cloned()
    {
        headers.remove(&existing);
    }
    headers.insert(name.into(), value.into());
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    use ed25519_dalek::SigningKey;
    use serde_json::json;

    use super::*;
    use crate::{
        BrokerGrant, CallerRequestAuthenticationInput, CallerResponseSigningInput,
        SESSION_BROKER_SIGNATURE_ALGORITHM, SessionBrokerAuthenticator,
        SessionBrokerAuthenticatorOptions, SessionBrokerAuthorityCredential,
        SessionBrokerDaemonIdentity, SessionBrokerLimitOptions,
    };

    struct Fixture {
        authenticator: Arc<SessionBrokerAuthenticator>,
        daemon: SigningKey,
        caller: SigningKey,
        grant: CallerGrant,
    }

    fn setup() -> Fixture {
        let now = now_millis();
        let daemon = SigningKey::from_bytes(&[51; 32]);
        let caller = SigningKey::from_bytes(&[52; 32]);
        let grant = CallerGrant {
            base: crate::BrokerGrantBase {
                app_id: "dev.example".into(),
                principal_id: "caller-1".into(),
                key_id: "caller-key-1".into(),
                grant_id: "caller-grant-1".into(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: now.saturating_sub(1_000),
                expires_at: now.saturating_add(60_000),
                revocation_id: "caller-revocation-1".into(),
                may_delegate: false,
                session_id: None,
            },
            operations: vec![CallerOperation::List],
            commands: Vec::new(),
        };
        let authenticator = SessionBrokerAuthenticator::new(SessionBrokerAuthenticatorOptions {
            app_id: "dev.example".into(),
            app_revision: 7,
            generation: "generation-1".into(),
            daemon_identity: SessionBrokerDaemonIdentity {
                key_id: "daemon-key-1".into(),
                private_key: daemon.clone(),
            },
            credentials: vec![SessionBrokerAuthorityCredential {
                grant: BrokerGrant::Caller(grant.clone()),
                public_key: caller.verifying_key(),
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
        Fixture {
            authenticator: Arc::new(authenticator),
            daemon,
            caller,
            grant,
        }
    }

    fn authenticated_transport(
        authenticator: Arc<SessionBrokerAuthenticator>,
        proof_count: Arc<AtomicUsize>,
        target_specific: bool,
    ) -> Arc<dyn SessionBrokerClientHttpTransport> {
        Arc::new(move |request: SessionBrokerClientHttpRequest| {
            let url = Url::parse(&request.url).map_err(|error| error.to_string())?;
            match url.path() {
                "/session-auth/challenge" => {
                    let request_value =
                        serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
                    let challenge = authenticator
                        .issue_challenge(request_value, &request.url)
                        .map_err(|error| error.to_string())?;
                    Ok(SessionBrokerClientHttpResponse::json(
                        200,
                        &serde_json::to_value(challenge).unwrap(),
                    ))
                }
                "/session-auth/proof" => {
                    proof_count.fetch_add(1, Ordering::AcqRel);
                    let proof =
                        serde_json::from_slice(&request.body).map_err(|error| error.to_string())?;
                    let session = authenticator
                        .complete_caller_hello(proof)
                        .map_err(|error| error.to_string())?;
                    Ok(SessionBrokerClientHttpResponse::json(
                        200,
                        &serde_json::to_value(session).unwrap(),
                    ))
                }
                _ => {
                    let authenticated =
                        authenticator.authenticate(&CallerRequestAuthenticationInput {
                            method: request.method,
                            url: request.url,
                            headers: request.headers,
                            body: request.body,
                        });
                    let Ok(authenticated) = authenticated else {
                        return Ok(SessionBrokerClientHttpResponse::json(
                            401,
                            &json!({"error": "authentication-required"}),
                        ));
                    };
                    let body = json!({"sessions": []});
                    let authentication = authenticated
                        .sign_response(&CallerResponseSigningInput {
                            http_status: 200,
                            body: body.clone(),
                            app_contract: target_specific.then_some(SignedBrokerAppContract {
                                app_revision: 7,
                                features: Vec::new(),
                            }),
                        })
                        .map_err(|error| error.to_string())?;
                    Ok(SessionBrokerClientHttpResponse::json(
                        200,
                        &serde_json::to_value(SessionBrokerAuthenticatedResponse {
                            body,
                            authentication,
                        })
                        .unwrap(),
                    ))
                }
            }
        })
    }

    fn client(
        fixture: &Fixture,
        transport: Arc<dyn SessionBrokerClientHttpTransport>,
    ) -> SessionBrokerCallerClient {
        SessionBrokerCallerClient::new(SessionBrokerCallerClientOptions::native(
            "dev.example",
            7,
            "http://broker.test",
            SessionBrokerClientCredential {
                grant: fixture.grant.clone(),
                private_key: fixture.caller.clone(),
            },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: fixture.daemon.verifying_key(),
            },
            transport,
        ))
    }

    fn post() -> SessionBrokerSignedRequestInit {
        SessionBrokerSignedRequestInit {
            method: Some("POST".into()),
            body: Some("{}".into()),
            ..SessionBrokerSignedRequestInit::default()
        }
    }

    fn response_value(mut response: SessionBrokerClientHttpResponse) -> Value {
        let mut bytes = Vec::new();
        response
            .body
            .take()
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn negotiates_once_allocates_monotonic_sequences_and_verifies_responses() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let client = client(
            &fixture,
            authenticated_transport(
                Arc::clone(&fixture.authenticator),
                Arc::clone(&proofs),
                false,
            ),
        );
        for _ in 0..2 {
            let response = client.request("/control", post()).unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.body, json!({"sessions": []}));
        }
        assert_eq!(proofs.load(Ordering::Acquire), 1);
    }

    #[test]
    fn rejects_responses_replayed_across_sessions_or_sequences() {
        for (field, replacement) in [
            ("callerSessionId", "caller-session-replayed"),
            ("sequence", "2"),
        ] {
            let fixture = setup();
            let proofs = Arc::new(AtomicUsize::new(0));
            let base = authenticated_transport(
                Arc::clone(&fixture.authenticator),
                Arc::clone(&proofs),
                false,
            );
            let transport: Arc<dyn SessionBrokerClientHttpTransport> =
                Arc::new(move |request: SessionBrokerClientHttpRequest| {
                    let path = Url::parse(&request.url).unwrap().path().to_owned();
                    let response = base.send(request)?;
                    if path != "/control" {
                        return Ok(response);
                    }
                    let mut envelope = response_value(response);
                    envelope["authentication"][field] = Value::String(replacement.into());
                    Ok(SessionBrokerClientHttpResponse::json(200, &envelope))
                });
            assert!(matches!(
                client(&fixture, transport).request("/control", post()),
                Err(SessionBrokerCallerClientError::Authentication(_))
            ));
        }
    }

    #[test]
    fn requires_exact_target_specific_application_contract() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let client = client(
            &fixture,
            authenticated_transport(Arc::clone(&fixture.authenticator), proofs, true),
        );
        let mut request = post();
        request.target_specific = true;
        assert_eq!(
            client.request("/control", request).unwrap().body,
            json!({"sessions": []})
        );
    }

    #[test]
    fn rejects_unsigned_second_401_after_one_fresh_session_retry() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let base = authenticated_transport(
            Arc::clone(&fixture.authenticator),
            Arc::clone(&proofs),
            false,
        );
        let transport: Arc<dyn SessionBrokerClientHttpTransport> =
            Arc::new(move |request: SessionBrokerClientHttpRequest| {
                if Url::parse(&request.url).unwrap().path() == "/control" {
                    Ok(SessionBrokerClientHttpResponse::json(
                        401,
                        &json!({"error": "forged"}),
                    ))
                } else {
                    base.send(request)
                }
            });
        assert!(matches!(
            client(&fixture, transport).request("/control", post()),
            Err(SessionBrokerCallerClientError::Authentication(_))
        ));
        assert_eq!(proofs.load(Ordering::Acquire), 2);
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
    fn delayed_stale_401s_do_not_clear_overlapping_shared_recovery() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let base = authenticated_transport(
            Arc::clone(&fixture.authenticator),
            Arc::clone(&proofs),
            false,
        );
        let stale_mode = Arc::new(AtomicBool::new(false));
        let stale_controls = Arc::new(AtomicUsize::new(0));
        let first_401 = Arc::new(Gate::default());
        let second_401 = Arc::new(Gate::default());
        let recovery = Arc::new(Gate::default());
        let both_stale = Arc::new((Mutex::new(false), Condvar::new()));
        let recovery_started = Arc::new((Mutex::new(false), Condvar::new()));
        let transport: Arc<dyn SessionBrokerClientHttpTransport> = Arc::new({
            let proofs = Arc::clone(&proofs);
            let stale_mode = Arc::clone(&stale_mode);
            let stale_controls = Arc::clone(&stale_controls);
            let first_401 = Arc::clone(&first_401);
            let second_401 = Arc::clone(&second_401);
            let recovery = Arc::clone(&recovery);
            let both_stale = Arc::clone(&both_stale);
            let recovery_started = Arc::clone(&recovery_started);
            move |request: SessionBrokerClientHttpRequest| {
                let path = Url::parse(&request.url).unwrap().path().to_owned();
                if stale_mode.load(Ordering::Acquire)
                    && path == "/session-auth/challenge"
                    && proofs.load(Ordering::Acquire) == 1
                {
                    let (started, changed) = &*recovery_started;
                    *started.lock().unwrap() = true;
                    changed.notify_all();
                    recovery.wait();
                }
                if stale_mode.load(Ordering::Acquire)
                    && path == "/control"
                    && proofs.load(Ordering::Acquire) == 1
                {
                    let index = stale_controls.fetch_add(1, Ordering::AcqRel);
                    if index < 2 {
                        if index == 1 {
                            let (ready, changed) = &*both_stale;
                            *ready.lock().unwrap() = true;
                            changed.notify_all();
                        }
                        if index == 0 {
                            first_401.wait();
                        } else {
                            second_401.wait();
                        }
                        return Ok(SessionBrokerClientHttpResponse::json(
                            401,
                            &json!({"error": "stale-session"}),
                        ));
                    }
                }
                base.send(request)
            }
        });
        let client = client(&fixture, transport);
        client
            .request("/control", SessionBrokerSignedRequestInit::default())
            .unwrap();
        stale_mode.store(true, Ordering::Release);
        let first_client = client.clone();
        let first = std::thread::spawn(move || {
            first_client.request("/control", SessionBrokerSignedRequestInit::default())
        });
        let second_client = client.clone();
        let second = std::thread::spawn(move || {
            second_client.request("/control", SessionBrokerSignedRequestInit::default())
        });
        {
            let (ready, changed) = &*both_stale;
            let mut ready = ready.lock().unwrap();
            while !*ready {
                ready = changed.wait(ready).unwrap();
            }
        }
        first_401.open();
        {
            let (started, changed) = &*recovery_started;
            let mut started = started.lock().unwrap();
            while !*started {
                started = changed.wait(started).unwrap();
            }
        }
        second_401.open();
        recovery.open();
        assert!(first.join().unwrap().is_ok());
        assert!(second.join().unwrap().is_ok());
        assert_eq!(proofs.load(Ordering::Acquire), 2);
    }

    #[test]
    fn coalesces_concurrent_negotiations_with_unique_sequences() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let client = client(
            &fixture,
            authenticated_transport(
                Arc::clone(&fixture.authenticator),
                Arc::clone(&proofs),
                false,
            ),
        );
        let handles = (0..32)
            .map(|_| {
                let client = client.clone();
                std::thread::spawn(move || client.request("/control", post()))
            })
            .collect::<Vec<_>>();
        for handle in handles {
            assert!(handle.join().unwrap().is_ok());
        }
        assert_eq!(proofs.load(Ordering::Acquire), 1);
    }

    #[test]
    fn aborting_one_negotiation_waiter_does_not_cancel_another() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let base = authenticated_transport(
            Arc::clone(&fixture.authenticator),
            Arc::clone(&proofs),
            false,
        );
        let gate = Arc::new(Gate::default());
        let entered = Arc::new((Mutex::new(false), Condvar::new()));
        let transport: Arc<dyn SessionBrokerClientHttpTransport> = Arc::new({
            let gate = Arc::clone(&gate);
            let entered = Arc::clone(&entered);
            move |request: SessionBrokerClientHttpRequest| {
                if Url::parse(&request.url).unwrap().path() == "/session-auth/challenge" {
                    let (flag, changed) = &*entered;
                    *flag.lock().unwrap() = true;
                    changed.notify_all();
                    gate.wait();
                }
                base.send(request)
            }
        });
        let client = client(&fixture, transport);
        let cancellation = SessionBrokerCallerCancellation::default();
        let aborted_client = client.clone();
        let aborted_cancellation = cancellation.clone();
        let aborted = std::thread::spawn(move || {
            aborted_client.request(
                "/control",
                SessionBrokerSignedRequestInit {
                    cancellation: Some(aborted_cancellation),
                    ..SessionBrokerSignedRequestInit::default()
                },
            )
        });
        {
            let (flag, changed) = &*entered;
            let mut flag = flag.lock().unwrap();
            while !*flag {
                flag = changed.wait(flag).unwrap();
            }
        }
        let surviving_client = client.clone();
        let surviving = std::thread::spawn(move || {
            surviving_client.request("/control", SessionBrokerSignedRequestInit::default())
        });
        std::thread::sleep(Duration::from_millis(20));
        cancellation.cancel("caller stopped");
        gate.open();
        assert_eq!(
            aborted.join().unwrap(),
            Err(SessionBrokerCallerClientError::Cancelled(
                "caller stopped".into()
            ))
        );
        assert!(surviving.join().unwrap().is_ok());
        assert_eq!(proofs.load(Ordering::Acquire), 1);
    }

    #[test]
    fn clear_invalidates_stale_installation_and_failed_negotiations_retry() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let base = authenticated_transport(
            Arc::clone(&fixture.authenticator),
            Arc::clone(&proofs),
            false,
        );
        let challenge_count = Arc::new(AtomicUsize::new(0));
        let first_gate = Arc::new(Gate::default());
        let first_entered = Arc::new((Mutex::new(false), Condvar::new()));
        let transport: Arc<dyn SessionBrokerClientHttpTransport> = Arc::new({
            let challenge_count = Arc::clone(&challenge_count);
            let first_gate = Arc::clone(&first_gate);
            let first_entered = Arc::clone(&first_entered);
            move |request: SessionBrokerClientHttpRequest| {
                if Url::parse(&request.url).unwrap().path() == "/session-auth/challenge" {
                    let count = challenge_count.fetch_add(1, Ordering::AcqRel) + 1;
                    if count == 1 {
                        let (entered, changed) = &*first_entered;
                        *entered.lock().unwrap() = true;
                        changed.notify_all();
                        first_gate.wait();
                    }
                    if count == 2 {
                        return Ok(SessionBrokerClientHttpResponse::json(503, &json!("no")));
                    }
                }
                base.send(request)
            }
        });
        let client = client(&fixture, transport);
        let stale_client = client.clone();
        let stale = std::thread::spawn(move || {
            stale_client.request("/control", SessionBrokerSignedRequestInit::default())
        });
        {
            let (entered, changed) = &*first_entered;
            let mut entered = entered.lock().unwrap();
            while !*entered {
                entered = changed.wait(entered).unwrap();
            }
        }
        client.clear();
        assert!(
            client
                .request("/control", SessionBrokerSignedRequestInit::default())
                .is_err()
        );
        first_gate.open();
        assert!(stale.join().unwrap().is_err());
        assert!(
            client
                .request("/control", SessionBrokerSignedRequestInit::default())
                .is_ok()
        );
        assert_eq!(challenge_count.load(Ordering::Acquire), 3);
    }

    struct CancellableBody {
        cursor: Cursor<Vec<u8>>,
        cancelled: Arc<AtomicBool>,
    }

    impl Read for CancellableBody {
        fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
            self.cursor.read(buffer)
        }
    }

    impl BrokerBody for CancellableBody {
        fn cancel(&mut self) -> std::io::Result<()> {
            self.cancelled.store(true, Ordering::Release);
            Ok(())
        }
    }

    #[test]
    fn rejects_oversized_unauthenticated_challenges_before_parsing() {
        let fixture = setup();
        let cancelled = Arc::new(AtomicBool::new(false));
        let transport: Arc<dyn SessionBrokerClientHttpTransport> = Arc::new({
            let cancelled = Arc::clone(&cancelled);
            move |_| {
                Ok(SessionBrokerClientHttpResponse {
                    status: 200,
                    headers: BTreeMap::new(),
                    body: Some(Box::new(CancellableBody {
                        cursor: Cursor::new(
                            serde_json::to_vec(&json!({
                                "padding": "x".repeat(128)
                            }))
                            .unwrap(),
                        ),
                        cancelled: Arc::clone(&cancelled),
                    })),
                })
            }
        });
        let mut options = SessionBrokerCallerClientOptions::native(
            "dev.example",
            7,
            "http://broker.test",
            SessionBrokerClientCredential {
                grant: fixture.grant.clone(),
                private_key: fixture.caller.clone(),
            },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: fixture.daemon.verifying_key(),
            },
            transport,
        );
        options.max_response_bytes = 32;
        assert!(
            SessionBrokerCallerClient::new(options)
                .request("/control", SessionBrokerSignedRequestInit::default())
                .is_err()
        );
        assert!(cancelled.load(Ordering::Acquire));
    }

    #[test]
    fn cancels_malformed_or_oversized_declared_response_bodies() {
        for declared in ["invalid", "33"] {
            let fixture = setup();
            let cancelled = Arc::new(AtomicBool::new(false));
            let transport: Arc<dyn SessionBrokerClientHttpTransport> = Arc::new({
                let cancelled = Arc::clone(&cancelled);
                move |_| {
                    let mut headers = BTreeMap::new();
                    headers.insert("content-length".into(), declared.into());
                    Ok(SessionBrokerClientHttpResponse {
                        status: 200,
                        headers,
                        body: Some(Box::new(CancellableBody {
                            cursor: Cursor::new(Vec::new()),
                            cancelled: Arc::clone(&cancelled),
                        })),
                    })
                }
            });
            let mut options = SessionBrokerCallerClientOptions::native(
                "dev.example",
                7,
                "http://broker.test",
                SessionBrokerClientCredential {
                    grant: fixture.grant.clone(),
                    private_key: fixture.caller.clone(),
                },
                SessionBrokerDaemonVerifier {
                    key_id: "daemon-key-1".into(),
                    public_key: fixture.daemon.verifying_key(),
                },
                transport,
            );
            options.max_response_bytes = 32;
            assert!(
                SessionBrokerCallerClient::new(options)
                    .request("/control", SessionBrokerSignedRequestInit::default())
                    .is_err()
            );
            assert!(cancelled.load(Ordering::Acquire));
        }
    }

    #[test]
    fn rejects_challenge_records_with_unknown_or_dangerous_keys() {
        for extra in ["extra", "__proto__"] {
            let fixture = setup();
            let proofs = Arc::new(AtomicUsize::new(0));
            let base = authenticated_transport(
                Arc::clone(&fixture.authenticator),
                Arc::clone(&proofs),
                false,
            );
            let transport: Arc<dyn SessionBrokerClientHttpTransport> =
                Arc::new(move |request: SessionBrokerClientHttpRequest| {
                    let path = Url::parse(&request.url).unwrap().path().to_owned();
                    let response = base.send(request)?;
                    if path != "/session-auth/challenge" {
                        return Ok(response);
                    }
                    let mut challenge = response_value(response);
                    challenge[extra] = Value::Bool(true);
                    Ok(SessionBrokerClientHttpResponse::json(200, &challenge))
                });
            assert!(
                client(&fixture, transport)
                    .request("/control", post())
                    .is_err()
            );
            assert_eq!(proofs.load(Ordering::Acquire), 0);
        }
    }

    #[test]
    fn verifies_daemon_challenge_before_presenting_caller_proof() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let transport = authenticated_transport(
            Arc::clone(&fixture.authenticator),
            Arc::clone(&proofs),
            false,
        );
        let wrong_daemon = SigningKey::from_bytes(&[99; 32]);
        let client = SessionBrokerCallerClient::new(SessionBrokerCallerClientOptions::native(
            "dev.example",
            7,
            "http://broker.test",
            SessionBrokerClientCredential {
                grant: fixture.grant,
                private_key: fixture.caller,
            },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: wrong_daemon.verifying_key(),
            },
            transport,
        ));
        assert!(client.request("/control", post()).is_err());
        assert_eq!(proofs.load(Ordering::Acquire), 0);
    }

    #[test]
    fn refuses_cross_origin_credentials_fragments_and_unsigned_contract_variants() {
        let fixture = setup();
        let proofs = Arc::new(AtomicUsize::new(0));
        let client = client(
            &fixture,
            authenticated_transport(Arc::clone(&fixture.authenticator), proofs, false),
        );
        for path in [
            "http://attacker.test/control",
            "http://user@broker.test/control",
            "/control#fragment",
        ] {
            assert!(client.request(path, post()).is_err());
        }
    }
}
