//! Browser-safe review command and resource-catalog wire schema.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use workdeck_core::{ReviewSide, has_exact_keys, is_review_sha256_digest};
use workdeck_review::{
    REVIEW_CANONICAL_FILE_CONTENT_TYPE, REVIEW_INTENT_TYPES, REVIEW_PATCH_CONTENT_TYPE,
    REVIEW_SOURCE_CONTENT_TYPE, ReadReviewResourceRequest, ReviewIntentPlanningErrorCode,
    ReviewPublicationAddress, ReviewRequestErrorCode, ReviewResourceChunk,
    ReviewResourceDescriptor, ReviewResourceDescriptorBase, ReviewResourceErrorCode,
    ReviewResourceKind, ReviewRevealRequest, ReviewSelectionScope, SemanticReviewIntent,
    parse_read_review_resource_request, parse_review_generation, parse_review_resource_id,
};

pub const WORKDECK_REVIEW_PROTOCOL_VERSION: u32 = 1;
pub const MAX_WORKDECK_REVIEW_ENVELOPE_BYTES: u64 = 4 * 1024 * 1024;
pub const MAX_WORKDECK_REVIEW_FILTER_BYTES: usize = 4 * 1024;
pub const MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES: usize = 1024;
pub const MAX_WORKDECK_REVIEW_CATALOG_RESOURCES: usize = 15_000;
const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkdeckReviewActorKindV1 {
    Terminal,
    Browser,
    Agent,
}

