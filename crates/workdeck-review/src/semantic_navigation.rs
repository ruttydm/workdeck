//! Relative navigation over the renderer-neutral review stream.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{ReviewRevealAnchor, ReviewRevealRequest, SemanticReviewSelection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewSelectionScope {
    Hunk,
    File,
    AnnotatedHunk,
    AnnotatedFile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewSelectionWrapPolicy {
    Clamp,
    Wrap,
}

#[must_use]
pub const fn review_selection_wrap_policy(
    scope: ReviewSelectionScope,
) -> ReviewSelectionWrapPolicy {
    match scope {
        ReviewSelectionScope::Hunk
        | ReviewSelectionScope::File
        | ReviewSelectionScope::AnnotatedHunk => ReviewSelectionWrapPolicy::Clamp,
        ReviewSelectionScope::AnnotatedFile => ReviewSelectionWrapPolicy::Wrap,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewNavigationFile {
    pub file_key: String,
    pub hunk_count: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SemanticReviewAnnotationIndex {
    pub annotated_hunk_indices_by_file_key: BTreeMap<String, BTreeSet<usize>>,
    pub annotated_file_keys: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewHunkCursor {
    pub file_key: String,
    pub hunk_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewSelectionMove {
    pub scope: ReviewSelectionScope,
    pub delta: isize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSelectionMoveTarget {
    pub file_key: String,
    pub hunk_index: usize,
    pub reveal: ReviewRevealRequest,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewNavigationModel {
    pub files: Vec<ReviewNavigationFile>,
    pub annotations: SemanticReviewAnnotationIndex,
}

pub const REVIEW_FILE_JUMP_REVEAL: ReviewRevealRequest = ReviewRevealRequest {
    anchor: ReviewRevealAnchor::FileTop,
    scroll_to_note: false,
};
pub const REVIEW_FILE_JUMP_HUNK_INDEX: usize = 0;

#[must_use]
pub fn review_stream_cursors(files: &[ReviewNavigationFile]) -> Vec<ReviewHunkCursor> {
    files
        .iter()
        .flat_map(|file| {
            (0..file.hunk_count).map(|hunk_index| ReviewHunkCursor {
                file_key: file.file_key.clone(),
                hunk_index,
            })
        })
        .collect()
}

#[must_use]
pub fn review_annotated_cursors(
    files: &[ReviewNavigationFile],
    annotations: &SemanticReviewAnnotationIndex,
) -> Vec<ReviewHunkCursor> {
    files
        .iter()
        .flat_map(|file| {
            let annotated = annotations
                .annotated_hunk_indices_by_file_key
                .get(&file.file_key);
            (0..file.hunk_count)
                .filter(move |hunk_index| annotated.is_some_and(|set| set.contains(hunk_index)))
                .map(|hunk_index| ReviewHunkCursor {
                    file_key: file.file_key.clone(),
                    hunk_index,
                })
        })
        .collect()
}

fn cursor_matches(cursor: &ReviewHunkCursor, selection: &SemanticReviewSelection) -> bool {
    selection.file_key.as_deref() == Some(cursor.file_key.as_str())
        && selection.hunk_index == cursor.hunk_index
}

fn nearest_cursor_index(
    cursors: &[ReviewHunkCursor],
    stream_cursors: &[ReviewHunkCursor],
    selection: &SemanticReviewSelection,
    delta: isize,
) -> usize {
    let edge_index = if delta >= 0 { 0 } else { cursors.len() - 1 };
    if selection.file_key.is_none() {
        return edge_index;
    }
    let Some(current_stream_index) = stream_cursors
        .iter()
        .position(|cursor| cursor_matches(cursor, selection))
    else {
        return edge_index;
    };
    let indexed_cursors = cursors
        .iter()
        .enumerate()
        .filter_map(|(index, cursor)| {
            stream_cursors
                .iter()
                .position(|stream| stream == cursor)
                .map(|stream_index| (index, stream_index))
        })
        .collect::<Vec<_>>();
    if indexed_cursors.is_empty() {
        return edge_index;
    }
    let remaining_steps = delta.unsigned_abs().saturating_sub(1);
    if delta >= 0 {
        let nearest = indexed_cursors
            .iter()
            .find(|(_, stream_index)| *stream_index > current_stream_index)
            .or_else(|| indexed_cursors.last())
            .expect("indexed cursors are not empty")
            .0;
        nearest
            .saturating_add(remaining_steps)
            .min(cursors.len() - 1)
    } else {
        indexed_cursors
            .iter()
            .rev()
            .find(|(_, stream_index)| *stream_index < current_stream_index)
            .map_or(0, |(index, _)| index.saturating_sub(remaining_steps))
    }
}

fn step_cursors(
    cursors: &[ReviewHunkCursor],
    stream_cursors: &[ReviewHunkCursor],
    selection: &SemanticReviewSelection,
    delta: isize,
) -> Option<ReviewHunkCursor> {
    if cursors.is_empty() {
        return None;
    }
    let next_index = cursors
        .iter()
        .position(|cursor| cursor_matches(cursor, selection))
        .map_or_else(
            || nearest_cursor_index(cursors, stream_cursors, selection, delta),
            |current| {
                let maximum = isize::try_from(cursors.len() - 1).unwrap_or(isize::MAX);
                usize::try_from(
                    isize::try_from(current)
                        .unwrap_or(isize::MAX)
                        .saturating_add(delta)
                        .clamp(0, maximum),
                )
                .unwrap_or(0)
            },
        );
    cursors.get(next_index).cloned()
}

fn plan_hunk_move(
    model: &ReviewNavigationModel,
    selection: &SemanticReviewSelection,
    delta: isize,
) -> Option<ReviewSelectionMoveTarget> {
    let cursors = review_stream_cursors(&model.files);
    let target = step_cursors(&cursors, &cursors, selection, delta)?;
    let crosses_file_forward =
        selection.file_key.as_deref() != Some(target.file_key.as_str()) && delta > 0;
    Some(ReviewSelectionMoveTarget {
        file_key: target.file_key,
        hunk_index: target.hunk_index,
        reveal: ReviewRevealRequest {
            anchor: if crosses_file_forward {
                ReviewRevealAnchor::FileTop
            } else {
                ReviewRevealAnchor::Hunk
            },
            scroll_to_note: false,
        },
    })
}

fn plan_file_move(
    model: &ReviewNavigationModel,
    selection: &SemanticReviewSelection,
    delta: isize,
) -> Option<ReviewSelectionMoveTarget> {
    let current = model
        .files
        .iter()
        .position(|file| selection.file_key.as_deref() == Some(file.file_key.as_str()))?;
    let maximum = isize::try_from(model.files.len() - 1).unwrap_or(isize::MAX);
    let next = usize::try_from(
        isize::try_from(current)
            .unwrap_or(isize::MAX)
            .saturating_add(delta)
            .clamp(0, maximum),
    )
    .unwrap_or(0);
    if next == current {
        return None;
    }
    let next_file = model.files.get(next)?;
    Some(ReviewSelectionMoveTarget {
        file_key: next_file.file_key.clone(),
        hunk_index: REVIEW_FILE_JUMP_HUNK_INDEX,
        reveal: REVIEW_FILE_JUMP_REVEAL,
    })
}

fn plan_annotated_hunk_move(
    model: &ReviewNavigationModel,
    selection: &SemanticReviewSelection,
    delta: isize,
) -> Option<ReviewSelectionMoveTarget> {
    let target = step_cursors(
        &review_annotated_cursors(&model.files, &model.annotations),
        &review_stream_cursors(&model.files),
        selection,
        delta,
    )?;
    Some(ReviewSelectionMoveTarget {
        file_key: target.file_key,
        hunk_index: target.hunk_index,
        reveal: ReviewRevealRequest {
            anchor: ReviewRevealAnchor::Hunk,
            scroll_to_note: true,
        },
    })
}

fn plan_annotated_file_move(
    model: &ReviewNavigationModel,
    selection: &SemanticReviewSelection,
    delta: isize,
) -> Option<ReviewSelectionMoveTarget> {
    let files = model
        .files
        .iter()
        .filter(|file| {
            model
                .annotations
                .annotated_file_keys
                .contains(&file.file_key)
        })
        .collect::<Vec<_>>();
    let length = isize::try_from(files.len()).ok()?;
    if length == 0 {
        return None;
    }
    let current = files
        .iter()
        .position(|file| selection.file_key.as_deref() == Some(file.file_key.as_str()))
        .unwrap_or(0);
    let next = isize::try_from(current)
        .unwrap_or(isize::MAX)
        .saturating_add(delta)
        .rem_euclid(length);
    let next_file = files.get(usize::try_from(next).unwrap_or(0))?;
    Some(ReviewSelectionMoveTarget {
        file_key: next_file.file_key.clone(),
        hunk_index: REVIEW_FILE_JUMP_HUNK_INDEX,
        reveal: ReviewRevealRequest {
            anchor: ReviewRevealAnchor::Hunk,
            scroll_to_note: false,
        },
    })
}

#[must_use]
pub fn plan_review_selection_move(
    model: &ReviewNavigationModel,
    selection: &SemanticReviewSelection,
    movement: ReviewSelectionMove,
) -> Option<ReviewSelectionMoveTarget> {
    match movement.scope {
        ReviewSelectionScope::Hunk => plan_hunk_move(model, selection, movement.delta),
        ReviewSelectionScope::File => plan_file_move(model, selection, movement.delta),
        ReviewSelectionScope::AnnotatedHunk => {
            plan_annotated_hunk_move(model, selection, movement.delta)
        }
        ReviewSelectionScope::AnnotatedFile => {
            plan_annotated_file_move(model, selection, movement.delta)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> Vec<ReviewNavigationFile> {
        ["alpha", "beta", "gamma"]
            .into_iter()
            .map(|file_key| ReviewNavigationFile {
                file_key: file_key.into(),
                hunk_count: 2,
            })
            .collect()
    }

    fn annotations(members: &[(&str, &[usize])], extras: &[&str]) -> SemanticReviewAnnotationIndex {
        SemanticReviewAnnotationIndex {
            annotated_hunk_indices_by_file_key: members
                .iter()
                .map(|(file, hunks)| {
                    (
                        (*file).into(),
                        hunks.iter().copied().collect::<BTreeSet<_>>(),
                    )
                })
                .collect(),
            annotated_file_keys: members
                .iter()
                .map(|(file, _)| (*file).into())
                .chain(extras.iter().map(|file| (*file).into()))
                .collect(),
        }
    }

    fn model(annotations: SemanticReviewAnnotationIndex) -> ReviewNavigationModel {
        ReviewNavigationModel {
            files: files(),
            annotations,
        }
    }

    fn at(file_key: Option<&str>, hunk_index: usize) -> SemanticReviewSelection {
        SemanticReviewSelection {
            file_key: file_key.map(str::to_owned),
            hunk_index,
        }
    }

    fn movement(
        model: &ReviewNavigationModel,
        selection: SemanticReviewSelection,
        scope: ReviewSelectionScope,
        delta: isize,
    ) -> Option<ReviewSelectionMoveTarget> {
        plan_review_selection_move(model, &selection, ReviewSelectionMove { scope, delta })
    }

    fn address(target: &Option<ReviewSelectionMoveTarget>) -> Option<String> {
        target
            .as_ref()
            .map(|target| format!("{}:{}", target.file_key, target.hunk_index))
    }

    #[test]
    fn declares_one_wrap_policy_per_scope() {
        for scope in [
            ReviewSelectionScope::Hunk,
            ReviewSelectionScope::File,
            ReviewSelectionScope::AnnotatedHunk,
        ] {
            assert_eq!(
                review_selection_wrap_policy(scope),
                ReviewSelectionWrapPolicy::Clamp
            );
        }
        assert_eq!(
            review_selection_wrap_policy(ReviewSelectionScope::AnnotatedFile),
            ReviewSelectionWrapPolicy::Wrap
        );
    }

    #[test]
    fn flattens_stream_and_annotated_subset_in_review_order() {
        let all = review_stream_cursors(&files());
        assert_eq!(
            all.iter()
                .map(|cursor| format!("{}:{}", cursor.file_key, cursor.hunk_index))
                .collect::<Vec<_>>(),
            [
                "alpha:0", "alpha:1", "beta:0", "beta:1", "gamma:0", "gamma:1"
            ]
        );
        let subset = review_annotated_cursors(
            &files(),
            &annotations(&[("alpha", &[1]), ("gamma", &[0])], &[]),
        );
        assert_eq!(
            subset
                .iter()
                .map(|cursor| format!("{}:{}", cursor.file_key, cursor.hunk_index))
                .collect::<Vec<_>>(),
            ["alpha:1", "gamma:0"]
        );
    }

    #[test]
    fn hunk_navigation_crosses_files_clamps_and_owns_reveal_policy() {
        let model = model(SemanticReviewAnnotationIndex::default());
        assert_eq!(
            address(&movement(
                &model,
                at(Some("alpha"), 1),
                ReviewSelectionScope::Hunk,
                1
            )),
            Some("beta:0".into())
        );
        let forward =
            movement(&model, at(Some("alpha"), 1), ReviewSelectionScope::Hunk, 1).unwrap();
        assert_eq!(forward.reveal, REVIEW_FILE_JUMP_REVEAL);
        let backward =
            movement(&model, at(Some("beta"), 0), ReviewSelectionScope::Hunk, -1).unwrap();
        assert_eq!(address(&Some(backward.clone())), Some("alpha:1".into()));
        assert_eq!(backward.reveal.anchor, ReviewRevealAnchor::Hunk);
        assert_eq!(
            address(&movement(
                &model,
                at(Some("alpha"), 0),
                ReviewSelectionScope::Hunk,
                3
            )),
            Some("beta:1".into())
        );
        assert_eq!(
            address(&movement(
                &model,
                at(Some("gamma"), 1),
                ReviewSelectionScope::Hunk,
                1
            )),
            Some("gamma:1".into())
        );
        assert_eq!(
            address(&movement(
                &model,
                at(Some("alpha"), 0),
                ReviewSelectionScope::Hunk,
                -1
            )),
            Some("alpha:0".into())
        );
        assert_eq!(
            movement(&model, at(Some("alpha"), 0), ReviewSelectionScope::Hunk, 1)
                .unwrap()
                .reveal
                .anchor,
            ReviewRevealAnchor::Hunk
        );
    }

    #[test]
    fn file_moves_land_on_first_hunk_and_refuse_noops_or_hidden_origins() {
        let model = model(SemanticReviewAnnotationIndex::default());
        let next = movement(&model, at(Some("alpha"), 1), ReviewSelectionScope::File, 1).unwrap();
        assert_eq!(address(&Some(next.clone())), Some("beta:0".into()));
        assert_eq!(next.reveal, REVIEW_FILE_JUMP_REVEAL);
        assert_eq!(
            address(&movement(
                &model,
                at(Some("alpha"), 0),
                ReviewSelectionScope::File,
                2
            )),
            Some("gamma:0".into())
        );
        for (file, delta) in [("gamma", 1), ("alpha", -1), ("hidden", 1)] {
            assert!(
                movement(&model, at(Some(file), 0), ReviewSelectionScope::File, delta).is_none()
            );
        }
    }

    #[test]
    fn annotated_hunks_reach_nearest_then_spend_remaining_steps_and_clamp() {
        let navigation = model(annotations(
            &[("alpha", &[0]), ("beta", &[1]), ("gamma", &[0, 1])],
            &[],
        ));
        for (start_file, start_hunk, delta, expected) in [
            ("alpha", 1, 1, "beta:1"),
            ("alpha", 1, 2, "gamma:0"),
            ("alpha", 1, 3, "gamma:1"),
            ("gamma", 0, -1, "beta:1"),
            ("gamma", 0, -2, "alpha:0"),
            ("alpha", 1, 9, "gamma:1"),
            ("beta", 0, -9, "alpha:0"),
        ] {
            assert_eq!(
                address(&movement(
                    &navigation,
                    at(Some(start_file), start_hunk),
                    ReviewSelectionScope::AnnotatedHunk,
                    delta
                )),
                Some(expected.into())
            );
        }
        assert!(
            movement(
                &model(SemanticReviewAnnotationIndex::default()),
                at(Some("alpha"), 0),
                ReviewSelectionScope::AnnotatedHunk,
                1
            )
            .is_none()
        );
        assert!(
            movement(
                &model(SemanticReviewAnnotationIndex::default()),
                at(Some("alpha"), 0),
                ReviewSelectionScope::AnnotatedFile,
                1
            )
            .is_none()
        );
        let target = movement(
            &navigation,
            at(Some("alpha"), 1),
            ReviewSelectionScope::AnnotatedHunk,
            1,
        )
        .unwrap();
        assert_eq!(target.reveal.anchor, ReviewRevealAnchor::Hunk);
        assert!(target.reveal.scroll_to_note);
    }

    #[test]
    fn annotated_files_form_a_ring_and_include_file_only_context() {
        let navigation = model(annotations(&[("alpha", &[0]), ("gamma", &[0])], &[]));
        for (file, delta, expected) in [
            ("alpha", 1, "gamma:0"),
            ("gamma", 1, "alpha:0"),
            ("alpha", -1, "gamma:0"),
            ("beta", 1, "gamma:0"),
            ("beta", -1, "gamma:0"),
        ] {
            assert_eq!(
                address(&movement(
                    &navigation,
                    at(Some(file), 0),
                    ReviewSelectionScope::AnnotatedFile,
                    delta
                )),
                Some(expected.into())
            );
        }
        let target = movement(
            &navigation,
            at(Some("alpha"), 0),
            ReviewSelectionScope::AnnotatedFile,
            1,
        )
        .unwrap();
        assert_eq!(target.reveal.anchor, ReviewRevealAnchor::Hunk);
        assert!(!target.reveal.scroll_to_note);

        let file_only = model(annotations(&[("alpha", &[0])], &["beta"]));
        assert_eq!(
            address(&movement(
                &file_only,
                at(Some("alpha"), 0),
                ReviewSelectionScope::AnnotatedFile,
                1
            )),
            Some("beta:0".into())
        );
        assert_eq!(
            address(&movement(
                &file_only,
                at(Some("alpha"), 0),
                ReviewSelectionScope::AnnotatedHunk,
                1
            )),
            Some("alpha:0".into())
        );
    }

    #[test]
    fn missing_positions_start_at_directional_edges() {
        let plain = model(SemanticReviewAnnotationIndex::default());
        let annotated = model(annotations(&[("beta", &[0]), ("gamma", &[1])], &[]));
        for (selection, model, scope, delta, expected) in [
            (
                at(None, 0),
                &plain,
                ReviewSelectionScope::Hunk,
                1,
                "alpha:0",
            ),
            (
                at(None, 0),
                &plain,
                ReviewSelectionScope::Hunk,
                -1,
                "gamma:1",
            ),
            (
                at(Some("hidden"), 4),
                &annotated,
                ReviewSelectionScope::AnnotatedHunk,
                1,
                "beta:0",
            ),
            (
                at(Some("hidden"), 4),
                &annotated,
                ReviewSelectionScope::AnnotatedHunk,
                -1,
                "gamma:1",
            ),
        ] {
            assert_eq!(
                address(&movement(model, selection, scope, delta)),
                Some(expected.into())
            );
        }
    }

    #[test]
    fn every_scope_refuses_an_empty_stream() {
        let empty = ReviewNavigationModel::default();
        for scope in [
            ReviewSelectionScope::Hunk,
            ReviewSelectionScope::File,
            ReviewSelectionScope::AnnotatedHunk,
            ReviewSelectionScope::AnnotatedFile,
        ] {
            assert!(movement(&empty, at(None, 0), scope, 1).is_none());
        }
    }
}
