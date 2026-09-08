//! Hunk MIT navigation fixtures, exercised through two real consumer paths.

use std::sync::Arc;

use serde_json::{Value, json};
use workdeck_core::{DiffFile, SemanticReviewDocument, project_review_file};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::{
    ReviewIntentFacts, ReviewSelectionScope, SemanticReviewAction, SemanticReviewAnnotationIndex,
    SemanticReviewIntent, SemanticReviewSelection, SemanticReviewState, SemanticReviewStore,
    apply_semantic_review_intent, plan_semantic_review_intent,
    select_normalized_semantic_selection, select_semantic_reveal_target,
};
use workdeck_tui::plan_terminal_selection_reconciliation;

type NavigationProjection = fn(&Fixture) -> Value;
pub(super) const CONSUMERS: [super::models::Consumer<NavigationProjection>; 2] = [
    super::models::Consumer::new("core intent planner", "Phase 1 PR 3", planner_projection),
    super::models::Consumer::new("terminal review", "Phase 1 PR 3", terminal_projection),
];

fn planner_projection(fixture: &Fixture) -> Value {
    projection(fixture, false)
}
fn terminal_projection(fixture: &Fixture) -> Value {
    projection(fixture, true)
}

#[derive(Clone, Copy)]
enum FilePosition {
    Index(usize),
    Vanished,
    None,
}

#[derive(Clone, Copy)]
struct Position(FilePosition, usize);

struct Move {
    scope: ReviewSelectionScope,
    delta: isize,
    from: Position,
}

pub(super) struct Fixture {
    files: Vec<DiffFile>,
    filter: &'static str,
    annotations: Vec<(usize, Vec<usize>)>,
    annotated_files: Option<Vec<usize>>,
    moves: Vec<Move>,
    selections: Vec<Position>,
}

fn at(file: usize, hunk: usize) -> Position {
    Position(FilePosition::Index(file), hunk)
}

fn movement(scope: ReviewSelectionScope, delta: isize, from: Position) -> Move {
    Move { scope, delta, from }
}

fn two_hunk_file(name: &str) -> DiffFile {
    let lines = (1..=12)
        .map(|line| format!("line {line}\n"))
        .collect::<Vec<_>>();
    let before = lines.concat();
    let mut edited = lines;
    edited[1] = "line two\n".into();
    edited[9] = "line ten\n".into();
    let after = edited.concat();
    let path = format!("{name}.ts");
    let file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "before",
            contents: &before,
            name: &path,
        },
        FileSnapshot {
            cache_key: "after",
            contents: &after,
            name: &path,
        },
        FileComparisonOptions { context_radius: 0 },
    )
    .unwrap();
    assert_eq!(file.hunks.len(), 2);
    file
}

fn fixture(id: &str) -> Fixture {
    use ReviewSelectionScope::{AnnotatedFile, AnnotatedHunk, File, Hunk};
    let mut fixture = Fixture {
        files: ["alpha", "beta", "gamma"].map(two_hunk_file).to_vec(),
        filter: "",
        annotations: Vec::new(),
        annotated_files: None,
        moves: Vec::new(),
        selections: Vec::new(),
    };
    match id {
        "annotated-hunk-multi-step-carry" => {
            fixture.annotations = vec![(0, vec![0]), (1, vec![1]), (2, vec![0, 1])];
            fixture.moves = vec![
                movement(AnnotatedHunk, 1, at(0, 1)),
                movement(AnnotatedHunk, 2, at(0, 1)),
                movement(AnnotatedHunk, 3, at(0, 1)),
                movement(AnnotatedHunk, 9, at(0, 1)),
                movement(AnnotatedHunk, -1, at(2, 0)),
                movement(AnnotatedHunk, -2, at(2, 0)),
                movement(AnnotatedHunk, -9, at(2, 0)),
                movement(AnnotatedHunk, 2, at(0, 0)),
            ];
            fixture.selections = vec![at(1, 1)];
        }
        "scope-wrap-and-clamp" => {
            fixture.annotations = vec![(0, vec![0]), (2, vec![0])];
            fixture.moves = vec![
                movement(Hunk, 1, at(2, 1)),
                movement(Hunk, -1, at(0, 0)),
                movement(Hunk, 1, at(0, 1)),
                movement(Hunk, -1, at(1, 0)),
                movement(File, 1, at(2, 1)),
                movement(File, -1, at(0, 0)),
                movement(File, 1, at(0, 1)),
                movement(AnnotatedHunk, 1, at(2, 0)),
                movement(AnnotatedFile, 1, at(2, 0)),
                movement(AnnotatedFile, -1, at(0, 0)),
                movement(AnnotatedFile, 1, at(1, 0)),
            ];
            fixture.selections = vec![at(2, 1)];
        }
        "selection-outliving-its-file" => {
            fixture.files.truncate(2);
            fixture.filter = "beta";
            fixture.moves = vec![
                movement(Hunk, 1, Position(FilePosition::Vanished, 0)),
                movement(File, 1, at(0, 0)),
            ];
            fixture.selections = vec![
                at(0, 1),
                Position(FilePosition::Vanished, 3),
                Position(FilePosition::None, 0),
                at(1, 9),
            ];
        }
        "selection-with-nothing-visible" => {
            fixture.files.truncate(1);
            fixture.filter = "matches-no-file";
            fixture.moves = vec![movement(Hunk, 1, at(0, 0))];
            fixture.selections = vec![
                Position(FilePosition::Vanished, 0),
                Position(FilePosition::None, 0),
            ];
        }
        "pure-deletion-reveal-target" => {
            fixture.files = vec![
                super::fixture("pure-deletion-hunk").0,
                super::fixture("hunk-with-leading-context").0,
            ];
            fixture.selections = vec![at(0, 0)];
        }
        _ => panic!("untranslated navigation fixture: {id}"),
    }
    fixture
}

