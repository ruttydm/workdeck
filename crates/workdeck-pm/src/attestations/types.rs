use crate::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_IMPORTED_REPORT_BYTES: usize = 8 * 1024 * 1024;
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportCheckReportRequest {
    /// Original DSSE envelope bytes as UTF-8, retained without rewriting.
    pub envelope: String,
    pub policy: ProducerTrustPolicy,
    pub expected_policy: ContentHash,
    pub expected_commit: GitOid,
    pub actor: String,
    /// Optional original red/green proof; retained history is not current qualification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_green: Option<RetainedRedGreenProof>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedReportAdmission {
    pub basis: ProducerAuthenticationBasis,
    pub producer: ProducerRef,
    pub payload: ContentHash,
    pub report: ContentHash,
    pub source: CiSourceIdentity,
    pub historical_state: RunState,
    pub observed_state: RunState,
}
impl From<&AuthenticatedCheckReport> for ImportedReportAdmission {
    fn from(value: &AuthenticatedCheckReport) -> Self {
        Self {
            basis: value.basis,
            producer: value.producer.clone(),
            payload: value.payload.clone(),
            report: value.report.fingerprint.clone(),
            source: value.source.clone(),
            historical_state: value.report.observation.historical_state,
            observed_state: value.report.observation.state,
        }
    }
}
/// Historical import attribution. Current admission requires an external policy pin.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedCheckReport {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: AttestationId,
    pub request_id: RequestId,
    pub imported_at: Timestamp,
    pub input: ImportCheckReportRequest,
    pub admission: ImportedReportAdmission,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedCheckReportRecord {
    pub record: ImportedCheckReport,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}

/// Bounded discovery metadata; retrieve the record by ID for its full signed proof.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedCheckReportSummary {
    pub id: AttestationId,
    pub repository: RepositoryId,
    pub request_id: RequestId,
    pub imported_at: Timestamp,
    pub actor: String,
    pub policy: ContentHash,
    pub admission: ImportedReportAdmission,
    pub path: PathBuf,
    pub content: ContentHash,
}
impl From<ImportedCheckReportRecord> for ImportedCheckReportSummary {
    fn from(value: ImportedCheckReportRecord) -> Self {
        Self {
            id: value.record.id,
            repository: value.record.repository,
            request_id: value.record.request_id,
            imported_at: value.record.imported_at,
            actor: value.record.input.actor,
            policy: value.record.input.expected_policy,
            admission: value.record.admission,
            path: value.path,
            content: value.content,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedRedGreenProof {
    pub baseline: CiBaselinePin,
    pub check: String,
    pub red: SignedCheckReport,
    pub red_artifact: String,
    pub green_artifact: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<RedGreenBaselineReview>,
}

/// All trust roots are supplied afresh, outside the retained import.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedRedGreenAuthority {
    pub baseline: CiBaselinePin,
    pub candidate: GitOid,
    pub policy: ProducerTrustPolicy,
    pub expected_policy: ContentHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewCoverageAuthority>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedRedGreenAssessment {
    pub pair: RedGreenAssessment,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<AuthenticatedContractReview>,
}

/// Fresh authority and exact selection for a current source-bound check result.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyImportedCheck {
    pub attestation: AttestationId,
    pub expected_attestation: ContentHash,
    pub check: String,
    pub candidate: GitOid,
    pub policy: ProducerTrustPolicy,
    pub expected_policy: ContentHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_green: Option<RetainedRedGreenAuthority>,
}
