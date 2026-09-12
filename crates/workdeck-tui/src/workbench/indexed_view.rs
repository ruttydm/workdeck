//! Painting visits only the requested viewport and an already captured excerpt.
use super::indexed_workspace::IndexedWorkspace;
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget},
};
use workdeck_diff::{format_terminal_path, sanitize_terminal_line};
use workdeck_pm::projection::{ProjectionQuery, ProjectionRowToken};

#[derive(Debug)]
pub(super) struct RenderedIndex {
    pub list: Rect,
    pub detail: Rect,
    pub selected: Option<ProjectionRowToken>,
    pub rows: Vec<(Rect, ProjectionRowToken)>,
}

fn row_title(row: &workdeck_pm::projection::ProjectionRow, query: &ProjectionQuery) -> String {
    let Some(position) = &row.tree else {
        return row.title.clone();
    };
    let collapsed = matches!(query, ProjectionQuery::Features { query } if query.collapsed.iter().any(|id| id.as_str() == row.token.key.id));
    let marker = if position.children == 0 {
        "·"
    } else if collapsed {
        "▸"
    } else {
        "▾"
    };
    let depth = if position.depth > 8 {
        format!("[depth {}] ", position.depth)
    } else {
        String::new()
    };
    format!(
        "{}{marker} {depth}{}{}",
        "  ".repeat(position.depth.min(8)),
        row.title,
        if position.parent_outside_view {
            " [parent outside view]"
        } else {
            ""
        }
    )
}

