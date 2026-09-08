//! Translation of Hunk's MIT-licensed cross-consumer review conformance harness.
//! Separate geometry, navigation, ordering, snapshot, wire, and event registries run
//! the full applicable corpus against every registered native consumer. New consumers
//! must join their registry and preserve earlier adversarial cases; these independent
//! projections keep a renderer or transport from defining its own expected semantics.
//! This harness is one upstream test file, not the complete upstream test corpus.

#[path = "review_conformance/events.rs"]
mod events;
#[path = "review_conformance/navigation.rs"]
mod navigation;
#[path = "review_conformance/notes.rs"]
mod notes;
#[path = "review_conformance/ordering.rs"]
mod ordering;
#[path = "review_conformance/snapshot.rs"]
mod snapshot;
#[path = "review_conformance/wire.rs"]
mod wire;

use serde_json::{Value, json};
use workdeck_core::{DiffFile, FileChangeKind, project_review_file, review_empty_diff_reason};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::{
    build_review_content_manifest_file, review_gap_address, review_gap_id, review_leading_gap,
    review_trailing_gap, semantic_review_gap_source,
};

type GeometryProjection = fn(&DiffFile, Option<&str>, &str) -> Value;
const GEOMETRY_CONSUMERS: [(&str, GeometryProjection); 3] = [
    ("core review model", core_projection),
    ("terminal render planning", terminal_projection),
    ("review producer", producer_projection),
];

fn fixture(id: &str) -> (DiffFile, Option<&'static str>, String) {
    let base: Vec<_> = (1..=12).map(|line| format!("line {line}")).collect();
    let mut edited = base.clone();
    let (name, context, expansion) = match id {
        "pure-insertion-hunk" => {
            edited.insert(6, "inserted".into());
            ("insertion.ts", 0, Some("before:0"))
        }
        "pure-deletion-hunk" => {
            edited.remove(5);
            ("deletion.ts", 0, Some("before:0"))
        }
        "hunk-with-leading-context" => ("context.ts", 3, None),
        "crlf-source" => ("crlf.ts", 0, Some("trailing:0")),
        "source-without-trailing-newline" => ("unterminated.ts", 0, Some("trailing:0")),
        "binary-rename-with-no-rows" => ("asset.png", 0, None),
        _ => panic!("untranslated geometry fixture: {id}"),
    };
    if matches!(name, "context.ts" | "crlf.ts" | "unterminated.ts") {
        edited[5] = "line six".into();
    }
    let encode = |lines: &[String]| {
        let separator = if name == "crlf.ts" { "\r\n" } else { "\n" };
        let mut text = lines.join(separator);
        if name != "unterminated.ts" {
            text.push_str(separator);
        }
        text
    };
    let before = encode(&base);
    let after = encode(&edited);
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "before",
            contents: &before,
            name,
        },
        FileSnapshot {
            cache_key: "after",
            contents: &after,
            name,
        },
        FileComparisonOptions {
            context_radius: context,
        },
    )
    .unwrap();
    if name == "asset.png" {
        file.flags.binary = true;
        file.previous_path = Some("old-asset.png".into());
        file.change_kind = FileChangeKind::Renamed;
        file.hunks.clear();
    }
    (file, expansion, after)
}

