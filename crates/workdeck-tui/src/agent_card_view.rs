//! Framed floating agent-note card painting.
//!
//! This is a native Ratatui reimplementation of Hunk's
//! `src/ui/components/panes/AgentCard.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.

use ratatui::style::Style;
use ratatui::text::{Line, Span};

use crate::{
    AgentPopoverContentInput, AppTheme, build_agent_popover_content, fit_text, measure_text_width,
    pad_text, ratatui_theme_color,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedAgentCardRun {
    pub text: String,
    pub foreground: Option<String>,
    pub background: String,
    pub close_action: bool,
}

impl PaintedAgentCardRun {
    fn new(
        text: impl Into<String>,
        foreground: Option<&str>,
        background: &str,
        close_action: bool,
    ) -> Self {
        Self {
            text: text.into(),
            foreground: foreground.map(str::to_owned),
            background: background.into(),
            close_action,
        }
    }

    #[must_use]
    pub fn ratatui_span(&self) -> Span<'static> {
        let mut style = Style::default().bg(ratatui_theme_color(&self.background));
        if let Some(foreground) = self.foreground.as_deref() {
            style = style.fg(ratatui_theme_color(foreground));
        }
        Span::styled(self.text.clone(), style)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaintedAgentCardLine {
    pub runs: Vec<PaintedAgentCardRun>,
}

impl PaintedAgentCardLine {
    #[must_use]
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[must_use]
    pub fn width(&self) -> usize {
        measure_text_width(&self.text())
    }

    #[must_use]
    pub fn ratatui_line(&self) -> Line<'static> {
        Line::from(
            self.runs
                .iter()
                .map(PaintedAgentCardRun::ratatui_span)
                .collect::<Vec<_>>(),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCardCloseHit {
    pub row: usize,
    pub column_start: usize,
    pub width: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedAgentCard {
    pub lines: Vec<PaintedAgentCardLine>,
    pub close_hit: Option<AgentCardCloseHit>,
    pub inner_width: usize,
}

impl PaintedAgentCard {
    #[must_use]
    pub fn closes_at(&self, row: usize, column: usize) -> bool {
        self.close_hit.is_some_and(|hit| {
            hit.row == row
                && column >= hit.column_start
                && column < hit.column_start.saturating_add(hit.width)
        })
    }
}

#[derive(Debug, Clone, Copy)]
pub struct AgentCardViewOptions<'a> {
    pub location_label: &'a str,
    pub note_count: usize,
    pub note_index: usize,
    pub rationale: Option<&'a str>,
    pub summary: &'a str,
    pub theme: &'a AppTheme,
    pub width: usize,
    pub author: Option<&'a str>,
    pub close_action: bool,
}

fn border_line(
    left: &str,
    fill: &str,
    right: &str,
    width: usize,
    theme: &AppTheme,
) -> PaintedAgentCardLine {
    let inner = width.saturating_sub(2);
    PaintedAgentCardLine {
        runs: vec![PaintedAgentCardRun::new(
            format!("{left}{}{right}", fill.repeat(inner)),
            Some(&theme.accent),
            &theme.panel,
            false,
        )],
    }
}

fn content_line(
    content: PaintedAgentCardRun,
    inner_width: usize,
    width: usize,
    theme: &AppTheme,
) -> PaintedAgentCardLine {
    let mut runs = vec![
        PaintedAgentCardRun::new("│", Some(&theme.accent), &theme.panel, false),
        PaintedAgentCardRun::new(" ", None, &theme.panel, false),
        content,
        PaintedAgentCardRun::new(" ", None, &theme.panel, false),
        PaintedAgentCardRun::new("│", Some(&theme.accent), &theme.panel, false),
    ];
    let current_width = runs
        .iter()
        .map(|run| measure_text_width(&run.text))
        .sum::<usize>();
    if current_width < width {
        let padding = " ".repeat(width - current_width);
        runs.insert(
            runs.len() - 2,
            PaintedAgentCardRun::new(padding, None, &theme.panel, false),
        );
    }
    debug_assert!(inner_width > 0);
    PaintedAgentCardLine { runs }
}

/// Paint the complete framed card and its optional close hit target.
#[must_use]
pub fn paint_agent_card(options: AgentCardViewOptions<'_>) -> PaintedAgentCard {
    if options.width == 0 {
        return PaintedAgentCard {
            lines: Vec::new(),
            close_hit: None,
            inner_width: 0,
        };
    }
    let popover = build_agent_popover_content(AgentPopoverContentInput {
        location_label: options.location_label,
        note_count: options.note_count,
        note_index: options.note_index,
        rationale: options.rationale,
        summary: options.summary,
        width: options.width,
        author: options.author,
    });
    let mut lines = vec![border_line("┌", "─", "┐", options.width, options.theme)];
    let title_width = popover
        .inner_width
        .saturating_sub(if options.close_action { 4 } else { 0 })
        .max(1);
    let mut title_runs = vec![PaintedAgentCardRun::new(
        pad_text(&fit_text(&popover.title, title_width, None), title_width),
        Some(&options.theme.accent),
        &options.theme.panel,
        false,
    )];
    let close_hit = if options.close_action {
        title_runs.push(PaintedAgentCardRun::new(
            " ",
            None,
            &options.theme.panel,
            false,
        ));
        title_runs.push(PaintedAgentCardRun::new(
            "[x]",
            Some(&options.theme.muted),
            &options.theme.panel,
            true,
        ));
        Some(AgentCardCloseHit {
            row: 1,
            column_start: title_width.saturating_add(3),
            width: 3,
        })
    } else {
        None
    };
    let title_content = PaintedAgentCardRun::new(
        title_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>(),
        Some(&options.theme.accent),
        &options.theme.panel,
        false,
    );
    let mut title_line = content_line(
        title_content,
        popover.inner_width,
        options.width,
        options.theme,
    );
    if options.close_action {
        title_line.runs = vec![
            PaintedAgentCardRun::new(
                "│",
                Some(&options.theme.accent),
                &options.theme.panel,
                false,
            ),
            PaintedAgentCardRun::new(" ", None, &options.theme.panel, false),
            title_runs.remove(0),
            title_runs.remove(0),
            title_runs.remove(0),
            PaintedAgentCardRun::new(" ", None, &options.theme.panel, false),
            PaintedAgentCardRun::new(
                "│",
                Some(&options.theme.accent),
                &options.theme.panel,
                false,
            ),
        ];
    }
    lines.push(title_line);

    for summary in &popover.summary_lines {
        lines.push(content_line(
            PaintedAgentCardRun::new(
                pad_text(summary, popover.inner_width),
                Some(&options.theme.text),
                &options.theme.panel,
                false,
            ),
            popover.inner_width,
            options.width,
            options.theme,
        ));
    }
    if !popover.rationale_lines.is_empty() {
        lines.push(content_line(
            PaintedAgentCardRun::new(
                " ".repeat(popover.inner_width),
                Some(&options.theme.text),
                &options.theme.panel,
                false,
            ),
            popover.inner_width,
            options.width,
            options.theme,
        ));
        for rationale in &popover.rationale_lines {
            lines.push(content_line(
                PaintedAgentCardRun::new(
                    pad_text(rationale, popover.inner_width),
                    Some(&options.theme.muted),
                    &options.theme.panel,
                    false,
                ),
                popover.inner_width,
                options.width,
                options.theme,
            ));
        }
    }
    lines.push(content_line(
        PaintedAgentCardRun::new(
            " ".repeat(popover.inner_width),
            Some(&options.theme.text),
            &options.theme.panel,
            false,
        ),
        popover.inner_width,
        options.width,
        options.theme,
    ));
    lines.push(content_line(
        PaintedAgentCardRun::new(
            pad_text(&popover.footer, popover.inner_width),
            Some(&options.theme.muted),
            &options.theme.panel,
            false,
        ),
        popover.inner_width,
        options.width,
        options.theme,
    ));
    lines.push(border_line("└", "─", "┘", options.width, options.theme));
    debug_assert_eq!(lines.len(), popover.height);
    debug_assert!(lines.iter().all(|line| line.width() == options.width));
    PaintedAgentCard {
        lines,
        close_hit,
        inner_width: popover.inner_width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_theme;

    fn options<'a>(theme: &'a AppTheme, author: Option<&'a str>) -> AgentCardViewOptions<'a> {
        AgentCardViewOptions {
            location_label: "alpha.ts +2",
            note_count: 1,
            note_index: 0,
            rationale: Some("Why alpha.ts changed"),
            summary: "Annotation for alpha.ts",
            theme,
            width: 34,
            author,
            close_action: true,
        }
    }

    #[test]
    fn removes_outer_padding_and_keeps_the_footer_inside_the_frame() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let painted = paint_agent_card(options(&theme, None));
        assert_eq!(
            painted
                .lines
                .iter()
                .map(PaintedAgentCardLine::text)
                .collect::<Vec<_>>(),
            vec![
                "┌────────────────────────────────┐",
                "│ AI note                    [x] │",
                "│ Annotation for alpha.ts        │",
                "│                                │",
                "│ Why alpha.ts changed           │",
                "│                                │",
                "│ alpha.ts +2                    │",
                "└────────────────────────────────┘",
            ]
        );
        assert_eq!(painted.lines.len(), 8);
        assert!(painted.closes_at(1, 29));
        assert!(!painted.closes_at(1, 28));
        assert_eq!(
            painted.lines[0].runs[0].foreground.as_deref(),
            Some("#bb8009")
        );
        assert_eq!(
            painted.lines[2].runs[2].foreground.as_deref(),
            Some("#e6edf3")
        );
        assert_eq!(
            painted.lines[4].runs[2].foreground.as_deref(),
            Some("#adaeb1")
        );
        assert!(
            painted
                .lines
                .iter()
                .flat_map(|line| &line.runs)
                .all(|run| run.background == "#1e2329")
        );
    }

    #[test]
    fn shows_author_in_the_title_when_set() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let title = paint_agent_card(options(&theme, Some("sonnet"))).lines[1].text();
        assert!(title.contains("sonnet"));
        assert!(!title.contains("AI note"));
    }

    #[test]
    fn falls_back_to_ai_note_when_author_is_absent() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        assert!(
            paint_agent_card(options(&theme, None)).lines[1]
                .text()
                .contains("AI note")
        );
    }

    #[test]
    fn no_close_variant_preserves_title_budget_and_ratatui_lines() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut input = options(&theme, Some("a long agent title"));
        input.close_action = false;
        let painted = paint_agent_card(input);
        assert!(painted.close_hit.is_none());
        assert!(!painted.lines[1].text().contains("[x]"));
        assert_eq!(painted.lines[1].ratatui_line().width(), 34);
    }
}
