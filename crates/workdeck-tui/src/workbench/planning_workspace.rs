//! Native planning views. Records, membership and mutations come from PM APIs;
//! drafts retain their original source token across refresh and tab switches.
use super::{
    PanelTarget,
    input::{FormAction, FormKind, TextField, WorkbenchForm},
};
use crate::{AppTheme, ratatui_theme_color};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, StatefulWidget, Widget, Wrap},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use workdeck_diff::sanitize_terminal_line;
use workdeck_pm::{
    CreatePlanning, ErrorCode, PlanningKind, PlanningMembership, PlanningMembershipQuery,
    PlanningMutation, PlanningRecord, PmError, PolicyAcceptance, PolicyAssessment, Repository,
    RequestId, SourceToken, transactions::MutationReceipt,
};

#[path = "planning_carryover.rs"]
mod carryover;
#[path = "planning_index.rs"]
mod indexed;

type Result<T> = std::result::Result<T, PmError>;
const KINDS: [PlanningKind; 6] = [
    PlanningKind::Initiative,
    PlanningKind::Project,
    PlanningKind::Milestone,
    PlanningKind::Cycle,
    PlanningKind::Target,
    PlanningKind::Label,
];
type DraftKey = (String, String);

pub(super) fn title(kind: PlanningKind) -> &'static str {
    match kind {
        PlanningKind::Initiative => "Initiatives",
        PlanningKind::Project => "Projects",
        PlanningKind::Milestone => "Milestones",
        PlanningKind::Cycle => "Cycles",
        PlanningKind::Target => "Targets",
        PlanningKind::Label => "Labels",
    }
}

#[derive(Debug)]
struct PlanningDraft {
    kind: PlanningKind,
    id: Option<String>,
    expected: Option<SourceToken>,
    form: WorkbenchForm,
    keys: Vec<&'static str>,
    attempt: Option<(Value, RequestId)>,
}

#[derive(Debug)]
struct ArchiveAttempt {
    id: String,
    source: SourceToken,
    archived: bool,
    request: RequestId,
}

#[derive(Debug)]
struct PolicyDraft {
    kind: PlanningKind,
    id: String,
    expected: SourceToken,
    form: WorkbenchForm,
    attempt: Option<(Value, RequestId)>,
}

#[derive(Debug)]
pub(super) struct PlanningWorkspace {
    repository: Option<Repository>,
    indexed: bool,
    index: Option<super::indexed_workspace::IndexedWorkspace>,
    attempted: Option<workdeck_pm::projection::ProjectionRowToken>,
    pending_selected: Option<String>,
    membership_worker: Option<indexed::MembershipWorker>,
    membership_requested: Option<workdeck_pm::projection::ProjectionRowToken>,
    pub kind: PlanningKind,
    records: Vec<PlanningRecord>,
    selection: BTreeMap<String, String>,
    offset: usize,
    detail_scroll: u16,
    membership: Option<PlanningMembership>,
    drafts: BTreeMap<DraftKey, PlanningDraft>,
    policy: Option<PolicyAssessment>,
    policy_draft: Option<PolicyDraft>,
    active: BTreeMap<String, String>,
    pending_archive: Option<ArchiveAttempt>,
    carryover: Option<carryover::CarryoverDraft>,
    carryover_visible: bool,
    include_archived: bool,
    pub error: Option<PmError>,
    pub last_receipt: Option<MutationReceipt>,
    pub bounds: Option<Rect>,
}

