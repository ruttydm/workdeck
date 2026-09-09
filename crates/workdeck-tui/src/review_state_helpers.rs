//! Pure review-stream composition for the terminal and live-session commands.

use std::collections::BTreeMap;
use std::fmt;

use workdeck_core::{AgentAnnotation, DiffFile, project_review_file};
use workdeck_review::{
    SemanticReviewSelection, SemanticReviewState, find_diff_file_by_path, find_hunk_index_for_line,
    review_file_matches_filter, review_hunk_ranges, select_normalized_semantic_selection,
};
use workdeck_session::{
    NavigateToHunkToolInput, SelectedHunkSummary, no_diff_file_matches_message,
};

use crate::merge_file_annotations_by_file_id;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewStreamState {
    pub all_files: Vec<DiffFile>,
    pub visible_files: Vec<DiffFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewNavigationTarget<'a> {
    pub file: &'a DiffFile,
    pub hunk_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewNavigationTargetError(String);

impl fmt::Display for ReviewNavigationTargetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ReviewNavigationTargetError {}

/// Merge live annotations, then derive the filtered terminal stream through the shared matcher.
#[must_use]
pub fn build_review_stream_state(
    files: &[DiffFile],
    live_comments_by_file_id: &BTreeMap<String, Vec<AgentAnnotation>>,
    filter_query: &str,
) -> ReviewStreamState {
    let all_files = merge_file_annotations_by_file_id(files, live_comments_by_file_id);
    let visible_files = all_files
        .iter()
        .enumerate()
        .filter(|(index, file)| {
            review_file_matches_filter(
                &project_review_file(file, "terminal-review", *index),
                filter_query,
            )
        })
        .map(|(_, file)| file.clone())
        .collect();
    ReviewStreamState {
        all_files,
        visible_files,
    }
}

/// Return a replacement only when the selected semantic file vanished or its hunk is stale.
#[must_use]
pub fn plan_terminal_selection_reconciliation(
    state: &SemanticReviewState,
) -> Option<SemanticReviewSelection> {
    let normalized = select_normalized_semantic_selection(state);
    normalized.file_key.as_ref()?;
    (normalized != state.selection).then_some(normalized)
}

/// Format a selected hunk for daemon snapshots, retaining stale requested indices without ranges.
#[must_use]
pub fn build_selected_hunk_summary(file: &DiffFile, hunk_index: usize) -> SelectedHunkSummary {
    let mut summary = SelectedHunkSummary {
        index: u64::try_from(hunk_index).unwrap_or(u64::MAX),
        old_range: None,
        new_range: None,
    };
    if let Some(hunk) = file.hunks.get(hunk_index) {
        let (old_range, new_range) = review_hunk_ranges(hunk);
        summary.old_range = Some([u64::from(old_range.start), u64::from(old_range.end)]);
        summary.new_range = Some([u64::from(new_range.start), u64::from(new_range.end)]);
    }
    summary
}

/// Resolve one absolute daemon navigation request without mutating review state.
pub fn resolve_review_navigation_target<'a>(
    all_files: &'a [DiffFile],
    input: &NavigateToHunkToolInput,
) -> Result<ReviewNavigationTarget<'a>, ReviewNavigationTargetError> {
    let Some(file_path) = input.file_path.as_deref() else {
        return Err(ReviewNavigationTargetError(
            "navigate requires --file when not using --next-comment or --prev-comment.".into(),
        ));
    };
    let file = find_diff_file_by_path(all_files, file_path)
        .ok_or_else(|| ReviewNavigationTargetError(no_diff_file_matches_message(file_path)))?;
    let hunk_index = if let Some(hunk_index) = input.hunk_index {
        usize::try_from(hunk_index).unwrap_or(usize::MAX)
    } else {
        let (Some(side), Some(line)) = (input.side, input.line) else {
            return Err(ReviewNavigationTargetError(
                "navigate_to_hunk requires either hunkIndex or both side and line.".into(),
            ));
        };
        u32::try_from(line)
            .ok()
            .and_then(|line| find_hunk_index_for_line(file, side, line))
            .unwrap_or(usize::MAX)
    };
    if hunk_index >= file.hunks.len() {
        return Err(ReviewNavigationTargetError(format!(
            "No diff hunk in {file_path} matches the requested target."
        )));
    }
    Ok(ReviewNavigationTarget { file, hunk_index })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use serde_json::Value;
    use workdeck_core::{
        AgentFileContext, Changeset, ChangesetSource, LineRange, ReviewSide,
        project_review_document,
    };
    use workdeck_diff::parse_patch;
    use workdeck_review::build_review_annotation_index;
    use workdeck_session::SessionSelector;

    use super::*;

    fn file(id: &str, path: &str) -> DiffFile {
        let mut file = parse_patch(
            "diff --git a/a.ts b/a.ts\n--- a/a.ts\n+++ b/a.ts\n@@ -1,2 +1,2 @@\n-const value = 1;\n+const value = 2;\n const stable = true;\n",
            "review-state",
            "Review state",
            ChangesetSource::Patch {
                label: "review-state".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        file.runtime_id = id.into();
        file.path = path.into();
        file
    }

    fn input(
        file_path: Option<&str>,
        hunk_index: Option<u64>,
        side: Option<ReviewSide>,
        line: Option<u64>,
    ) -> NavigateToHunkToolInput {
        NavigateToHunkToolInput {
            target_session: SessionSelector::default(),
            file_path: file_path.map(str::to_owned),
            hunk_index,
            side,
            line,
            comment_direction: None,
        }
    }

    fn oracle() -> Value {
        serde_json::from_str(include_str!("../../../port/hunk/oracles/review-state.json")).unwrap()
    }

    #[test]
    fn selected_hunk_summary_preserves_stale_out_of_range_indices() {
        let file = file("alpha", "src/alpha.ts");
        let summary = build_selected_hunk_summary(&file, 99);
        assert_eq!(
            summary.index,
            oracle()["sharedProjection"]["stale"]["index"]
        );
        assert!(summary.old_range.is_none());
        assert!(summary.new_range.is_none());

        let current = build_selected_hunk_summary(&file, 0);
        assert_eq!(current.old_range, Some([1, 2]));
        assert_eq!(current.new_range, Some([1, 2]));
    }

    #[test]
    fn stream_filters_on_path_previous_path_and_agent_summary() {
        let alpha = file("alpha", "src/alpha.ts");
        let mut beta = file("beta", "src/beta.ts");
        beta.previous_path = Some("src/legacy-name.ts".into());
        let mut gamma = file("gamma", "src/gamma.ts");
        gamma.agent = Some(AgentFileContext {
            path: gamma.path.clone(),
            summary: Some("note".into()),
            annotations: vec![workdeck_core::AgentAnnotation {
                extra: Default::default(),
                id: None,
                old_range: None,
                new_range: Some(LineRange { start: 1, end: 1 }),
                summary: "Explain src/gamma.ts".into(),
                rationale: None,
                markup: None,
                tags: Vec::new(),
                confidence: None,
                source: None,
                title: None,
                author: None,
                created_at: None,
                updated_at: None,
                editable: false,
            }],
        });
        let files = [alpha, beta, gamma];
        let expected = &oracle()["sharedProjection"]["visible"];
        for query in [
            "",
            "ALPHA",
            "legacy-name",
            "gamma.ts note",
            "nothing-matches",
        ] {
            let visible = build_review_stream_state(&files, &BTreeMap::new(), query)
                .visible_files
                .into_iter()
                .map(|file| file.runtime_id)
                .collect::<Vec<_>>();
            assert_eq!(
                visible,
                serde_json::from_value::<Vec<String>>(expected[query].clone()).unwrap()
            );
        }
    }

    #[test]
    fn stream_merges_file_id_keyed_live_annotations_without_mutating_input() {
        let alpha = file("alpha", "src/alpha.ts");
        let annotation = workdeck_core::AgentAnnotation {
            extra: Default::default(),
            id: Some("live:1".into()),
            old_range: None,
            new_range: Some(LineRange { start: 1, end: 1 }),
            summary: "live note".into(),
            rationale: None,
            markup: None,
            tags: Vec::new(),
            confidence: None,
            source: None,
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: false,
        };
        let stream = build_review_stream_state(
            std::slice::from_ref(&alpha),
            &BTreeMap::from([("alpha".into(), vec![annotation.clone()])]),
            "",
        );
        assert!(alpha.agent.is_none());
        assert_eq!(
            stream.all_files[0].agent.as_ref().unwrap().annotations,
            [annotation]
        );
        assert_eq!(stream.visible_files, stream.all_files);
    }

    #[test]
    fn annotation_index_separates_annotated_files_from_annotated_hunks() {
        let mut annotated = file("alpha", "alpha.ts");
        annotated.agent = Some(AgentFileContext {
            path: "alpha.ts".into(),
            summary: Some("context".into()),
            annotations: vec![workdeck_core::AgentAnnotation {
                extra: Default::default(),
                id: None,
                old_range: None,
                new_range: Some(LineRange { start: 1, end: 1 }),
                summary: "note".into(),
                rationale: None,
                markup: None,
                tags: Vec::new(),
                confidence: None,
                source: None,
                title: None,
                author: None,
                created_at: None,
                updated_at: None,
                editable: false,
            }],
        });
        let mut summary_only = file("beta", "beta.ts");
        summary_only.agent = Some(AgentFileContext {
            path: "beta.ts".into(),
            summary: Some("summary".into()),
            annotations: Vec::new(),
        });
        let plain = file("gamma", "gamma.ts");
        let key_by_runtime_id = HashMap::from([
            ("alpha".into(), "key:alpha".into()),
            ("beta".into(), "key:beta".into()),
            ("gamma".into(), "key:gamma".into()),
        ]);
        let index =
            build_review_annotation_index(&[annotated, summary_only, plain], &key_by_runtime_id);
        assert_eq!(
            index.annotated_file_keys.into_iter().collect::<Vec<_>>(),
            ["key:alpha", "key:beta"]
        );
        assert_eq!(
            index.annotated_hunk_indices_by_file_key["key:alpha"],
            [0].into()
        );
    }

    #[test]
    fn absolute_navigation_resolves_hunk_index_side_line_and_previous_path() {
        let mut alpha = file("alpha", "src/alpha.ts");
        alpha.previous_path = Some("src/old-alpha.ts".into());
        let files = [alpha];
        for request in [
            input(Some("src/alpha.ts"), Some(0), None, None),
            input(Some("src/alpha.ts"), None, Some(ReviewSide::New), Some(1)),
            input(Some("src/old-alpha.ts"), Some(0), None, None),
        ] {
            let target = resolve_review_navigation_target(&files, &request).unwrap();
            assert_eq!(target.file.runtime_id, "alpha");
            assert_eq!(target.hunk_index, 0);
        }
    }

    #[test]
    fn invalid_absolute_navigation_returns_the_exact_source_errors() {
        let files = [file("alpha", "src/alpha.ts")];
        let requests = [
            input(None, None, None, None),
            input(Some("missing.ts"), Some(0), None, None),
            input(Some("src/alpha.ts"), None, None, None),
            input(Some("src/alpha.ts"), Some(20), None, None),
        ];
        let actual = requests
            .iter()
            .map(|request| {
                resolve_review_navigation_target(&files, request)
                    .unwrap_err()
                    .to_string()
            })
            .collect::<Vec<_>>();
        let expected =
            serde_json::from_value::<Vec<String>>(oracle()["sharedProjection"]["invalid"].clone())
                .unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn selection_reconciliation_ignores_filters_and_has_no_reveal_side_effect() {
        let parsed = file("alpha", "src/alpha.ts");
        let changeset = Changeset {
            id: "review-state".into(),
            source_label: "review-state".into(),
            title: "Review state".into(),
            summary: None,
            agent_summary: None,
            source: ChangesetSource::Patch {
                label: "review-state".into(),
            },
            files: vec![parsed],
        };
        let document = Arc::new(project_review_document(&changeset, None));
        let mut state = SemanticReviewState::new(document, true);
        state.filter = "nothing-matches".into();
        assert!(plan_terminal_selection_reconciliation(&state).is_none());
        state.selection.hunk_index = 99;
        assert_eq!(
            plan_terminal_selection_reconciliation(&state),
            Some(SemanticReviewSelection {
                file_key: state.selection.file_key.clone(),
                hunk_index: 0,
            })
        );
        assert_eq!(state.reveal, Default::default());
    }
}
