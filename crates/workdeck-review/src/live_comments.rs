//! Renderer-independent targeting and construction for live review comments.

use workdeck_core::{AgentAnnotationConfidence, DiffFile, ReviewSide};

use crate::{
    CommentAnchor, ReviewComment, ReviewError, ReviewLineTarget, review_default_hunk_line_target,
    review_hunk_index_for_line, review_line_anchor,
};

#[derive(Debug, Clone)]
pub struct CommentTargetInput {
    pub file_path: String,
    pub hunk_index: Option<usize>,
    pub side: Option<ReviewSide>,
    pub line: Option<u32>,
    pub summary: String,
    pub rationale: Option<String>,
    pub markup: Option<String>,
    pub author: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedCommentTarget {
    pub hunk_index: usize,
    pub side: ReviewSide,
    pub line: u32,
}

pub fn find_diff_file_by_path<'a>(files: &'a [DiffFile], file_path: &str) -> Option<&'a DiffFile> {
    files
        .iter()
        .find(|file| file.path == file_path || file.previous_path.as_deref() == Some(file_path))
}

pub fn find_hunk_index_for_line(file: &DiffFile, side: ReviewSide, line: u32) -> Option<usize> {
    review_hunk_index_for_line(&file.hunks, side, line)
}

pub fn resolve_comment_target(
    file: &DiffFile,
    input: &CommentTargetInput,
) -> Result<ResolvedCommentTarget, ReviewError> {
    if let Some(hunk_index) = input.hunk_index {
        let hunk = file.hunks.get(hunk_index).ok_or_else(|| {
            ReviewError::InvalidCommentTarget(format!(
                "no diff hunk {} exists in {}",
                hunk_index.saturating_add(1),
                input.file_path
            ))
        })?;
        let target = review_default_hunk_line_target(hunk);
        return Ok(ResolvedCommentTarget {
            hunk_index,
            side: target.side,
            line: target.line,
        });
    }

    let (Some(side), Some(line)) = (input.side, input.line) else {
        return Err(ReviewError::InvalidCommentTarget(format!(
            "specify either hunk index or both side and line for {}",
            input.file_path
        )));
    };
    let hunk_index = find_hunk_index_for_line(file, side, line).ok_or_else(|| {
        ReviewError::InvalidCommentTarget(format!(
            "no {side:?} diff hunk in {} covers line {line}",
            input.file_path
        ))
    })?;
    Ok(ResolvedCommentTarget {
        hunk_index,
        side,
        line,
    })
}

