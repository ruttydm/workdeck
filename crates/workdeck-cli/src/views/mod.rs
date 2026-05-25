use crate::app::{
    App, ChangeGrouping, FileBrowserEntryKind, FocusPane, GitRowKind, LayoutMode, PreviewKind, Tab,
    TreeRowKind,
};
use crate::store::IssueStatus;
use crate::syntax::SyntaxHighlighter;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use std::collections::BTreeMap;
use std::path::Path;

const ADD_COLOR: Color = Color::Rgb(0, 128, 0);
const DELETE_COLOR: Color = Color::Rgb(176, 0, 0);

pub fn render(app: &App, highlighter: &SyntaxHighlighter, frame: &mut Frame) {
    let area = frame.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(app, chunks[0], frame);
    render_body(app, highlighter, chunks[1], frame);
    render_status(app, chunks[2], frame);

    if app.help_visible {
        render_help(area, frame);
    }
}

fn render_header(app: &App, area: Rect, frame: &mut Frame) {
    let mut spans = Vec::new();
    for tab in Tab::ALL {
        if tab == app.active_tab {
            spans.push(Span::styled(
                format!(" {} ", tab.title()),
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(
                format!(" {} ", tab.title()),
                Style::default().fg(Color::Gray),
            ));
        }
    }

    let used = spans
        .iter()
        .map(|span| span.content.chars().count())
        .sum::<usize>();
    let available_path = (area.width as usize).saturating_sub(used + 1);
    if available_path >= 8 {
        spans.push(Span::styled(" ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            trim_middle(&app.repo_root.to_string_lossy(), available_path),
            Style::default().fg(Color::DarkGray),
        ));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_body(app: &App, highlighter: &SyntaxHighlighter, area: Rect, frame: &mut Frame) {
    let layout = LayoutMode::for_width(area.width);
    match layout {
        LayoutMode::Narrow => {
            if app.preview_visible
                && app.focus == FocusPane::Preview
                && matches!(
                    app.active_tab,
                    Tab::Changes | Tab::Git | Tab::Files | Tab::Issues | Tab::Agents
                )
                && app.preview_target().is_some()
            {
                render_preview(app, highlighter, area, frame);
            } else {
                render_primary(app, area, frame);
            }
        }
        LayoutMode::Medium => {
            if app.preview_visible {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(43), Constraint::Percentage(57)])
                    .split(area);
                render_primary(app, chunks[0], frame);
                render_preview(app, highlighter, chunks[1], frame);
            } else {
                render_primary(app, area, frame);
            }
        }
        LayoutMode::Wide => {
            if app.preview_visible {
                let chunks = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(area);
                if app.active_tab == Tab::Changes {
                    render_preview(app, highlighter, chunks[0], frame);
                    render_primary(app, chunks[1], frame);
                } else {
                    render_primary(app, chunks[0], frame);
                    render_preview(app, highlighter, chunks[1], frame);
                }
            } else {
                render_primary(app, area, frame);
            }
        }
    }
}

fn render_primary(app: &App, area: Rect, frame: &mut Frame) {
    match app.active_tab {
        Tab::Changes => render_changes(app, area, frame),
        Tab::Git => render_git(app, area, frame),
        Tab::Files => render_files(app, area, frame),
        Tab::Issues => render_issues(app, area, frame),
        Tab::Agents => render_agents(app, area, frame),
        Tab::Search => render_search(app, area, frame),
    }
}

fn render_changes(app: &App, area: Rect, frame: &mut Frame) {
    match app.change_grouping {
        ChangeGrouping::Directory => render_changes_by_directory(app, area, frame),
        ChangeGrouping::Status => render_changes_by_status(app, area, frame),
    }
}

fn render_changes_by_directory(app: &App, area: Rect, frame: &mut Frame) {
    let mut items = Vec::new();
    let mut selected_row = None;
    let dir_stats = directory_stats(&app.changes);
    if !app.changes.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            change_summary(&app.changes),
            Style::default().fg(Color::Gray),
        ))));
    }

    for (row_index, row) in app.change_tree_rows().into_iter().enumerate() {
        let selected = row_index == app.selected_change_row;
        if selected {
            selected_row = Some(items.len());
        }
        match row.kind {
            TreeRowKind::Directory => {
                let stats = dir_stats
                    .get(&row.path.to_string_lossy().to_string())
                    .copied()
                    .unwrap_or_default();
                let indent = "  ".repeat(row.depth);
                let marker = if row.collapsed { "+ " } else { "> " };
                items.push(ListItem::new(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(marker, Style::default().fg(Color::Blue)),
                    Span::styled(
                        format!(
                            "{}/",
                            row.path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        selected_style(selected, ""),
                    ),
                    Span::raw(dirstat_label(app, stats)),
                ])));
            }
            TreeRowKind::File => {
                let Some(file_index) = row.file_index else {
                    continue;
                };
                let Some(file) = app.changes.get(file_index) else {
                    continue;
                };
                let name = row
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| row.path.to_string_lossy().to_string());
                let indent = "  ".repeat(row.depth);
                let available_name_width =
                    area.width.saturating_sub(15 + indent.len() as u16) as usize;
                let line = format!(
                    "{}{} {:<width$}",
                    indent,
                    change_glyph(file),
                    trim_middle(&name, available_name_width.max(8)),
                    width = available_name_width.max(8)
                );
                items.push(ListItem::new(change_line(
                    line,
                    file.additions,
                    file.deletions,
                    selected,
                    file.kind.marker(),
                )));
            }
        }
    }

    if items.is_empty() && app.loading {
        items.push(ListItem::new("loading changes..."));
    } else if items.is_empty() {
        items.push(ListItem::new("clean worktree"));
    }

    let items = visible_items(items, selected_row, area);
    frame.render_widget(
        List::new(items).block(block(&format!("Changes: {}", app.change_grouping.label()))),
        area,
    );
}

