//! Final composition of split and stack diff rows.
//!
//! This is a native Ratatui reimplementation of Hunk's
//! `src/ui/diff/CodeRowView.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. The original React component
//! owned nested hover and add-note callbacks. Here the renderer returns the
//! same painted cells together with explicit hit metadata for the TUI shell.

use workdeck_core::ReviewSide;
use workdeck_diff::{DiffRow, SplitLineKind, StackLineKind};

use crate::{
    AppTheme, CODE_ROW_ADD_NOTE_BADGE_TEXT, CODE_ROW_ADD_NOTE_BADGE_WIDTH, CodeCellHighlight,
    CodeCellHighlightKind, CodeCellPaintOptions, CodeCellPrefix, CodeRowLayoutOptions,
    CodeRowLayoutPlan, CopySelectedRowRange, CursorHighlight, CursorHighlightStyle,
    FULL_CODE_CELL_COL_RANGE, LineHighlightPaintIndex, PaintedCodeCellLine, PaintedCodeCellRun,
    PlannedReviewRow, RowCellKind, apply_code_cell_line_highlights, code_cell_spacer,
    diff_rail_marker, paint_nowrap_split_code_cells, paint_nowrap_stack_code_cell,
    paint_wrapped_split_code_cells, paint_wrapped_stack_code_cell, plan_code_row_layout,
    split_cell_palette, split_left_rail_color, split_right_rail_color, stack_cell_palette,
    stack_rail_color,
};

/// One source-side address offered by the hover-only add-note affordance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeRowLineTarget {
    pub side: ReviewSide,
    pub line: usize,
}

/// Host-owned hit area replacing the nested React mouse callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRowAddNoteHit {
    pub hunk_index: usize,
    pub target: Option<CodeRowLineTarget>,
    pub visual_line: usize,
    pub column_start: usize,
    pub width: usize,
}

/// Final code-row paint and interaction metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedCodeRow {
    pub anchor_id: Option<String>,
    pub row_key: String,
    pub lines: Vec<PaintedCodeCellLine>,
    pub add_note_hit: Option<CodeRowAddNoteHit>,
}

/// Inputs owned by the review shell while painting one canonical planned row.
#[derive(Debug, Clone, Copy)]
pub struct CodeRowViewOptions<'a> {
    pub width: usize,
    pub line_number_digits: usize,
    pub show_line_numbers: bool,
    pub wrap_lines: bool,
    pub code_horizontal_offset: usize,
    pub theme: &'a AppTheme,
    pub selected: bool,
    pub copy_selected_row_range: Option<CopySelectedRowRange>,
    pub copy_selected_side: Option<ReviewSide>,
    pub cursor_highlight: Option<&'a CursorHighlight>,
    pub line_highlights: Option<&'a LineHighlightPaintIndex>,
    pub show_add_note_badge: bool,
    /// Corresponds to the presence of Hunk's `onStartUserNoteAtHunk` callback.
    pub enable_add_note: bool,
}

fn pick_row_highlight(
    selection: CodeCellHighlight,
    cursor: Option<CodeCellHighlight>,
    has_selection: bool,
    on_cursor: bool,
) -> Option<CodeCellHighlight> {
    if has_selection {
        Some(selection)
    } else if on_cursor {
        cursor
    } else {
        None
    }
}

fn split_kind(kind: SplitLineKind) -> RowCellKind {
    match kind {
        SplitLineKind::Context => RowCellKind::Context,
        SplitLineKind::Addition => RowCellKind::Addition,
        SplitLineKind::Deletion => RowCellKind::Deletion,
        SplitLineKind::Empty => RowCellKind::Empty,
    }
}

fn stack_kind(kind: StackLineKind) -> RowCellKind {
    match kind {
        StackLineKind::Context => RowCellKind::Context,
        StackLineKind::Addition => RowCellKind::Addition,
        StackLineKind::Deletion => RowCellKind::Deletion,
    }
}

fn append_line(target: &mut PaintedCodeCellLine, source: PaintedCodeCellLine) {
    for run in source.runs {
        if let Some(previous) = target.runs.last_mut().filter(|previous| {
            previous.foreground == run.foreground && previous.background == run.background
        }) {
            previous.text.push_str(&run.text);
        } else {
            target.runs.push(run);
        }
    }
}

fn append_add_note_badge(line: &mut PaintedCodeCellLine, theme: &AppTheme) {
    line.runs.push(PaintedCodeCellRun {
        text: CODE_ROW_ADD_NOTE_BADGE_TEXT.into(),
        foreground: Some(theme.note_title_text.clone()),
        background: Some(theme.note_title_background.clone()),
    });
}

