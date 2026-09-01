//! Generation-bound daemon authentication for producer sockets and caller HTTP requests.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use ed25519_dalek::VerifyingKey;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Value, json};
use thiserror::Error;
use url::Url;

use crate::{
    BrokerChallengeTranscriptInput, BrokerCommandScope, BrokerGrant, BrokerGrantBase,
    BrokerHelloAckBinding, BrokerHelloAckTranscriptInput, BrokerHelloProposal,
    BrokerResponseTranscriptInput, BrokerRole, CallerGrant, CallerOperation, CallerPrincipal,
    CallerRequestTranscriptInput, CallerSequenceAdmission, CallerSequenceReplayWindow,
    NativeSessionBrokerCrypto, ProducerGrant, ProducerOperation, ProducerPrincipal,
    ReservationGroup, ResourceBudget, SESSION_BROKER_PROTOCOL_REVISION,
    SESSION_BROKER_SIGNATURE_ALGORITHM, SessionBrokerCrypto, SessionBrokerDaemonIdentity,
    SessionBrokerLimitOptions, SignedBrokerAppContract, build_broker_challenge_transcript,
    build_broker_hello_ack_transcript, build_broker_response_transcript,
    build_caller_request_transcript, canonical_json_bytes, decode_base64_url, encode_base64_url,
    is_grant_active, is_valid_broker_app_id, is_valid_broker_identifier, is_valid_broker_revision,
    principal_from_grant, resolve_session_broker_limits,
};

const UNIQUE_ID_RETRIES: usize = 16;
const RANDOM_ID_BYTES: usize = 24;
const MAX_ENDPOINT_LENGTH: usize = 2_048;
const CHALLENGE_RECORD_OVERHEAD_BYTES: u64 = 320;
const CALLER_SESSION_RECORD_OVERHEAD_BYTES: u64 = 384;
const CRYPTO_KEY_REFERENCE_BYTES: u64 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionBrokerAuthenticationFailureCode {
    AuthenticationRequired,
    InvalidCredential,
    CredentialExpired,
    CredentialRevoked,
    ChallengeExpired,
    ChallengeUsed,
    CallerSessionExpired,
    InvalidSignature,
    ReplayRejected,
    AuthenticationCapacity,
}

