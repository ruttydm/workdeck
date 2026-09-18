use crate::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, path::PathBuf};
pub const MAX_QUESTION_BYTES: usize = 128 * 1024;
pub(crate) const MAX_QUESTIONS: usize = 4096;
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionSubject {
    pub subject: SubjectRef,
    pub source: SourceToken,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateQuestion {
    pub actor: String,
    pub body: String,
    pub subjects: Vec<QuestionSubject>,
    #[serde(default)]
    pub requirements: Vec<CriterionRef>,
    #[serde(default)]
    pub blocks_work: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionState {
    Open,
    Answered,
    Superseded,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordedAnswer {
    pub actor: String,
    pub body: String,
    pub answered_at: Timestamp,
    #[serde(default)]
    pub decision_refs: Vec<SourcePin>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionSupersession {
    pub actor: String,
    pub reason: String,
    pub replacement: QuestionId,
    pub replacement_source: SourceToken,
    pub superseded_at: Timestamp,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuestionMetadata {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: QuestionId,
    pub revision: Revision,
    pub actor: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub subjects: Vec<QuestionSubject>,
    #[serde(default)]
    pub requirements: Vec<CriterionRef>,
    pub blocks_work: bool,
    pub state: QuestionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<RecordedAnswer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersession: Option<QuestionSupersession>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(default, flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionRecord {
    pub metadata: QuestionMetadata,
    pub body: String,
    pub path: PathBuf,
    pub source: SourceToken,
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionMutation {
    Answer {
        actor: String,
        body: String,
        #[serde(default)]
        decision_refs: Vec<SourcePin>,
    },
    Supersede {
        actor: String,
        reason: String,
        replacement: QuestionId,
        replacement_source: SourceToken,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuestionRequest {
    Create {
        input: CreateQuestion,
    },
    Mutate {
        id: QuestionId,
        expected: SourceToken,
        mutation: QuestionMutation,
    },
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionMutationResult {
    pub question: QuestionRecord,
    pub input: QuestionRequest,
    pub previous: Option<QuestionRecord>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuestionFreshness {
    Current,
    Stale,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionReason {
    pub code: String,
    pub message: String,
    pub subject: Option<SubjectRef>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
/// State/source applicability only. Mutation-time actor policy still applies;
/// this report never grants authorization to a caller.
pub struct QuestionActionState {
    pub allowed: bool,
    pub reasons: Vec<QuestionReason>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionApplicability {
    pub question_id: QuestionId,
    pub source: SourceToken,
    pub state: QuestionState,
    pub matched_subjects: Vec<SubjectRef>,
    pub freshness: QuestionFreshness,
    pub stale_reasons: Vec<QuestionReason>,
    pub blocks_implementation: bool,
    pub answer: QuestionActionState,
    pub supersede: QuestionActionState,
}
#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QuestionQuery {
    pub subjects: Vec<SubjectRef>,
    pub state: Option<QuestionState>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionReferenceBlocker {
    pub question: QuestionId,
    pub path: PathBuf,
    pub field: String,
    pub source: SourceToken,
}