fn selection(position: Position, document: &SemanticReviewDocument) -> SemanticReviewSelection {
    SemanticReviewSelection {
        file_key: match position.0 {
            FilePosition::Index(index) => Some(
                document
                    .files
                    .get(index)
                    .map(|file| file.key.clone())
                    .unwrap_or_else(|| "vanished:no-such-file".into()),
            ),
            FilePosition::Vanished => Some("vanished:no-such-file".into()),
            FilePosition::None => None,
        },
        hunk_index: position.1,
    }
}

fn report_selection(
    selection: &SemanticReviewSelection,
    document: &SemanticReviewDocument,
) -> Value {
    json!({"file": document.files.iter().position(|file| Some(&file.key) == selection.file_key.as_ref()),
        "hunkIndex": selection.hunk_index})
}

fn reconcile(state: SemanticReviewState) -> SemanticReviewState {
    let Some(selection) = plan_terminal_selection_reconciliation(&state) else {
        return state;
    };
    // These fixtures have only document, filter, and selection state. Apply the
    // terminal's reconciliation through the public store, not a copied reducer.
    let store = SemanticReviewStore::new(state.document.clone(), true);
    let _ = store.dispatch(SemanticReviewAction::SetFilter(state.filter));
    let reconciled = store.dispatch(SemanticReviewAction::Select {
        file_key: selection.file_key.unwrap(),
        hunk_index: selection.hunk_index as isize,
        reveal: None,
    });
    (*reconciled).clone()
}

fn annotation_index(
    fixture: &Fixture,
    document: &SemanticReviewDocument,
) -> SemanticReviewAnnotationIndex {
    SemanticReviewAnnotationIndex {
        annotated_hunk_indices_by_file_key: fixture
            .annotations
            .iter()
            .filter_map(|(index, hunks)| {
                document
                    .files
                    .get(*index)
                    .map(|file| (file.key.clone(), hunks.iter().copied().collect()))
            })
            .collect(),
        annotated_file_keys: fixture
            .annotated_files
            .clone()
            .unwrap_or_else(|| {
                fixture
                    .annotations
                    .iter()
                    .map(|(index, _)| *index)
                    .collect()
            })
            .into_iter()
            .filter_map(|index| document.files.get(index).map(|file| file.key.clone()))
            .collect(),
    }
}

