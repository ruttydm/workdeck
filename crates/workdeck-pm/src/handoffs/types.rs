use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
pub const MAX_HANDOFF_BYTES: usize = 128 * 1024;
pub(crate) const MAX_HANDOFFS: usize = 4096;
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffEvidenceRef {
    pub id: EvidenceId,
    pub content: ContentHash,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffOperationRef {
    pub request_id: RequestId,
    pub operation_id: Option<OperationId>,
    pub receipt_content: Option<ContentHash>,
}
/// Authored continuity, never acceptance or an admitted verification result.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateHandoff {
    pub actor: String,
    pub anchor: ContextAnchor,
    pub body: String,
    #[serde(default)]
    pub attempted: Vec<String>,
    #[serde(default)]
    pub uncertainties: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<HandoffEvidenceRef>,
    #[serde(default)]
    pub questions: Vec<QuestionId>,
    #[serde(default)]
    pub pending_operations: Vec<HandoffOperationRef>,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandoffMetadata {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: HandoffId,
    pub issue: IssueId,
    pub created_at: Timestamp,
    pub actor: String,
    pub anchor: ContextAnchor,
    #[serde(default)]
    pub attempted: Vec<String>,
    #[serde(default)]
    pub uncertainties: Vec<String>,
    #[serde(default)]
    pub evidence_refs: Vec<HandoffEvidenceRef>,
    #[serde(default)]
    pub questions: Vec<QuestionId>,
    #[serde(default)]
    pub pending_operations: Vec<HandoffOperationRef>,
    #[serde(default)]
    pub next_steps: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HandoffRecord {
    pub metadata: HandoffMetadata,
    pub body: String,
    pub path: PathBuf,
    pub content: ContentHash,
    pub document: String,
}
