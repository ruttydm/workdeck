//! Canonical terminal geometry and lazy height planning for diff code rows.
//!
//! This is a Rust reimplementation of Hunk's `src/ui/diff/codeRowLayout.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.

use std::cell::Cell;

use workdeck_core::ReviewSide;
use workdeck_diff::{
    DIFF_RAIL_PREFIX_WIDTH, DiffRow, RenderSpan, TextSegment, measure_wrapped_segments_line_count,
    resolve_split_cell_geometry, resolve_split_pane_widths, resolve_stack_cell_geometry,
};

use crate::{AppTheme, CODE_ROW_ADD_NOTE_BADGE_WIDTH, PlannedReviewRow};

#[derive(Debug)]
pub struct CodeCellLayoutPlan<'a> {
    pub width: usize,
    pub prefix_width: usize,
    pub gutter_width: usize,
    pub content_width: usize,
    spans: &'a [RenderSpan],
    wrap_lines: bool,
    measured_wrapped_line_count: Cell<Option<usize>>,
}

impl CodeCellLayoutPlan<'_> {
    #[must_use]
    pub fn wrapped_line_count(&self) -> usize {
        if let Some(count) = self.measured_wrapped_line_count.get() {
            return count;
        }
        let count = if self.wrap_lines {
            measure_wrapped_segments_line_count(&plain_segments(self.spans), self.content_width)
        } else {
            1
        };
        self.measured_wrapped_line_count.set(Some(count));
        count
    }
}

#[derive(Debug)]
pub enum CodeRowLayoutPlan<'a> {
    Split {
        left: CodeCellLayoutPlan<'a>,
        right: CodeCellLayoutPlan<'a>,
        left_pane_width: usize,
        right_pane_width: usize,
        note_guide_side: Option<ReviewSide>,
        trailing_guide_width: usize,
        add_note_badge_width: usize,
        measured_wrapped_line_count: Cell<Option<usize>>,
    },
    Stack {
        cell: CodeCellLayoutPlan<'a>,
        note_guide_side: Option<ReviewSide>,
        trailing_guide_width: usize,
        add_note_badge_width: usize,
        measured_wrapped_line_count: Cell<Option<usize>>,
    },
}

impl CodeRowLayoutPlan<'_> {
    #[must_use]
    pub fn wrapped_line_count(&self) -> usize {
        match self {
            Self::Split {
                left,
                right,
                measured_wrapped_line_count,
                ..
            } => {
                if let Some(count) = measured_wrapped_line_count.get() {
                    return count;
                }
                let count = left.wrapped_line_count().max(right.wrapped_line_count());
                measured_wrapped_line_count.set(Some(count));
                count
            }
            Self::Stack {
                cell,
                measured_wrapped_line_count,
                ..
            } => {
                if let Some(count) = measured_wrapped_line_count.get() {
                    return count;
                }
                let count = cell.wrapped_line_count();
                measured_wrapped_line_count.set(Some(count));
                count
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeRowLayoutOptions {
    pub width: usize,
    pub line_number_digits: usize,
    pub show_line_numbers: bool,
    pub wrap_lines: bool,
    pub reserve_add_note_column: bool,
    pub show_add_note_badge: bool,
}

fn plain_segments(spans: &[RenderSpan]) -> Vec<TextSegment<()>> {
    spans
        .iter()
        .map(|span| TextSegment {
            text: span.text.clone(),
            style: (),
        })
        .collect()
}

fn plan_code_cell_layout(
    spans: &[RenderSpan],
    width: usize,
    prefix_width: usize,
    gutter_width: usize,
    wrap_lines: bool,
) -> CodeCellLayoutPlan<'_> {
    CodeCellLayoutPlan {
        width,
        prefix_width,
        gutter_width,
        content_width: width.saturating_sub(prefix_width.saturating_add(gutter_width)),
        spans,
        wrap_lines,
        measured_wrapped_line_count: Cell::new(None),
    }
}

#[must_use]
pub fn plan_code_row_layout<'a>(
    planned_row: &'a PlannedReviewRow,
    options: CodeRowLayoutOptions,
) -> Option<CodeRowLayoutPlan<'a>> {
    let PlannedReviewRow::DiffRow {
        row,
        note_guide_side,
        ..
    } = planned_row
    else {
        return None;
    };
    if !matches!(row, DiffRow::SplitLine { .. } | DiffRow::StackLine { .. }) {
        return None;
    }

    let prefix_width = DIFF_RAIL_PREFIX_WIDTH;
    let trailing_guide_width = usize::from(*note_guide_side == Some(ReviewSide::New));
    let add_note_badge_width =
        if options.show_add_note_badge || (options.wrap_lines && options.reserve_add_note_column) {
            usize::from(CODE_ROW_ADD_NOTE_BADGE_WIDTH)
        } else {
            0
        };

    match row {
        DiffRow::SplitLine { left, right, .. } => {
            let panes = resolve_split_pane_widths(options.width);
            let right_width = panes
                .right_width
                .saturating_sub(trailing_guide_width)
                .saturating_sub(add_note_badge_width);
            let left_geometry = resolve_split_cell_geometry(
                panes.left_width,
                options.line_number_digits,
                options.show_line_numbers,
                prefix_width,
            );
            let right_geometry = resolve_split_cell_geometry(
                right_width,
                options.line_number_digits,
                options.show_line_numbers,
                prefix_width,
            );
            Some(CodeRowLayoutPlan::Split {
                left: plan_code_cell_layout(
                    &left.spans,
                    panes.left_width,
                    prefix_width,
                    left_geometry.gutter_width,
                    options.wrap_lines,
                ),
                right: plan_code_cell_layout(
                    &right.spans,
                    right_width,
                    prefix_width,
                    right_geometry.gutter_width,
                    options.wrap_lines,
                ),
                left_pane_width: panes.left_width,
                right_pane_width: panes.right_width,
                note_guide_side: *note_guide_side,
                trailing_guide_width,
                add_note_badge_width,
                measured_wrapped_line_count: Cell::new(None),
            })
        }
        DiffRow::StackLine { cell, .. } => {
            let cell_width = options
                .width
                .saturating_sub(trailing_guide_width)
                .saturating_sub(add_note_badge_width);
            let geometry = resolve_stack_cell_geometry(
                cell_width,
                options.line_number_digits,
                options.show_line_numbers,
                prefix_width,
            );
            Some(CodeRowLayoutPlan::Stack {
                cell: plan_code_cell_layout(
                    &cell.spans,
                    cell_width,
                    prefix_width,
                    geometry.gutter_width,
                    options.wrap_lines,
                ),
                note_guide_side: *note_guide_side,
                trailing_guide_width,
                add_note_badge_width,
                measured_wrapped_line_count: Cell::new(None),
            })
        }
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => None,
    }
}

