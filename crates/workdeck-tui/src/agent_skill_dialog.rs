//! Copyable agent-skill guidance inside the shared modal frame.
//!
//! This ports Hunk's `src/ui/components/chrome/AgentSkillDialog.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Product naming is intentionally
//! normalized to Workdeck while sizing, clipping, paint, and pointer behavior
//! remain source-compatible.

use base64::Engine as _;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};
use std::io::{self, Write};
use std::sync::LazyLock;

use crate::{
    AppTheme, ModalFrameOptions, ModalFramePlan, fit_text, measure_text_width, ratatui_theme_color,
    render_modal_frame,
};

pub const AGENT_SKILL_COMMAND: &str = "workdeck skill path";
pub const AGENT_SKILL_PROMPT_ROWS: [&str; 2] = [
    "Load the Workdeck skill and use it for this review.",
    "Run `workdeck skill path` to get the skill path.",
];
pub static AGENT_SKILL_PROMPT: LazyLock<String> =
    LazyLock::new(|| AGENT_SKILL_PROMPT_ROWS.join(" "));
pub const AGENT_SKILL_TITLE: &str = "Agent skill";
pub const AGENT_SKILL_INTRO: &str = "Teach your agent how to review this Workdeck session.";
pub const AGENT_SKILL_COPY_LABEL: &str = " ⧉  Copy prompt ";
pub const AGENT_SKILL_UNAVAILABLE_LABEL: &str = " Copy unavailable ";