fn render_changes_by_status(app: &App, area: Rect, frame: &mut Frame) {
    let mut items = Vec::new();
    let mut selected_row = None;

    if !app.changes.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            change_summary(&app.changes),
            Style::default().fg(Color::Gray),
        ))));
    }

    for kind in [
        crate::git::ChangeKind::Modified,
        crate::git::ChangeKind::Added,
        crate::git::ChangeKind::Deleted,
        crate::git::ChangeKind::Renamed,
        crate::git::ChangeKind::Typechange,
        crate::git::ChangeKind::Untracked,
        crate::git::ChangeKind::Conflicted,
    ] {
        let changes = app
            .changes
            .iter()
            .enumerate()
            .filter(|(_, change)| change.kind == kind)
            .collect::<Vec<_>>();
        if changes.is_empty() {
            continue;
        }

        let stats = DirectoryStats {
            files: changes.len(),
            additions: changes.iter().map(|(_, change)| change.additions).sum(),
            deletions: changes.iter().map(|(_, change)| change.deletions).sum(),
        };
        items.push(ListItem::new(Line::from(vec![
            Span::styled(kind.label(), Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(dirstat_label(app, stats)),
        ])));

        for (change_index, file) in changes {
            let selected = change_index == app.selected_change;
            if selected {
                selected_row = Some(items.len());
            }
            let available_width = area.width.saturating_sub(15) as usize;
            let line = format!(
                "  {} {:<width$}",
                change_glyph(file),
                trim_middle(&file.path.to_string_lossy(), available_width.max(8)),
                width = available_width.max(8)
            );
            items.push(ListItem::new(change_line(
                line,
                file.additions,
                file.deletions,
                selected,
                file.kind.marker(),
            )));
        }
    }

    if items.is_empty() && app.loading {
        items.push(ListItem::new("loading changes..."));
    } else if items.is_empty() {
        items.push(ListItem::new("clean worktree"));
    }

    let items = visible_items(items, selected_row, area);
    frame.render_widget(
        List::new(items).block(block(&format!("Changes: {}", app.change_grouping.label()))),
        area,
    );
}

#[derive(Debug, Clone, Copy, Default)]
struct DirectoryStats {
    files: usize,
    additions: usize,
    deletions: usize,
}

fn directory_stats(changes: &[crate::git::ChangeEntry]) -> BTreeMap<String, DirectoryStats> {
    let mut stats = BTreeMap::new();
    for change in changes {
        let dirs = path_dirs(&change.path);
        for depth in 0..dirs.len() {
            let dir = dirs[..=depth].join("/");
            let entry = stats.entry(dir).or_insert_with(DirectoryStats::default);
            entry.files += 1;
            entry.additions += change.additions;
            entry.deletions += change.deletions;
        }
    }
    stats
}

fn dirstat_label(app: &App, stats: DirectoryStats) -> String {
    if app.dirstat_visible {
        let churn = churn_label(stats.additions, stats.deletions);
        if churn.is_empty() {
            format!(" {}", stats.files)
        } else {
            format!(" {} {churn}", stats.files)
        }
    } else {
        format!(" {}", stats.files)
    }
}

fn change_summary(changes: &[crate::git::ChangeEntry]) -> String {
    let mut counts = BTreeMap::<crate::git::ChangeKind, usize>::new();
    let mut additions = 0;
    let mut deletions = 0;
    let mut staged = 0;
    let mut unstaged = 0;
    for change in changes {
        *counts.entry(change.kind).or_insert(0) += 1;
        additions += change.additions;
        deletions += change.deletions;
        staged += usize::from(change.staged);
        unstaged += usize::from(change.unstaged);
    }

    let mut parts = Vec::new();
    for kind in [
        crate::git::ChangeKind::Modified,
        crate::git::ChangeKind::Added,
        crate::git::ChangeKind::Deleted,
        crate::git::ChangeKind::Renamed,
        crate::git::ChangeKind::Typechange,
        crate::git::ChangeKind::Untracked,
        crate::git::ChangeKind::Conflicted,
    ] {
        if let Some(count) = counts.get(&kind) {
            parts.push(format!("{} {count}", kind.label()));
        }
    }

    let stage = match (staged, unstaged) {
        (0, 0) => String::new(),
        (0, unstaged) => format!("unstaged {unstaged}"),
        (staged, 0) => format!("staged {staged}"),
        (staged, unstaged) => format!("staged {staged} unstaged {unstaged}"),
    };
    let churn = churn_label(additions, deletions);
    let mut summary = vec![format!("{} files", changes.len())];
    if !parts.is_empty() {
        summary.push(parts.join(" "));
    }
    if !stage.is_empty() {
        summary.push(stage);
    }
    if !churn.is_empty() {
        summary.push(churn);
    }
    summary.join(" | ")
}

