use super::{DraftInput, DraftKey, WorkbenchController, WorkbenchPane};
use crate::{AppTheme, ratatui_theme_color};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use workdeck_diff::{format_terminal_path, sanitize_terminal_line};
use workdeck_pm::IssueId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchLayout {
    pub header: Rect,
    pub list: Option<Rect>,
    pub detail: Option<Rect>,
    pub footer: Rect,
}

impl WorkbenchLayout {
    pub fn new(area: Rect, pane: WorkbenchPane) -> Self {
        let rows = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
        let (list, detail) = if area.width >= 96 {
            let columns = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(rows[1]);
            (Some(columns[0]), Some(columns[1]))
        } else if pane == WorkbenchPane::List {
            (Some(rows[1]), None)
        } else {
            (None, Some(rows[1]))
        };
        Self {
            header: rows[0],
            list,
            detail,
            footer: rows[2],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedWorkbench {
    pub layout: WorkbenchLayout,
    pub visible_issue_ids: Vec<IssueId>,
    #[cfg(test)]
    pub(super) formatted_issue_rows: usize,
}

/// The parent shell allocates this rectangle beside or instead of its native
/// review canvas. Painting owns no input routing, terminal mode, or I/O.
pub fn render_workbench(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut WorkbenchController,
    theme: &AppTheme,
) -> RenderedWorkbench {
    render_surface(frame, area, state, theme)
}

pub(crate) fn render_workbench_buffer(
    buffer: &mut ratatui::buffer::Buffer,
    area: Rect,
    state: &mut WorkbenchController,
    theme: &AppTheme,
) -> RenderedWorkbench {
    render_surface(buffer, area, state, theme)
}

trait WorkbenchSurface {
    fn render_widget<W: Widget>(&mut self, widget: W, area: Rect);
    fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        area: Rect,
        state: &mut W::State,
    );
}

impl WorkbenchSurface for Frame<'_> {
    fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        Frame::render_widget(self, widget, area);
    }
    fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        area: Rect,
        state: &mut W::State,
    ) {
        Frame::render_stateful_widget(self, widget, area, state);
    }
}

impl WorkbenchSurface for ratatui::buffer::Buffer {
    fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self);
    }
    fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        area: Rect,
        state: &mut W::State,
    ) {
        widget.render(area, self, state);
    }
}

fn render_surface(
    frame: &mut impl WorkbenchSurface,
    area: Rect,
    state: &mut WorkbenchController,
    theme: &AppTheme,
) -> RenderedWorkbench {
    let layout = WorkbenchLayout::new(area, state.view().pane);
    let foreground = ratatui_theme_color(&theme.text);
    let background = ratatui_theme_color(&theme.panel);
    let muted = Style::default()
        .fg(ratatui_theme_color(&theme.muted))
        .bg(background);
    let regular = Style::default().fg(foreground).bg(background);
    frame.render_widget(Block::default().style(regular), area);
    let filter = state.filter();
    let mut active = Vec::new();
    if !filter.query.is_empty() {
        active.push(filter.query.clone());
    }
    for (name, value) in [
        ("project", filter.project.as_ref()),
        ("cycle", filter.cycle.as_ref()),
        ("milestone", filter.milestone.as_ref()),
        ("label", filter.label.as_ref()),
        ("status", filter.status.as_ref()),
        ("assignee", filter.assignee.as_ref()),
    ] {
        if let Some(value) = value {
            active.push(format!("{name}:{value}"));
        }
    }
    if !filter.targets.is_empty() {
        active.push(format!(
            "targets({}):{}",
            match filter.target_match {
                workdeck_pm::TargetMatch::All => "all",
                workdeck_pm::TargetMatch::Any => "any",
            },
            filter.targets.join(",")
        ));
    }
    if filter.archived_only {
        active.push("archive:archived".into());
    } else if filter.include_archived {
        active.push("archive:all".into());
    }
    let query = sanitize_terminal_line(&active.join(" · "));
    let heading = if query.is_empty() {
        format!("Issues · {}", state.visible_issues().len())
    } else {
        format!("Issues · {} · {query}", state.visible_issues().len())
    };
    frame.render_widget(
        Paragraph::new(heading).style(regular.add_modifier(Modifier::BOLD)),
        layout.header,
    );
    let mut visible_issue_ids = Vec::new();
    #[cfg(test)]
    let mut formatted_issue_rows = 0;
    if let Some(list_area) = layout.list {
        let border = block("Issues", state.view().pane == WorkbenchPane::List, theme);
        if state.visible_issues().len() == 0 {
            let text = if state.issues().is_empty() {
                "No issues yet"
            } else {
                "No matching issues"
            };
            frame.render_widget(Paragraph::new(text).block(border).style(muted), list_area);
        } else {
            let height = list_area.height.saturating_sub(2);
            let viewport = super::virtual_viewport::VirtualViewport::new(
                state.visible_issues().len() as u64,
                state.selected_index().map(|index| index as u64),
                state.view().list_offset as u64,
                u64::from(if height == 0 { 0 } else { (height / 2).max(1) }),
            );
            let visible = viewport.visible();
            let items = state
                .visible_issues()
                .skip(visible.start as usize)
                .take((visible.end - visible.start) as usize)
                .map(|issue| {
                    ListItem::new(vec![
                        Line::from(sanitize_terminal_line(&issue.metadata.title)),
                        Line::styled(
                            format!(
                                "{} · {}",
                                issue.metadata.id,
                                sanitize_terminal_line(&issue.metadata.status)
                            ),
                            muted,
                        ),
                    ])
                })
                .collect::<Vec<_>>();
            #[cfg(test)]
            {
                formatted_issue_rows = items.len();
            }
            let mut list_state = ListState::default().with_selected(
                viewport
                    .selected()
                    .map(|index| (index - viewport.offset()) as usize),
            );
            frame.render_stateful_widget(
                List::new(items)
                    .block(border)
                    .style(regular)
                    .highlight_style(
                        Style::default()
                            .bg(ratatui_theme_color(&theme.accent_muted))
                            .add_modifier(Modifier::BOLD),
                    )
                    .highlight_symbol("› "),
                list_area,
                &mut list_state,
            );
            state.view_mut().list_offset = viewport.offset() as usize + list_state.offset();
            visible_issue_ids = state
                .visible_issues()
                .skip(state.view().list_offset)
                .take((visible.end - visible.start) as usize - list_state.offset())
                .map(|issue| issue.metadata.id.clone())
                .collect();
        }
    }
    if let Some(detail_area) = layout.detail {
        let (title, lines) = detail_lines(state);
        frame.render_widget(
            Paragraph::new(lines)
                .block(block(
                    &title,
                    state.view().pane == WorkbenchPane::Detail,
                    theme,
                ))
                .style(regular)
                .wrap(Wrap { trim: false })
                .scroll((state.view().detail_scroll, 0)),
            detail_area,
        );
    }
    let footer = if let Some(error) = state.error() {
        let prefix = if error.after_commit {
            "Saved; refresh failed"
        } else if error.draft.is_some() {
            "Draft retained"
        } else {
            "Error"
        };
        let mut text = format!("{prefix}: {}", sanitize_terminal_line(&error.error.message));
        if let Some(path) = &error.error.path {
            text.push_str(&format!(" · {}", format_terminal_path(path)));
        }
        text
    } else if state.active_draft().is_some() {
        "Draft · Save or discard · Returning to review keeps this draft".into()
    } else {
        "Create · Edit · Comment · Status · Assign · Priority · Labels · Open file or commit".into()
    };
    frame.render_widget(
        Paragraph::new(footer)
            .style(muted)
            .wrap(Wrap { trim: false }),
        layout.footer,
    );
    RenderedWorkbench {
        layout,
        visible_issue_ids,
        #[cfg(test)]
        formatted_issue_rows,
    }
}

