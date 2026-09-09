//! Canonical JSON-safe review document shared by every consumer.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    AgentAnnotationConfidence, Changeset, DiffFile, DiffHunk, DiffLineKind, FileChangeKind,
    ReviewSide, review_file_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewFileChangeKind {
    Change,
    RenamePure,
    RenameChanged,
    New,
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewNoteSource {
    Ai,
    Agent,
    User,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticReviewLineAddress {
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReviewRangeAnchor {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_range: Option<[u32; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred: Option<SemanticReviewLineAddress>,
    pub intersecting_hunk_indices: Vec<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_hunk_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReviewNote {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    pub source: ReviewNoteSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_source: Option<String>,
    pub file_key: String,
    pub anchor: SemanticReviewRangeAnchor,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub markup: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
    pub editable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<AgentAnnotationConfidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum SemanticReviewHunkBlock {
    Context {
        lines: usize,
        addition_line_index: usize,
        deletion_line_index: usize,
    },
    Change {
        additions: usize,
        deletions: usize,
        addition_line_index: usize,
        deletion_line_index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReviewHunk {
    pub index: usize,
    pub collapsed_before: usize,
    pub split_line_start: usize,
    pub split_line_count: usize,
    pub unified_line_start: usize,
    pub unified_line_count: usize,
    pub addition_start: u32,
    pub addition_count: u32,
    pub addition_lines: usize,
    pub addition_line_index: usize,
    pub deletion_start: u32,
    pub deletion_count: u32,
    pub deletion_lines: usize,
    pub deletion_line_index: usize,
    pub hunk_content: Vec<SemanticReviewHunkBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_specs: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hunk_context: Option<String>,
    #[serde(rename = "noEOFCRAdditions")]
    pub no_eofcr_additions: bool,
    #[serde(rename = "noEOFCRDeletions")]
    pub no_eofcr_deletions: bool,
}

/// One semantic hunk laid out against new zero-based old/new source origins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebasedSemanticReviewHunk {
    pub hunk: SemanticReviewHunk,
    pub deletion_end_index: usize,
    pub addition_end_index: usize,
}

/// Lay a semantic hunk's blocks out contiguously from new source-array origins.
#[must_use]
pub fn rebase_semantic_review_hunk(
    hunk: &SemanticReviewHunk,
    deletion_origin: usize,
    addition_origin: usize,
) -> RebasedSemanticReviewHunk {
    let mut rebased = hunk.clone();
    rebased.deletion_line_index = deletion_origin;
    rebased.addition_line_index = addition_origin;
    let mut deletion_line_index = deletion_origin;
    let mut addition_line_index = addition_origin;

    for block in &mut rebased.hunk_content {
        match block {
            SemanticReviewHunkBlock::Context {
                lines,
                addition_line_index: block_addition_index,
                deletion_line_index: block_deletion_index,
            } => {
                *block_deletion_index = deletion_line_index;
                *block_addition_index = addition_line_index;
                deletion_line_index = deletion_line_index.saturating_add(*lines);
                addition_line_index = addition_line_index.saturating_add(*lines);
            }
            SemanticReviewHunkBlock::Change {
                additions,
                deletions,
                addition_line_index: block_addition_index,
                deletion_line_index: block_deletion_index,
            } => {
                *block_deletion_index = deletion_line_index;
                *block_addition_index = addition_line_index;
                deletion_line_index = deletion_line_index.saturating_add(*deletions);
                addition_line_index = addition_line_index.saturating_add(*additions);
            }
        }
    }

    RebasedSemanticReviewHunk {
        hunk: rebased,
        deletion_end_index: deletion_line_index,
        addition_end_index: addition_line_index,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReviewLineMoveKind {
    Moved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewLineMoveKinds {
    pub addition_lines: Vec<Option<ReviewLineMoveKind>>,
    pub deletion_lines: Vec<Option<ReviewLineMoveKind>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReviewFileStats {
    pub additions: usize,
    pub deletions: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReviewFileFlags {
    pub untracked: bool,
    pub binary: bool,
    pub too_large: bool,
    pub partial: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SemanticReviewFile {
    pub key: String,
    pub runtime_id: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_path: Option<String>,
    pub change_kind: ReviewFileChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_summary: Option<String>,
    pub stats: SemanticReviewFileStats,
    pub flags: SemanticReviewFileFlags,
    pub patch: String,
    pub split_line_count: usize,
    pub unified_line_count: usize,
    pub addition_lines: Vec<String>,
    pub deletion_lines: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line_move_kinds: Option<ReviewLineMoveKinds>,
    pub hunks: Vec<SemanticReviewHunk>,
    pub content_identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_attested: Option<bool>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticReviewDocument {
    pub files: Vec<SemanticReviewFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewEmptyDiffReason {
    RenameOnly,
    Binary,
    TooLarge,
    NewFile,
    DeletedFile,
    NoHunks,
}

pub fn review_empty_diff_reason(
    change_kind: ReviewFileChangeKind,
    binary: bool,
    too_large: bool,
) -> ReviewEmptyDiffReason {
    if change_kind == ReviewFileChangeKind::RenamePure {
        ReviewEmptyDiffReason::RenameOnly
    } else if binary {
        ReviewEmptyDiffReason::Binary
    } else if too_large {
        ReviewEmptyDiffReason::TooLarge
    } else if change_kind == ReviewFileChangeKind::New {
        ReviewEmptyDiffReason::NewFile
    } else if change_kind == ReviewFileChangeKind::Deleted {
        ReviewEmptyDiffReason::DeletedFile
    } else {
        ReviewEmptyDiffReason::NoHunks
    }
}

pub fn project_review_document(
    changeset: &Changeset,
    source_label: Option<&str>,
) -> SemanticReviewDocument {
    let source_label = source_label.unwrap_or_else(|| changeset.effective_source_label());
    project_review_files(&changeset.files, source_label)
}

/// Project owned semantic content without copying the renderer-model input first.
pub fn project_review_files(files: &[DiffFile], source_label: &str) -> SemanticReviewDocument {
    let mut occurrences = HashMap::<&str, usize>::new();
    SemanticReviewDocument {
        files: files
            .iter()
            .map(|file| {
                let occurrence = occurrences.entry(file.path.as_str()).or_default();
                let projected = project_review_file(file, source_label, *occurrence);
                *occurrence += 1;
                projected
            })
            .collect(),
    }
}

pub fn project_review_file(
    file: &DiffFile,
    source_label: &str,
    duplicate_index: usize,
) -> SemanticReviewFile {
    let full_addition_lines = (!file.flags.partial)
        .then_some(file.sources.new.as_ref())
        .flatten()
        .map(|source| rendered_source_lines(&source.content));
    let full_deletion_lines = (!file.flags.partial)
        .then_some(file.sources.old.as_ref())
        .flatten()
        .map(|source| rendered_source_lines(&source.content));
    let addition_lines_are_full = full_addition_lines.is_some();
    let deletion_lines_are_full = full_deletion_lines.is_some();
    let mut addition_lines = full_addition_lines.unwrap_or_default();
    let mut deletion_lines = full_deletion_lines.unwrap_or_default();
    let mut addition_moves = vec![None; addition_lines.len()];
    let mut deletion_moves = vec![None; deletion_lines.len()];
    let mut projected_hunks = Vec::with_capacity(file.hunks.len());
    let mut old_end = 1_u32;
    let mut new_end = 1_u32;

    for (index, hunk) in file.hunks.iter().enumerate() {
        let addition_line_index = if addition_lines_are_full {
            usize::try_from(hunk.new_start.saturating_sub(1)).unwrap_or(usize::MAX)
        } else {
            addition_lines.len()
        };
        let deletion_line_index = if deletion_lines_are_full {
            usize::try_from(hunk.old_start.saturating_sub(1)).unwrap_or(usize::MAX)
        } else {
            deletion_lines.len()
        };
        for line in &hunk.lines {
            // Complete source snapshots already own these lines. Materialize patch
            // text only for a side whose semantic lines must come from the hunk.
            let render_line = || {
                let mut rendered = line.content.clone();
                if !line.no_newline_at_eof {
                    rendered.push('\n');
                }
                rendered
            };
            if line.new_line.is_some() {
                if addition_lines_are_full {
                    if line.moved
                        && let Some(kind) = line
                            .new_line
                            .and_then(|line| usize::try_from(line.saturating_sub(1)).ok())
                            .and_then(|index| addition_moves.get_mut(index))
                    {
                        *kind = Some(ReviewLineMoveKind::Moved);
                    }
                } else {
                    addition_lines.push(render_line());
                    addition_moves.push(line.moved.then_some(ReviewLineMoveKind::Moved));
                }
            }
            if line.old_line.is_some() {
                if deletion_lines_are_full {
                    if line.moved
                        && let Some(kind) = line
                            .old_line
                            .and_then(|line| usize::try_from(line.saturating_sub(1)).ok())
                            .and_then(|index| deletion_moves.get_mut(index))
                    {
                        *kind = Some(ReviewLineMoveKind::Moved);
                    }
                } else {
                    deletion_lines.push(render_line());
                    deletion_moves.push(line.moved.then_some(ReviewLineMoveKind::Moved));
                }
            }
        }
        // A zero-count unified range points *after* the last unchanged line,
        // whereas a nonempty range starts at the first row inside the hunk.
        // Count that positioned line in the leading gap on the empty side.
        let old_gap = hunk
            .old_start
            .saturating_add(u32::from(hunk.old_count == 0))
            .saturating_sub(old_end);
        let new_gap = hunk
            .new_start
            .saturating_add(u32::from(hunk.new_count == 0))
            .saturating_sub(new_end);
        projected_hunks.push(project_review_hunk(
            hunk,
            index,
            usize::try_from(old_gap.min(new_gap)).unwrap_or(usize::MAX),
            addition_line_index,
            deletion_line_index,
        ));
        old_end = hunk.old_start.saturating_add(hunk.old_count);
        new_end = hunk.new_start.saturating_add(hunk.new_count);
    }

    let change_kind = review_file_change_kind(file);
    let any_moved = addition_moves
        .iter()
        .chain(&deletion_moves)
        .any(Option::is_some);
    SemanticReviewFile {
        key: review_file_key(
            source_label,
            &file.path,
            file.previous_path.as_deref(),
            duplicate_index,
        ),
        runtime_id: file.runtime_id.clone(),
        path: file.path.clone(),
        previous_path: file.previous_path.clone(),
        change_kind,
        language: file.language.clone(),
        agent_summary: file
            .agent
            .as_ref()
            .and_then(|context| context.summary.clone()),
        stats: SemanticReviewFileStats {
            additions: file.stats.additions,
            deletions: file.stats.deletions,
            truncated: file.stats.truncated,
        },
        flags: SemanticReviewFileFlags {
            untracked: file.flags.untracked,
            binary: file.flags.binary,
            too_large: file.flags.too_large,
            partial: file.flags.partial,
        },
        patch: file.patch.clone(),
        split_line_count: file.split_row_count,
        unified_line_count: file.stack_row_count,
        addition_lines,
        deletion_lines,
        line_move_kinds: any_moved.then_some(ReviewLineMoveKinds {
            addition_lines: addition_moves,
            deletion_lines: deletion_moves,
        }),
        hunks: projected_hunks,
        content_identity: crate::identity::review_file_content_identity(file),
        source_identity: file.source_identity.clone(),
        source_attested: file.source_identity.as_ref().map(|_| file.source_attested),
    }
}

fn rendered_source_lines(source: &str) -> Vec<String> {
    source
        .replace("\r\n", "\n")
        .split_inclusive('\n')
        .map(str::to_owned)
        .collect()
}

/// Project a provider change kind into the stable public review vocabulary.
#[must_use]
pub fn review_file_change_kind(file: &DiffFile) -> ReviewFileChangeKind {
    match file.change_kind {
        FileChangeKind::Renamed if file.stats.additions == 0 && file.stats.deletions == 0 => {
            ReviewFileChangeKind::RenamePure
        }
        FileChangeKind::Renamed => ReviewFileChangeKind::RenameChanged,
        FileChangeKind::Added | FileChangeKind::Untracked => ReviewFileChangeKind::New,
        FileChangeKind::Deleted => ReviewFileChangeKind::Deleted,
        FileChangeKind::Modified
        | FileChangeKind::Copied
        | FileChangeKind::TypeChanged
        | FileChangeKind::Conflicted => ReviewFileChangeKind::Change,
    }
}

fn project_review_hunk(
    hunk: &DiffHunk,
    index: usize,
    collapsed_before: usize,
    addition_line_index: usize,
    deletion_line_index: usize,
) -> SemanticReviewHunk {
    let mut blocks = Vec::<SemanticReviewHunkBlock>::new();
    let mut addition_cursor = addition_line_index;
    let mut deletion_cursor = deletion_line_index;
    for line in &hunk.lines {
        match line.kind {
            DiffLineKind::Context => match blocks.last_mut() {
                Some(SemanticReviewHunkBlock::Context { lines, .. }) => *lines += 1,
                _ => blocks.push(SemanticReviewHunkBlock::Context {
                    lines: 1,
                    addition_line_index: addition_cursor,
                    deletion_line_index: deletion_cursor,
                }),
            },
            DiffLineKind::Addition => match blocks.last_mut() {
                Some(SemanticReviewHunkBlock::Change { additions, .. }) => *additions += 1,
                _ => blocks.push(SemanticReviewHunkBlock::Change {
                    additions: 1,
                    deletions: 0,
                    addition_line_index: addition_cursor,
                    deletion_line_index: deletion_cursor,
                }),
            },
            DiffLineKind::Deletion => match blocks.last_mut() {
                Some(SemanticReviewHunkBlock::Change { deletions, .. }) => *deletions += 1,
                _ => blocks.push(SemanticReviewHunkBlock::Change {
                    additions: 0,
                    deletions: 1,
                    addition_line_index: addition_cursor,
                    deletion_line_index: deletion_cursor,
                }),
            },
        }
        addition_cursor += usize::from(line.new_line.is_some());
        deletion_cursor += usize::from(line.old_line.is_some());
    }
    SemanticReviewHunk {
        index,
        collapsed_before,
        split_line_start: hunk.split_row_start,
        split_line_count: hunk.split_row_count,
        unified_line_start: hunk.stack_row_start,
        unified_line_count: hunk.stack_row_count,
        addition_start: hunk.new_start,
        addition_count: hunk.new_count,
        addition_lines: hunk
            .lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Addition)
            .count(),
        addition_line_index,
        deletion_start: hunk.old_start,
        deletion_count: hunk.old_count,
        deletion_lines: hunk
            .lines
            .iter()
            .filter(|line| line.kind == DiffLineKind::Deletion)
            .count(),
        deletion_line_index,
        hunk_content: blocks,
        hunk_specs: (!hunk.header.is_empty()).then(|| hunk.header.clone()),
        hunk_context: hunk.context.clone(),
        no_eofcr_additions: hunk
            .lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Addition && line.no_newline_at_eof),
        no_eofcr_deletions: hunk
            .lines
            .iter()
            .any(|line| line.kind == DiffLineKind::Deletion && line.no_newline_at_eof),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ChangesetSource, DiffLine, FileFlags, FileSourceSnapshots, FileStats, SourceOrigin,
        SourceSnapshot,
    };

    fn file(runtime_id: &str, path: &str, replacement: &str) -> DiffFile {
        let mut file = DiffFile {
            key: String::new(),
            runtime_id: runtime_id.into(),
            path: path.into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("rust".into()),
            stats: FileStats {
                additions: 1,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: format!("@@ -3 +3 @@\n-old\n+{replacement}\n"),
            split_row_count: 1,
            stack_row_count: 2,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -3 +3 @@".into(),
                context: None,
                old_start: 3,
                old_count: 1,
                new_start: 3,
                new_count: 1,
                split_row_start: 0,
                split_row_count: 1,
                stack_row_start: 0,
                stack_row_count: 2,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        content: "old".into(),
                        old_line: Some(3),
                        new_line: None,
                        moved: false,
                        no_newline_at_eof: false,
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        content: replacement.into(),
                        old_line: None,
                        new_line: Some(3),
                        moved: true,
                        no_newline_at_eof: false,
                    },
                ],
            }],
            content_identity: String::new(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_capability: None,
            source_attested: false,
            agent: None,
        };
        file.refresh_identity();
        file
    }

    fn changeset(files: Vec<DiffFile>) -> Changeset {
        Changeset {
            id: "HEAD".into(),
            source_label: "HEAD".into(),
            title: "review".into(),
            summary: None,
            agent_summary: None,
            source: ChangesetSource::WorkingTree { staged: false },
            files,
        }
    }

    #[test]
    fn zero_count_hunks_include_the_positioned_line_in_the_leading_gap() {
        for (old_start, old_count, new_start, new_count, expected) in [
            (6, 0, 7, 1, 6),
            (6, 1, 5, 0, 5),
            (0, 0, 1, 1, 0),
            (1, 1, 0, 0, 0),
            (6, 1, 6, 1, 5),
        ] {
            let mut input = file("geometry", "geometry.rs", "new");
            let hunk = &mut input.hunks[0];
            hunk.old_start = old_start;
            hunk.old_count = old_count;
            hunk.new_start = new_start;
            hunk.new_count = new_count;
            hunk.lines.retain_mut(|line| match line.kind {
                DiffLineKind::Addition => {
                    line.new_line = Some(new_start);
                    new_count != 0
                }
                DiffLineKind::Deletion => {
                    line.old_line = Some(old_start);
                    old_count != 0
                }
                _ => false,
            });
            assert_eq!(
                project_review_file(&input, "conformance", 0).hunks[0].collapsed_before,
                expected,
                "@@ -{old_start},{old_count} +{new_start},{new_count} @@",
            );
        }
    }

    #[test]
    fn semantic_lines_choose_each_source_side_or_patch_without_changing_eof() {
        for partial in [false, true] {
            for old in [false, true] {
                for new in [false, true] {
                    for no_newline in [false, true] {
                        let mut input = file("mixed", "mixed.rs", "new");
                        input.flags.partial = partial;
                        for line in &mut input.hunks[0].lines {
                            line.no_newline_at_eof = no_newline;
                        }
                        input.set_sources(FileSourceSnapshots {
                            old: old.then(|| {
                                SourceSnapshot::new(
                                    "source old".into(),
                                    SourceOrigin::WorkingTree,
                                    true,
                                )
                            }),
                            new: new.then(|| {
                                SourceSnapshot::new(
                                    "source new".into(),
                                    SourceOrigin::WorkingTree,
                                    true,
                                )
                            }),
                        });
                        let projected = project_review_file(&input, "review", 0);
                        let expected = |full, source, patch| {
                            if full && !partial {
                                source
                            } else if no_newline {
                                patch
                            } else if patch == "old" {
                                "old\n"
                            } else {
                                "new\n"
                            }
                        };
                        assert_eq!(
                            projected.deletion_lines,
                            [expected(old, "source old", "old")]
                        );
                        assert_eq!(
                            projected.addition_lines,
                            [expected(new, "source new", "new")]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn projection_preserves_order_geometry_lines_and_move_kinds() {
        let document = project_review_document(
            &changeset(vec![
                file("alpha", "alpha.rs", "new"),
                file("beta", "beta.rs", "next"),
            ]),
            None,
        );
        assert_eq!(
            document
                .files
                .iter()
                .map(|file| (file.path.as_str(), file.runtime_id.as_str()))
                .collect::<Vec<_>>(),
            [("alpha.rs", "alpha"), ("beta.rs", "beta")]
        );
        let alpha = &document.files[0];
        assert_eq!(alpha.hunks[0].collapsed_before, 2);
        assert_eq!(alpha.addition_lines, ["new\n"]);
        assert_eq!(alpha.deletion_lines, ["old\n"]);
        assert_eq!(
            alpha.line_move_kinds.as_ref().unwrap().addition_lines,
            [Some(ReviewLineMoveKind::Moved)]
        );
        assert_eq!(alpha.hunks[0].hunk_content.len(), 1);
    }

    #[test]
    fn stable_addresses_ignore_content_and_separate_duplicates_and_reviews() {
        let before = project_review_document(
            &changeset(vec![
                file("one", "same.rs", "first"),
                file("two", "same.rs", "second"),
            ]),
            None,
        );
        let after =
            project_review_document(&changeset(vec![file("three", "same.rs", "changed")]), None);
        assert_eq!(before.files[0].key, after.files[0].key);
        assert_ne!(
            before.files[0].content_identity,
            after.files[0].content_identity
        );
        assert_ne!(before.files[0].key, before.files[1].key);
        assert_ne!(
            before.files[0].key,
            project_review_document(
                &changeset(vec![file("one", "same.rs", "first")]),
                Some("HEAD~1")
            )
            .files[0]
                .key
        );
    }

    #[test]
    fn source_identity_and_attestation_are_projected_without_source_text() {
        let mut file = file("alpha", "alpha.rs", "new");
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                "one\ntwo\nold\n".into(),
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                "one\ntwo\nnew\n".into(),
                SourceOrigin::WorkingTree,
                true,
            )),
        });
        let projected = project_review_document(&changeset(vec![file]), None);
        assert!(projected.files[0].source_identity.is_some());
        assert_eq!(projected.files[0].source_attested, Some(true));
        let json = serde_json::to_string(&projected).unwrap();
        assert!(!json.contains("one\\ntwo"));
    }

    #[test]
    fn empty_diff_reason_has_canonical_precedence() {
        assert_eq!(
            review_empty_diff_reason(ReviewFileChangeKind::RenamePure, true, true),
            ReviewEmptyDiffReason::RenameOnly
        );
        assert_eq!(
            review_empty_diff_reason(ReviewFileChangeKind::New, true, true),
            ReviewEmptyDiffReason::Binary
        );
        assert_eq!(
            review_empty_diff_reason(ReviewFileChangeKind::Deleted, false, true),
            ReviewEmptyDiffReason::TooLarge
        );
        assert_eq!(
            review_empty_diff_reason(ReviewFileChangeKind::Change, false, false),
            ReviewEmptyDiffReason::NoHunks
        );
    }

    #[test]
    fn semantic_wire_names_match_the_shared_v1_contract() {
        let file =
            project_review_document(&changeset(vec![file("alpha", "alpha.rs", "new")]), None)
                .files
                .remove(0);
        let value = serde_json::to_value(file).unwrap();
        assert_eq!(value["changeKind"], "change");
        assert_eq!(value["flags"]["tooLarge"], false);
        assert!(value["flags"].get("too_large").is_none());
        assert_eq!(value["hunks"][0]["noEOFCRAdditions"], false);
        assert_eq!(value["hunks"][0]["hunkContent"][0]["type"], "change");
        assert_eq!(value["hunks"][0]["hunkContent"][0]["additionLineIndex"], 0);
        assert!(value.get("sourceIdentity").is_none());

        let anchor = SemanticReviewRangeAnchor {
            old_range: Some([2, 4]),
            new_range: None,
            preferred: Some(SemanticReviewLineAddress {
                side: ReviewSide::Old,
                line: 3,
            }),
            intersecting_hunk_indices: vec![0, 1],
            owner_hunk_index: Some(0),
        };
        assert_eq!(
            serde_json::to_value(anchor).unwrap()["oldRange"],
            serde_json::json!([2, 4])
        );
    }
}