fn core_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
    // The upstream core consumer observes the canonical document, not parser output.
    // In particular, zero-count hunk positions must survive canonical projection.
    let file = project_review_file(file, "/repo", 0);
    let source = semantic_review_gap_source(&file);
    let manifest = build_review_content_manifest_file(&file);
    let gaps = (0..file.hunks.len())
        .filter_map(|index| review_leading_gap(&source, index))
        .chain(review_trailing_gap(&source))
        .map(|gap| {
            json!({
                "gapId": review_gap_id(gap.position, gap.hunk_index),
                "oldRange": [gap.old_range.start, gap.old_range.end],
                "newRange": [gap.new_range.start, gap.new_range.end],
                "lineCount": gap.line_count,
            })
        })
        .collect::<Vec<_>>();
    let ranges = manifest
        .hunks
        .iter()
        .map(|hunk| json!({"oldRange": hunk.old_range, "newRange": hunk.new_range}))
        .collect::<Vec<_>>();
    let targets = manifest
        .hunks
        .iter()
        .map(|hunk| {
            let target = hunk.default_note_target;
            json!({"side": target.side, "line": target.line})
        })
        .collect::<Vec<_>>();
    let mut value = json!({"path": file.path, "gaps": gaps,
        "hunkRanges": ranges, "defaultNoteTargets": targets});
    if file.hunks.is_empty() {
        value["emptyDiffReason"] = json!(review_empty_diff_reason(
            file.change_kind,
            file.flags.binary,
            file.flags.too_large
        ));
    }
    if let Some(id) = expansion {
        let gap = review_gap_address(&source, id).expect("fixture gap exists");
        let lines = workdeck_review::normalized_review_source_lines(source_text);
        let range = if manifest.expansion_side == workdeck_core::ReviewSide::Old {
            gap.old_range
        } else {
            gap.new_range
        };
        value["expandedRows"] = json!(
            (0..gap.line_count)
                .map(|offset| json!({
                    "oldLine": gap.old_range.start as usize + offset,
                    "newLine": gap.new_range.start as usize + offset,
                    "text": lines.get(range.start as usize + offset - 1)
                        .map(String::as_str).unwrap_or(""),
                }))
                .collect::<Vec<_>>()
        );
    }
    json!({"files": [value]})
}

#[test]
fn parsed_core_geometry_matches_both_pinned_conformance_oracles() {
    check_geometry_consumer(GEOMETRY_CONSUMERS[0].0, GEOMETRY_CONSUMERS[0].1);
}

#[test]
fn terminal_planner_geometry_matches_both_pinned_conformance_oracles() {
    check_geometry_consumer(GEOMETRY_CONSUMERS[1].0, GEOMETRY_CONSUMERS[1].1);
}

#[test]
fn producer_geometry_matches_both_pinned_conformance_oracles() {
    check_geometry_consumer(GEOMETRY_CONSUMERS[2].0, GEOMETRY_CONSUMERS[2].1);
}

