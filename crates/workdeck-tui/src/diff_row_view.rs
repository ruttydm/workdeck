//! Focused dispatcher joining metadata and code row painters.
//!
//! This is a native Ratatui reimplementation of Hunk's
//! `src/ui/diff/DiffRowView.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Rendering stays pure; React
//! callback identities become stable host-owned interaction tokens used by
//! the equivalent memoization predicate.

use workdeck_core::ReviewSide;
use workdeck_diff::DiffRow;

use crate::{
    AppTheme, CodeRowViewOptions, CopySelectedRowRange, CursorHighlight, DiffMetaRowViewOptions,
    LineHighlightPaintIndex, PaintedCodeCellLine, PaintedCodeRow, PaintedDiffMetaRow,
    PlannedReviewRow, legacy_planned_diff_row, paint_code_row, paint_diff_meta_row,
};

/// Stable host callback identities replacing React function-reference checks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DiffRowInteractionIdentity {
    pub hover_row: Option<u64>,
    pub start_user_note: Option<u64>,
    pub toggle_gap: Option<u64>,
}

/// Complete inputs accepted by the memoized row facade.
#[derive(Debug, Clone, Copy)]
pub struct DiffRowViewOptions<'a> {
    /// Preferred complete review-stream row.
    pub planned_row: Option<&'a PlannedReviewRow>,
    /// Renderer-only raw fallback outside the shared review stream.
    pub row: Option<&'a DiffRow>,
    pub width: usize,
    pub line_number_digits: usize,
    pub show_line_numbers: bool,
    pub show_hunk_headers: bool,
    pub wrap_lines: bool,
    pub code_horizontal_offset: usize,
    pub theme: &'a AppTheme,
    pub selected: bool,
    pub copy_selected_row_range: Option<&'a CopySelectedRowRange>,
    pub copy_selected_side: Option<ReviewSide>,
    pub cursor_highlight: Option<&'a CursorHighlight>,
    pub line_highlights: Option<&'a LineHighlightPaintIndex>,
    pub anchor_id: Option<&'a str>,
    pub note_guide_side: Option<ReviewSide>,
    pub show_add_note_badge: bool,
    pub interactions: DiffRowInteractionIdentity,
}

/// Complete native row result. Rust's closed [`DiffRow`] union makes Hunk's
/// defensive `Unsupported row.` JSX branch unrepresentable for valid rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaintedDiffRow {
    Metadata(PaintedDiffMetaRow),
    Code(PaintedCodeRow),
}

impl PaintedDiffRow {
    #[must_use]
    pub fn lines(&self) -> &[PaintedCodeCellLine] {
        match self {
            Self::Metadata(row) => std::slice::from_ref(&row.line),
            Self::Code(row) => &row.lines,
        }
    }

    #[must_use]
    pub fn row_key(&self) -> &str {
        match self {
            Self::Metadata(row) => &row.row_key,
            Self::Code(row) => &row.row_key,
        }
    }
}

