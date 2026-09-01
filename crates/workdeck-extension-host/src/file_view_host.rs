use std::sync::Arc;

use workdeck_core::{DiffFile, DiffLineKind, FileChangeKind};
use workdeck_extension_api::{
    ExtensionDiffFile, ExtensionDiffHunk, ExtensionDiffStats, ExtensionFileChangeKind,
    ExtensionFileChangeRange, ExtensionFileSide,
};

use crate::{ExtensionDocumentReader, ExtensionRequestCancellation};

/// Build public added/removed ranges from parsed hunks without renderer types.
#[must_use]
pub fn file_view_changes(file: &DiffFile) -> Vec<ExtensionFileChangeRange> {
    let mut changes = Vec::new();
    for (hunk_index, hunk) in file.hunks.iter().enumerate() {
        let mut old_line = usize::try_from(hunk.old_start).unwrap_or(usize::MAX);
        let mut new_line = usize::try_from(hunk.new_start).unwrap_or(usize::MAX);
        let mut deletion_start = old_line;
        let mut addition_start = new_line;
        let mut deletions = 0_usize;
        let mut additions = 0_usize;

        let flush = |changes: &mut Vec<ExtensionFileChangeRange>,
                     deletion_start: usize,
                     deletions: &mut usize,
                     addition_start: usize,
                     additions: &mut usize| {
            if *deletions > 0 {
                changes.push(ExtensionFileChangeRange {
                    hunk_index,
                    kind: ExtensionFileChangeKind::Removed,
                    range: [
                        deletion_start,
                        deletion_start.saturating_add(*deletions).saturating_sub(1),
                    ],
                });
            }
            if *additions > 0 {
                changes.push(ExtensionFileChangeRange {
                    hunk_index,
                    kind: ExtensionFileChangeKind::Added,
                    range: [
                        addition_start,
                        addition_start.saturating_add(*additions).saturating_sub(1),
                    ],
                });
            }
            *deletions = 0;
            *additions = 0;
        };

        for line in &hunk.lines {
            match line.kind {
                DiffLineKind::Context => {
                    flush(
                        &mut changes,
                        deletion_start,
                        &mut deletions,
                        addition_start,
                        &mut additions,
                    );
                    old_line = old_line.saturating_add(1);
                    new_line = new_line.saturating_add(1);
                    deletion_start = old_line;
                    addition_start = new_line;
                }
                DiffLineKind::Deletion => {
                    if deletions == 0 {
                        deletion_start = old_line;
                    }
                    deletions = deletions.saturating_add(1);
                    old_line = old_line.saturating_add(1);
                }
                DiffLineKind::Addition => {
                    if additions == 0 {
                        addition_start = new_line;
                    }
                    additions = additions.saturating_add(1);
                    new_line = new_line.saturating_add(1);
                }
            }
        }
        flush(
            &mut changes,
            deletion_start,
            &mut deletions,
            addition_start,
            &mut additions,
        );
    }
    changes
}

#[derive(Debug, Clone)]
pub struct FileViewInputSnapshot {
    pub file: Arc<ExtensionDiffFile>,
    pub changes: Arc<[ExtensionFileChangeRange]>,
}

/// Derive immutable public file data shared by matching and one layout request.
#[must_use]
pub fn create_file_view_input_snapshot(file: &DiffFile) -> FileViewInputSnapshot {
    FileViewInputSnapshot {
        file: Arc::new(to_extension_diff_file(file)),
        changes: Arc::from(file_view_changes(file)),
    }
}

#[derive(Debug, Clone)]
pub struct FileViewInput {
    pub file: Arc<ExtensionDiffFile>,
    pub width: usize,
    pub cancellation: ExtensionRequestCancellation,
    pub changes: Arc<[ExtensionFileChangeRange]>,
    pub documents: ExtensionDocumentReader,
}

/// Build one native file-view input from reusable immutable file data.
#[must_use]
pub fn create_file_view_input(
    file: &DiffFile,
    width: usize,
    cancellation: ExtensionRequestCancellation,
    snapshot: Option<&FileViewInputSnapshot>,
) -> FileViewInput {
    let snapshot = snapshot
        .cloned()
        .unwrap_or_else(|| create_file_view_input_snapshot(file));
    let sources = file.sources.clone();
    FileViewInput {
        file: snapshot.file,
        width,
        cancellation,
        changes: snapshot.changes,
        documents: ExtensionDocumentReader::new(move |side| {
            Ok(match side {
                ExtensionFileSide::Old => sources.old.as_ref(),
                ExtensionFileSide::New => sources.new.as_ref(),
            }
            .map(|snapshot| snapshot.content.clone()))
        }),
    }
}

#[must_use]
pub fn file_view_hunk_count(file: &DiffFile) -> usize {
    file.hunks.len()
}

fn to_extension_diff_file(file: &DiffFile) -> ExtensionDiffFile {
    ExtensionDiffFile {
        id: file.runtime_id.clone(),
        path: file.path.clone(),
        previous_path: file.previous_path.clone(),
        patch: file.patch.clone(),
        language: file.language.clone(),
        stats: ExtensionDiffStats {
            additions: file.stats.additions,
            deletions: file.stats.deletions,
        },
        change_type: extension_change_type(file).into(),
        stats_truncated: file.stats.truncated,
        hunks: file
            .hunks
            .iter()
            .enumerate()
            .map(|(index, hunk)| ExtensionDiffHunk {
                index,
                header: hunk.formatted_header(),
                old_range: inclusive_range(hunk.old_start, hunk.old_count),
                new_range: inclusive_range(hunk.new_start, hunk.new_count),
            })
            .collect(),
        agent: file.agent.clone(),
        is_untracked: file.flags.untracked,
        is_binary: file.flags.binary,
        is_too_large: file.flags.too_large,
    }
}