impl PlanningWorkspace {
    pub fn new(repository: Option<Repository>) -> Self {
        Self {
            repository,
            indexed: false,
            index: None,
            attempted: None,
            pending_selected: None,
            membership_worker: None,
            membership_requested: None,
            kind: PlanningKind::Project,
            records: Vec::new(),
            selection: BTreeMap::new(),
            offset: 0,
            detail_scroll: 0,
            membership: None,
            drafts: BTreeMap::new(),
            policy: None,
            policy_draft: None,
            active: BTreeMap::new(),
            pending_archive: None,
            carryover: None,
            carryover_visible: false,
            include_archived: false,
            error: None,
            last_receipt: None,
            bounds: None,
        }
    }
    pub fn new_indexed(repository: Option<Repository>) -> Self {
        let mut workspace = Self::new(repository);
        workspace.indexed = true;
        workspace
    }
    pub fn needs_source(&self) -> bool {
        self.repository.is_none()
    }
    pub fn issue_scope(&self) -> workdeck_pm::ArchiveFilter {
        if self.include_archived {
            workdeck_pm::ArchiveFilter::All
        } else {
            workdeck_pm::ArchiveFilter::Active
        }
    }
    pub fn bind_if_missing(&mut self, repository: Option<Repository>) {
        if self.repository.is_none() {
            self.repository = repository;
        }
    }
    fn repository(&self) -> Result<&Repository> {
        self.repository.as_ref().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotInitialized,
                "Run workdeck init, then refresh planning",
            )
        })
    }
    fn key(&self) -> String {
        title(self.kind).into()
    }
    fn selected_index(&self) -> Option<usize> {
        self.selection.get(&self.key()).and_then(|id| {
            self.records
                .iter()
                .position(|record| &record.metadata.id == id)
        })
    }
    pub fn selected(&self) -> Option<&PlanningRecord> {
        self.selected_index()
            .map(|index| &self.records[index])
            .filter(|record| record.kind == self.kind)
    }
    pub fn open(&mut self, kind: PlanningKind) {
        if self.kind != kind {
            self.records.clear();
            self.membership = None;
            self.policy = None;
            self.policy_draft = None;
            self.pending_archive = None;
        }
        self.kind = kind;
        if self.indexed {
            self.pending_selected = self.selection.get(&self.key()).cloned();
        }
        self.offset = 0;
        self.detail_scroll = 0;
        if let Err(error) = self.refresh() {
            self.error = Some(error);
        }
    }
    pub fn auto_refresh(&mut self) {
        if self.showing_carryover() {
            return;
        }
        if self.indexed && !self.reader_idle() {
            return;
        }
        if self.active_draft().is_none()
            && self.error.is_none()
            && let Err(error) = self.refresh()
        {
            self.error = Some(error);
        }
    }
    pub fn refresh(&mut self) -> Result<()> {
        if self.indexed {
            return self.refresh_index();
        }
        let mut records = self.repository()?.list_planning(self.kind)?;
        records.retain(|record| {
            self.include_archived || (!record.metadata.archived && record.retirement.is_none())
        });
        let selected = self
            .selection
            .get(&self.key())
            .and_then(|id| records.iter().find(|record| &record.metadata.id == id))
            .or_else(|| records.first());
        let membership = selected
            .map(|record| {
                self.repository()?
                    .planning_membership(&self.membership_query(&record.metadata.id))
            })
            .transpose()?;
        let policy = selected
            .filter(|_| matches!(self.kind, PlanningKind::Project | PlanningKind::Milestone))
            .map(|record| {
                self.repository()?
                    .assess_planning_policy(self.kind, &record.metadata.id)
            })
            .transpose()?;
        if let Some(view) = &membership {
            // Membership is a newer coherent capture than the initial list.
            if let Some(row) = records
                .iter_mut()
                .find(|row| row.metadata.id == view.record.metadata.id)
            {
                *row = view.record.clone();
            }
            self.selection
                .insert(self.key(), view.record.metadata.id.clone());
        } else {
            self.selection.remove(&self.key());
        }
        self.records = records;
        self.membership = membership;
        self.policy = policy;
        self.error = None;
        Ok(())
    }
    fn membership_query(&self, id: &str) -> PlanningMembershipQuery {
        let mut query = PlanningMembershipQuery::new(self.kind, id);
        query.issues.archive = self.issue_scope();
        query
    }
    fn active_key(&self) -> Option<DraftKey> {
        self.active
            .get(&self.key())
            .map(|id| (self.key(), id.clone()))
    }
    fn active_draft(&self) -> Option<&PlanningDraft> {
        self.active_key().and_then(|key| self.drafts.get(&key))
    }
    fn start_draft(&mut self, edit: bool) -> Result<()> {
        let record = if edit {
            Some(self.selected().cloned().ok_or_else(|| {
                PmError::new(ErrorCode::NotFound, "Select a planning record to edit")
            })?)
        } else {
            None
        };
        let id = record.as_ref().map(|record| record.metadata.id.clone());
        let key = (self.key(), id.clone().unwrap_or_default());
        if !self.drafts.contains_key(&key) {
            self.drafts
                .insert(key.clone(), PlanningDraft::new(self.kind, record.as_ref())?);
        }
        self.active.insert(key.0, key.1);
        Ok(())
    }
    fn start_policy(&mut self) -> Result<()> {
        if !matches!(self.kind, PlanningKind::Project | PlanningKind::Milestone) {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "Completion policy is available for projects and milestones",
            ));
        }
        let record = self.selected().cloned().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "Select a project or milestone to assess its completion policy",
            )
        })?;
        self.policy = Some(
            self.repository()?
                .assess_planning_policy(self.kind, &record.metadata.id)?,
        );
        self.policy_draft = Some(PolicyDraft {
            kind: self.kind,
            id: record.metadata.id,
            expected: record.source,
            form: WorkbenchForm::new(
                FormKind::Planning,
                format!("Complete {}", title(self.kind)),
                vec![
                    TextField::new("Acceptance actor", String::new(), false),
                    TextField::new("Acceptance reason", String::new(), true),
                ],
            ),
            attempt: None,
        });
        Ok(())
    }
    fn submit_policy(&mut self) -> Result<()> {
        let repository = self.repository()?.clone();
        let draft = self.policy_draft.as_mut().ok_or_else(|| {
            PmError::new(ErrorCode::InvalidInput, "No active completion policy form")
        })?;
        let actor = draft.form.fields[0].value.trim().to_owned();
        let reason = draft.form.fields[1].value.trim().to_owned();
        let acceptance = PolicyAcceptance { actor, reason };
        let payload = json!({
            "kind": draft.kind,
            "id": draft.id,
            "expected": draft.expected,
            "acceptance": acceptance,
        });
        let request = match &draft.attempt {
            Some((original, _)) if original != &payload => {
                return Err(PmError::new(
                    ErrorCode::IdempotencyConflict,
                    "Completion policy input changed after the first attempt; discard the form to start a new request",
                ));
            }
            Some((_, request)) => request.clone(),
            None => {
                let request = RequestId::new();
                draft.attempt = Some((payload, request.clone()));
                request
            }
        };
        let receipt = repository.complete_planning(
            draft.kind,
            &draft.id,
            &draft.expected,
            &acceptance,
            &request,
        )?;
        let record: PlanningRecord = serde_json::from_value(receipt.result.clone())
            .map_err(|error| PmError::new(ErrorCode::CorruptStore, error.to_string()))?;
        self.selection
            .insert(title(draft.kind).into(), record.metadata.id.clone());
        self.last_receipt = Some(receipt);
        self.policy_draft = None;
        self.policy = None;
        if self.indexed {
            self.pending_selected = Some(record.metadata.id);
        }
        if let Err(mut error) = self.refresh() {
            error.message = format!("Completion saved; refresh failed: {}", error.message);
            self.error = Some(error);
        }
        Ok(())
    }
    fn submit(&mut self) -> Result<()> {
        let key = self
            .active_key()
            .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "No active planning draft"))?;
        let repository = self.repository()?.clone();
        let draft = self.drafts.get_mut(&key).expect("active draft exists");
        let (name, body, fields) = draft.values()?;
        let payload = json!({"kind":draft.kind,"id":draft.id,"expected":draft.expected,"name":name,"body":body,"fields":fields});
        let request = match &draft.attempt {
            // A lost acknowledgement may follow publication. Retain the request
            // even when the draft changes so replay rejects changed input instead
            // of publishing a second record. Explicit discard starts a new intent.
            Some((_, request)) => request.clone(),
            _ => {
                let request = RequestId::new();
                draft.attempt = Some((payload, request.clone()));
                request
            }
        };
        let receipt = if let Some(id) = &draft.id {
            let mut fields = fields;
            fields.insert("name".into(), json!(name));
            repository.mutate_planning(
                draft.kind,
                id,
                draft.expected.as_ref(),
                &PlanningMutation::Update {
                    fields,
                    body: Some(body),
                },
                &request,
            )?
        } else {
            repository.create_planning(
                draft.kind,
                &CreatePlanning {
                    id: None,
                    name,
                    body,
                    fields,
                },
                &request,
            )?
        };
        let record: PlanningRecord = serde_json::from_value(receipt.result.clone())
            .map_err(|error| PmError::new(ErrorCode::CorruptStore, error.to_string()))?;
        self.selection.insert(self.key(), record.metadata.id);
        self.last_receipt = Some(receipt);
        self.drafts.remove(&key);
        self.active.remove(&self.key());
        if self.indexed {
            self.pending_selected = self.selection.get(&self.key()).cloned();
        }
        if let Err(mut error) = self.refresh() {
            error.message = format!("Saved planning record; refresh failed: {}", error.message);
            self.error = Some(error);
        }
        Ok(())
    }
    fn archive(&mut self) -> Result<()> {
        let record = self.selected().cloned().ok_or_else(|| {
            PmError::new(ErrorCode::NotFound, "Select a planning record to archive")
        })?;
        if self.pending_archive.as_ref().is_none_or(|pending| {
            pending.id != record.metadata.id || pending.source != record.source
        }) {
            self.pending_archive = Some(ArchiveAttempt {
                id: record.metadata.id.clone(),
                source: record.source.clone(),
                archived: !record.metadata.archived,
                request: RequestId::new(),
            });
        }
        let pending = self.pending_archive.as_ref().expect("prepared");
        let receipt = self.repository()?.mutate_planning(
            self.kind,
            &pending.id,
            Some(&pending.source),
            &PlanningMutation::Archive {
                archived: pending.archived,
            },
            &pending.request,
        )?;
        self.last_receipt = Some(receipt);
        self.pending_archive = None;
        if self.indexed {
            self.pending_selected = if self.include_archived || record.metadata.archived {
                Some(record.metadata.id.clone())
            } else {
                None
            };
        }
        if let Err(mut error) = self.refresh() {
            error.message = format!("Archive saved; refresh failed: {}", error.message);
            self.error = Some(error);
        }
        Ok(())
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if self.showing_carryover() {
            self.carryover.as_mut().unwrap().paste(text);
            return true;
        }
        if let Some(policy) = &mut self.policy_draft {
            if let Some(field) = policy.form.fields.get_mut(policy.form.selected) {
                field.insert(text);
            }
            return true;
        }
        let Some(key) = self.active_key() else {
            return false;
        };
        let draft = self.drafts.get_mut(&key).expect("active draft");
        if let Some(field) = draft.form.fields.get_mut(draft.form.selected) {
            field.insert(text);
        }
        true
    }
    pub fn handle_key(&mut self, key: KeyEvent) -> (bool, Option<PanelTarget>) {
        self.poll_index();
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return (false, None);
        }
        if self.showing_carryover() {
            self.carryover_key(key);
            return (true, None);
        }
        if key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('d')
            && let Some(draft_key) = self.active_key()
        {
            self.drafts.remove(&draft_key);
            self.active.remove(&self.key());
            self.error = None;
            if let Err(error) = self.refresh() {
                self.error = Some(error);
            }
            return (true, None);
        }
        if key.modifiers == KeyModifiers::CONTROL
            && key.code == KeyCode::Char('d')
            && self.policy_draft.take().is_some()
        {
            self.policy = None;
            self.error = None;
            if let Err(error) = self.refresh() {
                self.error = Some(error);
            }
            return (true, None);
        }
        if self.policy_draft.is_some() {
            let action = self
                .policy_draft
                .as_mut()
                .expect("active completion policy form")
                .form
                .key(key);
            match action {
                FormAction::Submit => {
                    if let Err(error) = self.submit_policy() {
                        self.error = Some(error);
                    }
                }
                FormAction::Close => {
                    self.policy_draft = None;
                    self.policy = None;
                }
                FormAction::Edited => {}
            }
            return (true, None);
        }
        if let Some(draft_key) = self.active_key() {
            let action = self
                .drafts
                .get_mut(&draft_key)
                .expect("active draft")
                .form
                .key(key);
            match action {
                FormAction::Submit => {
                    if let Err(error) = self.submit() {
                        self.error = Some(error);
                    }
                }
                FormAction::Close => {
                    self.active.remove(&self.key());
                }
                FormAction::Edited => {}
            }
            return (true, None);
        }
        if !key.modifiers.is_empty() {
            return (false, None);
        }
        if self.indexed
            && matches!(
                key.code,
                KeyCode::Up
                    | KeyCode::Down
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::Char('j' | 'k')
            )
        {
            if let Some(index) = &mut self.index {
                index.key(key);
            }
            self.detail_scroll = 0;
            self.poll_index();
            return (true, None);
        }
        if self.indexed
            && matches!(key.code, KeyCode::Enter | KeyCode::Char('e' | 'a'))
            && let Err(error) = self.select_index_record()
        {
            self.error = Some(error);
            return (true, None);
        }
        let result = match key.code {
            KeyCode::Char('q') => return (false, None),
            KeyCode::Char('u') if self.kind == PlanningKind::Cycle => self.start_carryover(),
            KeyCode::Char('n') => self.start_draft(false),
            KeyCode::Char('e') => self.start_draft(true),
            KeyCode::Char('p')
                if matches!(self.kind, PlanningKind::Project | PlanningKind::Milestone) =>
            {
                self.start_policy()
            }
            KeyCode::Char('a') => self.archive(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('x') => {
                self.include_archived = !self.include_archived;
                let result = self.refresh();
                if result.is_err() {
                    self.include_archived = !self.include_archived;
                }
                result
            }
            KeyCode::Char('[' | ']') => {
                let position = KINDS.iter().position(|kind| kind == &self.kind).unwrap();
                let next = if key.code == KeyCode::Char(']') {
                    (position + 1) % KINDS.len()
                } else {
                    (position + KINDS.len() - 1) % KINDS.len()
                };
                self.open(KINDS[next]);
                Ok(())
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
                if !self.records.is_empty() {
                    let current = self.selected_index().unwrap_or_default();
                    let next = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                        current.saturating_sub(1)
                    } else {
                        (current + 1).min(self.records.len() - 1)
                    };
                    let record = &self.records[next];
                    let view = self.repository().and_then(|repository| {
                        repository.planning_membership(&self.membership_query(&record.metadata.id))
                    });
                    match view {
                        Ok(view) => {
                            self.selection
                                .insert(self.key(), view.record.metadata.id.clone());
                            self.records[next] = view.record.clone();
                            self.membership = Some(view);
                            self.detail_scroll = 0;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                Ok(())
            }
            KeyCode::PageDown => {
                self.detail_scroll = self.detail_scroll.saturating_add(8);
                Ok(())
            }
            KeyCode::PageUp => {
                self.detail_scroll = self.detail_scroll.saturating_sub(8);
                Ok(())
            }
            KeyCode::Enter => {
                if let Some(record) = self.selected() {
                    let id = record.metadata.id.clone();
                    let target = match self.kind {
                        PlanningKind::Project => Some(PanelTarget::Project { id }),
                        PlanningKind::Cycle => Some(PanelTarget::Cycle { id }),
                        PlanningKind::Milestone => Some(PanelTarget::Milestone { id }),
                        PlanningKind::Target => Some(PanelTarget::Target { id }),
                        PlanningKind::Label => Some(PanelTarget::Label { id }),
                        PlanningKind::Initiative => None,
                    };
                    return (true, target);
                }
                Ok(())
            }
            _ => return (false, None),
        };
        if let Err(error) = result {
            self.error = Some(error);
        }
        (true, None)
    }
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        self.bounds = Some(area);
        if self.showing_carryover() {
            self.carryover.as_mut().unwrap().render(area, buffer, theme);
            return;
        }
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
        if let Some(policy) = &self.policy_draft {
            super::shell_view::render_form(&policy.form, rows[0], buffer, theme);
        } else if let Some(draft) = self.active_draft() {
            super::shell_view::render_form(&draft.form, rows[0], buffer, theme);
        } else {
            let columns = if area.width >= 90 {
                Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
                    .split(rows[0])
            } else {
                Layout::vertical([Constraint::Percentage(40), Constraint::Percentage(60)])
                    .split(rows[0])
            };
            if let Some(index) = &mut self.index {
                index.resize(columns[0].height.saturating_sub(2) / 2);
            }
            let (entries, selected) = if let Some(index) = &self.index {
                let selected = index.selected_row().map(|row| &row.token.key);
                let rows = if index.is_idle()
                    && !index.refreshing
                    && !index.querying
                    && !index.loading_page
                {
                    index.visible_rows().collect::<Vec<_>>()
                } else {
                    Vec::new()
                };
                let selected = rows
                    .iter()
                    .position(|(_, row)| Some(&row.token.key) == selected);
                (
                    rows.iter()
                        .map(|(_, row)| {
                            ListItem::new(format!(
                                "{}{}\n{}",
                                if row.archived { "[archived] " } else { "" },
                                sanitize_terminal_line(&row.title),
                                row.token.key.id
                            ))
                        })
                        .collect::<Vec<_>>(),
                    selected,
                )
            } else {
                (
                    self.records
                        .iter()
                        .map(|record| {
                            ListItem::new(format!(
                                "{}{}\n{}",
                                if record.metadata.archived {
                                    "[archived] "
                                } else {
                                    ""
                                },
                                sanitize_terminal_line(&record.metadata.name),
                                record.metadata.id
                            ))
                        })
                        .collect::<Vec<_>>(),
                    self.selected_index(),
                )
            };
            let mut state = ListState::default().with_selected(selected);
            *state.offset_mut() = if self.indexed { 0 } else { self.offset };
            StatefulWidget::render(
                List::new(entries)
                    .style(style)
                    .block(Block::default().borders(Borders::ALL).title(format!(
                        "{} · {}",
                        title(self.kind),
                        if self.include_archived {
                            "all"
                        } else {
                            "active"
                        }
                    )))
                    .highlight_symbol("› ")
                    .highlight_style(
                        style
                            .fg(ratatui_theme_color(&theme.accent))
                            .add_modifier(Modifier::BOLD),
                    ),
                columns[0],
                buffer,
                &mut state,
            );
            self.offset = state.offset();
            let body = if let Some(view) = &self.membership {
                let mut text = format!(
                    "{}\n{}\n\n{}\n\n",
                    view.record.metadata.name, view.record.metadata.id, view.record.body
                );
                if self.membership_requested.is_some() {
                    text.insert_str(
                        0,
                        "Loading selected membership; previous inspected membership retained\n\n",
                    );
                }
                let metadata = &view.record.metadata;
                for (label, value) in [
                    ("Lead", &metadata.lead),
                    ("Scope", &metadata.scope),
                    ("Goal", &metadata.goal),
                    ("Start", &metadata.starts_at),
                    ("End", &metadata.ends_at),
                    ("Initiative", &metadata.initiative),
                    ("Project", &metadata.project),
                ] {
                    if let Some(value) = value {
                        text.push_str(&format!("{label}: {value}\n"));
                    }
                }
                if !metadata.targets.is_empty() {
                    text.push_str(&format!("Targets: {}\n", metadata.targets.join(", ")));
                }
                for (label, criteria) in [
                    ("Exit criteria", &metadata.exit_criteria),
                    ("Outcomes", &metadata.outcomes),
                ] {
                    if !criteria.is_empty() {
                        text.push_str(&format!("\n{label}\n"));
                        for criterion in criteria {
                            text.push_str(&format!(
                                "{}: {}\n",
                                criterion.id, criterion.description
                            ));
                        }
                    }
                }
                if let Some(policy) = &self.policy {
                    text.push_str(&format!(
                        "\nCompletion policy: {} ({:?})\n",
                        if policy.allowed { "ready" } else { "blocked" },
                        policy.basis
                    ));
                    for condition in &policy.conditions {
                        text.push_str(&format!(
                            "{} · {:?} · {}\n",
                            condition.reason_code, condition.state, condition.message
                        ));
                    }
                }
                if !view.related_records.is_empty() {
                    text.push_str("\nRelated planning records\n");
                    for record in &view.related_records {
                        text.push_str(&format!(
                            "{} · {}{}\n",
                            record.metadata.id,
                            record.metadata.name,
                            if record.metadata.archived {
                                " [archived]"
                            } else {
                                ""
                            }
                        ));
                    }
                }
                text.push_str(&format!(
                    "\n{} {} issue members\n",
                    view.issues.len(),
                    if view.query.issues.archive == workdeck_pm::ArchiveFilter::All {
                        "total"
                    } else {
                        "active"
                    }
                ));
                for issue in &view.issues {
                    text.push_str(&format!(
                        "{} · {} · {}\n",
                        issue.metadata.id, issue.metadata.status, issue.metadata.title
                    ));
                }
                for warning in &view.warnings {
                    text.push_str(&format!("\nUnresolved: {}\n", warning.message));
                }
                text
            } else if self.membership_requested.is_some()
                || self.index.as_ref().is_some_and(|index| {
                    index.handle.is_none()
                        || index.refreshing
                        || index.querying
                        || index.loading_page
                })
            {
                String::from("Loading planning source and membership…")
            } else {
                format!(
                    "No {}. Press n to create one.\n\n[ / ] switches planning kind. Projects describe deliverables; cycles schedule work.",
                    title(self.kind).to_lowercase()
                )
            };
            let body = body
                .lines()
                .map(sanitize_terminal_line)
                .collect::<Vec<_>>()
                .join("\n");
            Paragraph::new(body)
                .style(style)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Details and membership"),
                )
                .wrap(Wrap { trim: false })
                .scroll((self.detail_scroll, 0))
                .render(columns[1], buffer);
        }
        let controls = if self.policy_draft.is_some() {
            "Tab field · Ctrl-S complete · Esc retain form · Ctrl-D discard"
        } else if self.active_draft().is_some() {
            "Tab field · Ctrl-S save · Ctrl-U clear
Esc retain draft · Ctrl-D discard draft"
        } else if self.kind == PlanningKind::Cycle {
            "u carryover · n create · e edit · a archive/restore · Enter members\nr refresh · x active/all · [ / ] kind · PgUp/PgDn details"
        } else if self.kind == PlanningKind::Initiative {
            "n create · e edit · a archive/restore · F11 projects\nr refresh · x active/all · [ / ] kind · PgUp/PgDn details"
        } else {
            "n create · e edit · p assess/complete · a archive/restore · Enter issue members\nr refresh · x active/all · [ / ] kind · PgUp/PgDn details"
        };
        let footer = if let Some(error) = &self.error {
            format!("{}\n{controls}", sanitize_terminal_line(&error.message))
        } else {
            controls.into()
        };
        Paragraph::new(footer)
            .style(style)
            .wrap(Wrap { trim: false })
            .render(rows[1], buffer);
    }
}

