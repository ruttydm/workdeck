use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};

pub const MAX_GATE_BYTES: usize = 128 * 1024;
pub(crate) const MAX_GATE_ENTRIES: usize = 4096;
#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum CriterionOwner {
    Issue(IssueId),
    Feature(FeatureId),
    Milestone(String),
    Project(String),
}
impl CriterionOwner {
    pub fn subject(&self) -> SubjectRef {
        match self {
            Self::Issue(id) => SubjectRef::Issue(id.clone()),
            Self::Feature(id) => SubjectRef::Feature(id.clone()),
            Self::Milestone(id) => SubjectRef::Milestone(id.clone()),
            Self::Project(id) => SubjectRef::Project(id.clone()),
        }
    }
    pub fn validate(&self) -> Result<()> {
        if let Self::Milestone(id) | Self::Project(id) = self {
            crate::planning::validate_id(id)?;
        }
        Ok(())
    }
}
#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(deny_unknown_fields)]
pub struct CriterionRef {
    pub repository: RepositoryId,
    pub owner: CriterionOwner,
    pub id: String,
    pub definition: ContentHash,
}
impl CriterionRef {
    pub fn validate(&self) -> Result<()> {
        self.owner.validate()?;
        if !crate::identity::valid_slug(&self.id) {
            return Err(super::invalid(
                "criterion ID must be a stable lowercase slug",
            ));
        }
        Ok(())
    }
    pub fn subject(&self) -> SubjectRef {
        SubjectRef::Criterion {
            owner: Box::new(self.owner.subject()),
            id: self.id.clone(),
        }
    }
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CriterionDeclaration {
    Checked,
    Unchecked,
    Declared,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolvedCriterion {
    pub reference: CriterionRef,
    pub description: String,
    pub declaration: CriterionDeclaration,
    pub source: SourcePin,
    pub retired: bool,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateRequirement {
    pub id: String,
    pub criterion: CriterionRef,
    pub producer: ProducerRef,
    pub check: CheckRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_seconds: Option<u64>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateDefinition {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: GateId,
    pub revision: Revision,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub requirements: Vec<GateRequirement>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateRecord {
    pub definition: GateDefinition,
    pub path: PathBuf,
    pub source: SourceToken,
    pub document: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<Tombstone>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateGate {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub requirements: Vec<GateRequirement>,
    #[serde(default)]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum GateMutation {
    Update { fields: BTreeMap<String, Value> },
    PatchCustom { patch: CustomPatch },
    Archive { archived: bool },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum GateRequest {
    Create {
        input: CreateGate,
    },
    Mutate {
        id: GateId,
        expected: SourceToken,
        mutation: GateMutation,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateMutationResult {
    pub gate: GateRecord,
    pub input: GateRequest,
    pub previous: Option<GateRecord>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateAssessmentRequest {
    pub gate: GateId,
    pub subject: ExactSubject,
    pub as_of: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateRequirementAssessment {
    pub requirement: String,
    pub state: ConditionState,
    pub conditions: Vec<CompletionCondition>,
    pub evidence: Vec<EvidenceId>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateAssessment {
    pub gate: GateId,
    pub subject: ExactSubject,
    pub subject_origin: String,
    pub as_of: Timestamp,
    pub state: ConditionState,
    pub requirements: Vec<GateRequirementAssessment>,
    pub conditions: Vec<CompletionCondition>,
    pub source_pins: Vec<SourcePin>,
    pub fingerprint: ContentHash,
}

/// Explicit evidence choices for the complete conjunction, never serialized verdicts.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateEvidenceSelection {
    pub requirement: String,
    pub evidence: EvidenceId,
    pub expected_evidence: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenGateRequest {
    pub gate: GateId,
    pub expected_gate: SourceToken,
    pub authority: RetainedRedGreenAuthority,
    pub evidence: Vec<GateEvidenceSelection>,
}
/// Explicit evidence choices for a gate whose requirements may be satisfied by
/// authenticated passed checks without a red/green pair.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedGateRequest {
    pub gate: GateId,
    pub expected_gate: SourceToken,
    pub authority: CompletionAuthority,
    pub evidence: Vec<GateEvidenceSelection>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerificationBasis {
    AuthenticatedRequirements,
    AuthenticatedGreenRequirements,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedGateRequirement {
    pub requirement: String,
    pub proof: RedGreenEvidenceAssessment,
}

/// A read-only current assessment, not a reusable completion credential.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedGreenGateAssessment {
    pub basis: GateVerificationBasis,
    pub gate: GateRecord,
    pub candidate: CiSourceIdentity,
    pub requirements: Vec<VerifiedGateRequirement>,
    pub assessed_at: Timestamp,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GreenGateRequirement {
    pub requirement: String,
    pub proof: VerifiedEvidenceAssessment,
}

/// A current green-only gate assessment. It is read-only evidence for a single
/// candidate and is never accepted as a serialized completion credential by
/// itself.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifiedGateAssessment {
    pub basis: GateVerificationBasis,
    pub gate: GateRecord,
    pub candidate: CiSourceIdentity,
    pub requirements: Vec<GreenGateRequirement>,
    pub assessed_at: Timestamp,
}
