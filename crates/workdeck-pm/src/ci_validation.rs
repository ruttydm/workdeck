//! Immutable planning validation. A caller-selected baseline is not a trusted
//! acceptance decision, and a valid planning tree is not check-run evidence.
use crate::{ContentHash, DoctorReport, GitOid, GitRefName, RepositoryId, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CiRevision {
    Commit { oid: GitOid },
    Reference { reference: GitRefName },
    Head {},
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiValidateRequest {
    pub base: CiRevision,
    pub head: CiRevision,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiSourceIdentity {
    pub repository: RepositoryId,
    pub commit: GitOid,
    pub tree: GitOid,
    pub content: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiValidationBasis {
    /// Caller-selected revisions; neither accepted-contract authority nor check execution.
    PlanningSourceValidation,
    /// Baseline matches caller-supplied pins; does not attest reviewer identity or checks.
    PinnedBaselineValidation,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiValidationReport {
    pub basis: CiValidationBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_pin: Option<CiBaselinePin>,
    pub base: CiSourceIdentity,
    pub head: CiSourceIdentity,
    pub contracts: crate::CiContractComparison,
    pub base_report: DoctorReport,
    pub head_report: DoctorReport,
    /// Source/schema validity and unchanged baseline evaluation contract only.
    /// Semantic contract changes require explicit review. This does not authorize completion or
    /// establish that the caller-selected base is an accepted contract.
    pub valid: bool,
}

pub fn ci_validate(worktree: &Path, request: &CiValidateRequest) -> Result<CiValidationReport> {
    ci_validate_with_faults(worktree, request, |_| Ok(()))
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CiValidationFaultPoint {
    BeforeRevalidation,
}

#[doc(hidden)]
pub fn ci_validate_with_faults(
    worktree: &Path,
    request: &CiValidateRequest,
    fault: impl FnMut(CiValidationFaultPoint) -> Result<()>,
) -> Result<CiValidationReport> {
    crate::sources::capture_commits(worktree, request, fault)
}

/// Obtain these pins from the accepted baseline through an independent channel.
/// Candidate-controlled pins provide consistency only, not acceptance authority.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CiBaselinePin {
    pub commit: GitOid,
    pub contract: ContentHash,
}

/// Capture immutable sources afresh before applying externally selected baseline pins.
/// Contract changes still require review and invalid candidates remain invalid.
pub fn ci_validate_pinned(
    worktree: &Path,
    request: &CiValidateRequest,
    pin: &CiBaselinePin,
) -> Result<CiValidationReport> {
    let mut report = ci_validate(worktree, request)?;
    if report.base.commit != pin.commit
        || report.contracts.base.as_ref().map(|base| &base.fingerprint) != Some(&pin.contract)
    {
        return Err(crate::PmError::new(
            crate::ErrorCode::PolicyBlocked,
            "CI baseline does not match the independently supplied commit and contract pins",
        )
        .details(serde_json::json!({
            "expected": pin, "actual_source": report.base,
            "actual_contract": report.contracts.base.as_ref().map(|base| &base.fingerprint),
        })));
    }
    report.basis = CiValidationBasis::PinnedBaselineValidation;
    report.baseline_pin = Some(pin.clone());
    Ok(report)
}
