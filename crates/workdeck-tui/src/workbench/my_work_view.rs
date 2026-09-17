use super::my_work::MyWorkWorkspace;
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use workdeck_diff::{format_terminal_path, sanitize_terminal_line};

impl MyWorkWorkspace {
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let regions = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
            Constraint::Length(3),
        ])
        .split(area);
        let status = if !self.is_idle() {
            "loading · retained observation"
        } else if self.error.is_some() {
            "stale retained observation"
        } else if self
            .report
            .as_ref()
            .is_some_and(|report| !report.all_sources_available)
        {
            "partial · unavailable sources"
        } else {
            "captured observations"
        };
        let title = self
            .report
            .as_ref()
            .map(|report| {
                format!(
                    "My work · {} · {} · {} known matches",
                    report.facet.title(),
                    sanitize_terminal_line(&report.assignee),
                    report.known_total
                )
            })
            .unwrap_or_else(|| "My work".into());
        let evaluated = self
            .report
            .as_ref()
            .and_then(|report| report.as_of)
            .map(|instant| format!("as of {} · ", instant.to_rfc3339()))
            .unwrap_or_default();
        Paragraph::new(format!("{title}\n{evaluated}{status} · read-only"))
            .style(style)
            .render(regions[0], buffer);
        self.hits.clear();
        if let Some(form) = &self.form {
            super::shell_view::render_form(form, regions[1], buffer, theme);
        } else {
            let columns = if area.width >= 110 {
                Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                    .split(regions[1])
            } else {
                Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(regions[1])
            };
            self.list_bounds = columns[0];
            self.detail_bounds = columns[1];
            let inner = columns[0].inner(Margin::new(1, 1));
            self.page_rows = usize::from(inner.height).max(1);
            let selected = if self.sources {
                self.source_selected
            } else {
                self.selected
            };
            let offset = selected / self.page_rows * self.page_rows;
            let block = Block::default()
                .title(if self.sources {
                    "Registered sources"
                } else {
                    self.report
                        .as_ref()
                        .map_or("Assignments", |report| report.facet.title())
                })
                .borders(Borders::ALL);
            block.render(columns[0], buffer);
            if let Some(report) = &self.report {
                let length = if self.sources {
                    report.sources.len()
                } else {
                    report.rows.len()
                };
                if length == 0 {
                    Paragraph::new(if report.sources.is_empty() {
                        "No registered checkouts. Use workdeck repository register."
                    } else {
                        "No matching work in this captured page."
                    })
                    .wrap(Wrap { trim: false })
                    .render(inner, buffer);
                }
                for (screen, index) in (offset..length).take(self.page_rows).enumerate() {
                    let text = if self.sources {
                        let source = &report.sources[index];
                        format!(
                            "{} · {}",
                            source.checkout.alias,
                            if source.error.is_some() {
                                "unavailable/stale".into()
                            } else {
                                format!("{} matches", source.matches.unwrap_or(0))
                            }
                        )
                    } else {
                        let row = &report.rows[index];
                        format!("{} · {}", row.alias, row.row.title)
                    };
                    let row = Rect::new(
                        inner.x,
                        inner.y.saturating_add(screen as u16),
                        inner.width,
                        1,
                    );
                    let item_style = if index == selected {
                        style
                            .add_modifier(Modifier::BOLD)
                            .fg(ratatui_theme_color(&theme.accent))
                    } else {
                        style
                    };
                    Paragraph::new(sanitize_terminal_line(&text))
                        .style(item_style)
                        .render(row, buffer);
                    self.hits.push((row, index));
                }
            }
            if self.sources {
                let text = self.report.as_ref().and_then(|report| report.sources.get(self.source_selected)).map(|source| format!("{}\n{}\n{}\n{:?}\n{}", source.checkout.alias, source.checkout.repository, format_terminal_path(&source.checkout.checkout.to_string_lossy()), source.checkout.source, source.error.as_ref().map(|error| error.message.as_str()).unwrap_or("Captured locally; this report does not authorize writes or confirm shared ownership."))).unwrap_or_else(|| "No source selected".into());
                Paragraph::new(sanitize_terminal_line(&text.replace('\n', " · ")))
                    .block(
                        Block::default()
                            .title("Source identity and availability")
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: false })
                    .render(columns[1], buffer);
            } else if self.show_evidence {
                self.render_evidence(columns[1], buffer, style);
            } else if let Some((checkout, detail)) = &self.opened {
                let detail_regions =
                    Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(columns[1]);
                Paragraph::new(format!(
                    "{} · {}\n{}\n{:?} · {}",
                    sanitize_terminal_line(&checkout.alias),
                    checkout.repository,
                    format_terminal_path(&detail.row.token.path.to_string_lossy()),
                    detail.row.token.view.source.role,
                    detail.row.token.content
                ))
                .style(style)
                .render(detail_regions[0], buffer);
                let lines = self
                    .opened_lines
                    .iter()
                    .skip(self.detail_scroll)
                    .take(usize::from(detail_regions[1].height))
                    .map(|line| Line::from(line.as_str()))
                    .collect::<Vec<_>>();
                Paragraph::new(lines)
                    .style(style)
                    .render(detail_regions[1], buffer);
            } else {
                Paragraph::new("Enter opens the exact cached source excerpt.\ns shows every registered source and its availability.").block(Block::default().title("Source excerpt").borders(Borders::ALL)).wrap(Wrap{trim:false}).render(columns[1],buffer);
            }
        }
        if let Some(error) = &self.error {
            Paragraph::new(sanitize_terminal_line(error))
                .style(style)
                .wrap(Wrap { trim: false })
                .render(regions[2], buffer);
        } else if self
            .report
            .as_ref()
            .is_some_and(|report| !report.all_sources_available)
        {
            Paragraph::new("Some sources are unavailable or stale. Press s to inspect each source; known totals are incomplete.").style(style).wrap(Wrap{trim:false}).render(regions[2],buffer);
        }
        Paragraph::new(if self.form.is_some() { "Tab field · Ctrl-S apply filter · Esc cancel\nF2 Review · F3 Issues · Shift-F9 My work" } else { "1 assigned · 2 reviews · 3 overdue · 4 blocked · 5 claimed\no open source · b previous · h launch · Enter excerpt\n↑↓ rows · [ ] pages · e evidence · / filter · s sources · r refresh" }).style(style).render(regions[3],buffer);
    }
}
