//! Exact split-row paint shared with native extension panes.

use std::sync::Arc;

use workdeck_core::ReviewSide;
use workdeck_diff::{DiffRow, SplitLineCell, SplitLineKind, StackLineCell, StackLineKind};

use crate::{AppTheme, LineCursor};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentLinePlannedRow {
    pub stable_key: String,
    pub stable_alias_keys: Vec<String>,
    pub row: DiffRow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentLineRowPlan {
    pub planned_rows: Vec<CurrentLinePlannedRow>,
    pub line_number_digits: usize,
}

/// The complete Ratatui row-render input produced when an extension requests one side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCurrentLineRenderedRow {
    pub row: DiffRow,
    pub width: usize,
    pub line_number_digits: usize,
    pub show_line_numbers: bool,
    pub show_hunk_headers: bool,
    pub wrap_lines: bool,
    pub code_horizontal_offset: usize,
    pub theme: AppTheme,
    pub selected: bool,
}

/// Host-owned current-line painter. Extensions receive only its public source address and output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCurrentLinePaint {
    pub side: ReviewSide,
    pub line: u32,
    rows: [DiffRow; 2],
    line_number_digits: usize,
    show_line_numbers: bool,
    code_horizontal_offset: usize,
    theme: AppTheme,
}

impl ExtensionCurrentLinePaint {
    #[must_use]
    pub fn render(&self, side: ReviewSide, width: usize) -> ExtensionCurrentLineRenderedRow {
        ExtensionCurrentLineRenderedRow {
            row: self.rows[usize::from(side == ReviewSide::New)].clone(),
            width,
            line_number_digits: self.line_number_digits,
            show_line_numbers: self.show_line_numbers,
            show_hunk_headers: false,
            wrap_lines: false,
            code_horizontal_offset: self.code_horizontal_offset,
            theme: self.theme.clone(),
            selected: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionCurrentLinePaintStatus {
    Unavailable,
    Pending,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionCurrentLinePaintState {
    pub status: ExtensionCurrentLinePaintStatus,
    pub file_id: Option<String>,
    pub cursor_key: Option<String>,
    pub paint: Option<ExtensionCurrentLinePaint>,
}

impl Default for ExtensionCurrentLinePaintState {
    fn default() -> Self {
        Self {
            status: ExtensionCurrentLinePaintStatus::Unavailable,
            file_id: None,
            cursor_key: None,
            paint: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionCurrentLinePaintUpdate {
    Unavailable,
    Pending,
    Ready {
        file_id: String,
        cursor_key: String,
        paint: Box<ExtensionCurrentLinePaint>,
    },
}

impl ExtensionCurrentLinePaintUpdate {
    fn status(&self) -> ExtensionCurrentLinePaintStatus {
        match self {
            Self::Unavailable => ExtensionCurrentLinePaintStatus::Unavailable,
            Self::Pending => ExtensionCurrentLinePaintStatus::Pending,
            Self::Ready { .. } => ExtensionCurrentLinePaintStatus::Ready,
        }
    }
}

/// Match accepted paint only to the exact file-scoped cursor identity that produced it.
#[must_use]
pub fn extension_current_line_paint_matches_cursor(
    state: &ExtensionCurrentLinePaintState,
    cursor: Option<(&str, &str)>,
) -> bool {
    state.status == ExtensionCurrentLinePaintStatus::Ready
        && state.file_id.as_deref() == cursor.map(|cursor| cursor.0)
        && state.cursor_key.as_deref() == cursor.map(|cursor| cursor.1)
}

/// Apply one renderer lifecycle update, retaining Arc identity for an equal empty state.
#[must_use]
pub fn apply_extension_current_line_paint_update(
    current: &Arc<ExtensionCurrentLinePaintState>,
    update: ExtensionCurrentLinePaintUpdate,
) -> Arc<ExtensionCurrentLinePaintState> {
    if let ExtensionCurrentLinePaintUpdate::Ready {
        file_id,
        cursor_key,
        paint,
    } = update
    {
        return Arc::new(ExtensionCurrentLinePaintState {
            status: ExtensionCurrentLinePaintStatus::Ready,
            file_id: Some(file_id),
            cursor_key: Some(cursor_key),
            paint: Some(*paint),
        });
    }
    if current.status == update.status() && current.paint.is_none() {
        return Arc::clone(current);
    }
    Arc::new(ExtensionCurrentLinePaintState {
        status: update.status(),
        file_id: None,
        cursor_key: None,
        paint: None,
    })
}

fn stack_row(row: &DiffRow, cell: &SplitLineCell, side: ReviewSide) -> DiffRow {
    let DiffRow::SplitLine {
        key,
        file_id,
        hunk_index,
        ..
    } = row
    else {
        unreachable!("current-line paint adapts only split rows")
    };
    let kind = match cell.kind {
        SplitLineKind::Context | SplitLineKind::Empty => StackLineKind::Context,
        SplitLineKind::Addition => StackLineKind::Addition,
        SplitLineKind::Deletion => StackLineKind::Deletion,
    };
    DiffRow::StackLine {
        key: format!(
            "{key}:pane:{}",
            if side == ReviewSide::Old {
                "old"
            } else {
                "new"
            }
        ),
        file_id: file_id.clone(),
        hunk_index: *hunk_index,
        cell: StackLineCell {
            kind,
            sign: if cell.kind == SplitLineKind::Empty {
                " ".into()
            } else {
                cell.sign.clone()
            },
            old_line_number: (side == ReviewSide::Old)
                .then_some(cell.line_number)
                .flatten(),
            new_line_number: (side == ReviewSide::New)
                .then_some(cell.line_number)
                .flatten(),
            move_kind: cell.move_kind,
            spans: cell.spans.clone(),
        },
        is_expansion_row: false,
        expanded_gap_key: None,
    }
}

/// Build a public current-line painter from the exact accepted split-row plan.
#[must_use]
pub fn create_extension_current_line_paint(
    cursor: &LineCursor,
    row_plan: &CurrentLineRowPlan,
    show_line_numbers: bool,
    code_horizontal_offset: usize,
    theme: &AppTheme,
) -> Option<ExtensionCurrentLinePaint> {
    let split_row = row_plan.planned_rows.iter().find_map(|planned| {
        let matches = planned.stable_key == cursor.stable_key
            || planned
                .stable_alias_keys
                .iter()
                .any(|alias| alias == &cursor.stable_key);
        (matches && matches!(planned.row, DiffRow::SplitLine { .. })).then_some(&planned.row)
    })?;
    let DiffRow::SplitLine { left, right, .. } = split_row else {
        unreachable!()
    };
    Some(ExtensionCurrentLinePaint {
        side: cursor.target.side,
        line: cursor.target.line,
        rows: [
            stack_row(split_row, left, ReviewSide::Old),
            stack_row(split_row, right, ReviewSide::New),
        ],
        line_number_digits: row_plan.line_number_digits,
        show_line_numbers,
        code_horizontal_offset,
        theme: theme.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ReviewLineMoveKind;
    use workdeck_diff::{RenderSpan, SplitLineCell};

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn fixture() -> (LineCursor, CurrentLineRowPlan, AppTheme) {
        let stable_key = "line:0:old:1";
        let row = DiffRow::SplitLine {
            key: "split:0".into(),
            file_id: "alpha".into(),
            hunk_index: 0,
            left: SplitLineCell {
                kind: SplitLineKind::Deletion,
                sign: "-".into(),
                line_number: Some(1),
                move_kind: None,
                spans: vec![span("const value = 1;")],
            },
            right: SplitLineCell {
                kind: SplitLineKind::Addition,
                sign: "+".into(),
                line_number: Some(1),
                move_kind: None,
                spans: vec![span("const value = 222;")],
            },
            is_expansion_row: false,
            expanded_gap_key: None,
        };
        (
            LineCursor {
                file_id: "alpha".into(),
                hunk_index: 0,
                stable_key: stable_key.into(),
                target: crate::LineCursorTarget {
                    side: ReviewSide::New,
                    line: 1,
                },
                expanded_gap_key: None,
            },
            CurrentLineRowPlan {
                planned_rows: vec![CurrentLinePlannedRow {
                    stable_key: stable_key.into(),
                    stable_alias_keys: vec!["line:0:new:1".into()],
                    row,
                }],
                line_number_digits: 3,
            },
            crate::resolve_theme(Some("github-dark-default"), None, &[]),
        )
    }

    #[test]
    fn frozen_hunk_extension_current_line_oracles_cover_both_pinned_trees() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/extension-current-line.json"
        ))
        .unwrap();
        assert_eq!(oracle["baselineOracle"]["passed"], 5);
        assert_eq!(oracle["baselineOracle"]["expectations"], 18);
        assert_eq!(oracle["stableOracle"]["passed"], 5);
        assert_eq!(oracle["stableOracle"]["expectations"], 16);
        assert_eq!(oracle["authoritative"], "baseline");
    }

    #[test]
    fn painter_exposes_source_address_and_exact_no_wrap_renderer_inputs() {
        let (cursor, plan, theme) = fixture();
        let paint = create_extension_current_line_paint(&cursor, &plan, true, 0, &theme).unwrap();
        assert_eq!(paint.side, cursor.target.side);
        assert_eq!(paint.line, cursor.target.line);
        let old = paint.render(ReviewSide::Old, 60);
        let new = paint.render(ReviewSide::New, 60);
        let DiffRow::StackLine { cell: old, .. } = old.row else {
            panic!("expected stack row")
        };
        let DiffRow::StackLine { cell: new, .. } = new.row else {
            panic!("expected stack row")
        };
        assert_eq!(old.kind, StackLineKind::Deletion);
        assert_eq!(old.old_line_number, Some(1));
        assert_eq!(old.spans, vec![span("const value = 1;")]);
        assert_eq!(new.kind, StackLineKind::Addition);
        assert_eq!(new.new_line_number, Some(1));
        assert_eq!(old.sign, "-");
        assert_eq!(new.sign, "+");
        let rendered = paint.render(ReviewSide::New, 42);
        assert_eq!(rendered.width, 42);
        assert!(rendered.show_line_numbers);
        assert!(!rendered.show_hunk_headers);
        assert!(!rendered.wrap_lines);
        assert_eq!(rendered.code_horizontal_offset, 0);
        assert!(!rendered.selected);
    }

    #[test]
    fn absent_side_becomes_blank_context_and_move_paint_survives() {
        let (cursor, mut plan, theme) = fixture();
        let DiffRow::SplitLine { left, right, .. } = &mut plan.planned_rows[0].row else {
            unreachable!()
        };
        *left = SplitLineCell {
            kind: SplitLineKind::Empty,
            sign: " ".into(),
            line_number: None,
            move_kind: None,
            spans: Vec::new(),
        };
        right.move_kind = Some(ReviewLineMoveKind::Moved);
        let paint = create_extension_current_line_paint(&cursor, &plan, false, 17, &theme).unwrap();
        let old = paint.render(ReviewSide::Old, 42);
        let new = paint.render(ReviewSide::New, 42);
        let DiffRow::StackLine { cell: old, .. } = old.row else {
            unreachable!()
        };
        let DiffRow::StackLine { cell: new, .. } = new.row else {
            unreachable!()
        };
        assert_eq!(old.kind, StackLineKind::Context);
        assert_eq!(old.sign, " ");
        assert!(old.spans.is_empty());
        assert_eq!(new.move_kind, Some(ReviewLineMoveKind::Moved));
        assert_eq!(paint.render(ReviewSide::New, 42).code_horizontal_offset, 17);
    }

    #[test]
    fn missing_cursor_key_returns_no_paint_while_aliases_resolve() {
        let (mut cursor, plan, theme) = fixture();
        cursor.stable_key = "missing".into();
        assert!(create_extension_current_line_paint(&cursor, &plan, true, 0, &theme).is_none());
        cursor.stable_key = "line:0:new:1".into();
        assert!(create_extension_current_line_paint(&cursor, &plan, true, 0, &theme).is_some());
    }

    #[test]
    fn pending_state_withholds_stale_paint_and_reuses_equal_empty_state() {
        let (cursor, plan, theme) = fixture();
        let paint = create_extension_current_line_paint(&cursor, &plan, true, 0, &theme).unwrap();
        let unavailable = Arc::new(ExtensionCurrentLinePaintState::default());
        let ready = apply_extension_current_line_paint_update(
            &unavailable,
            ExtensionCurrentLinePaintUpdate::Ready {
                file_id: "alpha".into(),
                cursor_key: "row:1".into(),
                paint: Box::new(paint),
            },
        );
        assert!(!extension_current_line_paint_matches_cursor(
            &ready,
            Some(("beta", "row:1"))
        ));
        assert!(extension_current_line_paint_matches_cursor(
            &ready,
            Some(("alpha", "row:1"))
        ));
        let pending = apply_extension_current_line_paint_update(
            &ready,
            ExtensionCurrentLinePaintUpdate::Pending,
        );
        assert_eq!(pending.status, ExtensionCurrentLinePaintStatus::Pending);
        assert_eq!(pending.file_id, None);
        assert_eq!(pending.cursor_key, None);
        assert_eq!(pending.paint, None);
        let still_pending = apply_extension_current_line_paint_update(
            &pending,
            ExtensionCurrentLinePaintUpdate::Pending,
        );
        assert!(Arc::ptr_eq(&pending, &still_pending));
    }

    #[test]
    fn unavailable_update_clears_every_accepted_paint_field() {
        let (cursor, plan, theme) = fixture();
        let paint = create_extension_current_line_paint(&cursor, &plan, true, 0, &theme).unwrap();
        let ready = Arc::new(ExtensionCurrentLinePaintState {
            status: ExtensionCurrentLinePaintStatus::Ready,
            file_id: Some("alpha".into()),
            cursor_key: Some("row:1".into()),
            paint: Some(paint),
        });
        let unavailable = apply_extension_current_line_paint_update(
            &ready,
            ExtensionCurrentLinePaintUpdate::Unavailable,
        );
        assert_eq!(
            unavailable.as_ref(),
            &ExtensionCurrentLinePaintState::default()
        );
    }
}
