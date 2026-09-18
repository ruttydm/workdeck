use crate::*;
use serde::{Deserialize, Serialize};
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCoverageAuthority {
    pub baseline: CiBaselinePin,
    pub policy: ContractReviewPolicy,
    pub expected_policy: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCoverageRequest {
    pub revision: CiRevision,
    /// Also compare live planning contracts and the committed evaluator selection.
    #[serde(default)]
    pub working_tree: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<CiSubjectIdentity>,
    /// Exact selected document bytes; detects dirty or mismatched workbench selection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_subject: Option<ContentHash>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authority: Option<ReviewCoverageAuthority>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewCoverageState {
    HistoricalMatch,
    Authenticated,
    Stale,
    Rejected,
    Unknown,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCoverageRow {
    pub review: ImportedContractReviewSummary,
    pub state: ReviewCoverageState,
    pub reason_codes: Vec<String>,
    pub diagnostic: Option<PmError>,
    pub current_reviewers: Vec<String>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewWorkingTree {
    pub contract: ContentHash,
    pub evaluators: ContentHash,
    pub matches_revision: bool,
}
/// Review of evaluation contracts only. Neither criterion success nor passing checks.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewCoverage {
    pub repository: RepositoryId,
    pub request: ReviewCoverageRequest,
    pub source: Option<CiSourceIdentity>,
    pub subject: Option<CiSubjectContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_tree: Option<ReviewWorkingTree>,
    pub diagnostics: Vec<PmError>,
    pub rows: Vec<ReviewCoverageRow>,
    pub authenticated: bool,
    pub assessed_at: Timestamp,
    pub fingerprint: ContentHash,
}
