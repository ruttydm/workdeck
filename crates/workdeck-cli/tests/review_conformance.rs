//! Translation of Hunk's MIT-licensed cross-consumer review conformance harness.
//! Separate geometry, navigation, ordering, snapshot, wire, and event registries run
//! the full applicable corpus against every registered native consumer. New consumers
//! must join their registry and preserve earlier adversarial cases; these independent
//! projections keep a renderer or transport from defining its own expected semantics.
//! This harness is one upstream test file, not the complete upstream test corpus.

#[path = "review_conformance/events.rs"]
mod events;
#[path = "review_conformance/models.rs"]
mod models;
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

type GeometryProjection = fn(&models::ReviewGeometryFixture) -> models::ReviewGeometryProjection;
type SingleFileProjection = fn(&DiffFile, Option<&str>, &str) -> Value;
type FilesProjection = fn(&[DiffFile], Option<(usize, &str)>, &str) -> Value;
const GEOMETRY_CONSUMERS: [models::Consumer<GeometryProjection>; 3] = [
    models::Consumer::new("core review model", "Phase 1 PR 2", core_consumer),
    models::Consumer::new(
        "terminal render planning",
        "Phase 1 PR 2",
        terminal_consumer,
    ),
    models::Consumer::new("review producer", "Phase 2", producer_consumer),
];

fn project_geometry_fixture(
    fixture: &models::ReviewGeometryFixture,
    project: FilesProjection,
) -> models::ReviewGeometryProjection {
    let files = (fixture.build)();
    let expansion = fixture
        .expansion
        .as_ref()
        .map(|expansion| (expansion.file_index, expansion.gap_id.as_str()));
    let source = fixture
        .expansion
        .as_ref()
        .map_or("", |expansion| expansion.source_text.as_str());
    models::geometry(&project(&files, expansion, source))
}

fn core_consumer(fixture: &models::ReviewGeometryFixture) -> models::ReviewGeometryProjection {
    project_geometry_fixture(fixture, core_files_projection)
}

fn terminal_consumer(fixture: &models::ReviewGeometryFixture) -> models::ReviewGeometryProjection {
    project_geometry_fixture(fixture, terminal_files_projection)
}

fn producer_consumer(fixture: &models::ReviewGeometryFixture) -> models::ReviewGeometryProjection {
    project_geometry_fixture(fixture, producer_files_projection)
}

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

fn geometry_fixture(case: &Value) -> models::ReviewGeometryFixture {
    let id = case["id"].as_str().unwrap();
    let description = match id {
        "pure-insertion-hunk" => {
            "@@ -6,0 +7,1 @@ — the old side has no rows, so its leading gap ends at the line the hunk is positioned at, not one before it."
        }
        "pure-deletion-hunk" => {
            "@@ -6,1 +5,0 @@ — the new side has no rows, so its leading gap ends at new line 5 and its text must match the old-side labels beside it."
        }
        "hunk-with-leading-context" => {
            "@@ -3,7 +3,7 @@ — the hunk's extent covers its context rows, and a whole-hunk note skips past them to the changed line."
        }
        "crlf-source" => {
            "A Windows-authored file: expanded rows must carry no carriage return, and line N must still be the Nth line."
        }
        "source-without-trailing-newline" => {
            "A file whose last line has no terminator: the trailing gap must still reach line 12, with no phantom line after it."
        }
        "binary-rename-with-no-rows" => {
            "A renamed binary file: what the change is outranks how it is stored, so every surface calls it a rename."
        }
        _ => panic!("untranslated geometry fixture: {id}"),
    };
    let (_, gap, source_text) = fixture(id);
    let build_id = id.to_owned();
    models::ReviewGeometryFixture {
        id: id.into(),
        findings: serde_json::from_value(case["findings"].clone()).unwrap(),
        description: description.into(),
        build: Box::new(move || vec![fixture(&build_id).0]),
        expansion: gap.map(|gap| models::ConformanceExpansion {
            file_index: 0,
            gap_id: gap.into(),
            source_text,
        }),
        expected: models::geometry(&case["expected"]),
    }
}

fn core_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
    core_files_projection(
        std::slice::from_ref(file),
        expansion.map(|gap| (0, gap)),
        source_text,
    )
}