pub const WORKDECK_REVIEW_ACTOR_KINDS: [WorkdeckReviewActorKindV1; 3] = [
    WorkdeckReviewActorKindV1::Terminal,
    WorkdeckReviewActorKindV1::Browser,
    WorkdeckReviewActorKindV1::Agent,
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewActorV1 {
    pub client_id: String,
    pub kind: WorkdeckReviewActorKindV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewExpandedLineProofV1 {
    pub gap_id: String,
    pub side: ReviewSide,
    pub line: u64,
    pub source_identity: String,
}

pub const WORKDECK_REVIEW_ACTION_TYPES: [&str; 17] = REVIEW_INTENT_TYPES;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkdeckReviewLineAddressV1 {
    pub side: ReviewSide,
    pub line: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all_fields = "camelCase")]
pub enum WorkdeckReviewActionV1 {
    #[serde(rename = "selection/select")]
    SelectionSelect {
        file_key: String,
        hunk_index: u64,
        reveal: ReviewRevealRequest,
    },
    #[serde(rename = "selection/move")]
    SelectionMove {
        scope: ReviewSelectionScope,
        delta: i64,
    },
    #[serde(rename = "selection/select-file")]
    SelectionSelectFile {
        file_key: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reveal: Option<ReviewRevealRequest>,
    },
    #[serde(rename = "selection/anchor")]
    SelectionAnchor { file_key: String, hunk_index: u64 },
    #[serde(rename = "filter/set")]
    FilterSet { filter: String },
    #[serde(rename = "notes/set-visibility")]
    NotesSetVisibility { visible: bool },
    #[serde(rename = "notes/start-draft")]
    NotesStartDraft {
        file_key: String,
        hunk_index: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<WorkdeckReviewLineAddressV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reveal: Option<ReviewRevealRequest>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expanded_line_proof: Option<WorkdeckReviewExpandedLineProofV1>,
    },
    #[serde(rename = "notes/start-edit")]
    NotesStartEdit {
        note_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reveal: Option<ReviewRevealRequest>,
    },
    #[serde(rename = "notes/start-reply")]
    NotesStartReply {
        note_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reveal: Option<ReviewRevealRequest>,
    },
    #[serde(rename = "notes/update-draft")]
    NotesUpdateDraft { body: String },
    #[serde(rename = "notes/cancel-draft")]
    NotesCancelDraft,
    #[serde(rename = "notes/create-user")]
    NotesCreateUser {
        consume_draft: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<WorkdeckReviewLineAddressV1>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expanded_line_proof: Option<WorkdeckReviewExpandedLineProofV1>,
    },
    #[serde(rename = "notes/update-user")]
    NotesUpdateUser {
        note_id: String,
        consume_draft: bool,
    },
    #[serde(rename = "notes/remove-user")]
    NotesRemoveUser { note_id: String },
    #[serde(rename = "notes/remove-live")]
    NotesRemoveLive { note_id: String },
    #[serde(rename = "notes/clear")]
    NotesClear {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        file_key: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        include_user: Option<bool>,
    },
    #[serde(rename = "expansion/toggle")]
    ExpansionToggle { file_key: String, gap_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewActionEnvelopeV1 {
    pub protocol_version: u32,
    pub generation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_state_revision: Option<u64>,
    pub actor: WorkdeckReviewActorV1,
    pub action: WorkdeckReviewActionV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewResourceReadEnvelopeV1 {
    pub protocol_version: u32,
    pub actor: WorkdeckReviewActorV1,
    pub request: ReadReviewResourceRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkdeckReviewFailureCodeV1 {
    StaleGeneration,
    InvalidRequest,
    FileNotFound,
    HunkNotFound,
    GapNotFound,
    DraftMissing,
    DraftActive,
    DraftModeMismatch,
    NoteNotFound,
    NoteNotEditable,
    NoteHasReplies,
    NoteIdConflict,
    InvalidNoteParent,
    BlankNote,
    NoteTooLarge,
    MissingFact,
    UnknownResource,
    ResourceUnavailable,
    ResourceTooLarge,
    ResourceIntegrity,
    InvalidRange,
}

impl From<ReviewRequestErrorCode> for WorkdeckReviewFailureCodeV1 {
    fn from(code: ReviewRequestErrorCode) -> Self {
        match code {
            ReviewRequestErrorCode::StaleGeneration => Self::StaleGeneration,
            ReviewRequestErrorCode::InvalidRequest => Self::InvalidRequest,
        }
    }
}

impl From<ReviewResourceErrorCode> for WorkdeckReviewFailureCodeV1 {
    fn from(code: ReviewResourceErrorCode) -> Self {
        match code {
            ReviewResourceErrorCode::UnknownResource => Self::UnknownResource,
            ReviewResourceErrorCode::ResourceUnavailable => Self::ResourceUnavailable,
            ReviewResourceErrorCode::ResourceTooLarge => Self::ResourceTooLarge,
            ReviewResourceErrorCode::ResourceIntegrity => Self::ResourceIntegrity,
            ReviewResourceErrorCode::InvalidRange => Self::InvalidRange,
        }
    }
}

impl From<ReviewIntentPlanningErrorCode> for WorkdeckReviewFailureCodeV1 {
    fn from(code: ReviewIntentPlanningErrorCode) -> Self {
        use ReviewIntentPlanningErrorCode as Code;
        match code {
            Code::FileNotFound => Self::FileNotFound,
            Code::HunkNotFound => Self::HunkNotFound,
            Code::GapNotFound => Self::GapNotFound,
            Code::DraftMissing => Self::DraftMissing,
            Code::DraftActive => Self::DraftActive,
            Code::DraftModeMismatch => Self::DraftModeMismatch,
            Code::NoteNotFound => Self::NoteNotFound,
            Code::NoteNotEditable => Self::NoteNotEditable,
            Code::NoteHasReplies => Self::NoteHasReplies,
            Code::NoteIdConflict => Self::NoteIdConflict,
            Code::InvalidNoteParent => Self::InvalidNoteParent,
            Code::BlankNote => Self::BlankNote,
            Code::NoteTooLarge => Self::NoteTooLarge,
            Code::MissingFact => Self::MissingFact,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewFailureV1 {
    pub ok: bool,
    pub code: WorkdeckReviewFailureCodeV1,
    pub message: String,
    pub current_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewActionAppliedV1 {
    pub ok: bool,
    pub generation: String,
    pub state_revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkdeckReviewActionResultV1 {
    Applied(WorkdeckReviewActionAppliedV1),
    Failed(WorkdeckReviewFailureV1),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum WorkdeckReviewResourceReadResultV1 {
    Chunk {
        ok: bool,
        chunk: ReviewResourceChunk,
    },
    Failed(WorkdeckReviewFailureV1),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkdeckReviewParseFailureReason {
    Invalid,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkdeckReviewParseResult<T> {
    Parsed(T),
    Failed(WorkdeckReviewParseFailureReason),
}

fn identifier(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|value| !value.is_empty() && value.len() <= MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES)
}

fn index(value: &Value) -> Option<u64> {
    value.as_u64().filter(|value| *value <= MAX_SAFE_INTEGER)
}

fn signed_index(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .filter(|value| value.unsigned_abs() <= MAX_SAFE_INTEGER)
}

fn exact<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a serde_json::Map<String, Value>> {
    let object = value.as_object()?;
    has_exact_keys(object, keys).then_some(object)
}

fn expected_keys<'a>(
    object: &serde_json::Map<String, Value>,
    required: &[&'a str],
    optional: &[&'a str],
) -> Vec<&'a str> {
    required
        .iter()
        .chain(optional.iter().filter(|key| object.contains_key(**key)))
        .copied()
        .collect()
}

fn parse_reveal(value: &Value) -> Option<ReviewRevealRequest> {
    let object = exact(value, &["anchor", "scrollToNote"])?;
    let anchor = object.get("anchor")?.as_str()?;
    if !matches!(anchor, "hunk" | "file-top" | "none") || !object.get("scrollToNote")?.is_boolean()
    {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

fn parse_line_address(value: &Value) -> Option<WorkdeckReviewLineAddressV1> {
    let object = exact(value, &["side", "line"])?;
    if !matches!(object.get("side")?.as_str()?, "old" | "new") || index(object.get("line")?)? == 0 {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

#[must_use]
pub fn parse_workdeck_review_expanded_line_proof(
    value: &Value,
) -> Option<WorkdeckReviewExpandedLineProofV1> {
    let object = exact(value, &["gapId", "side", "line", "sourceIdentity"])?;
    if identifier(object.get("gapId")?).is_none()
        || !matches!(object.get("side")?.as_str()?, "old" | "new")
        || index(object.get("line")?)? == 0
        || identifier(object.get("sourceIdentity")?).is_none()
    {
        return None;
    }
    Some(WorkdeckReviewExpandedLineProofV1 {
        gap_id: object.get("gapId")?.as_str()?.to_owned(),
        side: serde_json::from_value(object.get("side")?.clone()).ok()?,
        line: index(object.get("line")?)?,
        source_identity: object.get("sourceIdentity")?.as_str()?.to_owned(),
    })
}

#[must_use]
pub fn parse_workdeck_review_actor(value: &Value) -> Option<WorkdeckReviewActorV1> {
    let object = value.as_object()?;
    let keys = expected_keys(object, &["clientId", "kind"], &["displayName"]);
    if !has_exact_keys(object, &keys)
        || identifier(object.get("clientId")?).is_none()
        || !matches!(
            object.get("kind")?.as_str()?,
            "terminal" | "browser" | "agent"
        )
        || object
            .get("displayName")
            .is_some_and(|value| identifier(value).is_none())
    {
        return None;
    }
    serde_json::from_value(value.clone()).ok()
}

fn valid_action_shape(object: &serde_json::Map<String, Value>, action_type: &str) -> bool {
    let keys = |required: &[&str], optional: &[&str]| {
        let expected = expected_keys(object, required, optional);
        has_exact_keys(object, &expected)
    };
    let optional_reveal = || {
        object
            .get("reveal")
            .is_none_or(|value| parse_reveal(value).is_some())
    };
    let proof = || {
        object
            .get("expandedLineProof")
            .is_none_or(|value| parse_workdeck_review_expanded_line_proof(value).is_some())
    };
    match action_type {
        "selection/select" => {
            keys(&["type", "fileKey", "hunkIndex", "reveal"], &[])
                && identifier(&object["fileKey"]).is_some()
                && index(&object["hunkIndex"]).is_some()
                && parse_reveal(&object["reveal"]).is_some()
        }
        "selection/move" => {
            keys(&["type", "scope", "delta"], &[])
                && matches!(
                    object["scope"].as_str(),
                    Some("hunk" | "file" | "annotated-hunk" | "annotated-file")
                )
                && signed_index(&object["delta"]).is_some()
        }
        "selection/select-file" => {
            keys(&["type", "fileKey"], &["reveal"])
                && identifier(&object["fileKey"]).is_some()
                && optional_reveal()
        }
        "selection/anchor" => {
            keys(&["type", "fileKey", "hunkIndex"], &[])
                && identifier(&object["fileKey"]).is_some()
                && index(&object["hunkIndex"]).is_some()
        }
        "filter/set" => {
            keys(&["type", "filter"], &[])
                && object["filter"]
                    .as_str()
                    .is_some_and(|value| value.len() <= MAX_WORKDECK_REVIEW_FILTER_BYTES)
        }
        "notes/set-visibility" => keys(&["type", "visible"], &[]) && object["visible"].is_boolean(),
        "notes/start-draft" => {
            keys(
                &["type", "fileKey", "hunkIndex"],
                &["target", "reveal", "expandedLineProof"],
            ) && identifier(&object["fileKey"]).is_some()
                && index(&object["hunkIndex"]).is_some()
                && object
                    .get("target")
                    .is_none_or(|value| parse_line_address(value).is_some())
                && optional_reveal()
                && proof()
                && (!object.contains_key("expandedLineProof") || object.contains_key("target"))
        }
        "notes/start-edit" | "notes/start-reply" => {
            keys(&["type", "noteId"], &["reveal"])
                && identifier(&object["noteId"]).is_some()
                && optional_reveal()
        }
        "notes/update-draft" => {
            keys(&["type", "body"], &[])
                && object["body"]
                    .as_str()
                    .is_some_and(|value| value.len() <= workdeck_review::MAX_REVIEW_NOTE_BYTES)
        }
        "notes/cancel-draft" => keys(&["type"], &[]),
        "notes/create-user" => {
            keys(&["type", "consumeDraft"], &["target", "expandedLineProof"])
                && object["consumeDraft"] == Value::Bool(true)
                && object
                    .get("target")
                    .is_none_or(|value| parse_line_address(value).is_some())
                && proof()
                && (!object.contains_key("expandedLineProof") || object.contains_key("target"))
        }
        "notes/update-user" => {
            keys(&["type", "noteId", "consumeDraft"], &[])
                && identifier(&object["noteId"]).is_some()
                && object["consumeDraft"] == Value::Bool(true)
        }
        "notes/remove-user" | "notes/remove-live" => {
            keys(&["type", "noteId"], &[]) && identifier(&object["noteId"]).is_some()
        }
        "notes/clear" => {
            keys(&["type"], &["fileKey", "includeUser"])
                && object
                    .get("fileKey")
                    .is_none_or(|value| identifier(value).is_some())
                && object.get("includeUser").is_none_or(Value::is_boolean)
        }
        "expansion/toggle" => {
            keys(&["type", "fileKey", "gapId"], &[])
                && identifier(&object["fileKey"]).is_some()
                && identifier(&object["gapId"]).is_some()
        }
        _ => false,
    }
}

#[must_use]
pub fn parse_workdeck_review_action(
    value: &Value,
) -> WorkdeckReviewParseResult<WorkdeckReviewActionV1> {
    let Some(object) = value.as_object() else {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid);
    };
    let Some(action_type) = object.get("type").and_then(Value::as_str) else {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid);
    };
    if !WORKDECK_REVIEW_ACTION_TYPES.contains(&action_type) {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Unsupported);
    }
    if !valid_action_shape(object, action_type) {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid);
    }
    serde_json::from_value(value.clone()).map_or(
        WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid),
        WorkdeckReviewParseResult::Parsed,
    )
}

/// Strip wire-only proof/precondition fields while preserving the intent's JSON shape exactly.
#[must_use]
pub fn to_review_intent_value(action: &WorkdeckReviewActionV1) -> Value {
    let mut value = serde_json::to_value(action).expect("review actions are JSON serializable");
    let object = value
        .as_object_mut()
        .expect("an internally tagged review action is an object");
    match action {
        WorkdeckReviewActionV1::NotesStartDraft { .. } => {
            object.remove("expandedLineProof");
        }
        WorkdeckReviewActionV1::NotesCreateUser { .. } => {
            object.remove("expandedLineProof");
            object.remove("target");
        }
        _ => {}
    }
    value
}

/// Strip wire-only expanded-line evidence and lower to the renderer-neutral intent.
#[must_use]
pub fn to_semantic_review_intent(action: &WorkdeckReviewActionV1) -> Option<SemanticReviewIntent> {
    let signed = |value: i64| isize::try_from(value).ok();
    let line = |value: WorkdeckReviewLineAddressV1| {
        Some(workdeck_core::SemanticReviewLineAddress {
            side: value.side,
            line: u32::try_from(value.line).ok()?,
        })
    };
    Some(match action {
        WorkdeckReviewActionV1::SelectionSelect {
            file_key,
            hunk_index,
            reveal,
        } => SemanticReviewIntent::Select {
            file_key: file_key.clone(),
            hunk_index: isize::try_from(*hunk_index).ok()?,
            reveal: *reveal,
        },
        WorkdeckReviewActionV1::SelectionMove { scope, delta } => SemanticReviewIntent::Move {
            scope: *scope,
            delta: signed(*delta)?,
        },
        WorkdeckReviewActionV1::SelectionSelectFile { file_key, reveal } => {
            SemanticReviewIntent::SelectFile {
                file_key: file_key.clone(),
                reveal: *reveal,
            }
        }
        WorkdeckReviewActionV1::SelectionAnchor {
            file_key,
            hunk_index,
        } => SemanticReviewIntent::Anchor {
            file_key: file_key.clone(),
            hunk_index: isize::try_from(*hunk_index).ok()?,
        },
        WorkdeckReviewActionV1::FilterSet { filter } => {
            SemanticReviewIntent::SetFilter(filter.clone())
        }
        WorkdeckReviewActionV1::NotesSetVisibility { visible } => {
            SemanticReviewIntent::SetNoteVisibility(*visible)
        }
        WorkdeckReviewActionV1::NotesStartDraft {
            file_key,
            hunk_index,
            target,
            reveal,
            ..
        } => SemanticReviewIntent::StartDraft {
            file_key: file_key.clone(),
            hunk_index: isize::try_from(*hunk_index).ok()?,
            target: match target {
                Some(target) => Some(line(*target)?),
                None => None,
            },
            reveal: *reveal,
        },
        WorkdeckReviewActionV1::NotesStartEdit { note_id, reveal } => {
            SemanticReviewIntent::StartEdit {
                note_id: note_id.clone(),
                reveal: *reveal,
            }
        }
        WorkdeckReviewActionV1::NotesStartReply { note_id, reveal } => {
            SemanticReviewIntent::StartReply {
                note_id: note_id.clone(),
                reveal: *reveal,
            }
        }
        WorkdeckReviewActionV1::NotesUpdateDraft { body } => {
            SemanticReviewIntent::UpdateDraft(body.clone())
        }
        WorkdeckReviewActionV1::NotesCancelDraft => SemanticReviewIntent::CancelDraft,
        WorkdeckReviewActionV1::NotesCreateUser { .. } => SemanticReviewIntent::CreateUserNote,
        WorkdeckReviewActionV1::NotesUpdateUser { note_id, .. } => {
            SemanticReviewIntent::UpdateUserNote {
                note_id: note_id.clone(),
            }
        }
        WorkdeckReviewActionV1::NotesRemoveUser { note_id } => {
            SemanticReviewIntent::RemoveUserNote {
                note_id: note_id.clone(),
            }
        }
        WorkdeckReviewActionV1::NotesRemoveLive { note_id } => {
            SemanticReviewIntent::RemoveLiveNote {
                note_id: note_id.clone(),
            }
        }
        WorkdeckReviewActionV1::NotesClear {
            file_key,
            include_user,
        } => SemanticReviewIntent::ClearNotes {
            file_key: file_key.clone(),
            include_user: include_user.unwrap_or(false),
        },
        WorkdeckReviewActionV1::ExpansionToggle { file_key, gap_id } => {
            SemanticReviewIntent::ToggleExpansion {
                file_key: file_key.clone(),
                gap_id: gap_id.clone(),
            }
        }
    })
}

#[must_use]
pub fn parse_workdeck_review_action_envelope(
    value: &Value,
) -> WorkdeckReviewParseResult<WorkdeckReviewActionEnvelopeV1> {
    let Some(object) = value.as_object() else {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid);
    };
    let keys = expected_keys(
        object,
        &["protocolVersion", "generation", "actor", "action"],
        &["expectedStateRevision"],
    );
    if !has_exact_keys(object, &keys)
        || object.get("protocolVersion").and_then(Value::as_u64)
            != Some(u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION))
        || object
            .get("generation")
            .and_then(Value::as_str)
            .and_then(parse_review_generation)
            .is_none()
        || object
            .get("expectedStateRevision")
            .is_some_and(|value| index(value).is_none())
        || object
            .get("actor")
            .and_then(parse_workdeck_review_actor)
            .is_none()
    {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid);
    }
    match parse_workdeck_review_action(&object["action"]) {
        WorkdeckReviewParseResult::Failed(reason) => WorkdeckReviewParseResult::Failed(reason),
        WorkdeckReviewParseResult::Parsed(_) => serde_json::from_value(value.clone()).map_or(
            WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid),
            WorkdeckReviewParseResult::Parsed,
        ),
    }
}

#[must_use]
pub fn parse_workdeck_review_resource_read_envelope(
    value: &Value,
) -> WorkdeckReviewParseResult<WorkdeckReviewResourceReadEnvelopeV1> {
    let Some(object) = exact(value, &["protocolVersion", "actor", "request"]) else {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid);
    };
    if object.get("protocolVersion").and_then(Value::as_u64)
        != Some(u64::from(WORKDECK_REVIEW_PROTOCOL_VERSION))
        || object
            .get("actor")
            .and_then(parse_workdeck_review_actor)
            .is_none()
        || object
            .get("request")
            .and_then(parse_read_review_resource_request)
            .is_none()
    {
        return WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid);
    }
    serde_json::from_value(value.clone()).map_or(
        WorkdeckReviewParseResult::Failed(WorkdeckReviewParseFailureReason::Invalid),
        WorkdeckReviewParseResult::Parsed,
    )
}

fn resource_kind_name(kind: ReviewResourceKind) -> &'static str {
    match kind {
        ReviewResourceKind::CanonicalFile => "canonical-file",
        ReviewResourceKind::Patch => "patch",
        ReviewResourceKind::Source => "source",
    }
}

fn resource_content_type(kind: ReviewResourceKind) -> &'static str {
    match kind {
        ReviewResourceKind::CanonicalFile => REVIEW_CANONICAL_FILE_CONTENT_TYPE,
        ReviewResourceKind::Patch => REVIEW_PATCH_CONTENT_TYPE,
        ReviewResourceKind::Source => REVIEW_SOURCE_CONTENT_TYPE,
    }
}

#[must_use]
pub fn parse_workdeck_review_resource_descriptor(
    value: &Value,
) -> Option<ReviewResourceDescriptor> {
    let object = value.as_object()?;
    let keys = expected_keys(
        object,
        &["id", "generation", "fileKey", "kind", "contentType"],
        &["byteLength", "digest", "side", "sourceIdentity"],
    );
    if !has_exact_keys(object, &keys) {
        return None;
    }
    let id = object.get("id")?.as_str()?;
    let address = parse_review_resource_id(id)?;
    let generation = object.get("generation")?.as_str()?;
    let file_key = object.get("fileKey")?.as_str()?;
    if object.get("kind")?.as_str()? != resource_kind_name(address.kind)
        || file_key != address.file_key
        || parse_review_generation(generation).is_none()
        || object.get("contentType")?.as_str()? != resource_content_type(address.kind)
    {
        return None;
    }
    let byte_length = object.get("byteLength");
    let digest = object.get("digest");
    if byte_length.is_some() != digest.is_some()
        || byte_length.is_some_and(|value| index(value).is_none())
        || digest.is_some_and(|value| {
            value
                .as_str()
                .is_none_or(|value| !is_review_sha256_digest(value))
        })
    {
        return None;
    }
    let descriptor = ReviewResourceDescriptorBase {
        id: id.to_owned(),
        generation: generation.to_owned(),
        file_key: file_key.to_owned(),
        byte_length: byte_length.and_then(Value::as_u64),
        digest: digest.and_then(Value::as_str).map(str::to_owned),
    };
    let content_type = object.get("contentType")?.as_str()?.to_owned();
    match address.kind {
        ReviewResourceKind::Source => {
            let side: ReviewSide = serde_json::from_value(object.get("side")?.clone()).ok()?;
            if Some(side) != address.side {
                return None;
            }
            let source_identity = identifier(object.get("sourceIdentity")?)?.to_owned();
            Some(ReviewResourceDescriptor::Source {
                descriptor,
                content_type,
                side,
                source_identity,
            })
        }
        ReviewResourceKind::CanonicalFile => {
            if object.contains_key("side") || object.contains_key("sourceIdentity") {
                return None;
            }
            Some(ReviewResourceDescriptor::CanonicalFile {
                descriptor,
                content_type,
            })
        }
        ReviewResourceKind::Patch => {
            if object.contains_key("side") || object.contains_key("sourceIdentity") {
                return None;
            }
            Some(ReviewResourceDescriptor::Patch {
                descriptor,
                content_type,
            })
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkdeckReviewResourceCatalogV1 {
    pub generation: String,
    pub file_keys_by_runtime_id: BTreeMap<String, String>,
    pub resources: Vec<ReviewResourceDescriptor>,
}

#[must_use]
pub fn parse_workdeck_review_resource_catalog(
    value: &Value,
) -> Option<WorkdeckReviewResourceCatalogV1> {
    let object = exact(value, &["generation", "fileKeysByRuntimeId", "resources"])?;
    let generation = object.get("generation")?.as_str()?;
    parse_review_generation(generation)?;
    let resource_values = object.get("resources")?.as_array()?;
    if resource_values.len() > MAX_WORKDECK_REVIEW_CATALOG_RESOURCES {
        return None;
    }
    let resources = resource_values
        .iter()
        .map(parse_workdeck_review_resource_descriptor)
        .collect::<Option<Vec<_>>>()?;
    if resources
        .iter()
        .any(|resource| resource.base().generation != generation)
    {
        return None;
    }
    let file_keys = object.get("fileKeysByRuntimeId")?.as_object()?;
    if file_keys.len() > resource_values.len()
        || file_keys.iter().any(|(runtime_id, file_key)| {
            runtime_id.is_empty()
                || runtime_id.len() > MAX_WORKDECK_REVIEW_IDENTIFIER_BYTES
                || identifier(file_key).is_none()
        })
    {
        return None;
    }
    Some(WorkdeckReviewResourceCatalogV1 {
        generation: generation.to_owned(),
        file_keys_by_runtime_id: file_keys
            .iter()
            .map(|(runtime_id, file_key)| {
                (runtime_id.clone(), file_key.as_str().unwrap().to_owned())
            })
            .collect(),
        resources,
    })
}

#[must_use]
pub fn parse_workdeck_review_publication_address(
    value: &Value,
) -> Option<ReviewPublicationAddress> {
    let object = exact(value, &["generation", "stateRevision"])?;
    let generation = object.get("generation")?.as_str()?;
    parse_review_generation(generation)?;
    Some(ReviewPublicationAddress {
        generation: generation.to_owned(),
        state_revision: index(object.get("stateRevision")?)?,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use serde_json::json;
    use workdeck_review::{
        MAX_REVIEW_NOTE_BYTES, ReviewResourceAddress, review_note_within_size_limit,
        review_resource_id,
    };

    use super::*;

    const GENERATION: &str = "generation:p1:3";
    const FILE_KEY: &str = "file:0123456789abcdef";

    fn actor() -> Value {
        json!({"clientId": "client-1", "kind": "browser"})
    }

    fn envelope(action: Value) -> Value {
        json!({
            "protocolVersion": WORKDECK_REVIEW_PROTOCOL_VERSION,
            "generation": GENERATION,
            "actor": actor(),
            "action": action,
        })
    }

    fn parsed_action(value: &Value) -> WorkdeckReviewActionV1 {
        match parse_workdeck_review_action(value) {
            WorkdeckReviewParseResult::Parsed(action) => action,
            result => panic!("expected parsed action, got {result:?}"),
        }
    }

    fn assert_failed<T: std::fmt::Debug + PartialEq>(
        result: WorkdeckReviewParseResult<T>,
        reason: WorkdeckReviewParseFailureReason,
    ) {
        assert_eq!(result, WorkdeckReviewParseResult::Failed(reason));
    }

    #[test]
    fn action_vocabulary_is_exactly_the_semantic_intent_vocabulary() {
        assert_eq!(WORKDECK_REVIEW_ACTION_TYPES, REVIEW_INTENT_TYPES);
        let action_types = action_examples()
            .into_iter()
            .map(|value| value["type"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(
            action_types,
            WORKDECK_REVIEW_ACTION_TYPES
                .into_iter()
                .map(str::to_owned)
                .collect()
        );
    }

    #[test]
    fn unsupported_actions_remain_distinct_from_malformed_actions() {
        assert_failed(
            parse_workdeck_review_action(&json!({"type": "session/reload"})),
            WorkdeckReviewParseFailureReason::Unsupported,
        );
        assert_failed(
            parse_workdeck_review_action(&json!({"filter": "x"})),
            WorkdeckReviewParseFailureReason::Invalid,
        );
    }

    fn action_examples() -> Vec<Value> {
        vec![
            json!({
                "type": "selection/select",
                "fileKey": FILE_KEY,
                "hunkIndex": 2,
                "reveal": {"anchor": "hunk", "scrollToNote": false}
            }),
            json!({"type": "selection/move", "scope": "annotated-hunk", "delta": -1}),
            json!({"type": "selection/select-file", "fileKey": FILE_KEY}),
            json!({"type": "selection/anchor", "fileKey": FILE_KEY, "hunkIndex": 0}),
            json!({"type": "filter/set", "filter": "src/ui"}),
            json!({"type": "notes/set-visibility", "visible": true}),
            json!({"type": "notes/start-draft", "fileKey": FILE_KEY, "hunkIndex": 1}),
            json!({"type": "notes/start-edit", "noteId": "user:1"}),
            json!({"type": "notes/start-reply", "noteId": "live:1"}),
            json!({"type": "notes/update-draft", "body": "revised body"}),
            json!({"type": "notes/cancel-draft"}),
            json!({"type": "notes/create-user", "consumeDraft": true}),
            json!({"type": "notes/update-user", "noteId": "user:1", "consumeDraft": true}),
            json!({"type": "notes/remove-user", "noteId": "user:1"}),
            json!({"type": "notes/remove-live", "noteId": "live:1"}),
            json!({"type": "notes/clear", "fileKey": FILE_KEY, "includeUser": true}),
            json!({"type": "expansion/toggle", "fileKey": FILE_KEY, "gapId": "before:1"}),
        ]
    }

    #[test]
    fn every_action_survives_serialization_and_lowers_to_a_semantic_intent() {
        for value in action_examples() {
            let action = parsed_action(&value);
            assert_eq!(serde_json::to_value(&action).unwrap(), value);
            assert_eq!(to_review_intent_value(&action), value);
            assert!(to_semantic_review_intent(&action).is_some());
        }
    }

    #[test]
    fn action_parsers_reject_extra_missing_and_unnavigable_fields() {
        for value in [
            json!({"type": "filter/set", "filter": "x", "reveal": true}),
            json!({"type": "selection/select", "fileKey": FILE_KEY, "hunkIndex": 0}),
            json!({"type": "selection/move", "scope": "line", "delta": 1}),
        ] {
            assert_failed(
                parse_workdeck_review_action(&value),
                WorkdeckReviewParseFailureReason::Invalid,
            );
        }
    }

    fn expanded_proof() -> Value {
        json!({
            "gapId": "before:1",
            "side": "new",
            "line": 5,
            "sourceIdentity": "source:abc"
        })
    }

    #[test]
    fn expanded_line_proof_requires_a_complete_target_and_is_stripped_when_lowered() {
        let value = json!({
            "type": "notes/start-draft",
            "fileKey": FILE_KEY,
            "hunkIndex": 1,
            "target": {"side": "new", "line": 5},
            "expandedLineProof": expanded_proof(),
        });
        let action = parsed_action(&value);
        assert_eq!(serde_json::to_value(&action).unwrap(), value);
        assert_eq!(
            to_review_intent_value(&action),
            json!({
                "type": "notes/start-draft",
                "fileKey": FILE_KEY,
                "hunkIndex": 1,
                "target": {"side": "new", "line": 5},
            })
        );
        assert!(matches!(
            to_semantic_review_intent(&action),
            Some(SemanticReviewIntent::StartDraft {
                target: Some(workdeck_core::SemanticReviewLineAddress {
                    side: ReviewSide::New,
                    line: 5,
                }),
                ..
            })
        ));

        let create = parsed_action(&json!({
            "type": "notes/create-user",
            "consumeDraft": true,
            "target": {"side": "new", "line": 5},
            "expandedLineProof": expanded_proof(),
        }));
        assert_eq!(
            to_semantic_review_intent(&create),
            Some(SemanticReviewIntent::CreateUserNote)
        );
        assert_eq!(
            to_review_intent_value(&create),
            json!({"type": "notes/create-user", "consumeDraft": true})
        );

        assert_failed(
            parse_workdeck_review_action(&json!({
                "type": "notes/start-draft",
                "fileKey": FILE_KEY,
                "hunkIndex": 1,
                "expandedLineProof": expanded_proof(),
            })),
            WorkdeckReviewParseFailureReason::Invalid,
        );
        for invalid in [
            json!({"gapId": "before:1", "side": "new", "line": 5}),
            json!({"gapId": "before:1", "side": "new", "line": 0, "sourceIdentity": "source:abc"}),
            json!({"gapId": "before:1", "side": "both", "line": 5, "sourceIdentity": "source:abc"}),
        ] {
            assert_eq!(parse_workdeck_review_expanded_line_proof(&invalid), None);
        }
    }

    #[test]
    fn actor_tags_accept_every_kind_and_are_required_by_action_envelopes() {
        for kind in ["terminal", "browser", "agent"] {
            assert!(parse_workdeck_review_actor(&json!({"clientId": "c", "kind": kind})).is_some());
        }
        let labeled = json!({"clientId": "c", "kind": "agent", "displayName": "Pi"});
        assert_eq!(
            serde_json::to_value(parse_workdeck_review_actor(&labeled).unwrap()).unwrap(),
            labeled
        );
        for invalid in [
            json!({"clientId": "c", "kind": "robot"}),
            json!({"kind": "browser"}),
            json!({"clientId": "", "kind": "browser"}),
        ] {
            assert_eq!(parse_workdeck_review_actor(&invalid), None);
        }

        let valid = envelope(json!({"type": "notes/set-visibility", "visible": true}));
        assert!(matches!(
            parse_workdeck_review_action_envelope(&valid),
            WorkdeckReviewParseResult::Parsed(_)
        ));
        let mut missing = valid;
        missing.as_object_mut().unwrap().remove("actor");
        assert_failed(
            parse_workdeck_review_action_envelope(&missing),
            WorkdeckReviewParseFailureReason::Invalid,
        );
    }

    #[test]
    fn action_envelope_carries_position_and_propagates_version_generation_and_action_failures() {
        let mut positioned = envelope(json!({"type": "notes/set-visibility", "visible": true}));
        positioned["expectedStateRevision"] = json!(7);
        match parse_workdeck_review_action_envelope(&positioned) {
            WorkdeckReviewParseResult::Parsed(value) => {
                assert_eq!(value.expected_state_revision, Some(7));
            }
            result => panic!("{result:?}"),
        }
        for (key, value, reason) in [
            (
                "protocolVersion",
                json!(2),
                WorkdeckReviewParseFailureReason::Invalid,
            ),
            (
                "generation",
                json!("gen-3"),
                WorkdeckReviewParseFailureReason::Invalid,
            ),
        ] {
            let mut invalid = envelope(json!({"type": "notes/set-visibility", "visible": true}));
            invalid[key] = value;
            assert_failed(parse_workdeck_review_action_envelope(&invalid), reason);
        }
        assert_failed(
            parse_workdeck_review_action_envelope(&envelope(json!({"type": "trust/decide"}))),
            WorkdeckReviewParseFailureReason::Unsupported,
        );
    }

    #[test]
    fn resource_read_envelope_reuses_the_shared_window_parser() {
        let mut value = json!({
            "protocolVersion": WORKDECK_REVIEW_PROTOCOL_VERSION,
            "actor": actor(),
            "request": {
                "generation": GENERATION,
                "resourceId": format!("resource:patch:{FILE_KEY}"),
                "offset": 0,
                "length": 1024,
            }
        });
        assert!(matches!(
            parse_workdeck_review_resource_read_envelope(&value),
            WorkdeckReviewParseResult::Parsed(_)
        ));
        value["request"]["length"] = json!(1024 * 1024);
        assert_failed(
            parse_workdeck_review_resource_read_envelope(&value),
            WorkdeckReviewParseFailureReason::Invalid,
        );
    }

    fn patch_descriptor() -> Value {
        json!({
            "id": review_resource_id(&ReviewResourceAddress {
                kind: ReviewResourceKind::Patch,
                file_key: FILE_KEY.into(),
                side: None,
            }),
            "generation": GENERATION,
            "fileKey": FILE_KEY,
            "kind": "patch",
            "contentType": REVIEW_PATCH_CONTENT_TYPE,
        })
    }

    #[test]
    fn resource_descriptors_require_coherent_ids_content_types_and_complete_measurements() {
        let patch = patch_descriptor();
        assert_eq!(
            serde_json::to_value(parse_workdeck_review_resource_descriptor(&patch).unwrap())
                .unwrap(),
            patch
        );
        let mut measured = patch.clone();
        measured["byteLength"] = json!(12);
        measured["digest"] = json!("a".repeat(64));
        assert!(parse_workdeck_review_resource_descriptor(&measured).is_some());
        for invalid in [
            {
                let mut value = patch.clone();
                value["byteLength"] = json!(12);
                value
            },
            {
                let mut value = patch.clone();
                value["digest"] = json!("a".repeat(64));
                value
            },
            {
                let mut value = measured.clone();
                value["digest"] = json!("A".repeat(64));
                value
            },
            {
                let mut value = patch.clone();
                value["kind"] = json!("canonical-file");
                value
            },
            {
                let mut value = patch.clone();
                value["contentType"] = json!(REVIEW_SOURCE_CONTENT_TYPE);
                value
            },
        ] {
            assert_eq!(parse_workdeck_review_resource_descriptor(&invalid), None);
        }
    }

    #[test]
    fn source_descriptor_requires_the_id_side_and_source_identity_to_agree() {
        let source = json!({
            "id": review_resource_id(&ReviewResourceAddress {
                kind: ReviewResourceKind::Source,
                file_key: FILE_KEY.into(),
                side: Some(ReviewSide::New),
            }),
            "generation": GENERATION,
            "fileKey": FILE_KEY,
            "kind": "source",
            "contentType": REVIEW_SOURCE_CONTENT_TYPE,
            "side": "new",
            "sourceIdentity": "source:abc",
        });
        assert!(parse_workdeck_review_resource_descriptor(&source).is_some());
        let mut side = source.clone();
        side["side"] = json!("old");
        assert_eq!(parse_workdeck_review_resource_descriptor(&side), None);
        let mut identity = source;
        identity.as_object_mut().unwrap().remove("sourceIdentity");
        assert_eq!(parse_workdeck_review_resource_descriptor(&identity), None);
    }

    fn resource_catalog() -> Value {
        let canonical = json!({
            "id": review_resource_id(&ReviewResourceAddress {
                kind: ReviewResourceKind::CanonicalFile,
                file_key: FILE_KEY.into(),
                side: None,
            }),
            "generation": GENERATION,
            "fileKey": FILE_KEY,
            "kind": "canonical-file",
            "contentType": REVIEW_CANONICAL_FILE_CONTENT_TYPE,
        });
        json!({
            "generation": GENERATION,
            "fileKeysByRuntimeId": {"file-1": FILE_KEY},
            "resources": [canonical, patch_descriptor()],
        })
    }

    #[test]
    fn resource_catalog_accepts_one_generation_and_bounds_files_by_resources() {
        let catalog = resource_catalog();
        assert_eq!(
            serde_json::to_value(parse_workdeck_review_resource_catalog(&catalog).unwrap())
                .unwrap(),
            catalog
        );
        let mut generation = catalog.clone();
        generation["resources"][0]["generation"] = json!("generation:p1:4");
        assert_eq!(parse_workdeck_review_resource_catalog(&generation), None);
        let mut files = catalog;
        files["fileKeysByRuntimeId"] = json!({"a": "1", "b": "2", "c": "3"});
        files["resources"].as_array_mut().unwrap().truncate(1);
        assert_eq!(parse_workdeck_review_resource_catalog(&files), None);
    }

    #[test]
    fn publication_address_accepts_exactly_a_generation_and_nonnegative_safe_revision() {
        let valid = json!({"generation": GENERATION, "stateRevision": 4});
        assert_eq!(
            parse_workdeck_review_publication_address(&valid),
            Some(ReviewPublicationAddress {
                generation: GENERATION.into(),
                state_revision: 4,
            })
        );
        assert_eq!(
            parse_workdeck_review_publication_address(
                &json!({"generation": GENERATION, "stateRevision": -1})
            ),
            None
        );
        assert_eq!(
            parse_workdeck_review_publication_address(
                &json!({"generation": GENERATION, "stateRevision": 4, "extra": 1})
            ),
            None
        );
    }

    #[test]
    fn note_transport_uses_the_shared_whole_value_byte_bound() {
        let two_thirds = "x".repeat(MAX_REVIEW_NOTE_BYTES * 7 / 10);
        let oversized = json!({
            "id": "user:1",
            "source": "user",
            "fileKey": FILE_KEY,
            "anchor": {"intersectingHunkIndices": [0], "ownerHunkIndex": 0},
            "summary": two_thirds,
            "rationale": two_thirds,
            "markup": two_thirds,
            "editable": true,
        });
        for field in ["summary", "rationale", "markup"] {
            assert!(oversized[field].as_str().unwrap().len() <= MAX_REVIEW_NOTE_BYTES);
        }
        assert!(!review_note_within_size_limit(&oversized));
        assert!(review_note_within_size_limit(&json!({
            "id": "user:1",
            "source": "user",
            "fileKey": FILE_KEY,
            "anchor": {"intersectingHunkIndices": [0], "ownerHunkIndex": 0},
            "summary": "Tighten this wording",
            "editable": true,
        })));
    }
}
