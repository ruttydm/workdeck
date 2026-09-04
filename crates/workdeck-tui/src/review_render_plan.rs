//! Ordered presentational planning for one file's diff rows and inline notes.
//!
//! This is a Rust reimplementation of Hunk's `src/ui/diff/reviewRenderPlan.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.

use std::collections::{BTreeSet, HashMap, HashSet};

use workdeck_core::{ReviewGapPosition, ReviewSide};
use workdeck_diff::{DiffRow, SplitLineKind, StackLineKind};
use workdeck_review::{ReviewLineTarget, ReviewPreferredLine};

use crate::{PlannedReviewRow, VisibleAgentNote, diff_hunk_id};

pub const DEFAULT_HUNK_GAP: usize = 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextLineStableKeySides {
    pub hunk_index: usize,
    pub old_line: u32,
    pub new_line: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct ReviewRenderPlanOptions<'a> {
    pub file_id: &'a str,
    pub rows: &'a [DiffRow],
    pub show_hunk_headers: bool,
    pub visible_agent_notes: &'a [VisibleAgentNote],
    pub selected_hunk_index: Option<usize>,
    pub hunk_gap: usize,
}

impl<'a> ReviewRenderPlanOptions<'a> {
    #[must_use]
    pub const fn new(file_id: &'a str, rows: &'a [DiffRow], show_hunk_headers: bool) -> Self {
        Self {
            file_id,
            rows,
            show_hunk_headers,
            visible_agent_notes: &[],
            selected_hunk_index: None,
            hunk_gap: DEFAULT_HUNK_GAP,
        }
    }
}

#[derive(Debug)]
struct InlineVisibleNotePlacement<'a> {
    anchor_side: Option<ReviewSide>,
    guided_row_keys: BTreeSet<String>,
    hunk_index: usize,
    note: &'a VisibleAgentNote,
    note_count: usize,
    note_index: usize,
}

fn is_line_row(row: &DiffRow) -> bool {
    matches!(row, DiffRow::SplitLine { .. } | DiffRow::StackLine { .. })
}

fn unique_stable_keys(keys: impl IntoIterator<Item = Option<String>>) -> Vec<String> {
    let mut next = Vec::new();
    let mut seen = HashSet::new();
    for key in keys.into_iter().flatten() {
        if !key.is_empty() && seen.insert(key.clone()) {
            next.push(key);
        }
    }
    next
}

#[must_use]
pub fn line_stable_key(hunk_index: usize, side: ReviewSide, line_number: usize) -> String {
    let side = match side {
        ReviewSide::Old => "old",
        ReviewSide::New => "new",
    };
    format!("line:{hunk_index}:{side}:{line_number}")
}

#[must_use]
pub fn inline_note_stable_key(note_id: &str) -> String {
    format!("inline-note:{note_id}")
}

fn old_line_stable_key(hunk_index: usize, line_number: Option<usize>) -> Option<String> {
    line_number.map(|line| line_stable_key(hunk_index, ReviewSide::Old, line))
}

fn new_line_stable_key(hunk_index: usize, line_number: Option<usize>) -> Option<String> {
    line_number.map(|line| line_stable_key(hunk_index, ReviewSide::New, line))
}

fn context_line_stable_key(
    hunk_index: usize,
    old_line_number: Option<usize>,
    new_line_number: Option<usize>,
) -> Option<String> {
    Some(format!(
        "line:{hunk_index}:context:{}:{}",
        old_line_number?, new_line_number?
    ))
}

