//! Ref-backed planning browsing owns no native mutation controller or repository.
use super::{
    WorkbenchTab,
    indexed_view::{self, RenderedIndex},
    indexed_workspace::IndexedWorkspace,
    input::{FormAction, FormKind, TextField, WorkbenchForm},
    projection_reader::ProjectionReader,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    widgets::{Paragraph, Widget},
};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use workdeck_pm::{ArchiveFilter, PlanningKind, projection::*, registry::RegisteredCheckout};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Page {
    Issues,
    Features,
    Initiatives,
    Projects,
    Milestones,
    Cycles,
    Targets,
    Labels,
    Activity,
}
impl Page {
    fn planning(self) -> Option<PlanningKind> {
        Some(match self {
            Self::Initiatives => PlanningKind::Initiative,
            Self::Projects => PlanningKind::Project,
            Self::Milestones => PlanningKind::Milestone,
            Self::Cycles => PlanningKind::Cycle,
            Self::Targets => PlanningKind::Target,
            Self::Labels => PlanningKind::Label,
            _ => return None,
        })
    }
    fn query(self) -> ProjectionQuery {
        match self {
            Self::Issues => ProjectionQuery::default(),
            Self::Features => ProjectionQuery::Features {
                query: Default::default(),
            },
            Self::Activity => ProjectionQuery::Activity {
                query: Default::default(),
            },
            _ => ProjectionQuery::Planning {
                query: ProjectionPlanningQuery {
                    kind: self.planning().expect("planning page"),
                    query: String::new(),
                    archive: ArchiveFilter::Active,
                    project: None,
                    target: None,
                },
            },
        }
    }
}
#[derive(Debug)]
struct View {
    index: IndexedWorkspace,
    rendered: Option<RenderedIndex>,
    form: Option<WorkbenchForm>,
}
pub(super) struct ReadonlyPlanning {
    reader: Arc<Mutex<ProjectionReader>>,
    binding: RegisteredCheckout,
    page: Page,
    pages: BTreeMap<Page, View>,
    pub error: Option<String>,
}
impl std::fmt::Debug for ReadonlyPlanning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReadonlyPlanning")
            .field("binding", &self.binding)
            .field("page", &self.page)
            .field("pages", &self.pages)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl ReadonlyPlanning {
    pub fn new(binding: RegisteredCheckout) -> Result<Self, String> {
        let reader = Arc::new(Mutex::new(ProjectionReader::new(
            binding.checkout.clone(),
            binding.source.clone(),
            ProjectionLimits::default(),
        )));
        let mut view = Self {
            reader,
            binding,
            page: Page::Issues,
            pages: BTreeMap::new(),
            error: None,
        };
        view.select(WorkbenchTab::Issues, PlanningKind::Project)?;
        Ok(view)
    }
    pub fn select(&mut self, tab: WorkbenchTab, kind: PlanningKind) -> Result<(), String> {
        let page = match tab {
            WorkbenchTab::Issues => Page::Issues,
            WorkbenchTab::Features => Page::Features,
            WorkbenchTab::Activity => Page::Activity,
            WorkbenchTab::Planning => match kind {
                PlanningKind::Initiative => Page::Initiatives,
                PlanningKind::Project => Page::Projects,
                PlanningKind::Milestone => Page::Milestones,
                PlanningKind::Cycle => Page::Cycles,
                PlanningKind::Target => Page::Targets,
                PlanningKind::Label => Page::Labels,
            },
            _ => return Ok(()),
        };
        if !self.pages.contains_key(&page) {
            let reader = Arc::clone(&self.reader);
            let binding = self.binding.clone();
            let mut index = IndexedWorkspace::with_reader(
                page.query(),
                ProjectionLimits::default().max_page_rows,
                move |request| {
                    binding.resolve().map_err(|error| error.message)?;
                    let reply = reader
                        .lock()
                        .map_err(|_| "Planning projection reader failed".to_string())?
                        .read(request);
                    binding.resolve().map_err(|error| error.message)?;
                    Ok(reply)
                },
            )
            .map_err(|error| error.to_string())?;
            index.open();
            self.pages.insert(
                page,
                View {
                    index,
                    rendered: None,
                    form: None,
                },
            );
        }
        self.page = page;
        self.pages.get_mut(&page).expect("opened page").rendered = None;
        self.error = None;
        Ok(())
    }
    pub fn poll(&mut self) {
        for view in self.pages.values_mut() {
            if view.index.poll() {
                view.rendered = None;
            }
        }
    }
    #[cfg(test)]
    pub fn is_idle(&self) -> bool {
        self.pages.values().all(|view| view.index.is_idle())
    }
    pub fn begin_shutdown(&mut self) {
        for view in self.pages.values_mut() {
            view.index.begin_shutdown();
        }
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if let Some(form) = self
            .pages
            .get_mut(&self.page)
            .and_then(|view| view.form.as_mut())
        {
            form.fields[form.selected].insert(text);
        }
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let Some(view) = self.pages.get_mut(&self.page) else {
            return true;
        };
        if let Some(form) = &mut view.form {
            match form.key(key) {
                FormAction::Submit => {
                    let text = form.fields[0].value.clone();
                    let mut query = view.index.query.clone();
                    match &mut query {
                        ProjectionQuery::Issues { query, .. } => query.query = text,
                        ProjectionQuery::Features { query } => query.query = text,
                        ProjectionQuery::Planning { query } => query.query = text,
                        _ => {}
                    }
                    view.index.set_query(query);
                    view.form = None;
                    view.rendered = None;
                }
                FormAction::Close => view.form = None,
                FormAction::Edited => {}
            }
            return true;
        }
        if key.modifiers.is_empty() && key.code == KeyCode::Char('/') {
            let text = match &view.index.query {
                ProjectionQuery::Issues { query, .. } => Some(query.query.clone()),
                ProjectionQuery::Features { query } => Some(query.query.clone()),
                ProjectionQuery::Planning { query } => Some(query.query.clone()),
                _ => None,
            };
            if let Some(text) = text {
                view.form = Some(WorkbenchForm::new(
                    FormKind::Filter,
                    "Filter captured planning",
                    vec![TextField::new("Text", text, false)],
                ));
            }
            return true;
        }
        if key.modifiers.is_empty() && key.code == KeyCode::Char('x') {
            let mut query = view.index.query.clone();
            let archive = match &mut query {
                ProjectionQuery::Issues { query, .. } => Some(&mut query.archive),
                ProjectionQuery::Features { query } => Some(&mut query.archive),
                ProjectionQuery::Planning { query } => Some(&mut query.archive),
                _ => None,
            };
            if let Some(archive) = archive {
                *archive = if *archive == ArchiveFilter::Active {
                    ArchiveFilter::All
                } else {
                    ArchiveFilter::Active
                };
                view.index.set_query(query);
            }
            view.rendered = None;
            return true;
        }
        if key.modifiers.is_empty()
            && matches!(
                key.code,
                KeyCode::Char('t') | KeyCode::Left | KeyCode::Right
            )
            && let ProjectionQuery::Features { mut query } = view.index.query.clone()
        {
            if key.code == KeyCode::Char('t') {
                query.tree = !query.tree;
            } else if query.tree
                && let Some(row) = view.index.selected_row().cloned()
            {
                let Ok(id) = row.token.key.id.parse::<workdeck_pm::FeatureId>() else {
                    return true;
                };
                let children = row.tree.as_ref().map_or(0, |tree| tree.children);
                if key.code == KeyCode::Left {
                    if children > 0 && !query.collapsed.contains(&id) {
                        query.collapsed.push(id);
                    } else {
                        if let Some(parent) = row.parent {
                            view.index.locate(parent);
                        }
                        return true;
                    }
                } else if query.collapsed.contains(&id) {
                    query.collapsed.retain(|collapsed| collapsed != &id);
                } else {
                    if children > 0 {
                        view.index.move_by(1);
                    }
                    return true;
                }
            }
            view.index.set_query(ProjectionQuery::Features { query });
            view.rendered = None;
            return true;
        }
        if view.index.key(key) {
            self.error = None;
            view.rendered = None;
            return true;
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
            return false;
        }
        self.error = Some("Read-only planning source: return to a working-tree checkout to author, claim or run checks".into());
        true
    }
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &crate::AppTheme) {
        let regions = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(2),
        ])
        .split(area);
        let title = self
            .page
            .planning()
            .map(super::planning_workspace::title)
            .unwrap_or(match self.page {
                Page::Issues => "Issues",
                Page::Features => "Features",
                Page::Activity => "Activity",
                _ => "Planning",
            });
        Paragraph::new(format!("Read-only planning · {title}")).render(regions[0], buffer);
        if let Some(view) = self.pages.get_mut(&self.page) {
            if let Some(form) = &view.form {
                super::shell_view::render_form(form, regions[1], buffer, theme);
                view.rendered = None;
            } else {
                view.rendered = Some(indexed_view::render(
                    &mut view.index,
                    regions[1],
                    buffer,
                    theme,
                ));
            }
        }
        let controls = match self.page {
            Page::Issues => "Enter excerpt · r refresh · / filter · x archive · w board · z group",
            Page::Features => {
                "Enter excerpt · r refresh · / filter · x archive · t tree · ←→ branches"
            }
            Page::Activity => "Enter excerpt · r refresh · Shift-PgUp/PgDn excerpt",
            _ => "Enter excerpt · r refresh · / filter · x archive · 1–6 planning kinds",
        };
        let hints = format!(
            "{controls}\nF3 Issues · Shift-F11 Features · F11 Projects · F12 Cycles · Shift-F9 sources"
        );
        let hint = self.error.as_deref().unwrap_or(&hints);
        Paragraph::new(
            hint.lines()
                .map(workdeck_diff::sanitize_terminal_line)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .render(regions[2], buffer);
    }
    pub fn mouse(&mut self, event: &MouseEvent) -> bool {
        let Some(view) = self.pages.get_mut(&self.page) else {
            return true;
        };
        let Some(rendered) = &view.rendered else {
            return true;
        };
        let point = (event.column, event.row);
        let detail = rendered.detail.contains(point.into());
        match event.kind {
            MouseEventKind::ScrollDown if detail => {
                view.index.detail_scroll = view
                    .index
                    .detail_scroll
                    .saturating_add(3)
                    .min(view.index.opened_lines.len().saturating_sub(1))
            }
            MouseEventKind::ScrollUp if detail => {
                view.index.detail_scroll = view.index.detail_scroll.saturating_sub(3)
            }
            MouseEventKind::ScrollDown => view.index.move_by(1),
            MouseEventKind::ScrollUp => view.index.move_by(-1),
            MouseEventKind::Up(MouseButton::Left) => {
                let token = rendered
                    .rows
                    .iter()
                    .find(|(area, _)| area.contains(point.into()))
                    .map(|(_, token)| token.clone());
                if let Some(token) = token
                    && view.index.select_token(&token)
                {
                    view.index.open_selected();
                }
            }
            _ => {}
        }
        view.rendered = None;
        true
    }
}