#[must_use]
pub fn legacy_planned_diff_row(
    row: DiffRow,
    anchor_id: Option<String>,
    note_guide_side: Option<ReviewSide>,
) -> PlannedReviewRow {
    let (key, file_id, hunk_index) = match &row {
        DiffRow::Collapsed {
            key,
            file_id,
            hunk_index,
            ..
        }
        | DiffRow::HunkHeader {
            key,
            file_id,
            hunk_index,
            ..
        }
        | DiffRow::SplitLine {
            key,
            file_id,
            hunk_index,
            ..
        }
        | DiffRow::StackLine {
            key,
            file_id,
            hunk_index,
            ..
        } => (key.clone(), file_id.clone(), *hunk_index),
    };
    PlannedReviewRow::DiffRow {
        stable_key: key.clone(),
        key,
        stable_alias_keys: Vec::new(),
        file_id,
        hunk_index,
        row,
        anchor_id,
        note_guide_side,
    }
}

#[must_use]
pub fn measure_planned_rendered_row_height(
    planned_row: &PlannedReviewRow,
    options: CodeRowLayoutOptions,
    show_hunk_headers: bool,
) -> usize {
    let PlannedReviewRow::DiffRow { row, .. } = planned_row else {
        return 1;
    };
    match row {
        DiffRow::HunkHeader { .. } => usize::from(show_hunk_headers),
        DiffRow::Collapsed { .. } => 1,
        DiffRow::SplitLine { .. } | DiffRow::StackLine { .. } if options.wrap_lines => {
            plan_code_row_layout(planned_row, options).map_or(1, |plan| plan.wrapped_line_count())
        }
        DiffRow::SplitLine { .. } | DiffRow::StackLine { .. } => 1,
    }
}

