use super::{PanelPage, PanelTarget, WorkbenchShell, input::TextField, shell::ShellEffect};
use crate::{AppTheme, ratatui_theme_color};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;

impl WorkbenchShell {
    pub(super) fn panel_paste(&mut self, page: PanelPage, text: &str) -> bool {
        let Some(panels) = self.panels.as_mut() else {
            return false;
        };
        let state = panels.state(page);
        if !state.query_editing {
            return false;
        }
        let mut field = TextField::new("Query", state.query.clone(), false);
        field.cursor = state.query_cursor;
        field.insert(text);
        state.query = field.value;
        state.query_cursor = field.cursor;
        true
    }

    pub(super) fn panel_key(&mut self, page: PanelPage, key: KeyEvent) -> bool {
        let Some(panels) = self.panels.as_mut() else {
            return false;
        };
        // Read the provider binding before the page-state loan so the base
        // action below can reach the provider without re-borrowing `panels`.
        let base_binding = if page == PanelPage::Git {
            panels.provider.git_base_key()
        } else {
            None
        };
        let state = panels.state(page);
        if state.query_editing {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => {
                    state.query_editing = false;
                    panels.refresh(page);
                }
                KeyCode::Backspace => {
                    crate::remove_filter_character_before(&mut state.query, &mut state.query_cursor)
                }
                KeyCode::Delete => {
                    crate::remove_filter_character_at(&mut state.query, &mut state.query_cursor)
                }
                KeyCode::Left => state.query_cursor = state.query_cursor.saturating_sub(1),
                KeyCode::Right => {
                    state.query_cursor = (state.query_cursor + 1).min(state.query.chars().count())
                }
                KeyCode::Home => state.query_cursor = 0,
                KeyCode::End => state.query_cursor = state.query.chars().count(),
                KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    state.query.clear();
                    state.query_cursor = 0;
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    self.panel_paste(page, &ch.to_string());
                }
                _ => {}
            }
            return true;
        }
        if !key.modifiers.is_empty() {
            return false;
        }
        // The visible old snapshot may be retained during refresh, but Enter
        // must never open its selection under a newer query or directory.
        if state.loading && matches!(key.code, KeyCode::Enter | KeyCode::Char('v' | 'y')) {
            return true;
        }
        // The configured comparison base outranks the panel's fixed keys,
        // mirroring the retired Git tab. Cycling is session-local; the refreshed
        // snapshot and previews re-read ahead/behind against the new branch.
        if let Some(binding) = base_binding
            && configured_panel_key(key, &binding)
        {
            match panels.provider.cycle_git_base_branch() {
                Ok(next) => {
                    self.notice = Some(format!("base branch {next}"));
                    panels.refresh(page);
                }
                Err(failure) => self.notice = Some(failure.message),
            }
            return true;
        }
        match key.code {
            KeyCode::Char('q') => return false,
            KeyCode::Char('/') => {
                state.query_editing = true;
                state.query_cursor = state.query.chars().count();
            }
            KeyCode::Char('r') => panels.refresh(page),
            KeyCode::Char('j') | KeyCode::Down => panels.move_selection(page, 1),
            KeyCode::Char('k') | KeyCode::Up => panels.move_selection(page, -1),
            KeyCode::PageDown => {
                state.location.preview_scroll = state.location.preview_scroll.saturating_add(12)
            }
            KeyCode::PageUp => {
                state.location.preview_scroll = state.location.preview_scroll.saturating_sub(12)
            }
            KeyCode::Home => {
                state.location.preview_scroll = 0;
                state.location.list_offset = 0;
                panels.move_selection(page, isize::MIN);
            }
            KeyCode::End => panels.move_selection(page, isize::MAX),
            KeyCode::Char('g') if page == PanelPage::Changes => state.grouped = !state.grouped,
            KeyCode::Char('d') if page == PanelPage::Changes => state.dirstat = !state.dirstat,
            KeyCode::Esc | KeyCode::Backspace => {
                if state.preview_visible {
                    state.preview_visible = false;
                } else if page == PanelPage::Files && !state.directory.is_empty() {
                    let parent = std::path::Path::new(&state.directory)
                        .parent()
                        .unwrap_or(std::path::Path::new(""))
                        .to_string_lossy()
                        .into_owned();
                    panels.set_directory(page, parent);
                }
            }
            KeyCode::Enter => {
                if let Some(target) = state.selected().map(|entry| entry.target.clone()) {
                    if let PanelTarget::Directory { path } = target {
                        panels.set_directory(page, path);
                    } else if state.preview_visible {
                        self.effect = Some(ShellEffect::PanelNavigate(target));
                    } else {
                        state.preview_visible = true;
                        panels.request_preview(page);
                    }
                }
            }
            KeyCode::Char('v') => {
                if let Some(target) = state.selected().map(|entry| entry.target.clone()) {
                    if let PanelTarget::Directory { path } = target {
                        panels.set_directory(page, path);
                    } else {
                        self.effect = Some(ShellEffect::PanelNavigate(target));
                    }
                }
            }
            KeyCode::Char('y') => {
                if let Some(reference) = state
                    .selected()
                    .and_then(|entry| entry.target.reference())
                    .filter(|reference| !reference.is_empty())
                {
                    self.effect = Some(ShellEffect::CopyReference(reference.to_owned()));
                } else {
                    self.notice = Some("Select an entry with a reference to copy".into());
                }
            }
            _ => {}
        }
        true
    }

    pub(super) fn render_repository_panel(
        &mut self,
        page: PanelPage,
        area: Rect,
        buffer: &mut Buffer,
        theme: &AppTheme,
    ) {
        self.planning_bounds = Some(area);
        self.panel_hits.clear();
        self.panel_list_bounds = None;
        self.panel_preview_bounds = None;
        let Some(panels) = self.panels.as_mut() else {
            return;
        };
        let repository_scope = panels.source.root.display().to_string();
        let state = panels.state(page);
        let normal = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        let muted = normal.fg(ratatui_theme_color(&theme.muted));
        let selected = normal
            .fg(ratatui_theme_color(&theme.accent))
            .add_modifier(Modifier::BOLD);
        Block::default().style(normal).render(area, buffer);
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);
        let title = state
            .snapshot
            .as_ref()
            .map(|s| s.title.as_str())
            .unwrap_or(page.title());
        let mut query = state.query.clone();
        if state.query_editing {
            let at = query
                .char_indices()
                .nth(state.query_cursor)
                .map_or(query.len(), |(at, _)| at);
            query.insert(at, '▏');
        }
        Paragraph::new(format!(
            "{}{}  {}\n/ {}\nRepository: {}",
            sanitize_terminal_line(title),
            if state.loading { " · loading" } else { "" },
            sanitize_terminal_line(&state.directory),
            sanitize_terminal_line(&query),
            sanitize_terminal_line(&repository_scope)
        ))
        .style(normal)
        .render(rows[0], buffer);
        let (list, preview) = if rows[1].width >= 96 {
            let columns =
                Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                    .split(rows[1]);
            (Some(columns[0]), Some(columns[1]))
        } else if state.preview_visible {
            (None, Some(rows[1]))
        } else {
            (Some(rows[1]), None)
        };
        if let Some(list) = list {
            self.panel_list_bounds = Some(list);
            let block = Block::default()
                .title(format!(
                    "{} · {}",
                    page.title(),
                    state.snapshot.as_ref().map_or(0, |s| s.entries.len())
                ))
                .borders(Borders::ALL)
                .style(normal);
            let inner = block.inner(list);
            block.render(list, buffer);
            if let Some(snapshot) = &state.snapshot {
                let mut rendered = Vec::<(Option<String>, Line<'static>)>::new();
                let mut section = String::new();
                for entry in &snapshot.entries {
                    if state.grouped && !entry.section.is_empty() && section != entry.section {
                        section = entry.section.clone();
                        let stats = if page == PanelPage::Changes && state.dirstat {
                            let (add, del) = snapshot
                                .entries
                                .iter()
                                .filter(|e| e.section == section)
                                .filter_map(|e| e.changes.as_ref())
                                .fold((0usize, 0usize), |(a, d), s| {
                                    (a.saturating_add(s.additions), d.saturating_add(s.deletions))
                                });
                            format!(" +{add} -{del}")
                        } else {
                            String::new()
                        };
                        rendered.push((
                            None,
                            Line::styled(
                                format!("{}{}", sanitize_terminal_line(&section), stats),
                                muted,
                            ),
                        ));
                    }
                    let active = Some(&entry.id) == state.location.selected.as_ref();
                    let style = if active { selected } else { normal };
                    rendered.push((
                        Some(entry.id.clone()),
                        Line::from(vec![
                            Span::styled(if active { "› " } else { "  " }, style),
                            Span::styled(sanitize_terminal_line(&entry.label), style),
                            Span::styled(
                                format!("  {}", sanitize_terminal_line(&entry.detail)),
                                muted,
                            ),
                        ]),
                    ));
                }
                let at = rendered
                    .iter()
                    .position(|(id, _)| {
                        id.as_ref() == state.location.selected.as_ref() && id.is_some()
                    })
                    .unwrap_or(0);
                let height = usize::from(inner.height);
                if at < state.location.list_offset {
                    state.location.list_offset = at;
                } else if at >= state.location.list_offset.saturating_add(height) {
                    state.location.list_offset = at.saturating_add(1).saturating_sub(height);
                }
                for (offset, (id, line)) in rendered
                    .into_iter()
                    .skip(state.location.list_offset)
                    .take(height)
                    .enumerate()
                {
                    let row = Rect::new(inner.x, inner.y + offset as u16, inner.width, 1);
                    if let Some(id) = id {
                        self.panel_hits.push((row, id));
                    }
                    Paragraph::new(line).render(row, buffer);
                }
                if snapshot.entries.is_empty() {
                    Paragraph::new(if state.loading {
                        "Loading repository data…"
                    } else {
                        "No matching entries"
                    })
                    .style(muted)
                    .render(inner, buffer);
                }
            } else {
                Paragraph::new(if state.loading {
                    "Loading repository data…"
                } else {
                    "No snapshot available. Press r to retry."
                })
                .style(muted)
                .render(inner, buffer);
            }
        }
        if let Some(preview) = preview {
            self.panel_preview_bounds = Some(preview);
            let current = state.selected().map(|entry| &entry.target);
            let loaded = state
                .preview
                .as_ref()
                .filter(|(target, _)| Some(target) == current)
                .map(|(_, value)| value);
            let title = loaded
                .map(|p| sanitize_terminal_line(&p.title))
                .unwrap_or_else(|| "Preview".into());
            let body = if let Some(error) = &state.preview_error {
                format!(
                    "{}\n{}",
                    error.message,
                    error.hint.as_deref().unwrap_or("Press r to retry")
                )
            } else if state.preview_loading {
                "Loading preview…".into()
            } else if let Some(value) = loaded {
                if value.binary {
                    "Binary file · preview unavailable".into()
                } else {
                    format!(
                        "{}{}",
                        value.body,
                        if value.truncated {
                            "\n… preview truncated"
                        } else {
                            ""
                        }
                    )
                }
            } else {
                "Select an entry. Enter opens its preview; v opens supported targets in Review."
                    .into()
            };
            let body = body
                .lines()
                .map(sanitize_terminal_line)
                .collect::<Vec<_>>()
                .join("\n");
            Paragraph::new(body)
                .block(Block::default().title(title).borders(Borders::ALL))
                .style(normal)
                .wrap(Wrap { trim: false })
                .scroll((state.location.preview_scroll, 0))
                .render(preview, buffer);
        }
        let status = state
            .error
            .as_ref()
            .map(|e| format!("{} {}", e.message, e.hint.as_deref().unwrap_or("")))
            .or_else(|| self.notice.clone())
            .unwrap_or_else(|| {
                state
                    .snapshot
                    .as_ref()
                    .map(|s| {
                        format!(
                            "{}{}",
                            s.summary,
                            if s.truncated {
                                " · results truncated"
                            } else {
                                ""
                            }
                        )
                    })
                    .unwrap_or_default()
            });
        Paragraph::new(format!(
            "{}\n{}\n{}",
            sanitize_terminal_line(&status),
            "j/k select · Enter preview · v Review · y copy · Esc back · PgUp/PgDn scroll",
            if state.query_editing {
                "Enter search · Ctrl+U clear · Esc apply"
            } else if page == PanelPage::Changes {
                "/ filter · r refresh · g grouping · d directory stats"
            } else if page == PanelPage::Agents {
                "/ filter · r refresh · recorded sessions are read-only"
            } else {
                "/ search/filter · r refresh"
            }
        ))
        .style(muted)
        .render(rows[2], buffer);
    }
}

/// Match a normalized config binding (single character or a supported named
/// key) against an event. Modified keys stay with the host, matching the
/// repository panel's fixed-key convention.
fn configured_panel_key(key: KeyEvent, binding: &str) -> bool {
    match binding {
        "tab" => key.code == KeyCode::Tab,
        "enter" => key.code == KeyCode::Enter,
        "esc" => key.code == KeyCode::Esc,
        "space" => key.code == KeyCode::Char(' '),
        _ => {
            let mut chars = binding.chars();
            let Some(ch) = chars.next() else {
                return false;
            };
            chars.next().is_none() && key.code == KeyCode::Char(ch)
        }
    }
}