fn path_dirs(path: &Path) -> Vec<String> {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| {
            parent
                .components()
                .map(|component| component.as_os_str().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default()
}

fn render_git(app: &App, area: Rect, frame: &mut Frame) {
    let Some(overview) = &app.git_overview else {
        let message = if app.loading {
            "loading git..."
        } else {
            "no git overview"
        };
        frame.render_widget(
            List::new(vec![ListItem::new(message)]).block(block("Git")),
            area,
        );
        return;
    };

    let rows = app.git_rows();
    let mut items = Vec::new();
    let mut selected_row = None;
    let mut current_section = "";
    for (row_index, row) in rows.iter().enumerate() {
        if row.section != current_section {
            current_section = row.section;
            items.push(ListItem::new(Line::from(Span::styled(
                row.section,
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ))));
            if row.section == "Git" {
                items.push(ListItem::new(Line::from(Span::styled(
                    format!(
                        "  base: {}",
                        overview.base_branch.as_deref().unwrap_or("none")
                    ),
                    Style::default().fg(Color::Gray),
                ))));
                if let Some(remote) = overview.remotes.first() {
                    items.push(ListItem::new(Line::from(Span::styled(
                        format!("  remote: {}", remote.name),
                        Style::default().fg(Color::Gray),
                    ))));
                }
            }
        }

        let selected = row_index == app.selected_git_row;
        if selected {
            selected_row = Some(items.len());
        }
        let label_width = area.width.saturating_sub(26) as usize;
        let prefix = if selected { "> " } else { "  " };
        items.push(ListItem::new(Line::from(Span::styled(
            format!(
                "{prefix}{:<width$} {}",
                trim_middle(&row.label, label_width.max(8)),
                trim_middle(&row.detail, 22),
                width = label_width.max(8)
            ),
            selected_style(selected, ""),
        ))));
    }

    let items = if items.is_empty() {
        vec![ListItem::new("no git data")]
    } else {
        items
    };
    let items = visible_items(items, selected_row, area);
    frame.render_widget(List::new(items).block(block("Git")), area);
}

fn render_files(app: &App, area: Rect, frame: &mut Frame) {
    if LayoutMode::for_width(area.width) == LayoutMode::Narrow {
        render_file_browser(app, area, frame);
        return;
    }

    let mut items = Vec::new();
    let mut selected_row = None;

    for (row_index, row) in app.file_tree_rows().into_iter().enumerate() {
        let selected = row_index == app.selected_file_row;
        if selected {
            selected_row = Some(items.len());
        }
        match row.kind {
            TreeRowKind::Directory => {
                let indent = "  ".repeat(row.depth);
                let marker = if row.collapsed { "+ " } else { "> " };
                items.push(ListItem::new(Line::from(vec![
                    Span::raw(indent),
                    Span::styled(marker, Style::default().fg(Color::Blue)),
                    Span::styled(
                        format!(
                            "{}/",
                            row.path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        selected_style(selected, ""),
                    ),
                ])));
            }
            TreeRowKind::File => {
                let name = row
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| row.path.to_string_lossy().to_string());
                let indent = "  ".repeat(row.depth);
                let available_name_width =
                    area.width.saturating_sub(6 + indent.len() as u16) as usize;
                items.push(ListItem::new(Line::from(Span::styled(
                    format!(
                        "{}{}",
                        indent,
                        trim_middle(&name, available_name_width.max(8))
                    ),
                    selected_style(selected, ""),
                ))));
            }
        }
    }

    let items = if items.is_empty() && app.loading {
        vec![ListItem::new("loading files...")]
    } else if items.is_empty() {
        vec![ListItem::new("no files")]
    } else {
        items
    };
    let items = visible_items(items, selected_row, area);
    frame.render_widget(List::new(items).block(block("Files")), area);
}

fn render_file_browser(app: &App, area: Rect, frame: &mut Frame) {
    let mut items = Vec::new();
    let mut selected_row = None;

    for (index, entry) in app.file_browser_entries().into_iter().enumerate() {
        let selected = index == app.selected_file_entry;
        if selected {
            selected_row = Some(items.len());
        }
        let style = match entry.kind {
            FileBrowserEntryKind::Parent | FileBrowserEntryKind::Directory => {
                selected_style(selected, "").fg(Color::Blue)
            }
            FileBrowserEntryKind::File => selected_style(selected, ""),
        };
        items.push(ListItem::new(Line::from(Span::styled(
            trim_middle(&entry.name, area.width.saturating_sub(4) as usize),
            style,
        ))));
    }

    let items = if items.is_empty() && app.loading {
        vec![ListItem::new("loading files...")]
    } else if items.is_empty() {
        vec![ListItem::new("no files")]
    } else {
        items
    };
    let items = visible_items(items, selected_row, area);
    frame.render_widget(
        List::new(items).block(block(&file_browser_title(app, area.width))),
        area,
    );
}

fn file_browser_title(app: &App, width: u16) -> String {
    let mut title = String::from("Files");
    for component in app.files_cwd.components() {
        title.push_str(" > ");
        title.push_str(&component.as_os_str().to_string_lossy());
    }
    trim_middle(&title, width.saturating_sub(2) as usize)
}

fn render_issues(app: &App, area: Rect, frame: &mut Frame) {
    let mut items = Vec::new();
    let mut selected_row = None;
    let selected_key = app
        .issues
        .get(app.selected_issue)
        .map(|issue| issue.key.as_str());
    for status in IssueStatus::ALL {
        items.push(ListItem::new(Line::from(Span::styled(
            status.label(),
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD),
        ))));
        for issue in app.issues.iter().filter(|issue| issue.status == status) {
            let selected = Some(issue.key.as_str()) == selected_key;
            if selected {
                selected_row = Some(items.len());
            }
            items.push(ListItem::new(Line::from(Span::styled(
                format!(
                    "  {:<7} {:<28} {}",
                    issue.key,
                    trim_middle(&issue.title, 28),
                    issue.priority.label()
                ),
                if selected {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else {
                    Style::default()
                },
            ))));
        }
    }
    let items = visible_items(items, selected_row, area);
    frame.render_widget(List::new(items).block(block("Issues")), area);
}

fn render_agents(app: &App, area: Rect, frame: &mut Frame) {
    let items = app
        .sessions
        .iter()
        .enumerate()
        .map(|(index, session)| {
            let selected = index == app.selected_session;
            ListItem::new(Line::from(Span::styled(
                format!(
                    "{:<8} {:<30} {}",
                    session.agent,
                    trim_middle(&session.title, 30),
                    session.status
                ),
                if selected {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else {
                    Style::default()
                },
            )))
        })
        .collect::<Vec<_>>();
    let items = if items.is_empty() {
        vec![ListItem::new("no agent sessions")]
    } else {
        items
    };
    let selected = (!app.sessions.is_empty()).then_some(app.selected_session);
    let items = visible_items(items, selected, area);
    frame.render_widget(List::new(items).block(block("Agents")), area);
}

fn render_search(app: &App, area: Rect, frame: &mut Frame) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1)])
        .split(area);
    frame.render_widget(
        Paragraph::new(app.search_query.as_str()).block(block("Search")),
        chunks[0],
    );

    let items = app
        .search_results
        .iter()
        .enumerate()
        .map(|(index, result)| {
            let selected = index == app.selected_search;
            ListItem::new(Line::from(Span::styled(
                format!(
                    "{:<48} {}",
                    trim_middle(&result.record.label, 48),
                    result.record.detail
                ),
                if selected {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else {
                    Style::default()
                },
            )))
        })
        .collect::<Vec<_>>();
    let selected = (!app.search_results.is_empty()).then_some(app.selected_search);
    let items = visible_items(items, selected, chunks[1]);
    frame.render_widget(List::new(items).block(block("Results")), chunks[1]);
}

