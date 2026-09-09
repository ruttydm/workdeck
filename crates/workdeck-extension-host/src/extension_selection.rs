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
        return ExtensionReviewSelection::default();
    };

    let hunk_index = resolve_hunk_index(file, selected_hunk_index);
    let current_line = line_cursor
        .filter(|cursor| cursor.file_id == file.id && Some(cursor.hunk_index) == hunk_index)
        .map(|cursor| cursor.target);
    ExtensionReviewSelection {
        file: Some(file.clone()),
        hunk_index,
        current_line,
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
    let Some(file) = selected_file_id.and_then(|selected| {
        // Preserve the public resolver's first-match behavior even if a caller
        // supplies duplicate or empty mounted IDs. Only that file is serialized.
        changeset
            .files
            .iter()
            .find(|file| file.runtime_id == selected)
    }) else {
        return ExtensionReviewSelection::default();
    };
    let file = project_extension_diff_file(file);
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
        std::slice::from_ref(&file),
        selected_file_id,
        selection.hunk_index.map(|index| index as f64),
        line_cursor.as_ref(),
    )
}

fn resolve_hunk_index(file: &ExtensionDiffFile, selected_hunk_index: Option<f64>) -> Option<usize> {
    let selected = selected_hunk_index.filter(|index| index.is_finite())?;
    let last = file.hunks.len().checked_sub(1)?;
    let selected = selected.floor().max(0.0);
    Some(if selected >= last as f64 {
        last
    } else {
        selected as usize
    })
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
        let selection = build_extension_review_selection(&files(), Some("beta"), Some(0.0), None);
        assert_eq!(selection.file.unwrap().path, "beta.ts");
        assert_eq!(selection.hunk_index, Some(0));
        assert_eq!(selection.current_line, None);
    }

    #[test]
    fn absent_or_filtered_out_files_report_no_selection() {
        for file_id in [None, Some("gamma")] {
            assert_eq!(
                build_extension_review_selection(&files(), file_id, Some(0.0), None),
                ExtensionReviewSelection::default()
            );
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

    #[test]
    fn returned_file_is_an_owned_copy_of_the_visible_view() {
        let mut files = files();
        let selection = build_extension_review_selection(&files, Some("alpha"), Some(0.0), None);
        files[0].path = "mutated.ts".into();
        assert_eq!(selection.file.unwrap().path, "alpha.ts");
    }
}
