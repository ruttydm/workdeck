//! Authenticated contract review is separate from check-producer authentication.
mod types;
use crate::*;
use ed25519_dalek::Signature;
use std::{collections::BTreeSet, path::Path};
pub use types::*;

fn blocked(message: &str) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
impl ContractReviewPolicy {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_CONTRACT_REVIEW_POLICY_BYTES {
            return Err(blocked("contract review policy exceeds 64 KiB"));
        }
        let policy: Self = serde_json::from_slice(bytes)
            .map_err(|e| blocked(&format!("invalid contract review policy: {e}")))?;
        policy.fingerprint()?;
        Ok(policy)
    }
    pub fn fingerprint(&self) -> Result<ContentHash> {
        let bytes = serde_json::to_vec(self).map_err(|e| blocked(&e.to_string()))?;
        if bytes.len() > MAX_CONTRACT_REVIEW_POLICY_BYTES
            || self.required_reviewers.is_empty()
            || self.required_reviewers.len() > 16
        {
            return Err(blocked(
                "review policy requires 1..16 reviewers within 64 KiB",
            ));
        }
        let mut ids = BTreeSet::new();
        let mut keys = BTreeSet::new();
        for reviewer in &self.required_reviewers {
            crate::commands::validation::id(&reviewer.id)?;
            if !ids.insert(&reviewer.id)
                || !keys.insert(crate::producers::key_from_base64(&reviewer.public_key)?.to_bytes())
                || reviewer.not_before >= reviewer.expires_at
            {
                return Err(blocked(
                    "duplicate reviewer identity/key or invalid validity interval",
                ));
            }
        }
        crate::transactions::canonical_hash(
            &serde_json::to_value(self).map_err(|e| blocked(&e.to_string()))?,
        )
    }
}
impl SignedContractReview {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > MAX_CONTRACT_REVIEW_BYTES {
            return Err(blocked("signed contract review exceeds 256 KiB"));
        }
        let envelope: Self = serde_json::from_slice(bytes)
            .map_err(|e| blocked(&format!("invalid signed contract review: {e}")))?;
        envelope.payload_bytes()?;
        Ok(envelope)
    }
    fn payload_bytes(&self) -> Result<Vec<u8>> {
        if self.payload_type != CONTRACT_REVIEW_PAYLOAD_TYPE
            || self.payload.len() > 128 * 1024
            || self.signatures.is_empty()
            || self.signatures.len() > 16
            || self
                .signatures
                .iter()
                .any(|s| s.sig.len() > 128 || s.keyid.as_ref().is_some_and(|id| id.len() > 256))
        {
            return Err(blocked(
                "unsupported contract review payload type or input bounds",
            ));
        }
        let payload = crate::producers::decode(&self.payload)?;
        if payload.len() > 64 * 1024 {
            return Err(blocked("contract review payload exceeds 64 KiB"));
        }
        Ok(payload)
    }
}

/// Always capture current immutable sources and verify original signatures afresh.
/// Baseline and policy pins must be supplied independently of candidate files.
pub fn ci_validate_reviewed(
    worktree: &Path,
    request: &CiValidateRequest,
    baseline: &CiBaselinePin,
    policy: &ContractReviewPolicy,
    expected_policy: &ContentHash,
    envelope: &SignedContractReview,
    now: Timestamp,
) -> Result<CiReviewedValidation> {
    let admission = authenticate(envelope, policy, expected_policy, baseline, now)?;
    let approval = &admission.approval;
    let validation = ci_validate_pinned(worktree, request, baseline)?;
    if approval.head != validation.head
        || approval.repository != validation.head.repository
        || validation
            .contracts
            .head
            .as_ref()
            .map(|head| &head.fingerprint)
            != Some(&approval.head_contract)
    {
        return Err(blocked(
            "contract review does not match the exact candidate source and contract",
        ));
    }
    let valid = validation.base_report.valid
        && validation.head_report.valid
        && validation.head_report.policy_compliant
        && validation.contracts.errors.is_empty();
    Ok(CiReviewedValidation {
        valid,
        validation,
        admission,
    })
}

// Historical readers verify exact retained proof at its original admission time.
// This is not a current candidate/structural validation API.
pub(crate) fn authenticate(
    envelope: &SignedContractReview,
    policy: &ContractReviewPolicy,
    expected_policy: &ContentHash,
    baseline: &CiBaselinePin,
    now: Timestamp,
) -> Result<AuthenticatedContractReview> {
    let policy_hash = policy.fingerprint()?;
    if &policy_hash != expected_policy {
        return Err(blocked(
            "review policy differs from independently supplied fingerprint",
        ));
    }
    let payload = envelope.payload_bytes()?;
    let message = crate::producers::pae(&envelope.payload_type, &payload);
    let signatures = envelope
        .signatures
        .iter()
        .map(|signature| {
            Signature::from_slice(&crate::producers::decode(&signature.sig)?)
                .map_err(|_| blocked("review signature must contain 64 bytes"))
        })
        .collect::<Result<Vec<_>>>()?;
    // Policy IDs, not keyid hints, identify the authenticated required reviewers.
    for reviewer in &policy.required_reviewers {
        if now < reviewer.not_before || now >= reviewer.expires_at {
            return Err(blocked(
                "required reviewer is outside its current validity interval",
            ));
        }
        let key = crate::producers::key_from_base64(&reviewer.public_key)?;
        if !signatures
            .iter()
            .any(|signature| key.verify_strict(&message, signature).is_ok())
        {
            return Err(blocked(
                "missing valid signature from a required contract reviewer",
            ));
        }
    }
    let approval: CiContractApproval = serde_json::from_slice(&payload)
        .map_err(|e| blocked(&format!("invalid authenticated approval payload: {e}")))?;
    if approval.decision != ContractReviewDecision::Approve
        || approval.repository != policy.repository
        || approval.head.repository != approval.repository
        || &approval.baseline != baseline
        || approval.reviewed_at > now
        || approval.expires_at <= now
        || approval.reviewed_at >= approval.expires_at
        || policy
            .required_reviewers
            .iter()
            .any(|r| approval.reviewed_at < r.not_before || approval.reviewed_at >= r.expires_at)
    {
        return Err(blocked(
            "review decision, repository, baseline or validity does not admit approval",
        ));
    }
    Ok(AuthenticatedContractReview {
        basis: ContractReviewBasis::AuthenticatedContractReview,
        policy: policy_hash,
        payload: ContentHash::of(&payload),
        reviewers: policy
            .required_reviewers
            .iter()
            .map(|r| r.id.clone())
            .collect(),
        authenticated_at: now,
        approval,
    })
}