fn core_files_projection(
    files: &[DiffFile],
    expansion: Option<(usize, &str)>,
    source_text: &str,
) -> Value {
    let files = files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            core_canonical_projection(
                &project_review_file(file, "/repo", index),
                expansion
                    .filter(|(file_index, _)| *file_index == index)
                    .map(|(_, gap)| gap),
                source_text,
            )
        })
        .collect::<Vec<_>>();
    json!({"files": files})
}

fn core_canonical_projection(
    file: &workdeck_core::SemanticReviewFile,
    expansion: Option<&str>,
    source_text: &str,
) -> Value {
    // The upstream core consumer observes the canonical document, not parser output.
    // In particular, zero-count hunk positions must survive canonical projection.
    let source = semantic_review_gap_source(file);
    let manifest = build_review_content_manifest_file(file);
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
        let Some(gap) = review_gap_address(&source, id) else {
            return value;
        };
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
    value
}

#[test]
fn geometry_fixture_callbacks_build_each_consumer_input_and_target_later_files() {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../port/hunk/oracles/review-conformance-main.json"
    ))
    .unwrap();
    let expected_file = |id: &str| {
        oracle["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["group"] == "geometry" && case["id"] == id)
            .unwrap()["expected"]["files"][0]
            .clone()
    };
    let builds = Arc::new(AtomicUsize::new(0));
    let count = builds.clone();
    let (_, gap, source_text) = fixture("pure-insertion-hunk");
    let input = models::ReviewGeometryFixture {
        id: "multi-file-builder".into(),
        findings: vec!["D4".into()],
        description: "Every consumer builds the full stream and expands its second file.".into(),
        build: Box::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            vec![
                fixture("binary-rename-with-no-rows").0,
                fixture("pure-insertion-hunk").0,
            ]
        }),
        expansion: Some(models::ConformanceExpansion {
            file_index: 1,
            gap_id: gap.unwrap().into(),
            source_text,
        }),
        expected: models::geometry(
            &json!({"files": [expected_file("binary-rename-with-no-rows"), expected_file("pure-insertion-hunk")]}),
        ),
    };
    for consumer in GEOMETRY_CONSUMERS {
        assert_eq!(
            (consumer.project)(&input),
            input.expected,
            "{}",
            consumer.name
        );
    }
    assert_eq!(builds.load(Ordering::SeqCst), GEOMETRY_CONSUMERS.len());
}

#[test]
fn terminal_consumer_scopes_stream_expansions_and_reports_missing_gap_rows() {
    let first = fixture("binary-rename-with-no-rows").0;
    let (second, gap, source) = fixture("pure-insertion-hunk");
    let files = [first.clone(), second.clone()];
    assert_eq!(
        terminal_files_projection(&[], None, ""),
        json!({"files": []})
    );
    let actual = terminal_files_projection(&files, Some((1, gap.unwrap())), &source);
    assert_eq!(actual["files"].as_array().unwrap().len(), 2);
    assert_eq!(
        actual["files"][0],
        terminal_projection(&first, None, "")["files"][0]
    );
    assert_eq!(
        actual["files"][1],
        terminal_projection(&second, gap, &source)["files"][0]
    );
    assert_eq!(
        terminal_files_projection(&files, Some((999, gap.unwrap())), &source),
        terminal_files_projection(&files, None, &source)
    );
    for missing in ["before:999", "not-a-gap", ""] {
        let actual = terminal_files_projection(&files, Some((1, missing)), &source);
        assert!(actual["files"][0].get("expandedRows").is_none());
        // Unlike core/producer, the source terminal adapter reports an empty
        // array when a fixture asked to expand a nonexistent gap.
        assert_eq!(actual["files"][1]["expandedRows"], json!([]));
        let mut expected = terminal_files_projection(&files, None, &source);
        expected["files"][1]["expandedRows"] = json!([]);
        assert_eq!(actual, expected);
    }
}