impl PlanningDraft {
    fn new(kind: PlanningKind, record: Option<&PlanningRecord>) -> Result<Self> {
        let metadata = record
            .map(|record| serde_json::to_value(&record.metadata))
            .transpose()
            .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?
            .unwrap_or_else(|| json!({}));
        let mut keys = vec!["name", "body"];
        match kind {
            PlanningKind::Project => keys.extend([
                "lead",
                "scope",
                "goal",
                "initiative",
                "starts_at",
                "ends_at",
                "exit_criteria",
                "targets",
            ]),
            PlanningKind::Milestone => {
                keys.extend(["project", "outcomes", "starts_at", "ends_at", "targets"])
            }
            PlanningKind::Cycle => keys.extend(["status", "starts_at", "ends_at"]),
            PlanningKind::Initiative => keys.extend(["lead", "scope", "goal", "outcomes"]),
            PlanningKind::Target => keys.extend(["goal", "starts_at", "ends_at"]),
            PlanningKind::Label => keys.push("color"),
        }
        keys.push("custom");
        let fields = keys
            .iter()
            .map(|key| {
                let (label, multiline) = match *key {
                    "name" => ("Name", false),
                    "body" => ("Description", true),
                    "lead" => ("Lead", false),
                    "scope" => ("Scope", false),
                    "goal" => ("Goal", false),
                    "initiative" => ("Initiative ID", false),
                    "project" => ("Project ID", false),
                    "starts_at" => ("Start date", false),
                    "ends_at" => ("End date", false),
                    "exit_criteria" => ("Exit criteria (ID=description)", true),
                    "outcomes" => ("Outcomes (ID=description)", true),
                    "targets" => ("Target IDs (one per line)", true),
                    "status" => ("Status", false),
                    "color" => ("Color", false),
                    "custom" => ("Custom fields (JSON object)", true),
                    _ => unreachable!(),
                };
                let value = match *key {
                    "custom" => metadata["custom"]
                        .as_object()
                        .map(|value| serde_json::to_string(value).expect("JSON object"))
                        .unwrap_or_else(|| "{}".into()),
                    "body" => record.map(|record| record.body.clone()).unwrap_or_default(),
                    "targets" => metadata["targets"]
                        .as_array()
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(Value::as_str)
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default(),
                    "exit_criteria" | "outcomes" => metadata[*key]
                        .as_array()
                        .map(|values| {
                            values
                                .iter()
                                .map(|criterion| {
                                    format!(
                                        "{}={}",
                                        criterion["id"].as_str().unwrap_or_default(),
                                        criterion["description"].as_str().unwrap_or_default()
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join("\n")
                        })
                        .unwrap_or_default(),
                    _ => metadata[*key].as_str().unwrap_or_default().into(),
                };
                TextField::new(label, value, multiline)
            })
            .collect();
        Ok(Self {
            kind,
            id: record.map(|record| record.metadata.id.clone()),
            expected: record.map(|record| record.source.clone()),
            form: WorkbenchForm::new(
                FormKind::Planning,
                format!(
                    "{} {}",
                    if record.is_some() { "Edit" } else { "Create" },
                    title(kind)
                ),
                fields,
            ),
            keys,
            attempt: None,
        })
    }
    fn values(&self) -> Result<(String, String, BTreeMap<String, Value>)> {
        let mut fields = BTreeMap::new();
        for (key, field) in self.keys.iter().zip(&self.form.fields) {
            if ["name", "body"].contains(key) {
                continue;
            }
            let value = match *key {
                "custom" => {
                    let value: Value = serde_json::from_str(&field.value).map_err(|error| {
                        PmError::new(
                            ErrorCode::InvalidInput,
                            format!("Custom fields must be a JSON object: {error}"),
                        )
                    })?;
                    if !value.is_object() {
                        return Err(PmError::new(
                            ErrorCode::InvalidInput,
                            "Custom fields must be a JSON object",
                        ));
                    }
                    value
                }
                "targets" => json!(
                    field
                        .value
                        .lines()
                        .map(str::trim)
                        .filter(|line| !line.is_empty())
                        .collect::<Vec<_>>()
                ),
                "exit_criteria" | "outcomes" => {
                    let mut criteria = Vec::new();
                    for line in field.value.lines().filter(|line| !line.trim().is_empty()) {
                        let (id, description) = line.split_once('=').ok_or_else(|| {
                            PmError::new(
                                ErrorCode::InvalidInput,
                                "Write each criterion as ID=description",
                            )
                        })?;
                        criteria.push(json!({"id":id.trim(),"description":description.trim()}));
                    }
                    json!(criteria)
                }
                _ => {
                    if field.value.is_empty() {
                        Value::Null
                    } else {
                        json!(field.value)
                    }
                }
            };
            if self.id.is_some() || !value.is_null() && !value.as_array().is_some_and(Vec::is_empty)
            {
                fields.insert((*key).into(), value);
            }
        }
        Ok((
            self.form.fields[0].value.clone(),
            self.form.fields[1].value.clone(),
            fields,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selecting_another_record_retains_all_members_scope() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        for id in ["first", "second"] {
            let mut create = CreatePlanning::new(id);
            create.id = Some(id.into());
            repository
                .create_planning(PlanningKind::Project, &create, &RequestId::new())
                .unwrap();
            let receipt = repository
                .create_issue(
                    &workdeck_pm::CreateIssue {
                        title: format!("Archived {id}"),
                        body: String::new(),
                        fields: BTreeMap::from([("project".into(), json!(id))]),
                    },
                    &RequestId::new(),
                )
                .unwrap();
            let issue: workdeck_pm::IssueRecord = serde_json::from_value(receipt.result).unwrap();
            repository
                .mutate_issue(
                    issue.metadata.id.as_str(),
                    None,
                    &workdeck_pm::IssueMutation::Archive { archived: true },
                    &RequestId::new(),
                )
                .unwrap();
        }
        let mut workspace = PlanningWorkspace::new(Some(repository));
        workspace.open(PlanningKind::Project);
        workspace.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        assert_eq!(workspace.membership.as_ref().unwrap().issues.len(), 1);
        workspace.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        let membership = workspace.membership.as_ref().unwrap();
        assert_eq!(
            membership.query.issues.archive,
            workdeck_pm::ArchiveFilter::All
        );
        assert_eq!(membership.issues.len(), 1);
    }

    #[test]
    fn stale_draft_keeps_input_until_explicit_discard_then_edits_current_source() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let mut create = CreatePlanning::new("Original");
        create.id = Some("project".into());
        repository
            .create_planning(PlanningKind::Project, &create, &RequestId::new())
            .unwrap();
        let mut workspace = PlanningWorkspace::new(Some(repository.clone()));
        workspace.open(PlanningKind::Project);
        workspace.start_draft(true).unwrap();
        let original = workspace.active_draft().unwrap().expected.clone();
        workspace
            .drafts
            .get_mut(&("Projects".into(), "project".into()))
            .unwrap()
            .form
            .fields[1]
            .value = "My unsaved body".into();
        repository
            .mutate_planning(
                PlanningKind::Project,
                "project",
                None,
                &PlanningMutation::Update {
                    fields: BTreeMap::from([("name".into(), json!("External name"))]),
                    body: None,
                },
                &RequestId::new(),
            )
            .unwrap();
        assert_eq!(workspace.submit().unwrap_err().code, ErrorCode::StaleSource);
        workspace.refresh().unwrap();
        assert_eq!(workspace.active_draft().unwrap().expected, original);
        assert_eq!(
            workspace.active_draft().unwrap().form.fields[1].value,
            "My unsaved body"
        );
        workspace.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
        assert!(workspace.active_draft().is_none());
        workspace.start_draft(true).unwrap();
        assert_ne!(workspace.active_draft().unwrap().expected, original);
        assert_eq!(
            workspace.active_draft().unwrap().form.fields[0].value,
            "External name"
        );
        workspace.submit().unwrap();
        assert!(workspace.last_receipt.is_some());
        assert!(workspace.drafts.is_empty());
    }

    #[test]
    fn planning_form_satisfies_required_custom_fields_and_retains_invalid_input() {
        use workdeck_pm::{CustomFieldDefinition, CustomFieldType, CustomScope, SchemaChange};
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let change = SchemaChange {
            fields: BTreeMap::from([(
                "risk".into(),
                CustomFieldDefinition {
                    field_type: CustomFieldType::Text,
                    scopes: std::collections::BTreeSet::from([CustomScope::Project]),
                    required: true,
                    archived: false,
                    options: Vec::new(),
                    custom: BTreeMap::new(),
                    extra: BTreeMap::new(),
                },
            )]),
            ..Default::default()
        };
        repository
            .apply_schema_change(&change, None, &RequestId::new())
            .unwrap();
        let mut workspace = PlanningWorkspace::new(Some(repository.clone()));
        workspace.open(PlanningKind::Project);
        workspace.start_draft(false).unwrap();
        let key = workspace.active_key().unwrap();
        workspace.drafts.get_mut(&key).unwrap().form.fields[0].value = "Policy project".into();
        assert!(workspace.submit().is_err());
        assert!(
            repository
                .list_planning(PlanningKind::Project)
                .unwrap()
                .is_empty()
        );
        workspace
            .drafts
            .get_mut(&key)
            .unwrap()
            .form
            .fields
            .last_mut()
            .unwrap()
            .value = "{unfinished".into();
        assert_eq!(
            workspace.submit().unwrap_err().code,
            ErrorCode::InvalidInput
        );
        assert_eq!(
            workspace
                .active_draft()
                .unwrap()
                .form
                .fields
                .last()
                .unwrap()
                .value,
            "{unfinished"
        );
        workspace
            .drafts
            .get_mut(&key)
            .unwrap()
            .form
            .fields
            .last_mut()
            .unwrap()
            .value = "{\"risk\":\"reviewed\"}".into();
        workspace.submit().unwrap();
        assert_eq!(
            repository.list_planning(PlanningKind::Project).unwrap()[0]
                .metadata
                .custom["risk"],
            json!("reviewed")
        );
    }

    #[test]
    fn initiative_form_authors_outcomes_as_declarations() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let mut workspace = PlanningWorkspace::new(Some(repository.clone()));
        workspace.open(PlanningKind::Initiative);
        workspace.start_draft(false).unwrap();
        let key = workspace.active_key().unwrap();
        let draft = workspace.drafts.get_mut(&key).unwrap();
        draft.form.fields[0].value = "Outcome initiative".into();
        let index = draft
            .keys
            .iter()
            .position(|key| *key == "outcomes")
            .unwrap();
        draft.form.fields[index].value = "adoption=Users can organize work".into();
        workspace.submit().unwrap();
        let record = repository
            .list_planning(PlanningKind::Initiative)
            .unwrap()
            .remove(0);
        assert_eq!(record.metadata.outcomes[0].id, "adoption");
        assert!(record.metadata.exit_criteria.is_empty());
    }

    #[test]
    fn changed_draft_after_lost_create_acknowledgement_cannot_publish_a_duplicate() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let mut workspace = PlanningWorkspace::new(Some(repository.clone()));
        workspace.open(PlanningKind::Project);
        workspace.start_draft(false).unwrap();
        let key = workspace.active_key().unwrap();
        let draft = workspace.drafts.get_mut(&key).unwrap();
        draft.form.fields[0].value = "Original creation".into();
        let (name, body, fields) = draft.values().unwrap();
        let request = RequestId::new();
        let payload = json!({"kind":draft.kind,"id":draft.id,"expected":draft.expected,"name":name,"body":body,"fields":fields});
        draft.attempt = Some((payload, request.clone()));
        // Publication succeeded but the UI never received its acknowledgement.
        repository
            .create_planning(
                PlanningKind::Project,
                &CreatePlanning {
                    id: None,
                    name,
                    body,
                    fields,
                },
                &request,
            )
            .unwrap();
        draft.form.fields[0].value = "Edited after lost acknowledgement".into();
        assert_eq!(
            workspace.submit().unwrap_err().code,
            ErrorCode::IdempotencyConflict
        );
        assert_eq!(
            repository
                .list_planning(PlanningKind::Project)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            workspace.active_draft().unwrap().form.fields[0].value,
            "Edited after lost acknowledgement"
        );
        assert_eq!(
            workspace
                .active_draft()
                .unwrap()
                .attempt
                .as_ref()
                .unwrap()
                .1,
            request
        );
    }

    #[test]
    fn failed_kind_switch_never_exposes_previous_kind_rows_or_mutations() {
        let temp = tempfile::tempdir().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let mut create = CreatePlanning::new("Original project");
        create.id = Some("shared".into());
        repository
            .create_planning(PlanningKind::Project, &create, &RequestId::new())
            .unwrap();
        let mut workspace = PlanningWorkspace::new(Some(repository.clone()));
        workspace.open(PlanningKind::Project);
        workspace.start_draft(true).unwrap();
        std::fs::create_dir_all(repository.root().join("cycles/broken")).unwrap();
        std::fs::write(repository.root().join("cycles/broken/item.md"), "invalid").unwrap();
        workspace.open(PlanningKind::Cycle);
        assert!(workspace.error.is_some());
        assert!(
            workspace.selected().is_none(),
            "previous-kind rows must not be exposed as the new kind"
        );
        assert!(workspace.records.is_empty());
        assert!(workspace.membership.is_none());
        assert!(workspace.archive().is_err());
        assert!(
            workspace
                .drafts
                .contains_key(&("Projects".into(), "shared".into()))
        );
    }
    #[test]
    fn mounted_planning_collection_retains_only_selected_native_record() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        for name in ["First project", "Second project", "Third project"] {
            repository
                .create_planning(
                    PlanningKind::Project,
                    &CreatePlanning::new(name),
                    &RequestId::new(),
                )
                .unwrap();
        }
        let mut shell = crate::workbench::WorkbenchShell::open(
            crate::workbench::WorkbenchOptions::new(directory.path()),
            true,
        );
        shell.key(KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE));
        assert_eq!(shell.tab, crate::workbench::WorkbenchTab::Planning);
        crate::workbench::test_pump::settle_shell(&mut shell);
        assert!(
            shell
                .planning
                .index
                .as_ref()
                .is_some_and(|index| index.handle.is_some()),
            "{:?}",
            shell.planning.error
        );
        assert_eq!(
            shell
                .planning
                .index
                .as_ref()
                .unwrap()
                .handle
                .as_ref()
                .unwrap()
                .total,
            3
        );
        let first = shell.planning.selected().unwrap().metadata.id.clone();
        shell.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        crate::workbench::test_pump::settle_shell(&mut shell);
        let last = shell.planning.selected().unwrap().metadata.id.clone();
        assert_ne!(first, last);
        shell.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        let draft_key = shell.planning.active_key().unwrap();
        shell
            .planning
            .drafts
            .get_mut(&draft_key)
            .unwrap()
            .form
            .fields[0]
            .value = "Retained project draft".into();
        shell.key(KeyEvent::new(KeyCode::F(12), KeyModifiers::NONE));
        crate::workbench::test_pump::settle_shell(&mut shell);
        assert_eq!(shell.planning.kind, PlanningKind::Cycle);
        assert!(shell.planning.records.is_empty());
        shell.key(KeyEvent::new(KeyCode::F(11), KeyModifiers::NONE));
        crate::workbench::test_pump::settle_shell(&mut shell);
        assert_eq!(shell.planning.selected().unwrap().metadata.id, last);
        assert_eq!(
            shell.planning.active_draft().unwrap().form.fields[0].value,
            "Retained project draft"
        );
        assert_eq!(shell.planning.records.len(), 1);
        assert!(
            shell.planning.records.len() <= 1,
            "mounted Planning retained {} native documents; the indexed collection must remain separately navigable",
            shell.planning.records.len()
        );
    }