pub fn build_live_comment(
    file: &DiffFile,
    input: CommentTargetInput,
    comment_id: String,
    created_at: String,
    target: ResolvedCommentTarget,
) -> ReviewComment {
    let anchor = review_line_anchor(
        &file.hunks,
        ReviewLineTarget {
            hunk_index: target.hunk_index,
            side: target.side,
            line: target.line,
        },
    );
    ReviewComment {
        id: comment_id,
        parent_id: None,
        source: "mcp".into(),
        author: input.author,
        created_at: Some(created_at),
        file_path: Some(file.path.clone()),
        hunk_index: Some(target.hunk_index),
        side: Some(target.side),
        line: Some(target.line),
        summary: input.summary,
        rationale: input.rationale,
        markup: input.markup,
        title: None,
        tags: vec!["mcp".into()],
        confidence: Some(AgentAnnotationConfidence::High),
        updated_at: None,
        resolution: crate::ReviewNoteResolution::Active,
        anchor: CommentAnchor {
            file_key: file.key.clone(),
            old_range: anchor.old_range,
            new_range: anchor.new_range,
            preferred_side: anchor.preferred.map(|preferred| preferred.side),
            preferred_line: anchor.preferred.map(|preferred| preferred.line),
            intersecting_hunk_indices: anchor.intersecting_hunk_indices,
            owner_hunk_index: anchor.owner_hunk_index,
        },
        editable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{
        DiffHunk, DiffLine, DiffLineKind, FileChangeKind, FileFlags, FileSourceSnapshots,
        FileStats, LineRange,
    };

    fn example_file() -> DiffFile {
        DiffFile {
            key: "file:example".into(),
            runtime_id: "runtime:example".into(),
            path: "src/example.ts".into(),
            previous_path: Some("src/example-old.ts".into()),
            change_kind: FileChangeKind::Modified,
            language: Some("typescript".into()),
            stats: FileStats {
                additions: 2,
                deletions: 1,
                truncated: false,
            },
            flags: FileFlags::default(),
            patch: String::new(),
            split_row_count: 4,
            stack_row_count: 5,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -1,3 +1,4 @@".into(),
                context: None,
                old_start: 1,
                old_count: 3,
                new_start: 1,
                new_count: 4,
                split_row_start: 0,
                split_row_count: 4,
                stack_row_start: 0,
                stack_row_count: 5,
                lines: vec![
                    line(DiffLineKind::Deletion, Some(1), None),
                    line(DiffLineKind::Addition, None, Some(1)),
                    line(DiffLineKind::Context, Some(2), Some(2)),
                    line(DiffLineKind::Deletion, Some(3), None),
                    line(DiffLineKind::Addition, None, Some(3)),
                ],
            }],
            content_identity: "content".into(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        }
    }

    fn line(kind: DiffLineKind, old_line: Option<u32>, new_line: Option<u32>) -> DiffLine {
        DiffLine {
            kind,
            content: String::new(),
            old_line,
            new_line,
            moved: false,
            no_newline_at_eof: false,
        }
    }

    fn input() -> CommentTargetInput {
        CommentTargetInput {
            file_path: "src/example.ts".into(),
            hunk_index: None,
            side: Some(ReviewSide::New),
            line: Some(3),
            summary: "Note".into(),
            rationale: Some("Why this matters".into()),
            markup: Some("<box border>shape</box>".into()),
            author: Some("Pi".into()),
        }
    }

    #[test]
    fn finds_files_by_current_or_previous_path_and_lines_by_hunk_extent() {
        let file = example_file();
        assert!(find_diff_file_by_path(std::slice::from_ref(&file), "src/example.ts").is_some());
        assert!(
            find_diff_file_by_path(std::slice::from_ref(&file), "src/example-old.ts").is_some()
        );
        assert!(find_diff_file_by_path(std::slice::from_ref(&file), "missing.ts").is_none());
        assert_eq!(find_hunk_index_for_line(&file, ReviewSide::Old, 1), Some(0));
        assert_eq!(find_hunk_index_for_line(&file, ReviewSide::New, 4), Some(0));
        assert_eq!(find_hunk_index_for_line(&file, ReviewSide::New, 40), None);
    }

    #[test]
    fn resolves_hunk_wide_and_line_specific_targets() {
        let file = example_file();
        let mut hunk_input = input();
        hunk_input.hunk_index = Some(0);
        hunk_input.side = None;
        hunk_input.line = None;
        assert_eq!(
            resolve_comment_target(&file, &hunk_input).unwrap(),
            ResolvedCommentTarget {
                hunk_index: 0,
                side: ReviewSide::New,
                line: 1
            }
        );
        assert_eq!(
            resolve_comment_target(&file, &input()).unwrap(),
            ResolvedCommentTarget {
                hunk_index: 0,
                side: ReviewSide::New,
                line: 3
            }
        );
    }

    #[test]
    fn builds_a_live_mcp_annotation_with_markup_and_stable_ranges() {
        let file = example_file();
        let input = input();
        let target = resolve_comment_target(&file, &input).unwrap();
        let comment = build_live_comment(
            &file,
            input,
            "comment-1".into(),
            "2026-03-22T00:00:00.000Z".into(),
            target,
        );
        assert_eq!(comment.source, "mcp");
        assert_eq!(comment.author.as_deref(), Some("Pi"));
        assert_eq!(comment.file_path.as_deref(), Some("src/example.ts"));
        assert_eq!(comment.hunk_index, Some(0));
        assert_eq!(comment.side, Some(ReviewSide::New));
        assert_eq!(comment.line, Some(3));
        assert_eq!(comment.markup.as_deref(), Some("<box border>shape</box>"));
        assert_eq!(
            comment.anchor.new_range,
            Some(LineRange { start: 3, end: 3 })
        );
        assert_eq!(comment.tags, ["mcp"]);
        assert_eq!(comment.confidence, Some(AgentAnnotationConfidence::High));
    }
}
