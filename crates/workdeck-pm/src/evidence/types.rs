use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
pub const MAX_EVIDENCE_BYTES: usize = 128 * 1024;
pub(crate) const MAX_EVIDENCE_ENTRIES: usize = 4096;
#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ExactSubjectKind {
    Source,
    Artifact,
}
/// An explicit immutable subject declaration, not a claim that the subject was
/// materialized, admitted, executed, or verified by Workdeck.
#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct ExactSubject {
    pub repository: RepositoryId,
    pub kind: ExactSubjectKind,
    pub content: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProducerRef {
    pub id: String,
    pub definition: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckRef {
    pub id: String,
    pub definition: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultRef {
    pub id: String,
    pub content: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceLink {
    Source {
        link: SourceLink,
    },
    Url {
        url: String,
    },
    Attestation {
        id: AttestationId,
        content: ContentHash,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclaredProvenance {
    pub actor: String,
    pub reason: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSupersession {
    pub id: EvidenceId,
    pub content: ContentHash,
    pub reason: String,
}
/// This is the complete public authoring contract. There is deliberately no
/// passed/verified verdict or producer-trust flag among its inputs.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclareEvidence {
    pub criterion: CriterionRef,
    pub subject: ExactSubject,
    pub producer: ProducerRef,
    pub check: CheckRef,
    pub result: ResultRef,
    pub observed_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<Timestamp>,
    pub provenance: DeclaredProvenance,
    #[serde(default)]
    pub links: Vec<EvidenceLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<EvidenceSupersession>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceProvenanceKind {
    Declared,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceReference {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: EvidenceId,
    pub recorded_at: Timestamp,
    pub provenance_kind: EvidenceProvenanceKind,
    pub declaration: DeclareEvidence,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub reference: EvidenceReference,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EvidenceQuery {
    pub criterion: Option<CriterionRef>,
    pub subject: Option<ExactSubject>,
    pub include_superseded: bool,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenEvidenceRequest {
    pub evidence: EvidenceId,
    pub expected_evidence: ContentHash,
    pub authority: RetainedRedGreenAuthority,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceVerificationBasis {
    AuthenticatedCheckLink,
    AuthenticatedCheck,
}
/// Proves the exact declaration-to-execution link, not satisfaction of a gate or completion policy.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenEvidenceAssessment {
    pub basis: EvidenceVerificationBasis,
    pub evidence: EvidenceId,
    pub evidence_content: ContentHash,
    pub attestation: AttestationId,
    pub attestation_content: ContentHash,
    pub criterion: ResolvedCriterion,
    pub committed_criterion: ResolvedCriterion,
    pub verification: RetainedRedGreenAssessment,
}

/// Fresh producer authority and an exact declaration-to-attestation selection for
/// a passed check. This is read-only evidence qualification; it is not a
/// completion credential and cannot replace a configured red/green proof.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedEvidenceRequest {
    pub evidence: EvidenceId,
    pub expected_evidence: ContentHash,
    pub authority: CompletionAuthority,
}

/// A current, source-bound declaration-to-passed-check link. The authenticated
/// report is retained so historical gate/completion receipts can revalidate the
/// exact signed result without treating this assessment as a trust root.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedEvidenceAssessment {
    pub basis: EvidenceVerificationBasis,
    pub evidence: EvidenceId,
    pub evidence_content: ContentHash,
    pub attestation: AttestationId,
    pub attestation_content: ContentHash,
    pub criterion: ResolvedCriterion,
    pub committed_criterion: ResolvedCriterion,
    pub verification: AuthenticatedCheckReport,
}