#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn measure_rendered_row_height(
    row: DiffRow,
    width: usize,
    line_number_digits: usize,
    show_line_numbers: bool,
    show_hunk_headers: bool,
    wrap_lines: bool,
    _theme: &AppTheme,
    reserve_add_note_column: bool,
) -> usize {
    measure_planned_rendered_row_height(
        &legacy_planned_diff_row(row, None, None),
        CodeRowLayoutOptions {
            width,
            line_number_digits,
            show_line_numbers,
            wrap_lines,
            reserve_add_note_column,
            show_add_note_badge: false,
        },
        show_hunk_headers,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_diff::{SplitLineCell, SplitLineKind, StackLineCell, StackLineKind};

    use crate::{PlannedRowTextOptions, render_decorated_planned_row_text};

    const BOUNDARY_TEXT: &str = "1234567";

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn split_planned_row(note_guide_side: Option<ReviewSide>) -> PlannedReviewRow {
        legacy_planned_diff_row(
            DiffRow::SplitLine {
                key: "file:split:1".into(),
                file_id: "file".into(),
                hunk_index: 0,
                left: SplitLineCell {
                    kind: SplitLineKind::Empty,
                    sign: " ".into(),
                    line_number: None,
                    move_kind: None,
                    spans: Vec::new(),
                },
                right: SplitLineCell {
                    kind: SplitLineKind::Addition,
                    sign: "+".into(),
                    line_number: Some(1),
                    move_kind: None,
                    spans: vec![span(BOUNDARY_TEXT)],
                },
                is_expansion_row: false,
                expanded_gap_key: None,
            },
            None,
            note_guide_side,
        )
    }

    fn stack_planned_row(note_guide_side: Option<ReviewSide>) -> PlannedReviewRow {
        legacy_planned_diff_row(
            DiffRow::StackLine {
                key: "file:stack:1".into(),
                file_id: "file".into(),
                hunk_index: 0,
                cell: StackLineCell {
                    kind: StackLineKind::Addition,
                    sign: "+".into(),
                    old_line_number: None,
                    new_line_number: Some(1),
                    move_kind: None,
                    spans: vec![span(BOUNDARY_TEXT)],
                },
                is_expansion_row: false,
                expanded_gap_key: None,
            },
            None,
            note_guide_side,
        )
    }

    fn options(width: usize) -> CodeRowLayoutOptions {
        CodeRowLayoutOptions {
            width,
            line_number_digits: 1,
            show_line_numbers: false,
            wrap_lines: true,
            reserve_add_note_column: false,
            show_add_note_badge: false,
        }
    }

    fn decorated_lines(row: &PlannedReviewRow, layout: CodeRowLayoutOptions) -> Vec<String> {
        render_decorated_planned_row_text(
            row,
            PlannedRowTextOptions {
                width: layout.width,
                line_number_digits: layout.line_number_digits,
                show_line_numbers: layout.show_line_numbers,
                show_hunk_headers: true,
                wrap_lines: layout.wrap_lines,
                code_horizontal_offset: 0,
                reserve_add_note_column: layout.reserve_add_note_column,
                show_add_note_badge: layout.show_add_note_badge,
                side: None,
            },
        )
    }

    #[test]
    fn split_measurement_and_paint_reserve_new_guide_at_wrap_boundary() {
        let options = options(20);
        let unguided = split_planned_row(None);
        let guided = split_planned_row(Some(ReviewSide::New));
        let Some(CodeRowLayoutPlan::Split {
            right,
            trailing_guide_width,
            ..
        }) = plan_code_row_layout(&unguided, options)
        else {
            panic!("expected split plan");
        };
        assert_eq!(right.content_width, 7);
        assert_eq!(right.wrapped_line_count(), 1);
        assert_eq!(trailing_guide_width, 0);

        let Some(CodeRowLayoutPlan::Split {
            right,
            trailing_guide_width,
            ..
        }) = plan_code_row_layout(&guided, options)
        else {
            panic!("expected split plan");
        };
        assert_eq!(right.content_width, 6);
        assert_eq!(right.wrapped_line_count(), 2);
        assert_eq!(trailing_guide_width, 1);
        assert_eq!(
            measure_planned_rendered_row_height(&guided, options, true),
            2
        );
        let rendered = decorated_lines(&guided, options);
        assert_eq!(rendered.len(), 2);
        assert!(rendered.iter().all(|line| line.ends_with('│')));
    }

    #[test]
    fn stack_measurement_and_paint_reserve_new_guide_at_wrap_boundary() {
        let options = options(10);
        let unguided = stack_planned_row(None);
        let guided = stack_planned_row(Some(ReviewSide::New));
        let Some(CodeRowLayoutPlan::Stack {
            cell,
            trailing_guide_width,
            ..
        }) = plan_code_row_layout(&unguided, options)
        else {
            panic!("expected stack plan");
        };
        assert_eq!(cell.content_width, 7);
        assert_eq!(cell.wrapped_line_count(), 1);
        assert_eq!(trailing_guide_width, 0);

        let Some(CodeRowLayoutPlan::Stack {
            cell,
            trailing_guide_width,
            ..
        }) = plan_code_row_layout(&guided, options)
        else {
            panic!("expected stack plan");
        };
        assert_eq!(cell.content_width, 6);
        assert_eq!(cell.wrapped_line_count(), 2);
        assert_eq!(trailing_guide_width, 1);
        assert_eq!(
            measure_planned_rendered_row_height(&guided, options, true),
            2
        );
        let rendered = decorated_lines(&guided, options);
        assert_eq!(rendered.len(), 2);
        assert!(rendered.iter().all(|line| line.ends_with('│')));
    }

    #[test]
    fn wrapped_measurement_is_lazy_and_memoized() {
        for row in [split_planned_row(None), stack_planned_row(None)] {
            let width = if matches!(row.diff_row(), Some(DiffRow::SplitLine { .. })) {
                20
            } else {
                10
            };
            let plan = plan_code_row_layout(&row, options(width)).expect("code plan");
            match &plan {
                CodeRowLayoutPlan::Split {
                    left,
                    right,
                    measured_wrapped_line_count,
                    ..
                } => {
                    assert_eq!(left.measured_wrapped_line_count.get(), None);
                    assert_eq!(right.measured_wrapped_line_count.get(), None);
                    assert_eq!(measured_wrapped_line_count.get(), None);
                }
                CodeRowLayoutPlan::Stack {
                    cell,
                    measured_wrapped_line_count,
                    ..
                } => {
                    assert_eq!(cell.measured_wrapped_line_count.get(), None);
                    assert_eq!(measured_wrapped_line_count.get(), None);
                }
            }
            assert_eq!(plan.wrapped_line_count(), 1);
            assert_eq!(plan.wrapped_line_count(), 1);
            match &plan {
                CodeRowLayoutPlan::Split {
                    right,
                    measured_wrapped_line_count,
                    ..
                } => {
                    assert_eq!(right.measured_wrapped_line_count.get(), Some(1));
                    assert_eq!(measured_wrapped_line_count.get(), Some(1));
                }
                CodeRowLayoutPlan::Stack {
                    cell,
                    measured_wrapped_line_count,
                    ..
                } => {
                    assert_eq!(cell.measured_wrapped_line_count.get(), Some(1));
                    assert_eq!(measured_wrapped_line_count.get(), Some(1));
                }
            }
        }
    }

    #[test]
    fn guide_badge_and_wrap_reservations_match_split_and_stack() {
        for factory in [split_planned_row, stack_planned_row] {
            for guide in [None, Some(ReviewSide::Old), Some(ReviewSide::New)] {
                for wrap_lines in [false, true] {
                    for reserve_add_note_column in [false, true] {
                        for show_add_note_badge in [false, true] {
                            let options = CodeRowLayoutOptions {
                                width: 20,
                                line_number_digits: 2,
                                show_line_numbers: true,
                                wrap_lines,
                                reserve_add_note_column,
                                show_add_note_badge,
                            };
                            let row = factory(guide);
                            let plan = plan_code_row_layout(&row, options).expect("code plan");
                            let expected_badge =
                                if show_add_note_badge || (wrap_lines && reserve_add_note_column) {
                                    usize::from(CODE_ROW_ADD_NOTE_BADGE_WIDTH)
                                } else {
                                    0
                                };
                            let expected_guide = usize::from(guide == Some(ReviewSide::New));
                            match plan {
                                CodeRowLayoutPlan::Split {
                                    left,
                                    right,
                                    trailing_guide_width,
                                    add_note_badge_width,
                                    ..
                                } => {
                                    assert_eq!(add_note_badge_width, expected_badge);
                                    assert_eq!(trailing_guide_width, expected_guide);
                                    assert_eq!(
                                        left.width
                                            + right.width
                                            + trailing_guide_width
                                            + add_note_badge_width,
                                        options.width
                                    );
                                    assert_eq!(left.prefix_width, 1);
                                    assert_eq!(right.prefix_width, 1);
                                }
                                CodeRowLayoutPlan::Stack {
                                    cell,
                                    trailing_guide_width,
                                    add_note_badge_width,
                                    ..
                                } => {
                                    assert_eq!(add_note_badge_width, expected_badge);
                                    assert_eq!(trailing_guide_width, expected_guide);
                                    assert_eq!(
                                        cell.width + trailing_guide_width + add_note_badge_width,
                                        options.width
                                    );
                                    assert_eq!(cell.prefix_width, 1);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
