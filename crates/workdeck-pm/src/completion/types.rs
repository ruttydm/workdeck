use crate::*;
use serde::{Deserialize, Serialize};

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionCheckSelection {
    pub check: String,
    pub attestation: AttestationId,
    pub expected_attestation: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionGateSelection {
    pub gate: GateId,
    pub expected_gate: SourceToken,
    pub evidence: Vec<GateEvidenceSelection>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteRedGreenIssue {
    pub issue: IssueId,
    pub expected_issue: SourceToken,
    pub actor: String,
    pub authority: RetainedRedGreenAuthority,
    pub checks: Vec<CompletionCheckSelection>,
    pub gates: Vec<CompletionGateSelection>,
}
/// Historical publication proof. Reading or replaying it does not renew authority.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenIssueCompletion {
    pub input: CompleteRedGreenIssue,
    pub before: IssueRecord,
    pub after: IssueRecord,
    pub before_document: String,
    pub after_document: String,
    pub completion: CompletionReport,
    pub prepared: CiPreparedCheck,
    pub attestations: Vec<ImportedCheckReportRecord>,
    pub gates: Vec<RedGreenGateAssessment>,
    pub evidence: Vec<EvidenceRecord>,
    pub admitted_at: Timestamp,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionAuthority {
    pub candidate: GitOid,
    pub policy: ProducerTrustPolicy,
    pub expected_policy: ContentHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_green: Option<RetainedRedGreenAuthority>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompleteVerifiedIssue {
    pub issue: IssueId,
    pub expected_issue: SourceToken,
    pub actor: String,
    pub authority: CompletionAuthority,
    pub checks: Vec<CompletionCheckSelection>,
    pub gates: Vec<CompletionGateSelection>,
}
impl From<&CompleteRedGreenIssue> for CompleteVerifiedIssue {
    fn from(input: &CompleteRedGreenIssue) -> Self {
        Self {
            issue: input.issue.clone(),
            expected_issue: input.expected_issue.clone(),
            actor: input.actor.clone(),
            checks: input.checks.clone(),
            gates: input.gates.clone(),
            authority: CompletionAuthority {
                candidate: input.authority.candidate.clone(),
                policy: input.authority.policy.clone(),
                expected_policy: input.authority.expected_policy.clone(),
                red_green: Some(input.authority.clone()),
            },
        }
    }
}

/// Historical publication proof. Reading or replaying it does not renew authority.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedIssueCompletion {
    pub input: CompleteVerifiedIssue,
    pub before: IssueRecord,
    pub after: IssueRecord,
    pub before_document: String,
    pub after_document: String,
    pub completion: CompletionReport,
    pub prepared: CiPreparedCheck,
    pub attestations: Vec<ImportedCheckReportRecord>,
    pub gates: Vec<CompletionGateAssessment>,
    pub evidence: Vec<EvidenceRecord>,
    pub admitted_at: Timestamp,
}

/// Gate proof retained by verified completion. The untagged representation
/// keeps legacy red/green receipts byte-compatible while allowing green-only
/// gate evidence in new receipts.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CompletionGateAssessment {
    RedGreen(RedGreenGateAssessment),
    GreenOnly(VerifiedGateAssessment),
}
