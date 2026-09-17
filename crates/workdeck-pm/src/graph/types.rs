use crate::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(
    schemars::JsonSchema, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(
    tag = "kind",
    content = "reference",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum SubjectRef {
    Issue(IssueId),
    Feature(FeatureId),
    Gate(GateId),
    Milestone(String),
    Project(String),
    Criterion { owner: Box<SubjectRef>, id: String },
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionState {
    Satisfied,
    Unsatisfied,
    Unknown,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionKind {
    Prerequisite,
    Child,
    Graph,
    Gate,
    Criterion,
    Evidence,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePin {
    pub path: PathBuf,
    pub content: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletionCondition {
    pub kind: ConditionKind,
    pub subject: SubjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_subject: Option<SubjectRef>,
    pub state: ConditionState,
    pub reason_code: String,
    pub message: String,
    #[serde(default)]
    pub path: Vec<SubjectRef>,
    #[serde(default)]
    pub source_pins: Vec<SourcePin>,
    pub basis: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RelatedIssueLink {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub issues: [IssueId; 2],
    pub created_at: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrerequisiteWaiver {
    pub request_id: RequestId,
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub issue: IssueId,
    pub prerequisite: IssueId,
    pub requirement_hash: ContentHash,
    pub prerequisite_source: SourceToken,
    pub policy_hash: ContentHash,
    pub actor: String,
    pub reason: String,
    pub created_at: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum IssueGraphMutation {
    SetParent {
        parent: Option<String>,
    },
    AddPrerequisite {
        prerequisite: String,
    },
    RemovePrerequisite {
        prerequisite: String,
        reason: String,
    },
    ReplacePrerequisite {
        prerequisite: String,
        replacement: String,
        reason: String,
    },
    SetRelated {
        other: String,
        related: bool,
    },
    WaivePrerequisite {
        prerequisite: String,
        actor: String,
        reason: String,
    },
    RevokeWaiver {
        prerequisite: String,
        reason: String,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, Serialize)]
pub struct IssueGraphSnapshot {
    pub(crate) schema: SchemaVersion,
    pub(crate) repository: RepositoryId,
    pub(crate) fingerprint: ContentHash,
    pub(crate) issues: Vec<IssueRecord>,
    pub(crate) related: Vec<RelatedIssueLink>,
    pub(crate) waivers: Vec<PrerequisiteWaiver>,
    pub(crate) diagnostics: Vec<CompletionCondition>,
    #[serde(skip)]
    pub(crate) config: Config,
    #[serde(skip)]
    pub(crate) sources: std::collections::BTreeMap<PathBuf, ContentHash>,
    #[serde(skip)]
    pub(crate) waiver_sources: std::collections::BTreeMap<RequestId, SourcePin>,
    #[serde(skip)]
    pub(crate) gate_conditions: std::collections::BTreeMap<GateId, Vec<CompletionCondition>>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueRelations {
    pub repository: RepositoryId,
    pub issue: IssueId,
    pub source: SourceToken,
    pub fingerprint: ContentHash,
    pub parent: Option<IssueId>,
    pub children: Vec<IssueId>,
    pub prerequisites: Vec<IssueId>,
    pub dependents: Vec<IssueId>,
    pub related: Vec<IssueId>,
    pub conditions: Vec<CompletionCondition>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueReadiness {
    pub repository: RepositoryId,
    pub issue: IssueId,
    pub source: SourceToken,
    pub fingerprint: ContentHash,
    pub ready: bool,
    pub conditions: Vec<CompletionCondition>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IssueDependencyPath {
    pub repository: RepositoryId,
    pub fingerprint: ContentHash,
    pub from: IssueId,
    pub to: IssueId,
    pub path: Vec<IssueId>,
    pub found: bool,
    pub basis: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<CompletionCondition>,
}
