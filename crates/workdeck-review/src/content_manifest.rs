//! Deterministic semantic snapshot of a canonical review document.

use serde::{Deserialize, Serialize};
use workdeck_core::{
    ReviewEmptyDiffReason, ReviewFileChangeKind, ReviewSide, SemanticReviewDocument,
    SemanticReviewFile, SemanticReviewFileFlags, SemanticReviewFileStats, SemanticReviewHunk,
    SemanticReviewHunkBlock, SemanticReviewLineAddress, review_empty_diff_reason,
};

use crate::{ReviewGapAddress, ReviewGapGeometry, ReviewGapHunk, review_gap_id};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewContentManifestGap {
    pub gap_id: String,
    pub old_range: [u32; 2],
    pub new_range: [u32; 2],
    pub line_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewContentManifestHunk {
    pub index: usize,
    pub old_range: [u32; 2],
    pub new_range: [u32; 2],
    pub default_note_target: SemanticReviewLineAddress,
    pub blocks: Vec<SemanticReviewHunkBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_gap: Option<ReviewContentManifestGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewContentManifestFile {
    pub key: String,
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
    pub content_identity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_identity: Option<String>,
    pub split_line_count: usize,
    pub unified_line_count: usize,
    pub patch: String,
    pub addition_lines: Vec<String>,
    pub deletion_lines: Vec<String>,
    pub expansion_side: ReviewSide,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub empty_diff_reason: Option<ReviewEmptyDiffReason>,
    pub hunks: Vec<ReviewContentManifestHunk>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trailing_gap: Option<ReviewContentManifestGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewContentManifest {
    pub version: u8,
    pub files: Vec<ReviewContentManifestFile>,
}

pub fn build_review_content_manifest_file(file: &SemanticReviewFile) -> ReviewContentManifestFile {
    let gap_geometry = semantic_gap_geometry(file);
    ReviewContentManifestFile {
        key: file.key.clone(),
        path: file.path.clone(),
        previous_path: file.previous_path.clone(),
        change_kind: file.change_kind,
        language: file.language.clone(),
        agent_summary: file.agent_summary.clone(),
        stats: file.stats,
        flags: file.flags,
        content_identity: file.content_identity.clone(),
        source_identity: file.source_identity.clone(),
        split_line_count: file.split_line_count,
        unified_line_count: file.unified_line_count,
        patch: file.patch.clone(),
        addition_lines: file.addition_lines.clone(),
        deletion_lines: file.deletion_lines.clone(),
        expansion_side: if file.change_kind == ReviewFileChangeKind::Deleted {
            ReviewSide::Old
        } else {
            ReviewSide::New
        },
        empty_diff_reason: file.hunks.is_empty().then(|| {
            review_empty_diff_reason(file.change_kind, file.flags.binary, file.flags.too_large)
        }),
        hunks: file
            .hunks
            .iter()
            .enumerate()
            .map(|(index, hunk)| ReviewContentManifestHunk {
                index,
                old_range: hunk_range(hunk.deletion_start, hunk.deletion_count),
                new_range: hunk_range(hunk.addition_start, hunk.addition_count),
                default_note_target: default_note_target(hunk),
                blocks: hunk.hunk_content.clone(),
                leading_gap: gap_geometry.leading_gap(index).map(manifest_gap),
            })
            .collect(),
        trailing_gap: gap_geometry.trailing_gap().map(manifest_gap),
    }
}

pub fn build_review_content_manifest(document: &SemanticReviewDocument) -> ReviewContentManifest {
    ReviewContentManifest {
        version: 1,
        files: document
            .files
            .iter()
            .map(build_review_content_manifest_file)
            .collect(),
    }
}

fn semantic_gap_geometry(file: &SemanticReviewFile) -> ReviewGapGeometry {
    ReviewGapGeometry {
        hunks: file
            .hunks
            .iter()
            .map(|hunk| ReviewGapHunk {
                collapsed_before: hunk.collapsed_before,
                addition_start: hunk.addition_start,
                addition_count: hunk.addition_count,
                deletion_start: hunk.deletion_start,
                deletion_count: hunk.deletion_count,
                addition_line_index: hunk.addition_line_index,
                deletion_line_index: hunk.deletion_line_index,
            })
            .collect(),
        addition_line_count: file.addition_lines.len(),
        deletion_line_count: file.deletion_lines.len(),
        is_partial: file.flags.partial,
    }
}

fn manifest_gap(address: ReviewGapAddress) -> ReviewContentManifestGap {
    ReviewContentManifestGap {
        gap_id: review_gap_id(address.position, address.hunk_index),
        old_range: [address.old_range.start, address.old_range.end],
        new_range: [address.new_range.start, address.new_range.end],
        line_count: address.line_count,
    }
}

fn hunk_range(start: u32, count: u32) -> [u32; 2] {
    [start, start.saturating_add(count.max(1)).saturating_sub(1)]
}

fn default_note_target(hunk: &SemanticReviewHunk) -> SemanticReviewLineAddress {
    let mut deletion_line = hunk.deletion_start;
    let mut addition_line = hunk.addition_start;
    let mut first_deletion_line = None;
    for block in &hunk.hunk_content {
        match block {
            SemanticReviewHunkBlock::Context { lines, .. } => {
                let lines = u32::try_from(*lines).unwrap_or(u32::MAX);
                deletion_line = deletion_line.saturating_add(lines);
                addition_line = addition_line.saturating_add(lines);
            }
            SemanticReviewHunkBlock::Change {
                additions,
                deletions,
                ..
            } => {
                if *additions > 0 {
                    return SemanticReviewLineAddress {
                        side: ReviewSide::New,
                        line: addition_line,
                    };
                }
                if *deletions > 0 && first_deletion_line.is_none() {
                    first_deletion_line = Some(deletion_line);
                }
                deletion_line =
                    deletion_line.saturating_add(u32::try_from(*deletions).unwrap_or(u32::MAX));
                addition_line =
                    addition_line.saturating_add(u32::try_from(*additions).unwrap_or(u32::MAX));
            }
        }
    }
    first_deletion_line.map_or(
        SemanticReviewLineAddress {
            side: ReviewSide::New,
            line: hunk.addition_start,
        },
        |line| SemanticReviewLineAddress {
            side: ReviewSide::Old,
            line,
        },
    )
}

#[cfg(test)]
mod tests {
    use workdeck_core::{
        Changeset, ChangesetSource, DiffFile, DiffHunk, DiffLine, DiffLineKind, FileChangeKind,
        FileFlags, FileSourceSnapshots, FileStats, SourceOrigin, SourceSnapshot,
        project_review_document,
    };

    use super::*;
    use crate::ReviewGapPosition;

    fn projected_file() -> SemanticReviewFile {
        let mut file = DiffFile {
            key: String::new(),
            runtime_id: "runtime".into(),
            path: "alpha.rs".into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: Some("rust".into()),
            stats: FileStats {
                additions: 1,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: "@@ -6 +6 @@\n-old\n+new\n".into(),
            split_row_count: 1,
            stack_row_count: 2,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -6 +6 @@".into(),
                context: None,
                old_start: 6,
                old_count: 1,
                new_start: 6,
                new_count: 1,
                split_row_start: 0,
                split_row_count: 1,
                stack_row_start: 0,
                stack_row_count: 2,
                lines: vec![
                    DiffLine {
                        kind: DiffLineKind::Deletion,
                        content: "old".into(),
                        old_line: Some(6),
                        new_line: None,
                        moved: false,
                        no_newline_at_eof: false,
                    },
                    DiffLine {
                        kind: DiffLineKind::Addition,
                        content: "new".into(),
                        old_line: None,
                        new_line: Some(6),
                        moved: false,
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
        let old_source = (1..=12)
            .map(|line| {
                if line == 6 {
                    "old".into()
                } else {
                    format!("line {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        let new_source = (1..=12)
            .map(|line| {
                if line == 6 {
                    "new".into()
                } else {
                    format!("line {line}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
        file.set_sources(FileSourceSnapshots {
            old: Some(SourceSnapshot::new(
                old_source,
                SourceOrigin::Revision {
                    revision: "HEAD".into(),
                },
                true,
            )),
            new: Some(SourceSnapshot::new(
                new_source,
                SourceOrigin::WorkingTree,
                true,
            )),
        });
        file.refresh_identity();
        project_review_document(
            &Changeset {
                id: "HEAD".into(),
                source_label: "HEAD".into(),
                title: "review".into(),
                summary: None,
                agent_summary: None,
                source: ChangesetSource::WorkingTree { staged: false },
                files: vec![file],
            },
            None,
        )
        .files
        .remove(0)
    }

    #[test]
    fn manifest_records_content_derived_geometry_and_gaps() {
        let file = projected_file();
        let manifest = build_review_content_manifest_file(&file);
        assert_eq!(manifest.key, file.key);
        assert_eq!(manifest.patch, file.patch);
        assert_eq!(manifest.addition_lines[5], "new\n");
        assert_eq!(manifest.deletion_lines[5], "old\n");
        assert_eq!(manifest.hunks[0].old_range, [6, 6]);
        assert_eq!(manifest.hunks[0].new_range, [6, 6]);
        assert_eq!(
            manifest.hunks[0].default_note_target,
            SemanticReviewLineAddress {
                side: ReviewSide::New,
                line: 6
            }
        );
        assert_eq!(
            manifest.hunks[0].leading_gap.as_ref().unwrap().gap_id,
            review_gap_id(ReviewGapPosition::Before, 0)
        );
        assert_eq!(
            manifest.hunks[0].leading_gap.as_ref().unwrap().old_range,
            [1, 5]
        );
        assert_eq!(manifest.trailing_gap.as_ref().unwrap().old_range, [7, 12]);
        assert_eq!(manifest.hunks[0].blocks, file.hunks[0].hunk_content);
    }

    #[test]
    fn manifest_gap_geometry_matches_owned_sources_without_retaining_their_text() {
        for partial in [false, true] {
            for (old_len, new_len) in [(0, 0), (6, 6), (12, 12), (12, 13)] {
                let mut file = projected_file();
                file.flags.partial = partial;
                file.deletion_lines.resize(old_len, "old tail\n".into());
                file.addition_lines.resize(new_len, "new tail\n".into());
                let geometry = semantic_gap_geometry(&file);
                let owned = crate::ReviewGapSource {
                    hunks: geometry.hunks.clone(),
                    addition_lines: file.addition_lines.clone(),
                    deletion_lines: file.deletion_lines.clone(),
                    is_partial: partial,
                };
                let manifest = build_review_content_manifest_file(&file);
                for index in 0..=file.hunks.len() {
                    assert_eq!(
                        geometry.leading_gap(index),
                        crate::review_leading_gap(&owned, index)
                    );
                }
                assert_eq!(geometry.trailing_gap(), crate::review_trailing_gap(&owned));
                assert_eq!(
                    manifest.trailing_gap,
                    geometry.trailing_gap().map(manifest_gap)
                );
                assert_eq!(manifest.addition_lines, file.addition_lines);
                assert_eq!(manifest.deletion_lines, file.deletion_lines);
                file.addition_lines.clear();
                file.deletion_lines.clear();
                assert_eq!(geometry.addition_line_count, new_len);
                assert_eq!(geometry.deletion_line_count, old_len);
                assert_eq!(manifest.addition_lines.len(), new_len);
                assert_eq!(manifest.deletion_lines.len(), old_len);
            }
        }
    }

    #[test]
    fn empty_files_carry_one_canonical_reason() {
        let mut file = projected_file();
        file.hunks.clear();
        file.change_kind = ReviewFileChangeKind::RenamePure;
        file.flags.binary = true;
        file.flags.too_large = true;
        assert_eq!(
            build_review_content_manifest_file(&file).empty_diff_reason,
            Some(ReviewEmptyDiffReason::RenameOnly)
        );
    }
}