fn block<'a>(title: &'a str, focused: bool, theme: &AppTheme) -> Block<'a> {
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ratatui_theme_color(if focused {
            &theme.accent
        } else {
            &theme.border
        })))
}

fn detail_lines(state: &WorkbenchController) -> (String, Vec<Line<'static>>) {
    if let Some((key, draft)) = state.active_draft() {
        let mut lines = Vec::new();
        let title = match (&draft.input, key) {
            (DraftInput::Create(input), _) => {
                lines.push(Line::from(format!(
                    "Title: {}",
                    sanitize_terminal_line(&input.title)
                )));
                lines.extend(body_lines(&input.body));
                "New issue draft".to_owned()
            }
            (DraftInput::Edit(input), DraftKey::Edit(id)) => {
                if let Some(title) = input
                    .fields
                    .get("title")
                    .and_then(serde_json::Value::as_str)
                {
                    lines.push(Line::from(format!(
                        "Title: {}",
                        sanitize_terminal_line(title)
                    )));
                }
                lines.extend(body_lines(input.body.as_deref().unwrap_or_default()));
                format!("Edit {id}")
            }
            (DraftInput::Comment { author, body }, _) => {
                lines.push(Line::from(format!(
                    "Author: {}",
                    sanitize_terminal_line(author)
                )));
                lines.extend(body_lines(body));
                "Comment draft".to_owned()
            }
            _ => "Draft".to_owned(),
        };
        return (title, lines);
    }
    let Some(issue) = state.selected_issue() else {
        return ("Issue".into(), vec![Line::from("Select an issue")]);
    };
    let meta = &issue.metadata;
    let mut lines = vec![
        Line::from(Span::styled(
            sanitize_terminal_line(&meta.title),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "{} · {:?}",
            sanitize_terminal_line(&meta.status),
            meta.priority
        )),
        Line::from(format!(
            "Assignee: {}",
            sanitize_terminal_line(meta.assignee.as_deref().unwrap_or("Unassigned"))
        )),
    ];
    if !meta.labels.is_empty() {
        lines.push(Line::from(format!(
            "Labels: {}",
            sanitize_terminal_line(&meta.labels.join(", "))
        )));
    }
    lines.extend(body_lines(&issue.body));
    if !meta.files.is_empty() {
        lines.push(Line::from("Files"));
        for link in &meta.files {
            let position = link.line.map(|line| format!(":{line}")).unwrap_or_default();
            lines.push(Line::from(format!(
                "  {}{position}",
                format_terminal_path(&link.path)
            )));
        }
    }
    if !meta.commits.is_empty() {
        lines.push(Line::from("Commits"));
        lines.extend(
            meta.commits
                .iter()
                .map(|sha| Line::from(format!("  {}", sanitize_terminal_line(sha)))),
        );
    }
    if !state.comments().is_empty() {
        lines.push(Line::from("Comments"));
        for comment in state.comments() {
            lines.push(Line::from(sanitize_terminal_line(&comment.author)));
            lines.extend(body_lines(&comment.body));
        }
    }
    (meta.id.to_string(), lines)
}

fn body_lines(body: &str) -> Vec<Line<'static>> {
    std::iter::once(Line::from(""))
        .chain(
            body.lines()
                .map(|line| Line::from(sanitize_terminal_line(line))),
        )
        .collect()
}