impl super::WorkbenchShell {
    pub(super) fn readonly_key(&mut self, key: KeyEvent) -> bool {
        let has_form = self.readonly.as_ref().is_some_and(|source| {
            source
                .pages
                .get(&source.page)
                .is_some_and(|view| view.form.is_some())
        });
        let selected = match (key.code, key.modifiers) {
            (KeyCode::F(2), KeyModifiers::NONE) => Some((WorkbenchTab::Review, self.planning.kind)),
            (KeyCode::F(3), KeyModifiers::NONE) => Some((WorkbenchTab::Issues, self.planning.kind)),
            (KeyCode::F(11), KeyModifiers::SHIFT) => {
                Some((WorkbenchTab::Features, self.planning.kind))
            }
            (KeyCode::F(12), KeyModifiers::SHIFT) => {
                Some((WorkbenchTab::Activity, self.planning.kind))
            }
            (KeyCode::F(11), KeyModifiers::NONE) => {
                Some((WorkbenchTab::Planning, PlanningKind::Project))
            }
            (KeyCode::F(12), KeyModifiers::NONE) => {
                Some((WorkbenchTab::Planning, PlanningKind::Cycle))
            }
            (KeyCode::Char('v'), KeyModifiers::NONE)
                if self.tab == WorkbenchTab::Issues && !has_form =>
            {
                Some((WorkbenchTab::Features, self.planning.kind))
            }
            (KeyCode::Char(number @ '1'..='6'), KeyModifiers::NONE)
                if self.tab == WorkbenchTab::Planning && !has_form =>
            {
                let kinds = [
                    PlanningKind::Initiative,
                    PlanningKind::Project,
                    PlanningKind::Milestone,
                    PlanningKind::Cycle,
                    PlanningKind::Target,
                    PlanningKind::Label,
                ];
                Some((
                    WorkbenchTab::Planning,
                    kinds[number as usize - '1' as usize],
                ))
            }
            _ => None,
        };
        if let Some((tab, kind)) = selected {
            self.tab = tab;
            self.planning.kind = kind;
            if let Some(source) = &mut self.readonly
                && let Err(error) = source.select(tab, kind)
            {
                source.error = Some(error);
            }
            return true;
        }
        if key.code == KeyCode::F(4) || matches!(key.code, KeyCode::F(5..=10)) {
            self.notice = Some("Read-only planning source: native authoring and repository panels are unavailable in this view".into());
            if let Some(source) = &mut self.readonly {
                source.error = self.notice.clone();
            }
            return true;
        }
        if self.tab == WorkbenchTab::Review {
            return false;
        }
        self.readonly.as_mut().is_some_and(|source| source.key(key))
    }
}
