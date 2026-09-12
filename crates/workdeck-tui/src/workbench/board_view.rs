//! Bounded board painting; all cards carry the same immutable query handle.
use super::{indexed_view::RenderedIndex, indexed_workspace::IndexedWorkspace};
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Widget},
};
use workdeck_diff::sanitize_terminal_line;

pub(super) fn render(
    workspace: &mut IndexedWorkspace,
    area: Rect,
    buffer: &mut Buffer,
    theme: &AppTheme,
) -> RenderedIndex {
    let regular = Style::default()
        .fg(ratatui_theme_color(&theme.text))
        .bg(ratatui_theme_color(&theme.panel));
    let highlight = regular
        .bg(ratatui_theme_color(&theme.accent_muted))
        .add_modifier(Modifier::BOLD);
    Block::default().style(regular).render(area, buffer);
    let regions = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(if workspace.opened.is_some() { 35 } else { 65 }),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(area);
    let count = workspace.handle.as_ref().map_or(0, |handle| handle.total);
    let state = if workspace.stale() {
        "stale retained view"
    } else if !workspace.is_idle() {
        "loading"
    } else {
        "current"
    };
    let source = workspace
        .handle
        .as_ref()
        .map(|handle| {
            format!(
                "{:?} · {} · {}",
                handle.view.source.role, handle.view.source.repository, handle.view.source.content
            )
        })
        .unwrap_or_else(|| "Loading source".into());
    Paragraph::new(format!(
        "Issues board · {:?} · {count} · {state}\n{}",
        workspace.board_group,
        sanitize_terminal_line(&source)
    ))
    .style(regular)
    .render(regions[0], buffer);
    let column_count = usize::from((area.width / 28).clamp(1, 5));
    let card_rows = usize::from(regions[1].height.saturating_sub(2) / 3).max(1);
    workspace.resize_board(column_count, card_rows);
    let columns = Layout::horizontal(vec![
        Constraint::Ratio(1, column_count as u32);
        column_count
    ])
    .split(regions[1]);
    let selected = workspace.selected_row().map(|row| row.token.clone());
    let mut hits = Vec::new();
    for (area, column) in columns
        .iter()
        .zip(workspace.board_columns.iter().filter(|column| {
            workspace.board_ready() && Some(&column.page.handle) == workspace.handle.as_ref()
        }))
    {
        let title = format!(
            "{} · {}",
            column.group.value.as_deref().unwrap_or("Unassigned"),
            column.group.count
        );
        Block::default()
            .borders(Borders::ALL)
            .title(sanitize_terminal_line(&title))
            .style(regular)
            .render(*area, buffer);
        let inner = area.inner(ratatui::layout::Margin::new(1, 1));
        for (index, row) in column.page.rows.iter().take(card_rows).enumerate() {
            let y = inner.y.saturating_add((index as u16).saturating_mul(3));
            let height = inner.bottom().saturating_sub(y).min(3);
            if height == 0 {
                break;
            }
            let card = Rect::new(inner.x, y, inner.width, height);
            let style = if selected.as_ref() == Some(&row.token) {
                highlight
            } else {
                regular
            };
            Paragraph::new(vec![
                Line::from(sanitize_terminal_line(&row.title)),
                Line::from(sanitize_terminal_line(&row.token.key.id)),
                Line::from(""),
            ])
            .style(style)
            .render(card, buffer);
            hits.push((card, row.token.clone()));
        }
    }
    if count == 0 {
        Paragraph::new(if workspace.handle.is_some() {
            "No issues match this board"
        } else {
            "Loading board…"
        })
        .style(regular)
        .render(regions[1], buffer);
    }
    let detail = workspace
        .opened
        .as_ref()
        .map(|detail| {
            format!(
                "{} · source {}\n{}",
                detail.row.token.key.id,
                detail.row.token.content,
                workspace
                    .opened_lines
                    .iter()
                    .skip(workspace.detail_scroll)
                    .take(usize::from(regions[2].height.saturating_sub(3)))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
        .unwrap_or_else(|| "Enter opens the exact selected source.".into());
    Paragraph::new(detail)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Opened source"),
        )
        .style(regular)
        .render(regions[2], buffer);
    let error = workspace
        .worker_error
        .as_deref()
        .or(workspace.error.as_ref().map(|error| error.message.as_str()))
        .unwrap_or("Left/Right columns · Up/Down cards · z grouping · w list");
    Paragraph::new(format!(
        "{}\nEnter source · Shift-PgDn scroll · e edit · s status",
        sanitize_terminal_line(error)
    ))
    .style(regular)
    .render(regions[3], buffer);
    RenderedIndex {
        list: regions[1],
        detail: regions[2],
        selected,
        rows: hits,
    }
}