fn render_preview(app: &App, highlighter: &SyntaxHighlighter, area: Rect, frame: &mut Frame) {
    let Some(preview) = app.selected_preview() else {
        let message = if app.preview_loading.is_some() {
            "loading preview..."
        } else {
            "no preview"
        };
        frame.render_widget(Paragraph::new(message).block(block("Preview")), area);
        return;
    };
    let syntax_path = preview_syntax_path(app, &preview.title);
    let viewport_height = area.height.saturating_sub(1) as usize;
    let line_count = preview.content.lines().count();
    let max_scroll = line_count.saturating_sub(viewport_height.max(1));
    let scroll = app.preview_scroll.min(max_scroll);
    let text = if preview.binary {
        Text::from(
            preview
                .content
                .lines()
                .skip(scroll)
                .take(viewport_height)
                .map(|line| Line::from(line.to_string()))
                .collect::<Vec<_>>(),
        )
    } else {
        let mut text =
            highlighter.highlight(&syntax_path, &preview.content, scroll + viewport_height);
        text.lines = text
            .lines
            .into_iter()
            .skip(scroll)
            .take(viewport_height)
            .collect();
        text
    };
    frame.render_widget(
        Paragraph::new(text)
            .block(block(&preview_title(app, &preview.title)))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn preview_syntax_path(app: &App, title: &str) -> std::path::PathBuf {
    match app.preview_target().map(|target| target.kind) {
        Some(PreviewKind::Issue) => std::path::PathBuf::from("issue.md"),
        Some(PreviewKind::Agent) => std::path::PathBuf::from("agent.md"),
        Some(PreviewKind::GitSummary | PreviewKind::GitTag | PreviewKind::GitRemote) => {
            std::path::PathBuf::from("git.md")
        }
        _ => Path::new(title).to_path_buf(),
    }
}

fn preview_title(app: &App, title: &str) -> String {
    if app.focus == FocusPane::Preview {
        format!("Preview: {title} [focus]")
    } else {
        format!("Preview: {title}")
    }
}

fn render_status(app: &App, area: Rect, frame: &mut Frame) {
    let status = trim_end(
        &format!(
            " {}  |  {}  |  {}",
            context_summary(app),
            key_hint(app),
            app.status_message
        ),
        area.width as usize,
    );
    frame.render_widget(
        Paragraph::new(status).style(Style::default().fg(Color::Gray)),
        area,
    );
}

fn context_summary(app: &App) -> String {
    let focus = match app.focus {
        FocusPane::Tree => "TREE",
        FocusPane::Preview => "PREVIEW",
    };
    match app.active_tab {
        Tab::Changes => app
            .changes
            .get(app.selected_change)
            .map(|change| {
                format!(
                    "{focus} Changes:{} {}/{} {} {} +{} -{}",
                    app.change_grouping.label(),
                    app.selected_change.saturating_add(1),
                    app.changes.len(),
                    trim_middle(&change.path.to_string_lossy(), 36),
                    change.stage_label(),
                    change.additions,
                    change.deletions
                )
            })
            .unwrap_or_else(|| format!("{focus} Changes clean")),
        Tab::Git => app
            .selected_git_row_data()
            .map(|row| match row.kind {
                GitRowKind::Summary => app
                    .git_overview
                    .as_ref()
                    .map(|overview| {
                        format!(
                            "{focus} Git {} ↑{} ↓{} base {}",
                            overview.current_branch,
                            overview.ahead,
                            overview.behind,
                            overview.base_branch.as_deref().unwrap_or("none")
                        )
                    })
                    .unwrap_or_else(|| format!("{focus} Git empty")),
                GitRowKind::Commit(_) => {
                    format!("{focus} Git commit {}", trim_middle(&row.label, 36))
                }
                GitRowKind::Branch(_) => {
                    format!("{focus} Git branch {}", trim_middle(&row.label, 36))
                }
                GitRowKind::Stash(_) => {
                    format!("{focus} Git stash {}", trim_middle(&row.label, 36))
                }
                GitRowKind::Tag(_) => format!("{focus} Git tag {}", trim_middle(&row.label, 36)),
                GitRowKind::Remote(_) => {
                    format!("{focus} Git remote {}", trim_middle(&row.label, 36))
                }
            })
            .unwrap_or_else(|| format!("{focus} Git empty")),
        Tab::Files => app
            .selected_path()
            .map(|path| {
                format!(
                    "{focus} Files {}/{} {}",
                    app.selected_file_entry.saturating_add(1),
                    app.file_browser_entries().len(),
                    trim_middle(&path.to_string_lossy(), 44)
                )
            })
            .unwrap_or_else(|| format!("{focus} Files empty")),
        Tab::Issues => app
            .issues
            .get(app.selected_issue)
            .map(|issue| {
                format!(
                    "{focus} Issues {}/{} {} {} {}",
                    app.selected_issue.saturating_add(1),
                    app.issues.len(),
                    issue.key,
                    issue.status.label(),
                    trim_middle(&issue.title, 32)
                )
            })
            .unwrap_or_else(|| format!("{focus} Issues empty")),
        Tab::Agents => app
            .sessions
            .get(app.selected_session)
            .map(|session| {
                format!(
                    "{focus} Agents {}/{} {} {}",
                    app.selected_session.saturating_add(1),
                    app.sessions.len(),
                    session.status,
                    trim_middle(&session.title, 36)
                )
            })
            .unwrap_or_else(|| format!("{focus} Agents empty")),
        Tab::Search => app
            .search_results
            .get(app.selected_search)
            .map(|result| {
                format!(
                    "{focus} Search {}/{} /{} -> {}",
                    app.selected_search.saturating_add(1),
                    app.search_results.len(),
                    trim_middle(&app.search_query, 18),
                    trim_middle(&result.record.label, 32)
                )
            })
            .unwrap_or_else(|| format!("{focus} Search /{}", trim_middle(&app.search_query, 40))),
    }
}

fn key_hint(app: &App) -> &'static str {
    if app.focus == FocusPane::Preview {
        return "j/k scroll  g/G top/bottom  h tree  q quit";
    }
    match app.active_tab {
        Tab::Changes => "h collapse  l/Enter preview/expand  g group  / search",
        Tab::Git => "Enter preview  b base  p PRs  / search",
        Tab::Files => "h parent/collapse  l/Enter open/preview  . root  / search",
        Tab::Issues => "Enter edit  s status  p priority  Space jump",
        Tab::Agents => "Enter preview  Space file  / search",
        Tab::Search => "type filter  Enter jump  Esc close",
    }
}

fn render_help(area: Rect, frame: &mut Frame) {
    let popup = centered_rect(70, 70, area);
    frame.render_widget(Clear, popup);
    let lines = vec![
        Line::from("Workdeck keys"),
        Line::from(""),
        Line::from("j/k or arrows     move selection or preview scroll"),
        Line::from("h/l               collapse/expand or tree/preview focus"),
        Line::from("Tab / Shift-Tab  switch tabs"),
        Line::from("Enter            focus preview or open issue"),
        Line::from("g/G              preview top/bottom"),
        Line::from(".                repo root in narrow Files"),
        Line::from("Esc              close help or search input"),
        Line::from("/                search"),
        Line::from("g                group changes by directory/status"),
        Line::from("w                toggle dirstat weight"),
        Line::from("n                create issue from selection"),
        Line::from("e                edit selected issue in $EDITOR"),
        Line::from("s                cycle issue status"),
        Line::from("p                cycle issue priority"),
        Line::from("l                toggle configured label"),
        Line::from("A                assign/unassign issue"),
        Line::from("Space            jump linked issue/file"),
        Line::from("L                link selected file to selected issue"),
        Line::from("o                open file in $EDITOR"),
        Line::from("y                copy selected path or issue key"),
        Line::from("r                refresh"),
        Line::from("q                quit"),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(dialog_block("Help"))
            .alignment(Alignment::Left),
        popup,
    );
}

fn block(title: &str) -> Block<'_> {
    Block::default().title(title).borders(Borders::TOP)
}