fn parse_ascii_number<T: std::str::FromStr>(text: &str) -> Option<T> {
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

#[must_use]
pub fn line_stable_key_target(stable_key: &str) -> Option<ReviewLineTarget> {
    let mut parts = stable_key.split(':');
    if parts.next()? != "line" {
        return None;
    }
    let hunk_index = parse_ascii_number(parts.next()?)?;
    let side = match parts.next()? {
        "old" => ReviewSide::Old,
        "new" => ReviewSide::New,
        _ => return None,
    };
    let line = parse_ascii_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(ReviewLineTarget {
        hunk_index,
        side,
        line,
    })
}

#[must_use]
pub fn context_line_stable_key_target(stable_key: &str) -> Option<ReviewLineTarget> {
    let sides = context_line_stable_key_sides(stable_key)?;
    Some(ReviewLineTarget {
        hunk_index: sides.hunk_index,
        side: ReviewSide::New,
        line: sides.new_line,
    })
}

#[must_use]
pub fn context_line_stable_key_sides(stable_key: &str) -> Option<ContextLineStableKeySides> {
    let mut parts = stable_key.split(':');
    if parts.next()? != "line" {
        return None;
    }
    let hunk_index = parse_ascii_number(parts.next()?)?;
    if parts.next()? != "context" {
        return None;
    }
    let old_line = parse_ascii_number(parts.next()?)?;
    let new_line = parse_ascii_number(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(ContextLineStableKeySides {
        hunk_index,
        old_line,
        new_line,
    })
}

fn diff_row_stable_keys(row: &DiffRow) -> Vec<String> {
    match row {
        DiffRow::Collapsed {
            hunk_index,
            position,
            ..
        } => vec![format!(
            "meta:collapsed:{}:{hunk_index}",
            match position {
                ReviewGapPosition::Before => "before",
                ReviewGapPosition::Trailing => "trailing",
            }
        )],
        DiffRow::HunkHeader { hunk_index, .. } => {
            vec![format!("meta:hunk-header:{hunk_index}")]
        }
        DiffRow::SplitLine {
            hunk_index,
            left,
            right,
            ..
        } => {
            let context = context_line_stable_key(*hunk_index, left.line_number, right.line_number);
            if left.kind == SplitLineKind::Context && right.kind == SplitLineKind::Context {
                unique_stable_keys([
                    context,
                    new_line_stable_key(*hunk_index, right.line_number),
                    old_line_stable_key(*hunk_index, left.line_number),
                ])
            } else {
                unique_stable_keys([
                    old_line_stable_key(*hunk_index, left.line_number),
                    new_line_stable_key(*hunk_index, right.line_number),
                ])
            }
        }
        DiffRow::StackLine {
            hunk_index, cell, ..
        } => {
            let context =
                context_line_stable_key(*hunk_index, cell.old_line_number, cell.new_line_number);
            if cell.kind == StackLineKind::Context {
                unique_stable_keys([
                    context,
                    new_line_stable_key(*hunk_index, cell.new_line_number),
                    old_line_stable_key(*hunk_index, cell.old_line_number),
                ])
            } else {
                unique_stable_keys([
                    new_line_stable_key(*hunk_index, cell.new_line_number),
                    old_line_stable_key(*hunk_index, cell.old_line_number),
                ])
            }
        }
    }
}

fn row_line_number(row: &DiffRow, side: ReviewSide) -> Option<usize> {
    match row {
        DiffRow::SplitLine { left, right, .. } => match side {
            ReviewSide::Old => left.line_number,
            ReviewSide::New => right.line_number,
        },
        DiffRow::StackLine { cell, .. } => match side {
            ReviewSide::Old => cell.old_line_number,
            ReviewSide::New => cell.new_line_number,
        },
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => None,
    }
}

fn row_overlaps_note_range(row: &DiffRow, note: &VisibleAgentNote) -> bool {
    [ReviewSide::Old, ReviewSide::New].into_iter().any(|side| {
        let range = match side {
            ReviewSide::Old => note.anchor.old_range,
            ReviewSide::New => note.anchor.new_range,
        };
        let line = row_line_number(row, side).and_then(|line| u32::try_from(line).ok());
        range
            .zip(line)
            .is_some_and(|(range, line)| range.start <= line && line <= range.end)
    })
}

fn note_owner_hunk_index(note: &VisibleAgentNote) -> usize {
    note.anchor
        .owner_hunk_index
        .or_else(|| note.anchor.intersecting_hunk_indices.first().copied())
        .unwrap_or(0)
}

fn note_anchor_line(note: &VisibleAgentNote) -> ReviewPreferredLine {
    note.anchor.preferred.unwrap_or(ReviewPreferredLine {
        side: ReviewSide::New,
        line: 1,
    })
}

fn find_inline_note_anchor_row<'a>(
    rows: &'a [DiffRow],
    note: &VisibleAgentNote,
) -> Option<&'a DiffRow> {
    let owner_hunk_index = note_owner_hunk_index(note);
    let owner_rows = rows
        .iter()
        .filter(|row| is_line_row(row) && row_hunk_index(row) == owner_hunk_index)
        .collect::<Vec<_>>();
    let all_line_rows = rows
        .iter()
        .filter(|row| is_line_row(row))
        .collect::<Vec<_>>();
    let candidates = if owner_rows.is_empty() {
        &all_line_rows
    } else {
        &owner_rows
    };
    let anchor = note_anchor_line(note);
    let mut preceding = None;
    for row in candidates {
        let Some(line_number) = row_line_number(row, anchor.side) else {
            continue;
        };
        if line_number >= anchor.line as usize {
            return Some(row);
        }
        preceding = Some(*row);
    }
    preceding
        .or_else(|| candidates.first().copied())
        .or_else(|| {
            rows.iter().find(|row| {
                matches!(
                    row,
                    DiffRow::HunkHeader { hunk_index, .. } if *hunk_index == owner_hunk_index
                )
            })
        })
        .or_else(|| {
            rows.iter()
                .find(|row| matches!(row, DiffRow::HunkHeader { .. }))
        })
}

fn row_key(row: &DiffRow) -> &str {
    match row {
        DiffRow::Collapsed { key, .. }
        | DiffRow::HunkHeader { key, .. }
        | DiffRow::SplitLine { key, .. }
        | DiffRow::StackLine { key, .. } => key,
    }
}

fn row_file_id(row: &DiffRow) -> &str {
    match row {
        DiffRow::Collapsed { file_id, .. }
        | DiffRow::HunkHeader { file_id, .. }
        | DiffRow::SplitLine { file_id, .. }
        | DiffRow::StackLine { file_id, .. } => file_id,
    }
}

fn row_hunk_index(row: &DiffRow) -> usize {
    match row {
        DiffRow::Collapsed { hunk_index, .. }
        | DiffRow::HunkHeader { hunk_index, .. }
        | DiffRow::SplitLine { hunk_index, .. }
        | DiffRow::StackLine { hunk_index, .. } => *hunk_index,
    }
}

