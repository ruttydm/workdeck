//! Stable ownership and intersection semantics for review-note anchors.

use workdeck_core::{DiffHunk, LineRange, ReviewSide};

use crate::{review_hunk_range, review_ranges_overlap};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewPreferredLine {
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewLineTarget {
    pub hunk_index: usize,
    pub side: ReviewSide,
    pub line: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewNoteAnchorInput {
    pub old_range: Option<LineRange>,
    pub new_range: Option<LineRange>,
    pub preferred: Option<ReviewPreferredLine>,
    pub fallback_owner_hunk_index: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReviewNoteAnchor {
    pub old_range: Option<LineRange>,
    pub new_range: Option<LineRange>,
    pub preferred: Option<ReviewPreferredLine>,
    pub intersecting_hunk_indices: Vec<usize>,
    pub owner_hunk_index: Option<usize>,
}

/// Return the hunk owning an address in a collapsed source gap.
pub fn review_gap_owner_hunk_index(
    hunks: &[DiffHunk],
    side: ReviewSide,
    line: u32,
) -> Option<usize> {
    if hunks.is_empty() {
        return None;
    }
    Some(
        hunks
            .iter()
            .position(|hunk| review_hunk_range(hunk, side).start > line)
            .unwrap_or(hunks.len() - 1),
    )
}

/// Resolve one note's owner hunk and complete intersection set.
pub fn resolve_review_note_anchor(
    hunks: &[DiffHunk],
    input: ReviewNoteAnchorInput,
) -> ResolvedReviewNoteAnchor {
    let intersecting_hunk_indices = hunks
        .iter()
        .enumerate()
        .filter_map(|(index, hunk)| {
            let intersects = input.old_range.is_some_and(|range| {
                review_ranges_overlap(range, review_hunk_range(hunk, ReviewSide::Old))
            }) || input.new_range.is_some_and(|range| {
                review_ranges_overlap(range, review_hunk_range(hunk, ReviewSide::New))
            });
            intersects.then_some(index)
        })
        .collect::<Vec<_>>();

    let preferred_owner = input.preferred.and_then(|preferred| {
        hunks.iter().position(|hunk| {
            let range = review_hunk_range(hunk, preferred.side);
            range.start <= preferred.line && preferred.line <= range.end
        })
    });
    let validated_fallback = input
        .fallback_owner_hunk_index
        .filter(|index| hunks.get(*index).is_some());
    let owner_hunk_index = preferred_owner
        .or_else(|| intersecting_hunk_indices.first().copied())
        .or(validated_fallback)
        .or_else(|| (!hunks.is_empty()).then_some(0));

    ResolvedReviewNoteAnchor {
        old_range: input.old_range,
        new_range: input.new_range,
        preferred: input.preferred,
        intersecting_hunk_indices,
        owner_hunk_index,
    }
}

/// Anchor one note to one source line, keeping its declared hunk as the owner
/// when the line came from an expanded gap.
pub fn review_line_anchor(
    hunks: &[DiffHunk],
    target: ReviewLineTarget,
) -> ResolvedReviewNoteAnchor {
    let range = LineRange {
        start: target.line,
        end: target.line,
    };
    resolve_review_note_anchor(
        hunks,
        ReviewNoteAnchorInput {
            old_range: (target.side == ReviewSide::Old).then_some(range),
            new_range: (target.side == ReviewSide::New).then_some(range),
            preferred: Some(ReviewPreferredLine {
                side: target.side,
                line: target.line,
            }),
            fallback_owner_hunk_index: Some(target.hunk_index),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn separated_hunks() -> Vec<DiffHunk> {
        vec![hunk(1, 3), hunk(20, 3), hunk(40, 3)]
    }

    #[test]
    fn lists_every_intersection_and_prefers_the_explicit_line_owner() {
        let hunks = separated_hunks();
        let wide = resolve_review_note_anchor(
            &hunks,
            ReviewNoteAnchorInput {
                new_range: Some(LineRange { start: 2, end: 41 }),
                ..Default::default()
            },
        );
        assert_eq!(wide.intersecting_hunk_indices, [0, 1, 2]);
        assert_eq!(wide.owner_hunk_index, Some(0));

        let preferred = resolve_review_note_anchor(
            &hunks,
            ReviewNoteAnchorInput {
                new_range: Some(LineRange { start: 2, end: 41 }),
                preferred: Some(ReviewPreferredLine {
                    side: ReviewSide::New,
                    line: 40,
                }),
                ..Default::default()
            },
        );
        assert_eq!(preferred.owner_hunk_index, Some(2));
    }

    #[test]
    fn validates_fallbacks_and_owns_nothing_without_hunks() {
        let hunks = separated_hunks();
        let gap = resolve_review_note_anchor(
            &hunks,
            ReviewNoteAnchorInput {
                new_range: Some(LineRange { start: 10, end: 10 }),
                preferred: Some(ReviewPreferredLine {
                    side: ReviewSide::New,
                    line: 10,
                }),
                fallback_owner_hunk_index: Some(1),
                ..Default::default()
            },
        );
        assert!(gap.intersecting_hunk_indices.is_empty());
        assert_eq!(gap.owner_hunk_index, Some(1));

        let invalid = resolve_review_note_anchor(
            &hunks,
            ReviewNoteAnchorInput {
                new_range: Some(LineRange { start: 10, end: 10 }),
                fallback_owner_hunk_index: Some(99),
                ..Default::default()
            },
        );
        assert_eq!(invalid.owner_hunk_index, Some(0));
        assert_eq!(
            resolve_review_note_anchor(
                &[],
                ReviewNoteAnchorInput {
                    new_range: Some(LineRange { start: 1, end: 1 }),
                    ..Default::default()
                }
            )
            .owner_hunk_index,
            None
        );
    }

    #[test]
    fn gap_owner_follows_source_geometry_on_each_side() {
        let hunks = separated_hunks();
        assert_eq!(
            review_gap_owner_hunk_index(&hunks, ReviewSide::New, 10),
            Some(1)
        );
        assert_eq!(
            review_gap_owner_hunk_index(&hunks, ReviewSide::New, 30),
            Some(2)
        );
        assert_eq!(
            review_gap_owner_hunk_index(&hunks, ReviewSide::New, 500),
            Some(2)
        );
        assert_eq!(review_gap_owner_hunk_index(&[], ReviewSide::New, 1), None);

        let mut shifted = vec![hunk(1, 1), hunk(30, 1)];
        shifted[1].old_start = 10;
        assert_eq!(
            review_gap_owner_hunk_index(&shifted, ReviewSide::New, 20),
            Some(1)
        );
        assert_eq!(
            review_gap_owner_hunk_index(&shifted, ReviewSide::Old, 5),
            Some(1)
        );
    }

    #[test]
    fn line_anchor_keeps_expanded_gaps_and_side_specific_ranges() {
        let hunks = separated_hunks();
        let inside = review_line_anchor(
            &hunks,
            ReviewLineTarget {
                hunk_index: 1,
                side: ReviewSide::New,
                line: 21,
            },
        );
        assert_eq!(inside.intersecting_hunk_indices, [1]);
        assert_eq!(inside.owner_hunk_index, Some(1));

        let gap = review_line_anchor(
            &hunks,
            ReviewLineTarget {
                hunk_index: 2,
                side: ReviewSide::New,
                line: 30,
            },
        );
        assert!(gap.intersecting_hunk_indices.is_empty());
        assert_eq!(gap.owner_hunk_index, Some(2));

        let old = review_line_anchor(
            &hunks,
            ReviewLineTarget {
                hunk_index: 0,
                side: ReviewSide::Old,
                line: 2,
            },
        );
        assert_eq!(old.old_range, Some(LineRange { start: 2, end: 2 }));
        assert_eq!(old.new_range, None);
    }
}
