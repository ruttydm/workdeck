use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CHECK_REPORT_PAYLOAD_TYPE: &str = "application/vnd.workdeck.check-report.v1+json";
pub const MAX_PRODUCER_POLICY_BYTES: usize = 512 * 1024;
pub const MAX_SIGNED_REPORT_BYTES: usize = 96 * 1024 * 1024;

/// Supplied by the admitting environment and pinned separately from candidate data.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerTrustPolicy {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub producers: Vec<TrustedProducer>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedProducer {
    pub id: String,
    /// Base64-encoded 32-byte Ed25519 public key. No private keys are accepted.
    pub public_key: String,
    /// Exact allowed check definition fingerprints; no wildcard or name-only grants.
    pub checks: BTreeMap<String, ContentHash>,
    pub not_before: Timestamp,
    pub expires_at: Timestamp,
}
/// DSSE JSON envelope. keyid is an unauthenticated hint, never authority.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedCheckReport {
    #[serde(rename = "payloadType")]
    pub payload_type: String,
    pub payload: String,
    pub signatures: Vec<ReportSignature>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReportSignature {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keyid: Option<String>,
    pub sig: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProducerAuthenticationBasis {
    AuthenticatedProducer,
}
/// Authentication under a caller-pinned policy, not a completion qualification.
/// Consumers must reverify the envelope and policy; this serialized result is not a credential.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedCheckReport {
    pub schema: SchemaVersion,
    pub basis: ProducerAuthenticationBasis,
    pub policy: ContentHash,
    pub producer: ProducerRef,
    pub payload: ContentHash,
    pub source: CiSourceIdentity,
    pub authenticated_at: Timestamp,
    pub report: CheckReport,
}
