//! Responsive theme selector modal, list windowing, and pointer geometry.
//!
//! This ports Hunk's `src/ui/components/chrome/ThemeSelectorDialog.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Ratatui owns the cell buffer;
//! the application controller owns selection, delayed previews, and commits.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    AppTheme, ModalFrameOptions, ModalFramePlan, fit_text, list_window_start, measure_text_width,
    pad_text, ratatui_theme_color, render_modal_frame,
};

pub const THEME_HOVER_PREVIEW_DELAY_MS: u64 = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSelectorItem {
    pub id: String,
    pub label: String,
    pub description: String,
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeSelectorWindowState {
    pub item_count: usize,
    pub selected_index: usize,
    pub visible_rows: usize,
    pub window_start: usize,
}

impl ThemeSelectorWindowState {
    #[must_use]
    pub fn initial(selected_index: usize, item_count: usize, visible_rows: usize) -> Self {
        Self {
            item_count,
            selected_index,
            visible_rows,
            window_start: list_window_start(selected_index, item_count, visible_rows),
        }
    }
}

/// Keep the selected row visible while preserving an independently scrolled
/// window whenever its geometry and catalog still agree.
#[must_use]
pub fn synchronize_theme_selector_window(
    current: ThemeSelectorWindowState,
    selected_index: usize,
    item_count: usize,
    visible_rows: usize,
) -> ThemeSelectorWindowState {
    if current.selected_index == selected_index
        && current.item_count == item_count
        && current.visible_rows == visible_rows
    {
        return current;
    }

    let max_window_start = item_count.saturating_sub(visible_rows);
    let clamped = current.window_start.min(max_window_start);
    let window_start = if selected_index < clamped {
        selected_index
    } else if selected_index >= clamped.saturating_add(visible_rows) {
        selected_index
            .saturating_sub(visible_rows.saturating_sub(1))
            .min(max_window_start)
    } else {
        clamped
    };
    ThemeSelectorWindowState {
        item_count,
        selected_index,
        visible_rows,
        window_start,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSelectorItemHit {
    pub index: usize,
    pub id: String,
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThemeSelectorDialogPlan {
    pub modal: ModalFramePlan,
    pub requested_width: u16,
    pub requested_height: u16,
    pub body_width: usize,
    pub visible_rows: usize,
    pub window: ThemeSelectorWindowState,
    pub help_row: Rect,
    pub item_hits: Vec<ThemeSelectorItemHit>,
    pub more_row: Option<Rect>,
    pub remaining_items: usize,
}

fn clipped_row(area: Rect, x: u16, y: u16, width: u16) -> Rect {
    if y >= area.bottom() || x >= area.right() {
        return Rect::new(x.min(area.right()), y.min(area.bottom()), 0, 0);
    }
    Rect::new(x, y, width.min(area.right().saturating_sub(x)), 1)
}

fn paint_transparent_text(buffer: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut x = area.x;
    for grapheme in text.graphemes(true) {
        let width = u16::try_from(measure_text_width(grapheme)).unwrap_or(u16::MAX);
        if width == 0 {
            continue;
        }
        if x.saturating_add(width) > area.right() {
            break;
        }
        if !grapheme.chars().all(char::is_whitespace) {
            buffer.set_stringn(x, area.y, grapheme, usize::from(width), style);
        }
        x = x.saturating_add(width);
    }
}

/// Paint the source modal for one controller-owned selector state.
#[allow(clippy::too_many_arguments)]
pub fn render_theme_selector_dialog(
    area: Rect,
    buffer: &mut Buffer,
    items: &[ThemeSelectorItem],
    selected_index: usize,
    previous_window: Option<ThemeSelectorWindowState>,
    theme: &AppTheme,
) -> ThemeSelectorDialogPlan {
    let requested_width = 82_u16.min(56_u16.max(area.width.saturating_sub(8)));
    let requested_height = 28_u16.min(12_u16.max(area.height.saturating_sub(4)));
    let body_width = usize::from(requested_width.saturating_sub(4).max(1));
    let visible_rows = usize::from(requested_height.saturating_sub(7)).max(4);
    let selected_index = selected_index.min(items.len().saturating_sub(1));
    let window = previous_window.map_or_else(
        || ThemeSelectorWindowState::initial(selected_index, items.len(), visible_rows),
        |current| {
            synchronize_theme_selector_window(current, selected_index, items.len(), visible_rows)
        },
    );
    let modal = render_modal_frame(
        area,
        buffer,
        ModalFrameOptions {
            width: requested_width,
            height: requested_height,
            closeable: true,
            has_mouse_scroll_handler: true,
            terminal_width: area.width,
            terminal_height: area.height,
            theme,
            title: "Theme selector",
        },
    );
    // OpenTUI flexes away the frame's post-title spacer when the requested
    // selector is clamped vertically, while its fixed children keep
    // overflowing until the terminal boundary.
    let body_y = if modal.frame.height < requested_height {
        modal.frame.y.saturating_add(3)
    } else {
        modal.content.y
    };
    let row_x = modal.frame.x.saturating_add(2);
    let row_width = modal.frame.width.saturating_sub(4);
    let help_row = clipped_row(area, row_x, body_y, row_width);
    paint_transparent_text(
        buffer,
        help_row,
        &fit_text("Enter/click accept  Esc cancel", body_width, None),
        Style::default()
            .fg(ratatui_theme_color(&theme.muted))
            .bg(ratatui_theme_color(&theme.panel)),
    );

    let marker_width = 3_usize;
    let description_width = 12_usize;
    let label_width = body_width
        .saturating_sub(marker_width + description_width + 2)
        .max(8);
    let mut item_hits = Vec::new();
    for (offset, item) in items
        .iter()
        .enumerate()
        .skip(window.window_start)
        .take(visible_rows)
    {
        let index = offset;
        let row_offset = index.saturating_sub(window.window_start);
        let y = body_y
            .saturating_add(2)
            .saturating_add(u16::try_from(row_offset).unwrap_or(u16::MAX));
        let bounds = clipped_row(area, row_x, y, row_width);
        if bounds.width == 0 || bounds.height == 0 {
            continue;
        }
        let selected = index == selected_index;
        let marker = if selected {
            "›"
        } else if item.active {
            "✓"
        } else {
            " "
        };
        let background = ratatui_theme_color(if selected {
            &theme.accent_muted
        } else {
            &theme.panel
        });
        let foreground = ratatui_theme_color(if selected {
            &theme.text
        } else if item.active {
            &theme.badge_neutral
        } else {
            &theme.muted
        });
        for x in bounds.x..bounds.right() {
            buffer[(x, bounds.y)].set_symbol(" ");
        }
        buffer.set_style(bounds, Style::default().bg(background));
        Paragraph::new(Line::from(vec![
            Span::styled(
                pad_text(marker, marker_width),
                Style::default().fg(foreground).bg(background),
            ),
            Span::styled(
                pad_text(&fit_text(&item.label, label_width, None), label_width),
                Style::default().fg(foreground).bg(background),
            ),
            Span::styled(
                fit_text(&item.description, description_width, None),
                Style::default()
                    .fg(ratatui_theme_color(&theme.muted))
                    .bg(background),
            ),
        ]))
        .render(bounds, buffer);
        item_hits.push(ThemeSelectorItemHit {
            index,
            id: item.id.clone(),
            bounds,
        });
    }

    let rendered_end = window.window_start.saturating_add(visible_rows);
    let remaining_items = items.len().saturating_sub(rendered_end);
    let more_row = (remaining_items > 0).then(|| {
        clipped_row(
            area,
            row_x,
            body_y
                .saturating_add(2)
                .saturating_add(u16::try_from(visible_rows).unwrap_or(u16::MAX)),
            row_width,
        )
    });
    if let Some(more_row) = more_row {
        paint_transparent_text(
            buffer,
            more_row,
            &fit_text(&format!("… {remaining_items} more"), body_width, None),
            Style::default()
                .fg(ratatui_theme_color(&theme.muted))
                .bg(ratatui_theme_color(&theme.panel)),
        );
    }

    ThemeSelectorDialogPlan {
        modal,
        requested_width,
        requested_height,
        body_width,
        visible_rows,
        window,
        help_row,
        item_hits,
        more_row,
        remaining_items,
    }
}

#[must_use]
pub fn theme_selector_item_at(
    plan: &ThemeSelectorDialogPlan,
    column: u16,
    row: u16,
) -> Option<&ThemeSelectorItemHit> {
    plan.item_hits.iter().find(|hit| {
        column >= hit.bounds.x
            && column < hit.bounds.right()
            && row >= hit.bounds.y
            && row < hit.bounds.bottom()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_theme;

    fn items() -> Vec<ThemeSelectorItem> {
        (0..20)
            .map(|index| ThemeSelectorItem {
                id: format!("theme-{index:02}"),
                label: format!("Theme {index:02}"),
                description: if index == 2 { "active" } else { "" }.into(),
                active: index == 2,
            })
            .collect()
    }

    fn frame(buffer: &Buffer, width: u16) -> String {
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_specimen(
        width: u16,
        height: u16,
        previous_window: Option<ThemeSelectorWindowState>,
    ) -> (Buffer, ThemeSelectorDialogPlan) {
        let area = Rect::new(0, 0, width, height);
        let mut buffer = Buffer::empty(area);
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let plan =
            render_theme_selector_dialog(area, &mut buffer, &items(), 10, previous_window, &theme);
        (buffer, plan)
    }

    #[test]
    fn frozen_oracle_records_both_pins_stable_fix_frames_and_application_tests() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/theme-selector-dialog.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["source"]["baselineAndStableBlob"],
            "f881315024b48f465e13f015c3041bcc3810c06e"
        );
        assert_eq!(
            oracle["stableFix"]["commit"],
            "e76dbcc8acc2d17ba0bd0340b1e1b4102c736985"
        );
        assert_eq!(oracle["directOracle"]["eachPin"]["passed"], 4);
        assert_eq!(
            oracle["directOracle"]["frames"].as_array().unwrap().len(),
            3
        );
        assert_eq!(
            oracle["baselineAndStableApplicationOracle"]["tests"]
                .as_array()
                .unwrap()
                .len(),
            7
        );
    }

    #[test]
    fn window_state_centers_once_preserves_scroll_and_reveals_selection() {
        let initial = ThemeSelectorWindowState::initial(10, 20, 13);
        assert_eq!(initial.window_start, 4);
        assert_eq!(
            synchronize_theme_selector_window(initial, 10, 20, 13),
            initial
        );
        let scrolled = ThemeSelectorWindowState {
            window_start: 5,
            ..initial
        };
        assert_eq!(
            synchronize_theme_selector_window(scrolled, 10, 20, 13).window_start,
            5
        );
        assert_eq!(
            synchronize_theme_selector_window(scrolled, 19, 20, 13).window_start,
            7
        );
        assert_eq!(
            synchronize_theme_selector_window(scrolled, 1, 20, 13).window_start,
            1
        );
    }

    #[test]
    fn roomy_frame_matches_the_frozen_opentui_cells_and_geometry() {
        let (buffer, plan) = render_specimen(90, 24, None);
        let output = frame(&buffer, 90);
        assert_eq!(plan.modal.frame, Rect::new(4, 2, 82, 20));
        assert_eq!(plan.visible_rows, 13);
        assert_eq!(plan.window.window_start, 4);
        assert_eq!(plan.item_hits.first().unwrap().index, 4);
        assert_eq!(plan.item_hits.last().unwrap().index, 16);
        assert_eq!(plan.remaining_items, 3);
        assert!(output.contains("└─…─3─more"));
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n\n").as_bytes()),
            "a6596ee84b8aebacbd91ac04dfb46b0e3cda51c6be11685f281b8e860970cdef"
        );
    }

    #[test]
    fn narrow_frame_preserves_flexed_body_and_terminal_overflow() {
        let (buffer, plan) = render_specimen(50, 10, None);
        let output = frame(&buffer, 50);
        assert_eq!(plan.requested_width, 56);
        assert_eq!(plan.requested_height, 12);
        assert_eq!(plan.modal.frame, Rect::new(1, 1, 48, 8));
        assert_eq!(plan.window.window_start, 8);
        assert!(output.contains("└─›  Theme 10"));
        assert!(output.lines().last().unwrap().contains("Theme 11"));
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n\n").as_bytes()),
            "9debc8afb568ecfa46599dd8c9d7965407148c6c938b2ef5a244ad8c47ac312b",
            "{output}"
        );
    }

    #[test]
    fn one_wheel_step_changes_only_the_window_and_matches_the_oracle() {
        let previous = ThemeSelectorWindowState {
            item_count: 20,
            selected_index: 10,
            visible_rows: 13,
            window_start: 5,
        };
        let (buffer, plan) = render_specimen(90, 24, Some(previous));
        let output = frame(&buffer, 90);
        assert_eq!(plan.window.window_start, 5);
        assert!(output.contains("Theme 05"));
        assert!(!output.contains("Theme 04"));
        assert!(output.contains("└─…─2─more"));
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n\n").as_bytes()),
            "63dbf47361362528c735e9ab0d918a8d114cbe6300b0f19991df7c57f00ea508"
        );
    }

    #[test]
    fn pointer_hits_cover_each_complete_visible_row() {
        let (_, plan) = render_specimen(90, 24, None);
        let row = plan.item_hits[2].bounds;
        assert_eq!(
            theme_selector_item_at(&plan, row.x, row.y).map(|hit| hit.index),
            Some(6)
        );
        assert_eq!(
            theme_selector_item_at(&plan, row.right() - 1, row.y).map(|hit| hit.index),
            Some(6)
        );
        assert!(theme_selector_item_at(&plan, row.x, plan.help_row.y).is_none());
    }
}
