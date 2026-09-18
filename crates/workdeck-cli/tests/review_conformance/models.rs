//! Incremental translation of Hunk's MIT conformance interfaces.
//! Keep projections renderer-neutral and compare them without dropping fields.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use workdeck_core::ReviewSide;

/// One registered native adapter, including the upstream phase that introduced it.
/// `Project` carries its family's callable signature or named callback bundle.
/// Phase labels record upstream history, not the Workdeck SDK's version.
#[derive(Clone, Copy)]
pub(super) struct Consumer<Project> {
    pub name: &'static str,
    pub phase: &'static str,
    pub project: Project,
}

impl<Project> Consumer<Project> {
    pub const fn new(name: &'static str, phase: &'static str, project: Project) -> Self {
        Self {
            name,
            phase,
            project,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceGap {
    gap_id: String,
    old_range: [u32; 2],
    new_range: [u32; 2],
    line_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceLineAddress {
    side: ReviewSide,
    line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceHunkRanges {
    old_range: [u32; 2],
    new_range: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceExpandedRow {
    old_line: u32,
    new_line: u32,
    text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceFileProjection {
    path: String,
    gaps: Vec<ConformanceGap>,
    hunk_ranges: Vec<ConformanceHunkRanges>,
    default_note_targets: Vec<ConformanceLineAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    empty_diff_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    expanded_rows: Option<Vec<ConformanceExpandedRow>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewGeometryProjection {
    files: Vec<ConformanceFileProjection>,
}

pub(super) struct ConformanceExpansion {
    pub file_index: usize,
    pub gap_id: String,
    pub source_text: String,
}

pub(super) struct ReviewGeometryFixture {
    pub id: String,
    pub findings: Vec<String>,
    pub description: String,
    pub build: Box<dyn Fn() -> Vec<workdeck_core::DiffFile>>,
    pub expansion: Option<ConformanceExpansion>,
    /// The source corpus's hand-written expectation, not a consumer result.
    pub expected: ReviewGeometryProjection,
}

pub(super) fn geometry(value: &Value) -> ReviewGeometryProjection {
    checked(value)
}

fn checked<T: serde::de::DeserializeOwned + Serialize>(value: &Value) -> T {
    let projection: T =
        serde_json::from_value(value.clone()).expect("complete typed conformance projection");
    assert_eq!(
        serde_json::to_value(&projection).unwrap(),
        *value,
        "typing must not normalize away fields, nulls, or content"
    );
    projection
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceSelection {
    file: Option<usize>,
    hunk_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceReveal {
    anchor: workdeck_review::ReviewRevealAnchor,
    scroll_to_note: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ConformanceMoveOutcome {
    to: Option<ConformanceSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reveal: Option<ConformanceReveal>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReviewNavigationProjection {
    moves: Vec<ConformanceMoveOutcome>,
    normalized_selections: Vec<ConformanceSelection>,
    reveal_targets: Vec<Vec<Option<ConformanceLineAddress>>>,
}

pub(super) fn navigation(value: &Value) -> ReviewNavigationProjection {
    checked(value)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SnapshotFile {
    file_key: String,
    content_identity: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SnapshotNote {
    id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    parent_id: Option<String>,
    file_key: String,
    resolution: workdeck_review::ReviewNoteResolution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    preferred: Option<ConformanceLineAddress>,
    intersecting_hunk_indices: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    owner_hunk_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReviewSnapshotProjection {
    generation: String,
    state_revision: u64,
    files: Vec<SnapshotFile>,
    notes: Vec<SnapshotNote>,
}

pub(super) fn snapshot(value: &Value) -> ReviewSnapshotProjection {
    checked(value)
}

pub(super) struct ReviewSnapshotFixture {
    pub id: String,
    pub findings: Vec<String>,
    pub description: String,
    pub generation: String,
    pub build: Box<dyn Fn() -> workdeck_review::ReviewState>,
    pub expected: ReviewSnapshotProjection,
}

/// The source intent JSON retains `consumeDraft` but excludes wire-only proofs
/// and save preconditions. Keep a distinct type so those fields cannot leak into
/// expectations just because the broader native action model accepts them.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub(super) struct ConformanceReviewIntent(workdeck_session::WorkdeckReviewActionV1);

impl<'de> Deserialize<'de> for ConformanceReviewIntent {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let action: workdeck_session::WorkdeckReviewActionV1 =
            serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?;
        if workdeck_session::to_review_intent_value(&action) != value {
            return Err(serde::de::Error::custom(
                "intent contains wire-only or lossy fields",
            ));
        }
        if value
            .get("consumeDraft")
            .is_some_and(|consume| consume != &Value::Bool(true))
        {
            return Err(serde::de::Error::custom("consumeDraft must be true"));
        }
        Ok(Self(action))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReviewWireParseOutcome {
    pub accepted: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<ConformanceReviewIntent>,
}

pub(super) fn wire_outcome(value: &Value) -> ReviewWireParseOutcome {
    checked(value)
}

pub(super) struct ReviewWireFixture {
    pub id: String,
    pub findings: Vec<String>,
    pub description: String,
    pub action: serde_json::Map<String, Value>,
    pub expected: ReviewWireParseOutcome,
}

/// A typed note with wire field presence retained. The domain model deliberately
/// omits empty tags; the source wire size contract counts an explicit empty list.
#[derive(Debug, Clone)]
pub(super) struct ConformanceReviewNote {
    note: workdeck_core::SemanticReviewNote,
    tags: Option<Vec<String>>,
}

impl Serialize for ConformanceReviewNote {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::to_value(&self.note).map_err(serde::ser::Error::custom)?;
        if let Some(tags) = &self.tags {
            value["tags"] = serde_json::to_value(tags).map_err(serde::ser::Error::custom)?;
        }
        value.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for ConformanceReviewNote {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let tags = value
            .get("tags")
            .map(|tags| serde_json::from_value(tags.clone()))
            .transpose()
            .map_err(serde::de::Error::custom)?;
        let note = serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?;
        let result = Self { note, tags };
        // Reject unsupported fields/nulls instead of silently shrinking the wire body.
        if serde_json::to_value(&result).map_err(serde::de::Error::custom)? != value {
            return Err(serde::de::Error::custom(
                "note typing changed wire field presence",
            ));
        }
        Ok(result)
    }
}

#[derive(Clone, Copy)]
pub(super) enum ConformanceFilePosition {
    Index(usize),
    Vanished,
    None,
}

#[derive(Clone, Copy)]
pub(super) struct ConformanceSelectionInput(pub ConformanceFilePosition, pub usize);

#[derive(Clone)]
pub(super) struct ConformanceMove {
    pub scope: workdeck_review::ReviewSelectionScope,
    pub delta: isize,
    pub from: ConformanceSelectionInput,
}

pub(super) struct ReviewNavigationFixture {
    pub id: String,
    pub findings: Vec<String>,
    pub description: String,
    pub build: Box<dyn Fn() -> Vec<workdeck_core::DiffFile>>,
    pub filter: Option<String>,
    pub annotated_hunks: Option<std::collections::BTreeMap<usize, Vec<usize>>>,
    pub annotated_files: Option<Vec<usize>>,
    pub moves: Vec<ConformanceMove>,
    pub selections: Vec<ConformanceSelectionInput>,
    pub expected: ReviewNavigationProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReviewEventFramingProjection {
    frames: Vec<String>,
    resumable_frames: usize,
    round_trips: bool,
}

pub(super) fn event_framing(value: &Value) -> ReviewEventFramingProjection {
    checked(value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EventWindow {
    Bytes(u64),
    PayloadSize,
    PayloadSizeMinusOne,
}

pub(super) struct ReviewEventFixture {
    pub id: String,
    pub findings: Vec<String>,
    pub description: String,
    pub body: workdeck_session::WorkdeckReviewPublicationBodyV1,
    pub chunk_bytes: EventWindow,
    pub expected: ReviewEventFramingProjection,
}

#[test]
fn projection_contracts_preserve_required_nulls_and_reject_invalid_variants() {
    use serde_json::json;
    let valid = json!({"moves": [{"to": null}],
        "normalizedSelections": [{"file": null, "hunkIndex": 0}], "revealTargets": [[null]]});
    let _ = navigation(&valid);
    let mut missing = valid.clone();
    missing["moves"][0].as_object_mut().unwrap().remove("to");
    assert!(std::panic::catch_unwind(|| navigation(&missing)).is_err());
    let mut invalid = valid;
    invalid["moves"][0]["reveal"] = json!({"anchor": "screen", "scrollToNote": false});
    assert!(serde_json::from_value::<ReviewNavigationProjection>(invalid).is_err());
    let invalid_note = json!({"generation": "generation:test:1", "stateRevision": 0,
        "files": [], "notes": [{"id": "note", "fileKey": "file", "resolution": "unknown",
            "intersectingHunkIndices": []}]});
    assert!(serde_json::from_value::<ReviewSnapshotProjection>(invalid_note).is_err());
    assert!(
        serde_json::from_value::<ReviewEventFramingProjection>(json!({
            "frames": ["publication"], "resumableFrames": "one", "roundTrips": true
        }))
        .is_err()
    );
}

#[test]
fn intent_contract_excludes_wire_proofs_and_requires_draft_consumption() {
    use serde_json::json;
    let proof = json!({"gapId": "before:1", "side": "new", "line": 7,
        "sourceIdentity": "source:0123456789abcdef"});
    let draft = json!({"type": "notes/start-draft", "fileKey": "file:0123456789abcdef",
        "hunkIndex": 1, "target": {"side": "new", "line": 7}});
    let typed: ConformanceReviewIntent = serde_json::from_value(draft.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), draft);
    let mut with_proof = draft;
    with_proof["expandedLineProof"] = proof;
    assert!(serde_json::from_value::<ConformanceReviewIntent>(with_proof).is_err());
    let save = json!({"type": "notes/create-user", "consumeDraft": true});
    let typed: ConformanceReviewIntent = serde_json::from_value(save.clone()).unwrap();
    assert_eq!(serde_json::to_value(typed).unwrap(), save);
    let mut with_target = save;
    with_target["target"] = json!({"side": "new", "line": 7});
    assert!(serde_json::from_value::<ConformanceReviewIntent>(with_target).is_err());
    for kind in ["notes/create-user", "notes/update-user"] {
        let mut invalid = json!({"type": kind, "consumeDraft": false});
        if kind == "notes/update-user" {
            invalid["noteId"] = json!("user:1");
        }
        assert!(serde_json::from_value::<ConformanceReviewIntent>(invalid).is_err());
    }
}

#[test]
fn geometry_contract_rejects_lost_fields_wrong_ranges_and_renderer_specific_data() {
    use serde_json::json;
    let valid = json!({"files": [{"path": "a.rs", "gaps": [], "hunkRanges": [],
        "defaultNoteTargets": [], "expandedRows": [{"oldLine": 1, "newLine": 1, "text": "界"}]}]});
    let _ = geometry(&valid);
    let mut extra = valid.clone();
    extra["files"][0]["terminalWidth"] = json!(80);
    assert!(serde_json::from_value::<ReviewGeometryProjection>(extra).is_err());
    let mut missing = valid.clone();
    missing["files"][0].as_object_mut().unwrap().remove("path");
    assert!(serde_json::from_value::<ReviewGeometryProjection>(missing).is_err());
    let mut range = valid.clone();
    range["files"][0]["hunkRanges"] = json!([{"oldRange": [1], "newRange": [1, 2]}]);
    assert!(serde_json::from_value::<ReviewGeometryProjection>(range).is_err());
    let mut side = valid.clone();
    side["files"][0]["defaultNoteTargets"] = json!([{"side": "left", "line": 1}]);
    assert!(serde_json::from_value::<ReviewGeometryProjection>(side).is_err());
    let mut negative = valid.clone();
    negative["files"][0]["expandedRows"][0]["oldLine"] = json!(-1);
    assert!(serde_json::from_value::<ReviewGeometryProjection>(negative).is_err());
    let mut null = valid;
    null["files"][0]["expandedRows"] = Value::Null;
    assert!(std::panic::catch_unwind(|| geometry(&null)).is_err());
}
