//! Final paint and hit geometry for collapsed gaps and hunk headers.
//!
//! This is a native Ratatui reimplementation of Hunk's
//! `src/ui/diff/DiffMetaRowView.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Nested OpenTUI mouse handlers
//! become explicit host-owned hit records without changing terminal geometry.

use workdeck_diff::DiffRow;
use workdeck_review::review_gap_id;

use crate::{
    AppTheme, CODE_ROW_ADD_NOTE_BADGE_TEXT, CodeRowAddNoteHit, PaintedCodeCellLine,
    PaintedCodeCellRun, PlannedReviewRow, diff_rail_marker, dim_rail_color, fit_planned_row_text,
    measure_text_width, neutral_rail_color, slice_text_by_width,
};

const META_ADD_NOTE_BADGE_PREFIX: &str = " ";

/// Clickable part of an expandable collapsed-gap row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffMetaGapToggleHit {
    pub gap_id: String,
    pub visual_line: usize,
    pub column_start: usize,
    pub width: usize,
}

/// Final metadata-row paint and nested interaction geometry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedDiffMetaRow {
    pub anchor_id: Option<String>,
    pub row_key: String,
    pub line: PaintedCodeCellLine,
    pub gap_toggle_hit: Option<DiffMetaGapToggleHit>,
    pub add_note_hit: Option<CodeRowAddNoteHit>,
}

/// Shell-owned state affecting one collapsed gap or hunk header.
#[derive(Debug, Clone, Copy)]
pub struct DiffMetaRowViewOptions<'a> {
    pub width: usize,
    pub theme: &'a AppTheme,
    pub selected: bool,
    pub show_hunk_headers: bool,
    pub show_add_note_badge: bool,
    /// Corresponds to the presence of Hunk's `onToggleGap` callback.
    pub enable_gap_toggle: bool,
}

fn append_run(
    line: &mut PaintedCodeCellLine,
    text: impl Into<String>,
    foreground: Option<String>,
    background: Option<String>,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    if let Some(previous) = line
        .runs
        .last_mut()
        .filter(|previous| previous.foreground == foreground && previous.background == background)
    {
        previous.text.push_str(&text);
    } else {
        line.runs.push(PaintedCodeCellRun {
            text,
            foreground,
            background,
        });
    }
}

/// Build the pinned label for one collapsed gap.
#[must_use]
pub fn collapsed_diff_meta_row_label(text: &str, expandable: bool) -> String {
    if expandable {
        format!("▾ {text}")
    } else {
        format!("··· {text} ···")
    }
}

