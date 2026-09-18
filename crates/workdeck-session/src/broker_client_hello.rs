//! Client half of the generation-bound producer and caller hello handshake.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ed25519_dalek::{SigningKey, VerifyingKey};
use serde_json::{Map, Value};
use thiserror::Error;

use crate::{
    BrokerGrantBase, BrokerHelloAckBinding, BrokerHelloAckTranscriptInput, BrokerHelloProposal,
    CallerGrant, NativeSessionBrokerCrypto, ProducerGrant, ProducerOperation,
    SESSION_BROKER_PROTOCOL_REVISION, SessionBrokerCrypto, SessionBrokerHelloChallenge,
    SessionBrokerHelloChallengeRequest, SessionBrokerHelloProposalWire, SessionBrokerHelloRole,
    SessionBrokerProducerHelloAck, build_broker_hello_ack_transcript,
    challenge_transcript_for_client, decode_base64_url, encode_base64_url,
    is_valid_broker_identifier,
};

#[derive(Debug, Error, Clone, PartialEq, Eq)]
#[error("Session broker authentication failed or the daemon identity could not be verified.")]
pub struct SessionBrokerClientAuthenticationError;

fn client_error() -> SessionBrokerClientAuthenticationError {
    SessionBrokerClientAuthenticationError
}

pub trait SessionBrokerClientGrant: Clone + Send + Sync + 'static {
    fn role(&self) -> SessionBrokerHelloRole;
    fn base(&self) -> &BrokerGrantBase;
}

impl SessionBrokerClientGrant for ProducerGrant {
    fn role(&self) -> SessionBrokerHelloRole {
        SessionBrokerHelloRole::Producer
    }

    fn base(&self) -> &BrokerGrantBase {
        &self.base
    }
}

impl SessionBrokerClientGrant for CallerGrant {
    fn role(&self) -> SessionBrokerHelloRole {
        SessionBrokerHelloRole::Caller
    }

    fn base(&self) -> &BrokerGrantBase {
        &self.base
    }
}

#[derive(Debug, Clone)]
pub struct SessionBrokerClientCredential<Grant> {
    pub grant: Grant,
    pub private_key: SigningKey,
}

#[derive(Debug, Clone)]
pub struct SessionBrokerDaemonVerifier {
    pub key_id: String,
    pub public_key: VerifyingKey,
}

#[derive(Clone)]
pub struct SessionBrokerHelloClientOptions<Grant> {
    pub app_id: String,
    pub app_revision: u32,
    pub endpoint: String,
    pub credential: SessionBrokerClientCredential<Grant>,
    pub daemon: SessionBrokerDaemonVerifier,
    pub crypto: Arc<dyn SessionBrokerCrypto>,
}

impl<Grant> std::fmt::Debug for SessionBrokerHelloClientOptions<Grant>
where
    Grant: std::fmt::Debug,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionBrokerHelloClientOptions")
            .field("app_id", &self.app_id)
            .field("app_revision", &self.app_revision)
            .field("endpoint", &self.endpoint)
            .field("credential", &self.credential)
            .field("daemon", &self.daemon)
            .finish_non_exhaustive()
    }
}

