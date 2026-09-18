//! Provider-neutral review geometry shared by renderers, sessions, and extensions.

use workdeck_core::{DiffHunk, DiffLineKind, LineRange, ReviewSide};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HunkSummary {
    pub index: usize,
    pub header: String,
    pub old_range: LineRange,
    pub new_range: LineRange,
}

/// Produce the single-line, inclusive-range hunk record shared by sessions and extensions.
pub fn summarize_hunk(hunk: &DiffHunk, index: usize) -> HunkSummary {
    let mut header = String::with_capacity(hunk.header.len());
    let mut in_newline_run = false;
    for character in hunk.formatted_header().chars() {
        if matches!(character, '\r' | '\n') {
            if !in_newline_run {
                header.push(' ');
                in_newline_run = true;
            }
        } else {
            header.push(character);
            in_newline_run = false;
        }
    }
    HunkSummary {
        index,
        header: header.trim_end().to_owned(),
        old_range: review_hunk_range(hunk, ReviewSide::Old),
        new_range: review_hunk_range(hunk, ReviewSide::New),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewLineAddress {
    pub side: ReviewSide,
    pub line: u32,
}

/// The inclusive extent occupied by one hunk on a source side.
pub fn review_hunk_range(hunk: &DiffHunk, side: ReviewSide) -> LineRange {
    let (start, count) = match side {
        ReviewSide::Old => (hunk.old_start, hunk.old_count),
        ReviewSide::New => (hunk.new_start, hunk.new_count),
    };
    LineRange {
        start,
        end: start.saturating_add(count.max(1)).saturating_sub(1),
    }
}

pub fn review_hunk_ranges(hunk: &DiffHunk) -> (LineRange, LineRange) {
    (
        review_hunk_range(hunk, ReviewSide::Old),
        review_hunk_range(hunk, ReviewSide::New),
    )
}

pub fn review_ranges_overlap(left: LineRange, right: LineRange) -> bool {
    left.start <= right.end && right.start <= left.end
}

pub fn review_hunk_index_for_line(
    hunks: &[DiffHunk],
    side: ReviewSide,
    line: u32,
) -> Option<usize> {
    hunks.iter().position(|hunk| {
        let range = review_hunk_range(hunk, side);
        range.start <= line && line <= range.end
    })
}

/// Prefer the first added row, then the first deleted row, then the new-side hunk position.
pub fn review_default_hunk_line_target(hunk: &DiffHunk) -> ReviewLineAddress {
    if let Some(line) = hunk
        .lines
        .iter()
        .find(|line| line.kind == DiffLineKind::Addition)
        .and_then(|line| line.new_line)
    {
        return ReviewLineAddress {
            side: ReviewSide::New,
            line,
        };
    }
    if let Some(line) = hunk
        .lines
        .iter()
        .find(|line| line.kind == DiffLineKind::Deletion)
        .and_then(|line| line.old_line)
    {
        return ReviewLineAddress {
            side: ReviewSide::Old,
            line,
        };
    }
    ReviewLineAddress {
        side: ReviewSide::New,
        line: review_hunk_range(hunk, ReviewSide::New).start,
    }
}

/// Return the hunk position on a side that actually contains rows.
pub fn review_canonical_hunk_line(
    hunk: &DiffHunk,
    preferred_side: ReviewSide,
) -> Option<ReviewLineAddress> {
    let fallback = match preferred_side {
        ReviewSide::Old => ReviewSide::New,
        ReviewSide::New => ReviewSide::Old,
    };
    [preferred_side, fallback].into_iter().find_map(|side| {
        let (start, count) = match side {
            ReviewSide::Old => (hunk.old_start, hunk.old_count),
            ReviewSide::New => (hunk.new_start, hunk.new_count),
        };
        (count > 0).then_some(ReviewLineAddress { side, line: start })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewHunkContentKind {
    Context,
    Change,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewHunkContentBlock {
    pub kind: ReviewHunkContentKind,
    pub lines: usize,
    pub additions: usize,
    pub deletions: usize,
    pub deletion_line_index: usize,
    pub addition_line_index: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebasableReviewHunk {
    pub deletion_line_index: usize,
    pub addition_line_index: usize,
    pub content: Vec<ReviewHunkContentBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RebasedReviewHunk {
    pub hunk: RebasableReviewHunk,
    pub deletion_end_index: usize,
    pub addition_end_index: usize,
}

/// Lay a hunk's blocks out contiguously from new zero-based source origins.
pub fn rebase_review_hunk(
    hunk: &RebasableReviewHunk,
    deletion_origin: usize,
    addition_origin: usize,
) -> RebasedReviewHunk {
    let mut deletion_line_index = deletion_origin;
    let mut addition_line_index = addition_origin;
    let content = hunk
        .content
        .iter()
        .cloned()
        .map(|mut block| {
            block.deletion_line_index = deletion_line_index;
            block.addition_line_index = addition_line_index;
            match block.kind {
                ReviewHunkContentKind::Context => {
                    deletion_line_index = deletion_line_index.saturating_add(block.lines);
                    addition_line_index = addition_line_index.saturating_add(block.lines);
                }
                ReviewHunkContentKind::Change => {
                    deletion_line_index = deletion_line_index.saturating_add(block.deletions);
                    addition_line_index = addition_line_index.saturating_add(block.additions);
                }
            }
            block
        })
        .collect();
    RebasedReviewHunk {
        hunk: RebasableReviewHunk {
            deletion_line_index: deletion_origin,
            addition_line_index: addition_origin,
            content,
        },
        deletion_end_index: deletion_line_index,
        addition_end_index: addition_line_index,
    }
}

/// Split source into addressable rows, normalizing CRLF and dropping one terminator newline.
pub fn normalized_review_source_lines(source: &str) -> Vec<String> {
    let normalized = source.replace("\r\n", "\n");
    let trimmed = normalized.strip_suffix('\n').unwrap_or(&normalized);
    if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('\n').map(str::to_owned).collect()
    }
}

/// Exact length of `normalized_review_source_lines`, without allocating its text.
pub fn normalized_review_source_line_count(source: &str) -> usize {
    if matches!(source, "" | "\n" | "\r\n") {
        0
    } else {
        source.bytes().filter(|byte| *byte == b'\n').count() + usize::from(!source.ends_with('\n'))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::DiffLine;

    #[test]
    fn source_line_count_matches_normalization_exhaustively() {
        let mut inputs = vec![String::new()];
        for _ in 0..6 {
            let next = inputs
                .iter()
                .flat_map(|prefix| {
                    ['a', '\r', '\n', '日'].map(|suffix| format!("{prefix}{suffix}"))
                })
                .collect::<Vec<_>>();
            for source in &inputs {
                assert_eq!(
                    normalized_review_source_line_count(source),
                    normalized_review_source_lines(source).len(),
                    "{source:?}"
                );
            }
            inputs = next;
        }
        for source in &inputs {
            assert_eq!(
                normalized_review_source_line_count(source),
                normalized_review_source_lines(source).len(),
                "{source:?}"
            );
        }
    }

    fn hunk(start: u32, count: u32) -> DiffHunk {
        DiffHunk {
            index: 0,
            header: String::new(),
            context: None,
            old_start: start,
            old_count: count,
            new_start: start,
            new_count: count,
            split_row_start: 0,
            split_row_count: 0,
            stack_row_start: 0,
            stack_row_count: 0,
            lines: Vec::new(),
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

    #[test]
    fn hunk_ranges_cover_headers_and_zero_count_positions() {
        assert_eq!(
            review_hunk_range(&hunk(3, 7), ReviewSide::New),
            LineRange { start: 3, end: 9 }
        );
        let insertion = DiffHunk {
            old_start: 6,
            old_count: 0,
            new_start: 7,
            new_count: 1,
            ..hunk(1, 0)
        };
        assert_eq!(
            review_hunk_range(&insertion, ReviewSide::Old),
            LineRange { start: 6, end: 6 }
        );
        assert_eq!(
            review_hunk_ranges(&insertion),
            (
                LineRange { start: 6, end: 6 },
                LineRange { start: 7, end: 7 }
            )
        );
    }

    #[test]
    fn finds_hunks_by_extent_and_inclusive_overlaps() {
        let hunks = [hunk(1, 3), hunk(20, 1)];
        assert_eq!(
            review_hunk_index_for_line(&hunks, ReviewSide::New, 3),
            Some(0)
        );
        assert_eq!(
            review_hunk_index_for_line(&hunks, ReviewSide::New, 20),
            Some(1)
        );
        assert_eq!(
            review_hunk_index_for_line(&hunks, ReviewSide::Old, 10),
            None
        );
        assert!(review_ranges_overlap(
            LineRange { start: 1, end: 3 },
            LineRange { start: 3, end: 5 }
        ));
        assert!(!review_ranges_overlap(
            LineRange { start: 1, end: 3 },
            LineRange { start: 4, end: 5 }
        ));
    }

    #[test]
    fn default_target_prefers_any_addition_then_the_first_deletion() {
        let mut changed = hunk(10, 4);
        changed.lines = vec![
            line(DiffLineKind::Deletion, Some(10), None),
            line(DiffLineKind::Context, Some(11), Some(10)),
            line(DiffLineKind::Context, Some(12), Some(11)),
            line(DiffLineKind::Addition, None, Some(12)),
        ];
        assert_eq!(
            review_default_hunk_line_target(&changed),
            ReviewLineAddress {
                side: ReviewSide::New,
                line: 12
            }
        );
        changed.lines.pop();
        assert_eq!(
            review_default_hunk_line_target(&changed),
            ReviewLineAddress {
                side: ReviewSide::Old,
                line: 10
            }
        );
    }

    #[test]
    fn canonical_target_uses_only_sides_backed_by_rows() {
        let deletion = DiffHunk {
            old_start: 6,
            old_count: 1,
            new_start: 5,
            new_count: 0,
            ..hunk(1, 0)
        };
        assert_eq!(
            review_canonical_hunk_line(&deletion, ReviewSide::New),
            Some(ReviewLineAddress {
                side: ReviewSide::Old,
                line: 6
            })
        );
        assert_eq!(
            review_canonical_hunk_line(&hunk(1, 0), ReviewSide::New),
            None
        );
    }

    #[test]
    fn rebases_blocks_contiguously_and_preserves_the_input() {
        let source = RebasableReviewHunk {
            deletion_line_index: 4,
            addition_line_index: 4,
            content: vec![
                ReviewHunkContentBlock {
                    kind: ReviewHunkContentKind::Context,
                    lines: 2,
                    additions: 0,
                    deletions: 0,
                    deletion_line_index: 4,
                    addition_line_index: 4,
                },
                ReviewHunkContentBlock {
                    kind: ReviewHunkContentKind::Change,
                    lines: 0,
                    additions: 2,
                    deletions: 1,
                    deletion_line_index: 6,
                    addition_line_index: 6,
                },
            ],
        };
        let rebased = rebase_review_hunk(&source, 10, 20);
        assert_eq!(rebased.hunk.deletion_line_index, 10);
        assert_eq!(rebased.hunk.addition_line_index, 20);
        assert_eq!(rebased.hunk.content[1].deletion_line_index, 12);
        assert_eq!(rebased.hunk.content[1].addition_line_index, 22);
        assert_eq!(rebased.deletion_end_index, 13);
        assert_eq!(rebased.addition_end_index, 24);
        assert_eq!(source.content[0].deletion_line_index, 4);
    }

    #[test]
    fn normalizes_source_rows_without_a_phantom_final_line() {
        assert_eq!(
            normalized_review_source_lines("one\ntwo\nthree\n"),
            ["one", "two", "three"]
        );
        assert_eq!(
            normalized_review_source_lines("one\r\ntwo\r\n"),
            ["one", "two"]
        );
        assert_eq!(normalized_review_source_lines("one\n\n"), ["one", ""]);
        assert!(normalized_review_source_lines("").is_empty());
        assert!(normalized_review_source_lines("\n").is_empty());
    }

    #[test]
    fn summarizes_hunks_with_single_line_headers_and_inclusive_ranges() {
        let mut summary_hunk = hunk(10, 2);
        summary_hunk.new_start = 20;
        summary_hunk.new_count = 3;
        summary_hunk.header = "@@ -10,2 +20,3 @@\r\nfunction name\n\t".into();
        assert_eq!(
            summarize_hunk(&summary_hunk, 7),
            HunkSummary {
                index: 7,
                header: "@@ -10,2 +20,3 @@ function name".into(),
                old_range: LineRange { start: 10, end: 11 },
                new_range: LineRange { start: 20, end: 22 },
            }
        );
    }
}