/// Paint one collapsed gap or hunk header. Code rows and hidden headers are
/// deliberately left for the owning dispatcher.
#[must_use]
pub fn paint_diff_meta_row(
    planned_row: &PlannedReviewRow,
    options: DiffMetaRowViewOptions<'_>,
) -> Option<PaintedDiffMetaRow> {
    let PlannedReviewRow::DiffRow { row, anchor_id, .. } = planned_row else {
        return None;
    };
    if matches!(row, DiffRow::HunkHeader { .. }) && !options.show_hunk_headers {
        return None;
    }

    let (row_key, hunk_index, text, collapsed) = match row {
        DiffRow::Collapsed {
            key,
            hunk_index,
            text,
            position,
            ..
        } => (key, *hunk_index, text.as_str(), Some(*position)),
        DiffRow::HunkHeader {
            key,
            hunk_index,
            text,
            ..
        } => (key, *hunk_index, text.as_str(), None),
        DiffRow::SplitLine { .. } | DiffRow::StackLine { .. } => return None,
    };
    let expandable = collapsed.is_some() && options.enable_gap_toggle;
    let label_text = collapsed.map_or_else(
        || text.to_owned(),
        |_| collapsed_diff_meta_row_label(text, expandable),
    );
    let badge_text = format!("{META_ADD_NOTE_BADGE_PREFIX}{CODE_ROW_ADD_NOTE_BADGE_TEXT}");
    let badge_width = if options.show_add_note_badge {
        measure_text_width(&badge_text)
    } else {
        0
    };
    let content_width = options.width.saturating_sub(badge_width);
    let label_width = options
        .width
        .saturating_sub(1_usize.saturating_add(badge_width));
    let label = fit_planned_row_text(&label_text, label_width);
    let mut line = PaintedCodeCellLine::default();

    if content_width > 0 {
        append_run(
            &mut line,
            diff_rail_marker(),
            Some(if options.selected {
                neutral_rail_color(options.theme).to_owned()
            } else {
                dim_rail_color(neutral_rail_color(options.theme), options.theme)
            }),
            Some(options.theme.panel_alt.clone()),
        );
        append_run(
            &mut line,
            label,
            Some(if collapsed.is_some() {
                options.theme.muted.clone()
            } else {
                options.theme.badge_neutral.clone()
            }),
            Some(options.theme.panel_alt.clone()),
        );
        let padding = content_width.saturating_sub(line.width());
        append_run(
            &mut line,
            " ".repeat(padding),
            None,
            Some(options.theme.panel_alt.clone()),
        );
    }

    let visible_badge_width = options.width.saturating_sub(content_width);
    if options.show_add_note_badge && visible_badge_width > 0 {
        append_run(
            &mut line,
            slice_text_by_width(&badge_text, 0, visible_badge_width).text,
            Some(options.theme.note_title_text.clone()),
            Some(options.theme.note_title_background.clone()),
        );
    }

    let gap_toggle_hit = collapsed
        .filter(|_| options.enable_gap_toggle)
        .map(|position| DiffMetaGapToggleHit {
            gap_id: review_gap_id(position, hunk_index),
            visual_line: 0,
            column_start: 0,
            width: content_width,
        });
    let add_note_hit = options.show_add_note_badge.then_some(CodeRowAddNoteHit {
        hunk_index,
        target: None,
        visual_line: 0,
        column_start: content_width,
        width: visible_badge_width,
    });
    Some(PaintedDiffMetaRow {
        anchor_id: anchor_id.clone(),
        row_key: row_key.clone(),
        line,
        gap_toggle_hit,
        add_note_hit,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::ReviewGapPosition;

    use crate::{ratatui_theme_color, resolve_theme};
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    fn meta_row(row: DiffRow, anchor_id: Option<&str>) -> PlannedReviewRow {
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
            } => (key.clone(), file_id.clone(), *hunk_index),
            DiffRow::SplitLine { .. } | DiffRow::StackLine { .. } => unreachable!(),
        };
        PlannedReviewRow::DiffRow {
            stable_key: key.clone(),
            key,
            stable_alias_keys: Vec::new(),
            file_id,
            hunk_index,
            row,
            anchor_id: anchor_id.map(str::to_owned),
            note_guide_side: None,
        }
    }

    fn collapsed() -> PlannedReviewRow {
        meta_row(
            DiffRow::Collapsed {
                key: "gap".into(),
                file_id: "file".into(),
                hunk_index: 3,
                text: "12 unchanged lines".into(),
                position: ReviewGapPosition::Before,
                old_range: [1, 12],
                new_range: [1, 12],
            },
            Some("gap-anchor"),
        )
    }

    fn header() -> PlannedReviewRow {
        meta_row(
            DiffRow::HunkHeader {
                key: "header".into(),
                file_id: "file".into(),
                hunk_index: 3,
                text: "@@ -13,2 +13,2 @@".into(),
            },
            None,
        )
    }

    fn options<'a>(theme: &'a AppTheme, width: usize) -> DiffMetaRowViewOptions<'a> {
        DiffMetaRowViewOptions {
            width,
            theme,
            selected: false,
            show_hunk_headers: true,
            show_add_note_badge: false,
            enable_gap_toggle: false,
        }
    }

    #[test]
    fn collapsed_label_switches_between_static_ellipsis_and_interactive_chevron() {
        assert_eq!(
            collapsed_diff_meta_row_label("12 unchanged lines", false),
            "··· 12 unchanged lines ···"
        );
        assert_eq!(
            collapsed_diff_meta_row_label("12 unchanged lines", true),
            "▾ 12 unchanged lines"
        );
    }

    #[test]
    fn static_collapsed_rows_fill_the_width_with_dimmed_rail_and_muted_label() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let painted = paint_diff_meta_row(&collapsed(), options(&theme, 32)).unwrap();
        assert_eq!(painted.anchor_id.as_deref(), Some("gap-anchor"));
        assert_eq!(painted.row_key, "gap");
        assert_eq!(painted.line.text(), "▌··· 12 unchanged lines ···     ");
        assert_eq!(painted.line.width(), 32);
        assert_eq!(
            painted.line.runs[0].foreground,
            Some(dim_rail_color(&theme.line_number_fg, &theme))
        );
        assert_eq!(painted.line.runs[1].foreground, Some(theme.muted.clone()));
        assert!(painted.gap_toggle_hit.is_none());

        let mut buffer = Buffer::empty(Rect::new(0, 0, 32, 1));
        painted.line.ratatui_line().render(buffer.area, &mut buffer);
        assert!(
            buffer
                .content
                .iter()
                .all(|cell| { cell.bg == ratatui_theme_color(&theme.panel_alt) })
        );
    }

    #[test]
    fn expandable_gap_exposes_the_exact_review_gap_hit_without_claiming_the_badge() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let painted = paint_diff_meta_row(
            &collapsed(),
            DiffMetaRowViewOptions {
                selected: true,
                show_add_note_badge: true,
                enable_gap_toggle: true,
                ..options(&theme, 24)
            },
        )
        .unwrap();
        assert_eq!(painted.line.text(), "▌▾ 12 unchanged lin… [+]");
        assert_eq!(painted.line.width(), 24);
        assert_eq!(
            painted.line.runs[0].foreground.as_deref(),
            Some(theme.line_number_fg.as_str())
        );
        assert_eq!(
            painted.gap_toggle_hit,
            Some(DiffMetaGapToggleHit {
                gap_id: "before:3".into(),
                visual_line: 0,
                column_start: 0,
                width: 20,
            })
        );
        assert_eq!(
            painted.add_note_hit,
            Some(CodeRowAddNoteHit {
                hunk_index: 3,
                target: None,
                visual_line: 0,
                column_start: 20,
                width: 4,
            })
        );
    }

    #[test]
    fn hunk_headers_obey_visibility_and_use_the_neutral_badge_color() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        assert!(
            paint_diff_meta_row(
                &header(),
                DiffMetaRowViewOptions {
                    show_hunk_headers: false,
                    ..options(&theme, 24)
                }
            )
            .is_none()
        );
        let painted = paint_diff_meta_row(&header(), options(&theme, 24)).unwrap();
        assert_eq!(painted.line.text(), "▌@@ -13,2 +13,2 @@      ");
        assert_eq!(
            painted.line.runs[1].foreground.as_deref(),
            Some(theme.badge_neutral.as_str())
        );
        assert!(painted.gap_toggle_hit.is_none());
    }

    #[test]
    fn labels_are_sanitized_truncated_and_clipped_to_narrow_terminal_widths() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut row = header();
        let PlannedReviewRow::DiffRow {
            row: DiffRow::HunkHeader { text, .. },
            ..
        } = &mut row
        else {
            unreachable!();
        };
        *text = "日本語\u{1b}[2J tail".into();
        let painted = paint_diff_meta_row(&row, options(&theme, 6)).unwrap();
        assert_eq!(painted.line.text(), "▌日本…");
        assert_eq!(painted.line.width(), 6);

        let badge_only = paint_diff_meta_row(
            &row,
            DiffMetaRowViewOptions {
                show_add_note_badge: true,
                ..options(&theme, 2)
            },
        )
        .unwrap();
        assert_eq!(badge_only.line.text(), " [");
        assert_eq!(badge_only.line.width(), 2);
    }

    #[test]
    fn code_and_inline_note_rows_are_not_claimed() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = PlannedReviewRow::HunkGap {
            key: "hunk-gap".into(),
            stable_key: "hunk-gap".into(),
            file_id: "file".into(),
            hunk_index: 0,
            height: 1,
        };
        assert!(paint_diff_meta_row(&row, options(&theme, 20)).is_none());
    }
}
