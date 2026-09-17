use super::sources_workspace::SourcesWorkspace;
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;
use workdeck_pm::{SourceFreshness, SourceRole, SourceSelector};

fn safe(text: &str) -> String {
    text.lines()
        .map(sanitize_terminal_line)
        .collect::<Vec<_>>()
        .join("\n")
}
fn role(role: SourceRole) -> &'static str {
    match role {
        SourceRole::Local => "Local",
        SourceRole::Accepted => "Accepted",
        SourceRole::Proposal => "Proposal",
        SourceRole::Coordination => "Coordination",
        SourceRole::Staged => "Staged",
    }
}
fn freshness(value: SourceFreshness) -> &'static str {
    match value {
        SourceFreshness::CurrentAtObservation => "Current at observation",
        SourceFreshness::Cached => "Cached",
        SourceFreshness::Unknown => "Unknown",
        SourceFreshness::Diverged => "Diverged",
    }
}
fn clipped(text: &str) -> String {
    if text.len() <= 64 * 1024 {
        return safe(text);
    }
    let mut end = 64 * 1024;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n[Document excerpt clipped at 64 KiB; the displayed source hash identifies the complete captured bytes.]",
        safe(&text[..end])
    )
}
impl SourcesWorkspace {
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let layout = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(4),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);
        Paragraph::new("Planning sources · immutable inspection · no fetch")
            .style(style.add_modifier(Modifier::BOLD))
            .render(layout[0], buffer);
        let label = match &self.selector {
            SourceSelector::WorkingTree => "Working tree".to_owned(),
            SourceSelector::Accepted => "Accepted ref".into(),
            SourceSelector::Proposal { reference } => format!("Proposal {reference}"),
            SourceSelector::Coordination => "Coordination ref".into(),
            SourceSelector::Staged { .. } => "Staged candidate".into(),
        };
        let identity = self.state().map(|state| {
            let observation = &state.view.observation; let source = &observation.identity;
            format!("{} · {} · {}\nRef: {} · Commit: {}\nObserved: {} · {}\nContent: {}", label, role(source.role), freshness(observation.freshness), source.ref_name.as_ref().map(ToString::to_string).unwrap_or_else(|| "none".into()), source.commit.as_ref().map(ToString::to_string).unwrap_or_else(|| "none".into()), observation.observed_at, observation.reason_codes.join(" · "), source.content)
        }).unwrap_or_else(|| format!("{label}\nNo source has been captured. Existing views and local drafts are retained.\nChoose an available source or finish workdeck init/migration, then press r."));
        Paragraph::new(safe(&identity))
            .style(style)
            .wrap(Wrap { trim: false })
            .render(layout[1], buffer);
        if let Some(form) = &self.proposal_form {
            super::shell_view::render_form(form, layout[2], buffer, theme);
        } else if let Some(state) = self.state_mut() {
            let wide = area.width >= 96;
            let columns = if wide {
                Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                    .split(layout[2])
            } else {
                Layout::vertical([
                    Constraint::Length(if state.opened.is_some() { 0 } else { 7 }),
                    Constraint::Min(0),
                ])
                .split(layout[2])
            };
            let selected = state
                .issues
                .iter()
                .position(|issue| Some(&issue.id) == state.selected.as_ref());
            let mut list_state = ListState::default()
                .with_selected(selected)
                .with_offset(state.offset);
            StatefulWidget::render(
                List::new(state.issues.iter().map(|issue| {
                    ListItem::new(sanitize_terminal_line(&format!(
                        "{} · {} · {}",
                        issue.id, issue.status, issue.title
                    )))
                }))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Captured issues "),
                )
                .style(style)
                .highlight_symbol("› ")
                .highlight_style(style.add_modifier(Modifier::BOLD)),
                columns[0],
                buffer,
                &mut list_state,
            );
            state.offset = list_state.offset();
            let text = if let Some(opened) = &state.opened {
                let identity = &opened.observation.identity;
                format!(
                    "{} · {}\n{} · {}\nSource content: {}\nDocument: {} · {}\n{}\n\n{}",
                    opened.issue.metadata.id,
                    opened.issue.metadata.title,
                    role(identity.role),
                    identity
                        .ref_name
                        .as_ref()
                        .map(ToString::to_string)
                        .unwrap_or_else(|| "working tree".into()),
                    identity.content,
                    opened.issue.path.display(),
                    opened.issue.source.content,
                    if identity != &state.view.observation.identity {
                        "Prior observation retained. Enter explicitly opens the selected current capture."
                    } else {
                        "Exact captured document; this reader does not open the working-tree file."
                    },
                    clipped(&opened.document)
                )
            } else {
                "Enter opens the selected issue from these immutable bytes.\n\nAccepted and proposal records retain distinct source identities. This view performs no edits, fetches, claims, or publication.\n\nNo issues can mean this is a coordination-only source; inspect Claims for typed coordination status.".into()
            };
            Paragraph::new(safe(&text))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Captured document citation "),
                )
                .style(style)
                .wrap(Wrap { trim: false })
                .scroll((state.scroll, 0))
                .render(columns[1], buffer);
        }
        let mut footer = "l working tree · a accepted · p proposal ref · c coordination · r capture\nEnter document · 7 Claims · o operations · PgUp/PgDn · F2 Review".to_owned();
        if let Some(error) = &self.error {
            footer = format!(
                "{} · retained source was not rebased\n{}",
                error.message, footer
            );
        }
        Paragraph::new(safe(&footer))
            .style(style)
            .wrap(Wrap { trim: false })
            .render(layout[3], buffer);
    }
}