/// Write the same OSC 52 clipboard transport used by Hunk's terminal renderer.
pub fn write_osc52_clipboard(writer: &mut impl Write, text: &str) -> io::Result<()> {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    writer.write_all(b"\x1b]52;c;")?;
    writer.write_all(encoded.as_bytes())?;
    writer.write_all(b"\x07")?;
    writer.flush()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSkillDialogPlan {
    pub requested_width: u16,
    pub body_width: usize,
    pub prompt_width: usize,
    pub card_width: usize,
    pub card_text_width: usize,
    pub required_modal_height: usize,
    pub modal_height: u16,
    pub copy_supported: bool,
    pub copy_label: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSkillDialogRenderMap {
    pub modal: ModalFramePlan,
    pub body: Rect,
    pub prompt_card: Rect,
    pub copy_button: Rect,
    pub copy_supported: bool,
}

/// Resolve the responsive source dimensions before the shared frame clamps to
/// the real terminal. Hunk performs its text fitting from this requested width,
/// so narrow terminals clip the already-fitted rows instead of recomputing them.
#[must_use]
pub fn plan_agent_skill_dialog(
    terminal_width: u16,
    terminal_height: u16,
    copy_supported: bool,
) -> AgentSkillDialogPlan {
    let requested_width = 84_u16.min(58_u16.max(terminal_width.saturating_sub(8)));
    let body_width = usize::from(requested_width.saturating_sub(4).max(1));
    let prompt_width = body_width.saturating_sub(4).max(1);
    let card_width = body_width.saturating_sub(4).max(1);
    let card_text_width = card_width.saturating_sub(4).max(1);
    let required_modal_height = AGENT_SKILL_PROMPT_ROWS.len().saturating_add(11);
    let modal_height = u16::try_from(required_modal_height)
        .unwrap_or(u16::MAX)
        .min(terminal_height.saturating_sub(2).max(10));
    AgentSkillDialogPlan {
        requested_width,
        body_width,
        prompt_width,
        card_width,
        card_text_width,
        required_modal_height,
        modal_height,
        copy_supported,
        copy_label: if copy_supported {
            AGENT_SKILL_COPY_LABEL
        } else {
            AGENT_SKILL_UNAVAILABLE_LABEL
        },
    }
}

fn row(area: Rect, offset: u16, height: u16) -> Rect {
    if offset >= area.height {
        return Rect::new(area.x, area.bottom(), area.width, 0);
    }
    Rect::new(
        area.x,
        area.y.saturating_add(offset),
        area.width,
        height.min(area.height.saturating_sub(offset)),
    )
}

fn inset_left(area: Rect, left: u16, requested_width: usize) -> Rect {
    let width = u16::try_from(requested_width)
        .unwrap_or(u16::MAX)
        .min(area.width.saturating_sub(left));
    Rect::new(
        area.x.saturating_add(left.min(area.width)),
        area.y,
        width,
        area.height,
    )
}

/// Paint the guidance and return exact hit rectangles for the shell.
pub fn render_agent_skill_dialog(
    area: Rect,
    buffer: &mut Buffer,
    theme: &AppTheme,
    copy_supported: bool,
) -> AgentSkillDialogRenderMap {
    let plan = plan_agent_skill_dialog(area.width, area.height, copy_supported);
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
            title: AGENT_SKILL_TITLE,
        },
    );

    // This fixed-height consumer uses every row after ModalFrame's title. Its
    // OpenTUI flex allocation is one row taller than the scrolling help body.
    let body = Rect::new(
        modal.frame.x.saturating_add(2),
        modal.frame.y.saturating_add(3),
        modal.frame.width.saturating_sub(4),
        modal.frame.height.saturating_sub(4),
    );
    let panel = ratatui_theme_color(&theme.panel);
    let text = Style::default()
        .fg(ratatui_theme_color(&theme.text))
        .bg(panel);
    let muted = Style::default()
        .fg(ratatui_theme_color(&theme.muted))
        .bg(panel);
    let neutral = Style::default()
        .fg(ratatui_theme_color(&theme.badge_neutral))
        .bg(panel);

    let intro = row(body, 0, 1);
    if intro.width > 0 && intro.height > 0 {
        Paragraph::new(Line::styled(
            fit_text(AGENT_SKILL_INTRO, plan.body_width, None),
            text,
        ))
        .render(intro, buffer);
    }

    let prompt_label = inset_left(row(body, 2, 1), 1, plan.prompt_width);
    if prompt_label.width > 0 && prompt_label.height > 0 {
        Paragraph::new(Line::styled(
            fit_text("Prompt", plan.prompt_width, None),
            neutral,
        ))
        .render(prompt_label, buffer);
    }

    let prompt_card = inset_left(
        row(
            body,
            3,
            u16::try_from(AGENT_SKILL_PROMPT_ROWS.len() + 2).unwrap_or(u16::MAX),
        ),
        1,
        plan.card_width,
    );
    if prompt_card.width > 0 && prompt_card.height > 0 {
        Block::default()
            .borders(Borders::ALL)
            .style(Style::default().bg(panel))
            .border_style(Style::default().fg(ratatui_theme_color(&theme.border)))
            .render(prompt_card, buffer);
        let card_text = Rect::new(
            prompt_card.x.saturating_add(2),
            prompt_card.y.saturating_add(1),
            prompt_card.width.saturating_sub(4),
            prompt_card.height.saturating_sub(2),
        );
        if card_text.width > 0 && card_text.height > 0 {
            Paragraph::new(
                AGENT_SKILL_PROMPT_ROWS
                    .iter()
                    .map(|line| Line::styled(fit_text(line, plan.card_text_width, None), text))
                    .collect::<Vec<_>>(),
            )
            .render(card_text, buffer);
        }
    }

    let button_offset = u16::try_from(AGENT_SKILL_PROMPT_ROWS.len() + 6).unwrap_or(u16::MAX);
    let copy_row = row(body, button_offset, 1);
    let copy_button = Rect::new(
        copy_row.x,
        copy_row.y,
        u16::try_from(measure_text_width(plan.copy_label))
            .unwrap_or(u16::MAX)
            .min(copy_row.width),
        copy_row.height,
    );
    if copy_button.width > 0 && copy_button.height > 0 {
        let style = if plan.copy_supported {
            Style::default()
                .fg(ratatui_theme_color(&theme.text))
                .bg(ratatui_theme_color(&theme.accent_muted))
        } else {
            Style::default()
                .fg(ratatui_theme_color(&theme.muted))
                .bg(ratatui_theme_color(&theme.panel_alt))
        };
        Paragraph::new(Line::styled(plan.copy_label, style)).render(copy_button, buffer);
        let filler = Rect::new(
            copy_button.right(),
            copy_row.y,
            copy_row.width.saturating_sub(copy_button.width).max(1),
            copy_row.height,
        );
        if filler.x < copy_row.right() && filler.width > 0 {
            Paragraph::new(Line::styled("", muted)).render(filler, buffer);
        }
    }

    AgentSkillDialogRenderMap {
        modal,
        body,
        prompt_card,
        copy_button,
        copy_supported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_theme;

    fn frame(buffer: &Buffer, width: u16) -> String {
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn plans_source_dimensions_labels_and_clamps() {
        let roomy = plan_agent_skill_dialog(120, 24, true);
        assert_eq!(roomy.requested_width, 84);
        assert_eq!(roomy.body_width, 80);
        assert_eq!(roomy.prompt_width, 76);
        assert_eq!(roomy.card_width, 76);
        assert_eq!(roomy.card_text_width, 72);
        assert_eq!(roomy.required_modal_height, 13);
        assert_eq!(roomy.modal_height, 13);
        assert_eq!(roomy.copy_label, AGENT_SKILL_COPY_LABEL);

        let narrow = plan_agent_skill_dialog(20, 8, false);
        assert_eq!(narrow.requested_width, 58);
        assert_eq!(narrow.modal_height, 10);
        assert_eq!(narrow.copy_label, AGENT_SKILL_UNAVAILABLE_LABEL);
    }

    #[test]
    fn renders_the_workdeck_branded_equivalent_of_the_frozen_hunk_frame() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let area = Rect::new(0, 0, 120, 24);
        let mut buffer = Buffer::empty(area);
        let map = render_agent_skill_dialog(area, &mut buffer, &theme, true);
        let output = frame(&buffer, area.width);
        for expected in [
            "Agent skill",
            "Teach your agent how to review this Workdeck session.",
            "Load the Workdeck skill and use it for this review.",
            AGENT_SKILL_COMMAND,
            "⧉  Copy prompt",
            "[Esc]",
        ] {
            assert!(output.contains(expected), "missing {expected:?}");
        }
        assert_eq!(map.modal.frame, Rect::new(18, 5, 84, 13));
        assert_eq!(map.body, Rect::new(20, 8, 80, 9));
        assert_eq!(map.prompt_card, Rect::new(21, 11, 76, 4));
        assert_eq!(map.copy_button, Rect::new(20, 16, 16, 1));
        assert_eq!(
            buffer[(20, 16)].bg,
            ratatui_theme_color(&theme.accent_muted)
        );
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n").as_bytes()),
            "3be946aab16729fa868660f63232e259656cc7803662b1350efa77b7679b301e"
        );
    }

    #[test]
    fn unavailable_copy_surface_is_muted_and_not_actionable() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let area = Rect::new(0, 0, 60, 16);
        let mut buffer = Buffer::empty(area);
        let map = render_agent_skill_dialog(area, &mut buffer, &theme, false);
        let output = frame(&buffer, area.width);
        assert!(output.contains("Copy unavailable"));
        assert!(!map.copy_supported);
        assert_eq!(
            buffer[map.copy_button.as_position()].fg,
            ratatui_theme_color(&theme.muted)
        );
        assert_eq!(
            buffer[map.copy_button.as_position()].bg,
            ratatui_theme_color(&theme.panel_alt)
        );
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n").as_bytes()),
            "a403e724c62a47b63425656b7034db0eb4ab5cf4b36328669d804d8caa8b47a9"
        );
    }

    #[test]
    fn clipboard_transport_is_exact_osc52_base64() {
        let mut bytes = Vec::new();
        write_osc52_clipboard(&mut bytes, AGENT_SKILL_PROMPT.as_str()).unwrap();
        assert!(bytes.starts_with(b"\x1b]52;c;"));
        assert!(bytes.ends_with(b"\x07"));
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            "\x1b]52;c;TG9hZCB0aGUgV29ya2RlY2sgc2tpbGwgYW5kIHVzZSBpdCBmb3IgdGhpcyByZXZpZXcuIFJ1biBgd29ya2RlY2sgc2tpbGwgcGF0aGAgdG8gZ2V0IHRoZSBza2lsbCBwYXRoLg==\x07"
        );
    }
}