    #[test]
    fn indexed_empty_planning_kinds_do_not_invent_selectable_records() {
        let directory = tempfile::tempdir().unwrap();
        Repository::init(directory.path(), "WD").unwrap();
        std::fs::write(
            directory.path().join(".workdeck/labels.yml"),
            "schema: 1\nlabels: []\n",
        )
        .unwrap();
        let mut shell = crate::workbench::WorkbenchShell::open(
            crate::workbench::WorkbenchOptions::new(directory.path()),
            true,
        );
        for kind in KINDS {
            shell.planning.open(kind);
            crate::workbench::test_pump::settle_shell(&mut shell);
            assert!(
                shell.planning.error.is_none(),
                "{kind:?}: {:?}",
                shell.planning.error
            );
            assert_eq!(
                shell
                    .planning
                    .index
                    .as_ref()
                    .unwrap()
                    .handle
                    .as_ref()
                    .unwrap()
                    .total,
                0,
                "{kind:?} must have no selectable records"
            );
            assert!(shell.planning.selected().is_none());
            assert!(shell.planning.membership.is_none());
        }
    }

    #[test]
    fn project_policy_control_retains_source_pin_and_publishes_completion() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        let project: PlanningRecord = serde_json::from_value(
            repository
                .create_planning(
                    PlanningKind::Project,
                    &CreatePlanning {
                        id: Some("policy-project".into()),
                        name: "Policy project".into(),
                        body: "Project body".into(),
                        fields: BTreeMap::from([(
                            "exit_criteria".into(),
                            json!([{"id":"shipped","description":"Release shipped"}]),
                        )]),
                    },
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        let issue: workdeck_pm::IssueRecord = serde_json::from_value(
            repository
                .create_issue(
                    &workdeck_pm::CreateIssue {
                        title: "Ship project".into(),
                        body: "Implementation".into(),
                        fields: BTreeMap::from([("project".into(), json!("policy-project"))]),
                    },
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        repository
            .complete_issue(
                issue.metadata.id.as_str(),
                &issue.source,
                None,
                &RequestId::new(),
            )
            .unwrap();
        let mut workspace = PlanningWorkspace::new(Some(repository.clone()));
        workspace.open(PlanningKind::Project);
        assert!(
            workspace
                .policy
                .as_ref()
                .is_some_and(|policy| !policy.allowed)
        );
        let expected = workspace.selected().unwrap().source.clone();
        workspace.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
        assert_eq!(workspace.policy_draft.as_ref().unwrap().expected, expected);
        let draft = workspace.policy_draft.as_mut().unwrap();
        draft.form.fields[0].value = "local".into();
        draft.form.fields[1].value = "Reviewed the project exit criteria".into();
        workspace.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(workspace.error.is_none(), "{:?}", workspace.error);
        assert!(workspace.policy_draft.is_none());
        assert!(workspace.last_receipt.is_some());
        assert_eq!(
            repository
                .planning_record(PlanningKind::Project, &project.metadata.id)
                .unwrap()
                .metadata
                .status
                .as_deref(),
            Some("done")
        );
    }
}
