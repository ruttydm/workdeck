//! Authoritative-source highlight planning translated from Hunk's
//! `src/ui/diff/sourceBackedHighlight.ts`.

use workdeck_core::{
    ReviewFileChangeKind, SemanticReviewFile, SemanticReviewHunkBlock, rebase_semantic_review_hunk,
};

/// Metadata plus index maps for highlighting a partial diff against authoritative source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBackedHighlightPlan {
    pub metadata: SemanticReviewFile,
    pub deletion_line_map: Vec<usize>,
    pub addition_line_map: Vec<usize>,
}

/// Highlighted lines kept independently for the old and new sides of a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HighlightLineArrays<T> {
    pub deletion_lines: Vec<Option<T>>,
    pub addition_lines: Vec<Option<T>>,
}

/// Split normalized source into highlighter lines while retaining final newlines.
fn split_source_lines(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect()
}

/// Convert a unified-diff side start into a zero-based source insertion/line index.
fn source_start_index(start: u32, count: u32) -> usize {
    usize::try_from(if count == 0 {
        start
    } else {
        start.saturating_sub(1)
    })
    .unwrap_or(usize::MAX)
}

/// Assign one validated partial-line index to its corresponding full-source index.
fn assign_source_line(
    map: &mut [Option<usize>],
    partial_lines: &[String],
    full_lines: &[String],
    partial_index: usize,
    full_index: usize,
) -> bool {
    if partial_lines.get(partial_index) != full_lines.get(full_index) {
        return false;
    }
    let Some(entry) = map.get_mut(partial_index) else {
        return false;
    };
    if entry.is_some_and(|existing| existing != full_index) {
        return false;
    }
    *entry = Some(full_index);
    true
}

/// Build highlight-only full-source metadata while preserving the partial diff's hunk structure.
///
/// A plan is returned only when every visible patch line can be proven to match its source side.
#[must_use]
pub fn create_source_backed_highlight_plan(
    metadata: &SemanticReviewFile,
    old_text: Option<&str>,
    new_text: Option<&str>,
) -> Option<SourceBackedHighlightPlan> {
    if !metadata.flags.partial || metadata.hunks.is_empty() {
        return None;
    }
    if (old_text.is_none() && metadata.change_kind != ReviewFileChangeKind::New)
        || (new_text.is_none() && metadata.change_kind != ReviewFileChangeKind::Deleted)
    {
        return None;
    }

    let full_deletion_lines = split_source_lines(old_text.unwrap_or_default());
    let full_addition_lines = split_source_lines(new_text.unwrap_or_default());
    let mut deletion_line_map = vec![None; metadata.deletion_lines.len()];
    let mut addition_line_map = vec![None; metadata.addition_lines.len()];
    let mut previous_deletion_end = 0;
    let mut previous_addition_end = 0;
    let mut final_deletion_end = 0;
    let mut final_addition_end = 0;
    let mut valid = true;
    let mut rebased_hunks = Vec::with_capacity(metadata.hunks.len());

    for hunk in &metadata.hunks {
        let deletion_start_index = source_start_index(hunk.deletion_start, hunk.deletion_count);
        let addition_start_index = source_start_index(hunk.addition_start, hunk.addition_count);
        if old_text.is_some()
            && deletion_start_index.checked_sub(previous_deletion_end)
                != Some(hunk.collapsed_before)
        {
            valid = false;
        }
        if new_text.is_some()
            && addition_start_index.checked_sub(previous_addition_end)
                != Some(hunk.collapsed_before)
        {
            valid = false;
        }

        let rebased = rebase_semantic_review_hunk(hunk, deletion_start_index, addition_start_index);
        for (content, target) in hunk.hunk_content.iter().zip(&rebased.hunk.hunk_content) {
            let (deletions, additions, deletion_index, addition_index) = match content {
                SemanticReviewHunkBlock::Context {
                    lines,
                    deletion_line_index,
                    addition_line_index,
                } => (*lines, *lines, *deletion_line_index, *addition_line_index),
                SemanticReviewHunkBlock::Change {
                    deletions,
                    additions,
                    deletion_line_index,
                    addition_line_index,
                } => (
                    *deletions,
                    *additions,
                    *deletion_line_index,
                    *addition_line_index,
                ),
            };
            let (target_deletion_index, target_addition_index) = match target {
                SemanticReviewHunkBlock::Context {
                    deletion_line_index,
                    addition_line_index,
                    ..
                }
                | SemanticReviewHunkBlock::Change {
                    deletion_line_index,
                    addition_line_index,
                    ..
                } => (*deletion_line_index, *addition_line_index),
            };

            for offset in 0..deletions {
                valid = assign_source_line(
                    &mut deletion_line_map,
                    &metadata.deletion_lines,
                    &full_deletion_lines,
                    deletion_index.saturating_add(offset),
                    target_deletion_index.saturating_add(offset),
                ) && valid;
            }
            for offset in 0..additions {
                valid = assign_source_line(
                    &mut addition_line_map,
                    &metadata.addition_lines,
                    &full_addition_lines,
                    addition_index.saturating_add(offset),
                    target_addition_index.saturating_add(offset),
                ) && valid;
            }
        }

        if rebased
            .deletion_end_index
            .saturating_sub(deletion_start_index)
            != usize::try_from(hunk.deletion_count).unwrap_or(usize::MAX)
            || rebased
                .addition_end_index
                .saturating_sub(addition_start_index)
                != usize::try_from(hunk.addition_count).unwrap_or(usize::MAX)
            || rebased.deletion_end_index > full_deletion_lines.len()
            || rebased.addition_end_index > full_addition_lines.len()
        {
            valid = false;
        }

        previous_deletion_end = rebased.deletion_end_index;
        previous_addition_end = rebased.addition_end_index;
        final_deletion_end = rebased.deletion_end_index;
        final_addition_end = rebased.addition_end_index;
        rebased_hunks.push(rebased.hunk);
    }

    if !valid {
        return None;
    }
    let deletion_line_map = deletion_line_map.into_iter().collect::<Option<Vec<_>>>()?;
    let addition_line_map = addition_line_map.into_iter().collect::<Option<Vec<_>>>()?;
    let mut rebased_metadata = metadata.clone();
    rebased_metadata.flags.partial = false;
    rebased_metadata.deletion_lines = full_deletion_lines[..final_deletion_end].to_vec();
    rebased_metadata.addition_lines = full_addition_lines[..final_addition_end].to_vec();
    rebased_metadata.hunks = rebased_hunks;

    Some(SourceBackedHighlightPlan {
        metadata: rebased_metadata,
        deletion_line_map,
        addition_line_map,
    })
}