impl<Grant> SessionBrokerHelloClientOptions<Grant> {
    #[must_use]
    pub fn native(
        app_id: impl Into<String>,
        app_revision: u32,
        endpoint: impl Into<String>,
        credential: SessionBrokerClientCredential<Grant>,
        daemon: SessionBrokerDaemonVerifier,
    ) -> Self {
        Self {
            app_id: app_id.into(),
            app_revision,
            endpoint: endpoint.into(),
            credential,
            daemon,
            crypto: Arc::new(NativeSessionBrokerCrypto),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PendingSessionBrokerHello<Grant> {
    pub request: SessionBrokerHelloChallengeRequest,
    pub transcript: Vec<u8>,
    pub transcript_hash: String,
    pub proof: crate::SessionBrokerHelloProof,
    pub challenge: SessionBrokerHelloChallenge,
    pub options: SessionBrokerHelloClientOptions<Grant>,
}

fn fixed_proposal(app_revision: u32) -> SessionBrokerHelloProposalWire {
    SessionBrokerHelloProposalWire {
        broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
        app_revision,
        features: Vec::new(),
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn random_id(
    crypto: &dyn SessionBrokerCrypto,
) -> Result<String, SessionBrokerClientAuthenticationError> {
    let bytes = crypto.random_bytes(24).map_err(|_| client_error())?;
    if bytes.len() != 24 {
        return Err(client_error());
    }
    Ok(format!("b_{}_0", encode_base64_url(&bytes)))
}

/// Create the credential-free proposal starting producer or caller authentication.
pub fn create_session_broker_hello_request<Grant: SessionBrokerClientGrant>(
    options: &SessionBrokerHelloClientOptions<Grant>,
) -> Result<SessionBrokerHelloChallengeRequest, SessionBrokerClientAuthenticationError> {
    let base = options.credential.grant.base();
    Ok(SessionBrokerHelloChallengeRequest {
        role: options.credential.grant.role(),
        app_id: options.app_id.clone(),
        endpoint: options.endpoint.clone(),
        key_id: base.key_id.clone(),
        grant_id: base.grant_id.clone(),
        initiator_nonce: random_id(options.crypto.as_ref())?,
        proposal: fixed_proposal(options.app_revision),
    })
}

/// Verify the daemon challenge before signing the same generation-bound transcript.
pub fn answer_session_broker_hello_challenge<Grant: SessionBrokerClientGrant>(
    options: &SessionBrokerHelloClientOptions<Grant>,
    request: &SessionBrokerHelloChallengeRequest,
    challenge: &SessionBrokerHelloChallenge,
) -> Result<PendingSessionBrokerHello<Grant>, SessionBrokerClientAuthenticationError> {
    if challenge.daemon_key_id != options.daemon.key_id
        || !is_valid_broker_identifier(&challenge.challenge_id)
        || !is_valid_broker_identifier(&challenge.generation)
        || !is_valid_broker_identifier(&challenge.responder_nonce)
        || now_millis() >= challenge.expires_at
    {
        return Err(client_error());
    }
    let transcript = challenge_transcript_for_client(request, challenge, &challenge.generation)
        .map_err(|_| client_error())?;
    let daemon_signature =
        decode_base64_url(&challenge.daemon_signature).ok_or_else(client_error)?;
    if !options
        .crypto
        .verify(&options.daemon.public_key, &daemon_signature, &transcript)
    {
        return Err(client_error());
    }
    let signature = encode_base64_url(
        &options
            .crypto
            .sign(&options.credential.private_key, &transcript),
    );
    Ok(PendingSessionBrokerHello {
        request: request.clone(),
        transcript_hash: encode_base64_url(&options.crypto.sha256(&transcript)),
        transcript,
        proof: crate::SessionBrokerHelloProof {
            challenge_id: challenge.challenge_id.clone(),
            signature,
        },
        challenge: challenge.clone(),
        options: options.clone(),
    })
}

fn exact_record<'a>(
    value: &'a Value,
    keys: &[&str],
) -> Result<&'a Map<String, Value>, SessionBrokerClientAuthenticationError> {
    let record = value.as_object().ok_or_else(client_error)?;
    let expected = keys.iter().copied().collect::<BTreeSet<_>>();
    if record.len() != expected.len()
        || record.keys().any(|key| {
            matches!(key.as_str(), "__proto__" | "prototype" | "constructor")
                || !expected.contains(key.as_str())
        })
    {
        return Err(client_error());
    }
    Ok(record)
}

/// Strictly parse an untrusted daemon challenge before it reaches the signing path.
pub fn parse_session_broker_hello_challenge(
    value: &Value,
) -> Result<SessionBrokerHelloChallenge, SessionBrokerClientAuthenticationError> {
    let record = exact_record(
        value,
        &[
            "challengeId",
            "generation",
            "responderNonce",
            "expiresAt",
            "daemonKeyId",
            "daemonSignature",
        ],
    )?;
    let challenge = SessionBrokerHelloChallenge {
        challenge_id: required_string(record, "challengeId")?.into(),
        generation: required_string(record, "generation")?.into(),
        responder_nonce: required_string(record, "responderNonce")?.into(),
        expires_at: record
            .get("expiresAt")
            .and_then(Value::as_u64)
            .ok_or_else(client_error)?,
        daemon_key_id: required_string(record, "daemonKeyId")?.into(),
        daemon_signature: required_string(record, "daemonSignature")?.into(),
    };
    if !is_valid_broker_identifier(&challenge.challenge_id)
        || !is_valid_broker_identifier(&challenge.generation)
        || !is_valid_broker_identifier(&challenge.responder_nonce)
    {
        return Err(client_error());
    }
    Ok(challenge)
}

fn required_string<'a>(
    record: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, SessionBrokerClientAuthenticationError> {
    record
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(client_error)
}

fn required_u32(
    record: &Map<String, Value>,
    key: &str,
) -> Result<u32, SessionBrokerClientAuthenticationError> {
    record
        .get(key)
        .and_then(Value::as_u64)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(client_error)
}

fn parse_producer_operation(value: &Value) -> Option<ProducerOperation> {
    match value.as_str()? {
        "register" => Some(ProducerOperation::Register),
        "reconnect" => Some(ProducerOperation::Reconnect),
        _ => None,
    }
}

/// Strictly parse and verify a producer acknowledgement against its authenticated transcript.
pub fn verify_producer_hello_ack(
    pending: &PendingSessionBrokerHello<ProducerGrant>,
    ack: &Value,
) -> Result<SessionBrokerProducerHelloAck, SessionBrokerClientAuthenticationError> {
    let record = exact_record(
        ack,
        &[
            "principal",
            "connectionId",
            "brokerRevision",
            "appRevision",
            "features",
            "helloTranscriptHash",
            "daemonKeyId",
            "daemonSignature",
        ],
    )?;
    let grant = &pending.options.credential.grant;
    let principal_keys = if grant.base.session_id.is_some() {
        vec![
            "kind",
            "appId",
            "principalId",
            "keyId",
            "grantId",
            "scopes",
            "sessionId",
        ]
    } else {
        vec!["kind", "appId", "principalId", "keyId", "grantId", "scopes"]
    };
    let principal = exact_record(
        record.get("principal").ok_or_else(client_error)?,
        &principal_keys,
    )?;
    let scopes = principal
        .get("scopes")
        .and_then(Value::as_array)
        .ok_or_else(client_error)?
        .iter()
        .map(parse_producer_operation)
        .collect::<Option<Vec<_>>>()
        .ok_or_else(client_error)?;
    let session_id = principal.get("sessionId").and_then(Value::as_str);
    let features = record
        .get("features")
        .and_then(Value::as_array)
        .ok_or_else(client_error)?;
    let connection_id = required_string(record, "connectionId")?;
    let hello_transcript_hash = required_string(record, "helloTranscriptHash")?;
    let daemon_key_id = required_string(record, "daemonKeyId")?;
    if required_string(principal, "kind")? != "producer"
        || required_string(principal, "appId")? != grant.base.app_id
        || required_string(principal, "principalId")? != grant.base.principal_id
        || required_string(principal, "keyId")? != grant.base.key_id
        || required_string(principal, "grantId")? != grant.base.grant_id
        || session_id != grant.base.session_id.as_deref()
        || scopes != grant.operations
        || daemon_key_id != pending.options.daemon.key_id
        || hello_transcript_hash != pending.transcript_hash
        || required_u32(record, "brokerRevision")? != SESSION_BROKER_PROTOCOL_REVISION
        || required_u32(record, "appRevision")? != pending.options.app_revision
        || !features.is_empty()
        || !is_valid_broker_identifier(connection_id)
    {
        return Err(client_error());
    }
    let daemon_signature = required_string(record, "daemonSignature")?;
    let signature = decode_base64_url(daemon_signature).ok_or_else(client_error)?;
    let transcript = build_broker_hello_ack_transcript(&BrokerHelloAckTranscriptInput {
        app_id: pending.options.app_id.clone(),
        generation: pending.challenge.generation.clone(),
        key_id: grant.base.key_id.clone(),
        grant_id: grant.base.grant_id.clone(),
        hello_transcript_hash: pending.transcript_hash.clone(),
        selection: BrokerHelloProposal {
            broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
            app_revision: pending.options.app_revision,
            features: Vec::new(),
        },
        binding: BrokerHelloAckBinding::Producer {
            connection_id: connection_id.into(),
        },
    })
    .map_err(|_| client_error())?;
    if !pending
        .options
        .crypto
        .verify(&pending.options.daemon.public_key, &signature, &transcript)
    {
        return Err(client_error());
    }
    Ok(SessionBrokerProducerHelloAck {
        principal: crate::ProducerPrincipal {
            app_id: grant.base.app_id.clone(),
            principal_id: grant.base.principal_id.clone(),
            key_id: grant.base.key_id.clone(),
            grant_id: grant.base.grant_id.clone(),
            session_id: grant.base.session_id.clone(),
            scopes,
        },
        connection_id: connection_id.into(),
        broker_revision: SESSION_BROKER_PROTOCOL_REVISION,
        app_revision: pending.options.app_revision,
        features: Vec::new(),
        hello_transcript_hash: hello_transcript_hash.into(),
        daemon_key_id: daemon_key_id.into(),
        daemon_signature: daemon_signature.into(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ed25519_dalek::SigningKey;
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        BrokerGrant, CallerOperation, SESSION_BROKER_SIGNATURE_ALGORITHM,
        SessionBrokerAuthenticator, SessionBrokerAuthenticatorOptions,
        SessionBrokerAuthorityCredential, SessionBrokerDaemonIdentity, SessionBrokerLimitOptions,
    };

    struct Fixture {
        authenticator: SessionBrokerAuthenticator,
        daemon: SigningKey,
        producer: SigningKey,
        caller: SigningKey,
        producer_grant: ProducerGrant,
        caller_grant: CallerGrant,
        now: u64,
    }

    fn grant_base(role: &str, now: u64) -> BrokerGrantBase {
        BrokerGrantBase {
            app_id: "dev.example".into(),
            principal_id: format!("{role}-1"),
            key_id: format!("{role}-key-1"),
            grant_id: format!("{role}-grant-1"),
            algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
            issued_at: now.saturating_sub(1_000),
            expires_at: now.saturating_add(60_000),
            revocation_id: format!("{role}-revocation-1"),
            may_delegate: false,
            session_id: None,
        }
    }

    fn fixture() -> Fixture {
        let now = now_millis();
        let daemon = SigningKey::from_bytes(&[41; 32]);
        let producer = SigningKey::from_bytes(&[42; 32]);
        let caller = SigningKey::from_bytes(&[43; 32]);
        let producer_grant = ProducerGrant {
            base: grant_base("producer", now),
            operations: vec![ProducerOperation::Register, ProducerOperation::Reconnect],
        };
        let caller_grant = CallerGrant {
            base: grant_base("caller", now),
            operations: vec![CallerOperation::List, CallerOperation::Get],
            commands: Vec::new(),
        };
        let fixed_now = now;
        let authenticator = SessionBrokerAuthenticator::new(SessionBrokerAuthenticatorOptions {
            app_id: "dev.example".into(),
            app_revision: 7,
            generation: "generation-1".into(),
            daemon_identity: SessionBrokerDaemonIdentity {
                key_id: "daemon-key-1".into(),
                private_key: daemon.clone(),
            },
            credentials: vec![
                SessionBrokerAuthorityCredential {
                    grant: BrokerGrant::Producer(producer_grant.clone()),
                    public_key: producer.verifying_key(),
                },
                SessionBrokerAuthorityCredential {
                    grant: BrokerGrant::Caller(caller_grant.clone()),
                    public_key: caller.verifying_key(),
                },
            ],
            crypto: None,
            now: Some(Arc::new(move || fixed_now)),
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
            authenticator,
            daemon,
            producer,
            caller,
            producer_grant,
            caller_grant,
            now,
        }
    }

    fn options<Grant>(
        fixture: &Fixture,
        grant: Grant,
        private_key: SigningKey,
    ) -> SessionBrokerHelloClientOptions<Grant> {
        SessionBrokerHelloClientOptions::native(
            "dev.example",
            7,
            "http://broker.test/session-auth/challenge",
            SessionBrokerClientCredential { grant, private_key },
            SessionBrokerDaemonVerifier {
                key_id: "daemon-key-1".into(),
                public_key: fixture.daemon.verifying_key(),
            },
        )
    }

    fn issue<Grant: SessionBrokerClientGrant>(
        fixture: &Fixture,
        options: &SessionBrokerHelloClientOptions<Grant>,
    ) -> (
        SessionBrokerHelloChallengeRequest,
        SessionBrokerHelloChallenge,
    ) {
        let request = create_session_broker_hello_request(options).unwrap();
        let challenge = fixture
            .authenticator
            .issue_challenge(serde_json::to_value(&request).unwrap(), &request.endpoint)
            .unwrap();
        (request, challenge)
    }

    #[test]
    fn creates_exact_credential_free_proposals_for_both_roles() {
        let fixture = fixture();
        let producer = options(
            &fixture,
            fixture.producer_grant.clone(),
            fixture.producer.clone(),
        );
        let caller = options(
            &fixture,
            fixture.caller_grant.clone(),
            fixture.caller.clone(),
        );

        for (options, expected_role) in [
            (
                create_session_broker_hello_request(&producer).unwrap(),
                SessionBrokerHelloRole::Producer,
            ),
            (
                create_session_broker_hello_request(&caller).unwrap(),
                SessionBrokerHelloRole::Caller,
            ),
        ] {
            assert_eq!(options.role, expected_role);
            assert_eq!(options.app_id, "dev.example");
            assert_eq!(
                options.proposal.broker_revision,
                SESSION_BROKER_PROTOCOL_REVISION
            );
            assert_eq!(options.proposal.app_revision, 7);
            assert!(options.proposal.features.is_empty());
            assert!(is_valid_broker_identifier(&options.initiator_nonce));
            assert!(options.initiator_nonce.starts_with("b_"));
            assert!(options.initiator_nonce.ends_with("_0"));
        }
    }

    #[test]
    fn answers_server_challenges_for_producers_and_callers() {
        let fixture = fixture();
        let producer = options(
            &fixture,
            fixture.producer_grant.clone(),
            fixture.producer.clone(),
        );
        let (request, challenge) = issue(&fixture, &producer);
        let pending =
            answer_session_broker_hello_challenge(&producer, &request, &challenge).unwrap();
        let authenticated = fixture
            .authenticator
            .complete_producer_hello(
                serde_json::to_value(&pending.proof).unwrap(),
                "connection-1",
            )
            .unwrap();
        authenticated.assert_active().unwrap();

        let caller = options(
            &fixture,
            fixture.caller_grant.clone(),
            fixture.caller.clone(),
        );
        let (request, challenge) = issue(&fixture, &caller);
        let pending = answer_session_broker_hello_challenge(&caller, &request, &challenge).unwrap();
        let authenticated = fixture
            .authenticator
            .complete_caller_hello(serde_json::to_value(&pending.proof).unwrap())
            .unwrap();
        assert_eq!(authenticated.principal.app_id, "dev.example");
        assert_eq!(authenticated.hello_transcript_hash, pending.transcript_hash);
    }

    #[test]
    fn rejects_untrusted_challenge_shapes_and_identifiers() {
        let fixture = fixture();
        let producer = options(
            &fixture,
            fixture.producer_grant.clone(),
            fixture.producer.clone(),
        );
        let (_, challenge) = issue(&fixture, &producer);
        let value = serde_json::to_value(&challenge).unwrap();
        assert_eq!(
            parse_session_broker_hello_challenge(&value).unwrap(),
            challenge
        );

        for invalid in [
            json!([]),
            json!({}),
            {
                let mut value = value.clone();
                value["extra"] = json!(true);
                value
            },
            {
                let mut value = value.clone();
                value["challengeId"] = json!("not valid");
                value
            },
            {
                let mut value = value.clone();
                value["expiresAt"] = json!(1.5);
                value
            },
        ] {
            assert!(parse_session_broker_hello_challenge(&invalid).is_err());
        }
    }

    #[test]
    fn rejects_unverified_expired_and_tampered_challenges() {
        let fixture = fixture();
        let producer = options(
            &fixture,
            fixture.producer_grant.clone(),
            fixture.producer.clone(),
        );
        let (request, challenge) = issue(&fixture, &producer);
        let mut wrong_daemon = challenge.clone();
        wrong_daemon.daemon_key_id = "daemon-key-2".into();
        assert!(answer_session_broker_hello_challenge(&producer, &request, &wrong_daemon).is_err());

        let mut expired = challenge.clone();
        expired.expires_at = fixture.now.saturating_sub(1);
        assert!(answer_session_broker_hello_challenge(&producer, &request, &expired).is_err());

        let mut tampered = challenge.clone();
        tampered.generation = "generation-2".into();
        assert!(answer_session_broker_hello_challenge(&producer, &request, &tampered).is_err());

        let mut bad_signature = challenge;
        bad_signature.daemon_signature = encode_base64_url(&[0; 64]);
        assert!(
            answer_session_broker_hello_challenge(&producer, &request, &bad_signature).is_err()
        );
    }

    #[test]
    fn verifies_real_producer_ack_and_rejects_mutation_or_extra_fields() {
        let fixture = fixture();
        let producer = options(
            &fixture,
            fixture.producer_grant.clone(),
            fixture.producer.clone(),
        );
        let (request, challenge) = issue(&fixture, &producer);
        let pending =
            answer_session_broker_hello_challenge(&producer, &request, &challenge).unwrap();
        let authenticated = fixture
            .authenticator
            .complete_producer_hello(
                serde_json::to_value(&pending.proof).unwrap(),
                "connection-1",
            )
            .unwrap();
        let ack = serde_json::to_value(&authenticated.ack).unwrap();
        assert_eq!(
            verify_producer_hello_ack(&pending, &ack).unwrap(),
            authenticated.ack
        );

        let mut extra = ack.clone();
        extra["extra"] = Value::Bool(true);
        assert!(verify_producer_hello_ack(&pending, &extra).is_err());

        for key in ["connectionId", "helloTranscriptHash", "daemonSignature"] {
            let mut mutated = ack.clone();
            mutated[key] = Value::String(if key == "connectionId" {
                "connection-2".into()
            } else if key == "helloTranscriptHash" {
                encode_base64_url(&[0; 32])
            } else {
                encode_base64_url(&[0; 64])
            });
            assert!(verify_producer_hello_ack(&pending, &mutated).is_err());
        }
    }
}
