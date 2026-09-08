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