impl SessionBrokerAuthenticationFailureCode {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::AuthenticationRequired => "authentication-required",
            Self::InvalidCredential => "invalid-credential",
            Self::CredentialExpired => "credential-expired",
            Self::CredentialRevoked => "credential-revoked",
            Self::ChallengeExpired => "challenge-expired",
            Self::ChallengeUsed => "challenge-used",
            Self::CallerSessionExpired => "caller-session-expired",
            Self::InvalidSignature => "invalid-signature",
            Self::ReplayRejected => "replay-rejected",
            Self::AuthenticationCapacity => "authentication-capacity",
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Session broker authentication failed.")]
pub struct SessionBrokerAuthenticationError {
    pub code: SessionBrokerAuthenticationFailureCode,
}

fn auth_error(code: SessionBrokerAuthenticationFailureCode) -> SessionBrokerAuthenticationError {
    SessionBrokerAuthenticationError { code }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Invalid session broker authenticator configuration: {0}")]
pub struct SessionBrokerAuthenticatorConfigError(pub String);

#[derive(Debug, Clone)]
pub struct SessionBrokerAuthorityCredential {
    pub grant: BrokerGrant,
    pub public_key: VerifyingKey,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionBrokerHelloRole {
    Producer,
    Caller,
}

impl SessionBrokerHelloRole {
    fn broker_role(self) -> BrokerRole {
        match self {
            Self::Producer => BrokerRole::Producer,
            Self::Caller => BrokerRole::Caller,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerHelloProposalWire {
    pub broker_revision: u32,
    pub app_revision: u32,
    pub features: Vec<String>,
}

impl From<&SessionBrokerHelloProposalWire> for BrokerHelloProposal {
    fn from(value: &SessionBrokerHelloProposalWire) -> Self {
        Self {
            broker_revision: value.broker_revision,
            app_revision: value.app_revision,
            features: value.features.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerHelloChallengeRequest {
    pub role: SessionBrokerHelloRole,
    pub app_id: String,
    pub endpoint: String,
    pub key_id: String,
    pub grant_id: String,
    pub initiator_nonce: String,
    pub proposal: SessionBrokerHelloProposalWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerHelloChallenge {
    pub challenge_id: String,
    pub generation: String,
    pub responder_nonce: String,
    pub expires_at: u64,
    pub daemon_key_id: String,
    pub daemon_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionBrokerHelloProof {
    pub challenge_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatedCallerSession {
    pub caller_session_id: String,
    #[serde(serialize_with = "serialize_caller_principal")]
    pub principal: CallerPrincipal,
    pub expires_at: u64,
    pub initial_sequence: String,
    pub broker_revision: u32,
    pub app_revision: u32,
    pub features: Vec<String>,
    pub hello_transcript_hash: String,
    pub daemon_key_id: String,
    pub daemon_signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionBrokerProducerHelloAck {
    #[serde(serialize_with = "serialize_producer_principal")]
    pub principal: ProducerPrincipal,
    pub connection_id: String,
    pub broker_revision: u32,
    pub app_revision: u32,
    pub features: Vec<String>,
    pub hello_transcript_hash: String,
    pub daemon_key_id: String,
    pub daemon_signature: String,
}

fn serialize_caller_principal<S>(
    principal: &CallerPrincipal,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let fields = 7 + usize::from(principal.session_id.is_some());
    let mut record = serializer.serialize_struct("CallerPrincipal", fields)?;
    record.serialize_field("kind", "caller")?;
    record.serialize_field("appId", &principal.app_id)?;
    record.serialize_field("principalId", &principal.principal_id)?;
    record.serialize_field("keyId", &principal.key_id)?;
    record.serialize_field("grantId", &principal.grant_id)?;
    if let Some(session_id) = &principal.session_id {
        record.serialize_field("sessionId", session_id)?;
    }
    let operations = principal
        .operations
        .iter()
        .copied()
        .map(caller_operation)
        .collect::<Vec<_>>();
    record.serialize_field("operations", &operations)?;
    let commands = principal
        .commands
        .iter()
        .map(|scope| json!({"name": scope.name, "version": scope.version}))
        .collect::<Vec<_>>();
    record.serialize_field("commands", &commands)?;
    record.end()
}

fn serialize_producer_principal<S>(
    principal: &ProducerPrincipal,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let fields = 6 + usize::from(principal.session_id.is_some());
    let mut record = serializer.serialize_struct("ProducerPrincipal", fields)?;
    record.serialize_field("kind", "producer")?;
    record.serialize_field("appId", &principal.app_id)?;
    record.serialize_field("principalId", &principal.principal_id)?;
    record.serialize_field("keyId", &principal.key_id)?;
    record.serialize_field("grantId", &principal.grant_id)?;
    if let Some(session_id) = &principal.session_id {
        record.serialize_field("sessionId", session_id)?;
    }
    let scopes = principal
        .scopes
        .iter()
        .copied()
        .map(producer_operation)
        .collect::<Vec<_>>();
    record.serialize_field("scopes", &scopes)?;
    record.end()
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallerRequestAuthenticationInput {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CallerResponseSigningInput {
    pub http_status: u16,
    pub body: Value,
    pub app_contract: Option<SignedBrokerAppContract>,
}

pub type SessionBrokerClock = Arc<dyn Fn() -> u64 + Send + Sync>;
pub type SessionBrokerRevocationCheck = Arc<dyn Fn(&str) -> bool + Send + Sync>;

pub struct SessionBrokerAuthenticatorOptions {
    pub app_id: String,
    pub app_revision: u32,
    pub generation: String,
    pub daemon_identity: SessionBrokerDaemonIdentity,
    pub credentials: Vec<SessionBrokerAuthorityCredential>,
    pub crypto: Option<Arc<dyn SessionBrokerCrypto>>,
    pub now: Option<SessionBrokerClock>,
    pub is_revoked: Option<SessionBrokerRevocationCheck>,
    pub challenge_ttl_ms: Option<u64>,
    pub caller_session_ttl_ms: Option<u64>,
    pub max_challenges: Option<u64>,
    pub max_challenge_bytes: Option<u64>,
    pub max_challenge_transcript_bytes: Option<u64>,
    pub max_caller_sessions: Option<u64>,
    pub limits: SessionBrokerLimitOptions,
}

struct AuthenticatorConfig {
    app_id: String,
    app_revision: u32,
    generation: String,
    daemon_identity: SessionBrokerDaemonIdentity,
    now: SessionBrokerClock,
    is_revoked: Option<SessionBrokerRevocationCheck>,
    challenge_ttl_ms: u64,
    caller_session_ttl_ms: u64,
    max_challenge_transcript_bytes: u64,
    max_caller_session_bytes: u64,
}

struct PendingChallenge {
    request: SessionBrokerHelloChallengeRequest,
    transcript: Vec<u8>,
    grant: BrokerGrant,
    public_key: VerifyingKey,
    expires_at: u64,
    _reservation: ReservationGroup,
}

struct CallerSessionRecord {
    principal: CallerPrincipal,
    grant: CallerGrant,
    public_key: VerifyingKey,
    hello_transcript_hash: String,
    expires_at: u64,
    replay: CallerSequenceReplayWindow,
    _reservation: ReservationGroup,
}

struct AuthenticatorState {
    credentials: BTreeMap<String, SessionBrokerAuthorityCredential>,
    challenges: BTreeMap<String, PendingChallenge>,
    caller_sessions: BTreeMap<String, CallerSessionRecord>,
    reserved_challenge_ids: BTreeSet<String>,
    reserved_caller_session_ids: BTreeSet<String>,
    clear_epoch: u64,
}

struct AuthenticatorShared {
    config: AuthenticatorConfig,
    crypto: Arc<dyn SessionBrokerCrypto>,
    state: Mutex<AuthenticatorState>,
    challenge_count_budget: ResourceBudget,
    challenge_byte_budget: ResourceBudget,
    caller_session_count_budget: ResourceBudget,
    caller_session_byte_budget: ResourceBudget,
}

#[derive(Clone)]
pub struct SessionBrokerAuthenticator {
    shared: Arc<AuthenticatorShared>,
}

pub struct AuthenticatedProducerHello {
    pub ack: SessionBrokerProducerHelloAck,
    shared: Option<Arc<AuthenticatorShared>>,
    epoch: u64,
    grant: Option<ProducerGrant>,
    custom_assert_active:
        Option<Arc<dyn Fn() -> Result<(), SessionBrokerAuthenticationError> + Send + Sync>>,
}

type CallerActiveAssertion =
    Arc<dyn Fn() -> Result<(), SessionBrokerAuthenticationError> + Send + Sync>;
type CallerResponseSigner = Arc<
    dyn Fn(
            &CallerResponseSigningInput,
        )
            -> Result<crate::SessionBrokerResponseAuthentication, SessionBrokerAuthenticationError>
        + Send
        + Sync,
>;

impl std::fmt::Debug for AuthenticatedProducerHello {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthenticatedProducerHello")
            .field("ack", &self.ack)
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}

impl AuthenticatedProducerHello {
    /// Compose a runtime-owned producer authority around an application callback.
    ///
    /// Native authenticators use the generation-bound grant state below. Runtime adapters and
    /// parity fixtures can supply the same fail-closed assertion contract without manufacturing
    /// private authenticator state.
    #[must_use]
    pub fn from_assertion(
        ack: SessionBrokerProducerHelloAck,
        assert_active: Arc<dyn Fn() -> Result<(), SessionBrokerAuthenticationError> + Send + Sync>,
    ) -> Self {
        Self {
            ack,
            shared: None,
            epoch: 0,
            grant: None,
            custom_assert_active: Some(assert_active),
        }
    }

    pub fn assert_active(&self) -> Result<(), SessionBrokerAuthenticationError> {
        if let Some(assert_active) = &self.custom_assert_active {
            return assert_active();
        }
        let shared = self
            .shared
            .as_ref()
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        let grant = self
            .grant
            .as_ref()
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        shared.assert_epoch(self.epoch)?;
        shared.require_active_grant(&BrokerGrant::Producer(grant.clone()))
    }
}

pub struct AuthenticatedCallerRequest {
    pub principal: CallerPrincipal,
    pub request_id: String,
    caller_session_id: Option<String>,
    sequence: Option<String>,
    hello_transcript_hash: Option<String>,
    shared: Option<Arc<AuthenticatorShared>>,
    epoch: u64,
    custom_assert_active: Option<CallerActiveAssertion>,
    custom_sign_response: Option<CallerResponseSigner>,
}

pub trait CallerRequestAuthenticator: Send + Sync {
    fn authenticate_request(
        &self,
        input: &CallerRequestAuthenticationInput,
    ) -> Result<AuthenticatedCallerRequest, SessionBrokerAuthenticationError>;
    fn clear_authentication(&self);
}

pub trait SessionBrokerHelloAuthenticator: Send + Sync {
    fn issue_hello_challenge(
        &self,
        request: Value,
        listener_endpoint: &str,
    ) -> Result<SessionBrokerHelloChallenge, SessionBrokerAuthenticationError>;
    fn complete_caller_hello_proof(
        &self,
        proof: Value,
    ) -> Result<AuthenticatedCallerSession, SessionBrokerAuthenticationError>;
    fn complete_producer_hello_proof(
        &self,
        proof: Value,
        connection_id: &str,
    ) -> Result<AuthenticatedProducerHello, SessionBrokerAuthenticationError>;
}

impl std::fmt::Debug for AuthenticatedCallerRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AuthenticatedCallerRequest")
            .field("principal", &self.principal)
            .field("request_id", &self.request_id)
            .field("caller_session_id", &self.caller_session_id)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}

impl AuthenticatedCallerRequest {
    /// Compose an authenticated caller supplied by a native transport or application host.
    #[must_use]
    pub fn from_callbacks(
        principal: CallerPrincipal,
        request_id: impl Into<String>,
        assert_active: CallerActiveAssertion,
        sign_response: CallerResponseSigner,
    ) -> Self {
        Self {
            principal,
            request_id: request_id.into(),
            caller_session_id: None,
            sequence: None,
            hello_transcript_hash: None,
            shared: None,
            epoch: 0,
            custom_assert_active: Some(assert_active),
            custom_sign_response: Some(sign_response),
        }
    }

    pub fn assert_active(&self) -> Result<(), SessionBrokerAuthenticationError> {
        if let Some(assert_active) = &self.custom_assert_active {
            return assert_active();
        }
        let shared = self
            .shared
            .as_ref()
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        shared.assert_caller_active(
            self.epoch,
            self.caller_session_id.as_deref().ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
            })?,
            self.hello_transcript_hash.as_deref().ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
            })?,
        )
    }

    pub fn sign_response(
        &self,
        input: &CallerResponseSigningInput,
    ) -> Result<crate::SessionBrokerResponseAuthentication, SessionBrokerAuthenticationError> {
        if let Some(sign_response) = &self.custom_sign_response {
            self.assert_active()?;
            return sign_response(input);
        }
        self.assert_active()?;
        let shared = self
            .shared
            .as_ref()
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        if !(100..=599).contains(&input.http_status) {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        if input.app_contract.as_ref().is_some_and(|contract| {
            contract.app_revision != shared.config.app_revision || !contract.features.is_empty()
        }) {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        let app_contract = input
            .app_contract
            .as_ref()
            .map(|_| SignedBrokerAppContract {
                app_revision: shared.config.app_revision,
                features: Vec::new(),
            });
        let body = canonical_json_bytes(&input.body)
            .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        let body_digest = encode_base64_url(&shared.crypto.sha256(&body));
        self.assert_active()?;
        let transcript = build_broker_response_transcript(&BrokerResponseTranscriptInput {
            app_id: shared.config.app_id.clone(),
            generation: shared.config.generation.clone(),
            broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
            caller_session_id: self.caller_session_id.clone().ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
            })?,
            request_id: self.request_id.clone(),
            sequence: self.sequence.clone().ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
            })?,
            http_status: input.http_status,
            body_digest: body_digest.clone(),
            app_contract: app_contract.clone(),
        })
        .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        let daemon_signature = encode_base64_url(
            &self
                .shared
                .as_ref()
                .ok_or_else(|| {
                    auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
                })?
                .crypto
                .sign(&shared.config.daemon_identity.private_key, &transcript),
        );
        self.assert_active()?;
        Ok(crate::SessionBrokerResponseAuthentication {
            generation: shared.config.generation.clone(),
            broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
            app_contract: app_contract.as_ref().map(Into::into),
            caller_session_id: self.caller_session_id.clone().ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
            })?,
            request_id: self.request_id.clone(),
            sequence: self.sequence.clone().ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
            })?,
            http_status: input.http_status,
            body_digest,
            daemon_key_id: shared.config.daemon_identity.key_id.clone(),
            daemon_signature,
        })
    }
}

fn grant_base(grant: &BrokerGrant) -> &BrokerGrantBase {
    match grant {
        BrokerGrant::Producer(grant) => &grant.base,
        BrokerGrant::Caller(grant) => &grant.base,
    }
}

fn grant_role(grant: &BrokerGrant) -> SessionBrokerHelloRole {
    match grant {
        BrokerGrant::Producer(_) => SessionBrokerHelloRole::Producer,
        BrokerGrant::Caller(_) => SessionBrokerHelloRole::Caller,
    }
}

fn producer_operation(operation: ProducerOperation) -> &'static str {
    match operation {
        ProducerOperation::Register => "register",
        ProducerOperation::Reconnect => "reconnect",
    }
}

fn caller_operation(operation: CallerOperation) -> &'static str {
    match operation {
        CallerOperation::List => "list",
        CallerOperation::Get => "get",
        CallerOperation::Dispatch => "dispatch",
        CallerOperation::Diagnostics => "diagnostics",
        CallerOperation::Shutdown => "shutdown",
        CallerOperation::CapabilityIssue => "capability:issue",
    }
}

fn base_value(base: &BrokerGrantBase) -> serde_json::Map<String, Value> {
    let mut value = serde_json::Map::new();
    value.insert("appId".into(), base.app_id.clone().into());
    value.insert("principalId".into(), base.principal_id.clone().into());
    value.insert("keyId".into(), base.key_id.clone().into());
    value.insert("grantId".into(), base.grant_id.clone().into());
    value.insert("algorithm".into(), base.algorithm.clone().into());
    value.insert("issuedAt".into(), base.issued_at.into());
    value.insert("expiresAt".into(), base.expires_at.into());
    value.insert("revocationId".into(), base.revocation_id.clone().into());
    value.insert("mayDelegate".into(), base.may_delegate.into());
    if let Some(session_id) = &base.session_id {
        value.insert("sessionId".into(), session_id.clone().into());
    }
    value
}

fn grant_value(grant: &BrokerGrant) -> Value {
    let mut value = base_value(grant_base(grant));
    match grant {
        BrokerGrant::Producer(grant) => {
            value.insert("kind".into(), "producer".into());
            value.insert(
                "operations".into(),
                grant
                    .operations
                    .iter()
                    .copied()
                    .map(producer_operation)
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
        BrokerGrant::Caller(grant) => {
            value.insert("kind".into(), "caller".into());
            value.insert(
                "operations".into(),
                grant
                    .operations
                    .iter()
                    .copied()
                    .map(caller_operation)
                    .collect::<Vec<_>>()
                    .into(),
            );
            value.insert(
                "commands".into(),
                grant
                    .commands
                    .iter()
                    .map(|scope| json!({"name": scope.name, "version": scope.version}))
                    .collect::<Vec<_>>()
                    .into(),
            );
        }
    }
    Value::Object(value)
}

fn caller_principal_value(principal: &CallerPrincipal) -> Value {
    let mut value = serde_json::Map::new();
    value.insert("kind".into(), "caller".into());
    value.insert("appId".into(), principal.app_id.clone().into());
    value.insert("principalId".into(), principal.principal_id.clone().into());
    value.insert("keyId".into(), principal.key_id.clone().into());
    value.insert("grantId".into(), principal.grant_id.clone().into());
    if let Some(session_id) = &principal.session_id {
        value.insert("sessionId".into(), session_id.clone().into());
    }
    value.insert(
        "operations".into(),
        principal
            .operations
            .iter()
            .copied()
            .map(caller_operation)
            .collect::<Vec<_>>()
            .into(),
    );
    value.insert(
        "commands".into(),
        principal
            .commands
            .iter()
            .map(|scope| json!({"name": scope.name, "version": scope.version}))
            .collect::<Vec<_>>()
            .into(),
    );
    Value::Object(value)
}

fn retained_record_bytes(
    values: &[Value],
    binary: &[u8],
    overhead: u64,
) -> Result<u64, SessionBrokerAuthenticationError> {
    let mut total = overhead
        .checked_add(CRYPTO_KEY_REFERENCE_BYTES)
        .and_then(|value| value.checked_add(binary.len() as u64))
        .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
    for value in values {
        total = total
            .checked_add(
                serde_json::to_vec(value)
                    .map_err(|_| {
                        auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
                    })?
                    .len() as u64,
            )
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
    }
    Ok(total)
}

fn credential_key(role: SessionBrokerHelloRole, key_id: &str, grant_id: &str) -> String {
    let role = match role {
        SessionBrokerHelloRole::Producer => "producer",
        SessionBrokerHelloRole::Caller => "caller",
    };
    format!("{role}:{key_id}:{grant_id}")
}

fn validate_grant(
    grant: &BrokerGrant,
    app_id: &str,
) -> Result<(), SessionBrokerAuthenticatorConfigError> {
    let base = grant_base(grant);
    if !is_valid_broker_app_id(&base.app_id) || base.app_id != app_id {
        return Err(SessionBrokerAuthenticatorConfigError(
            "every credential must exactly match the configured appId.".into(),
        ));
    }
    for (name, value) in [
        ("principalId", base.principal_id.as_str()),
        ("keyId", base.key_id.as_str()),
        ("grantId", base.grant_id.as_str()),
        ("revocationId", base.revocation_id.as_str()),
    ] {
        if !is_valid_broker_identifier(value) {
            return Err(SessionBrokerAuthenticatorConfigError(format!(
                "{name} has an invalid identifier."
            )));
        }
    }
    if base
        .session_id
        .as_deref()
        .is_some_and(|value| !is_valid_broker_identifier(value))
    {
        return Err(SessionBrokerAuthenticatorConfigError(
            "sessionId has an invalid identifier.".into(),
        ));
    }
    if base.algorithm != SESSION_BROKER_SIGNATURE_ALGORITHM {
        return Err(SessionBrokerAuthenticatorConfigError(
            "credential algorithm must be Ed25519.".into(),
        ));
    }
    if base.issued_at >= base.expires_at {
        return Err(SessionBrokerAuthenticatorConfigError(
            "credential timestamps must be finite and strictly ordered.".into(),
        ));
    }
    match grant {
        BrokerGrant::Producer(grant) => {
            let unique = grant.operations.iter().copied().collect::<BTreeSet<_>>();
            if unique.len() != grant.operations.len() {
                return Err(SessionBrokerAuthenticatorConfigError(
                    "credential operations must be recognized and unique.".into(),
                ));
            }
        }
        BrokerGrant::Caller(grant) => {
            let unique = grant.operations.iter().copied().collect::<BTreeSet<_>>();
            if unique.len() != grant.operations.len() {
                return Err(SessionBrokerAuthenticatorConfigError(
                    "credential operations must be recognized and unique.".into(),
                ));
            }
            if grant.commands.len() > crate::MAX_BROKER_COMMAND_SCOPES {
                return Err(SessionBrokerAuthenticatorConfigError(
                    "caller command scopes exceed the configured bound.".into(),
                ));
            }
            let mut keys = BTreeSet::new();
            for BrokerCommandScope { name, version } in &grant.commands {
                if !is_valid_broker_identifier(name) || !is_valid_broker_revision(*version) {
                    return Err(SessionBrokerAuthenticatorConfigError(
                        "caller command scope is invalid.".into(),
                    ));
                }
                if !keys.insert((name.clone(), *version)) {
                    return Err(SessionBrokerAuthenticatorConfigError(
                        "caller command scopes must be unique.".into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn lower_only(
    selected: Option<u64>,
    fallback: u64,
    name: &str,
    allow_zero: bool,
) -> Result<u64, SessionBrokerAuthenticatorConfigError> {
    let selected = selected.unwrap_or(fallback);
    if (!allow_zero && selected == 0) || selected > fallback {
        let message = if selected > fallback {
            format!("{name} may only be raised through unsafeLimits.")
        } else {
            format!("{name} must be a positive safe integer.")
        };
        return Err(SessionBrokerAuthenticatorConfigError(message));
    }
    Ok(selected)
}

fn parse_endpoint(value: &str, http_only: bool) -> Option<Url> {
    if value.is_empty() || value.len() > MAX_ENDPOINT_LENGTH {
        return None;
    }
    let url = Url::parse(value).ok()?;
    let valid_scheme = if http_only {
        matches!(url.scheme(), "http" | "https")
    } else {
        matches!(url.scheme(), "http" | "https" | "ws" | "wss")
    };
    if !valid_scheme
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || canonical_http_target(&url).is_err()
    {
        return None;
    }
    Some(url)
}

fn percent_decode(value: &str, plus_as_space: bool) -> Result<String, ()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'+' if plus_as_space => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(());
                }
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).map_err(|_| ())?;
                decoded.push(u8::from_str_radix(hex, 16).map_err(|_| ())?);
                index += 3;
            }
            byte if byte.is_ascii() => {
                decoded.push(byte);
                index += 1;
            }
            _ => {
                let character = value[index..].chars().next().ok_or(())?;
                let mut buffer = [0; 4];
                decoded.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
                index += character.len_utf8();
            }
        }
    }
    String::from_utf8(decoded).map_err(|_| ())
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(*byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

/// Produce the canonical path and RFC 3986 encoded sorted query covered by signatures.
pub fn canonical_http_target(url: &Url) -> Result<String, SessionBrokerAuthenticationError> {
    let path = url
        .path()
        .split('/')
        .map(|segment| percent_decode(segment, false).map(|value| percent_encode(&value)))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidSignature))?
        .join("/");
    let mut pairs = Vec::new();
    if let Some(query) = url.query() {
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            pairs.push((
                percent_encode(&percent_decode(key, true).map_err(|_| {
                    auth_error(SessionBrokerAuthenticationFailureCode::InvalidSignature)
                })?),
                percent_encode(&percent_decode(value, true).map_err(|_| {
                    auth_error(SessionBrokerAuthenticationFailureCode::InvalidSignature)
                })?),
            ));
        }
    }
    pairs.sort();
    if pairs.is_empty() {
        Ok(path)
    } else {
        Ok(format!(
            "{path}?{}",
            pairs
                .into_iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("&")
        ))
    }
}

impl AuthenticatorShared {
    fn now(&self) -> u64 {
        (self.config.now)()
    }

    fn assert_epoch(&self, epoch: u64) -> Result<(), SessionBrokerAuthenticationError> {
        let state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.clear_epoch != epoch {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        Ok(())
    }

    fn require_active_grant(
        &self,
        grant: &BrokerGrant,
    ) -> Result<(), SessionBrokerAuthenticationError> {
        let base = grant_base(grant);
        if self
            .config
            .is_revoked
            .as_ref()
            .is_some_and(|is_revoked| is_revoked(&base.revocation_id))
        {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::CredentialRevoked,
            ));
        }
        if !is_grant_active(grant, &self.config.app_id, self.now(), |_| false) {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::CredentialExpired,
            ));
        }
        Ok(())
    }

    fn assert_caller_active(
        &self,
        epoch: u64,
        caller_session_id: &str,
        hello_transcript_hash: &str,
    ) -> Result<(), SessionBrokerAuthenticationError> {
        let now = self.now();
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.clear_epoch != epoch {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        let Some(session) = state.caller_sessions.get(caller_session_id) else {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::CallerSessionExpired,
            ));
        };
        if session.hello_transcript_hash != hello_transcript_hash {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::CallerSessionExpired,
            ));
        }
        let grant = BrokerGrant::Caller(session.grant.clone());
        let expires_at = session.expires_at;
        if let Err(error) = self.require_active_grant(&grant) {
            state.caller_sessions.remove(caller_session_id);
            return Err(error);
        }
        if now >= expires_at {
            state.caller_sessions.remove(caller_session_id);
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::CallerSessionExpired,
            ));
        }
        Ok(())
    }

    fn random_id(&self) -> Result<String, SessionBrokerAuthenticationError> {
        let bytes = self
            .crypto
            .random_bytes(RANDOM_ID_BYTES)
            .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        if bytes.len() != RANDOM_ID_BYTES {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        Ok(format!("b_{}_0", encode_base64_url(&bytes)))
    }

    fn unique_id(
        &self,
        state: &AuthenticatorState,
        caller: bool,
    ) -> Result<String, SessionBrokerAuthenticationError> {
        for _ in 0..UNIQUE_ID_RETRIES {
            let id = self.random_id()?;
            let occupied = if caller {
                state.caller_sessions.contains_key(&id)
                    || state.reserved_caller_session_ids.contains(&id)
            } else {
                state.challenges.contains_key(&id) || state.reserved_challenge_ids.contains(&id)
            };
            if !occupied {
                return Ok(id);
            }
        }
        Err(auth_error(
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity,
        ))
    }

    fn prune_expired(&self, state: &mut AuthenticatorState, now: u64) {
        state
            .challenges
            .retain(|_, challenge| now < challenge.expires_at);
        state
            .caller_sessions
            .retain(|_, session| now < session.expires_at);
    }
}

