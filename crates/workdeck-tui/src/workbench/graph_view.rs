//! A captured full graph remains visible even when the issue list is filtered.
use crate::{AppTheme, ratatui_theme_color};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use workdeck_diff::sanitize_terminal_line;
use workdeck_pm::{IssueGraphSnapshot, IssueId, PmError, Repository};

#[derive(Debug)]
pub(super) struct GraphView {
    repository: Repository,
    snapshot: IssueGraphSnapshot,
    anchor: IssueId,
    history: Vec<IssueId>,
    selected: usize,
    scroll: u16,
    offset: usize,
    error: Option<PmError>,
}
impl GraphView {
    pub fn open(repository: Repository, anchor: IssueId) -> Result<Self, PmError> {
        let snapshot = repository.issue_graph_snapshot()?;
        snapshot.relations(&anchor)?;
        Ok(Self {
            repository,
            snapshot,
            anchor,
            history: Vec::new(),
            selected: 0,
            scroll: 0,
            offset: 0,
            error: None,
        })
    }
    fn links(&self) -> Vec<(&'static str, IssueId)> {
        let Ok(relations) = self.snapshot.relations(&self.anchor) else {
            return Vec::new();
        };
        let mut links = Vec::new();
        links.extend(relations.parent.into_iter().map(|id| ("parent", id)));
        for (kind, ids) in [
            ("child", relations.children),
            ("prerequisite", relations.prerequisites),
            ("dependent", relations.dependents),
            ("related", relations.related),
        ] {
            links.extend(ids.into_iter().map(|id| (kind, id)));
        }
        links
    }
    pub fn key(&mut self, key: KeyEvent) {
        if !key.modifiers.is_empty() {
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.selected = self.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = self
                    .selected
                    .saturating_add(1)
                    .min(self.links().len().saturating_sub(1))
            }
            KeyCode::PageUp => self.scroll = self.scroll.saturating_sub(10),
            KeyCode::PageDown => self.scroll = self.scroll.saturating_add(10),
            KeyCode::Home => {
                self.scroll = 0;
                self.selected = 0;
            }
            KeyCode::Enter => {
                if let Some((_, id)) = self.links().get(self.selected) {
                    match self.snapshot.relations(id) {
                        Ok(_) => {
                            self.history.push(self.anchor.clone());
                            self.anchor = id.clone();
                            self.selected = 0;
                            self.scroll = 0;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(anchor) = self.history.pop() {
                    match self.snapshot.relations(&anchor) {
                        Ok(_) => {
                            self.anchor = anchor;
                            self.selected = 0;
                            self.scroll = 0;
                            self.offset = 0;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            KeyCode::Char('r') => {
                match self.repository.issue_graph_snapshot().and_then(|snapshot| {
                    snapshot.relations(&self.anchor)?;
                    Ok(snapshot)
                }) {
                    Ok(snapshot) => {
                        self.snapshot = snapshot;
                        self.selected = self.selected.min(self.links().len().saturating_sub(1));
                        self.error = None;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            _ => {}
        }
    }
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let rows = Layout::vertical([
            Constraint::Length(2),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
        Paragraph::new(format!(
            "Issue graph · {}\nFull graph, including outside-filter requirements",
            self.anchor
        ))
        .style(style)
        .render(rows[0], buffer);
        let panes = if area.width >= 120 {
            Layout::horizontal([Constraint::Percentage(45), Constraint::Percentage(55)])
                .split(rows[1])
        } else {
            Layout::vertical([Constraint::Percentage(45), Constraint::Min(0)]).split(rows[1])
        };
        let links = self.links();
        let index = self
            .snapshot
            .issues()
            .iter()
            .map(|issue| (&issue.metadata.id, issue))
            .collect::<std::collections::BTreeMap<_, _>>();
        let items = links
            .iter()
            .map(|(kind, id)| {
                let summary = index
                    .get(id)
                    .map(|issue| {
                        format!(
                            "{} · {}{}",
                            issue.metadata.status,
                            issue.metadata.title,
                            if issue.retirement.is_some() {
                                " [retired]"
                            } else {
                                ""
                            }
                        )
                    })
                    .unwrap_or_else(|| "[unresolved reference]".into());
                ListItem::new(format!(
                    "{}\n  {id}",
                    sanitize_terminal_line(&format!("{kind} · {summary}"))
                ))
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default()
            .with_selected((!items.is_empty()).then_some(self.selected))
            .with_offset(self.offset);
        StatefulWidget::render(
            List::new(items)
                .style(style)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(if links.is_empty() {
                            " No declared issue relationships "
                        } else {
                            " Relationships "
                        }),
                )
                .highlight_symbol("› "),
            panes[0],
            buffer,
            &mut state,
        );
        self.offset = state.offset();
        let mut lines = Vec::new();
        if let Some(error) = &self.error {
            lines.push(format!("Inspected graph retained: {}", error.message));
        }
        match self.snapshot.readiness(&self.anchor) {
            Ok(readiness) => {
                lines.push(format!("Ready for work: {}", readiness.ready));
                for condition in readiness.conditions {
                    lines.push(format!(
                        "{:?} · {} · {}",
                        condition.state, condition.reason_code, condition.message
                    ));
                }
            }
            Err(error) => lines.push(format!("Readiness unresolved: {}", error.message)),
        }
        if let Some((_, id)) = links.get(self.selected) {
            match self.snapshot.dependency_path(&self.anchor, id) {
                Ok(path) => {
                    if path.found {
                        lines.push(format!(
                            "Hard prerequisite path: {}",
                            path.path
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(" → ")
                        ));
                    } else if path.conditions.is_empty() {
                        lines.push(
                            "Selected relationship has no directed hard-prerequisite path".into(),
                        );
                    } else {
                        lines
                            .push("No path found; reachable requirements remain unresolved".into());
                    }
                    for condition in path.conditions {
                        lines.push(format!(
                            "{:?} · {} · {}",
                            condition.state, condition.reason_code, condition.message
                        ));
                    }
                }
                Err(error) => lines.push(format!("Path unresolved: {}", error.message)),
            }
        }
        lines.push("Paths express dependencies, not delivery dates".into());
        match self.snapshot.relations(&self.anchor) {
            Ok(relations) => {
                lines.push(String::new());
                lines.push("Dependency and child completion conditions".into());
                for condition in relations.conditions {
                    lines.push(format!(
                        "{:?} · {} · {}",
                        condition.state, condition.reason_code, condition.message
                    ));
                }
            }
            Err(error) => lines.push(format!("Relationships unresolved: {}", error.message)),
        }
        lines.push(format!(
            "\nRepository {}\nInspected {}",
            self.snapshot.repository(),
            self.snapshot.fingerprint()
        ));
        let text = lines
            .iter()
            .map(|line| sanitize_terminal_line(line))
            .collect::<Vec<_>>()
            .join("\n");
        Paragraph::new(text)
            .style(style)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Readiness and paths "),
            )
            .wrap(Wrap { trim: false })
            .scroll((self.scroll, 0))
            .render(panes[1], buffer);
        Paragraph::new("↑/↓ select · Enter follow · Backspace back · Esc issues\nPgUp/PgDn details · r refresh").style(style).render(rows[2], buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use workdeck_pm::{CreateIssue, IssueGraphMutation, IssueRecord, RequestId};
    fn create(repo: &Repository, title: &str) -> IssueRecord {
        serde_json::from_value(
            repo.create_issue(&CreateIssue::new(title, ""), &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap()
    }
    #[test]
    fn back_navigation_rejects_an_anchor_removed_from_the_refreshed_graph() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let a = create(&repo, "A");
        let b = create(&repo, "B");
        repo.mutate_issue_graph(
            a.metadata.id.as_str(),
            None,
            None,
            &IssueGraphMutation::AddPrerequisite {
                prerequisite: b.metadata.id.to_string(),
            },
            &RequestId::new(),
        )
        .unwrap();
        let mut view = GraphView::open(repo.clone(), a.metadata.id.clone()).unwrap();
        view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        std::fs::remove_file(repo.root().join(a.path)).unwrap();
        view.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(view.error.is_none());
        view.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(view.anchor, b.metadata.id);
        assert_eq!(
            view.error.as_ref().unwrap().code,
            workdeck_pm::ErrorCode::NotFound
        );
    }
    #[test]
    fn arrow_navigation_reveals_every_selected_relationship_in_a_short_view() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let anchor = create(&repo, "Anchor");
        for i in 0..20 {
            let target = create(&repo, &format!("Prerequisite {i:02}"));
            repo.mutate_issue_graph(
                anchor.metadata.id.as_str(),
                None,
                None,
                &IssueGraphMutation::AddPrerequisite {
                    prerequisite: target.metadata.id.to_string(),
                },
                &RequestId::new(),
            )
            .unwrap();
        }
        let mut view = GraphView::open(repo, anchor.metadata.id).unwrap();
        let theme = crate::resolve_theme(None, None, &[]);
        let area = Rect::new(0, 0, 70, 14);
        for index in 0..20 {
            let mut buffer = Buffer::empty(area);
            view.render(area, &mut buffer, &theme);
            let text = buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            let selected = view.links()[index].1.to_string();
            assert!(
                text.contains(&selected),
                "selected {selected} is not visible: {text}"
            );
            view.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
    }
    #[test]
    fn mounted_graph_keeps_issue_filter_and_renders_outside_requirements_in_narrow_and_wide_views()
    {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let a = create(&repo, "Visible issue");
        let b = create(&repo, "Hidden prerequisite");
        repo.mutate_issue_graph(
            a.metadata.id.as_str(),
            None,
            None,
            &IssueGraphMutation::AddPrerequisite {
                prerequisite: b.metadata.id.to_string(),
            },
            &RequestId::new(),
        )
        .unwrap();
        let mut shell = crate::workbench::WorkbenchShell::open(
            crate::workbench::WorkbenchOptions::new(root.path()),
            true,
        );
        shell
            .controller
            .set_filter(crate::workbench::IssueFilter {
                query: "Visible".into(),
                ..Default::default()
            })
            .unwrap();
        crate::workbench::test_pump::settle_shell(&mut shell);
        assert_eq!(shell.controller.visible_issues().len(), 1);
        assert!(shell.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE)));
        assert!(shell.graph.is_some());
        let theme = crate::resolve_theme(None, None, &[]);
        for width in [70, 180] {
            let area = Rect::new(0, 0, width, 30);
            let mut buffer = Buffer::empty(area);
            shell.render_planning(area, &mut buffer, &theme);
            let text = buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(text.contains("Issue graph"));
            assert!(text.contains("Hidden prerequisite"), "{text}");
            assert!(text.contains("Ready for work: false"), "{text}");
        }
        shell.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(shell.graph.as_ref().unwrap().anchor, b.metadata.id);
        shell.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(shell.graph.is_none());
        assert_eq!(shell.controller.filter().query, "Visible");
        assert_eq!(shell.controller.selected_id(), Some(&a.metadata.id));
    }
    #[test]
    fn captured_graph_navigation_reaches_hidden_prerequisites_and_refresh_retains_identity() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let a = create(&repo, "Visible");
        let b = create(&repo, "Outside filter");
        repo.mutate_issue_graph(
            a.metadata.id.as_str(),
            None,
            None,
            &IssueGraphMutation::AddPrerequisite {
                prerequisite: b.metadata.id.to_string(),
            },
            &RequestId::new(),
        )
        .unwrap();
        let mut view = GraphView::open(repo.clone(), a.metadata.id.clone()).unwrap();
        assert_eq!(view.links(), vec![("prerequisite", b.metadata.id.clone())]);
        view.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(view.anchor, b.metadata.id);
        view.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(view.anchor, a.metadata.id);
        let fingerprint = view.snapshot.fingerprint().clone();
        let path = repo.root().join(&b.path);
        let bytes = std::fs::read(&path).unwrap();
        std::fs::write(&path, "invalid direct edit").unwrap();
        view.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(view.error.is_some());
        assert_eq!(view.snapshot.fingerprint(), &fingerprint);
        std::fs::write(&path, bytes).unwrap();
        view.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert!(view.error.is_none());
        assert_eq!(view.snapshot.repository(), repo.identity());
    }
}
