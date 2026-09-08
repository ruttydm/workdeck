//! Complete Hunk MIT note-body and whole-note-size fixture inputs.

use std::sync::Arc;

use serde_json::{Value, json};
use workdeck_core::{ReviewSide, SemanticReviewDocument, project_review_file};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::{
    MAX_REVIEW_NOTE_BYTES, ReviewDraftKind, ReviewDraftNote, ReviewIntentFacts,
    SemanticReviewAction, SemanticReviewIntent, SemanticReviewState, is_blank_review_note_body,
    plan_semantic_review_intent, review_note_within_size_limit,
};

fn note_document() -> Arc<SemanticReviewDocument> {
    let file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "before",
            contents: "a\nB\nc\n",
            name: "alpha.ts",
        },
        FileSnapshot {
            cache_key: "after",
            contents: "a\nb\nc\n",
            name: "alpha.ts",
        },
        FileComparisonOptions { context_radius: 1 },
    )
    .unwrap();
    Arc::new(SemanticReviewDocument {
        files: vec![project_review_file(&file, "conformance", 0)],
    })
}

#[test]
fn blank_body_policy_and_draft_retirement_match_every_source_fixture() {
    let document = note_document();
    for (id, body, blank) in [
        ("empty", "", true),
        ("spaces", "   ", true),
        ("newlines", "\n\n", true),
        ("tabs-and-newlines", "\t \r\n ", true),
        ("unicode-space", "\u{a0}", true),
        ("single-character", "x", false),
        ("padded-text", "  needs a test  ", false),
        ("markup-only", "<hr/>", false),
    ] {
        assert_eq!(is_blank_review_note_body(body), blank, "{id}");
        let mut state = SemanticReviewState::new(document.clone(), true);
        state.draft_note = Some(ReviewDraftNote {
            id: "draft:1".into(),
            file_key: document.files[0].key.clone(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 1,
            body: body.into(),
            kind: ReviewDraftKind::Create,
        });
        let plan = plan_semantic_review_intent(
            &state,
            SemanticReviewIntent::CreateUserNote,
            &ReviewIntentFacts {
                note_id: Some("user:1".into()),
                timestamp: Some("2024-01-01T00:00:00.000Z".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(plan.actions.len(), 1, "{id}");
        if blank {
            assert!(
                matches!(plan.actions[0], SemanticReviewAction::CancelDraft),
                "{id}"
            );
        } else {
            assert!(
                matches!(plan.actions[0], SemanticReviewAction::SaveDraft(_)),
                "{id}"
            );
        }
        let action = match plan.actions[0] {
            SemanticReviewAction::CancelDraft => "draft/cancel",
            SemanticReviewAction::SaveDraft(_) => "draft/save",
            _ => panic!("unexpected draft action for {id}"),
        };
        check_oracles(
            "note-body",
            id,
            &json!({"body": body}),
            &json!({"blank": is_blank_review_note_body(body), "actions": [action]}),
            8,
            None,
        );
    }
}

fn minimal_note() -> Value {
    json!({"id": "note:1", "source": "user", "fileKey": "file:abc",
        "anchor": {"intersectingHunkIndices": [0], "ownerHunkIndex": 0},
        "summary": "", "editable": true})
}

#[test]
fn whole_note_size_counts_framing_combined_fields_and_utf8() {
    let framing = serde_json::to_vec(&minimal_note()).unwrap().len();
    for (id, summary, other_fields, fits) in [
        ("empty-note", String::new(), false, true),
        (
            "whole-note-exactly-at-the-bound",
            "x".repeat(MAX_REVIEW_NOTE_BYTES - framing),
            false,
            true,
        ),
        (
            "whole-note-one-byte-over",
            "x".repeat(MAX_REVIEW_NOTE_BYTES - framing + 1),
            false,
            false,
        ),
        (
            "every-field-fits-but-the-note-does-not",
            "x".repeat(MAX_REVIEW_NOTE_BYTES - 1),
            true,
            false,
        ),
        (
            "multibyte-summary-under-the-per-character-limit",
            "🧪".repeat(MAX_REVIEW_NOTE_BYTES / 4),
            false,
            false,
        ),
        (
            "multibyte-summary-just-inside",
            "🧪".repeat((MAX_REVIEW_NOTE_BYTES - framing) / 4),
            false,
            true,
        ),
    ] {
        let mut note = minimal_note();
        note["summary"] = json!(summary);
        if other_fields {
            note["rationale"] = json!("x".repeat(MAX_REVIEW_NOTE_BYTES - 1));
            note["markup"] = json!("x".repeat(MAX_REVIEW_NOTE_BYTES - 1));
        }
        assert_eq!(review_note_within_size_limit(&note), fits, "{id}");
        let bytes = serde_json::to_vec(&note).unwrap().len();
        assert_eq!(
            bytes <= MAX_REVIEW_NOTE_BYTES,
            fits,
            "serialized bytes: {id}"
        );
        if id == "whole-note-exactly-at-the-bound" {
            assert_eq!(bytes, MAX_REVIEW_NOTE_BYTES);
        } else if id == "whole-note-one-byte-over" {
            assert_eq!(bytes, MAX_REVIEW_NOTE_BYTES + 1);
        }
        let typed_note: super::models::ConformanceReviewNote =
            serde_json::from_value(note.clone()).unwrap();
        assert_eq!(
            serde_json::to_value(&typed_note).unwrap(),
            note,
            "typing must preserve the exact note-size fixture: {id}"
        );
        check_oracles(
            "note-size",
            id,
            &json!({"maxReviewNoteBytes": MAX_REVIEW_NOTE_BYTES, "serializedBytes": bytes}),
            &json!(review_note_within_size_limit(&note)),
            6,
            Some(json!((super::wire::CONSUMER.project.accepts_note)(
                &typed_note
            ))),
        );
    }
}

#[test]
fn typed_wire_note_size_preserves_explicit_empty_tags_at_the_boundary() {
    let mut note = minimal_note();
    let framing = serde_json::to_vec(&note).unwrap().len();
    note["summary"] = json!("x".repeat(MAX_REVIEW_NOTE_BYTES - framing));
    let typed: super::models::ConformanceReviewNote = serde_json::from_value(note.clone()).unwrap();
    assert!((super::wire::CONSUMER.project.accepts_note)(&typed));
    note["tags"] = json!([]);
    let typed: super::models::ConformanceReviewNote = serde_json::from_value(note.clone()).unwrap();
    assert_eq!(serde_json::to_value(&typed).unwrap(), note);
    assert!(!review_note_within_size_limit(&note));
    assert!(!(super::wire::CONSUMER.project.accepts_note)(&typed));
    note["tags"] = Value::Null;
    assert!(serde_json::from_value::<super::models::ConformanceReviewNote>(note.clone()).is_err());
    note.as_object_mut().unwrap().remove("tags");
    note["unrecognized"] = json!(true);
    assert!(serde_json::from_value::<super::models::ConformanceReviewNote>(note).is_err());
}

fn check_oracles(
    group: &str,
    id: &str,
    input: &Value,
    actual: &Value,
    expected_count: usize,
    wire_actual: Option<Value>,
) {
    for encoded in [
        include_str!("../../../../port/hunk/oracles/review-conformance-main.json"),
        include_str!("../../../../port/hunk/oracles/review-conformance-stable.json"),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let cases = oracle["results"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| case["group"] == group)
            .collect::<Vec<_>>();
        assert_eq!(cases.len(), expected_count);
        let case = cases.into_iter().find(|case| case["id"] == id).unwrap();
        assert_eq!(input, &case["input"], "input/measurement {group}/{id}");
        assert_eq!(
            actual, &case["expected"],
            "{}: {group}/{id}",
            oracle["upstream"]
        );
        let consumers = case["actual"].as_array().unwrap();
        assert_eq!(consumers.len(), if group == "note-size" { 2 } else { 1 });
        for consumer in consumers {
            let actual = if consumer["consumer"] == "review wire note size" {
                wire_actual
                    .as_ref()
                    .expect("registered wire note policy was executed")
            } else {
                actual
            };
            assert_eq!(
                actual, &consumer["output"],
                "captured {}: {id}",
                consumer["consumer"]
            );
        }
    }
}