fn optional_ptr_eq<T>(left: Option<&T>, right: Option<&T>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => std::ptr::eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

/// Native equivalent of Hunk's explicit `memo` comparator.
///
/// Source objects, theme state, selection ranges, cursors, and prepared
/// highlight indices retain referential semantics. Scalar geometry and stable
/// interaction tokens compare by value.
#[must_use]
pub fn same_diff_row_view_inputs(
    previous: DiffRowViewOptions<'_>,
    next: DiffRowViewOptions<'_>,
) -> bool {
    optional_ptr_eq(previous.planned_row, next.planned_row)
        && optional_ptr_eq(previous.row, next.row)
        && previous.width == next.width
        && previous.line_number_digits == next.line_number_digits
        && previous.show_line_numbers == next.show_line_numbers
        && previous.show_hunk_headers == next.show_hunk_headers
        && previous.wrap_lines == next.wrap_lines
        && previous.code_horizontal_offset == next.code_horizontal_offset
        && std::ptr::eq(previous.theme, next.theme)
        && previous.selected == next.selected
        && optional_ptr_eq(
            previous.copy_selected_row_range,
            next.copy_selected_row_range,
        )
        && previous.copy_selected_side == next.copy_selected_side
        && optional_ptr_eq(previous.cursor_highlight, next.cursor_highlight)
        && optional_ptr_eq(previous.line_highlights, next.line_highlights)
        && previous.anchor_id == next.anchor_id
        && previous.note_guide_side == next.note_guide_side
        && previous.show_add_note_badge == next.show_add_note_badge
        && previous.interactions == next.interactions
}

/// Resolve the preferred planned row or raw fallback, then dispatch it to its
/// focused metadata or code painter.
#[must_use]
pub fn paint_diff_row(options: DiffRowViewOptions<'_>) -> Option<PaintedDiffRow> {
    let owned_planned;
    let planned_row = if let Some(planned_row) = options.planned_row {
        planned_row
    } else if let Some(row) = options.row {
        owned_planned = legacy_planned_diff_row(
            row.clone(),
            options.anchor_id.map(str::to_owned),
            options.note_guide_side,
        );
        &owned_planned
    } else {
        return None;
    };
    let row = planned_row.diff_row()?;

    match row {
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => paint_diff_meta_row(
            planned_row,
            DiffMetaRowViewOptions {
                width: options.width,
                theme: options.theme,
                selected: options.selected || options.copy_selected_row_range.is_some(),
                show_hunk_headers: options.show_hunk_headers,
                show_add_note_badge: options.show_add_note_badge,
                enable_gap_toggle: options.interactions.toggle_gap.is_some(),
            },
        )
        .map(PaintedDiffRow::Metadata),
        DiffRow::SplitLine { .. } | DiffRow::StackLine { .. } => paint_code_row(
            planned_row,
            CodeRowViewOptions {
                width: options.width,
                line_number_digits: options.line_number_digits,
                show_line_numbers: options.show_line_numbers,
                wrap_lines: options.wrap_lines,
                code_horizontal_offset: options.code_horizontal_offset,
                theme: options.theme,
                selected: options.selected,
                copy_selected_row_range: options.copy_selected_row_range.copied(),
                copy_selected_side: options.copy_selected_side,
                cursor_highlight: options.cursor_highlight,
                line_highlights: options.line_highlights,
                show_add_note_badge: options.show_add_note_badge,
                enable_add_note: options.interactions.start_user_note.is_some(),
            },
        )
        .map(PaintedDiffRow::Code),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ReviewGapPosition;
    use workdeck_diff::{RenderSpan, StackLineCell, StackLineKind};
    use workdeck_extension_api::HighlightTone;

    use crate::{FULL_CODE_CELL_COL_RANGE, LineHighlightColRange, resolve_theme};

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn stack() -> DiffRow {
        DiffRow::StackLine {
            key: "stack".into(),
            file_id: "file".into(),
            hunk_index: 1,
            cell: StackLineCell {
                kind: StackLineKind::Addition,
                sign: "+".into(),
                old_line_number: None,
                new_line_number: Some(4),
                move_kind: None,
                spans: vec![span("new")],
            },
            is_expansion_row: false,
            expanded_gap_key: None,
        }
    }

    fn collapsed() -> DiffRow {
        DiffRow::Collapsed {
            key: "gap".into(),
            file_id: "file".into(),
            hunk_index: 1,
            text: "8 unchanged lines".into(),
            position: ReviewGapPosition::Trailing,
            old_range: [5, 12],
            new_range: [5, 12],
        }
    }

    fn options<'a>(theme: &'a AppTheme) -> DiffRowViewOptions<'a> {
        DiffRowViewOptions {
            planned_row: None,
            row: None,
            width: 24,
            line_number_digits: 1,
            show_line_numbers: false,
            show_hunk_headers: true,
            wrap_lines: false,
            code_horizontal_offset: 0,
            theme,
            selected: false,
            copy_selected_row_range: None,
            copy_selected_side: None,
            cursor_highlight: None,
            line_highlights: None,
            anchor_id: None,
            note_guide_side: None,
            show_add_note_badge: false,
            interactions: DiffRowInteractionIdentity::default(),
        }
    }

    #[test]
    fn planned_metadata_rows_include_copy_selection_and_gap_actions() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let planned = legacy_planned_diff_row(collapsed(), Some("anchor".into()), None);
        let range = FULL_CODE_CELL_COL_RANGE;
        let painted = paint_diff_row(DiffRowViewOptions {
            planned_row: Some(&planned),
            copy_selected_row_range: Some(&range),
            interactions: DiffRowInteractionIdentity {
                hover_row: Some(1),
                start_user_note: None,
                toggle_gap: Some(2),
            },
            ..options(&theme)
        })
        .unwrap();
        let PaintedDiffRow::Metadata(painted) = painted else {
            panic!("collapsed row did not dispatch to metadata paint");
        };
        assert_eq!(painted.anchor_id.as_deref(), Some("anchor"));
        assert_eq!(
            painted.line.runs[0].foreground.as_deref(),
            Some(theme.line_number_fg.as_str())
        );
        assert_eq!(painted.gap_toggle_hit.unwrap().gap_id, "trailing:1");
    }

    #[test]
    fn raw_code_fallback_carries_anchor_note_guide_and_add_note_capability() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = stack();
        let painted = paint_diff_row(DiffRowViewOptions {
            row: Some(&row),
            anchor_id: Some("legacy-anchor"),
            note_guide_side: Some(ReviewSide::New),
            show_add_note_badge: true,
            interactions: DiffRowInteractionIdentity {
                hover_row: Some(10),
                start_user_note: Some(11),
                toggle_gap: None,
            },
            ..options(&theme)
        })
        .unwrap();
        let PaintedDiffRow::Code(painted) = painted else {
            panic!("stack row did not dispatch to code paint");
        };
        assert_eq!(painted.anchor_id.as_deref(), Some("legacy-anchor"));
        assert!(painted.lines[0].text().contains("│[+]"));
        assert_eq!(painted.add_note_hit.unwrap().target.unwrap().line, 4);
    }

    #[test]
    fn planned_row_takes_precedence_over_raw_fallback() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let planned = legacy_planned_diff_row(collapsed(), None, None);
        let raw = stack();
        assert!(matches!(
            paint_diff_row(DiffRowViewOptions {
                planned_row: Some(&planned),
                row: Some(&raw),
                ..options(&theme)
            }),
            Some(PaintedDiffRow::Metadata(_))
        ));
    }

    #[test]
    fn absent_or_non_diff_planned_rows_return_no_view() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        assert!(paint_diff_row(options(&theme)).is_none());
        let gap = PlannedReviewRow::HunkGap {
            key: "hunk-gap".into(),
            stable_key: "hunk-gap".into(),
            file_id: "file".into(),
            hunk_index: 0,
            height: 1,
        };
        assert!(
            paint_diff_row(DiffRowViewOptions {
                planned_row: Some(&gap),
                ..options(&theme)
            })
            .is_none()
        );
    }

    #[test]
    fn memo_comparator_retains_referential_inputs_and_value_scalars() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = stack();
        let planned = legacy_planned_diff_row(row.clone(), None, None);
        let range = FULL_CODE_CELL_COL_RANGE;
        let cursor = CursorHighlight {
            stable_key: "line:1:new:4".into(),
            style: crate::CursorHighlightStyle::Row,
            side: ReviewSide::New,
        };
        let highlights = LineHighlightPaintIndex::from_line_ranges([(
            ReviewSide::New,
            4,
            vec![LineHighlightColRange {
                start_col: 0,
                end_col: 3,
                tone: HighlightTone::Info,
            }],
        )]);
        let base = DiffRowViewOptions {
            planned_row: Some(&planned),
            row: Some(&row),
            copy_selected_row_range: Some(&range),
            cursor_highlight: Some(&cursor),
            line_highlights: Some(&highlights),
            interactions: DiffRowInteractionIdentity {
                hover_row: Some(1),
                start_user_note: Some(2),
                toggle_gap: Some(3),
            },
            ..options(&theme)
        };
        assert!(same_diff_row_view_inputs(base, base));

        let equal_but_distinct_planned = planned.clone();
        assert!(!same_diff_row_view_inputs(
            base,
            DiffRowViewOptions {
                planned_row: Some(&equal_but_distinct_planned),
                ..base
            }
        ));
        let equal_but_distinct_range = range;
        assert!(!same_diff_row_view_inputs(
            base,
            DiffRowViewOptions {
                copy_selected_row_range: Some(&equal_but_distinct_range),
                ..base
            }
        ));
        assert!(!same_diff_row_view_inputs(
            base,
            DiffRowViewOptions {
                width: base.width + 1,
                ..base
            }
        ));
        assert!(!same_diff_row_view_inputs(
            base,
            DiffRowViewOptions {
                interactions: DiffRowInteractionIdentity {
                    hover_row: Some(99),
                    ..base.interactions
                },
                ..base
            }
        ));
    }
}