fn build_inline_visible_note_placements<'a>(
    rows: &'a [DiffRow],
    visible_agent_notes: &'a [VisibleAgentNote],
) -> Vec<(String, Vec<InlineVisibleNotePlacement<'a>>)> {
    let line_rows = rows
        .iter()
        .filter(|row| is_line_row(row))
        .collect::<Vec<_>>();
    let mut placement_groups = Vec::<(String, Vec<InlineVisibleNotePlacement<'a>>)>::new();
    for note in visible_agent_notes {
        let Some(anchor_row) = find_inline_note_anchor_row(rows, note) else {
            continue;
        };
        let guide_rows = line_rows
            .iter()
            .copied()
            .filter(|row| row_overlaps_note_range(row, note))
            .filter(|row| row_key(row) != row_key(anchor_row))
            .map(|row| row_key(row).to_owned())
            .collect::<BTreeSet<_>>();
        let anchor_key = row_key(anchor_row);
        let group_index = placement_groups
            .iter()
            .position(|(key, _)| key == anchor_key)
            .unwrap_or_else(|| {
                placement_groups.push((anchor_key.to_owned(), Vec::new()));
                placement_groups.len() - 1
            });
        placement_groups[group_index]
            .1
            .push(InlineVisibleNotePlacement {
                anchor_side: note.anchor.preferred.map(|preferred| preferred.side),
                guided_row_keys: guide_rows,
                hunk_index: row_hunk_index(anchor_row),
                note,
                note_count: 1,
                note_index: 0,
            });
    }
    for (_, placements) in &mut placement_groups {
        let count = placements.len();
        for (index, placement) in placements.iter_mut().enumerate() {
            placement.note_count = count;
            placement.note_index = index;
        }
    }
    placement_groups
}

fn build_note_guide_side_by_row_key(
    placement_groups: &[(String, Vec<InlineVisibleNotePlacement<'_>>)],
) -> HashMap<String, ReviewSide> {
    let mut guide_side_by_row_key = HashMap::new();
    for (_, placements) in placement_groups {
        for placement in placements {
            let Some(side) = placement.anchor_side else {
                continue;
            };
            for row_key in &placement.guided_row_keys {
                guide_side_by_row_key.entry(row_key.clone()).or_insert(side);
            }
        }
    }
    guide_side_by_row_key
}

fn row_can_anchor_hunk(row: &DiffRow, show_hunk_headers: bool) -> bool {
    if show_hunk_headers {
        return matches!(row, DiffRow::HunkHeader { .. });
    }
    match row {
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => false,
        DiffRow::SplitLine {
            is_expansion_row, ..
        }
        | DiffRow::StackLine {
            is_expansion_row, ..
        } => !is_expansion_row,
    }
}

#[must_use]
pub fn build_review_render_plan(options: ReviewRenderPlanOptions<'_>) -> Vec<PlannedReviewRow> {
    let ReviewRenderPlanOptions {
        file_id,
        rows,
        show_hunk_headers,
        visible_agent_notes,
        selected_hunk_index: _,
        hunk_gap,
    } = options;
    let placement_groups = build_inline_visible_note_placements(rows, visible_agent_notes);
    let guide_side_by_row_key = build_note_guide_side_by_row_key(&placement_groups);
    let mut planned_rows = Vec::new();
    let mut anchored_hunks = HashSet::new();

    for row in rows {
        let hunk_index = row_hunk_index(row);
        let should_anchor_hunk =
            row_can_anchor_hunk(row, show_hunk_headers) && !anchored_hunks.contains(&hunk_index);
        let anchor_id = should_anchor_hunk.then(|| diff_hunk_id(file_id, hunk_index));
        let stable_keys = diff_row_stable_keys(row);
        let stable_key = stable_keys
            .first()
            .cloned()
            .unwrap_or_else(|| format!("row:{}", row_key(row)));
        let stable_alias_keys = stable_keys.into_iter().skip(1).collect();

        if hunk_gap > 0 && matches!(row, DiffRow::HunkHeader { .. }) && hunk_index > 0 {
            planned_rows.push(PlannedReviewRow::HunkGap {
                key: format!("hunk-gap:{file_id}:{hunk_index}"),
                stable_key: format!("hunk-gap:{hunk_index}"),
                file_id: file_id.into(),
                hunk_index,
                height: hunk_gap,
            });
        }
        if should_anchor_hunk {
            anchored_hunks.insert(hunk_index);
        }

        planned_rows.push(PlannedReviewRow::DiffRow {
            key: format!("diff-row:{}", row_key(row)),
            stable_key,
            stable_alias_keys,
            file_id: row_file_id(row).into(),
            hunk_index,
            row: row.clone(),
            anchor_id,
            note_guide_side: guide_side_by_row_key.get(row_key(row)).copied(),
        });

        if let Some((_, placements)) = placement_groups
            .iter()
            .find(|(anchor_key, _)| anchor_key == row_key(row))
        {
            for placement in placements {
                planned_rows.push(PlannedReviewRow::InlineNote {
                    key: format!(
                        "inline-note:{}:{}:{}",
                        placement.note.id,
                        row_key(row),
                        placement.note_index
                    ),
                    stable_key: inline_note_stable_key(&placement.note.id),
                    file_id: file_id.into(),
                    hunk_index: placement.hunk_index,
                    annotation_id: placement.note.id.clone(),
                    annotation: placement.note.annotation.clone(),
                    note: Box::new(placement.note.clone()),
                    anchor_side: placement.anchor_side,
                    note_count: placement.note_count,
                    note_index: placement.note_index,
                });
            }
        }
    }
    planned_rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{AgentAnnotation, LineRange};
    use workdeck_diff::{RenderSpan, SplitLineCell, StackLineCell};
    use workdeck_review::ResolvedReviewNoteAnchor;

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn header(hunk_index: usize) -> DiffRow {
        DiffRow::HunkHeader {
            key: format!("header:{hunk_index}"),
            file_id: "file".into(),
            hunk_index,
            text: format!("@@ hunk {hunk_index} @@"),
        }
    }

    fn split(
        key: &str,
        hunk_index: usize,
        left: (SplitLineKind, Option<usize>),
        right: (SplitLineKind, Option<usize>),
        expansion: bool,
    ) -> DiffRow {
        DiffRow::SplitLine {
            key: key.into(),
            file_id: "file".into(),
            hunk_index,
            left: SplitLineCell {
                kind: left.0,
                sign: match left.0 {
                    SplitLineKind::Deletion => "-",
                    SplitLineKind::Addition => "+",
                    SplitLineKind::Context | SplitLineKind::Empty => " ",
                }
                .into(),
                line_number: left.1,
                move_kind: None,
                spans: left.1.map_or_else(Vec::new, |_| vec![span(key)]),
            },
            right: SplitLineCell {
                kind: right.0,
                sign: match right.0 {
                    SplitLineKind::Deletion => "-",
                    SplitLineKind::Addition => "+",
                    SplitLineKind::Context | SplitLineKind::Empty => " ",
                }
                .into(),
                line_number: right.1,
                move_kind: None,
                spans: right.1.map_or_else(Vec::new, |_| vec![span(key)]),
            },
            is_expansion_row: expansion,
            expanded_gap_key: expansion.then(|| "before:1".into()),
        }
    }

    fn stack(
        key: &str,
        hunk_index: usize,
        kind: StackLineKind,
        old: Option<usize>,
        new: Option<usize>,
        expansion: bool,
    ) -> DiffRow {
        DiffRow::StackLine {
            key: key.into(),
            file_id: "file".into(),
            hunk_index,
            cell: StackLineCell {
                kind,
                sign: match kind {
                    StackLineKind::Addition => "+",
                    StackLineKind::Deletion => "-",
                    StackLineKind::Context => " ",
                }
                .into(),
                old_line_number: old,
                new_line_number: new,
                move_kind: None,
                spans: vec![span(key)],
            },
            is_expansion_row: expansion,
            expanded_gap_key: expansion.then(|| "before:1".into()),
        }
    }

    fn range(start: u32, end: u32) -> LineRange {
        LineRange { start, end }
    }

    fn note(
        id: &str,
        old_range: Option<LineRange>,
        new_range: Option<LineRange>,
        preferred: Option<(ReviewSide, u32)>,
        owner_hunk_index: Option<usize>,
        intersecting_hunk_indices: Vec<usize>,
    ) -> VisibleAgentNote {
        let annotation = AgentAnnotation {
            id: Some(id.into()),
            old_range,
            new_range,
            summary: id.into(),
            rationale: None,
            markup: None,
            tags: Vec::new(),
            confidence: None,
            source: Some("agent".into()),
            title: None,
            author: None,
            created_at: None,
            updated_at: None,
            editable: false,
        };
        VisibleAgentNote {
            id: id.into(),
            annotation,
            anchor: ResolvedReviewNoteAnchor {
                old_range,
                new_range,
                preferred: preferred.map(|(side, line)| ReviewPreferredLine { side, line }),
                intersecting_hunk_indices,
                owner_hunk_index,
            },
            source: None,
            editable: false,
            thread: None,
            actions: None,
            draft: None,
        }
    }

    fn plan(
        rows: &[DiffRow],
        show_hunk_headers: bool,
        notes: &[VisibleAgentNote],
        hunk_gap: usize,
    ) -> Vec<PlannedReviewRow> {
        build_review_render_plan(ReviewRenderPlanOptions {
            file_id: "file",
            rows,
            show_hunk_headers,
            visible_agent_notes: notes,
            selected_hunk_index: Some(0),
            hunk_gap,
        })
    }

    fn inline_note_index(rows: &[PlannedReviewRow]) -> usize {
        rows.iter()
            .position(|row| matches!(row, PlannedReviewRow::InlineNote { .. }))
            .expect("inline note")
    }

    fn planned_source_line(row: &PlannedReviewRow, side: ReviewSide) -> Option<usize> {
        row.diff_row().and_then(|row| row_line_number(row, side))
    }

    #[test]
    fn inserts_note_after_anchor_and_starts_guide_below_note() {
        let rows = vec![
            header(0),
            split(
                "line:1",
                0,
                (SplitLineKind::Deletion, Some(1)),
                (SplitLineKind::Addition, Some(1)),
                false,
            ),
            split(
                "line:2",
                0,
                (SplitLineKind::Empty, None),
                (SplitLineKind::Addition, Some(2)),
                false,
            ),
            split(
                "line:3",
                0,
                (SplitLineKind::Empty, None),
                (SplitLineKind::Addition, Some(3)),
                false,
            ),
        ];
        let notes = vec![note(
            "annotation:alpha:0:0",
            None,
            Some(range(2, 3)),
            Some((ReviewSide::New, 2)),
            Some(0),
            vec![0],
        )];
        let planned = plan(&rows, true, &notes, 0);
        let note_index = inline_note_index(&planned);
        assert_eq!(
            planned_source_line(&planned[note_index - 1], ReviewSide::New),
            Some(2)
        );
        let PlannedReviewRow::InlineNote {
            anchor_side,
            note_count,
            note_index,
            ..
        } = &planned[note_index]
        else {
            unreachable!()
        };
        assert_eq!(*anchor_side, Some(ReviewSide::New));
        assert_eq!((*note_count, *note_index), (1, 0));
        assert_eq!(
            planned
                .iter()
                .filter(|row| row.note_guide_side() == Some(ReviewSide::New))
                .filter_map(|row| planned_source_line(row, ReviewSide::New))
                .collect::<Vec<_>>(),
            [3]
        );
    }

    #[test]
    fn deletion_note_anchors_old_without_dangling_guide() {
        let rows = vec![
            header(0),
            split(
                "removed",
                0,
                (SplitLineKind::Deletion, Some(1)),
                (SplitLineKind::Empty, None),
                false,
            ),
            split(
                "kept",
                0,
                (SplitLineKind::Context, Some(2)),
                (SplitLineKind::Context, Some(1)),
                false,
            ),
        ];
        let notes = vec![note(
            "annotation:deleted:0:0",
            Some(range(1, 1)),
            None,
            Some((ReviewSide::Old, 1)),
            Some(0),
            vec![0],
        )];
        let planned = plan(&rows, true, &notes, 0);
        let note_index = inline_note_index(&planned);
        assert_eq!(
            planned_source_line(&planned[note_index - 1], ReviewSide::Old),
            Some(1)
        );
        assert_eq!(
            planned_source_line(&planned[note_index - 1], ReviewSide::New),
            None
        );
        assert!(
            planned
                .iter()
                .all(|row| row.note_guide_side() != Some(ReviewSide::Old))
        );
    }

    #[test]
    fn hidden_headers_anchor_each_hunk_at_first_non_expansion_code_row() {
        let rows = vec![
            header(0),
            split(
                "h0",
                0,
                (SplitLineKind::Context, Some(1)),
                (SplitLineKind::Context, Some(1)),
                false,
            ),
            header(1),
            split(
                "h1-gap",
                1,
                (SplitLineKind::Context, Some(7)),
                (SplitLineKind::Context, Some(7)),
                true,
            ),
            split(
                "h1",
                1,
                (SplitLineKind::Deletion, Some(11)),
                (SplitLineKind::Addition, Some(11)),
                false,
            ),
        ];
        let planned = plan(&rows, false, &[], 0);
        let anchors = planned
            .iter()
            .filter_map(|row| match row {
                PlannedReviewRow::DiffRow {
                    anchor_id: Some(anchor),
                    row,
                    ..
                } => Some((anchor.as_str(), row_key(row))),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            anchors,
            [("diff-hunk:file:0", "h0"), ("diff-hunk:file:1", "h1")]
        );
    }

    #[test]
    fn range_less_note_uses_default_new_first_line_without_guides() {
        let rows = vec![
            header(0),
            stack("old", 0, StackLineKind::Deletion, Some(1), None, false),
            stack("new", 0, StackLineKind::Addition, None, Some(1), false),
        ];
        let notes = vec![note("general", None, None, None, Some(0), Vec::new())];
        let planned = plan(&rows, true, &notes, 0);
        let note_index = inline_note_index(&planned);
        assert_eq!(
            planned_source_line(&planned[note_index - 1], ReviewSide::New),
            Some(1)
        );
        let PlannedReviewRow::InlineNote { anchor_side, .. } = &planned[note_index] else {
            unreachable!()
        };
        assert_eq!(*anchor_side, None);
        assert!(planned.iter().all(|row| row.note_guide_side().is_none()));
    }

    #[test]
    fn note_uses_matching_hunk_and_collapsed_owner_fallback() {
        let rows = vec![
            header(0),
            split(
                "h0-line",
                0,
                (SplitLineKind::Context, Some(1)),
                (SplitLineKind::Context, Some(1)),
                false,
            ),
            header(1),
            split(
                "h1-line8",
                1,
                (SplitLineKind::Context, Some(8)),
                (SplitLineKind::Context, Some(8)),
                false,
            ),
            split(
                "h1-line11",
                1,
                (SplitLineKind::Deletion, Some(11)),
                (SplitLineKind::Addition, Some(11)),
                false,
            ),
        ];
        let later_notes = vec![note(
            "later",
            None,
            Some(range(11, 11)),
            Some((ReviewSide::New, 11)),
            Some(1),
            vec![1],
        )];
        let later = plan(&rows, true, &later_notes, 0);
        let later_index = inline_note_index(&later);
        assert_eq!(
            planned_source_line(&later[later_index - 1], ReviewSide::New),
            Some(11)
        );

        let collapsed_notes = vec![note(
            "collapsed",
            None,
            Some(range(6, 7)),
            Some((ReviewSide::New, 6)),
            Some(1),
            Vec::new(),
        )];
        let collapsed = plan(&rows, true, &collapsed_notes, 0);
        let collapsed_index = inline_note_index(&collapsed);
        assert_eq!(
            planned_source_line(&collapsed[collapsed_index - 1], ReviewSide::New),
            Some(8)
        );
        assert!(matches!(
            &collapsed[collapsed_index],
            PlannedReviewRow::InlineNote { hunk_index: 1, .. }
        ));
    }

    #[test]
    fn expanded_gap_note_stays_beside_revealed_line() {
        let rows = vec![
            header(1),
            split(
                "expanded:7",
                1,
                (SplitLineKind::Context, Some(7)),
                (SplitLineKind::Context, Some(7)),
                true,
            ),
            split(
                "changed:11",
                1,
                (SplitLineKind::Deletion, Some(11)),
                (SplitLineKind::Addition, Some(11)),
                false,
            ),
        ];
        let notes = vec![note(
            "expanded-note",
            None,
            Some(range(7, 7)),
            Some((ReviewSide::New, 7)),
            Some(1),
            Vec::new(),
        )];
        let planned = plan(&rows, true, &notes, 0);
        let note_index = inline_note_index(&planned);
        assert!(matches!(
            planned[note_index - 1].diff_row(),
            Some(DiffRow::SplitLine {
                is_expansion_row: true,
                right: SplitLineCell {
                    line_number: Some(7),
                    ..
                },
                ..
            })
        ));
    }

    #[test]
    fn every_visible_note_renders_at_its_own_anchor_in_row_order() {
        let rows = vec![
            header(0),
            split(
                "line1",
                0,
                (SplitLineKind::Deletion, Some(1)),
                (SplitLineKind::Addition, Some(1)),
                false,
            ),
            split(
                "line2",
                0,
                (SplitLineKind::Empty, None),
                (SplitLineKind::Addition, Some(2)),
                false,
            ),
        ];
        let notes = vec![
            note(
                "annotation:counted:0:0",
                None,
                Some(range(2, 2)),
                Some((ReviewSide::New, 2)),
                Some(0),
                vec![0],
            ),
            note(
                "annotation:counted:0:1",
                None,
                Some(range(1, 1)),
                Some((ReviewSide::New, 1)),
                Some(0),
                vec![0],
            ),
        ];
        let planned = plan(&rows, true, &notes, 0);
        let inline = planned
            .iter()
            .filter_map(|row| match row {
                PlannedReviewRow::InlineNote {
                    annotation_id,
                    note_index,
                    note_count,
                    ..
                } => Some((annotation_id.as_str(), *note_index, *note_count)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            inline,
            [
                ("annotation:counted:0:1", 0, 1),
                ("annotation:counted:0:0", 0, 1),
            ]
        );
    }

    #[test]
    fn inserts_configured_gap_before_each_later_hunk_in_split_and_stack() {
        for rows in [
            vec![
                header(0),
                split(
                    "s0",
                    0,
                    (SplitLineKind::Context, Some(1)),
                    (SplitLineKind::Context, Some(1)),
                    false,
                ),
                header(1),
                split(
                    "s1",
                    1,
                    (SplitLineKind::Context, Some(9)),
                    (SplitLineKind::Context, Some(9)),
                    false,
                ),
            ],
            vec![
                header(0),
                stack("t0", 0, StackLineKind::Context, Some(1), Some(1), false),
                header(1),
                stack("t1", 1, StackLineKind::Context, Some(9), Some(9), false),
            ],
        ] {
            assert!(
                !plan(&rows, true, &[], 0)
                    .iter()
                    .any(|row| matches!(row, PlannedReviewRow::HunkGap { .. }))
            );
            let planned = plan(&rows, true, &[], 2);
            let header_index = planned
                .iter()
                .position(|row| {
                    matches!(
                        row,
                        PlannedReviewRow::DiffRow {
                            row: DiffRow::HunkHeader { hunk_index: 1, .. },
                            ..
                        }
                    )
                })
                .expect("second header");
            assert!(matches!(
                &planned[header_index - 1],
                PlannedReviewRow::HunkGap {
                    height: 2,
                    hunk_index: 1,
                    ..
                }
            ));
            assert_eq!(
                planned
                    .iter()
                    .filter(|row| matches!(row, PlannedReviewRow::HunkGap { .. }))
                    .count(),
                1
            );
        }
    }

    #[test]
    fn hunk_gap_stays_before_hidden_later_header() {
        let rows = vec![
            header(0),
            split(
                "h0",
                0,
                (SplitLineKind::Context, Some(1)),
                (SplitLineKind::Context, Some(1)),
                false,
            ),
            header(1),
            split(
                "h1",
                1,
                (SplitLineKind::Context, Some(9)),
                (SplitLineKind::Context, Some(9)),
                false,
            ),
        ];
        let planned = plan(&rows, false, &[], 2);
        let header_index = planned
            .iter()
            .position(|row| {
                matches!(
                    row,
                    PlannedReviewRow::DiffRow {
                        row: DiffRow::HunkHeader { hunk_index: 1, .. },
                        ..
                    }
                )
            })
            .expect("second header");
        assert!(matches!(
            &planned[header_index - 1],
            PlannedReviewRow::HunkGap { height: 2, .. }
        ));
    }

    #[test]
    fn single_sided_stable_keys_round_trip_to_note_targets() {
        assert_eq!(
            line_stable_key_target(&line_stable_key(2, ReviewSide::Old, 41)),
            Some(ReviewLineTarget {
                hunk_index: 2,
                side: ReviewSide::Old,
                line: 41,
            })
        );
        assert_eq!(
            line_stable_key_target(&line_stable_key(0, ReviewSide::New, 7)),
            Some(ReviewLineTarget {
                hunk_index: 0,
                side: ReviewSide::New,
                line: 7,
            })
        );
    }

    #[test]
    fn context_stable_key_targets_new_side_and_preserves_both_sides() {
        assert_eq!(
            context_line_stable_key_target("line:3:context:12:14"),
            Some(ReviewLineTarget {
                hunk_index: 3,
                side: ReviewSide::New,
                line: 14,
            })
        );
        assert_eq!(
            context_line_stable_key_sides("line:3:context:12:14"),
            Some(ContextLineStableKeySides {
                hunk_index: 3,
                old_line: 12,
                new_line: 14,
            })
        );
    }

    #[test]
    fn stable_key_shapes_are_disjoint_and_invalid_metadata_is_ignored() {
        assert_eq!(line_stable_key_target("line:3:context:12:14"), None);
        assert_eq!(
            context_line_stable_key_target(&line_stable_key(3, ReviewSide::New, 14)),
            None
        );
        for stable_key in [
            "meta:hunk-header:1",
            "meta:collapsed:before:0",
            "inline-note:x",
        ] {
            assert_eq!(line_stable_key_target(stable_key), None);
            assert_eq!(context_line_stable_key_target(stable_key), None);
        }
        for invalid in [
            "line:-1:new:2",
            "line:1:new:2:extra",
            "line:one:new:2",
            "line:1:new:two",
            "line:1:context:2",
        ] {
            assert_eq!(line_stable_key_target(invalid), None);
            assert_eq!(context_line_stable_key_target(invalid), None);
        }
    }

    #[test]
    fn stable_keys_preserve_cross_layout_resolution_order_and_meta_rows() {
        let rows = vec![
            DiffRow::Collapsed {
                key: "gap".into(),
                file_id: "file".into(),
                hunk_index: 1,
                text: "3 lines".into(),
                position: ReviewGapPosition::Before,
                old_range: [2, 4],
                new_range: [3, 5],
            },
            header(1),
            split(
                "context",
                1,
                (SplitLineKind::Context, Some(12)),
                (SplitLineKind::Context, Some(14)),
                false,
            ),
            split(
                "changed",
                1,
                (SplitLineKind::Deletion, Some(15)),
                (SplitLineKind::Addition, Some(17)),
                false,
            ),
            stack(
                "addition",
                1,
                StackLineKind::Addition,
                None,
                Some(18),
                false,
            ),
        ];
        let planned = plan(&rows, true, &[], 0);
        let keys = planned
            .iter()
            .filter_map(|row| match row {
                PlannedReviewRow::DiffRow {
                    stable_key,
                    stable_alias_keys,
                    ..
                } => Some((stable_key.clone(), stable_alias_keys.clone())),
                PlannedReviewRow::InlineNote { .. } | PlannedReviewRow::HunkGap { .. } => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(keys[0], ("meta:collapsed:before:1".into(), Vec::new()));
        assert_eq!(keys[1], ("meta:hunk-header:1".into(), Vec::new()));
        assert_eq!(
            keys[2],
            (
                "line:1:context:12:14".into(),
                vec!["line:1:new:14".into(), "line:1:old:12".into()]
            )
        );
        assert_eq!(
            keys[3],
            ("line:1:old:15".into(), vec!["line:1:new:17".into()])
        );
        assert_eq!(keys[4], ("line:1:new:18".into(), Vec::new()));
    }

    #[test]
    fn first_visible_note_wins_conflicting_guide_side_deterministically() {
        let rows = vec![
            header(0),
            split(
                "line1",
                0,
                (SplitLineKind::Context, Some(1)),
                (SplitLineKind::Context, Some(1)),
                false,
            ),
            split(
                "line2",
                0,
                (SplitLineKind::Context, Some(2)),
                (SplitLineKind::Context, Some(2)),
                false,
            ),
            split(
                "line3",
                0,
                (SplitLineKind::Context, Some(3)),
                (SplitLineKind::Context, Some(3)),
                false,
            ),
        ];
        let notes = vec![
            note(
                "old-first",
                Some(range(1, 3)),
                None,
                Some((ReviewSide::Old, 1)),
                Some(0),
                vec![0],
            ),
            note(
                "new-second",
                None,
                Some(range(1, 3)),
                Some((ReviewSide::New, 1)),
                Some(0),
                vec![0],
            ),
        ];
        let planned = plan(&rows, true, &notes, 0);
        assert_eq!(
            planned
                .iter()
                .filter_map(|row| match row {
                    PlannedReviewRow::DiffRow {
                        row,
                        note_guide_side,
                        ..
                    } if row_key(row) == "line2" || row_key(row) == "line3" => {
                        *note_guide_side
                    }
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [ReviewSide::Old, ReviewSide::Old]
        );
    }

    fn side_name(side: ReviewSide) -> &'static str {
        match side {
            ReviewSide::Old => "old",
            ReviewSide::New => "new",
        }
    }

    fn compact_plan(rows: &[PlannedReviewRow]) -> serde_json::Value {
        serde_json::Value::Array(
            rows.iter()
                .map(|row| match row {
                    PlannedReviewRow::DiffRow {
                        key,
                        stable_key,
                        stable_alias_keys,
                        anchor_id,
                        note_guide_side,
                        ..
                    } => serde_json::json!({
                        "kind": "diff-row",
                        "key": key,
                        "stableKey": stable_key,
                        "aliases": stable_alias_keys,
                        "anchorId": anchor_id,
                        "guide": note_guide_side.map(side_name),
                    }),
                    PlannedReviewRow::InlineNote {
                        key,
                        stable_key,
                        hunk_index,
                        anchor_side,
                        note_count,
                        note_index,
                        ..
                    } => serde_json::json!({
                        "kind": "inline-note",
                        "key": key,
                        "stableKey": stable_key,
                        "hunkIndex": hunk_index,
                        "side": anchor_side.map(side_name),
                        "count": note_count,
                        "index": note_index,
                    }),
                    PlannedReviewRow::HunkGap {
                        key,
                        stable_key,
                        hunk_index,
                        height,
                        ..
                    } => serde_json::json!({
                        "kind": "hunk-gap",
                        "key": key,
                        "stableKey": stable_key,
                        "hunkIndex": hunk_index,
                        "height": height,
                    }),
                })
                .collect(),
        )
    }

    #[test]
    fn frozen_hunk_review_render_plan_vectors_match_native() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/review-render-plan.json"
        ))
        .expect("valid frozen Hunk review-render-plan oracle");
        assert_eq!(oracle["baselineOracle"]["passed"], 14);
        assert_eq!(oracle["stableOracle"]["passed"], 12);
        let vectors = &oracle["projectionVectors"];

        let note_rows = vec![
            header(0),
            split(
                "l1",
                0,
                (SplitLineKind::Deletion, Some(1)),
                (SplitLineKind::Addition, Some(1)),
                false,
            ),
            split(
                "l2",
                0,
                (SplitLineKind::Empty, None),
                (SplitLineKind::Addition, Some(2)),
                false,
            ),
            split(
                "l3",
                0,
                (SplitLineKind::Empty, None),
                (SplitLineKind::Addition, Some(3)),
                false,
            ),
        ];
        let notes = vec![note(
            "n",
            None,
            Some(range(2, 3)),
            Some((ReviewSide::New, 2)),
            Some(0),
            vec![0],
        )];
        assert_eq!(
            compact_plan(&plan(&note_rows, true, &notes, 0)),
            vectors["notePlan"]
        );

        let gap_rows = vec![header(0), header(1)];
        assert_eq!(
            compact_plan(&plan(&gap_rows, false, &[], 2)),
            vectors["gapPlan"]
        );

        let stable_rows = vec![
            DiffRow::Collapsed {
                key: "gap".into(),
                file_id: "file".into(),
                hunk_index: 1,
                text: "3".into(),
                position: ReviewGapPosition::Before,
                old_range: [2, 4],
                new_range: [3, 5],
            },
            header(1),
            split(
                "context",
                1,
                (SplitLineKind::Context, Some(12)),
                (SplitLineKind::Context, Some(14)),
                false,
            ),
            split(
                "changed",
                1,
                (SplitLineKind::Deletion, Some(15)),
                (SplitLineKind::Addition, Some(17)),
                false,
            ),
            stack(
                "addition",
                1,
                StackLineKind::Addition,
                None,
                Some(18),
                false,
            ),
        ];
        assert_eq!(
            compact_plan(&plan(&stable_rows, true, &[], 0)),
            vectors["stablePlan"]
        );

        let sided = line_stable_key_target("line:2:old:41").expect("sided target");
        let context =
            context_line_stable_key_target("line:3:context:12:14").expect("context target");
        let context_sides =
            context_line_stable_key_sides("line:3:context:12:14").expect("context sides");
        let targets = serde_json::json!([
            {"hunkIndex":sided.hunk_index,"side":side_name(sided.side),"line":sided.line},
            {"hunkIndex":context.hunk_index,"side":side_name(context.side),"line":context.line},
            {"hunkIndex":context_sides.hunk_index,"oldLine":context_sides.old_line,"newLine":context_sides.new_line},
            line_stable_key_target("line:3:context:12:14").map(|_| "unexpected"),
        ]);
        assert_eq!(targets, vectors["targets"]);
    }
}
