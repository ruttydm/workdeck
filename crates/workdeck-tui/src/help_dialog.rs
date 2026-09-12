//! Command-derived controls help inside the shared modal frame.
//!
//! This ports Hunk's `src/ui/components/chrome/HelpDialog.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. The command catalog remains
//! authoritative, including remaps and disabled commands, while Ratatui owns
//! the cell buffer and the shell owns the scroll offset.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::{
    AppTheme, HelpCommand, HelpSection, ModalFrameOptions, ModalFramePlan, build_help_sections,
    fit_text, pad_text, ratatui_theme_color, render_modal_frame,
};

pub const HELP_MODAL_TITLE: &str = "Controls help";
pub const HELP_MODAL_FRAME_CHROME_ROWS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpDialogPlan {
    pub requested_width: u16,
    pub body_width: usize,
    pub key_width: usize,
    pub description_width: usize,
    pub content_row_count: usize,
    pub required_modal_height: usize,
    pub modal_height: u16,
    pub should_scroll: bool,
    pub sections: Vec<HelpSection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelpDialogRenderMap {
    pub modal: ModalFramePlan,
    pub applied_scroll: usize,
    pub max_scroll: usize,
    pub visible_rows: usize,
}

fn utf16_len(value: &str) -> usize {
    value.encode_utf16().count()
}

/// Derive sizing and columns from the live command table exactly once.
#[must_use]
pub fn plan_help_dialog(
    commands: &[HelpCommand],
    terminal_width: u16,
    terminal_height: u16,
) -> HelpDialogPlan {
    let sections = build_help_sections(commands);
    let requested_width = 74_u16.min(56_u16.max(terminal_width.saturating_sub(8)));
    let body_width = usize::from(requested_width.saturating_sub(4).max(1));
    let longest_keys = sections
        .iter()
        .flat_map(|section| &section.rows)
        .map(|row| utf16_len(&row.keys))
        .max()
        .unwrap_or_default();
    let longest_description = sections
        .iter()
        .flat_map(|section| &section.rows)
        .map(|row| utf16_len(&row.description))
        .max()
        .unwrap_or_default();
    let description_first_budget = isize::try_from(body_width)
        .unwrap_or(isize::MAX)
        .saturating_sub(isize::try_from(longest_description).unwrap_or(isize::MAX));
    let key_width = 12_usize.max(
        longest_keys
            .saturating_add(1)
            .min(usize::try_from(description_first_budget.max(0)).unwrap_or_default()),
    );
    let description_width = body_width.saturating_sub(key_width).max(1);
    let section_spacer_row_count = sections.len().saturating_sub(1);
    let content_row_count = sections
        .iter()
        .map(|section| 1_usize.saturating_add(section.rows.len()))
        .sum::<usize>()
        .saturating_add(section_spacer_row_count);
    let required_modal_height = content_row_count.saturating_add(HELP_MODAL_FRAME_CHROME_ROWS);
    let maximum_height = terminal_height.saturating_sub(2).max(8);
    let modal_height = u16::try_from(required_modal_height)
        .unwrap_or(u16::MAX)
        .min(maximum_height);
    HelpDialogPlan {
        requested_width,
        body_width,
        key_width,
        description_width,
        content_row_count,
        required_modal_height,
        modal_height,
        should_scroll: usize::from(modal_height) < required_modal_height,
        sections,
    }
}

fn help_content_lines(plan: &HelpDialogPlan, theme: &AppTheme) -> Vec<Line<'static>> {
    let mut lines = Vec::with_capacity(plan.content_row_count);
    for (section_index, section) in plan.sections.iter().enumerate() {
        lines.push(Line::styled(
            section.title.clone(),
            Style::default().fg(ratatui_theme_color(&theme.badge_neutral)),
        ));
        lines.extend(section.rows.iter().map(|row| {
            Line::from(vec![
                Span::styled(
                    pad_text(&fit_text(&row.keys, plan.key_width, None), plan.key_width),
                    Style::default().fg(ratatui_theme_color(&theme.accent)),
                ),
                Span::styled(
                    fit_text(&row.description, plan.description_width, None),
                    Style::default().fg(ratatui_theme_color(&theme.muted)),
                ),
            ])
        }));
        if section_index + 1 < plan.sections.len() {
            lines.push(Line::default());
        }
    }
    lines
}

