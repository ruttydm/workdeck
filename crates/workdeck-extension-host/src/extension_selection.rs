use workdeck_core::{Changeset, ReviewSelection, ReviewSide, ReviewSnapshot};
use workdeck_extension_api::{
    ExtensionDiffFile, ExtensionFileSide, ExtensionReviewSelection, ExtensionReviewSelectionLine,
};

use crate::file_view_host::project_extension_diff_file;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionLineCursor {
    pub file_id: String,
    pub hunk_index: usize,
    pub target: ExtensionReviewSelectionLine,
}

/// Resolve a selection only against the files visible to the extension.
///
/// The floating-point hunk index is intentional at this boundary: it preserves Hunk's defensive
/// behavior for JavaScript values such as negative fractions and NaN in the frozen oracle. Native
/// product callers supply ordinary integral indexes.
#[must_use]
pub fn build_extension_review_selection(
    files: &[ExtensionDiffFile],
    selected_file_id: Option<&str>,
    selected_hunk_index: Option<f64>,
    line_cursor: Option<&ExtensionLineCursor>,
) -> ExtensionReviewSelection {
    let Some(file) = selected_file_id
        .and_then(|selected| files.iter().find(|candidate| candidate.id == selected))
    else {
        // No selection still carries the corpus: a whole-review command needs
        // the visible list even when the filter hides the selected file.
        return ExtensionReviewSelection {
            files: files.to_vec(),
            ..ExtensionReviewSelection::default()
        };
    };

    let hunk_index = resolve_hunk_index(file, selected_hunk_index);
    let current_line = line_cursor
        .filter(|cursor| cursor.file_id == file.id && Some(cursor.hunk_index) == hunk_index)
        .map(|cursor| cursor.target);
    ExtensionReviewSelection {
        file: Some(file.clone()),
        hunk_index,
        current_line,
        // The visible list itself, in review order, so a whole-review command
        // searches exactly what the user can see.
        files: files.to_vec(),
    }
}

#[must_use]
pub fn build_extension_review_selection_from_snapshot(
    snapshot: &ReviewSnapshot,
) -> ExtensionReviewSelection {
    build_extension_review_selection_from_document(&snapshot.changeset, snapshot.selection)
}

/// Project a selection from an immutable document without cloning its full contents.
#[must_use]
pub fn build_extension_review_selection_from_document(
    changeset: &Changeset,
    selection: ReviewSelection,
) -> ExtensionReviewSelection {
    let selected_file_id = changeset
        .files
        .get(selection.file_index)
        .map(|file| file.runtime_id.as_str());
    // The document is the widest corpus a host can hand a command: every file
    // in review order. A host that applies a file filter replaces this list
    // with its own visible projection before the selection crosses the wire.
    let files = changeset
        .files
        .iter()
        .map(project_extension_diff_file)
        .collect::<Vec<_>>();
    let Some(selected) = selected_file_id else {
        return ExtensionReviewSelection {
            files,
            ..ExtensionReviewSelection::default()
        };
    };
    // Preserve the public resolver's first-match behavior even if a caller
    // supplies duplicate or empty mounted IDs.
    if !files.iter().any(|file| file.id == selected) {
        return ExtensionReviewSelection {
            files,
            ..ExtensionReviewSelection::default()
        };
    }
    let line_cursor = selection
        .hunk_index
        .zip(selection.side)
        .zip(selection.line)
        .map(|((hunk_index, side), line)| ExtensionLineCursor {
            file_id: selected_file_id.unwrap_or_default().to_owned(),
            hunk_index,
            target: ExtensionReviewSelectionLine {
                side: match side {
                    ReviewSide::Old => ExtensionFileSide::Old,
                    ReviewSide::New => ExtensionFileSide::New,
                },
                line,
            },
        });
    build_extension_review_selection(
        &files,
        selected_file_id,
        selection.hunk_index.map(|index| index as f64),
        line_cursor.as_ref(),
    )
}

fn resolve_hunk_index(file: &ExtensionDiffFile, selected_hunk_index: Option<f64>) -> Option<usize> {
    resolve_hunk_index_for_count(file.hunks.len(), selected_hunk_index)
}

fn resolve_hunk_index_for_count(
    hunk_count: usize,
    selected_hunk_index: Option<f64>,
) -> Option<usize> {
    let selected = selected_hunk_index.filter(|index| index.is_finite())?;
    let last = hunk_count.checked_sub(1)?;
    let selected = selected.floor().max(0.0);
    Some(if selected >= last as f64 {
        last
    } else {
        selected as usize
    })
}