/// Remap full-source highlighted lines onto the original partial metadata indexes.
#[must_use]
pub fn remap_source_backed_highlight<T: Clone>(
    plan: &SourceBackedHighlightPlan,
    highlighted: &HighlightLineArrays<T>,
) -> HighlightLineArrays<T> {
    HighlightLineArrays {
        deletion_lines: plan
            .deletion_line_map
            .iter()
            .map(|&source_index| {
                highlighted
                    .deletion_lines
                    .get(source_index)
                    .cloned()
                    .flatten()
            })
            .collect(),
        addition_lines: plan
            .addition_line_map
            .iter()
            .map(|&source_index| {
                highlighted
                    .addition_lines
                    .get(source_index)
                    .cloned()
                    .flatten()
            })
            .collect(),
    }
}

/// Share addition-side context with the old side when no authoritative source is available.
pub fn alias_context_highlight_lines<T: Clone>(
    metadata: &SemanticReviewFile,
    highlighted: &mut HighlightLineArrays<T>,
) {
    for hunk in &metadata.hunks {
        let mut deletion_line_index = hunk.deletion_line_index;
        let mut addition_line_index = hunk.addition_line_index;
        for content in &hunk.hunk_content {
            match content {
                SemanticReviewHunkBlock::Context { lines, .. } => {
                    for offset in 0..*lines {
                        let deletion_index = deletion_line_index.saturating_add(offset);
                        let addition_index = addition_line_index.saturating_add(offset);
                        let shared = highlighted
                            .addition_lines
                            .get(addition_index)
                            .and_then(Clone::clone)
                            .or_else(|| {
                                highlighted
                                    .deletion_lines
                                    .get(deletion_index)
                                    .and_then(Clone::clone)
                            });
                        if let Some(shared) = shared {
                            if let Some(line) = highlighted.deletion_lines.get_mut(deletion_index) {
                                *line = Some(shared.clone());
                            }
                            if let Some(line) = highlighted.addition_lines.get_mut(addition_index) {
                                *line = Some(shared);
                            }
                        }
                    }
                    deletion_line_index = deletion_line_index.saturating_add(*lines);
                    addition_line_index = addition_line_index.saturating_add(*lines);
                }
                SemanticReviewHunkBlock::Change {
                    deletions,
                    additions,
                    ..
                } => {
                    deletion_line_index = deletion_line_index.saturating_add(*deletions);
                    addition_line_index = addition_line_index.saturating_add(*additions);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_patch;
    use workdeck_core::{ChangesetSource, project_review_file};

    const PARTIAL_PATCH: &str = "diff --git a/repro.ex b/repro.ex\n--- a/repro.ex\n+++ b/repro.ex\n@@ -4,4 +4,4 @@\n   Line two.\n-  Line three.\n+  Line three, edited.\n   \"\"\"\n   def hello do\n";
    const OLD_SOURCE: &str = "defmodule Repro do\n  @doc \"\"\"\n  Line one.\n  Line two.\n  Line three.\n  \"\"\"\n  def hello do\n    :world\n  end\nend\n";

    fn metadata(patch: &str) -> SemanticReviewFile {
        let file = parse_patch(
            patch,
            "source-backed-test",
            "source-backed-test",
            ChangesetSource::Patch {
                label: "source-backed-test".into(),
            },
        )
        .unwrap()
        .files
        .remove(0);
        project_review_file(&file, "source-backed-test", 0)
    }

    #[test]
    fn grafts_validated_source_prefixes_and_remaps_partial_indexes() {
        let metadata = metadata(PARTIAL_PATCH);
        let old_source = OLD_SOURCE.replace('\n', "\r\n");
        let new_source = OLD_SOURCE.replace("Line three.", "Line three, edited.");
        let plan =
            create_source_backed_highlight_plan(&metadata, Some(&old_source), Some(&new_source))
                .unwrap();

        assert!(!plan.metadata.flags.partial);
        assert_eq!(plan.metadata.deletion_lines[0], "defmodule Repro do\n");
        assert_eq!(
            plan.metadata.deletion_lines.last().map(String::as_str),
            Some("  def hello do\n")
        );
        assert_eq!(plan.deletion_line_map, [3, 4, 5, 6]);
        assert_eq!(plan.addition_line_map, [3, 4, 5, 6]);

        let highlighted = HighlightLineArrays {
            deletion_lines: plan
                .metadata
                .deletion_lines
                .iter()
                .enumerate()
                .map(|(index, _)| Some(format!("old-{index}")))
                .collect(),
            addition_lines: plan
                .metadata
                .addition_lines
                .iter()
                .enumerate()
                .map(|(index, _)| Some(format!("new-{index}")))
                .collect(),
        };
        let remapped = remap_source_backed_highlight(&plan, &highlighted);
        assert_eq!(
            remapped.deletion_lines,
            ["old-3", "old-4", "old-5", "old-6"].map(|line| Some(line.into()))
        );
        assert_eq!(
            remapped.addition_lines,
            ["new-3", "new-4", "new-5", "new-6"].map(|line| Some(line.into()))
        );
    }

    #[test]
    fn rejects_raced_mismatched_or_missing_source_snapshots() {
        let metadata = metadata(PARTIAL_PATCH);
        let new_source = OLD_SOURCE.replace("Line three.", "Line three, edited.");
        let mismatched = new_source.replace("Line two.", "A mismatched hidden snapshot.");
        assert!(
            create_source_backed_highlight_plan(&metadata, Some(OLD_SOURCE), Some(&mismatched))
                .is_none()
        );
        assert!(create_source_backed_highlight_plan(&metadata, None, Some(&new_source)).is_none());
    }

    #[test]
    fn accepts_an_absent_side_for_added_and_deleted_files() {
        let added = metadata(
            "diff --git a/added.ex b/added.ex\nnew file mode 100644\n--- /dev/null\n+++ b/added.ex\n@@ -0,0 +1,2 @@\n+@doc \"\"\"\n+body\n\\ No newline at end of file\n",
        );
        let deleted = metadata(
            "diff --git a/deleted.ex b/deleted.ex\ndeleted file mode 100644\n--- a/deleted.ex\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-@doc \"\"\"\n-body\n\\ No newline at end of file\n",
        );

        let added =
            create_source_backed_highlight_plan(&added, None, Some("@doc \"\"\"\nbody")).unwrap();
        let deleted =
            create_source_backed_highlight_plan(&deleted, Some("@doc \"\"\"\nbody"), None).unwrap();
        assert!(added.metadata.deletion_lines.is_empty());
        assert_eq!(
            added.metadata.addition_lines.last().map(String::as_str),
            Some("body")
        );
        assert!(deleted.metadata.addition_lines.is_empty());
        assert_eq!(
            deleted.metadata.deletion_lines.last().map(String::as_str),
            Some("body")
        );
    }

    #[test]
    fn aliases_patch_context_from_the_addition_side_only_without_a_source_plan() {
        let metadata = metadata(PARTIAL_PATCH);
        let mut lines = HighlightLineArrays {
            deletion_lines: (0..4).map(|index| Some(format!("old-{index}"))).collect(),
            addition_lines: (0..4).map(|index| Some(format!("new-{index}"))).collect(),
        };
        alias_context_highlight_lines(&metadata, &mut lines);
        assert_eq!(lines.deletion_lines[0], Some("new-0".into()));
        assert_eq!(lines.addition_lines[0], Some("new-0".into()));
        assert_eq!(lines.deletion_lines[1], Some("old-1".into()));
        assert_eq!(lines.addition_lines[1], Some("new-1".into()));
        assert_eq!(lines.deletion_lines[2], Some("new-2".into()));
        assert_eq!(lines.addition_lines[2], Some("new-2".into()));
    }
}