fn producer_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
    use workdeck_core::ReviewSide;
    use workdeck_review::{
        PublishReviewInput, ReviewIntentFacts, ReviewIntentOutcome, ReviewProducer,
        ReviewProducerOptions, SemanticReviewIntent, SemanticReviewStore,
        assert_canonical_file_matches_manifest,
    };
    let producer = ReviewProducer::new(
        PublishReviewInput {
            files: vec![file.clone()],
            source_label: Some("conformance".into()),
        },
        ReviewProducerOptions {
            producer_id: Some("conformance".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let publication = producer.get_publication();
    producer.attach_store(SemanticReviewStore::new(publication.document.clone(), true));
    let manifest = &publication.manifest.files[0];
    assert_canonical_file_matches_manifest(&publication.document.files[0], manifest).unwrap();
    let gaps = manifest
        .hunks
        .iter()
        .filter_map(|hunk| hunk.leading_gap.as_ref())
        .chain(manifest.trailing_gap.as_ref())
        .collect::<Vec<_>>();
    let ranges = manifest
        .hunks
        .iter()
        .map(|hunk| {
            json!({
                "oldRange": hunk.old_range, "newRange": hunk.new_range,
            })
        })
        .collect::<Vec<_>>();
    let targets = manifest
        .hunks
        .iter()
        .map(|hunk| hunk.default_note_target)
        .collect::<Vec<_>>();
    let mut value = json!({"path": manifest.path, "gaps": gaps,
        "hunkRanges": ranges, "defaultNoteTargets": targets});
    if let Some(reason) = manifest.empty_diff_reason {
        value["emptyDiffReason"] = json!(reason);
    }
    if let Some(gap_id) = expansion {
        let outcome = producer
            .apply_intent(
                SemanticReviewIntent::ToggleExpansion {
                    file_key: manifest.key.clone(),
                    gap_id: gap_id.into(),
                },
                ReviewIntentFacts::default(),
            )
            .unwrap();
        let Some(ReviewIntentOutcome::ExpansionToggled {
            old_range,
            new_range,
            line_count,
            side,
            expanded,
            ..
        }) = outcome
        else {
            panic!("expected a producer expansion outcome")
        };
        assert!(expanded);
        let lines = workdeck_review::normalized_review_source_lines(source_text);
        let range = if side == ReviewSide::Old {
            old_range
        } else {
            new_range
        };
        value["expandedRows"] = json!(
            (0..line_count)
                .map(|offset| json!({
                    "oldLine": old_range[0] as usize + offset,
                    "newLine": new_range[0] as usize + offset,
                    "text": lines[range[0] as usize + offset - 1],
                }))
                .collect::<Vec<_>>()
        );
    }
    json!({"files": [value]})
}

fn terminal_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
    use workdeck_core::ReviewEmptyDiffReason;
    use workdeck_diff::DiffRow;
    use workdeck_review::{
        CommentTargetInput, ExpandedSourceStatus, LayoutMode, resolve_comment_target,
    };
    use workdeck_tui::{
        BuildDiffSectionRowPlanOptions, build_diff_section_row_plan, build_selected_hunk_summary,
        diff_message, diff_message_for_reason, resolve_theme,
    };

    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let keys = expansion.into_iter().map(str::to_owned).collect();
    let mut options = BuildDiffSectionRowPlanOptions::new(Some(file), LayoutMode::Split, &theme);
    options.expanded_keys = &keys;
    if expansion.is_some() {
        options.source_status = ExpandedSourceStatus::Loaded(source_text);
    }
    let plan = build_diff_section_row_plan(options);
    let mut gaps = Vec::new();
    let mut expanded_rows = Vec::new();
    for row in plan
        .planned_rows
        .iter()
        .filter_map(|planned| planned.diff_row())
    {
        match row {
            DiffRow::Collapsed {
                position,
                hunk_index,
                old_range,
                new_range,
                ..
            } => {
                gaps.push(json!({
                    "gapId": review_gap_id(*position, *hunk_index),
                    "oldRange": old_range, "newRange": new_range,
                    "lineCount": old_range[1] - old_range[0] + 1,
                }));
            }
            DiffRow::SplitLine {
                left,
                right,
                is_expansion_row: true,
                expanded_gap_key,
                ..
            } if expanded_gap_key.as_deref() == expansion => {
                expanded_rows.push(json!({
                    "oldLine": left.line_number.unwrap_or(0),
                    "newLine": right.line_number.unwrap_or(0),
                    "text": left.spans.iter().map(|span| span.text.as_str()).collect::<String>(),
                }));
            }
            _ => {}
        }
    }
    let ranges = (0..file.hunks.len())
        .map(|index| {
            let summary = build_selected_hunk_summary(file, index);
            json!({"oldRange": summary.old_range.unwrap_or([0, 0]),
            "newRange": summary.new_range.unwrap_or([0, 0])})
        })
        .collect::<Vec<_>>();
    let targets = (0..file.hunks.len())
        .map(|index| {
            let target = resolve_comment_target(
                file,
                &CommentTargetInput {
                    file_path: file.path.clone(),
                    hunk_index: Some(index),
                    side: None,
                    line: None,
                    summary: "conformance".into(),
                    rationale: None,
                    markup: None,
                    author: None,
                },
            )
            .unwrap();
            json!({"side": target.side, "line": target.line})
        })
        .collect::<Vec<_>>();
    let mut value = json!({"path": file.path, "gaps": gaps,
        "hunkRanges": ranges, "defaultNoteTargets": targets});
    if file.hunks.is_empty() {
        let message = diff_message(file);
        let reason = [
            ReviewEmptyDiffReason::RenameOnly,
            ReviewEmptyDiffReason::Binary,
            ReviewEmptyDiffReason::TooLarge,
            ReviewEmptyDiffReason::NewFile,
            ReviewEmptyDiffReason::DeletedFile,
            ReviewEmptyDiffReason::NoHunks,
        ]
        .into_iter()
        .find(|reason| diff_message_for_reason(*reason) == message)
        .expect("known terminal empty diff message");
        value["emptyDiffReason"] = json!(reason);
    }
    if expansion.is_some() {
        value["expandedRows"] = json!(expanded_rows);
    }
    json!({"files": [value]})
}

fn check_geometry_consumer(name: &str, project: fn(&DiffFile, Option<&str>, &str) -> Value) {
    for encoded in [
        include_str!("../../../port/hunk/oracles/review-conformance-main.json"),
        include_str!("../../../port/hunk/oracles/review-conformance-stable.json"),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let mut count = 0;
        for case in oracle["results"].as_array().unwrap() {
            if case["group"] != "geometry" {
                continue;
            }
            let id = case["id"].as_str().unwrap();
            let (file, expansion, source) = fixture(id);
            let actual = project(&file, expansion, &source);
            assert_eq!(actual, case["expected"], "{}: {id}", oracle["upstream"]);
            let captured = case["actual"]
                .as_array()
                .unwrap()
                .iter()
                .find(|consumer| consumer["consumer"] == name)
                .expect("actual pinned consumer output");
            assert_eq!(actual, captured["output"], "captured {name}: {id}");
            count += 1;
        }
        assert_eq!(count, 6);
    }
}

#[test]
fn registers_every_pinned_consumer_in_the_executable_rust_drivers() {
    assert_eq!(
        GEOMETRY_CONSUMERS.map(|(name, _)| name),
        [
            "core review model",
            "terminal render planning",
            "review producer"
        ]
    );
    assert_eq!(
        navigation::CONSUMERS.map(|(name, _)| name),
        ["core intent planner", "terminal review"]
    );
    assert_eq!(
        ordering::CONSUMERS.map(|(name, _)| name),
        ["core publication ordering", "broker review mirror"]
    );
    assert_eq!([snapshot::CONSUMER.0], ["extension review snapshot"]);
    assert_eq!([wire::CONSUMER.0], ["review wire protocol"]);
    assert_eq!(
        events::CONSUMERS.map(|(name, _)| name),
        ["review event protocol", "browser review HTTP surface"]
    );
}

#[test]
fn conformance_corpus_covers_every_claimed_finding_and_source_case() {
    use std::collections::BTreeSet;

    let required = [
        "A1", "A2", "A3", "A4", "A8", "A10", "B1", "B2", "B3", "B4", "B6", "B10", "B12", "C1",
        "C4", "D1", "EXT1",
    ];
    for (encoded, expected_source_cases) in [
        (
            include_str!("../../../port/hunk/oracles/review-conformance-main.json"),
            111,
        ),
        (
            include_str!("../../../port/hunk/oracles/review-conformance-stable.json"),
            106,
        ),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let cases = oracle["results"].as_array().unwrap();
        let mut findings = cases
            .iter()
            .filter_map(|case| case["findings"].as_array())
            .flatten()
            .map(|finding| finding.as_str().unwrap())
            .collect::<BTreeSet<_>>();
        let count = |group: &str| cases.iter().filter(|case| case["group"] == group).count();
        assert_eq!(count("note-size"), 6);
        findings.insert("D1");
        assert!(
            required
                .into_iter()
                .all(|finding| findings.contains(finding))
        );

        // Count the upstream dynamically registered cases, not Rust test functions.
        // Every term is executed by the corresponding module's fixture loop.
        let source_cases = 2
            + count("geometry") * GEOMETRY_CONSUMERS.len()
            + count("navigation") * navigation::CONSUMERS.len()
            + count("ordering") * ordering::CONSUMERS.len()
            + count("snapshot")
            + count("wire")
            + count("producer-ordering")
            + count("events") * events::CONSUMERS.len()
            + count("note-body") * 2
            + count("note-size") * 2;
        assert_eq!(source_cases, expected_source_cases);
        for case in cases {
            let names = match case["group"].as_str().unwrap() {
                "geometry" => GEOMETRY_CONSUMERS.map(|(name, _)| name).to_vec(),
                "navigation" => navigation::CONSUMERS.map(|(name, _)| name).to_vec(),
                "ordering" => ordering::CONSUMERS.map(|(name, _)| name).to_vec(),
                "snapshot" => vec![snapshot::CONSUMER.0],
                "wire" => vec![wire::CONSUMER.0],
                "events" => events::CONSUMERS.map(|(name, _)| name).to_vec(),
                "producer-ordering" => vec!["producer ordering"],
                "note-body" => vec!["core note policy and draft planner"],
                "note-size" => vec!["core note size", "review wire note size"],
                other => panic!("untranslated conformance group {other}"),
            };
            assert_eq!(
                case["actual"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|consumer| consumer["consumer"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                names
            );
        }
    }
}