impl SessionBrokerAuthenticator {
    pub fn new(
        options: SessionBrokerAuthenticatorOptions,
    ) -> Result<Self, SessionBrokerAuthenticatorConfigError> {
        if !is_valid_broker_app_id(&options.app_id) {
            return Err(SessionBrokerAuthenticatorConfigError(
                "appId has an invalid grammar.".into(),
            ));
        }
        if !is_valid_broker_revision(u64::from(options.app_revision)) {
            return Err(SessionBrokerAuthenticatorConfigError(
                "appRevision must be a positive safe integer.".into(),
            ));
        }
        if !is_valid_broker_identifier(&options.generation) {
            return Err(SessionBrokerAuthenticatorConfigError(
                "generation has an invalid identifier.".into(),
            ));
        }
        if !is_valid_broker_identifier(&options.daemon_identity.key_id) {
            return Err(SessionBrokerAuthenticatorConfigError(
                "daemon keyId has an invalid identifier.".into(),
            ));
        }
        let limits = resolve_session_broker_limits(&options.limits)
            .map_err(|error| SessionBrokerAuthenticatorConfigError(error.to_string()))?;
        let retained_caller_capacity = limits
            .max_caller_sessions_bytes
            .checked_div(limits.max_caller_session_bytes)
            .unwrap_or(0);
        let max_caller_sessions = limits.max_caller_sessions.min(retained_caller_capacity);
        let challenge_ttl_ms = lower_only(
            options.challenge_ttl_ms,
            limits.challenge_ttl_ms,
            "challengeTtlMs",
            false,
        )?;
        let caller_session_ttl_ms = lower_only(
            options.caller_session_ttl_ms,
            limits.caller_session_ttl_ms,
            "callerSessionTtlMs",
            false,
        )?;
        let max_challenges = lower_only(
            options.max_challenges,
            limits.max_incomplete_handshakes,
            "maxChallenges",
            true,
        )?;
        let max_challenge_bytes = lower_only(
            options.max_challenge_bytes,
            limits.max_incomplete_handshake_bytes,
            "maxChallengeBytes",
            true,
        )?;
        let max_challenge_transcript_bytes = lower_only(
            options.max_challenge_transcript_bytes,
            limits.max_handshake_proposal_bytes,
            "maxChallengeTranscriptBytes",
            true,
        )?;
        let max_caller_sessions = lower_only(
            options.max_caller_sessions,
            max_caller_sessions,
            "maxCallerSessions",
            true,
        )?;
        let mut credentials = BTreeMap::new();
        for credential in options.credentials {
            validate_grant(&credential.grant, &options.app_id)?;
            let base = grant_base(&credential.grant);
            let key = credential_key(grant_role(&credential.grant), &base.key_id, &base.grant_id);
            if credentials.insert(key, credential).is_some() {
                return Err(SessionBrokerAuthenticatorConfigError(
                    "credential identities must be unique.".into(),
                ));
            }
        }
        let config = AuthenticatorConfig {
            app_id: options.app_id,
            app_revision: options.app_revision,
            generation: options.generation,
            daemon_identity: options.daemon_identity,
            now: options.now.unwrap_or_else(|| {
                Arc::new(|| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis()
                        .try_into()
                        .unwrap_or(u64::MAX)
                })
            }),
            is_revoked: options.is_revoked,
            challenge_ttl_ms,
            caller_session_ttl_ms,
            max_challenge_transcript_bytes,
            max_caller_session_bytes: limits.max_caller_session_bytes,
        };
        Ok(Self {
            shared: Arc::new(AuthenticatorShared {
                config,
                crypto: options
                    .crypto
                    .unwrap_or_else(|| Arc::new(NativeSessionBrokerCrypto)),
                state: Mutex::new(AuthenticatorState {
                    credentials,
                    challenges: BTreeMap::new(),
                    caller_sessions: BTreeMap::new(),
                    reserved_challenge_ids: BTreeSet::new(),
                    reserved_caller_session_ids: BTreeSet::new(),
                    clear_epoch: 0,
                }),
                challenge_count_budget: ResourceBudget::new(
                    max_challenges,
                    "maxIncompleteHandshakes",
                ),
                challenge_byte_budget: ResourceBudget::new(max_challenge_bytes, "challengeBytes"),
                caller_session_count_budget: ResourceBudget::new(
                    max_caller_sessions,
                    "maxCallerSessions",
                ),
                caller_session_byte_budget: ResourceBudget::new(
                    limits.max_caller_sessions_bytes,
                    "maxCallerSessionsBytes",
                ),
            }),
        })
    }

    pub fn issue_challenge(
        &self,
        request: Value,
        listener_endpoint: &str,
    ) -> Result<SessionBrokerHelloChallenge, SessionBrokerAuthenticationError> {
        let normalized: SessionBrokerHelloChallengeRequest = serde_json::from_value(request)
            .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        if normalized.app_id != self.shared.config.app_id
            || normalized.endpoint != listener_endpoint
            || parse_endpoint(&normalized.endpoint, false).is_none()
            || parse_endpoint(listener_endpoint, false).is_none()
            || normalized.proposal.broker_revision != SESSION_BROKER_PROTOCOL_REVISION
            || normalized.proposal.app_revision != self.shared.config.app_revision
            || !normalized.proposal.features.is_empty()
            || !is_valid_broker_identifier(&normalized.key_id)
            || !is_valid_broker_identifier(&normalized.grant_id)
            || !is_valid_broker_identifier(&normalized.initiator_nonce)
        {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        let now = self.shared.now();
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.shared.prune_expired(&mut state, now);
        let key = credential_key(normalized.role, &normalized.key_id, &normalized.grant_id);
        let credential =
            state.credentials.get(&key).cloned().ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
            })?;
        if grant_role(&credential.grant) != normalized.role {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        self.shared.require_active_grant(&credential.grant)?;
        let challenge_id = self.shared.unique_id(&state, false)?;
        let responder_nonce = self.shared.random_id()?;
        let expires_at = now
            .checked_add(self.shared.config.challenge_ttl_ms)
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        let transcript = build_broker_challenge_transcript(&BrokerChallengeTranscriptInput {
            role: normalized.role.broker_role(),
            app_id: normalized.app_id.clone(),
            generation: self.shared.config.generation.clone(),
            endpoint: normalized.endpoint.clone(),
            key_id: normalized.key_id.clone(),
            grant_id: normalized.grant_id.clone(),
            initiator_nonce: normalized.initiator_nonce.clone(),
            responder_nonce: responder_nonce.clone(),
            proposal: (&normalized.proposal).into(),
        })
        .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        let retained_bytes = retained_record_bytes(
            &[
                Value::String(challenge_id.clone()),
                serde_json::to_value(&normalized).map_err(|_| {
                    auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
                })?,
                grant_value(&credential.grant),
                Value::from(expires_at),
            ],
            &transcript,
            CHALLENGE_RECORD_OVERHEAD_BYTES,
        )?;
        if retained_bytes > self.shared.config.max_challenge_transcript_bytes {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::AuthenticationCapacity,
            ));
        }
        let mut reservation = ReservationGroup::default();
        let count = self.shared.challenge_count_budget.reserve(1).map_err(|_| {
            auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
        })?;
        reservation.add(count).map_err(|_| {
            auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
        })?;
        let bytes = self
            .shared
            .challenge_byte_budget
            .reserve(retained_bytes)
            .map_err(|_| {
                auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
            })?;
        reservation.add(bytes).map_err(|_| {
            auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
        })?;
        let epoch = state.clear_epoch;
        state.reserved_challenge_ids.insert(challenge_id.clone());
        drop(state);
        let daemon_signature = encode_base64_url(
            &self
                .shared
                .crypto
                .sign(&self.shared.config.daemon_identity.private_key, &transcript),
        );
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.reserved_challenge_ids.remove(&challenge_id);
        if state.clear_epoch != epoch {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        state.challenges.insert(
            challenge_id.clone(),
            PendingChallenge {
                request: normalized,
                transcript,
                grant: credential.grant,
                public_key: credential.public_key,
                expires_at,
                _reservation: reservation,
            },
        );
        Ok(SessionBrokerHelloChallenge {
            challenge_id,
            generation: self.shared.config.generation.clone(),
            responder_nonce,
            expires_at,
            daemon_key_id: self.shared.config.daemon_identity.key_id.clone(),
            daemon_signature,
        })
    }

    fn take_challenge(
        &self,
        proof: &SessionBrokerHelloProof,
        role: SessionBrokerHelloRole,
    ) -> Result<PendingChallenge, SessionBrokerAuthenticationError> {
        if !is_valid_broker_identifier(&proof.challenge_id) {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::ChallengeUsed,
            ));
        }
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let pending = state
            .challenges
            .remove(&proof.challenge_id)
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::ChallengeUsed))?;
        if self.shared.now() >= pending.expires_at {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::ChallengeExpired,
            ));
        }
        if grant_role(&pending.grant) != role {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        Ok(pending)
    }

    fn parse_proof(
        proof: Value,
    ) -> Result<SessionBrokerHelloProof, SessionBrokerAuthenticationError> {
        let proof: SessionBrokerHelloProof = serde_json::from_value(proof)
            .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        if !is_valid_broker_identifier(&proof.challenge_id) || proof.signature.len() > 1_024 {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        Ok(proof)
    }

    fn verify_proof(
        &self,
        pending: &PendingChallenge,
        signature: &str,
    ) -> Result<(), SessionBrokerAuthenticationError> {
        let signature = decode_base64_url(signature).filter(|bytes| !bytes.is_empty());
        if signature.as_deref().is_none_or(|signature| {
            !self
                .shared
                .crypto
                .verify(&pending.public_key, signature, &pending.transcript)
        }) {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidSignature,
            ));
        }
        Ok(())
    }

    pub fn complete_caller_hello(
        &self,
        proof: Value,
    ) -> Result<AuthenticatedCallerSession, SessionBrokerAuthenticationError> {
        let epoch = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear_epoch;
        let proof = Self::parse_proof(proof)?;
        let pending = self.take_challenge(&proof, SessionBrokerHelloRole::Caller)?;
        self.verify_proof(&pending, &proof.signature)?;
        self.shared.assert_epoch(epoch)?;
        self.shared.require_active_grant(&pending.grant)?;
        let BrokerGrant::Caller(grant) = pending.grant.clone() else {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        };
        let hello_transcript_hash =
            encode_base64_url(&self.shared.crypto.sha256(&pending.transcript));
        let now = self.shared.now();
        let expires_at = grant.base.expires_at.min(
            now.checked_add(self.shared.config.caller_session_ttl_ms)
                .ok_or_else(|| {
                    auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential)
                })?,
        );
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.shared.prune_expired(&mut state, now);
        if state.clear_epoch != epoch {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        let caller_session_id = self.shared.unique_id(&state, true)?;
        let principal = match principal_from_grant(&BrokerGrant::Caller(grant.clone())) {
            crate::BrokerPrincipal::Caller(principal) => principal,
            crate::BrokerPrincipal::Producer(_) => unreachable!(),
        };
        let retained_bytes = retained_record_bytes(
            &[
                Value::String(caller_session_id.clone()),
                caller_principal_value(&principal),
                grant_value(&BrokerGrant::Caller(grant.clone())),
                Value::String(hello_transcript_hash.clone()),
                Value::from(expires_at),
            ],
            &[],
            CALLER_SESSION_RECORD_OVERHEAD_BYTES,
        )?;
        if retained_bytes > self.shared.config.max_caller_session_bytes {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::AuthenticationCapacity,
            ));
        }
        let mut reservation = ReservationGroup::default();
        let count = self
            .shared
            .caller_session_count_budget
            .reserve(1)
            .map_err(|_| {
                auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
            })?;
        reservation.add(count).map_err(|_| {
            auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
        })?;
        let bytes = self
            .shared
            .caller_session_byte_budget
            .reserve(retained_bytes)
            .map_err(|_| {
                auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
            })?;
        reservation.add(bytes).map_err(|_| {
            auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationCapacity)
        })?;
        state
            .reserved_caller_session_ids
            .insert(caller_session_id.clone());
        drop(state);
        let transcript = build_broker_hello_ack_transcript(&BrokerHelloAckTranscriptInput {
            app_id: self.shared.config.app_id.clone(),
            generation: self.shared.config.generation.clone(),
            key_id: grant.base.key_id.clone(),
            grant_id: grant.base.grant_id.clone(),
            hello_transcript_hash: hello_transcript_hash.clone(),
            selection: (&pending.request.proposal).into(),
            binding: BrokerHelloAckBinding::Caller {
                caller_session_id: caller_session_id.clone(),
                initial_sequence: "1".into(),
            },
        })
        .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        let daemon_signature = encode_base64_url(
            &self
                .shared
                .crypto
                .sign(&self.shared.config.daemon_identity.private_key, &transcript),
        );
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.reserved_caller_session_ids.remove(&caller_session_id);
        if state.clear_epoch != epoch {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        state.caller_sessions.insert(
            caller_session_id.clone(),
            CallerSessionRecord {
                principal: principal.clone(),
                grant,
                public_key: pending.public_key,
                hello_transcript_hash: hello_transcript_hash.clone(),
                expires_at,
                replay: CallerSequenceReplayWindow::default(),
                _reservation: reservation,
            },
        );
        Ok(AuthenticatedCallerSession {
            caller_session_id,
            principal,
            expires_at,
            initial_sequence: "1".into(),
            broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
            app_revision: self.shared.config.app_revision,
            features: Vec::new(),
            hello_transcript_hash,
            daemon_key_id: self.shared.config.daemon_identity.key_id.clone(),
            daemon_signature,
        })
    }

    pub fn complete_producer_hello(
        &self,
        proof: Value,
        connection_id: &str,
    ) -> Result<AuthenticatedProducerHello, SessionBrokerAuthenticationError> {
        if !is_valid_broker_identifier(connection_id) {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        }
        let epoch = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear_epoch;
        let proof = Self::parse_proof(proof)?;
        let pending = self.take_challenge(&proof, SessionBrokerHelloRole::Producer)?;
        self.verify_proof(&pending, &proof.signature)?;
        self.shared.assert_epoch(epoch)?;
        self.shared.require_active_grant(&pending.grant)?;
        let BrokerGrant::Producer(grant) = pending.grant.clone() else {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidCredential,
            ));
        };
        let hello_transcript_hash =
            encode_base64_url(&self.shared.crypto.sha256(&pending.transcript));
        let transcript = build_broker_hello_ack_transcript(&BrokerHelloAckTranscriptInput {
            app_id: self.shared.config.app_id.clone(),
            generation: self.shared.config.generation.clone(),
            key_id: grant.base.key_id.clone(),
            grant_id: grant.base.grant_id.clone(),
            hello_transcript_hash: hello_transcript_hash.clone(),
            selection: (&pending.request.proposal).into(),
            binding: BrokerHelloAckBinding::Producer {
                connection_id: connection_id.into(),
            },
        })
        .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))?;
        let daemon_signature = encode_base64_url(
            &self
                .shared
                .crypto
                .sign(&self.shared.config.daemon_identity.private_key, &transcript),
        );
        self.shared.assert_epoch(epoch)?;
        self.shared
            .require_active_grant(&BrokerGrant::Producer(grant.clone()))?;
        let principal = match principal_from_grant(&BrokerGrant::Producer(grant.clone())) {
            crate::BrokerPrincipal::Producer(principal) => principal,
            crate::BrokerPrincipal::Caller(_) => unreachable!(),
        };
        Ok(AuthenticatedProducerHello {
            ack: SessionBrokerProducerHelloAck {
                principal,
                connection_id: connection_id.into(),
                broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
                app_revision: self.shared.config.app_revision,
                features: Vec::new(),
                hello_transcript_hash,
                daemon_key_id: self.shared.config.daemon_identity.key_id.clone(),
                daemon_signature,
            },
            shared: Some(Arc::clone(&self.shared)),
            epoch,
            grant: Some(grant),
            custom_assert_active: None,
        })
    }

    fn header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    pub fn authenticate(
        &self,
        input: &CallerRequestAuthenticationInput,
    ) -> Result<AuthenticatedCallerRequest, SessionBrokerAuthenticationError> {
        let caller_session_id = Self::header(&input.headers, "x-session-broker-caller-session")
            .ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationRequired)
            })?;
        let request_id =
            Self::header(&input.headers, "x-session-broker-request-id").ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationRequired)
            })?;
        let sequence =
            Self::header(&input.headers, "x-session-broker-sequence").ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationRequired)
            })?;
        let encoded_signature = Self::header(&input.headers, "x-session-broker-signature")
            .ok_or_else(|| {
                auth_error(SessionBrokerAuthenticationFailureCode::AuthenticationRequired)
            })?;
        if !is_valid_broker_identifier(caller_session_id) || !is_valid_broker_identifier(request_id)
        {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidSignature,
            ));
        }
        let signature = decode_base64_url(encoded_signature).filter(|value| !value.is_empty());
        let Some(signature) = signature else {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidSignature,
            ));
        };
        let url = parse_endpoint(&input.url, true)
            .ok_or_else(|| auth_error(SessionBrokerAuthenticationFailureCode::InvalidSignature))?;
        let target = canonical_http_target(&url)?;
        let body_digest = encode_base64_url(&self.shared.crypto.sha256(&input.body));
        let now = self.shared.now();
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.shared.prune_expired(&mut state, now);
        let Some(session) = state.caller_sessions.get(caller_session_id) else {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::CallerSessionExpired,
            ));
        };
        let grant = BrokerGrant::Caller(session.grant.clone());
        if let Err(error) = self.shared.require_active_grant(&grant) {
            state.caller_sessions.remove(caller_session_id);
            return Err(error);
        }
        if now >= session.expires_at {
            state.caller_sessions.remove(caller_session_id);
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::CallerSessionExpired,
            ));
        }
        let transcript = build_caller_request_transcript(&CallerRequestTranscriptInput {
            app_id: self.shared.config.app_id.clone(),
            generation: self.shared.config.generation.clone(),
            caller_session_id: caller_session_id.into(),
            key_id: session.grant.base.key_id.clone(),
            grant_id: session.grant.base.grant_id.clone(),
            hello_transcript_hash: session.hello_transcript_hash.clone(),
            method: input.method.clone(),
            target,
            body_digest,
            request_id: request_id.into(),
            sequence: sequence.into(),
        })
        .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidSignature))?;
        if !self
            .shared
            .crypto
            .verify(&session.public_key, &signature, &transcript)
        {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::InvalidSignature,
            ));
        }
        let session = state
            .caller_sessions
            .get_mut(caller_session_id)
            .expect("caller session remains present while locked");
        if session.replay.admit(sequence) != CallerSequenceAdmission::Accepted {
            return Err(auth_error(
                SessionBrokerAuthenticationFailureCode::ReplayRejected,
            ));
        }
        Ok(AuthenticatedCallerRequest {
            principal: session.principal.clone(),
            request_id: request_id.into(),
            caller_session_id: Some(caller_session_id.into()),
            sequence: Some(sequence.into()),
            hello_transcript_hash: Some(session.hello_transcript_hash.clone()),
            shared: Some(Arc::clone(&self.shared)),
            epoch: state.clear_epoch,
            custom_assert_active: None,
            custom_sign_response: None,
        })
    }

    pub fn revoke_caller_session(&self, caller_session_id: &str) {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .caller_sessions
            .remove(caller_session_id);
    }

    pub fn clear(&self) {
        let mut state = self
            .shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.clear_epoch = state.clear_epoch.wrapping_add(1);
        state.challenges.clear();
        state.caller_sessions.clear();
    }

    #[must_use]
    pub fn pending_challenge_count(&self) -> usize {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .challenges
            .len()
    }

    #[must_use]
    pub fn caller_session_count(&self) -> usize {
        self.shared
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .caller_sessions
            .len()
    }
}