fn split_add_note_target(row: &DiffRow) -> Option<CodeRowLineTarget> {
    let DiffRow::SplitLine { left, right, .. } = row else {
        return None;
    };
    right
        .line_number
        .map(|line| CodeRowLineTarget {
            side: ReviewSide::New,
            line,
        })
        .or_else(|| {
            left.line_number.map(|line| CodeRowLineTarget {
                side: ReviewSide::Old,
                line,
            })
        })
}

fn stack_add_note_target(row: &DiffRow) -> Option<CodeRowLineTarget> {
    let DiffRow::StackLine { cell, .. } = row else {
        return None;
    };
    cell.new_line_number
        .map(|line| CodeRowLineTarget {
            side: ReviewSide::New,
            line,
        })
        .or_else(|| {
            cell.old_line_number.map(|line| CodeRowLineTarget {
                side: ReviewSide::Old,
                line,
            })
        })
}

fn row_identity(planned_row: &PlannedReviewRow) -> Option<(&str, usize, Option<&str>)> {
    let PlannedReviewRow::DiffRow { row, anchor_id, .. } = planned_row else {
        return None;
    };
    let (key, hunk_index) = match row {
        DiffRow::SplitLine {
            key, hunk_index, ..
        }
        | DiffRow::StackLine {
            key, hunk_index, ..
        } => (key, hunk_index),
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => return None,
    };
    Some((key, *hunk_index, anchor_id.as_deref()))
}

fn add_note_hit(
    options: CodeRowViewOptions<'_>,
    hunk_index: usize,
    target: Option<CodeRowLineTarget>,
) -> Option<CodeRowAddNoteHit> {
    options.show_add_note_badge.then_some(CodeRowAddNoteHit {
        hunk_index,
        target,
        visual_line: 0,
        column_start: options
            .width
            .saturating_sub(usize::from(CODE_ROW_ADD_NOTE_BADGE_WIDTH)),
        width: usize::from(CODE_ROW_ADD_NOTE_BADGE_WIDTH),
    })
}

