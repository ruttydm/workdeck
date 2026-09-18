//! Explicit producer authentication. Candidate files never select their own trust root.
mod types;
use crate::*;
use base64::{Engine as _, engine::general_purpose};
use ed25519_dalek::{Signature, VerifyingKey};
use std::collections::BTreeSet;
pub use types::*;

fn blocked(message: &str) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
pub(crate) fn decode(value: &str) -> Result<Vec<u8>> {
    for engine in [
        general_purpose::STANDARD,
        general_purpose::URL_SAFE,
        general_purpose::STANDARD_NO_PAD,
        general_purpose::URL_SAFE_NO_PAD,
    ] {
        if let Ok(bytes) = engine.decode(value) {
            return Ok(bytes);
        }
    }
    Err(blocked("invalid base64 in producer authentication input"))
}
fn key(producer: &TrustedProducer) -> Result<VerifyingKey> {
    key_from_base64(&producer.public_key)
}
pub(crate) fn key_from_base64(value: &str) -> Result<VerifyingKey> {
    let bytes: [u8; 32] = decode(value)?
        .try_into()
        .map_err(|_| blocked("producer public key must contain 32 bytes"))?;
    let key =
        VerifyingKey::from_bytes(&bytes).map_err(|_| blocked("invalid Ed25519 public key"))?;
    if key.is_weak() {
        return Err(blocked("weak Ed25519 producer public key"));
    }
    Ok(key)
}
impl ProducerTrustPolicy {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_PRODUCER_POLICY_BYTES {
            return Err(blocked("producer policy exceeds 512 KiB"));
        }
        let policy: Self = serde_json::from_slice(bytes)
            .map_err(|e| blocked(&format!("invalid producer policy: {e}")))?;
        policy.fingerprint()?;
        Ok(policy)
    }
    pub fn fingerprint(&self) -> Result<ContentHash> {
        let value = serde_json::to_value(self).map_err(|e| blocked(&e.to_string()))?;
        if serde_json::to_vec(&value)
            .map_err(|e| blocked(&e.to_string()))?
            .len()
            > MAX_PRODUCER_POLICY_BYTES
            || self.producers.is_empty()
            || self.producers.len() > 16
        {
            return Err(blocked(
                "producer policy requires 1..16 producers within 512 KiB",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for producer in &self.producers {
            crate::commands::validation::id(&producer.id)?;
            if !ids.insert(&producer.id)
                || !keys.insert(key(producer)?.to_bytes())
                || producer.not_before >= producer.expires_at
                || producer.checks.is_empty()
                || producer.checks.len() > 256
            {
                return Err(blocked(
                    "producer identities, keys, validity interval or check scope are invalid",
                ));
            }
            for check in producer.checks.keys() {
                crate::commands::validation::id(check)?;
            }
        }
        crate::transactions::canonical_hash(&value)
    }
}
impl SignedCheckReport {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_SIGNED_REPORT_BYTES {
            return Err(blocked("signed report exceeds 96 MiB"));
        }
        let envelope: Self = serde_json::from_slice(bytes)
            .map_err(|e| blocked(&format!("invalid signed report: {e}")))?;
        envelope.payload_bytes()?;
        Ok(envelope)
    }
    fn payload_bytes(&self) -> Result<Vec<u8>> {
        if self.payload_type != CHECK_REPORT_PAYLOAD_TYPE
            || self.signatures.is_empty()
            || self.signatures.len() > 8
            || self.payload.len() > MAX_CHECK_REPORT_BYTES.div_ceil(3) * 4
            || self
                .signatures
                .iter()
                .any(|s| s.sig.len() > 128 || s.keyid.as_ref().is_some_and(|s| s.len() > 256))
        {
            return Err(blocked(
                "unsupported DSSE payload type, signature count or input bounds",
            ));
        }
        let payload = decode(&self.payload)?;
        if payload.len() > MAX_CHECK_REPORT_BYTES {
            return Err(blocked("signed check report payload exceeds 64 MiB"));
        }
        Ok(payload)
    }
}
/// DSSE pre-authentication encoding authenticates the exact payload bytes and type.
pub(crate) fn pae(payload_type: &str, payload: &[u8]) -> Vec<u8> {
    let mut message = format!(
        "DSSEv1 {} {} {} ",
        payload_type.len(),
        payload_type,
        payload.len()
    )
    .into_bytes();
    message.extend_from_slice(payload);
    message
}

pub fn authenticate_check_report(
    envelope: &SignedCheckReport,
    policy: &ProducerTrustPolicy,
    expected_policy: &ContentHash,
    expected_commit: &GitOid,
    now: Timestamp,
) -> Result<AuthenticatedCheckReport> {
    let policy_hash = policy.fingerprint()?;
    if &policy_hash != expected_policy {
        return Err(blocked(
            "producer policy differs from the independently supplied fingerprint",
        ));
    }
    let payload = envelope.payload_bytes()?;
    if payload
        .len()
        .checked_mul(envelope.signatures.len())
        .and_then(|bytes| bytes.checked_mul(policy.producers.len()))
        .is_none_or(|bytes| bytes > 512 * 1024 * 1024)
    {
        return Err(blocked(
            "DSSE verification workload exceeds 512 MiB of payload hashing",
        ));
    }
    let message = pae(&envelope.payload_type, &payload);
    let signatures: Vec<_> = envelope
        .signatures
        .iter()
        .map(|s| {
            let bytes = decode(&s.sig)?;
            Signature::from_slice(&bytes)
                .map_err(|_| blocked("Ed25519 signature must contain 64 bytes"))
        })
        .collect::<Result<_>>()?;
    // Ignore keyid: only cryptographic verification against the policy key grants attribution.
    let mut verified = Vec::new();
    for producer in &policy.producers {
        if now < producer.not_before || now >= producer.expires_at {
            continue;
        }
        let key = key(producer)?;
        if signatures
            .iter()
            .any(|signature| key.verify_strict(&message, signature).is_ok())
        {
            verified.push(producer);
        }
    }
    if verified.is_empty() {
        return Err(blocked(
            "no signature authenticates a currently admitted producer",
        ));
    }
    let report = CheckReport::from_json(&payload)?;
    let source = report
        .publication
        .intent
        .intent
        .revision
        .as_ref()
        .ok_or_else(|| blocked("producer admission requires revision-bound report inputs"))?
        .source
        .clone();
    if source.repository != policy.repository
        || &source.commit != expected_commit
        || report.observed_at > now
        || report.publication.result.result.finished_at > report.observed_at
    {
        return Err(blocked(
            "signed report repository, commit or observation time differs from admission",
        ));
    }
    let checks = &report.publication.result.result.checks;
    if checks.is_empty() {
        return Err(blocked(
            "producer admission requires nonempty check results",
        ));
    }
    let producer = verified
        .into_iter()
        .find(|producer| {
            report.observed_at >= producer.not_before
                && report.observed_at < producer.expires_at
                && checks.iter().all(|check| {
                    producer.checks.get(&check.check.id) == Some(&check.check.definition)
                })
        })
        .ok_or_else(|| {
            blocked("signed report falls outside producer check scope or validity interval")
        })?;
    Ok(AuthenticatedCheckReport {
        schema: SchemaVersion::CURRENT,
        basis: ProducerAuthenticationBasis::AuthenticatedProducer,
        policy: policy_hash,
        producer: ProducerRef {
            id: producer.id.clone(),
            definition: crate::transactions::canonical_hash(&serde_json::json!(producer))?,
        },
        payload: ContentHash::of(&payload),
        source,
        authenticated_at: now,
        report,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn dsse_reference_pae_vector() {
        assert_eq!(
            super::pae("http://example.com/HelloWorld", b"hello world"),
            b"DSSEv1 29 http://example.com/HelloWorld 11 hello world"
        );
    }
}
