use crate::*;
use serde::{Deserialize, Serialize};

pub const CONTRACT_REVIEW_PAYLOAD_TYPE: &str = "application/vnd.workdeck.contract-review.v1+json";
pub const MAX_CONTRACT_REVIEW_BYTES: usize = 256 * 1024;
pub const MAX_CONTRACT_REVIEW_POLICY_BYTES: usize = 64 * 1024;

/// Independently selected policy; every listed reviewer must sign the same approval.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractReviewPolicy {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub required_reviewers: Vec<ContractReviewer>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContractReviewer {
    pub id: String,
    pub public_key: String,
    pub not_before: Timestamp,
    pub expires_at: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractReviewDecision {
    Approve,
    RequestChanges,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiContractApproval {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub baseline: CiBaselinePin,
    pub head: CiSourceIdentity,
    pub head_contract: ContentHash,
    pub decision: ContractReviewDecision,
    pub reviewed_at: Timestamp,
    pub expires_at: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedContractReview {
    #[serde(rename = "payloadType")]
    pub payload_type: String,
    pub payload: String,
    pub signatures: Vec<ReportSignature>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContractReviewBasis {
    AuthenticatedContractReview,
}
/// Observation only. Reverify original signed bytes and current policy for future admission.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthenticatedContractReview {
    pub basis: ContractReviewBasis,
    pub policy: ContentHash,
    pub payload: ContentHash,
    pub reviewers: Vec<String>,
    pub authenticated_at: Timestamp,
    pub approval: CiContractApproval,
}
#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiReviewedValidation {
    pub validation: CiValidationReport,
    pub admission: AuthenticatedContractReview,
    /// Planning and required contract review only; not passing checks or completion.
    pub valid: bool,
}
