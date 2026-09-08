//! Hunk MIT publication-ordering corpus through core, broker, and producer.

use serde_json::{Value, json};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::{
    PublishReviewInput, REVIEW_PATCH_CONTENT_TYPE, ReviewIntentFacts, ReviewProducer,
    ReviewProducerOptions, ReviewPublicationAddress, ReviewPublicationOrder, SemanticReviewIntent,
    SemanticReviewStore, classify_review_publication,
};
use workdeck_session::{
    ObserveReviewPublicationInput, ReviewMirror, ReviewMirrorUpdate,
    WorkdeckReviewResourceCatalogV1,
};

fn verdict(order: ReviewPublicationOrder) -> &'static str {
    match order {
        ReviewPublicationOrder::Accepted => "accepted",
        ReviewPublicationOrder::Gap => "gap",
        ReviewPublicationOrder::Stale => "stale",
    }
}

fn catalog(generation: &str) -> WorkdeckReviewResourceCatalogV1 {
    use workdeck_review::{ReviewResourceAddress, ReviewResourceKind, review_resource_id};
    let file_key = "file:0123456789abcdef";
    let id = review_resource_id(&ReviewResourceAddress {
        kind: ReviewResourceKind::Patch,
        file_key: file_key.into(),
        side: None,
    });
    // Typed construction intentionally permits the invalid-generation fixture;
    // ordering, not catalog parsing, is the boundary being exercised here.
    serde_json::from_value(json!({
        "generation": generation,
        "fileKeysByRuntimeId": {"file-1": file_key},
        "resources": [{"id": id, "generation": generation, "fileKey": file_key,
            "kind": "patch", "contentType": REVIEW_PATCH_CONTENT_TYPE}],
    }))
    .unwrap()
}

fn mirror_verdict(
    current: &ReviewPublicationAddress,
    incoming: &ReviewPublicationAddress,
) -> &'static str {
    let mut mirror = ReviewMirror::new();
    let initial = mirror.observe(ObserveReviewPublicationInput {
        session_id: "session-1",
        catalog: Some(&catalog(&current.generation)),
        address: Some(current),
    });
    assert!(matches!(initial, ReviewMirrorUpdate::Adopted { .. }));
    let update = mirror.observe(ObserveReviewPublicationInput {
        session_id: "session-1",
        catalog: Some(&catalog(&incoming.generation)),
        address: Some(incoming),
    });
    match update {
        ReviewMirrorUpdate::Advanced { .. } => "accepted",
        ReviewMirrorUpdate::Replaced { .. } => "gap",
        ReviewMirrorUpdate::Ignored => "stale",
        ReviewMirrorUpdate::Adopted { .. } => panic!("existing session cannot be adopted twice"),
    }
}

fn producer_verdicts(steps: &[Value]) -> Value {
    let file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "before",
            contents: "alpha\n",
            name: "example.ts",
        },
        FileSnapshot {
            cache_key: "after",
            contents: "beta\n",
            name: "example.ts",
        },
        FileComparisonOptions { context_radius: 0 },
    )
    .unwrap();
    let input = PublishReviewInput {
        files: vec![file],
        source_label: Some("/repo".into()),
    };
    let producer = ReviewProducer::new(
        input.clone(),
        ReviewProducerOptions {
            producer_id: Some("conformance".into()),
            ..Default::default()
        },
    )
    .unwrap();
    producer.attach_store(SemanticReviewStore::new(
        producer.get_publication().document.clone(),
        true,
    ));
    let mut previous = producer.get_publication_address();
    let output = steps
        .iter()
        .enumerate()
        .map(|(index, step)| {
            match step.as_str().unwrap() {
                "reload" => {
                    let publication = producer.publish(&input).unwrap();
                    producer
                        .attach_store(SemanticReviewStore::new(publication.document.clone(), true));
                }
                "state" => {
                    producer
                        .apply_intent(
                            SemanticReviewIntent::SetFilter(format!("step-{index}")),
                            ReviewIntentFacts::default(),
                        )
                        .unwrap();
                }
                other => panic!("unknown producer step {other}"),
            }
            let next = producer.get_publication_address();
            let result = verdict(classify_review_publication(&previous, &next));
            previous = next;
            result
        })
        .collect::<Vec<_>>();
    json!(output)
}

#[test]
fn core_broker_and_producer_ordering_match_both_pinned_corpora() {
    for encoded in [
        include_str!("../../../../port/hunk/oracles/review-conformance-main.json"),
        include_str!("../../../../port/hunk/oracles/review-conformance-stable.json"),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let mut ordering_count = 0;
        let mut producer_count = 0;
        for case in oracle["results"].as_array().unwrap() {
            let actual = match case["group"].as_str().unwrap() {
                "ordering" => {
                    let current = serde_json::from_value(case["input"]["current"].clone()).unwrap();
                    let incoming =
                        serde_json::from_value(case["input"]["incoming"].clone()).unwrap();
                    ordering_count += 1;
                    vec![
                        (
                            "core publication ordering",
                            json!(verdict(classify_review_publication(&current, &incoming))),
                        ),
                        (
                            "broker review mirror",
                            json!(mirror_verdict(&current, &incoming)),
                        ),
                    ]
                }
                "producer-ordering" => {
                    producer_count += 1;
                    vec![(
                        "producer ordering",
                        producer_verdicts(case["input"]["steps"].as_array().unwrap()),
                    )]
                }
                _ => continue,
            };
            for (name, output) in actual {
                assert_eq!(
                    output, case["expected"],
                    "{}: {name}: {}",
                    case["id"], oracle["upstream"]
                );
                let captured = case["actual"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|consumer| consumer["consumer"] == name)
                    .unwrap();
                assert_eq!(
                    output, captured["output"],
                    "captured {name}: {}",
                    case["id"]
                );
            }
        }
        assert_eq!(ordering_count, 10);
        assert_eq!(producer_count, 2);
    }
}