fn inclusive_range(start: u32, count: u32) -> Option<[u32; 2]> {
    (count > 0).then(|| [start, start.saturating_add(count).saturating_sub(1)])
}

fn extension_change_type(file: &DiffFile) -> &'static str {
    match file.change_kind {
        FileChangeKind::Renamed if file.stats.additions == 0 && file.stats.deletions == 0 => {
            "rename-pure"
        }
        FileChangeKind::Renamed => "rename-changed",
        FileChangeKind::Added | FileChangeKind::Untracked | FileChangeKind::Copied => "new",
        FileChangeKind::Deleted => "deleted",
        FileChangeKind::Modified | FileChangeKind::TypeChanged | FileChangeKind::Conflicted => {
            "change"
        }
    }
}

#[cfg(test)]
mod tests {
    use workdeck_core::{
        DiffHunk, DiffLine, FileFlags, FileSourceSnapshots, FileStats, SourceOrigin, SourceSnapshot,
    };

    use super::*;

    #[test]
    fn exposes_only_added_and_removed_ranges() {
        let changes = file_view_changes(&file());
        assert!(!changes.is_empty());
        assert_eq!(
            changes
                .iter()
                .map(|change| change.kind)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from([
                ExtensionFileChangeKind::Added,
                ExtensionFileChangeKind::Removed,
            ])
        );
        assert_eq!(
            changes,
            [
                ExtensionFileChangeRange {
                    hunk_index: 0,
                    kind: ExtensionFileChangeKind::Removed,
                    range: [1, 1],
                },
                ExtensionFileChangeRange {
                    hunk_index: 0,
                    kind: ExtensionFileChangeKind::Added,
                    range: [1, 1],
                },
                ExtensionFileChangeRange {
                    hunk_index: 0,
                    kind: ExtensionFileChangeKind::Removed,
                    range: [3, 3],
                },
                ExtensionFileChangeRange {
                    hunk_index: 0,
                    kind: ExtensionFileChangeKind::Added,
                    range: [3, 3],
                },
            ]
        );
    }

    #[test]
    fn reuses_one_immutable_file_and_change_snapshot_for_matching_and_layout() {
        let file = file();
        let snapshot = create_file_view_input_snapshot(&file);
        let cancellation = ExtensionRequestCancellation::default();
        let input = create_file_view_input(&file, 72, cancellation.clone(), Some(&snapshot));
        assert!(Arc::ptr_eq(&input.file, &snapshot.file));
        assert!(Arc::ptr_eq(&input.changes, &snapshot.changes));
        assert!(input.cancellation.ptr_eq(&cancellation));
        assert_eq!(input.width, 72);
    }

    #[test]
    fn exposes_exact_document_text_and_hunk_count() {
        let file = file();
        let input =
            create_file_view_input(&file, 80, ExtensionRequestCancellation::default(), None);
        assert_eq!(
            input
                .documents
                .read_document(ExtensionFileSide::New)
                .wait(&input.cancellation),
            Ok(Some("after\nstable\nadded\n".into()))
        );
        assert_eq!(file_view_hunk_count(&file), 1);
    }

    fn file() -> DiffFile {
        DiffFile {
            key: "file-key".into(),
            runtime_id: "file:0".into(),
            path: "example.txt".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("text".into()),
            stats: FileStats {
                additions: 2,
                deletions: 2,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: "@@ -1,3 +1,3 @@\n-before\n+after\n stable\n-removed\n+added\n".into(),
            split_row_count: 4,
            stack_row_count: 5,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -1,3 +1,3 @@".into(),
                context: None,
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 3,
                split_row_start: 0,
                split_row_count: 4,
                stack_row_start: 0,
                stack_row_count: 5,
                lines: vec![
                    line(DiffLineKind::Deletion, Some(1), None, "before"),
                    line(DiffLineKind::Addition, None, Some(1), "after"),
                    line(DiffLineKind::Context, Some(2), Some(2), "stable"),
                    line(DiffLineKind::Deletion, Some(3), None, "removed"),
                    line(DiffLineKind::Addition, None, Some(3), "added"),
                ],
            }],
            content_identity: "content".into(),
            sources: FileSourceSnapshots {
                old: Some(SourceSnapshot::new(
                    "before\nstable\nremoved\n".into(),
                    SourceOrigin::Revision {
                        revision: "HEAD".into(),
                    },
                    true,
                )),
                new: Some(SourceSnapshot::new(
                    "after\nstable\nadded\n".into(),
                    SourceOrigin::WorkingTree,
                    true,
                )),
            },
            source_identity: None,
            source_attested: true,
            agent: None,
        }
    }

    fn line(
        kind: DiffLineKind,
        old_line: Option<u32>,
        new_line: Option<u32>,
        content: &str,
    ) -> DiffLine {
        DiffLine {
            kind,
            content: content.into(),
            old_line,
            new_line,
            moved: false,
            no_newline_at_eof: false,
        }
    }
}