impl CallerRequestAuthenticator for SessionBrokerAuthenticator {
    fn authenticate_request(
        &self,
        input: &CallerRequestAuthenticationInput,
    ) -> Result<AuthenticatedCallerRequest, SessionBrokerAuthenticationError> {
        self.authenticate(input)
    }

    fn clear_authentication(&self) {
        self.clear();
    }
}

impl SessionBrokerHelloAuthenticator for SessionBrokerAuthenticator {
    fn issue_hello_challenge(
        &self,
        request: Value,
        listener_endpoint: &str,
    ) -> Result<SessionBrokerHelloChallenge, SessionBrokerAuthenticationError> {
        self.issue_challenge(request, listener_endpoint)
    }

    fn complete_caller_hello_proof(
        &self,
        proof: Value,
    ) -> Result<AuthenticatedCallerSession, SessionBrokerAuthenticationError> {
        self.complete_caller_hello(proof)
    }

    fn complete_producer_hello_proof(
        &self,
        proof: Value,
        connection_id: &str,
    ) -> Result<AuthenticatedProducerHello, SessionBrokerAuthenticationError> {
        self.complete_producer_hello(proof, connection_id)
    }
}

/// Build the exact challenge transcript so clients verify daemon identity before signing.
pub fn challenge_transcript_for_client(
    request: &SessionBrokerHelloChallengeRequest,
    challenge: &SessionBrokerHelloChallenge,
    generation: &str,
) -> Result<Vec<u8>, SessionBrokerAuthenticationError> {
    build_broker_challenge_transcript(&BrokerChallengeTranscriptInput {
        role: request.role.broker_role(),
        app_id: request.app_id.clone(),
        generation: generation.into(),
        endpoint: request.endpoint.clone(),
        key_id: request.key_id.clone(),
        grant_id: request.grant_id.clone(),
        initiator_nonce: request.initiator_nonce.clone(),
        responder_nonce: challenge.responder_nonce.clone(),
        proposal: (&request.proposal).into(),
    })
    .map_err(|_| auth_error(SessionBrokerAuthenticationFailureCode::InvalidCredential))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
    use std::sync::{Condvar, Mutex};

    use ed25519_dalek::SigningKey;

    use super::*;

    struct Values {
        authenticator: SessionBrokerAuthenticator,
        daemon: SigningKey,
        caller: SigningKey,
        producer: SigningKey,
        caller_grant: CallerGrant,
        now: Arc<AtomicU64>,
        revoked: Arc<AtomicBool>,
    }

    fn base(role: &str, expires_at: u64) -> BrokerGrantBase {
        BrokerGrantBase {
            app_id: "dev.example".into(),
            principal_id: format!("{role}-1"),
            key_id: format!("{role}-key-1"),
            grant_id: format!("{role}-grant-1"),
            algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
            issued_at: 1,
            expires_at,
            revocation_id: format!("{role}-revocation-1"),
            may_delegate: false,
            session_id: None,
        }
    }

    fn setup_with(
        configure: impl FnOnce(&mut SessionBrokerAuthenticatorOptions),
    ) -> Result<Values, SessionBrokerAuthenticatorConfigError> {
        let daemon = SigningKey::from_bytes(&[1; 32]);
        let caller = SigningKey::from_bytes(&[2; 32]);
        let producer = SigningKey::from_bytes(&[3; 32]);
        let caller_grant = CallerGrant {
            base: base("caller", 10_000),
            operations: vec![
                CallerOperation::List,
                CallerOperation::Get,
                CallerOperation::Dispatch,
            ],
            commands: vec![BrokerCommandScope {
                name: "navigate_to_hunk".into(),
                version: 1,
            }],
        };
        let producer_grant = ProducerGrant {
            base: base("producer", 10_000),
            operations: vec![ProducerOperation::Register],
        };
        let now = Arc::new(AtomicU64::new(2_000));
        let revoked = Arc::new(AtomicBool::new(false));
        let now_reader = Arc::clone(&now);
        let revoked_reader = Arc::clone(&revoked);
        let mut options = SessionBrokerAuthenticatorOptions {
            app_id: "dev.example".into(),
            app_revision: 7,
            generation: "generation-1".into(),
            daemon_identity: SessionBrokerDaemonIdentity {
                key_id: "daemon-key-1".into(),
                private_key: daemon.clone(),
            },
            credentials: vec![
                SessionBrokerAuthorityCredential {
                    grant: BrokerGrant::Caller(caller_grant.clone()),
                    public_key: caller.verifying_key(),
                },
                SessionBrokerAuthorityCredential {
                    grant: BrokerGrant::Producer(producer_grant),
                    public_key: producer.verifying_key(),
                },
            ],
            crypto: None,
            now: Some(Arc::new(move || now_reader.load(Ordering::Acquire))),
            is_revoked: Some(Arc::new(move |_| revoked_reader.load(Ordering::Acquire))),
            challenge_ttl_ms: None,
            caller_session_ttl_ms: None,
            max_challenges: None,
            max_challenge_bytes: None,
            max_challenge_transcript_bytes: None,
            max_caller_sessions: None,
            limits: SessionBrokerLimitOptions::default(),
        };
        configure(&mut options);
        let authenticator = SessionBrokerAuthenticator::new(options)?;
        Ok(Values {
            authenticator,
            daemon,
            caller,
            producer,
            caller_grant,
            now,
            revoked,
        })
    }

    fn setup() -> Values {
        setup_with(|_| {}).unwrap()
    }

    fn hello(role: SessionBrokerHelloRole) -> SessionBrokerHelloChallengeRequest {
        let role_name = match role {
            SessionBrokerHelloRole::Caller => "caller",
            SessionBrokerHelloRole::Producer => "producer",
        };
        SessionBrokerHelloChallengeRequest {
            role,
            app_id: "dev.example".into(),
            endpoint: "http://broker.test/session-auth/challenge".into(),
            key_id: format!("{role_name}-key-1"),
            grant_id: format!("{role_name}-grant-1"),
            initiator_nonce: "initiator-nonce-1".into(),
            proposal: SessionBrokerHelloProposalWire {
                broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
                app_revision: 7,
                features: Vec::new(),
            },
        }
    }

    fn signed_proof(
        values: &Values,
        role: SessionBrokerHelloRole,
    ) -> (SessionBrokerHelloChallenge, Value) {
        let request = hello(role);
        let challenge = values
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
        let transcript =
            challenge_transcript_for_client(&request, &challenge, "generation-1").unwrap();
        let key = match role {
            SessionBrokerHelloRole::Caller => &values.caller,
            SessionBrokerHelloRole::Producer => &values.producer,
        };
        let signature = encode_base64_url(&NativeSessionBrokerCrypto.sign(key, &transcript));
        let proof = json!({"challengeId": challenge.challenge_id, "signature": signature});
        (challenge, proof)
    }

    fn open_caller(values: &Values) -> AuthenticatedCallerSession {
        let (_, proof) = signed_proof(values, SessionBrokerHelloRole::Caller);
        values.authenticator.complete_caller_hello(proof).unwrap()
    }

    fn signed_request(
        values: &Values,
        session: &AuthenticatedCallerSession,
        sequence: &str,
        body: &[u8],
    ) -> CallerRequestAuthenticationInput {
        let method = "POST";
        let url = "http://broker.test/broker?z=2&a=hello%20world";
        let target = canonical_http_target(&Url::parse(url).unwrap()).unwrap();
        let request_id = format!("request-{sequence}");
        let body_digest = encode_base64_url(&NativeSessionBrokerCrypto.sha256(body));
        let transcript = build_caller_request_transcript(&CallerRequestTranscriptInput {
            app_id: "dev.example".into(),
            generation: "generation-1".into(),
            caller_session_id: session.caller_session_id.clone(),
            key_id: values.caller_grant.base.key_id.clone(),
            grant_id: values.caller_grant.base.grant_id.clone(),
            hello_transcript_hash: session.hello_transcript_hash.clone(),
            method: method.into(),
            target,
            body_digest,
            request_id: request_id.clone(),
            sequence: sequence.into(),
        })
        .unwrap();
        let signature =
            encode_base64_url(&NativeSessionBrokerCrypto.sign(&values.caller, &transcript));
        CallerRequestAuthenticationInput {
            method: method.into(),
            url: url.into(),
            headers: BTreeMap::from([
                (
                    "x-session-broker-caller-session".into(),
                    session.caller_session_id.clone(),
                ),
                ("x-session-broker-request-id".into(), request_id),
                ("x-session-broker-sequence".into(), sequence.into()),
                ("x-session-broker-signature".into(), signature),
            ]),
            body: body.into(),
        }
    }

    #[test]
    fn canonicalizes_encoded_paths_and_sorted_duplicate_query_values() {
        let url = Url::parse("http://broker.test/review/%7euser?z=2&a=hello+world&a=%2F").unwrap();
        assert_eq!(
            canonical_http_target(&url).unwrap(),
            "/review/~user?a=%2F&a=hello%20world&z=2"
        );
        for malformed in ["http://broker.test/%", "http://broker.test/%ff"] {
            let url = Url::parse(malformed).unwrap();
            assert_eq!(
                canonical_http_target(&url).unwrap_err().code,
                SessionBrokerAuthenticationFailureCode::InvalidSignature
            );
        }
    }

    #[test]
    fn verifies_daemon_identity_before_accepting_caller_and_producer_proofs() {
        let values = setup();
        let (challenge, proof) = signed_proof(&values, SessionBrokerHelloRole::Caller);
        let request = hello(SessionBrokerHelloRole::Caller);
        let transcript =
            challenge_transcript_for_client(&request, &challenge, "generation-1").unwrap();
        let daemon_signature = decode_base64_url(&challenge.daemon_signature).unwrap();
        assert!(NativeSessionBrokerCrypto.verify(
            &values.daemon.verifying_key(),
            &daemon_signature,
            &transcript
        ));
        let caller = values.authenticator.complete_caller_hello(proof).unwrap();
        assert_eq!(caller.initial_sequence, "1");
        assert_eq!(caller.broker_revision, 1);
        assert_eq!(
            serde_json::to_value(&caller).unwrap()["principal"]["kind"],
            "caller"
        );

        let (_, proof) = signed_proof(&values, SessionBrokerHelloRole::Producer);
        let producer = values
            .authenticator
            .complete_producer_hello(proof, "connection-1")
            .unwrap();
        assert_eq!(producer.ack.connection_id, "connection-1");
        assert_eq!(
            producer.ack.principal.scopes,
            vec![ProducerOperation::Register]
        );
        assert_eq!(
            serde_json::to_value(&producer.ack).unwrap()["principal"]["kind"],
            "producer"
        );
        producer.assert_active().unwrap();
    }

    #[test]
    fn producer_authority_rechecks_revocation_expiry_and_clear_epochs() {
        let values = setup();
        let (_, proof) = signed_proof(&values, SessionBrokerHelloRole::Producer);
        let producer = values
            .authenticator
            .complete_producer_hello(proof, "connection-1")
            .unwrap();
        values.revoked.store(true, Ordering::Release);
        assert_eq!(
            producer.assert_active().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::CredentialRevoked
        );
        values.revoked.store(false, Ordering::Release);
        values.now.store(10_000, Ordering::Release);
        assert_eq!(
            producer.assert_active().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::CredentialExpired
        );

        let cleared = setup();
        let (_, proof) = signed_proof(&cleared, SessionBrokerHelloRole::Producer);
        let producer = cleared
            .authenticator
            .complete_producer_hello(proof, "connection-2")
            .unwrap();
        cleared.authenticator.clear();
        assert_eq!(
            producer.assert_active().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
    }

    #[test]
    fn rejects_missing_wrong_expired_revoked_and_reused_credentials_redacted() {
        let values = setup();
        let mut wrong = hello(SessionBrokerHelloRole::Caller);
        wrong.key_id = "wrong-key".into();
        assert_eq!(
            values
                .authenticator
                .issue_challenge(
                    serde_json::to_value(wrong).unwrap(),
                    &hello(SessionBrokerHelloRole::Caller).endpoint
                )
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
        assert_eq!(
            values
                .authenticator
                .authenticate(&CallerRequestAuthenticationInput {
                    method: "GET".into(),
                    url: "http://broker.test/broker".into(),
                    headers: BTreeMap::new(),
                    body: Vec::new(),
                })
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationRequired
        );
        let (_, proof) = signed_proof(&values, SessionBrokerHelloRole::Caller);
        values
            .authenticator
            .complete_caller_hello(proof.clone())
            .unwrap();
        assert_eq!(
            values
                .authenticator
                .complete_caller_hello(proof)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::ChallengeUsed
        );
        let error = auth_error(SessionBrokerAuthenticationFailureCode::InvalidSignature);
        assert_eq!(error.to_string(), "Session broker authentication failed.");
        assert!(!error.to_string().contains("signature"));
    }

    #[test]
    fn expires_challenges_and_short_lived_caller_sessions() {
        let challenge_values = setup();
        let (challenge, proof) = signed_proof(&challenge_values, SessionBrokerHelloRole::Caller);
        challenge_values
            .now
            .store(challenge.expires_at, Ordering::Release);
        assert_eq!(
            challenge_values
                .authenticator
                .complete_caller_hello(proof)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::ChallengeExpired
        );

        let values = setup_with(|options| options.caller_session_ttl_ms = Some(500)).unwrap();
        let session = open_caller(&values);
        values.now.store(session.expires_at, Ordering::Release);
        let request = signed_request(&values, &session, "1", br#"{"action":"list"}"#);
        assert_eq!(
            values
                .authenticator
                .authenticate(&request)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::CallerSessionExpired
        );
    }

    #[test]
    fn authentication_ttls_are_lower_only_and_capacity_is_reused() {
        let lowered = setup_with(|options| {
            options.limits.limits.challenge_ttl_ms = Some(700);
            options.limits.limits.caller_session_ttl_ms = Some(900);
            options.challenge_ttl_ms = Some(600);
            options.caller_session_ttl_ms = Some(800);
        })
        .unwrap();
        let (challenge, _) = signed_proof(&lowered, SessionBrokerHelloRole::Caller);
        assert_eq!(challenge.expires_at, 2_600);
        assert_eq!(open_caller(&lowered).expires_at, 2_800);
        assert!(setup_with(|options| options.challenge_ttl_ms = Some(15_001)).is_err());

        let limited = setup_with(|options| options.max_challenges = Some(1)).unwrap();
        let request = hello(SessionBrokerHelloRole::Caller);
        limited
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
        assert_eq!(
            limited
                .authenticator
                .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );
        limited.authenticator.clear();
        limited
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
    }

    #[test]
    fn binds_method_target_body_request_id_and_replay_sequence() {
        let values = setup();
        let session = open_caller(&values);
        let body = br#"{"action":"list"}"#;
        let request = signed_request(&values, &session, "1", body);
        let authenticated = values.authenticator.authenticate(&request).unwrap();
        assert_eq!(authenticated.principal.principal_id, "caller-1");
        assert_eq!(
            values
                .authenticator
                .authenticate(&request)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::ReplayRejected
        );
        let request = signed_request(&values, &session, "2", body);
        let mut tampered = request.clone();
        tampered.body = br#"{"action":"get"}"#.to_vec();
        assert_eq!(
            values
                .authenticator
                .authenticate(&tampered)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::InvalidSignature
        );
        values.authenticator.authenticate(&request).unwrap();
        let mut ftp = signed_request(&values, &session, "3", body);
        ftp.url = "ftp://broker.test/broker".into();
        assert_eq!(
            values.authenticator.authenticate(&ftp).unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidSignature
        );
    }

    #[test]
    fn signs_response_envelopes_and_invalidates_live_handles() {
        let values = setup();
        let session = open_caller(&values);
        let request = signed_request(&values, &session, "1", br#"{"action":"list"}"#);
        let authenticated = values.authenticator.authenticate(&request).unwrap();
        let response = authenticated
            .sign_response(&CallerResponseSigningInput {
                http_status: 200,
                body: json!({"sessions": []}),
                app_contract: Some(SignedBrokerAppContract {
                    app_revision: 7,
                    features: Vec::new(),
                }),
            })
            .unwrap();
        let transcript = build_broker_response_transcript(&BrokerResponseTranscriptInput {
            app_id: "dev.example".into(),
            generation: response.generation.clone(),
            broker_revision: response.broker_revision,
            caller_session_id: response.caller_session_id.clone(),
            request_id: response.request_id.clone(),
            sequence: response.sequence.clone(),
            http_status: response.http_status,
            body_digest: response.body_digest.clone(),
            app_contract: Some(SignedBrokerAppContract {
                app_revision: 7,
                features: Vec::new(),
            }),
        })
        .unwrap();
        assert!(NativeSessionBrokerCrypto.verify(
            &values.daemon.verifying_key(),
            &decode_base64_url(&response.daemon_signature).unwrap(),
            &transcript
        ));
        values.authenticator.clear();
        assert_eq!(
            authenticated.assert_active().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
    }

    #[test]
    fn validates_exact_hello_proposals_identifiers_endpoints_and_startup_authority() {
        let values = setup();
        let request = hello(SessionBrokerHelloRole::Caller);
        let mut value = serde_json::to_value(&request).unwrap();
        value["proposal"]["features"] = json!(["extra"]);
        assert_eq!(
            values
                .authenticator
                .issue_challenge(value, &request.endpoint)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
        for endpoint in [
            "ftp://broker.test/challenge",
            "http://user@broker.test/challenge",
            "http://broker.test/challenge#fragment",
        ] {
            let mut request = hello(SessionBrokerHelloRole::Caller);
            request.endpoint = endpoint.into();
            assert_eq!(
                values
                    .authenticator
                    .issue_challenge(serde_json::to_value(&request).unwrap(), endpoint)
                    .unwrap_err()
                    .code,
                SessionBrokerAuthenticationFailureCode::InvalidCredential
            );
        }
        assert!(setup_with(|options| options.app_id = "Bad App".into()).is_err());
        assert!(
            setup_with(|options| {
                options.credentials.push(options.credentials[0].clone());
            })
            .is_err()
        );
    }

    #[test]
    fn caller_session_revocation_is_idempotent_and_non_enumerating() {
        let values = setup();
        let session = open_caller(&values);
        assert_eq!(values.authenticator.caller_session_count(), 1);
        values
            .authenticator
            .revoke_caller_session(&session.caller_session_id);
        values
            .authenticator
            .revoke_caller_session(&session.caller_session_id);
        assert_eq!(values.authenticator.caller_session_count(), 0);
        assert_eq!(
            values
                .authenticator
                .authenticate(&signed_request(
                    &values,
                    &session,
                    "1",
                    br#"{"action":"list"}"#
                ))
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::CallerSessionExpired
        );
    }

    #[test]
    fn wrong_proofs_are_consumed_and_role_mismatches_are_redacted() {
        let values = setup();
        let (challenge, _) = signed_proof(&values, SessionBrokerHelloRole::Caller);
        let wrong = json!({
            "challengeId": challenge.challenge_id,
            "signature": encode_base64_url(&[7; 64]),
        });
        assert_eq!(
            values
                .authenticator
                .complete_caller_hello(wrong.clone())
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::InvalidSignature
        );
        assert_eq!(
            values
                .authenticator
                .complete_caller_hello(wrong)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::ChallengeUsed
        );

        let (_, producer_proof) = signed_proof(&values, SessionBrokerHelloRole::Producer);
        assert_eq!(
            values
                .authenticator
                .complete_caller_hello(producer_proof)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
    }

    #[test]
    fn caller_session_count_and_byte_budgets_release_after_revocation() {
        let values = setup_with(|options| options.max_caller_sessions = Some(1)).unwrap();
        let first = open_caller(&values);
        let (_, proof) = signed_proof(&values, SessionBrokerHelloRole::Caller);
        assert_eq!(
            values
                .authenticator
                .complete_caller_hello(proof)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );
        values
            .authenticator
            .revoke_caller_session(&first.caller_session_id);
        open_caller(&values);

        let bounded = setup_with(|options| {
            options.limits.limits.max_caller_session_bytes = Some(1);
        })
        .unwrap();
        let (_, proof) = signed_proof(&bounded, SessionBrokerHelloRole::Caller);
        assert_eq!(
            bounded
                .authenticator
                .complete_caller_hello(proof)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );
    }

    #[test]
    fn bounds_retained_challenge_transcripts_and_malformed_proofs() {
        let values =
            setup_with(|options| options.max_challenge_transcript_bytes = Some(1)).unwrap();
        let request = hello(SessionBrokerHelloRole::Caller);
        assert_eq!(
            values
                .authenticator
                .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );

        let values = setup();
        for proof in [
            json!(null),
            json!({"challengeId": "challenge-1"}),
            json!({"challengeId": "challenge-1", "signature": "x", "extra": true}),
            json!({"challengeId": "bad id", "signature": "x"}),
            json!({"challengeId": "challenge-1", "signature": "x".repeat(1_025)}),
        ] {
            assert_eq!(
                values
                    .authenticator
                    .complete_caller_hello(proof)
                    .unwrap_err()
                    .code,
                SessionBrokerAuthenticationFailureCode::InvalidCredential
            );
        }
    }

    #[test]
    fn response_signing_rejects_invalid_status_and_target_contract() {
        let values = setup();
        let session = open_caller(&values);
        let request = signed_request(&values, &session, "1", br#"{"action":"list"}"#);
        let authenticated = values.authenticator.authenticate(&request).unwrap();
        assert_eq!(
            authenticated
                .sign_response(&CallerResponseSigningInput {
                    http_status: 99,
                    body: json!({"ok": true}),
                    app_contract: None,
                })
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
        assert_eq!(
            authenticated
                .sign_response(&CallerResponseSigningInput {
                    http_status: 200,
                    body: json!({"ok": true}),
                    app_contract: Some(SignedBrokerAppContract {
                        app_revision: 8,
                        features: Vec::new(),
                    }),
                })
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
    }

    #[derive(Debug)]
    struct FixedRandomCrypto;

    impl SessionBrokerCrypto for FixedRandomCrypto {
        fn random_bytes(&self, length: usize) -> Result<Vec<u8>, crate::BrokerCryptoError> {
            Ok(vec![9; length])
        }

        fn sha256(&self, value: &[u8]) -> [u8; 32] {
            NativeSessionBrokerCrypto.sha256(value)
        }

        fn sign(&self, private_key: &SigningKey, value: &[u8]) -> Vec<u8> {
            NativeSessionBrokerCrypto.sign(private_key, value)
        }

        fn verify(&self, public_key: &VerifyingKey, signature: &[u8], value: &[u8]) -> bool {
            NativeSessionBrokerCrypto.verify(public_key, signature, value)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CryptoGateKind {
        Sign,
        Verify,
    }

    struct CryptoGate {
        kind: CryptoGateKind,
        entered: SyncSender<()>,
        release: Receiver<()>,
    }

    #[derive(Default)]
    struct ControlledCrypto {
        gate: Mutex<Option<CryptoGate>>,
        gate_changed: Condvar,
    }

    impl ControlledCrypto {
        fn arm(&self, kind: CryptoGateKind) -> (Receiver<()>, SyncSender<()>) {
            let (entered_sender, entered_receiver) = sync_channel(1);
            let (release_sender, release_receiver) = sync_channel(1);
            let mut gate = self.gate.lock().unwrap();
            *gate = Some(CryptoGate {
                kind,
                entered: entered_sender,
                release: release_receiver,
            });
            self.gate_changed.notify_all();
            (entered_receiver, release_sender)
        }

        fn pause(&self, kind: CryptoGateKind) {
            let gate = {
                let mut slot = self.gate.lock().unwrap();
                if slot.as_ref().is_some_and(|gate| gate.kind == kind) {
                    slot.take()
                } else {
                    None
                }
            };
            if let Some(gate) = gate {
                gate.entered.send(()).unwrap();
                gate.release.recv().unwrap();
            }
        }
    }

    impl SessionBrokerCrypto for ControlledCrypto {
        fn random_bytes(&self, length: usize) -> Result<Vec<u8>, crate::BrokerCryptoError> {
            NativeSessionBrokerCrypto.random_bytes(length)
        }

        fn sha256(&self, value: &[u8]) -> [u8; 32] {
            NativeSessionBrokerCrypto.sha256(value)
        }

        fn sign(&self, private_key: &SigningKey, value: &[u8]) -> Vec<u8> {
            self.pause(CryptoGateKind::Sign);
            NativeSessionBrokerCrypto.sign(private_key, value)
        }

        fn verify(&self, public_key: &VerifyingKey, signature: &[u8], value: &[u8]) -> bool {
            self.pause(CryptoGateKind::Verify);
            NativeSessionBrokerCrypto.verify(public_key, signature, value)
        }
    }

    #[test]
    fn collision_safe_ids_fail_closed_after_the_bounded_retry_count() {
        let values =
            setup_with(|options| options.crypto = Some(Arc::new(FixedRandomCrypto))).unwrap();
        let request = hello(SessionBrokerHelloRole::Caller);
        values
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
        assert_eq!(
            values
                .authenticator
                .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );
        values.authenticator.clear();
        values
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
    }

    #[test]
    fn startup_grants_are_snapshotted_and_reject_duplicate_authority() {
        let mut original = CallerGrant {
            base: base("caller", 10_000),
            operations: vec![CallerOperation::List],
            commands: Vec::new(),
        };
        let saved = original.clone();
        let values = setup_with(|options| {
            options.credentials[0].grant = BrokerGrant::Caller(original.clone());
        })
        .unwrap();
        original.base.app_id = "attacker.example".into();
        let request = hello(SessionBrokerHelloRole::Caller);
        values
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
        assert_eq!(saved.base.app_id, "dev.example");

        assert!(
            setup_with(|options| {
                let BrokerGrant::Caller(grant) = &mut options.credentials[0].grant else {
                    unreachable!()
                };
                grant.operations.push(CallerOperation::List);
            })
            .is_err()
        );
    }

    #[test]
    fn unsafe_limits_can_raise_the_explicit_ttl_contract() {
        let values = setup_with(|options| {
            options.limits.unsafe_limits.challenge_ttl_ms = Some(20_000);
            options.limits.unsafe_limits.caller_session_ttl_ms = Some(400_000);
            let BrokerGrant::Caller(grant) = &mut options.credentials[0].grant else {
                unreachable!()
            };
            grant.base.expires_at = 1_000_000;
        })
        .unwrap();
        let (challenge, _) = signed_proof(&values, SessionBrokerHelloRole::Caller);
        assert_eq!(challenge.expires_at, 22_000);
        assert_eq!(open_caller(&values).expires_at, 402_000);
    }

    #[test]
    fn clear_releases_challenges_sessions_and_invalidates_every_authority() {
        let values = setup();
        let (_, caller_proof) = signed_proof(&values, SessionBrokerHelloRole::Caller);
        let session = values
            .authenticator
            .complete_caller_hello(caller_proof)
            .unwrap();
        let (_, producer_proof) = signed_proof(&values, SessionBrokerHelloRole::Producer);
        let producer = values
            .authenticator
            .complete_producer_hello(producer_proof, "connection-1")
            .unwrap();
        let pending = signed_proof(&values, SessionBrokerHelloRole::Caller).1;
        assert_eq!(values.authenticator.pending_challenge_count(), 1);
        assert_eq!(values.authenticator.caller_session_count(), 1);
        values.authenticator.clear();
        assert_eq!(values.authenticator.pending_challenge_count(), 0);
        assert_eq!(values.authenticator.caller_session_count(), 0);
        assert_eq!(
            values
                .authenticator
                .complete_caller_hello(pending)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::ChallengeUsed
        );
        assert_eq!(
            producer.assert_active().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
        assert_eq!(
            values
                .authenticator
                .authenticate(&signed_request(
                    &values,
                    &session,
                    "1",
                    br#"{"action":"list"}"#
                ))
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::CallerSessionExpired
        );
    }

    #[test]
    fn clear_invalidates_deferred_challenge_signing_but_retains_capacity_until_settlement() {
        let crypto = Arc::new(ControlledCrypto::default());
        let values = setup_with(|options| {
            options.crypto = Some(crypto.clone());
            options.max_challenges = Some(1);
        })
        .unwrap();
        let (entered, release) = crypto.arm(CryptoGateKind::Sign);
        let authenticator = values.authenticator.clone();
        let request = hello(SessionBrokerHelloRole::Caller);
        let endpoint = request.endpoint.clone();
        let issue = std::thread::spawn(move || {
            authenticator.issue_challenge(serde_json::to_value(request).unwrap(), &endpoint)
        });
        entered.recv().unwrap();
        values.authenticator.clear();
        let request = hello(SessionBrokerHelloRole::Caller);
        assert_eq!(
            values
                .authenticator
                .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );
        release.send(()).unwrap();
        assert_eq!(
            issue.join().unwrap().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
        values
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
    }

    #[test]
    fn clear_invalidates_deferred_caller_proof_and_reuses_capacity_after_settlement() {
        let crypto = Arc::new(ControlledCrypto::default());
        let values = setup_with(|options| {
            options.crypto = Some(crypto.clone());
            options.max_challenges = Some(1);
            options.max_caller_sessions = Some(1);
        })
        .unwrap();
        let (_, proof) = signed_proof(&values, SessionBrokerHelloRole::Caller);
        let (entered, release) = crypto.arm(CryptoGateKind::Verify);
        let authenticator = values.authenticator.clone();
        let completion = std::thread::spawn(move || authenticator.complete_caller_hello(proof));
        entered.recv().unwrap();
        values.authenticator.clear();
        let request = hello(SessionBrokerHelloRole::Caller);
        assert_eq!(
            values
                .authenticator
                .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );
        release.send(()).unwrap();
        assert_eq!(
            completion.join().unwrap().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
        open_caller(&values);
    }

    #[test]
    fn clear_invalidates_deferred_producer_acknowledgement_until_capacity_settles() {
        let crypto = Arc::new(ControlledCrypto::default());
        let values = setup_with(|options| {
            options.crypto = Some(crypto.clone());
            options.max_challenges = Some(1);
        })
        .unwrap();
        let (_, proof) = signed_proof(&values, SessionBrokerHelloRole::Producer);
        let (entered, release) = crypto.arm(CryptoGateKind::Verify);
        let authenticator = values.authenticator.clone();
        let completion = std::thread::spawn(move || {
            authenticator.complete_producer_hello(proof, "connection-1")
        });
        entered.recv().unwrap();
        values.authenticator.clear();
        let request = hello(SessionBrokerHelloRole::Producer);
        assert_eq!(
            values
                .authenticator
                .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
                .unwrap_err()
                .code,
            SessionBrokerAuthenticationFailureCode::AuthenticationCapacity
        );
        release.send(()).unwrap();
        assert_eq!(
            completion.join().unwrap().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
        let (_, proof) = signed_proof(&values, SessionBrokerHelloRole::Producer);
        values
            .authenticator
            .complete_producer_hello(proof, "connection-2")
            .unwrap();
    }

    #[test]
    fn clear_invalidates_response_signing_across_deferred_crypto() {
        let crypto = Arc::new(ControlledCrypto::default());
        let values = setup_with(|options| options.crypto = Some(crypto.clone())).unwrap();
        let session = open_caller(&values);
        let request = signed_request(&values, &session, "1", br#"{"action":"list"}"#);
        let authenticated = values.authenticator.authenticate(&request).unwrap();
        let (entered, release) = crypto.arm(CryptoGateKind::Sign);
        let signing = std::thread::spawn(move || {
            authenticated.sign_response(&CallerResponseSigningInput {
                http_status: 200,
                body: json!({"ok": true}),
                app_contract: None,
            })
        });
        entered.recv().unwrap();
        values.authenticator.clear();
        release.send(()).unwrap();
        assert_eq!(
            signing.join().unwrap().unwrap_err().code,
            SessionBrokerAuthenticationFailureCode::InvalidCredential
        );
    }
}
