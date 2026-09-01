//! Stable collapsed-gap addressing shared by renderers and note validation.

use workdeck_core::{DiffFile, FileChangeKind, LineRange, ReviewSide};

use crate::normalized_review_source_lines;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewGapPosition {
    Before,
    Trailing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewGapHunk {
    pub collapsed_before: usize,
    pub addition_start: u32,
    pub addition_count: u32,
    pub deletion_start: u32,
    pub deletion_count: u32,
    pub addition_line_index: usize,
    pub deletion_line_index: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReviewGapSource {
    pub hunks: Vec<ReviewGapHunk>,
    pub addition_lines: Vec<String>,
    pub deletion_lines: Vec<String>,
    pub is_partial: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewGapId {
    pub position: ReviewGapPosition,
    pub hunk_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewGapAddress {
    pub position: ReviewGapPosition,
    pub hunk_index: usize,
    pub old_range: LineRange,
    pub new_range: LineRange,
    pub line_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewExpandedLineClaim {
    pub gap_id: String,
    pub side: ReviewSide,
    pub line: u32,
    pub source_identity: String,
}

pub fn review_gap_id(position: ReviewGapPosition, hunk_index: usize) -> String {
    let position = match position {
        ReviewGapPosition::Before => "before",
        ReviewGapPosition::Trailing => "trailing",
    };
    format!("{position}:{hunk_index}")
}

pub fn parse_review_gap_id(gap_id: &str) -> Option<ReviewGapId> {
    let (position, index) = gap_id.split_once(':')?;
    if index.is_empty() || index.contains(':') || !index.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let raw = index.parse::<u64>().ok()?;
    if raw > MAX_SAFE_INTEGER {
        return None;
    }
    Some(ReviewGapId {
        position: match position {
            "before" => ReviewGapPosition::Before,
            "trailing" => ReviewGapPosition::Trailing,
            _ => return None,
        },
        hunk_index: usize::try_from(raw).ok()?,
    })
}

/// Adapt the full source snapshots and parsed hunk geometry of a Rust review file.
pub fn review_gap_source_for_file(file: &DiffFile) -> ReviewGapSource {
    let deletion_lines = file
        .sources
        .old
        .as_ref()
        .map(|source| normalized_review_source_lines(&source.content))
        .unwrap_or_default();
    let addition_lines = file
        .sources
        .new
        .as_ref()
        .map(|source| normalized_review_source_lines(&source.content))
        .unwrap_or_default();
    let mut old_cursor = 1_u32;
    let mut new_cursor = 1_u32;
    let hunks = file
        .hunks
        .iter()
        .map(|hunk| {
            let old_end = hunk.old_start.saturating_sub(u32::from(hunk.old_count > 0));
            let new_end = hunk.new_start.saturating_sub(u32::from(hunk.new_count > 0));
            let old_gap = old_end.saturating_sub(old_cursor).saturating_add(1);
            let new_gap = new_end.saturating_sub(new_cursor).saturating_add(1);
            let collapsed_before = usize::try_from(old_gap.min(new_gap)).unwrap_or(usize::MAX);
            let projected = ReviewGapHunk {
                collapsed_before,
                addition_start: hunk.new_start,
                addition_count: hunk.new_count,
                deletion_start: hunk.old_start,
                deletion_count: hunk.old_count,
                addition_line_index: usize::try_from(hunk.new_start.saturating_sub(1))
                    .unwrap_or(usize::MAX),
                deletion_line_index: usize::try_from(hunk.old_start.saturating_sub(1))
                    .unwrap_or(usize::MAX),
            };
            old_cursor = hunk.old_start.saturating_add(hunk.old_count);
            new_cursor = hunk.new_start.saturating_add(hunk.new_count);
            projected
        })
        .collect();
    ReviewGapSource {
        hunks,
        addition_lines,
        deletion_lines,
        is_partial: file.flags.partial,
    }
}

pub fn review_leading_gap(source: &ReviewGapSource, hunk_index: usize) -> Option<ReviewGapAddress> {
    let hunk = source.hunks.get(hunk_index)?;
    if hunk.collapsed_before == 0 {
        return None;
    }
    let old_end = hunk
        .deletion_start
        .checked_sub(u32::from(hunk.deletion_count > 0))?;
    let new_end = hunk
        .addition_start
        .checked_sub(u32::from(hunk.addition_count > 0))?;
    let line_count = u32::try_from(hunk.collapsed_before).ok()?;
    let old_start = old_end.checked_sub(line_count)?.checked_add(1)?;
    let new_start = new_end.checked_sub(line_count)?.checked_add(1)?;
    if old_start == 0 || new_start == 0 {
        return None;
    }
    Some(ReviewGapAddress {
        position: ReviewGapPosition::Before,
        hunk_index,
        old_range: LineRange {
            start: old_start,
            end: old_end,
        },
        new_range: LineRange {
            start: new_start,
            end: new_end,
        },
        line_count: hunk.collapsed_before,
    })
}

pub fn review_trailing_gap(source: &ReviewGapSource) -> Option<ReviewGapAddress> {
    let hunk_index = source.hunks.len().checked_sub(1)?;
    let hunk = source.hunks.get(hunk_index)?;
    if source.is_partial {
        return None;
    }
    let old_used = hunk
        .deletion_line_index
        .checked_add(usize::try_from(hunk.deletion_count).ok()?)?;
    let new_used = hunk
        .addition_line_index
        .checked_add(usize::try_from(hunk.addition_count).ok()?)?;
    let old_count = source.deletion_lines.len().checked_sub(old_used)?;
    let new_count = source.addition_lines.len().checked_sub(new_used)?;
    if old_count == 0 || old_count != new_count {
        return None;
    }
    let line_count = u32::try_from(old_count).ok()?;
    let old_start = hunk.deletion_start.checked_add(hunk.deletion_count)?;
    let new_start = hunk.addition_start.checked_add(hunk.addition_count)?;
    Some(ReviewGapAddress {
        position: ReviewGapPosition::Trailing,
        hunk_index,
        old_range: LineRange {
            start: old_start,
            end: old_start.checked_add(line_count)?.checked_sub(1)?,
        },
        new_range: LineRange {
            start: new_start,
            end: new_start.checked_add(line_count)?.checked_sub(1)?,
        },
        line_count: old_count,
    })
}

pub fn review_gap_address(source: &ReviewGapSource, gap_id: &str) -> Option<ReviewGapAddress> {
    let parsed = parse_review_gap_id(gap_id)?;
    match parsed.position {
        ReviewGapPosition::Before => review_leading_gap(source, parsed.hunk_index),
        ReviewGapPosition::Trailing => {
            review_trailing_gap(source).filter(|address| address.hunk_index == parsed.hunk_index)
        }
    }
}

pub fn resolve_review_expanded_line(
    file: &DiffFile,
    claim: &ReviewExpandedLineClaim,
) -> Option<ReviewGapAddress> {
    if file.source_identity.as_deref() != Some(&claim.source_identity) {
        return None;
    }
    let address = review_gap_address(&review_gap_source_for_file(file), &claim.gap_id)?;
    let range = match claim.side {
        ReviewSide::Old => address.old_range,
        ReviewSide::New => address.new_range,
    };
    (range.start <= claim.line && claim.line <= range.end).then_some(address)
}

pub fn review_expansion_side(change_kind: FileChangeKind) -> ReviewSide {
    if change_kind == FileChangeKind::Deleted {
        ReviewSide::Old
    } else {
        ReviewSide::New
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hunk(
        collapsed_before: usize,
        addition_start: u32,
        addition_count: u32,
        deletion_start: u32,
        deletion_count: u32,
    ) -> ReviewGapHunk {
        ReviewGapHunk {
            collapsed_before,
            addition_start,
            addition_count,
            deletion_start,
            deletion_count,
            addition_line_index: usize::try_from(addition_start - 1).unwrap(),
            deletion_line_index: usize::try_from(deletion_start - 1).unwrap(),
        }
    }

    fn source(hunks: Vec<ReviewGapHunk>, old: usize, new: usize) -> ReviewGapSource {
        ReviewGapSource {
            hunks,
            deletion_lines: (1..=old).map(|line| format!("old {line}")).collect(),
            addition_lines: (1..=new).map(|line| format!("new {line}")).collect(),
            is_partial: false,
        }
    }

    #[test]
    fn gap_ids_round_trip_and_reject_malformed_addresses() {
        assert_eq!(review_gap_id(ReviewGapPosition::Before, 3), "before:3");
        assert_eq!(
            parse_review_gap_id("trailing:0"),
            Some(ReviewGapId {
                position: ReviewGapPosition::Trailing,
                hunk_index: 0
            })
        );
        for invalid in [
            "",
            "before",
            "before:",
            "before:-1",
            "middle:1",
            "before:1:2",
        ] {
            assert_eq!(parse_review_gap_id(invalid), None);
        }
    }

    #[test]
    fn leading_gap_handles_regular_insert_and_delete_boundaries() {
        let regular = source(vec![hunk(5, 6, 1, 6, 1)], 12, 12);
        let gap = review_leading_gap(&regular, 0).unwrap();
        assert_eq!(gap.old_range, LineRange { start: 1, end: 5 });
        assert_eq!(gap.new_range, LineRange { start: 1, end: 5 });

        let insertion = source(vec![hunk(6, 7, 1, 6, 0)], 12, 13);
        let gap = review_leading_gap(&insertion, 0).unwrap();
        assert_eq!(gap.old_range, LineRange { start: 1, end: 6 });
        assert_eq!(gap.new_range, LineRange { start: 1, end: 6 });

        let deletion = source(vec![hunk(1, 2, 0, 3, 1)], 12, 11);
        let gap = review_leading_gap(&deletion, 0).unwrap();
        assert_eq!(gap.old_range, LineRange { start: 2, end: 2 });
        assert_eq!(gap.new_range, LineRange { start: 2, end: 2 });
        assert!(review_leading_gap(&source(vec![hunk(0, 1, 1, 1, 1)], 3, 3), 0).is_none());
        assert!(review_leading_gap(&source(vec![hunk(9, 3, 1, 3, 1)], 12, 12), 0).is_none());
    }

    #[test]
    fn trailing_gap_requires_equal_authoritative_tails() {
        let regular = source(vec![hunk(9, 10, 1, 10, 1)], 12, 12);
        let gap = review_trailing_gap(&regular).unwrap();
        assert_eq!(gap.old_range, LineRange { start: 11, end: 12 });
        assert_eq!(gap.new_range, LineRange { start: 11, end: 12 });
        assert!(review_trailing_gap(&source(vec![hunk(11, 12, 1, 12, 1)], 12, 12)).is_none());
        assert!(review_trailing_gap(&source(vec![hunk(6, 7, 1, 6, 0)], 12, 13)).is_none());
        let mut partial = regular;
        partial.is_partial = true;
        assert!(review_trailing_gap(&partial).is_none());
    }

    #[test]
    fn resolves_only_real_gap_ids_and_selects_deleted_source_side() {
        let source = source(vec![hunk(2, 3, 1, 3, 1), hunk(5, 9, 1, 9, 1)], 12, 12);
        assert_eq!(
            review_gap_address(&source, "before:1").unwrap().old_range,
            LineRange { start: 4, end: 8 }
        );
        assert_eq!(
            review_gap_address(&source, "trailing:1").unwrap().old_range,
            LineRange { start: 10, end: 12 }
        );
        assert!(review_gap_address(&source, "trailing:0").is_none());
        assert!(review_gap_address(&source, "before:9").is_none());
        assert_eq!(
            review_expansion_side(FileChangeKind::Deleted),
            ReviewSide::Old
        );
        assert_eq!(
            review_expansion_side(FileChangeKind::Modified),
            ReviewSide::New
        );
    }
}