fn projection(fixture: &Fixture, terminal: bool) -> Value {
    let document = Arc::new(SemanticReviewDocument {
        files: fixture
            .files
            .iter()
            .map(|file| project_review_file(file, "conformance", 0))
            .collect(),
    });
    let annotations = annotation_index(fixture, &document);
    let facts = ReviewIntentFacts {
        annotations: Some(annotations),
        ..Default::default()
    };
    let state_at = |position| {
        let mut state = SemanticReviewState::new(document.clone(), true);
        state.filter = fixture.filter.into();
        state.selection = selection(position, &document);
        if terminal { reconcile(state) } else { state }
    };
    let moves = fixture
        .moves
        .iter()
        .map(|movement| {
            let state = state_at(movement.from);
            let intent = SemanticReviewIntent::Move {
                scope: movement.scope,
                delta: movement.delta,
            };
            if terminal {
                let store = SemanticReviewStore::new(document.clone(), true);
                let _ = store.dispatch(SemanticReviewAction::SetFilter(state.filter.clone()));
                if let Some(file_key) = &state.selection.file_key {
                    let _ = store.dispatch(SemanticReviewAction::Select {
                        file_key: file_key.clone(),
                        hunk_index: state.selection.hunk_index as isize,
                        reveal: None,
                    });
                }
                // No custom stand-in store: verify the real store has the precise fixture position.
                let before = store.snapshot();
                assert_eq!(before.selection, state.selection);
                let outcome = apply_semantic_review_intent(&store, intent, &facts).unwrap();
                if outcome.is_none() {
                    return json!({"to": null});
                }
                let after = store.snapshot();
                let anchor = if after.reveal.file_top_token != before.reveal.file_top_token {
                    "file-top"
                } else if after.reveal.hunk_token != before.reveal.hunk_token {
                    "hunk"
                } else {
                    "none"
                };
                json!({"to": report_selection(&after.selection, &document),
                "reveal": {"anchor": anchor, "scrollToNote": after.reveal.scroll_to_note}})
            } else {
                let plan = plan_semantic_review_intent(&state, intent, &facts).unwrap();
                let Some(SemanticReviewAction::Select {
                    file_key,
                    hunk_index,
                    reveal: Some(reveal),
                }) = plan.actions.first()
                else {
                    return json!({"to": null});
                };
                json!({"to": report_selection(&SemanticReviewSelection {
                file_key: Some(file_key.clone()), hunk_index: *hunk_index as usize,
            }, &document), "reveal": reveal})
            }
        })
        .collect::<Vec<_>>();
    let normalized = fixture
        .selections
        .iter()
        .map(|position| {
            let state = state_at(*position);
            let selection = if terminal {
                state.selection.clone()
            } else {
                select_normalized_semantic_selection(&state)
            };
            report_selection(&selection, &document)
        })
        .collect::<Vec<_>>();
    let targets = document
        .files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            (0..file.hunks.len())
                .map(|hunk| {
                    let mut state = SemanticReviewState::new(document.clone(), true);
                    state.filter = fixture.filter.into();
                    state.selection = selection(at(index, hunk), &document);
                    select_semantic_reveal_target(&state)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    json!({"moves": moves, "normalizedSelections": normalized, "revealTargets": targets})
}

#[test]
fn positional_helpers_handle_vanished_files_and_independent_annotation_scopes() {
    let mut fixture = fixture("scope-wrap-and-clamp");
    let document = SemanticReviewDocument {
        files: fixture
            .files
            .iter()
            .map(|file| project_review_file(file, "conformance", 0))
            .collect(),
    };
    assert_eq!(
        selection(at(999, 7), &document),
        selection(Position(FilePosition::Vanished, 7), &document)
    );
    assert_eq!(
        report_selection(&selection(at(999, 7), &document), &document),
        json!({"file": null, "hunkIndex": 7})
    );
    assert_eq!(
        report_selection(
            &selection(Position(FilePosition::None, 2), &document),
            &document
        ),
        json!({"file": null, "hunkIndex": 2})
    );
    assert_eq!(
        report_selection(&selection(at(1, 3), &document), &document),
        json!({"file": 1, "hunkIndex": 3})
    );

    fixture.annotations = vec![(0, vec![0, 0, 1]), (999, vec![0])];
    let inferred = annotation_index(&fixture, &document);
    assert_eq!(inferred.annotated_file_keys.len(), 1);
    assert!(
        inferred
            .annotated_file_keys
            .contains(&document.files[0].key)
    );
    assert_eq!(inferred.annotated_hunk_indices_by_file_key.len(), 1);
    assert_eq!(
        inferred.annotated_hunk_indices_by_file_key[&document.files[0].key].len(),
        2
    );
    fixture.annotated_files = Some(vec![1, 1, 999]);
    let explicit = annotation_index(&fixture, &document);
    assert_eq!(explicit.annotated_file_keys.len(), 1);
    assert!(
        explicit
            .annotated_file_keys
            .contains(&document.files[1].key)
    );
    assert_eq!(
        explicit.annotated_hunk_indices_by_file_key,
        inferred.annotated_hunk_indices_by_file_key
    );
    fixture.moves = vec![movement(ReviewSelectionScope::AnnotatedFile, 1, at(0, 0))];
    for consumer in CONSUMERS {
        assert_eq!(
            (consumer.project)(&fixture)["moves"][0]["to"],
            json!({"file": 1, "hunkIndex": 0}),
            "explicit file scope must reach both real navigation consumers"
        );
    }
    fixture.annotated_files = Some(Vec::new());
    assert!(
        annotation_index(&fixture, &document)
            .annotated_file_keys
            .is_empty()
    );
}

#[test]
fn both_navigation_consumers_match_both_pinned_corpora() {
    for encoded in [
        include_str!("../../../../port/hunk/oracles/review-conformance-main.json"),
        include_str!("../../../../port/hunk/oracles/review-conformance-stable.json"),
    ] {
        let oracle: Value = serde_json::from_str(encoded).unwrap();
        let mut count = 0;
        for case in oracle["results"].as_array().unwrap() {
            if case["group"] != "navigation" {
                continue;
            }
            let id = case["id"].as_str().unwrap();
            let fixture = fixture(id);
            for consumer in CONSUMERS {
                let name = consumer.name;
                let actual = (consumer.project)(&fixture);
                assert_eq!(
                    super::models::navigation(&actual),
                    super::models::navigation(&case["expected"])
                );
                assert_eq!(
                    actual, case["expected"],
                    "{id}: {name}: {}",
                    oracle["upstream"]
                );
                let captured = case["actual"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|consumer| consumer["consumer"] == name)
                    .unwrap();
                assert_eq!(actual, captured["output"], "captured {name}: {id}");
            }
            count += 1;
        }
        assert_eq!(count, 5);
    }
}
