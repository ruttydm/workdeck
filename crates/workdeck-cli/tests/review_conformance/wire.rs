//! Hunk MIT wire-action corpus, parsed and lowered by the native session protocol.

use super::models::{ReviewWireFixture, ReviewWireParseOutcome};
use serde_json::Value;
use workdeck_session::{
    WorkdeckReviewParseResult, parse_workdeck_review_action, to_review_intent_value,
    to_semantic_review_intent,
};

type ActionParser = fn(&serde_json::Map<String, Value>) -> ReviewWireParseOutcome;
type NotePolicy = fn(&workdeck_core::SemanticReviewNote) -> bool;
#[derive(Clone, Copy)]
pub(super) struct WireProjections {
    pub parse_action: ActionParser,
    pub accepts_note: NotePolicy,
}

pub(super) const CONSUMER: super::models::Consumer<WireProjections> = super::models::Consumer::new(
    "review wire protocol",
    "Phase 3",
    WireProjections {
        parse_action,
        accepts_note: workdeck_review::review_note_within_size_limit,
    },
);

fn parse_action(input: &serde_json::Map<String, Value>) -> ReviewWireParseOutcome {
    match parse_workdeck_review_action(&Value::Object(input.clone())) {
        WorkdeckReviewParseResult::Parsed(action) => {
            assert!(
                to_semantic_review_intent(&action).is_some(),
                "typed lowering"
            );
            let value = to_review_intent_value(&action);
            let intent = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(serde_json::to_value(&intent).unwrap(), value);
            ReviewWireParseOutcome {
                accepted: true,
                intent: Some(intent),
            }
        }
        WorkdeckReviewParseResult::Failed(_) => ReviewWireParseOutcome {
            accepted: false,
            intent: None,
        },
    }
}

fn fixture(case: &Value) -> ReviewWireFixture {
    let id = case["id"].as_str().unwrap();
    // Source descriptions explain the semantic boundary, independently of the
    // captured parser output. Never synthesize expectations from the consumer.
    let description = match id {
        "select-hunk" => "Selecting one hunk, with the reveal the caller wants stated explicitly.",
        "move-annotated-hunk-backwards" => {
            "Relative navigation, whose scope and wrap policy are core's to decide."
        }
        "select-file" => "A file jump with no reveal stated; the file-jump rule supplies one.",
        "anchor-selection" => {
            "Adopting the position a remote viewport settled on, which moves no viewport."
        }
        "set-filter" => "Filtering is shared review state, so it travels as an action.",
        "set-note-visibility" => "Note-layer visibility, likewise shared.",
        "start-draft-on-a-hunk" => {
            "Opening a draft with no line stated; the whole-hunk default supplies one."
        }
        "start-edit-draft" => "Opening one saved reviewer note for identity-preserving editing.",
        "start-reply-draft" => "Opening a reply composer beneath one semantically stored note.",
        "update-draft-body" => "Transporting composer text through the shared semantic path.",
        "cancel-draft" => "Cancelling the one active shared composer.",
        "start-draft-on-an-expanded-line" => {
            "A draft on a line the patch does not contain, carrying the proof that makes it addressable — the case the prototype's browser could not express at all."
        }
        "start-draft-with-a-proof-about-nothing" => {
            "Evidence for a line the action does not name is malformed, not tolerated."
        }
        "create-user-note" => "Persisting the active draft, with no precondition on where it sits.",
        "update-user-note" => "Committing an edit against the same saved reviewer note.",
        "create-user-note-at-an-expanded-line" => {
            "Saving the draft opened on an expanded line, restating where it is as a precondition so two clients cannot save each other's drafts."
        }
        "remove-user-note" => "Removing one note the reviewer wrote.",
        "remove-live-note" => "Removing one note an agent contributed.",
        "clear-notes-for-one-file" => "A scoped clear, including the reviewer's own notes.",
        "toggle-gap" => "Expanding one addressable collapsed gap.",
        "unknown-field-on-a-known-action" => {
            "A field the intent does not have is refused rather than ignored, so a field added on one side cannot be silently dropped on the other."
        }
        "missing-field-on-a-known-action" => {
            "An action missing what its intent requires is refused before planning."
        }
        _ => panic!("untranslated wire fixture: {id}"),
    };
    ReviewWireFixture {
        id: id.into(),
        findings: serde_json::from_value(case["findings"].clone()).unwrap(),
        description: description.into(),
        action: case["input"]["action"].as_object().unwrap().clone(),
        expected: super::models::wire_outcome(&case["expected"]),
    }
}

#[test]
fn wire_actions_lower_to_the_pinned_intents_and_reject_invalid_shapes() {
    for (encoded, expected_count) in [
        (
            include_str!("../../../../port/hunk/oracles/review-conformance-main.json"),
            22,
        ),
        (
            include_str!("../../../../port/hunk/oracles/review-conformance-stable.json"),
            17,
        ),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let mut count = 0;
        for case in oracle["results"].as_array().unwrap() {
            if case["group"] != "wire" {
                continue;
            }
            let fixture = fixture(case);
            assert_eq!(fixture.id, case["id"].as_str().unwrap());
            assert!(!fixture.description.is_empty());
            assert!(!fixture.findings.is_empty());
            let actual = (CONSUMER.project.parse_action)(&fixture.action);
            assert_eq!(actual, fixture.expected);
            let actual = serde_json::to_value(actual).unwrap();
            assert_eq!(
                actual, case["expected"],
                "{}: {}",
                oracle["upstream"], case["id"]
            );
            let captured = case["actual"]
                .as_array()
                .unwrap()
                .iter()
                .find(|consumer| consumer["consumer"] == CONSUMER.name)
                .unwrap();
            assert_eq!(actual, captured["output"], "captured wire: {}", case["id"]);
            count += 1;
        }
        assert_eq!(count, expected_count);
    }
}