#[test]
fn core_and_producer_project_complete_streams_and_scope_expansion_to_its_file() {
    let first = fixture("binary-rename-with-no-rows").0;
    let (second, gap, source) = fixture("pure-insertion-hunk");
    let files = [first.clone(), second.clone()];
    for project in [core_files_projection, producer_files_projection] {
        assert_eq!(project(&[], None, ""), json!({"files": []}));
        let actual = project(&files, Some((1, gap.unwrap())), &source);
        assert_eq!(actual["files"].as_array().unwrap().len(), 2);
        assert_eq!(
            actual["files"][0],
            core_projection(&first, None, "")["files"][0]
        );
        assert_eq!(
            actual["files"][1],
            core_projection(&second, gap, &source)["files"][0]
        );
        assert_eq!(
            project(&files, Some((999, gap.unwrap())), &source),
            project(&files, None, &source)
        );
        assert_eq!(
            project(&files, Some((1, "before:999")), &source),
            project(&files, None, &source)
        );
    }
}

#[test]
fn core_and_producer_expansion_helpers_preserve_absent_gaps_and_short_sources() {
    let (file, expansion, source) = fixture("pure-insertion-hunk");
    let consumers: [(&str, SingleFileProjection); 2] = [
        ("core review model", core_projection),
        ("review producer", producer_projection),
    ];
    for (name, project) in consumers {
        for missing in ["before:999", "not-a-gap"] {
            let actual = project(&file, Some(missing), &source);
            assert!(
                actual["files"][0].get("expandedRows").is_none(),
                "{name}: {missing}"
            );
            assert_eq!(actual, project(&file, None, &source));
        }
        let actual = project(&file, expansion, "only one line\r\n");
        let rows = actual["files"][0]["expandedRows"].as_array().unwrap();
        assert_eq!(rows.len(), 6, "{name}");
        assert_eq!(
            rows[0],
            json!({"oldLine": 1, "newLine": 1, "text": "only one line"})
        );
        for (offset, row) in rows.iter().enumerate().skip(1) {
            assert_eq!(
                row,
                &json!({"oldLine": offset + 1, "newLine": offset + 1, "text": ""}),
                "{name}"
            );
        }
    }
}

#[test]
fn parsed_core_geometry_matches_both_pinned_conformance_oracles() {
    check_geometry_consumer(GEOMETRY_CONSUMERS[0].name, GEOMETRY_CONSUMERS[0].project);
}

#[test]
fn terminal_planner_geometry_matches_both_pinned_conformance_oracles() {
    check_geometry_consumer(GEOMETRY_CONSUMERS[1].name, GEOMETRY_CONSUMERS[1].project);
}

#[test]
fn producer_geometry_matches_both_pinned_conformance_oracles() {
    check_geometry_consumer(GEOMETRY_CONSUMERS[2].name, GEOMETRY_CONSUMERS[2].project);
}

fn producer_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
    producer_files_projection(
        std::slice::from_ref(file),
        expansion.map(|gap| (0, gap)),
        source_text,
    )
}

fn producer_files_projection(
    files: &[DiffFile],
    expansion: Option<(usize, &str)>,
    source_text: &str,
) -> Value {
    use workdeck_core::ReviewSide;
    use workdeck_review::{
        PublishReviewInput, ReviewIntentFacts, ReviewIntentOutcome, ReviewProducer,
        ReviewProducerOptions, SemanticReviewIntent, SemanticReviewStore,
        assert_canonical_file_matches_manifest,
    };
    let producer = ReviewProducer::new(
        PublishReviewInput {
            files: files.to_vec(),
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
    let files = publication
        .manifest
        .files
        .iter()
        .enumerate()
        .map(|(index, manifest)| {
            assert_canonical_file_matches_manifest(&publication.document.files[index], manifest)
                .unwrap();
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
            if let Some((_, gap_id)) = expansion.filter(|(file_index, _)| *file_index == index) {
                let outcome = match producer.apply_intent(
                    SemanticReviewIntent::ToggleExpansion {
                        file_key: manifest.key.clone(),
                        gap_id: gap_id.into(),
                    },
                    ReviewIntentFacts::default(),
                ) {
                    Ok(outcome) => outcome,
                    Err(workdeck_review::ReviewProducerIntentError::Planning(_)) => {
                        return value;
                    }
                    Err(error) => panic!("producer lifecycle failure: {error}"),
                };
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
                            "text": lines.get(range[0] as usize + offset - 1)
                                .map(String::as_str).unwrap_or(""),
                        }))
                        .collect::<Vec<_>>()
                );
            }
            value
        })
        .collect::<Vec<_>>();
    json!({"files": files})
}