/// Paint one split or stack review row with selection, cursor, extension,
/// note-guide, wrapping, and add-note-affordance state applied in Hunk order.
#[must_use]
pub fn paint_code_row(
    planned_row: &PlannedReviewRow,
    options: CodeRowViewOptions<'_>,
) -> Option<PaintedCodeRow> {
    let (row_key, hunk_index, anchor_id) = row_identity(planned_row)?;
    let source_row = planned_row.diff_row()?;
    let row = apply_code_cell_line_highlights(source_row, options.line_highlights, options.theme);
    let layout = plan_code_row_layout(
        planned_row,
        CodeRowLayoutOptions {
            width: options.width,
            line_number_digits: options.line_number_digits,
            show_line_numbers: options.show_line_numbers,
            wrap_lines: options.wrap_lines,
            reserve_add_note_column: options.enable_add_note,
            show_add_note_badge: options.show_add_note_badge,
        },
    )?;

    let has_copy_selection = options.copy_selected_row_range.is_some();
    let has_left_selection =
        has_copy_selection && options.copy_selected_side != Some(ReviewSide::New);
    let has_right_selection =
        has_copy_selection && options.copy_selected_side != Some(ReviewSide::Old);
    let selection_highlight = CodeCellHighlight {
        kind: CodeCellHighlightKind::Selection,
        col_range: options.copy_selected_row_range,
    };
    let cursor_row_highlight = options.cursor_highlight.map(|cursor| CodeCellHighlight {
        kind: CodeCellHighlightKind::Cursor,
        col_range: (cursor.style == CursorHighlightStyle::Row).then_some(FULL_CODE_CELL_COL_RANGE),
    });
    let on_cursor_row = options.cursor_highlight.is_some();

    match (&row, &layout) {
        (
            DiffRow::SplitLine { left, right, .. },
            CodeRowLayoutPlan::Split {
                note_guide_side,
                add_note_badge_width,
                ..
            },
        ) => {
            let split_context_row =
                left.kind == SplitLineKind::Context && right.kind == SplitLineKind::Context;
            let cursor_side = options.cursor_highlight.map(|cursor| cursor.side);
            let left_highlight = pick_row_highlight(
                selection_highlight,
                cursor_row_highlight,
                has_left_selection,
                on_cursor_row && (split_context_row || cursor_side == Some(ReviewSide::Old)),
            );
            let right_highlight = pick_row_highlight(
                selection_highlight,
                cursor_row_highlight,
                has_right_selection,
                on_cursor_row && (split_context_row || cursor_side == Some(ReviewSide::New)),
            );
            let emphasized_rail = options.selected || has_copy_selection;
            let guide_on_old_side = *note_guide_side == Some(ReviewSide::Old);
            let guide_on_new_side = *note_guide_side == Some(ReviewSide::New);
            let left_prefix = CodeCellPrefix {
                text: if guide_on_old_side {
                    "│".into()
                } else {
                    diff_rail_marker().into()
                },
                foreground: if guide_on_old_side {
                    options.theme.note_border.clone()
                } else {
                    split_left_rail_color(split_kind(left.kind), options.theme, emphasized_rail)
                },
                background: options.theme.panel.clone(),
            };
            let right_prefix = CodeCellPrefix {
                text: diff_rail_marker().into(),
                foreground: split_right_rail_color(
                    split_kind(right.kind),
                    options.theme,
                    emphasized_rail,
                ),
                background: options.theme.panel.clone(),
            };
            let paint_options = CodeCellPaintOptions {
                line_number_digits: options.line_number_digits,
                show_line_numbers: options.show_line_numbers,
                theme: options.theme,
                horizontal_offset: options.code_horizontal_offset,
                guide_on_new_side,
            };
            let mut lines = if options.wrap_lines {
                paint_wrapped_split_code_cells(
                    &row,
                    &layout,
                    paint_options,
                    &left_prefix,
                    &right_prefix,
                    left_highlight,
                    right_highlight,
                    0,
                )?
            } else {
                vec![paint_nowrap_split_code_cells(
                    &row,
                    &layout,
                    paint_options,
                    &left_prefix,
                    &right_prefix,
                    left_highlight,
                    right_highlight,
                )?]
            };
            if options.show_add_note_badge {
                append_add_note_badge(&mut lines[0], options.theme);
            }
            if options.wrap_lines && *add_note_badge_width > 0 {
                let background = split_cell_palette(
                    split_kind(right.kind),
                    options.theme,
                    right.move_kind.is_some(),
                )
                .content_background;
                let first_spacer_line = usize::from(options.show_add_note_badge);
                for line in lines.iter_mut().skip(first_spacer_line) {
                    append_line(line, code_cell_spacer(*add_note_badge_width, background));
                }
            }
            Some(PaintedCodeRow {
                anchor_id: anchor_id.map(str::to_owned),
                row_key: row_key.to_owned(),
                lines,
                add_note_hit: add_note_hit(options, hunk_index, split_add_note_target(&row)),
            })
        }
        (
            DiffRow::StackLine { cell, .. },
            CodeRowLayoutPlan::Stack {
                note_guide_side,
                add_note_badge_width,
                ..
            },
        ) => {
            let cell_highlight = pick_row_highlight(
                selection_highlight,
                cursor_row_highlight,
                has_copy_selection,
                on_cursor_row,
            );
            let emphasized_rail = options.selected || has_copy_selection;
            let guide_on_old_side = *note_guide_side == Some(ReviewSide::Old);
            let guide_on_new_side = *note_guide_side == Some(ReviewSide::New);
            let prefix = CodeCellPrefix {
                text: if guide_on_old_side {
                    "│".into()
                } else {
                    diff_rail_marker().into()
                },
                foreground: if guide_on_old_side {
                    options.theme.note_border.clone()
                } else {
                    stack_rail_color(stack_kind(cell.kind), options.theme, emphasized_rail)
                },
                background: options.theme.panel.clone(),
            };
            let paint_options = CodeCellPaintOptions {
                line_number_digits: options.line_number_digits,
                show_line_numbers: options.show_line_numbers,
                theme: options.theme,
                horizontal_offset: options.code_horizontal_offset,
                guide_on_new_side,
            };
            let mut lines = if options.wrap_lines {
                paint_wrapped_stack_code_cell(
                    &row,
                    &layout,
                    paint_options,
                    &prefix,
                    cell_highlight,
                    0,
                )?
            } else {
                vec![paint_nowrap_stack_code_cell(
                    &row,
                    &layout,
                    paint_options,
                    &prefix,
                    cell_highlight,
                )?]
            };
            if options.show_add_note_badge {
                append_add_note_badge(&mut lines[0], options.theme);
            }
            if options.wrap_lines && *add_note_badge_width > 0 {
                let background = stack_cell_palette(
                    stack_kind(cell.kind),
                    options.theme,
                    cell.move_kind.is_some(),
                )
                .content_background;
                let first_spacer_line = usize::from(options.show_add_note_badge);
                for line in lines.iter_mut().skip(first_spacer_line) {
                    append_line(line, code_cell_spacer(*add_note_badge_width, background));
                }
            }
            Some(PaintedCodeRow {
                anchor_id: anchor_id.map(str::to_owned),
                row_key: row_key.to_owned(),
                lines,
                add_note_hit: add_note_hit(options, hunk_index, stack_add_note_target(&row)),
            })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_diff::{RenderSpan, SplitLineCell, StackLineCell};

    use crate::{
        LineHighlightColRange, LineHighlightPaintIndex, ratatui_theme_color, resolve_theme,
        selection_highlight_background,
    };
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;
    use workdeck_extension_api::HighlightTone;

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn stack_row(text: &str, note_guide_side: Option<ReviewSide>) -> PlannedReviewRow {
        PlannedReviewRow::DiffRow {
            key: "diff-row:precedence".into(),
            stable_key: "line:0:new:1".into(),
            stable_alias_keys: Vec::new(),
            file_id: "paint".into(),
            hunk_index: 0,
            row: DiffRow::StackLine {
                key: "precedence".into(),
                file_id: "paint".into(),
                hunk_index: 0,
                cell: StackLineCell {
                    kind: StackLineKind::Addition,
                    sign: "+".into(),
                    old_line_number: None,
                    new_line_number: Some(1),
                    move_kind: None,
                    spans: vec![span(text)],
                },
                is_expansion_row: false,
                expanded_gap_key: None,
            },
            anchor_id: Some("row-anchor".into()),
            note_guide_side,
        }
    }

    fn split_row(
        left_kind: SplitLineKind,
        right_kind: SplitLineKind,
        note_guide_side: Option<ReviewSide>,
    ) -> PlannedReviewRow {
        PlannedReviewRow::DiffRow {
            key: "diff-row:split".into(),
            stable_key: "line:0:new:8".into(),
            stable_alias_keys: Vec::new(),
            file_id: "paint".into(),
            hunk_index: 2,
            row: DiffRow::SplitLine {
                key: "split".into(),
                file_id: "paint".into(),
                hunk_index: 2,
                left: SplitLineCell {
                    kind: left_kind,
                    sign: if left_kind == SplitLineKind::Deletion {
                        "-"
                    } else {
                        " "
                    }
                    .into(),
                    line_number: Some(7),
                    move_kind: None,
                    spans: vec![span("left")],
                },
                right: SplitLineCell {
                    kind: right_kind,
                    sign: if right_kind == SplitLineKind::Addition {
                        "+"
                    } else {
                        " "
                    }
                    .into(),
                    line_number: Some(8),
                    move_kind: None,
                    spans: vec![span("right")],
                },
                is_expansion_row: false,
                expanded_gap_key: None,
            },
            anchor_id: None,
            note_guide_side,
        }
    }

    fn options<'a>(theme: &'a AppTheme, width: usize) -> CodeRowViewOptions<'a> {
        CodeRowViewOptions {
            width,
            line_number_digits: 1,
            show_line_numbers: false,
            wrap_lines: false,
            code_horizontal_offset: 0,
            theme,
            selected: false,
            copy_selected_row_range: None,
            copy_selected_side: None,
            cursor_highlight: None,
            line_highlights: None,
            show_add_note_badge: false,
            enable_add_note: false,
        }
    }

    fn background_for_text(line: &PaintedCodeCellLine, text: &str) -> Option<String> {
        line.runs
            .iter()
            .find(|run| run.text.contains(text))
            .and_then(|run| run.background.clone())
    }

    #[test]
    fn copy_selection_precedes_cursor_paint_like_the_frozen_baseline() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = stack_row("selected", None);
        let cursor = CursorHighlight {
            stable_key: "line:0:new:1".into(),
            side: ReviewSide::New,
            style: CursorHighlightStyle::Row,
        };
        let painted = paint_code_row(
            &row,
            CodeRowViewOptions {
                copy_selected_row_range: Some(FULL_CODE_CELL_COL_RANGE),
                cursor_highlight: Some(&cursor),
                ..options(&theme, 16)
            },
        )
        .unwrap();
        let base = stack_cell_palette(RowCellKind::Addition, &theme, false).content_background;
        let background = background_for_text(&painted.lines[0], "selected").unwrap();

        assert_eq!(painted.lines[0].text(), "▌+ selected     ");
        assert_eq!(painted.anchor_id.as_deref(), Some("row-anchor"));
        assert_eq!(painted.row_key, "precedence");
        assert_eq!(background, "#2f2b16");
        assert_eq!(background, selection_highlight_background(base, &theme));
        assert_ne!(
            background,
            crate::cursor_line_highlight_background(base, &theme)
        );

        let mut buffer = Buffer::empty(Rect::new(0, 0, 16, 1));
        painted.lines[0]
            .ratatui_line()
            .render(buffer.area, &mut buffer);
        assert_eq!(
            buffer[(3, 0)].bg,
            ratatui_theme_color(&selection_highlight_background(base, &theme))
        );
    }

    #[test]
    fn split_context_cursor_paints_both_halves_but_change_cursor_paints_its_side() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let cursor = CursorHighlight {
            stable_key: "line:0:new:8".into(),
            side: ReviewSide::New,
            style: CursorHighlightStyle::Row,
        };
        let context = paint_code_row(
            &split_row(SplitLineKind::Context, SplitLineKind::Context, None),
            CodeRowViewOptions {
                cursor_highlight: Some(&cursor),
                ..options(&theme, 24)
            },
        )
        .unwrap();
        assert_eq!(
            background_for_text(&context.lines[0], "left"),
            Some(crate::cursor_line_highlight_background(
                &theme.context_bg,
                &theme
            ))
        );
        assert_eq!(
            background_for_text(&context.lines[0], "right"),
            Some(crate::cursor_line_highlight_background(
                &theme.context_bg,
                &theme
            ))
        );

        let changed = paint_code_row(
            &split_row(SplitLineKind::Deletion, SplitLineKind::Addition, None),
            CodeRowViewOptions {
                cursor_highlight: Some(&cursor),
                ..options(&theme, 24)
            },
        )
        .unwrap();
        assert_eq!(
            background_for_text(&changed.lines[0], "left"),
            Some(theme.removed_bg.clone())
        );
        assert_eq!(
            background_for_text(&changed.lines[0], "right"),
            Some(crate::cursor_line_highlight_background(
                &theme.added_bg,
                &theme
            ))
        );
    }

    #[test]
    fn split_copy_side_and_number_cursor_keep_their_exact_paint_scope() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = split_row(SplitLineKind::Deletion, SplitLineKind::Addition, None);
        let selected = paint_code_row(
            &row,
            CodeRowViewOptions {
                copy_selected_row_range: Some(FULL_CODE_CELL_COL_RANGE),
                copy_selected_side: Some(ReviewSide::Old),
                ..options(&theme, 24)
            },
        )
        .unwrap();
        assert_eq!(
            background_for_text(&selected.lines[0], "left"),
            Some(selection_highlight_background(&theme.removed_bg, &theme))
        );
        assert_eq!(
            background_for_text(&selected.lines[0], "right"),
            Some(theme.added_bg.clone())
        );

        let cursor = CursorHighlight {
            stable_key: "line:0:new:8".into(),
            side: ReviewSide::New,
            style: CursorHighlightStyle::Number,
        };
        let numbered = paint_code_row(
            &row,
            CodeRowViewOptions {
                cursor_highlight: Some(&cursor),
                ..options(&theme, 24)
            },
        )
        .unwrap();
        assert_eq!(
            background_for_text(&numbered.lines[0], "right"),
            Some(theme.added_bg.clone())
        );
        assert_eq!(
            numbered.lines[0].runs[3].background,
            Some(crate::cursor_line_highlight_background(
                &theme.panel,
                &theme
            ))
        );
        assert_eq!(
            numbered.lines[0].runs[4].background,
            Some(crate::cursor_line_highlight_background(
                &theme.added_bg,
                &theme
            ))
        );
    }

    #[test]
    fn note_guides_and_add_note_hit_metadata_follow_new_then_old_target_precedence() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let split = paint_code_row(
            &split_row(
                SplitLineKind::Deletion,
                SplitLineKind::Addition,
                Some(ReviewSide::Old),
            ),
            CodeRowViewOptions {
                show_add_note_badge: true,
                enable_add_note: true,
                ..options(&theme, 24)
            },
        )
        .unwrap();
        assert!(split.lines[0].text().starts_with('│'));
        assert!(split.lines[0].text().ends_with("[+]"));
        assert_eq!(split.lines[0].width(), 24);
        assert_eq!(
            split.add_note_hit,
            Some(CodeRowAddNoteHit {
                hunk_index: 2,
                target: Some(CodeRowLineTarget {
                    side: ReviewSide::New,
                    line: 8,
                }),
                visual_line: 0,
                column_start: 21,
                width: 3,
            })
        );

        let mut old_only_row = split_row(
            SplitLineKind::Deletion,
            SplitLineKind::Empty,
            Some(ReviewSide::Old),
        );
        let PlannedReviewRow::DiffRow {
            row: DiffRow::SplitLine { right, .. },
            ..
        } = &mut old_only_row
        else {
            unreachable!();
        };
        right.line_number = None;
        let old_only = paint_code_row(
            &old_only_row,
            CodeRowViewOptions {
                show_add_note_badge: true,
                enable_add_note: true,
                ..options(&theme, 24)
            },
        )
        .unwrap();
        assert_eq!(
            old_only.add_note_hit.and_then(|hit| hit.target),
            Some(CodeRowLineTarget {
                side: ReviewSide::Old,
                line: 7,
            })
        );

        let stack = paint_code_row(
            &stack_row("note", Some(ReviewSide::New)),
            CodeRowViewOptions {
                show_add_note_badge: true,
                enable_add_note: true,
                ..options(&theme, 16)
            },
        )
        .unwrap();
        assert!(stack.lines[0].text().contains("│[+]"));
        assert_eq!(
            stack.add_note_hit.as_ref().and_then(|hit| hit.target),
            Some(CodeRowLineTarget {
                side: ReviewSide::New,
                line: 1,
            })
        );
    }

    #[test]
    fn wrapped_rows_reserve_and_fill_the_add_note_column_on_continuations() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let painted = paint_code_row(
            &stack_row("abcdefghijk", None),
            CodeRowViewOptions {
                wrap_lines: true,
                show_add_note_badge: true,
                enable_add_note: true,
                ..options(&theme, 10)
            },
        )
        .unwrap();
        assert!(painted.lines.len() > 1);
        assert!(painted.lines[0].text().ends_with("[+]"));
        assert!(painted.lines.iter().all(|line| line.width() == 10));
        assert_eq!(
            painted.lines[1].runs.last().unwrap().background.as_deref(),
            Some(theme.added_bg.as_str())
        );

        let hidden_badge = paint_code_row(
            &stack_row("abcdefghijk", None),
            CodeRowViewOptions {
                wrap_lines: true,
                enable_add_note: true,
                ..options(&theme, 10)
            },
        )
        .unwrap();
        assert!(hidden_badge.add_note_hit.is_none());
        assert!(
            hidden_badge
                .lines
                .iter()
                .all(|line| line.width() == 10 && !line.text().contains("[+]"))
        );
    }

    #[test]
    fn extension_line_highlights_are_applied_before_selection_paint() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let highlights = LineHighlightPaintIndex::from_line_ranges([(
            ReviewSide::New,
            1,
            vec![LineHighlightColRange {
                start_col: 0,
                end_col: 3,
                tone: HighlightTone::Warning,
            }],
        )]);
        let marked = paint_code_row(
            &stack_row("marked", None),
            CodeRowViewOptions {
                line_highlights: Some(&highlights),
                ..options(&theme, 16)
            },
        )
        .unwrap();
        assert_ne!(
            background_for_text(&marked.lines[0], "mar"),
            Some(theme.added_bg.clone())
        );

        let selected = paint_code_row(
            &stack_row("marked", None),
            CodeRowViewOptions {
                line_highlights: Some(&highlights),
                copy_selected_row_range: Some(FULL_CODE_CELL_COL_RANGE),
                ..options(&theme, 16)
            },
        )
        .unwrap();
        let selected_background = background_for_text(&selected.lines[0], "mar").unwrap();
        assert_ne!(
            selected_background,
            background_for_text(&marked.lines[0], "mar").unwrap()
        );
    }

    #[test]
    fn non_code_rows_are_not_claimed_by_the_code_row_view() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = PlannedReviewRow::DiffRow {
            key: "header".into(),
            stable_key: "header".into(),
            stable_alias_keys: Vec::new(),
            file_id: "paint".into(),
            hunk_index: 0,
            row: DiffRow::HunkHeader {
                key: "header".into(),
                file_id: "paint".into(),
                hunk_index: 0,
                text: "@@".into(),
            },
            anchor_id: None,
            note_guide_side: None,
        };
        assert!(paint_code_row(&row, options(&theme, 16)).is_none());
    }
}
