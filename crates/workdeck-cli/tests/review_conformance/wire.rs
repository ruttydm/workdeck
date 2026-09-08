//! Hunk MIT wire-action corpus, parsed and lowered by the native session protocol.

use serde_json::{Value, json};
use workdeck_session::{
    WorkdeckReviewParseResult, parse_workdeck_review_action, to_review_intent_value,
    to_semantic_review_intent,
};

type ActionParser = fn(&Value) -> Value;
type NotePolicy = fn(&Value) -> bool;
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

fn parse_action(input: &Value) -> Value {
    match parse_workdeck_review_action(input) {
        WorkdeckReviewParseResult::Parsed(action) => {
            assert!(
                to_semantic_review_intent(&action).is_some(),
                "typed lowering"
            );
            json!({"accepted": true, "intent": to_review_intent_value(&action)})
        }
        WorkdeckReviewParseResult::Failed(_) => json!({"accepted": false}),
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
            let actual = (CONSUMER.project.parse_action)(&case["input"]["action"]);
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