fn dialog_block(title: &str) -> Block<'_> {
    Block::default().title(title).borders(Borders::ALL)
}

fn visible_items<'a>(
    items: Vec<ListItem<'a>>,
    selected_row: Option<usize>,
    area: Rect,
) -> Vec<ListItem<'a>> {
    let visible = area.height.saturating_sub(1).max(1) as usize;
    if items.len() <= visible {
        return items;
    }

    let selected = selected_row.unwrap_or(0).min(items.len() - 1);
    let start = selected
        .saturating_sub(visible / 2)
        .min(items.len().saturating_sub(visible));
    items.into_iter().skip(start).take(visible).collect()
}

fn selected_style(selected: bool, marker: &str) -> Style {
    if selected {
        Style::default()
            .fg(Color::Black)
            .bg(Color::White)
            .add_modifier(Modifier::BOLD)
    } else if marker == "A" || marker == "?" {
        Style::default().fg(Color::Green)
    } else if marker == "D" {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    }
}

fn change_glyph(change: &crate::git::ChangeEntry) -> &'static str {
    if change.staged && change.unstaged {
        "S+"
    } else if change.staged {
        "S "
    } else {
        match change.kind {
            crate::git::ChangeKind::Modified => "M ",
            crate::git::ChangeKind::Added => "A ",
            crate::git::ChangeKind::Deleted => "D ",
            crate::git::ChangeKind::Renamed => "R ",
            crate::git::ChangeKind::Typechange => "T ",
            crate::git::ChangeKind::Untracked => "+ ",
            crate::git::ChangeKind::Conflicted => "! ",
        }
    }
}

fn change_line(
    prefix: String,
    additions: usize,
    deletions: usize,
    selected: bool,
    marker: &str,
) -> Line<'static> {
    if selected {
        return Line::from(Span::styled(
            format!("{prefix} {}", churn_label(additions, deletions)),
            selected_style(true, marker),
        ));
    }

    Line::from(vec![
        Span::styled(prefix, selected_style(false, marker)),
        Span::styled(
            format!(" {}", churn_label(additions, deletions)),
            churn_style(additions, deletions),
        ),
    ])
}

fn churn_label(additions: usize, deletions: usize) -> String {
    match (additions, deletions) {
        (0, 0) => String::new(),
        (_, 0) => format!("+{additions}"),
        (0, _) => format!("-{deletions}"),
        _ => format!("+{additions}/-{deletions}"),
    }
}

