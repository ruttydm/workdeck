//! Authentication authority, signed transcripts, and replay admission for the session broker.

use crate::{CanonicalJsonError, canonical_json_bytes};
use serde_json::{Map, Value};

pub const SESSION_BROKER_SIGNATURE_ALGORITHM: &str = "Ed25519";
pub const SESSION_BROKER_AUTH_DOMAIN: &str = "dev.workdeck.session-broker.v1";
pub const MAX_BROKER_IDENTIFIER_LENGTH: usize = 128;
pub const MAX_BROKER_COMMAND_SCOPES: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProducerOperation {
    Register,
    Reconnect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerOperation {
    List,
    Get,
    Dispatch,
    Diagnostics,
    Shutdown,
    CapabilityIssue,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerCommandScope {
    pub name: String,
    pub version: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerGrantBase {
    pub app_id: String,
    pub principal_id: String,
    pub key_id: String,
    pub grant_id: String,
    pub algorithm: String,
    pub issued_at: u64,
    pub expires_at: u64,
    pub revocation_id: String,
    pub may_delegate: bool,
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerGrant {
    pub base: BrokerGrantBase,
    pub operations: Vec<ProducerOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerGrant {
    pub base: BrokerGrantBase,
    pub operations: Vec<CallerOperation>,
    pub commands: Vec<BrokerCommandScope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerGrant {
    Producer(ProducerGrant),
    Caller(CallerGrant),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerPrincipal {
    pub app_id: String,
    pub principal_id: String,
    pub key_id: String,
    pub grant_id: String,
    pub session_id: Option<String>,
    pub scopes: Vec<ProducerOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerPrincipal {
    pub app_id: String,
    pub principal_id: String,
    pub key_id: String,
    pub grant_id: String,
    pub session_id: Option<String>,
    pub operations: Vec<CallerOperation>,
    pub commands: Vec<BrokerCommandScope>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerPrincipal {
    Producer(ProducerPrincipal),
    Caller(CallerPrincipal),
}

impl BrokerGrant {
    fn base(&self) -> &BrokerGrantBase {
        match self {
            Self::Producer(grant) => &grant.base,
            Self::Caller(grant) => &grant.base,
        }
    }
}

/// Consume a grant into the broker's immutable-by-default Rust authority model.
///
/// JavaScript callers needed a deep runtime freeze. Rust callers can only observe a grant through
/// an immutable borrow in all authorization APIs, while constructing a delegated child remains an
/// explicit owned clone.
#[must_use]
pub const fn freeze_broker_grant(grant: BrokerGrant) -> BrokerGrant {
    grant
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerHelloProposal {
    pub broker_revision: u32,
    pub app_revision: u32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedBrokerAppContract {
    pub app_revision: u32,
    pub features: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerRole {
    Producer,
    Caller,
}

impl BrokerRole {
    const fn wire_name(self) -> &'static str {
        match self {
            Self::Producer => "producer",
            Self::Caller => "caller",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerChallengeTranscriptInput {
    pub role: BrokerRole,
    pub app_id: String,
    pub generation: String,
    pub endpoint: String,
    pub key_id: String,
    pub grant_id: String,
    pub initiator_nonce: String,
    pub responder_nonce: String,
    pub proposal: BrokerHelloProposal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrokerHelloAckBinding {
    Producer {
        connection_id: String,
    },
    Caller {
        caller_session_id: String,
        initial_sequence: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerHelloAckTranscriptInput {
    pub app_id: String,
    pub generation: String,
    pub key_id: String,
    pub grant_id: String,
    pub hello_transcript_hash: String,
    pub selection: BrokerHelloProposal,
    pub binding: BrokerHelloAckBinding,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerRequestTranscriptInput {
    pub app_id: String,
    pub generation: String,
    pub caller_session_id: String,
    pub key_id: String,
    pub grant_id: String,
    pub hello_transcript_hash: String,
    pub method: String,
    pub target: String,
    pub body_digest: String,
    pub request_id: String,
    pub sequence: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerResponseTranscriptInput {
    pub app_id: String,
    pub generation: String,
    pub broker_revision: u32,
    pub caller_session_id: String,
    pub request_id: String,
    pub sequence: String,
    pub http_status: u16,
    pub body_digest: String,
    pub app_contract: Option<SignedBrokerAppContract>,
}

#[must_use]
pub fn is_valid_broker_app_id(value: &str) -> bool {
    bounded_identifier(value, true, |byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
    })
}

#[must_use]
pub fn is_valid_broker_identifier(value: &str) -> bool {
    bounded_identifier(value, false, |byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-')
    })
}

fn bounded_identifier(value: &str, lowercase: bool, allowed: impl Fn(u8) -> bool) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() || bytes.len() > MAX_BROKER_IDENTIFIER_LENGTH {
        return false;
    }
    let endpoint = |byte: u8| {
        if lowercase {
            byte.is_ascii_lowercase() || byte.is_ascii_digit()
        } else {
            byte.is_ascii_alphanumeric()
        }
    };
    endpoint(bytes[0]) && endpoint(bytes[bytes.len() - 1]) && bytes.iter().copied().all(allowed)
}

#[must_use]
pub const fn is_valid_broker_revision(value: u64) -> bool {
    value > 0 && value <= 9_007_199_254_740_991
}

#[must_use]
pub fn principal_from_grant(grant: &BrokerGrant) -> BrokerPrincipal {
    match grant {
        BrokerGrant::Producer(grant) => BrokerPrincipal::Producer(ProducerPrincipal {
            app_id: grant.base.app_id.clone(),
            principal_id: grant.base.principal_id.clone(),
            key_id: grant.base.key_id.clone(),
            grant_id: grant.base.grant_id.clone(),
            session_id: grant.base.session_id.clone(),
            scopes: grant.operations.clone(),
        }),
        BrokerGrant::Caller(grant) => BrokerPrincipal::Caller(CallerPrincipal {
            app_id: grant.base.app_id.clone(),
            principal_id: grant.base.principal_id.clone(),
            key_id: grant.base.key_id.clone(),
            grant_id: grant.base.grant_id.clone(),
            session_id: grant.base.session_id.clone(),
            operations: grant.operations.clone(),
            commands: grant.commands.clone(),
        }),
    }
}

#[must_use]
pub fn is_grant_active(
    grant: &BrokerGrant,
    app_id: &str,
    now: u64,
    is_revoked: impl FnOnce(&str) -> bool,
) -> bool {
    let base = grant.base();
    base.app_id == app_id
        && base.issued_at <= now
        && now < base.expires_at
        && !is_revoked(&base.revocation_id)
}

#[must_use]
pub fn is_grant_narrowing(parent: &BrokerGrant, child: &BrokerGrant) -> bool {
    let parent_base = parent.base();
    let child_base = child.base();
    if !parent_base.may_delegate
        || parent_base.app_id != child_base.app_id
        || child_base.issued_at < parent_base.issued_at
        || child_base.expires_at > parent_base.expires_at
        || parent_base
            .session_id
            .as_ref()
            .is_some_and(|session| child_base.session_id.as_ref() != Some(session))
    {
        return false;
    }
    match (parent, child) {
        (BrokerGrant::Producer(parent), BrokerGrant::Producer(child)) => child
            .operations
            .iter()
            .all(|operation| parent.operations.contains(operation)),
        (BrokerGrant::Caller(parent), BrokerGrant::Caller(child)) => {
            child
                .operations
                .iter()
                .all(|operation| parent.operations.contains(operation))
                && child
                    .commands
                    .iter()
                    .all(|scope| parent.commands.contains(scope))
        }
        _ => false,
    }
}

#[must_use]
pub fn producer_principal_allows(
    principal: &ProducerPrincipal,
    app_id: &str,
    operation: ProducerOperation,
    session_id: Option<&str>,
) -> bool {
    principal.app_id == app_id
        && principal.scopes.contains(&operation)
        && principal
            .session_id
            .as_deref()
            .is_none_or(|bound| Some(bound) == session_id)
}

#[must_use]
pub fn caller_principal_allows(
    principal: &CallerPrincipal,
    app_id: &str,
    operation: CallerOperation,
    session_id: Option<&str>,
    command: Option<(&str, u64)>,
) -> bool {
    if principal.app_id != app_id
        || !principal.operations.contains(&operation)
        || principal
            .session_id
            .as_deref()
            .is_some_and(|bound| Some(bound) != session_id)
    {
        return false;
    }
    if operation != CallerOperation::Dispatch {
        return true;
    }
    command.is_some_and(|(name, version)| {
        principal
            .commands
            .iter()
            .any(|scope| scope.name == name && scope.version == version)
    })
}

pub fn build_broker_challenge_transcript(
    input: &BrokerChallengeTranscriptInput,
) -> Result<Vec<u8>, CanonicalJsonError> {
    let mut features = input.proposal.features.clone();
    features.sort();
    canonical_json_bytes(&serde_json::json!({
        "appId": input.app_id,
        "domain": format!("{SESSION_BROKER_AUTH_DOMAIN}/{}-hello", input.role.wire_name()),
        "endpoint": input.endpoint,
        "generation": input.generation,
        "grantId": input.grant_id,
        "initiatorNonce": input.initiator_nonce,
        "keyId": input.key_id,
        "proposal": {
            "appRevision": input.proposal.app_revision,
            "brokerRevision": input.proposal.broker_revision,
            "features": features,
        },
        "responderNonce": input.responder_nonce,
    }))
}

pub fn build_broker_hello_ack_transcript(
    input: &BrokerHelloAckTranscriptInput,
) -> Result<Vec<u8>, CanonicalJsonError> {
    let role = match &input.binding {
        BrokerHelloAckBinding::Producer { .. } => BrokerRole::Producer,
        BrokerHelloAckBinding::Caller {
            caller_session_id: _,
            initial_sequence: _,
        } => BrokerRole::Caller,
    };
    let mut features = input.selection.features.clone();
    features.sort();
    let mut record = Map::new();
    record.insert("appId".into(), input.app_id.clone().into());
    match &input.binding {
        BrokerHelloAckBinding::Producer { connection_id } => {
            record.insert("connectionId".into(), connection_id.clone().into());
        }
        BrokerHelloAckBinding::Caller {
            caller_session_id,
            initial_sequence,
        } => {
            record.insert("callerSessionId".into(), caller_session_id.clone().into());
            record.insert("initialSequence".into(), initial_sequence.clone().into());
        }
    }
    record.insert(
        "domain".into(),
        format!(
            "{SESSION_BROKER_AUTH_DOMAIN}/{}-hello-ack",
            role.wire_name()
        )
        .into(),
    );
    record.insert("generation".into(), input.generation.clone().into());
    record.insert("grantId".into(), input.grant_id.clone().into());
    record.insert(
        "helloTranscriptHash".into(),
        input.hello_transcript_hash.clone().into(),
    );
    record.insert("keyId".into(), input.key_id.clone().into());
    record.insert(
        "selection".into(),
        serde_json::json!({
            "appRevision": input.selection.app_revision,
            "brokerRevision": input.selection.broker_revision,
            "features": features,
        }),
    );
    canonical_json_bytes(&Value::Object(record))
}

pub fn build_caller_request_transcript(
    input: &CallerRequestTranscriptInput,
) -> Result<Vec<u8>, CanonicalJsonError> {
    canonical_json_bytes(&serde_json::json!({
        "appId": input.app_id,
        "bodyDigest": input.body_digest,
        "callerSessionId": input.caller_session_id,
        "domain": format!("{SESSION_BROKER_AUTH_DOMAIN}/caller-request"),
        "generation": input.generation,
        "grantId": input.grant_id,
        "helloTranscriptHash": input.hello_transcript_hash,
        "keyId": input.key_id,
        "method": input.method.to_uppercase(),
        "requestId": input.request_id,
        "sequence": input.sequence,
        "target": input.target,
    }))
}

pub fn build_broker_response_transcript(
    input: &BrokerResponseTranscriptInput,
) -> Result<Vec<u8>, CanonicalJsonError> {
    let mut record = Map::new();
    if let Some(contract) = &input.app_contract {
        let mut features = contract.features.clone();
        features.sort();
        record.insert(
            "appContract".into(),
            serde_json::json!({
                "appRevision": contract.app_revision,
                "features": features,
            }),
        );
    }
    record.insert("appId".into(), input.app_id.clone().into());
    record.insert("bodyDigest".into(), input.body_digest.clone().into());
    record.insert("brokerRevision".into(), input.broker_revision.into());
    record.insert(
        "callerSessionId".into(),
        input.caller_session_id.clone().into(),
    );
    record.insert(
        "domain".into(),
        format!("{SESSION_BROKER_AUTH_DOMAIN}/caller-response").into(),
    );
    record.insert("generation".into(), input.generation.clone().into());
    record.insert("httpStatus".into(), input.http_status.into());
    record.insert("requestId".into(), input.request_id.clone().into());
    record.insert("sequence".into(), input.sequence.clone().into());
    canonical_json_bytes(&Value::Object(record))
}

#[must_use]
pub fn parse_caller_sequence(value: &str) -> Option<u64> {
    if value == "0" {
        return Some(0);
    }
    if value.is_empty()
        || value.len() > 20
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    value.parse().ok()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerSequenceAllocator {
    next: Option<u64>,
}

impl CallerSequenceAllocator {
    pub fn new(initial_next: u64) -> Result<Self, &'static str> {
        if initial_next == 0 {
            return Err("Invalid initial caller sequence.");
        }
        Ok(Self {
            next: Some(initial_next),
        })
    }

    pub fn allocate(&mut self) -> Option<String> {
        let allocated = self.next?;
        self.next = allocated.checked_add(1);
        Some(allocated.to_string())
    }
}

impl Default for CallerSequenceAllocator {
    fn default() -> Self {
        Self { next: Some(1) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerSequenceAdmission {
    Accepted,
    Zero,
    Duplicate,
    TooOld,
    TooFarAhead,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallerSequenceReplayWindow {
    highest: u64,
    bitmap: u64,
}

impl CallerSequenceReplayWindow {
    #[must_use]
    pub const fn with_state(highest: u64, bitmap: u64) -> Self {
        Self { highest, bitmap }
    }

    pub fn admit(&mut self, canonical_sequence: &str) -> CallerSequenceAdmission {
        let Some(sequence) = parse_caller_sequence(canonical_sequence) else {
            return CallerSequenceAdmission::Invalid;
        };
        if sequence == 0 {
            return CallerSequenceAdmission::Zero;
        }
        if sequence <= self.highest {
            let distance = self.highest - sequence;
            if distance >= 64 {
                return CallerSequenceAdmission::TooOld;
            }
            let bit = 1_u64 << distance;
            if self.bitmap & bit != 0 {
                return CallerSequenceAdmission::Duplicate;
            }
            self.bitmap |= bit;
            return CallerSequenceAdmission::Accepted;
        }
        let delta = sequence - self.highest;
        if delta > 64 {
            return CallerSequenceAdmission::TooFarAhead;
        }
        self.bitmap = if delta == 64 { 0 } else { self.bitmap << delta };
        self.highest = sequence;
        self.bitmap |= 1;
        CallerSequenceAdmission::Accepted
    }

    #[must_use]
    pub fn snapshot(&self) -> CallerSequenceReplaySnapshot {
        CallerSequenceReplaySnapshot {
            highest: self.highest.to_string(),
            bitmap: format!("{:x}", self.bitmap),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerSequenceReplaySnapshot {
    pub highest: String,
    pub bitmap: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caller_grant() -> BrokerGrant {
        BrokerGrant::Caller(CallerGrant {
            base: BrokerGrantBase {
                app_id: "dev.example".into(),
                principal_id: "caller-1".into(),
                key_id: "caller-key-1".into(),
                grant_id: "caller-grant-1".into(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: 1_000,
                expires_at: 2_000,
                revocation_id: "revoke-1".into(),
                may_delegate: false,
                session_id: None,
            },
            operations: vec![CallerOperation::Get, CallerOperation::Dispatch],
            commands: vec![BrokerCommandScope {
                name: "review".into(),
                version: 1,
            }],
        })
    }

    #[test]
    fn builds_golden_canonical_domain_separated_transcripts() {
        let challenge = build_broker_challenge_transcript(&BrokerChallengeTranscriptInput {
            role: BrokerRole::Caller,
            app_id: "dev.example".into(),
            generation: "generation-1".into(),
            endpoint: "http://127.0.0.1:47657/broker".into(),
            key_id: "caller-key-1".into(),
            grant_id: "caller-grant-1".into(),
            initiator_nonce: "nonce-a".into(),
            responder_nonce: "nonce-b".into(),
            proposal: BrokerHelloProposal {
                broker_revision: 1,
                app_revision: 7,
                features: vec!["z".into(), "a".into()],
            },
        })
        .unwrap();
        assert_eq!(
            String::from_utf8(challenge).unwrap(),
            "{\"appId\":\"dev.example\",\"domain\":\"dev.workdeck.session-broker.v1/caller-hello\",\"endpoint\":\"http://127.0.0.1:47657/broker\",\"generation\":\"generation-1\",\"grantId\":\"caller-grant-1\",\"initiatorNonce\":\"nonce-a\",\"keyId\":\"caller-key-1\",\"proposal\":{\"appRevision\":7,\"brokerRevision\":1,\"features\":[\"a\",\"z\"]},\"responderNonce\":\"nonce-b\"}"
        );

        let ack = build_broker_hello_ack_transcript(&BrokerHelloAckTranscriptInput {
            app_id: "dev.example".into(),
            generation: "generation-1".into(),
            key_id: "producer-key-1".into(),
            grant_id: "producer-grant-1".into(),
            hello_transcript_hash: "hello-hash".into(),
            selection: BrokerHelloProposal {
                broker_revision: 1,
                app_revision: 7,
                features: vec![],
            },
            binding: BrokerHelloAckBinding::Producer {
                connection_id: "connection-1".into(),
            },
        })
        .unwrap();
        assert_eq!(
            String::from_utf8(ack).unwrap(),
            "{\"appId\":\"dev.example\",\"connectionId\":\"connection-1\",\"domain\":\"dev.workdeck.session-broker.v1/producer-hello-ack\",\"generation\":\"generation-1\",\"grantId\":\"producer-grant-1\",\"helloTranscriptHash\":\"hello-hash\",\"keyId\":\"producer-key-1\",\"selection\":{\"appRevision\":7,\"brokerRevision\":1,\"features\":[]}}"
        );

        let response = build_broker_response_transcript(&BrokerResponseTranscriptInput {
            app_id: "dev.example".into(),
            generation: "generation-1".into(),
            broker_revision: 1,
            app_contract: Some(SignedBrokerAppContract {
                app_revision: 7,
                features: vec![],
            }),
            caller_session_id: "caller-session-1".into(),
            request_id: "request-1".into(),
            sequence: "1".into(),
            http_status: 200,
            body_digest: "body-hash".into(),
        })
        .unwrap();
        assert_eq!(
            String::from_utf8(response).unwrap(),
            "{\"appContract\":{\"appRevision\":7,\"features\":[]},\"appId\":\"dev.example\",\"bodyDigest\":\"body-hash\",\"brokerRevision\":1,\"callerSessionId\":\"caller-session-1\",\"domain\":\"dev.workdeck.session-broker.v1/caller-response\",\"generation\":\"generation-1\",\"httpStatus\":200,\"requestId\":\"request-1\",\"sequence\":\"1\"}"
        );

        let request = build_caller_request_transcript(&CallerRequestTranscriptInput {
            app_id: "dev.example".into(),
            generation: "generation-1".into(),
            caller_session_id: "caller-session-1".into(),
            key_id: "caller-key-1".into(),
            grant_id: "caller-grant-1".into(),
            hello_transcript_hash: "hello-hash".into(),
            method: "post".into(),
            target: "/broker?a=1&b=2".into(),
            body_digest: "body-hash".into(),
            request_id: "request-1".into(),
            sequence: "1".into(),
        })
        .unwrap();
        assert_eq!(
            String::from_utf8(request).unwrap(),
            "{\"appId\":\"dev.example\",\"bodyDigest\":\"body-hash\",\"callerSessionId\":\"caller-session-1\",\"domain\":\"dev.workdeck.session-broker.v1/caller-request\",\"generation\":\"generation-1\",\"grantId\":\"caller-grant-1\",\"helloTranscriptHash\":\"hello-hash\",\"keyId\":\"caller-key-1\",\"method\":\"POST\",\"requestId\":\"request-1\",\"sequence\":\"1\",\"target\":\"/broker?a=1&b=2\"}"
        );
    }

    #[test]
    fn enforces_expiry_revocation_delegation_and_scoped_authority() {
        let grant = freeze_broker_grant(caller_grant());
        assert!(is_grant_active(&grant, "dev.example", 1_500, |_| false));
        assert!(!is_grant_active(&grant, "wrong.app", 1_500, |_| false));
        assert!(!is_grant_active(&grant, "dev.example", 2_000, |_| false));
        assert!(!is_grant_active(&grant, "dev.example", 1_500, |id| id == "revoke-1"));

        let BrokerGrant::Caller(mut parent) = grant.clone() else {
            unreachable!();
        };
        parent.base.may_delegate = true;
        let mut child = parent.clone();
        child.base.key_id = "delegated-key".into();
        child.base.grant_id = "delegated-grant".into();
        child.base.expires_at = 1_900;
        child.operations = vec![CallerOperation::Get];
        child.commands.clear();
        assert!(is_grant_narrowing(
            &BrokerGrant::Caller(parent.clone()),
            &BrokerGrant::Caller(child)
        ));
        let mut overbroad = parent;
        overbroad.operations = vec![CallerOperation::List];
        overbroad.base.expires_at = 1_900;
        assert!(!is_grant_narrowing(
            &grant_with_delegation(grant.clone()),
            &BrokerGrant::Caller(overbroad)
        ));

        let BrokerPrincipal::Caller(principal) = principal_from_grant(&grant) else {
            unreachable!();
        };
        assert!(caller_principal_allows(
            &principal,
            "dev.example",
            CallerOperation::Dispatch,
            None,
            Some(("review", 1))
        ));
        assert!(!caller_principal_allows(
            &principal,
            "dev.example",
            CallerOperation::Dispatch,
            None,
            Some(("review", 2))
        ));
        assert!(!caller_principal_allows(
            &principal,
            "dev.example",
            CallerOperation::List,
            None,
            None
        ));

        let producer = BrokerGrant::Producer(ProducerGrant {
            base: BrokerGrantBase {
                app_id: "dev.example".into(),
                principal_id: "producer-1".into(),
                key_id: "producer-key-1".into(),
                grant_id: "producer-grant-1".into(),
                algorithm: SESSION_BROKER_SIGNATURE_ALGORITHM.into(),
                issued_at: 1_000,
                expires_at: 2_000,
                revocation_id: "producer-revocation-1".into(),
                may_delegate: false,
                session_id: Some("session-1".into()),
            },
            operations: vec![ProducerOperation::Register],
        });
        let BrokerPrincipal::Producer(principal) = principal_from_grant(&producer) else {
            unreachable!();
        };
        assert!(producer_principal_allows(
            &principal,
            "dev.example",
            ProducerOperation::Register,
            Some("session-1")
        ));
        assert!(!producer_principal_allows(
            &principal,
            "dev.example",
            ProducerOperation::Reconnect,
            Some("session-1")
        ));
    }

    fn grant_with_delegation(grant: BrokerGrant) -> BrokerGrant {
        match grant {
            BrokerGrant::Caller(mut grant) => {
                grant.base.may_delegate = true;
                BrokerGrant::Caller(grant)
            }
            other => other,
        }
    }

    #[test]
    fn rejects_noncanonical_overflow_duplicate_and_old_sequences() {
        assert_eq!(parse_caller_sequence(&u64::MAX.to_string()), Some(u64::MAX));
        assert_eq!(parse_caller_sequence("18446744073709551616"), None);
        assert_eq!(parse_caller_sequence("01"), None);
        let mut replay = CallerSequenceReplayWindow::default();
        assert_eq!(replay.admit("0"), CallerSequenceAdmission::Zero);
        assert_eq!(replay.admit("1"), CallerSequenceAdmission::Accepted);
        assert_eq!(replay.admit("1"), CallerSequenceAdmission::Duplicate);
        assert_eq!(replay.admit("65"), CallerSequenceAdmission::Accepted);
        assert_eq!(replay.admit("1"), CallerSequenceAdmission::TooOld);
    }

    #[test]
    fn accepts_out_of_order_and_exact_window_edges() {
        let mut replay = CallerSequenceReplayWindow::default();
        assert_eq!(replay.admit("4"), CallerSequenceAdmission::Accepted);
        assert_eq!(replay.admit("2"), CallerSequenceAdmission::Accepted);
        assert_eq!(replay.admit("3"), CallerSequenceAdmission::Accepted);
        assert_eq!(replay.admit("68"), CallerSequenceAdmission::Accepted);
        assert_eq!(replay.admit("133"), CallerSequenceAdmission::TooFarAhead);
        assert_eq!(replay.admit("132"), CallerSequenceAdmission::Accepted);
    }

    #[test]
    fn handles_uint64_exhaustion_without_wrapping() {
        let mut allocator = CallerSequenceAllocator::new(u64::MAX).unwrap();
        assert_eq!(allocator.allocate(), Some(u64::MAX.to_string()));
        assert_eq!(allocator.allocate(), None);

        let mut replay = CallerSequenceReplayWindow::default();
        assert_eq!(
            replay.admit(&(u64::MAX - 64).to_string()),
            CallerSequenceAdmission::TooFarAhead
        );
        let mut near_max = CallerSequenceReplayWindow::with_state(u64::MAX - 64, 1);
        assert_eq!(
            near_max.admit(&u64::MAX.to_string()),
            CallerSequenceAdmission::Accepted
        );
        assert_eq!(
            near_max.admit(&u64::MAX.to_string()),
            CallerSequenceAdmission::Duplicate
        );
        assert_eq!(
            near_max.admit("18446744073709551616"),
            CallerSequenceAdmission::Invalid
        );
    }

    #[test]
    fn enforces_app_and_opaque_identifier_grammars() {
        for accepted in ["dev.hunk", "a", "a_b.c-d"] {
            assert!(is_valid_broker_app_id(accepted));
        }
        for rejected in ["Dev.hunk", ".dev", "dev.", "dev/hunk", ""] {
            assert!(!is_valid_broker_app_id(rejected));
        }
        for accepted in ["session-1", "A_B.C~d", "z"] {
            assert!(is_valid_broker_identifier(accepted));
        }
        for rejected in ["~session", "session~", "session/id", ""] {
            assert!(!is_valid_broker_identifier(rejected));
        }
    }
}
