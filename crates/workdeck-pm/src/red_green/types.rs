use crate::*;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, path::PathBuf};

/// Part of the independently accepted check definition, never supplied by a result.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenRequirement {
    pub red_exit_codes: Vec<i32>,
    pub cases: Vec<JUnitCaseIdentity>,
}
impl RedGreenRequirement {
    pub(crate) fn validate(&self, expectation: &ReportExpectation) -> Result<()> {
        let invalid = |message: &str| PmError::new(ErrorCode::InvalidSchema, message);
        let ReportExpectation::JUnit { suites, .. } = expectation else {
            return Err(invalid("red/green requires a JUnit check contract"));
        };
        if self.red_exit_codes.is_empty()
            || self.red_exit_codes.len() > 16
            || self
                .red_exit_codes
                .iter()
                .any(|code| !(0..=255).contains(code))
            || self.red_exit_codes.iter().collect::<BTreeSet<_>>().len()
                != self.red_exit_codes.len()
            || self.cases.is_empty()
            || self.cases.len() > 128
            || self.cases.iter().collect::<BTreeSet<_>>().len() != self.cases.len()
        {
            return Err(invalid(
                "red/green requires unique bounded exit codes and 1..128 required cases",
            ));
        }
        for case in &self.cases {
            case.validate().map_err(|error| invalid(&error.message))?;
            if !case.suites.iter().any(|name| suites.contains(name)) {
                return Err(invalid(
                    "red/green case must belong to a required JUnit suite",
                ));
            }
        }
        Ok(())
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenRequest {
    pub baseline: CiBaselinePin,
    pub candidate: GitOid,
    pub check: String,
    pub producer_policy: ProducerTrustPolicy,
    pub expected_producer_policy: ContentHash,
    pub red: SignedCheckReport,
    pub green: SignedCheckReport,
    /// Exact UTF-8 artifacts, independently matched to signed descriptors.
    pub red_artifact: String,
    pub green_artifact: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenAssessment {
    pub basis: RedGreenBasis,
    pub repository: RepositoryId,
    pub baseline: CiBaselinePin,
    pub candidate: CiSourceIdentity,
    pub check: CheckRef,
    pub policy: ContentHash,
    pub red_producer: ProducerRef,
    pub green_producer: ProducerRef,
    pub red_run: LocalRunId,
    pub green_run: LocalRunId,
    pub red_payload: ContentHash,
    pub green_payload: ContentHash,
    pub red_artifact: ContentHash,
    pub green_artifact: ContentHash,
    pub cases: Vec<JUnitCaseIdentity>,
    pub changed_inputs: Vec<PathBuf>,
    pub assessed_at: Timestamp,
    pub fingerprint: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RedGreenBasis {
    AuthenticatedCheckPair,
}

/// Independent prior acceptance and current reviewer authority for the proposed red baseline.
/// The envelope must approve that exact red commit and evaluation contract.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenBaselineReview {
    pub accepted: CiBaselinePin,
    pub policy: ContractReviewPolicy,
    pub expected_policy: ContentHash,
    pub envelope: SignedContractReview,
}

/// Fresh composition of contract-review admission and check-pair verification.
/// Serialized observations cannot replace the original proofs on a future evaluation.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewedRedGreenAssessment {
    pub review: AuthenticatedContractReview,
    pub pair: RedGreenAssessment,
}