fn churn_style(additions: usize, deletions: usize) -> Style {
    if deletions > additions {
        Style::default().fg(DELETE_COLOR)
    } else if additions > 0 {
        Style::default().fg(ADD_COLOR)
    } else if deletions > 0 {
        Style::default().fg(DELETE_COLOR)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

fn trim_middle(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let left = (max_chars - 3) / 2;
    let right = max_chars - 3 - left;
    let prefix = value.chars().take(left).collect::<String>();
    let suffix = value
        .chars()
        .rev()
        .take(right)
        .collect::<String>()
        .chars()
        .rev()
        .collect::<String>();
    format!("{prefix}...{suffix}")
}

fn trim_end(value: &str, max_chars: usize) -> String {
    let count = value.chars().count();
    if count <= max_chars {
        return value.to_string();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let visible = max_chars.saturating_sub(3);
    format!("{}...", value.chars().take(visible).collect::<String>())
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::config::Config;
    use crate::git::{ChangeEntry, ChangeKind, RepoSnapshot};
    use crate::search::{SearchIndex, SearchRecord, SearchResult, SearchTarget};
    use crate::store::WorkdeckStore;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;

    #[test]
    fn trims_middle_without_overflowing() {
        assert_eq!(trim_middle("abcdef", 6), "abcdef");
        assert_eq!(trim_middle("abcdefghijklmnopqrstuvwxyz", 10), "abc...wxyz");
        assert_eq!(trim_end("abcdefghijklmnopqrstuvwxyz", 10), "abcdefg...");
    }

    #[test]
    fn computes_nested_change_directory_stats() {
        let changes = vec![
            ChangeEntry {
                path: PathBuf::from("app/Http/Controllers/UserController.php"),
                kind: ChangeKind::Modified,
                staged: false,
                unstaged: true,
                additions: 1,
                deletions: 0,
            },
            ChangeEntry {
                path: PathBuf::from("app/Models/User.php"),
                kind: ChangeKind::Modified,
                staged: false,
                unstaged: true,
                additions: 2,
                deletions: 0,
            },
        ];

        let stats = directory_stats(&changes);

        assert_eq!(stats.get("app").unwrap().files, 2);
        assert_eq!(stats.get("app").unwrap().additions, 3);
        assert_eq!(stats.get("app/Http").unwrap().files, 1);
        assert_eq!(stats.get("app/Http/Controllers").unwrap().files, 1);
        assert_eq!(stats.get("app/Models").unwrap().files, 1);
    }

    #[test]
    fn summarizes_changes_by_status_and_stage() {
        let changes = vec![
            ChangeEntry {
                path: PathBuf::from("src/main.rs"),
                kind: ChangeKind::Modified,
                staged: true,
                unstaged: true,
                additions: 3,
                deletions: 1,
            },
            ChangeEntry {
                path: PathBuf::from("src/lib.rs"),
                kind: ChangeKind::Untracked,
                staged: false,
                unstaged: true,
                additions: 2,
                deletions: 0,
            },
        ];

        let summary = change_summary(&changes);

        assert!(summary.contains("2 files"));
        assert!(summary.contains("modified 1"));
        assert!(summary.contains("untracked 1"));
        assert!(summary.contains("staged 1 unstaged 2"));
        assert!(summary.contains("+5/-1"));
    }

    #[test]
    fn renders_narrow_changes_screen() {
        let mut terminal = Terminal::new(TestBackend::new(44, 20)).unwrap();
        let app = test_app();
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Changes"));
        assert!(rendered.contains("src"));
        assert!(rendered.contains("modified 1"));
        assert!(rendered.contains("+3/-1"));
        assert!(rendered.contains("M"));
        assert!(!rendered.contains("[ U]"));
    }

    #[test]
    fn renders_staged_and_unstaged_change_marker() {
        let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
        let app = test_app_with_changes(vec![ChangeEntry {
            path: PathBuf::from("src/main.rs"),
            kind: ChangeKind::Modified,
            staged: true,
            unstaged: true,
            additions: 2,
            deletions: 1,
        }]);
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("S+"));
        assert!(!rendered.contains("[SU]"));
    }

    #[test]
    fn narrow_changes_defaults_to_tree_even_when_preview_is_enabled() {
        let mut terminal = Terminal::new(TestBackend::new(44, 22)).unwrap();
        let mut app = test_app();
        app.preview_visible = true;
        app.focus = FocusPane::Tree;
        app.preview_cache = Some(crate::app::PreviewCache {
            target: app.preview_target().unwrap(),
            preview: crate::git::FilePreview {
                title: "src/main.rs".to_string(),
                content: "preview should stay hidden until focused".to_string(),
                truncated: false,
                binary: false,
            },
        });
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Changes"));
        assert!(rendered.contains("main.rs"));
        assert!(!rendered.contains("preview should stay hidden"));
    }

    #[test]
    fn renders_changes_grouped_by_status() {
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
        let mut app = test_app_with_changes(vec![
            ChangeEntry {
                path: PathBuf::from("src/main.rs"),
                kind: ChangeKind::Modified,
                staged: false,
                unstaged: true,
                additions: 3,
                deletions: 1,
            },
            ChangeEntry {
                path: PathBuf::from("README.md"),
                kind: ChangeKind::Untracked,
                staged: false,
                unstaged: true,
                additions: 5,
                deletions: 0,
            },
        ]);
        app.change_grouping = ChangeGrouping::Status;
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Changes: status"));
        assert!(rendered.contains("modified"));
        assert!(rendered.contains("untracked"));
        assert!(rendered.contains("src/main.rs"));
        assert!(rendered.contains("README.md"));
    }

    #[test]
    fn dirstat_label_can_hide_churn_weight() {
        let mut app = test_app();
        let stats = DirectoryStats {
            files: 3,
            additions: 12,
            deletions: 4,
        };

        assert_eq!(dirstat_label(&app, stats), " 3 +12/-4");
        app.dirstat_visible = false;
        assert_eq!(dirstat_label(&app, stats), " 3");
    }

    #[test]
    fn change_rows_colorize_churn_when_unselected() {
        let line = change_line("M  file.rs".to_string(), 4, 2, false, "M");

        assert_eq!(line.spans[1].style.fg, Some(ADD_COLOR));
        assert_eq!(line.spans.len(), 2);
        assert!(line.spans[1].content.contains("+4/-2"));
    }

    #[test]
    fn change_glyphs_are_compact_and_stage_aware() {
        let mut change = ChangeEntry {
            path: PathBuf::from("new.txt"),
            kind: ChangeKind::Untracked,
            staged: false,
            unstaged: true,
            additions: 1,
            deletions: 0,
        };

        assert_eq!(change_glyph(&change), "+ ");
        change.staged = true;
        change.unstaged = false;
        assert_eq!(change_glyph(&change), "S ");
        change.unstaged = true;
        assert_eq!(change_glyph(&change), "S+");
    }

    #[test]
    fn footer_context_explains_current_selection() {
        let app = test_app();
        let summary = context_summary(&app);

        assert!(summary.contains("Changes:directory"));
        assert!(summary.contains("1/1"));
        assert!(summary.contains("src/main.rs"));
        assert!(summary.contains("unstaged"));
    }

    #[test]
    fn renders_wide_changes_screen_with_preview_without_details() {
        let mut terminal = Terminal::new(TestBackend::new(120, 28)).unwrap();
        let mut app = test_app();
        app.preview_visible = true;
        app.preview_cache = Some(crate::app::PreviewCache {
            target: app.preview_target().unwrap(),
            preview: crate::git::FilePreview {
                title: "diff src/main.rs".to_string(),
                content: "# unstaged\n+hello\n".to_string(),
                truncated: false,
                binary: false,
            },
        });
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Changes: directory"));
        assert!(rendered.contains("# unstaged"));
        assert!(rendered.contains("+hello"));
        assert!(!rendered.contains("Details"));
        assert!(!rendered.contains("Actions"));
    }

    #[test]
    fn wide_preview_hidden_uses_full_width_without_details() {
        let mut terminal = Terminal::new(TestBackend::new(120, 20)).unwrap();
        let mut app = test_app_with_changes(vec![ChangeEntry {
            path: PathBuf::from(
                "src/very_long_file_name_that_should_fit_when_preview_is_hidden.rs",
            ),
            kind: ChangeKind::Modified,
            staged: false,
            unstaged: true,
            additions: 1,
            deletions: 0,
        }]);
        app.preview_visible = false;
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("very_long_file_name_that_should_fit_when_preview_is_hidden.rs"));
        assert!(!rendered.contains("Preview"));
        assert!(!rendered.contains("Details"));
        assert!(!rendered.contains("Actions"));
    }

    #[test]
    fn compact_header_keeps_tabs_on_one_line() {
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Files;
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(!rendered.contains(" workdeck "));
        assert!(rendered.contains("Changes"));
        assert!(rendered.contains("Git"));
        assert!(rendered.contains("Files"));
        assert!(rendered.contains("Issues"));
        assert!(!rendered.contains("Details"));
    }

    #[test]
    fn renders_narrow_git_tab_summary_and_sections() {
        let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Git;
        app.git_overview = Some(test_git_overview());
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Git"));
        assert!(rendered.contains("feature/git"));
        assert!(rendered.contains("base: origin/main"));
        assert!(rendered.contains("Branches"));
        assert!(rendered.contains("Recent commits"));
        assert!(rendered.contains("Stashes"));
        assert!(rendered.contains("Tags"));
        assert!(rendered.contains("Remotes"));
    }

    #[test]
    fn renders_git_commit_preview_when_cached() {
        let mut terminal = Terminal::new(TestBackend::new(120, 22)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Git;
        app.preview_visible = true;
        app.git_overview = Some(test_git_overview());
        app.selected_git_row = 2;
        let target = app.preview_target().unwrap();
        app.preview_cache = Some(crate::app::PreviewCache {
            target,
            preview: crate::git::FilePreview {
                title: "commit abc123456".to_string(),
                content: "diff --git a/README.md b/README.md\n+hello\n".to_string(),
                truncated: false,
                binary: false,
            },
        });
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("diff --git"));
        assert!(rendered.contains("+hello"));
    }

    #[test]
    fn narrow_issue_preview_uses_issue_contents_when_visible() {
        let mut terminal = Terminal::new(TestBackend::new(44, 14)).unwrap();
        let mut app = test_app();
        let mut issue = crate::store::Issue::new("WD-1".to_string(), "Follow up".to_string());
        issue.description = "Issue body, not linked file body.".to_string();
        issue.linked_files = vec!["src/issue_link.rs".to_string()];
        app.issues = vec![issue];
        app.active_tab = Tab::Issues;
        app.preview_visible = true;
        app.focus = FocusPane::Preview;
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("WD-1 Follow up"));
        assert!(rendered.contains("Issue body"));
        assert!(rendered.contains("src/issue_link.rs"));
        assert!(!rendered.contains("issue linked preview"));
    }

    #[test]
    fn issue_preview_uses_markdown_syntax_path() {
        let mut app = test_app();
        app.active_tab = Tab::Issues;
        app.issues = vec![crate::store::Issue::new(
            "WD-1".to_string(),
            "Highlight me".to_string(),
        )];

        assert_eq!(
            preview_syntax_path(&app, "WD-1 Highlight me"),
            Path::new("issue.md")
        );
    }

    #[test]
    fn narrow_agent_preview_uses_session_contents_when_visible() {
        let mut terminal = Terminal::new(TestBackend::new(44, 14)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Agents;
        app.preview_visible = true;
        app.focus = FocusPane::Preview;
        app.sessions = vec![crate::store::AgentSession {
            id: "session-1".to_string(),
            title: "Touch file".to_string(),
            agent: "codex".to_string(),
            cwd: "/tmp/workdeck".to_string(),
            status: "active".to_string(),
            started_at: "2026-05-24T12:00:00Z".to_string(),
            ended_at: String::new(),
            goal: "Keep agent work state visible.".to_string(),
            summary: "Session summary body.".to_string(),
            plan: vec!["Preview the session, not only a file.".to_string()],
            commands_run: vec!["cargo test".to_string()],
            tests_run: vec!["cargo test".to_string()],
            handoff_notes: vec!["Check narrow layout.".to_string()],
            touched_files: vec![crate::store::AgentTouchedFile {
                path: "src/agent_touch.rs".to_string(),
                change_type: "modified".to_string(),
            }],
            extra: toml::Table::new(),
        }];
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Touch file"));
        assert!(!rendered.contains("agent touched preview"));
    }

    #[test]
    fn agent_preview_uses_markdown_syntax_path() {
        let mut app = test_app();
        app.active_tab = Tab::Agents;
        app.sessions = vec![crate::store::AgentSession {
            id: "session-1".to_string(),
            title: "Highlight agent".to_string(),
            agent: "codex".to_string(),
            cwd: "/tmp/workdeck".to_string(),
            status: "active".to_string(),
            started_at: String::new(),
            ended_at: String::new(),
            goal: String::new(),
            summary: String::new(),
            plan: Vec::new(),
            commands_run: Vec::new(),
            tests_run: Vec::new(),
            handoff_notes: Vec::new(),
            touched_files: Vec::new(),
            extra: toml::Table::new(),
        }];

        assert_eq!(
            preview_syntax_path(&app, "agent session-1"),
            Path::new("agent.md")
        );
    }

    #[test]
    fn renders_selected_change_in_long_narrow_list() {
        let mut terminal = Terminal::new(TestBackend::new(44, 14)).unwrap();
        let mut app = test_app_with_changes(
            (0..50)
                .map(|index| ChangeEntry {
                    path: PathBuf::from(format!("src/file_{index}.rs")),
                    kind: ChangeKind::Modified,
                    staged: false,
                    unstaged: true,
                    additions: index,
                    deletions: 0,
                })
                .collect(),
        );
        app.selected_change = 49;
        app.selected_change_row = 50;
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("file_49.rs"));
        assert!(!rendered.contains("file_0.rs"));
    }

    #[test]
    fn renders_selected_file_in_long_file_list() {
        let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Files;
        app.preview_visible = false;
        app.files = (0..80)
            .map(|index| PathBuf::from(format!("src/file_{index}.rs")))
            .collect();
        app.selected_file = 79;
        app.selected_file_row = 80;
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("file_79.rs"));
        assert!(!rendered.contains("file_0.rs"));
    }

    #[test]
    fn renders_files_as_nested_tree() {
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Files;
        app.preview_visible = false;
        app.files = vec![
            PathBuf::from("app/Http/Controllers/UserController.php"),
            PathBuf::from("app/Models/User.php"),
            PathBuf::from("resources/js/pages/Dashboard.vue"),
        ];
        app.selected_file = 0;
        app.selected_file_row = 3;
        app.reveal_file_in_browser(Path::new("app/Http/Controllers/UserController.php"));
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("app/"));
        assert!(rendered.contains("Http/"));
        assert!(rendered.contains("Controllers/"));
        assert!(rendered.contains("UserController.php"));
        assert!(context_summary(&app).contains("app/Http/Controllers/UserController.php"));
    }

    #[test]
    fn renders_narrow_files_as_drill_down_browser() {
        let mut terminal = Terminal::new(TestBackend::new(44, 14)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Files;
        app.preview_visible = true;
        app.files = vec![
            PathBuf::from("crates/workdeck-cli/src/app.rs"),
            PathBuf::from("crates/workdeck-cli/src/views/mod.rs"),
            PathBuf::from("README.md"),
        ];
        app.files_cwd = PathBuf::from("crates/workdeck-cli/src");
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("Files > crates > workdeck-cli > src"));
        assert!(rendered.contains("../"));
        assert!(rendered.contains("app.rs"));
        assert!(rendered.contains("views/"));
        assert!(!rendered.contains("crates/workdeck-cli/src/app.rs"));
    }

    #[test]
    fn renders_selected_search_result_in_long_result_list() {
        let mut terminal = Terminal::new(TestBackend::new(80, 16)).unwrap();
        let mut app = test_app();
        app.active_tab = Tab::Search;
        app.search_results = (0..80)
            .map(|index| SearchResult {
                score: 100 - index,
                record: SearchRecord {
                    label: format!("src/file_{index}.rs"),
                    haystack: format!("src/file_{index}.rs"),
                    detail: "file".to_string(),
                    target: SearchTarget::File(PathBuf::from(format!("src/file_{index}.rs"))),
                },
            })
            .collect();
        app.selected_search = 79;
        let highlighter = SyntaxHighlighter::default();

        terminal
            .draw(|frame| render(&app, &highlighter, frame))
            .unwrap();

        let rendered = format!("{:?}", terminal.backend().buffer());
        assert!(rendered.contains("file_79.rs"));
        assert!(!rendered.contains("file_0.rs"));
    }

    fn test_app() -> App {
        let change = ChangeEntry {
            path: PathBuf::from("src/main.rs"),
            kind: ChangeKind::Modified,
            staged: false,
            unstaged: true,
            additions: 3,
            deletions: 1,
        };
        test_app_with_changes(vec![change])
    }

    fn test_app_with_changes(changes: Vec<ChangeEntry>) -> App {
        App {
            cwd: PathBuf::from("/tmp/workdeck"),
            repo_root: PathBuf::from("/tmp/workdeck"),
            config: Config::default(),
            store: WorkdeckStore::new("/tmp/workdeck/.agents/workdeck"),
            active_tab: Tab::Changes,
            preview_visible: false,
            focus: FocusPane::Tree,
            preview_scroll: 0,
            collapsed_change_dirs: std::collections::BTreeSet::new(),
            collapsed_file_dirs: std::collections::BTreeSet::new(),
            change_grouping: ChangeGrouping::Directory,
            dirstat_visible: true,
            help_visible: false,
            search_query: String::new(),
            status_message: String::new(),
            loading: false,
            refresh_pending: false,
            refresh_generation: 0,
            changes: changes.clone(),
            snapshot: Some(RepoSnapshot {
                root: PathBuf::from("/tmp/workdeck"),
                changes: changes.clone(),
                groups: crate::git::group_by_directory(&changes),
            }),
            git_overview: None,
            files: vec![PathBuf::from("src/main.rs")],
            issues: Vec::new(),
            sessions: Vec::new(),
            reference_data: crate::store::ReferenceData::default(),
            symbols: Vec::new(),
            search_index: SearchIndex::default(),
            search_results: Vec::new(),
            preview_cache: None,
            preview_loading: None,
            selected_change: 0,
            selected_change_row: usize::from(
                changes
                    .first()
                    .and_then(|change| change.path.parent())
                    .is_some_and(|parent| !parent.as_os_str().is_empty()),
            ),
            selected_git_row: 0,
            selected_file: 0,
            selected_file_row: 1,
            files_cwd: PathBuf::new(),
            selected_file_entry: 0,
            file_browser_scroll: 0,
            last_selected_by_dir: std::collections::BTreeMap::new(),
            selected_issue: 0,
            selected_session: 0,
            selected_search: 0,
        }
    }

    fn test_git_overview() -> crate::git::GitOverview {
        crate::git::GitOverview {
            current_branch: "feature/git".to_string(),
            upstream: Some("origin/feature/git".to_string()),
            ahead: 2,
            behind: 0,
            base_branch: Some("origin/main".to_string()),
            remotes: vec![crate::git::GitRemote {
                name: "origin".to_string(),
                fetch_url: "https://example.test/repo.git".to_string(),
                push_url: "https://example.test/repo.git".to_string(),
            }],
            branches: vec![
                crate::git::GitBranch {
                    name: "feature/git".to_string(),
                    is_current: true,
                    is_remote: false,
                    upstream: Some("origin/feature/git".to_string()),
                },
                crate::git::GitBranch {
                    name: "origin/main".to_string(),
                    is_current: false,
                    is_remote: true,
                    upstream: None,
                },
            ],
            recent_commits: vec![crate::git::GitCommit {
                sha: "abc123456".to_string(),
                short_sha: "abc1234".to_string(),
                summary: "Add Git tab".to_string(),
                author: "Rutger".to_string(),
                date: "2026-05-25".to_string(),
            }],
            stashes: vec![crate::git::GitStash {
                name: "stash@{0}".to_string(),
                summary: "WIP on feature/git".to_string(),
            }],
            tags: vec![crate::git::GitTag {
                name: "v0.1.0".to_string(),
            }],
        }
    }
}