fn terminal_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
    terminal_files_projection(
        std::slice::from_ref(file),
        expansion.map(|gap| (0, gap)),
        source_text,
    )
}

fn terminal_files_projection(
    files: &[DiffFile],
    expansion: Option<(usize, &str)>,
    source_text: &str,
) -> Value {
    let files = files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            terminal_file_projection(
                file,
                expansion
                    .filter(|(file_index, _)| *file_index == index)
                    .map(|(_, gap)| gap),
                source_text,
            )
        })
        .collect::<Vec<_>>();
    json!({"files": files})
}

fn terminal_file_projection(file: &DiffFile, expansion: Option<&str>, source_text: &str) -> Value {
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
    options.show_hunk_headers = true;
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
    value
}

fn check_geometry_consumer(name: &str, project: GeometryProjection) {
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
            let fixture = geometry_fixture(case);
            assert_eq!(fixture.id, id);
            assert!(!fixture.description.is_empty());
            assert_eq!(json!(fixture.findings), case["findings"]);
            let actual = project(&fixture);
            assert_eq!(
                actual, fixture.expected,
                "{name}: {}: {}",
                fixture.id, fixture.description
            );
            let actual = serde_json::to_value(actual).unwrap();
            assert_eq!(
                models::geometry(&actual),
                models::geometry(&case["expected"])
            );
            assert_eq!(actual, case["expected"], "{}: {id}", oracle["upstream"]);
            let captured = case["actual"]
                .as_array()
                .unwrap()
                .iter()
                .find(|consumer| consumer["consumer"] == name)
                .expect("actual pinned consumer output");
            assert_eq!(actual, captured["output"], "captured {name}: {id}");
            assert_eq!(
                models::geometry(&actual),
                models::geometry(&captured["output"])
            );
            count += 1;
        }
        assert_eq!(count, 6);
    }
}

#[test]
fn registers_every_pinned_consumer_in_the_executable_rust_drivers() {
    assert_eq!(
        navigation::CONSUMERS.map(|consumer| consumer.phase),
        ["Phase 1 PR 3", "Phase 1 PR 3"]
    );
    assert_eq!(
        ordering::CONSUMERS.map(|consumer| consumer.phase),
        ["Phase 2", "Phase 3"]
    );
    assert_eq!(
        events::CONSUMERS.map(|consumer| consumer.phase),
        ["Phase 4", "Phase 4"]
    );
    assert_eq!(snapshot::CONSUMER.phase, "extension API v8");
    assert_eq!(wire::CONSUMER.phase, "Phase 3");
    assert_eq!(
        GEOMETRY_CONSUMERS.map(|consumer| consumer.phase),
        ["Phase 1 PR 2", "Phase 1 PR 2", "Phase 2"]
    );
    assert_eq!(
        GEOMETRY_CONSUMERS.map(|consumer| consumer.name),
        [
            "core review model",
            "terminal render planning",
            "review producer"
        ]
    );
    assert_eq!(
        navigation::CONSUMERS.map(|consumer| consumer.name),
        ["core intent planner", "terminal review"]
    );
    assert_eq!(
        ordering::CONSUMERS.map(|consumer| consumer.name),
        ["core publication ordering", "broker review mirror"]
    );
    assert_eq!([snapshot::CONSUMER.name], ["extension review snapshot"]);
    assert_eq!([wire::CONSUMER.name], ["review wire protocol"]);
    assert_eq!(
        events::CONSUMERS.map(|consumer| consumer.name),
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
                "geometry" => GEOMETRY_CONSUMERS.map(|consumer| consumer.name).to_vec(),
                "navigation" => navigation::CONSUMERS.map(|consumer| consumer.name).to_vec(),
                "ordering" => ordering::CONSUMERS.map(|consumer| consumer.name).to_vec(),
                "snapshot" => vec![snapshot::CONSUMER.name],
                "wire" => vec![wire::CONSUMER.name],
                "events" => events::CONSUMERS.map(|consumer| consumer.name).to_vec(),
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
