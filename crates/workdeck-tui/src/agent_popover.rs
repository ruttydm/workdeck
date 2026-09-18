//! Agent-note popover content measurement and viewport placement.

use crate::{fit_text, wrap_text};
use workdeck_diff::sanitize_terminal_line;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPopoverContent {
    pub title: String,
    pub summary_lines: Vec<String>,
    pub rationale_lines: Vec<String>,
    pub footer: String,
    pub height: usize,
    pub inner_width: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct AgentPopoverContentInput<'a> {
    pub location_label: &'a str,
    pub note_count: usize,
    pub note_index: usize,
    pub rationale: Option<&'a str>,
    pub summary: &'a str,
    pub width: usize,
    pub author: Option<&'a str>,
}

/// Author title or `AI note`, with a one-based thread position when needed.
#[must_use]
pub fn format_agent_note_title(
    note_index: usize,
    note_count: usize,
    author: Option<&str>,
) -> String {
    if let Some(author) = author.filter(|author| !author.is_empty()) {
        let author = sanitize_terminal_line(author);
        return if note_count > 1 {
            format!("{author} {}/{}", note_index + 1, note_count)
        } else {
            author
        };
    }
    if note_count > 1 {
        format!("AI note {}/{}", note_index + 1, note_count)
    } else {
        "AI note".into()
    }
}

/// Measure the content and total framed height of one popover.
#[must_use]
pub fn build_agent_popover_content(input: AgentPopoverContentInput<'_>) -> AgentPopoverContent {
    let inner_width = input.width.saturating_sub(4).max(1);
    let summary_lines = wrap_text(input.summary, inner_width);
    let rationale_lines = input
        .rationale
        .filter(|rationale| !rationale.is_empty())
        .map_or_else(Vec::new, |rationale| wrap_text(rationale, inner_width));
    let footer = fit_text(input.location_label, inner_width, None);
    let content_line_count = 1
        + summary_lines.len()
        + usize::from(!rationale_lines.is_empty())
        + rationale_lines.len()
        + 1
        + 1;
    AgentPopoverContent {
        title: format_agent_note_title(input.note_index, input.note_count, input.author),
        summary_lines,
        rationale_lines,
        footer,
        height: content_line_count + 2,
        inner_width,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPopoverSide {
    Right,
    Left,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentPopoverPlacement {
    pub left: i64,
    pub top: i64,
    pub side: AgentPopoverSide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentPopoverPlacementInput {
    pub anchor_column: i64,
    pub anchor_row_height: i64,
    pub anchor_row_top: i64,
    pub content_height: i64,
    pub note_height: i64,
    pub note_width: i64,
    pub viewport_width: i64,
}

/// Right-align within the viewport while anchoring vertically to the diff row.
#[must_use]
pub fn resolve_agent_popover_placement(input: AgentPopoverPlacementInput) -> AgentPopoverPlacement {
    let left = (input.viewport_width - input.note_width).max(1);
    let side = if left >= input.anchor_column {
        AgentPopoverSide::Right
    } else {
        AgentPopoverSide::Left
    };
    let preferred_top = input.anchor_row_top + ((input.anchor_row_height - 1).max(0) / 2);
    let max_top = (input.content_height - input.note_height).max(0);
    AgentPopoverPlacement {
        left,
        top: preferred_top.clamp(0, max_top),
        side,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_wraps_and_measures_the_exact_frame_rows() {
        let content = build_agent_popover_content(AgentPopoverContentInput {
            summary: "Guard missing socket path",
            rationale: Some("Prevents noisy reconnect errors during first launch."),
            location_label: "startup.ts +43-44",
            note_index: 0,
            note_count: 2,
            width: 34,
            author: None,
        });
        assert_eq!(content.title, "AI note 1/2");
        assert!(!content.summary_lines.is_empty());
        assert!(!content.rationale_lines.is_empty());
        assert_eq!(content.height, 9);
        assert_eq!(content.inner_width, 30);
    }

    #[test]
    fn titles_sanitize_authors_and_omit_unneeded_position_suffixes() {
        assert_eq!(format_agent_note_title(0, 1, None), "AI note");
        assert_eq!(format_agent_note_title(0, 1, Some("Codex")), "Codex");
        assert_eq!(
            format_agent_note_title(1, 3, Some("Agent\nspoof")),
            "Agentspoof 2/3"
        );
    }

    #[test]
    fn placement_right_aligns_and_clamps_at_the_content_bottom() {
        assert_eq!(
            resolve_agent_popover_placement(AgentPopoverPlacementInput {
                anchor_column: 12,
                anchor_row_top: 4,
                anchor_row_height: 1,
                content_height: 20,
                note_width: 18,
                note_height: 7,
                viewport_width: 60,
            }),
            AgentPopoverPlacement {
                left: 42,
                top: 4,
                side: AgentPopoverSide::Right,
            }
        );
        assert_eq!(
            resolve_agent_popover_placement(AgentPopoverPlacementInput {
                anchor_column: 48,
                anchor_row_top: 16,
                anchor_row_height: 1,
                content_height: 20,
                note_width: 18,
                note_height: 7,
                viewport_width: 60,
            }),
            AgentPopoverPlacement {
                left: 42,
                top: 13,
                side: AgentPopoverSide::Left,
            }
        );
    }
}