pub(super) fn render(
    workspace: &mut IndexedWorkspace,
    area: Rect,
    buffer: &mut Buffer,
    theme: &AppTheme,
) -> RenderedIndex {
    if workspace.board {
        return super::board_view::render(workspace, area, buffer, theme);
    }
    let regular = Style::default()
        .fg(ratatui_theme_color(&theme.text))
        .bg(ratatui_theme_color(&theme.panel));
    let muted = Style::default().fg(ratatui_theme_color(&theme.muted));
    Block::default().style(regular).render(area, buffer);
    let regions = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(area);
    let title = match &workspace.query {
        ProjectionQuery::Issues { .. } => "Issues",
        ProjectionQuery::Features { .. } => "Features",
        ProjectionQuery::Planning { .. } => "Planning",
        ProjectionQuery::Activity { .. } => "Activity",
        ProjectionQuery::Records { .. } => "Records",
    };
    let progress = if workspace.refreshing {
        " · refreshing"
    } else if workspace.querying {
        " · filtering"
    } else if workspace.loading_page {
        " · loading rows"
    } else {
        ""
    };
    let stale = if workspace.stale() {
        " · stale retained view"
    } else {
        ""
    };
    let observation = workspace
        .status
        .as_ref()
        .and_then(|status| status.observation.as_ref())
        .map(|observation| format!("{:?}", observation.freshness))
        .unwrap_or_else(|| "observation unavailable".into());
    let source = workspace
        .handle
        .as_ref()
        .map(|handle| {
            format!(
                "{:?} · {observation} · {} · source {}",
                handle.view.source.role, handle.view.source.repository, handle.view.source.content
            )
        })
        .unwrap_or_else(|| "Loading source".into());
    Paragraph::new(vec![
        Line::from(format!(
            "{title} · {}{progress}{stale}",
            workspace.handle.as_ref().map_or(0, |handle| handle.total)
        )),
        Line::from(sanitize_terminal_line(&source)),
    ])
    .style(regular)
    .render(regions[0], buffer);
    let columns = if area.width >= 100 {
        Layout::horizontal([Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(regions[1])
    } else {
        Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(regions[1])
    };
    let inner = columns[0].inner(ratatui::layout::Margin::new(1, 1));
    workspace.resize(inner.height / 2);
    let mut hits = Vec::new();
    let mut selected = None;
    let mut items = workspace
        .viewport()
        .visible()
        .enumerate()
        .map(|(index, ordinal)| {
            if workspace.viewport().selected() == Some(ordinal) {
                selected = Some(index);
            }
            let Some(row) = workspace.row_at(ordinal as usize) else {
                return ListItem::new(vec![Line::from("Loading row…"), Line::from("")]);
            };
            hits.push((
                Rect::new(
                    inner.x,
                    inner.y.saturating_add((index as u16).saturating_mul(2)),
                    inner.width,
                    2.min(
                        inner
                            .height
                            .saturating_sub((index as u16).saturating_mul(2)),
                    ),
                ),
                row.token.clone(),
            ));
            let state =
                if row.decision.is_some() || row.maturity.is_some() || row.availability.is_some() {
                    format!(
                        "Decision {} · Maturity {} · Availability {}",
                        named(row.decision.as_ref()),
                        named(row.maturity.as_ref()),
                        named(row.availability.as_ref())
                    )
                } else {
                    if matches!(workspace.query, ProjectionQuery::Activity { .. }) {
                        format!(
                            "{:?} · {}",
                            row.token.key.kind,
                            row.updated_at
                                .as_deref()
                                .or(row.created_at.as_deref())
                                .unwrap_or("time unknown")
                        )
                    } else {
                        row.status.clone().unwrap_or_default()
                    }
                };
            ListItem::new(vec![
                Line::from(sanitize_terminal_line(&row_title(row, &workspace.query))),
                Line::styled(
                    sanitize_terminal_line(&format!("{} · {state}", row.token.key.id)),
                    muted,
                ),
            ])
        })
        .collect::<Vec<_>>();
    if workspace
        .handle
        .as_ref()
        .is_some_and(|handle| handle.total == 0)
        && !workspace.querying
    {
        let message = if workspace.query == ProjectionQuery::default() {
            "No issues yet"
        } else {
            "No records match this query"
        };
        items.push(ListItem::new(message));
    }
    let mut list_state = ListState::default().with_selected(selected);
    StatefulWidget::render(
        List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .style(regular)
            .highlight_symbol("› ")
            .highlight_style(
                Style::default()
                    .bg(ratatui_theme_color(&theme.accent_muted))
                    .add_modifier(Modifier::BOLD),
            ),
        columns[0],
        buffer,
        &mut list_state,
    );
    let detail_area = columns[1].inner(ratatui::layout::Margin::new(1, 1));
    let mut lines = Vec::new();
    if let Some(detail) = &workspace.opened {
        lines.push(Line::from(sanitize_terminal_line(&format!(
            "{} · source {}",
            format_terminal_path(&detail.row.token.path.to_string_lossy()),
            detail.row.token.content
        ))));
        lines.push(Line::from(sanitize_terminal_line(&format!(
            "{:?} · {}",
            detail.row.token.view.source.role, detail.row.token.key.repository
        ))));
        if detail.omitted_document_bytes > 0 || detail.total_relations > detail.relations.len() {
            lines.push(Line::from(format!(
                "Excerpt · {} document bytes and {} relationships omitted",
                detail.omitted_document_bytes,
                detail
                    .total_relations
                    .saturating_sub(detail.relations.len())
            )));
        }
        lines.extend(
            workspace
                .opened_lines
                .iter()
                .skip(workspace.detail_scroll)
                .take(usize::from(detail_area.height).saturating_sub(lines.len()))
                .cloned()
                .map(Line::from),
        );
    } else {
        lines.push(Line::from("Enter opens the exact selected source."));
    }
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Opened source"),
        )
        .style(regular)
        .render(columns[1], buffer);
    let error = workspace
        .worker_error
        .clone()
        .or_else(|| workspace.error.as_ref().map(|error| error.message.clone()))
        .or_else(|| {
            workspace
                .status
                .as_ref()
                .and_then(|status| status.diagnostics.first())
                .map(|error| error.message.clone())
        });
    let footer = error.map(|error| format!("{}\nw board · Home/End · PgUp/PgDn · Enter source · r refresh", sanitize_terminal_line(&error)))
        .unwrap_or_else(|| "w board · Home/End · PgUp/PgDn · Enter source · r refresh\nShift-PgUp/PgDn scroll the retained opened source.".into());
    Paragraph::new(footer)
        .style(muted)
        .render(regions[2], buffer);
    RenderedIndex {
        list: columns[0],
        detail: columns[1],
        selected: workspace.selected_row().map(|row| row.token.clone()),
        rows: hits,
    }
}

fn named(value: Option<&impl std::fmt::Debug>) -> String {
    value
        .map(|value| format!("{value:?}"))
        .unwrap_or_else(|| "unknown".into())
}
