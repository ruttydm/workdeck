use crate::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub const MAX_IMPORTED_REVIEW_BYTES: usize = 1024 * 1024;
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportContractReviewRequest {
    pub envelope: String,
    pub policy: ContractReviewPolicy,
    pub expected_policy: ContentHash,
    pub baseline: CiBaselinePin,
    pub expected_commit: GitOid,
    pub actor: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedContractReview {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: ContractReviewId,
    pub request_id: RequestId,
    pub imported_at: Timestamp,
    pub input: ImportContractReviewRequest,
    pub admission: AuthenticatedContractReview,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedContractReviewRecord {
    pub record: ImportedContractReview,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedContractReviewSummary {
    pub id: ContractReviewId,
    pub repository: RepositoryId,
    pub request_id: RequestId,
    pub imported_at: Timestamp,
    pub actor: String,
    pub admission: AuthenticatedContractReview,
    pub path: PathBuf,
    pub content: ContentHash,
}
impl From<ImportedContractReviewRecord> for ImportedContractReviewSummary {
    fn from(value: ImportedContractReviewRecord) -> Self {
        Self {
            id: value.record.id,
            repository: value.record.repository,
            request_id: value.record.request_id,
            imported_at: value.record.imported_at,
            actor: value.record.input.actor,
            admission: value.record.admission,
            path: value.path,
            content: value.content,
        }
    }
}