/// Paint the help dialog and return frame/scroll geometry for shell input routing.
pub fn render_help_dialog(
    area: Rect,
    buffer: &mut Buffer,
    commands: &[HelpCommand],
    theme: &AppTheme,
    vertical_offset: usize,
) -> HelpDialogRenderMap {
    let plan = plan_help_dialog(commands, area.width, area.height);
    let modal = render_modal_frame(
        area,
        buffer,
        ModalFrameOptions {
            width: plan.requested_width,
            height: plan.modal_height,
            closeable: true,
            has_mouse_scroll_handler: false,
            terminal_width: area.width,
            terminal_height: area.height,
            theme,
            title: HELP_MODAL_TITLE,
        },
    );
    let visible_rows = usize::from(modal.content.height);
    let max_scroll = if plan.should_scroll {
        plan.content_row_count.saturating_sub(visible_rows)
    } else {
        0
    };
    let applied_scroll = vertical_offset.min(max_scroll);
    if modal.content.width > 0 && modal.content.height > 0 {
        let lines = help_content_lines(&plan, theme)
            .into_iter()
            .skip(applied_scroll)
            .take(visible_rows)
            .collect::<Vec<_>>();
        Paragraph::new(lines)
            .style(Style::default().bg(ratatui_theme_color(&theme.panel)))
            .render(modal.content, buffer);
    }
    HelpDialogRenderMap {
        modal,
        applied_scroll,
        max_scroll,
        visible_rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{default_help_commands, resolve_theme};

    fn frame(buffer: &Buffer, width: u16) -> String {
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn plans_source_columns_content_rows_and_scroll_threshold() {
        let commands = default_help_commands();
        let roomy = plan_help_dialog(&commands, 76, 39);
        assert_eq!(roomy.requested_width, 68);
        assert_eq!(roomy.body_width, 64);
        assert_eq!(roomy.description_width, roomy.body_width - roomy.key_width);
        assert_eq!(
            roomy.required_modal_height,
            roomy.content_row_count + HELP_MODAL_FRAME_CHROME_ROWS
        );
        assert_eq!(
            roomy.should_scroll,
            (roomy.modal_height as usize) < roomy.required_modal_height
        );

        let short = plan_help_dialog(&commands, 76, 12);
        assert_eq!(short.modal_height, 10);
        assert!(short.should_scroll);
    }

    #[test]
    fn renders_every_section_with_exact_modal_title_palette_and_spacing() {
        let commands = default_help_commands();
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let area = Rect::new(0, 0, 76, 39);
        let mut buffer = Buffer::empty(area);
        let map = render_help_dialog(area, &mut buffer, &commands, &theme, 0);
        let output = frame(&buffer, area.width);
        assert!(output.contains("Controls help"));
        assert!(output.contains("[Esc]"));
        for expected in [
            "Navigation",
            "PageDown / Space / f",
            "Mouse",
            "Shift+Wheel",
            "View",
            "1 / 2 / 0",
            "Review",
            "create review note",
        ] {
            assert!(output.contains(expected), "missing {expected:?}");
        }
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n").as_bytes()),
            // Updated with the bundled search rows and the unbound filter: the
            // same rows upstream's search commit renders.
            "fa3f57a7a5d868a8f5ed29ca77b7a5ce8f27a1e33a3cc24c3456b0597cb834f9"
        );
        assert_eq!(
            buffer[map.modal.frame.as_position()].fg,
            ratatui_theme_color(&theme.accent)
        );
        assert_eq!(map.visible_rows, map.modal.content.height as usize);
    }

    #[test]
    fn remaps_reach_the_rows_and_small_dialogs_apply_bounded_scroll() {
        let mut commands = default_help_commands();
        commands
            .iter_mut()
            .find(|command| command.id == "workdeck.app.quit")
            .unwrap()
            .key_labels = vec!["Ctrl+X".into()];
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let area = Rect::new(0, 0, 76, 12);
        let mut first = Buffer::empty(area);
        let first_map = render_help_dialog(area, &mut first, &commands, &theme, 0);
        assert!(first_map.max_scroll > 0);
        assert_eq!(first_map.applied_scroll, 0);

        let mut last = Buffer::empty(area);
        let last_map = render_help_dialog(area, &mut last, &commands, &theme, usize::MAX);
        assert_eq!(last_map.applied_scroll, last_map.max_scroll);
        let last_frame = frame(&last, area.width);
        assert!(last_frame.contains("Ctrl+X"));
        assert!(!last_frame.contains("  q "));
    }
}