/// Resolve selection metadata without projecting the file corpus.
///
/// Interaction-time bridge commits run on every key press and frame commit;
/// the corpus list is materialized only when a command context is actually
/// frozen, so this builder keeps `files` empty and projects only the selected
/// file.
#[must_use]
pub fn build_extension_review_selection_metadata(
    changeset: &Changeset,
    selection: ReviewSelection,
) -> ExtensionReviewSelection {
    let Some(file) = changeset.files.get(selection.file_index) else {
        return ExtensionReviewSelection::default();
    };
    let hunk_index = resolve_hunk_index_for_count(
        file.hunks.len(),
        selection.hunk_index.map(|index| index as f64),
    );
    // Mirror the public resolver: the cursor only carries a target when the
    // raw hunk index survives the clamp unchanged.
    let current_line =
        match (selection.hunk_index, hunk_index) {
            (Some(raw), Some(resolved)) if raw == resolved => selection
                .side
                .zip(selection.line)
                .map(|(side, line)| ExtensionReviewSelectionLine {
                    side: match side {
                        ReviewSide::Old => ExtensionFileSide::Old,
                        ReviewSide::New => ExtensionFileSide::New,
                    },
                    line,
                }),
            _ => None,
        };
    ExtensionReviewSelection {
        file: Some(project_extension_diff_file(file)),
        hunk_index,
        current_line,
        files: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_extension_api::{ExtensionDiffHunk, ExtensionDiffStats};

    fn file(id: &str, path: &str, hunk_count: usize) -> ExtensionDiffFile {
        ExtensionDiffFile {
            id: id.into(),
            path: path.into(),
            previous_path: None,
            patch: String::new(),
            language: Some("typescript".into()),
            stats: ExtensionDiffStats {
                additions: hunk_count,
                deletions: hunk_count,
            },
            metadata: serde_json::json!({ "hunks": [] }),
            change_type: Some(workdeck_extension_api::ExtensionVcsFileChangeType::Change),
            stats_truncated: false,
            hunks: (0..hunk_count)
                .map(|index| ExtensionDiffHunk {
                    index,
                    header: format!("@@ hunk {index} @@"),
                    old_range: Some([index as u32 + 1, index as u32 + 1]),
                    new_range: Some([index as u32 + 1, index as u32 + 1]),
                })
                .collect(),
            agent: None,
            is_untracked: false,
            is_binary: false,
            is_too_large: false,
        }
    }

    fn files() -> Vec<ExtensionDiffFile> {
        vec![file("alpha", "alpha.ts", 2), file("beta", "beta.ts", 1)]
    }

    #[test]
    fn frozen_hunk_extension_selection_oracle_records_both_pinned_baselines() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-selection.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["stablePresence"], "identical");
        assert_eq!(oracle["executedOracle"]["passed"], 8);
        assert_eq!(oracle["executedOracle"]["expectations"], 18);
    }

    #[test]
    fn resolves_the_selected_file_and_hunk_from_visible_views() {
        let files = files();
        let selection = build_extension_review_selection(&files, Some("beta"), Some(0.0), None);
        assert_eq!(selection.file.as_ref().unwrap().path, "beta.ts");
        assert_eq!(selection.hunk_index, Some(0));
        assert_eq!(selection.current_line, None);
        assert_eq!(selection.files.len(), files.len());
        assert_eq!(selection.files[1].id, selection.file.as_ref().unwrap().id);
    }

    #[test]
    fn carries_the_visible_files_in_review_order_even_with_no_selection() {
        // A content search or any whole-review command needs the corpus the
        // user can see, not the one file under the cursor, and it needs it even
        // when the filter hides the selection.
        let files = files();
        let selection = build_extension_review_selection(&files, Some("gamma"), Some(0.0), None);

        assert_eq!(selection.file, None);
        assert_eq!(selection.hunk_index, None);
        assert_eq!(selection.current_line, None);
        assert_eq!(
            selection
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["alpha.ts", "beta.ts"]
        );
    }

    #[test]
    fn absent_or_filtered_out_files_report_no_selection() {
        for file_id in [None, Some("gamma")] {
            let selection = build_extension_review_selection(&files(), file_id, Some(0.0), None);
            assert_eq!(selection.file, None);
            assert_eq!(selection.hunk_index, None);
            assert_eq!(selection.current_line, None);
        }
    }

    #[test]
    fn matching_current_line_is_copied_into_the_owned_snapshot() {
        let mut cursor = ExtensionLineCursor {
            file_id: "alpha".into(),
            hunk_index: 0,
            target: ExtensionReviewSelectionLine {
                side: ExtensionFileSide::Old,
                line: 42,
            },
        };
        let selection =
            build_extension_review_selection(&files(), Some("alpha"), Some(0.0), Some(&cursor));
        cursor.target.line = 7;
        assert_eq!(selection.current_line.unwrap().line, 42);
    }

    #[test]
    fn current_line_must_belong_to_the_resolved_file_and_hunk() {
        let mut cursor = ExtensionLineCursor {
            file_id: "beta".into(),
            hunk_index: 0,
            target: ExtensionReviewSelectionLine {
                side: ExtensionFileSide::Old,
                line: 7,
            },
        };
        assert_eq!(
            build_extension_review_selection(&files(), Some("alpha"), Some(0.0), Some(&cursor))
                .current_line,
            None
        );
        cursor.file_id = "alpha".into();
        cursor.hunk_index = 1;
        assert_eq!(
            build_extension_review_selection(&files(), Some("alpha"), Some(0.0), Some(&cursor))
                .current_line,
            None
        );
    }

    #[test]
    fn stale_fractional_negative_and_nonfinite_hunk_indexes_follow_hunk_clamping() {
        let files = files();
        assert_eq!(
            build_extension_review_selection(&files, Some("alpha"), Some(99.0), None).hunk_index,
            Some(1)
        );
        assert_eq!(
            build_extension_review_selection(&files, Some("alpha"), Some(-4.0), None).hunk_index,
            Some(0)
        );
        assert_eq!(
            build_extension_review_selection(&files, Some("alpha"), Some(1.9), None).hunk_index,
            Some(1)
        );
        assert_eq!(
            build_extension_review_selection(&files, Some("alpha"), Some(f64::NAN), None)
                .hunk_index,
            None
        );
    }

    #[test]
    fn a_file_without_hunks_has_no_hunk_selection() {
        let files = [file("binary", "binary.dat", 0)];
        let selection = build_extension_review_selection(&files, Some("binary"), Some(0.0), None);
        assert_eq!(selection.file.unwrap().id, "binary");
        assert_eq!(selection.hunk_index, None);
    }

    fn document() -> Changeset {
        workdeck_diff::parse_patch(
            "--- a/alpha.ts\n+++ b/alpha.ts\n@@ -1,3 +1,4 @@\n-removed\n context\n+added\n+second\n",
            "selection-fixture",
            "Working tree",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    #[test]
    fn metadata_selection_matches_the_document_builder_without_the_corpus() {
        let document = document();
        let file_id = document.files[0].runtime_id.clone();
        for selection in [
            ReviewSelection {
                file_index: 0,
                hunk_index: Some(0),
                side: Some(ReviewSide::New),
                line: Some(3),
            },
            ReviewSelection {
                file_index: 0,
                hunk_index: Some(9),
                side: Some(ReviewSide::Old),
                line: Some(1),
            },
            ReviewSelection {
                file_index: 0,
                hunk_index: None,
                side: None,
                line: None,
            },
            ReviewSelection {
                file_index: 4,
                hunk_index: Some(0),
                side: Some(ReviewSide::New),
                line: Some(1),
            },
        ] {
            let full = build_extension_review_selection_from_document(&document, selection);
            let metadata = build_extension_review_selection_metadata(&document, selection);
            assert_eq!(metadata.file, full.file, "file for {selection:?}");
            assert_eq!(
                metadata.hunk_index, full.hunk_index,
                "hunk for {selection:?}"
            );
            assert_eq!(
                metadata.current_line, full.current_line,
                "current line for {selection:?}"
            );
            assert!(
                metadata.files.is_empty(),
                "metadata builder must not project the corpus"
            );
        }
        assert_eq!(full_files_guard(&document), vec![file_id]);
    }

    fn full_files_guard(document: &Changeset) -> Vec<String> {
        build_extension_review_selection_from_document(
            document,
            ReviewSelection {
                file_index: 0,
                hunk_index: None,
                side: None,
                line: None,
            },
        )
        .files
        .into_iter()
        .map(|file| file.id)
        .collect()
    }

    #[test]
    fn returned_file_is_an_owned_copy_of_the_visible_view() {
        let mut files = files();
        let selection = build_extension_review_selection(&files, Some("alpha"), Some(0.0), None);
        files[0].path = "mutated.ts".into();
        assert_eq!(selection.file.unwrap().path, "alpha.ts");
    }
}
