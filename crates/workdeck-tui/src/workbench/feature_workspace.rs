//! Native feature authoring retains source-bound drafts and uses shared coverage.
use super::input::{FormAction, FormKind, TextField, WorkbenchForm};
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
    CreateFeature, FeatureCoverage, FeatureCoverageQuery, FeatureMaturity, FeatureMutation,
    FeatureOutcome, FeatureRecord, PmError, PolicyAcceptance, PolicyAssessment, Repository,
    RequestId, SourceToken, transactions::MutationReceipt,
};

#[path = "feature_filter.rs"]
mod filtering;
#[path = "feature_index.rs"]
mod indexed;

type Result<T> = std::result::Result<T, PmError>;

fn maturity(value: &str) -> Result<FeatureMaturity> {
    match value.trim() {
        "draft" => Ok(FeatureMaturity::Draft),
        "specified" => Ok(FeatureMaturity::Specified),
        "implemented" => Ok(FeatureMaturity::Implemented),
        _ => Err(PmError::new(
            workdeck_pm::ErrorCode::InvalidInput,
            "Maturity must be draft, specified, or implemented",
        )),
    }
}

fn next_maturity(value: FeatureMaturity) -> Option<FeatureMaturity> {
    match value {
        FeatureMaturity::Draft => Some(FeatureMaturity::Specified),
        FeatureMaturity::Specified => Some(FeatureMaturity::Implemented),
        FeatureMaturity::Implemented => None,
    }
}

#[derive(Debug)]
struct Draft {
    id: Option<String>,
    expected: Option<SourceToken>,
    form: WorkbenchForm,
    request: Option<RequestId>,
}

#[derive(Debug)]
struct ArchiveAttempt {
    id: String,
    expected: SourceToken,
    archived: bool,
    request: RequestId,
}

#[derive(Debug)]
struct MaturityDraft {
    id: String,
    expected: SourceToken,
    form: WorkbenchForm,
    attempt: Option<(Value, RequestId)>,
}
#[derive(Debug)]
pub(super) struct FeatureWorkspace {
    repository: Option<Repository>,
    indexed: bool,
    filter: workdeck_pm::projection::ProjectionFeatureQuery,
    filter_form: Option<WorkbenchForm>,
    tree: bool,
    pending_tree: Option<(String, KeyCode)>,
    collapsed: std::collections::BTreeSet<workdeck_pm::FeatureId>,
    index: Option<super::indexed_workspace::IndexedWorkspace>,
    attempted: Option<workdeck_pm::projection::ProjectionRowToken>,
    pending_selected: Option<String>,
    coverage_worker: Option<indexed::CoverageWorker>,
    coverage_requested: Option<workdeck_pm::projection::ProjectionRowToken>,
    records: Vec<FeatureRecord>,
    selected: Option<String>,
    coverage: Option<FeatureCoverage>,
    include_archived: bool,
    offset: usize,
    scroll: u16,
    drafts: BTreeMap<String, Draft>,
    active: Option<String>,
    policy: Option<PolicyAssessment>,
    maturity_draft: Option<MaturityDraft>,
    archive: Option<ArchiveAttempt>,
    pub error: Option<PmError>,
    pub last_receipt: Option<MutationReceipt>,
}
impl FeatureWorkspace {
    pub fn new(repository: Option<Repository>) -> Self {
        Self {
            repository,
            indexed: false,
            filter: Default::default(),
            filter_form: None,
            tree: false,
            pending_tree: None,
            collapsed: Default::default(),
            index: None,
            attempted: None,
            pending_selected: None,
            coverage_worker: None,
            coverage_requested: None,
            records: Vec::new(),
            selected: None,
            coverage: None,
            include_archived: false,
            offset: 0,
            scroll: 0,
            drafts: BTreeMap::new(),
            active: None,
            policy: None,
            maturity_draft: None,
            archive: None,
            error: None,
            last_receipt: None,
        }
    }
    pub fn new_indexed(repository: Option<Repository>) -> Self {
        let mut workspace = Self::new(repository);
        workspace.indexed = true;
        workspace
    }
    pub fn bind_if_missing(&mut self, repository: Option<Repository>) {
        if self.repository.is_none() {
            self.repository = repository;
        }
    }
    fn repository(&self) -> Result<&Repository> {
        self.repository.as_ref().ok_or_else(|| {
            PmError::new(
                workdeck_pm::ErrorCode::NotInitialized,
                "Run workdeck init, then refresh features",
            )
        })
    }
    fn selected_record(&self) -> Option<&FeatureRecord> {
        self.records
            .iter()
            .find(|record| Some(record.metadata.id.as_str()) == self.selected.as_deref())
    }
    pub fn refresh(&mut self) -> Result<()> {
        if self.indexed {
            return self.refresh_index();
        }
        self.refresh_after_list(|| {})
    }
    fn refresh_after_list(&mut self, after_list: impl FnOnce()) -> Result<()> {
        let mut records = self.repository()?.list_features()?;
        after_list();
        records.retain(|record| {
            self.include_archived || (!record.metadata.archived && record.retirement.is_none())
        });
        let selected = records
            .iter()
            .find(|record| Some(record.metadata.id.as_str()) == self.selected.as_deref())
            .or_else(|| records.first());
        let coverage = selected
            .map(|record| {
                self.repository()?
                    .feature_coverage(&FeatureCoverageQuery::new(record.metadata.id.as_str()))
            })
            .transpose()?;
        if let (Some(coverage), Some(record)) = (&coverage, selected)
            && coverage.feature != *record
        {
            return Err(PmError::new(
                workdeck_pm::ErrorCode::StaleSource,
                "Selected feature changed between the feature list and coverage reads",
            )
            .at(&record.path)
            .hint("Refresh to inspect the updated source; retained draft preconditions remain unchanged."));
        }
        let policy = coverage
            .as_ref()
            .and_then(|view| next_maturity(view.feature.metadata.maturity))
            .map(|requested| {
                let id = coverage
                    .as_ref()
                    .expect("coverage present")
                    .feature
                    .metadata
                    .id
                    .clone();
                self.repository()?
                    .assess_feature_maturity(id.as_str(), requested)
            })
            .transpose()?;
        self.selected = coverage
            .as_ref()
            .map(|view| view.feature.metadata.id.to_string());
        self.records = records;
        self.coverage = coverage;
        self.policy = policy;
        self.error = None;
        Ok(())
    }
    pub fn open(&mut self) {
        if let Err(error) = self.refresh() {
            self.error = Some(error);
        }
    }
    fn draft(&mut self, edit: bool) -> Result<()> {
        let record = if edit {
            Some(self.selected_record().cloned().ok_or_else(|| {
                PmError::new(workdeck_pm::ErrorCode::NotFound, "Select a feature to edit")
            })?)
        } else {
            None
        };
        let key = record
            .as_ref()
            .map(|record| record.metadata.id.to_string())
            .unwrap_or_default();
        if !self.drafts.contains_key(&key) {
            let value = record
                .as_ref()
                .map(|record| serde_json::to_value(&record.metadata))
                .transpose()
                .map_err(|e| PmError::new(workdeck_pm::ErrorCode::InvalidInput, e.to_string()))?
                .unwrap_or_else(|| json!({}));
            let text =
                |field: &str, default: &str| value[field].as_str().unwrap_or(default).to_owned();
            let custom = serde_json::to_string_pretty(
                &value.get("custom").cloned().unwrap_or_else(|| json!({})),
            )
            .map_err(|e| PmError::new(workdeck_pm::ErrorCode::InvalidInput, e.to_string()))?;
            self.drafts.insert(
                key.clone(),
                Draft {
                    id: record.as_ref().map(|r| r.metadata.id.to_string()),
                    expected: record.as_ref().map(|r| r.source.clone()),
                    request: None,
                    form: WorkbenchForm::new(
                        FormKind::Planning,
                        if edit {
                            "Edit feature"
                        } else {
                            "Create feature"
                        },
                        vec![
                            TextField::new(
                                "Name",
                                record
                                    .as_ref()
                                    .map(|r| r.metadata.name.clone())
                                    .unwrap_or_default(),
                                false,
                            ),
                            TextField::new(
                                "Description",
                                record.as_ref().map(|r| r.body.clone()).unwrap_or_default(),
                                true,
                            ),
                            TextField::new(
                                "Decision (proposed/accepted/deferred/rejected)",
                                text("decision", "proposed"),
                                false,
                            ),
                            TextField::new(
                                "Maturity (draft/specified/implemented)",
                                text("maturity", "draft"),
                                false,
                            ),
                            TextField::new(
                                "Availability (unavailable/experimental/available/deprecated)",
                                text("availability", "unavailable"),
                                false,
                            ),
                            TextField::new("Custom fields (JSON object)", custom, true),
                        ],
                    ),
                },
            );
        }
        self.active = Some(key);
        Ok(())
    }
    fn start_maturity(&mut self) -> Result<()> {
        let record = self.selected_record().cloned().ok_or_else(|| {
            PmError::new(
                workdeck_pm::ErrorCode::NotFound,
                "Select a feature to assess its maturity policy",
            )
        })?;
        let requested = next_maturity(record.metadata.maturity).ok_or_else(|| {
            PmError::new(
                workdeck_pm::ErrorCode::InvalidInput,
                "This feature is already implemented",
            )
        })?;
        self.policy = Some(
            self.repository()?
                .assess_feature_maturity(record.metadata.id.as_str(), requested)?,
        );
        self.maturity_draft = Some(MaturityDraft {
            id: record.metadata.id.to_string(),
            expected: record.source,
            form: WorkbenchForm::new(
                FormKind::Planning,
                "Promote feature maturity",
                vec![
                    TextField::new(
                        "Target maturity (specified/implemented)",
                        match requested {
                            FeatureMaturity::Specified => "specified",
                            FeatureMaturity::Implemented => "implemented",
                            FeatureMaturity::Draft => "draft",
                        }
                        .into(),
                        false,
                    ),
                    TextField::new("Acceptance actor", String::new(), false),
                    TextField::new("Acceptance reason", String::new(), true),
                ],
            ),
            attempt: None,
        });
        Ok(())
    }
    fn submit_maturity(&mut self) -> Result<()> {
        let repository = self.repository()?.clone();
        let draft = self.maturity_draft.as_mut().ok_or_else(|| {
            PmError::new(
                workdeck_pm::ErrorCode::InvalidInput,
                "No active feature maturity form",
            )
        })?;
        let target = maturity(&draft.form.fields[0].value)?;
        let acceptance = PolicyAcceptance {
            actor: draft.form.fields[1].value.trim().to_owned(),
            reason: draft.form.fields[2].value.trim().to_owned(),
        };
        let payload = json!({
            "id": draft.id,
            "expected": draft.expected,
            "maturity": target,
            "acceptance": acceptance,
        });
        let request = match &draft.attempt {
            Some((original, _)) if original != &payload => {
                return Err(PmError::new(
                    workdeck_pm::ErrorCode::IdempotencyConflict,
                    "Maturity input changed after the first attempt; discard the form to start a new request",
                ));
            }
            Some((_, request)) => request.clone(),
            None => {
                let request = RequestId::new();
                draft.attempt = Some((payload, request.clone()));
                request
            }
        };
        let receipt = repository.promote_feature(
            &draft.id,
            &draft.expected,
            target,
            &acceptance,
            &request,
        )?;
        let outcome: FeatureOutcome =
            serde_json::from_value(receipt.result.clone()).map_err(|error| {
                PmError::new(workdeck_pm::ErrorCode::CorruptStore, error.to_string())
            })?;
        self.selected = Some(outcome.record.metadata.id.to_string());
        self.last_receipt = Some(receipt);
        self.maturity_draft = None;
        self.policy = None;
        if self.indexed {
            self.pending_selected = self.selected.clone();
            self.attempted = None;
        }
        if let Err(mut error) = self.refresh() {
            error.message = format!("Maturity saved; refresh failed: {}", error.message);
            self.error = Some(error);
        }
        Ok(())
    }
    fn submit(&mut self) -> Result<()> {
        let key = self.active.clone().ok_or_else(|| {
            PmError::new(
                workdeck_pm::ErrorCode::InvalidInput,
                "No active feature draft",
            )
        })?;
        let repository = self.repository()?.clone();
        let draft = self.drafts.get_mut(&key).expect("active draft exists");
        let value = |index: usize| draft.form.fields[index].value.clone();
        let custom: BTreeMap<String, Value> = serde_json::from_str(&value(5)).map_err(|error| {
            PmError::new(
                workdeck_pm::ErrorCode::InvalidInput,
                format!("Custom fields must be a JSON object: {error}"),
            )
        })?;
        let mut fields = BTreeMap::from([
            ("decision".into(), json!(value(2))),
            ("maturity".into(), json!(value(3))),
            ("availability".into(), json!(value(4))),
            ("custom".into(), json!(custom)),
        ]);
        let name = value(0);
        let body = value(1);
        // Retain the first request after any uncertain outcome; editing the form
        // must produce an idempotency conflict rather than a second publication.
        let request = draft.request.get_or_insert_with(RequestId::new).clone();
        let receipt = if let Some(id) = &draft.id {
            fields.insert("name".into(), json!(name));
            repository.mutate_feature(
                id,
                draft.expected.as_ref(),
                &FeatureMutation::Update {
                    fields,
                    body: Some(body),
                },
                &request,
            )?
        } else {
            repository.create_feature(
                &CreateFeature {
                    name,
                    body,
                    fields,
                    directory: None,
                },
                &request,
            )?
        };
        let outcome: FeatureOutcome = serde_json::from_value(receipt.result.clone())
            .map_err(|e| PmError::new(workdeck_pm::ErrorCode::CorruptStore, e.to_string()))?;
        self.selected = Some(outcome.record.metadata.id.to_string());
        self.last_receipt = Some(receipt);
        self.drafts.remove(&key);
        self.active = None;
        if self.indexed {
            self.pending_selected = self.selected.clone();
            self.attempted = None;
        }
        if let Err(mut error) = self.refresh() {
            error.message = format!("Feature saved; refresh failed: {}", error.message);
            self.error = Some(error);
        }
        Ok(())
    }
    fn archive(&mut self) -> Result<()> {
        let record = self.selected_record().cloned().ok_or_else(|| {
            PmError::new(
                workdeck_pm::ErrorCode::NotFound,
                "Select a feature to archive or restore",
            )
        })?;
        if self
            .archive
            .as_ref()
            .is_none_or(|attempt| attempt.id != record.metadata.id.as_str())
        {
            self.archive = Some(ArchiveAttempt {
                id: record.metadata.id.to_string(),
                expected: record.source,
                archived: !record.metadata.archived,
                request: RequestId::new(),
            });
        }
        let attempt = self.archive.as_ref().expect("prepared archive");
        let receipt = self.repository()?.mutate_feature(
            &attempt.id,
            Some(&attempt.expected),
            &FeatureMutation::Archive {
                archived: attempt.archived,
            },
            &attempt.request,
        )?;
        self.last_receipt = Some(receipt);
        self.archive = None;
        if self.indexed {
            self.pending_selected = if self.include_archived || record.metadata.archived {
                self.selected.clone()
            } else {
                None
            };
            self.attempted = None;
        }
        if let Err(mut error) = self.refresh() {
            error.message = format!("Archive saved; refresh failed: {}", error.message);
            self.error = Some(error);
        }
        Ok(())
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if let Some(form) = &mut self.filter_form {
            if let Some(field) = form.fields.get_mut(form.selected) {
                field.insert(text);
            }
            return true;
        }
        if let Some(draft) = &mut self.maturity_draft {
            if let Some(field) = draft.form.fields.get_mut(draft.form.selected) {
                field.insert(text);
            }
            return true;
        }
        if let Some(draft) = self
            .active
            .as_ref()
            .and_then(|key| self.drafts.get_mut(key))
            && let Some(field) = draft.form.fields.get_mut(draft.form.selected)
        {
            field.insert(text);
        }
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        self.poll_index();
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return false;
        }
        if let Some(form) = &mut self.filter_form {
            let result = match form.key(key) {
                FormAction::Submit => self.submit_filter(),
                FormAction::Close => {
                    self.filter_form = None;
                    Ok(())
                }
                FormAction::Edited => Ok(()),
            };
            if let Err(error) = result {
                self.error = Some(error);
            }
            return true;
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('d') {
            if let Some(key) = self.active.take() {
                self.drafts.remove(&key);
            }
            self.maturity_draft = None;
            self.policy = None;
            self.archive = None;
            self.error = None;
            return true;
        }
        if self.maturity_draft.is_some() {
            let action = self
                .maturity_draft
                .as_mut()
                .expect("active maturity form")
                .form
                .key(key);
            match action {
                FormAction::Submit => {
                    if let Err(error) = self.submit_maturity() {
                        self.error = Some(error);
                    }
                }
                FormAction::Close => {
                    self.maturity_draft = None;
                    self.policy = None;
                }
                FormAction::Edited => {}
            }
            return true;
        }
        if let Some(draft) = self
            .active
            .as_ref()
            .and_then(|key| self.drafts.get_mut(key))
        {
            let result = match draft.form.key(key) {
                FormAction::Submit => self.submit(),
                FormAction::Close => {
                    self.active = None;
                    Ok(())
                }
                FormAction::Edited => Ok(()),
            };
            if let Err(error) = result {
                self.error = Some(error);
            }
            return true;
        }
        if !key.modifiers.is_empty() {
            return false;
        }
        if self.indexed
            && (key.code == KeyCode::Char('t')
                || self.tree && matches!(key.code, KeyCode::Left | KeyCode::Right))
        {
            if let Err(error) = self.tree_key(key.code) {
                self.error = Some(error);
            }
            self.poll_index();
            return true;
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
            self.poll_index();
            return true;
        }
        if self.indexed
            && matches!(key.code, KeyCode::Char('e' | 'a'))
            && let Err(error) = self.select_index_record()
        {
            self.error = Some(error);
            return true;
        }
        let result = match key.code {
            KeyCode::Char('q') => return false,
            KeyCode::Char('/') if self.indexed => self.open_filter(),
            KeyCode::Char('n') => self.draft(false),
            KeyCode::Char('e') => self.draft(true),
            KeyCode::Char('m') => self.start_maturity(),
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
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
                let index = self
                    .records
                    .iter()
                    .position(|record| {
                        Some(record.metadata.id.as_str()) == self.selected.as_deref()
                    })
                    .unwrap_or(0);
                let index = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                    index.saturating_sub(1)
                } else {
                    index
                        .saturating_add(1)
                        .min(self.records.len().saturating_sub(1))
                };
                let previous = self.selected.clone();
                if let Some(record) = self.records.get(index) {
                    self.selected = Some(record.metadata.id.to_string());
                }
                self.scroll = 0;
                let result = self.refresh();
                if result.is_err() {
                    self.selected = previous;
                }
                result
            }
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(10);
                Ok(())
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(10);
                Ok(())
            }
            _ => return false,
        };
        if let Err(error) = result {
            self.error = Some(error);
        }
        true
    }
    pub fn render(&mut self, area: Rect, buffer: &mut Buffer, theme: &AppTheme) {
        let style = Style::default()
            .fg(ratatui_theme_color(&theme.text))
            .bg(ratatui_theme_color(&theme.panel));
        Block::default().style(style).render(area, buffer);
        let rows = Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).split(area);
        if let Some(form) = &self.filter_form {
            super::shell_view::render_form(form, rows[0], buffer, theme);
        } else if let Some(draft) = &self.maturity_draft {
            super::shell_view::render_form(&draft.form, rows[0], buffer, theme);
        } else if let Some(draft) = self.active.as_ref().and_then(|key| self.drafts.get(key)) {
            super::shell_view::render_form(&draft.form, rows[0], buffer, theme);
        } else {
            let columns = if area.width >= 100 {
                Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
                    .split(rows[0])
            } else {
                Layout::vertical([Constraint::Length(6), Constraint::Min(0)]).split(rows[0])
            };
            if let Some(index) = &mut self.index {
                index.resize(columns[0].height.saturating_sub(2));
            }
            let (items, selected) = if let Some(index) = &self.index {
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
                            ListItem::new(sanitize_terminal_line(&format!(
                                "{}{}",
                                self.tree_label(row),
                                if row.archived { " [archived]" } else { "" }
                            )))
                        })
                        .collect::<Vec<_>>(),
                    selected,
                )
            } else {
                (
                    self.records
                        .iter()
                        .map(|record| {
                            ListItem::new(sanitize_terminal_line(&format!(
                                "{}{}",
                                record.metadata.name,
                                if record.metadata.archived {
                                    " [archived]"
                                } else {
                                    ""
                                }
                            )))
                        })
                        .collect::<Vec<_>>(),
                    self.records.iter().position(|record| {
                        Some(record.metadata.id.as_str()) == self.selected.as_deref()
                    }),
                )
            };
            let mut state = ListState::default()
                .with_selected(selected)
                .with_offset(if self.indexed { 0 } else { self.offset });
            StatefulWidget::render(
                List::new(items)
                    .block(Block::default().borders(Borders::ALL).title(format!(
                            " {} · {}{}{} ",
                            if self.tree {
                                "Feature tree"
                            } else {
                                "Features"
                            },
                            self.index
                                .as_ref()
                                .and_then(|index| index.handle.as_ref())
                                .map(|handle| handle.total.to_string())
                                .unwrap_or_else(|| if self.indexed {
                                    "loading".into()
                                } else {
                                    self.records.len().to_string()
                                }),
                            if self.filtered() { " · filtered" } else { "" },
                            if self.index.as_ref().is_some_and(|index| index.stale()) {
                                " · retained"
                            } else {
                                ""
                            }
                        )))
                    .highlight_symbol("› ")
                    .highlight_style(style.add_modifier(Modifier::BOLD)),
                columns[0],
                buffer,
                &mut state,
            );
            self.offset = state.offset();
            let mut text = if self.indexed
                && self
                    .index
                    .as_ref()
                    .is_none_or(|index| index.handle.is_none())
                && self.error.is_some()
            {
                String::from("Feature source unavailable")
            } else if self.indexed
                && self.index.as_ref().is_none_or(|index| {
                    index.handle.is_none()
                        || index.refreshing
                        || index.querying
                        || index.loading_page
                })
            {
                String::from("Loading feature source…")
            } else if self.coverage_requested.is_some() {
                String::from("Loading selected feature coverage…")
            } else if self.filtered() {
                String::from("No features match this filter · / changes the filter")
            } else {
                String::from("No features yet · n creates a native feature")
            };
            if let Some(coverage) = &self.coverage {
                let record = &coverage.feature;
                let meta = &record.metadata;
                text = format!(
                    "{}\n{}\nDecision {:?} · Maturity {:?} · Availability {:?}\nDeclared capability; work completion does not promote maturity\n\n{}\n\nIssue coverage\n",
                    meta.name,
                    meta.id,
                    meta.decision,
                    meta.maturity,
                    meta.availability,
                    record.body
                );
                if self.coverage_requested.is_some() {
                    text.insert_str(
                        0,
                        "Loading selected coverage; previous inspected coverage retained\n\n",
                    );
                }
                for issue in &coverage.issues {
                    text.push_str(&format!(
                        "{} · {}\n  {}\n",
                        issue.metadata.status, issue.metadata.title, issue.metadata.id
                    ));
                }
                if let Some(policy) = &self.policy {
                    text.push_str(&format!(
                        "\nNext maturity policy: {} ({:?})\n",
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
                if !coverage.outside_prerequisites.is_empty() {
                    text.push_str("\nPrerequisites outside displayed coverage\n");
                    for issue in &coverage.outside_prerequisites {
                        text.push_str(&format!(
                            "{} · {}\n  {}\n",
                            issue.metadata.status, issue.metadata.title, issue.metadata.id
                        ));
                    }
                }
                for warning in &coverage.warnings {
                    text.push_str(&format!("\nUnresolved: {}", warning.message));
                }
                text.push_str(&format!("\n\nInspected {}", coverage.fingerprint));
            }
            let text = text
                .lines()
                .map(sanitize_terminal_line)
                .collect::<Vec<_>>()
                .join("\n");
            Paragraph::new(text)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Capability and coverage "),
                )
                .wrap(Wrap { trim: false })
                .scroll((self.scroll, 0))
                .render(columns[1], buffer);
        }
        let help = if self.filter_form.is_some() {
            "Tab field · Ctrl-S apply filter · Esc cancel · Ctrl-U clear field"
        } else if self.maturity_draft.is_some() {
            "Tab field · Ctrl-S promote · Esc retain form · Ctrl-D discard"
        } else if self.active.is_some() {
            "Tab field · Ctrl-S save · Esc retain draft · Ctrl-D discard"
        } else {
            "/ filter · t list/tree · Left collapse/parent · Right expand · n create · e edit · m assess/promote · a archive/restore · x active/all · r refresh · PgUp/PgDn scroll · F3 issues"
        };
        let text = self
            .error
            .as_ref()
            .map(|error| format!("{}\n{help}", sanitize_terminal_line(&error.message)))
            .unwrap_or_else(|| help.into());
        Paragraph::new(text)
            .style(style)
            .wrap(Wrap { trim: false })
            .render(rows[1], buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(view: &mut FeatureWorkspace, code: KeyCode) {
        assert!(view.key(KeyEvent::new(code, KeyModifiers::NONE)));
    }
    fn save(view: &mut FeatureWorkspace) {
        assert!(view.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)));
    }
    #[test]
    fn native_feature_drafts_retain_invalid_custom_input_and_save_through_shared_engine() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let mut view = FeatureWorkspace::new(Some(repo.clone()));
        view.open();
        key(&mut view, KeyCode::Char('n'));
        let draft = view.drafts.get_mut("").unwrap();
        draft.form.fields[0].value = "Native capability".into();
        draft.form.fields[1].value = "Scope".into();
        draft.form.fields[5].value = "[invalid".into();
        save(&mut view);
        assert_eq!(
            view.error.as_ref().unwrap().code,
            workdeck_pm::ErrorCode::InvalidInput
        );
        assert!(view.drafts[""].request.is_none());
        assert!(repo.list_features().unwrap().is_empty());
        key(&mut view, KeyCode::Esc);
        key(&mut view, KeyCode::Char('n'));
        assert_eq!(view.drafts[""].form.fields[5].value, "[invalid");
        view.drafts.get_mut("").unwrap().form.fields[5].value =
            r#"{"unknown":{"keep":true}}"#.into();
        save(&mut view);
        assert!(view.error.is_none(), "{:?}", view.error);
        assert!(view.active.is_none());
        let record = repo.list_features().unwrap().remove(0);
        assert_eq!(record.metadata.name, "Native capability");
        assert_eq!(record.body, "Scope");
        assert_eq!(record.metadata.custom["unknown"]["keep"], true);
        key(&mut view, KeyCode::Char('a'));
        assert!(
            repo.feature(record.metadata.id.as_str())
                .unwrap()
                .metadata
                .archived
        );
        key(&mut view, KeyCode::Char('x'));
        key(&mut view, KeyCode::Char('a'));
        assert!(
            !repo
                .feature(record.metadata.id.as_str())
                .unwrap()
                .metadata
                .archived
        );
    }
    #[test]
    fn feature_edit_keeps_original_source_across_refresh_and_requires_discard_to_rebase() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        repo.create_feature(&CreateFeature::new("Initial"), &RequestId::new())
            .unwrap();
        let mut view = FeatureWorkspace::new(Some(repo.clone()));
        view.open();
        key(&mut view, KeyCode::Char('e'));
        let id = view.selected.clone().unwrap();
        let captured = view.drafts[&id].expected.clone();
        repo.mutate_feature(
            &id,
            None,
            &FeatureMutation::Update {
                fields: BTreeMap::from([("name".into(), json!("External edit"))]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
        view.drafts.get_mut(&id).unwrap().form.fields[0].value = "Retained draft".into();
        key(&mut view, KeyCode::Esc);
        key(&mut view, KeyCode::Char('r'));
        key(&mut view, KeyCode::Char('e'));
        assert_eq!(view.drafts[&id].expected, captured);
        save(&mut view);
        assert_eq!(
            view.error.as_ref().unwrap().code,
            workdeck_pm::ErrorCode::StaleSource
        );
        assert_eq!(repo.feature(&id).unwrap().metadata.name, "External edit");
        assert!(view.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)));
        key(&mut view, KeyCode::Char('e'));
        assert_eq!(view.drafts[&id].form.fields[0].value, "External edit");
    }
    #[test]
    fn mounted_feature_access_retains_draft_across_review_and_issues_tabs() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let mut shell = crate::workbench::WorkbenchShell::open(
            crate::workbench::WorkbenchOptions::new(root.path()),
            true,
        );
        assert!(shell.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)));
        assert_eq!(shell.tab, crate::workbench::WorkbenchTab::Features);
        shell.key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        shell.features.paste("Retained feature");
        shell.key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        shell.key(KeyEvent::new(KeyCode::F(11), KeyModifiers::SHIFT));
        assert_eq!(
            shell.features.drafts[""].form.fields[0].value,
            "Retained feature"
        );
        shell.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(shell.features.error.is_none(), "{:?}", shell.features.error);
        assert_eq!(
            repo.list_features().unwrap().remove(0).metadata.name,
            "Retained feature"
        );
        shell.key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE));
        assert_eq!(shell.tab, crate::workbench::WorkbenchTab::Issues);
    }
    #[test]
    fn feature_refresh_rejects_a_selected_source_race_without_rebasing_retained_drafts() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let created: FeatureOutcome = serde_json::from_value(
            repo.create_feature(&CreateFeature::new("Inspected feature"), &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        repo.create_feature(&CreateFeature::new("Other feature"), &RequestId::new())
            .unwrap();
        let mut view = FeatureWorkspace::new(Some(repo.clone()));
        let id = created.record.metadata.id.to_string();
        view.selected = Some(id.clone());
        view.open();
        key(&mut view, KeyCode::Char('e'));
        view.drafts.get_mut(&id).unwrap().form.fields[0].value = "Retained authored draft".into();
        key(&mut view, KeyCode::Esc);
        let records = view.records.clone();
        let coverage = view.coverage.clone();
        let expected = view.drafts[&id].expected.clone();
        let result = view.refresh_after_list(|| {
            repo.mutate_feature(
                &id,
                Some(&created.record.source),
                &FeatureMutation::Archive { archived: true },
                &RequestId::new(),
            )
            .unwrap();
        });
        assert_eq!(
            result.unwrap_err().code,
            workdeck_pm::ErrorCode::StaleSource,
            "refresh must not publish an archived detail into its earlier active list"
        );
        assert_eq!(view.selected.as_deref(), Some(id.as_str()));
        assert_eq!(view.records, records);
        assert_eq!(view.coverage, coverage);
        assert_eq!(view.drafts[&id].expected, expected);
        assert_eq!(
            view.drafts[&id].form.fields[0].value,
            "Retained authored draft"
        );
        view.refresh().unwrap();
        assert!(
            !view
                .records
                .iter()
                .any(|record| record.metadata.id.as_str() == id)
        );
        assert_eq!(view.drafts[&id].expected, expected);
    }
    #[test]
    fn feature_create_retry_after_lost_acknowledgement_retains_original_request() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let mut view = FeatureWorkspace::new(Some(repo.clone()));
        view.open();
        key(&mut view, KeyCode::Char('n'));
        let request = RequestId::new();
        let draft = view.drafts.get_mut("").unwrap();
        draft.form.fields[0].value = "Original creation".into();
        draft.request = Some(request.clone());
        // The engine published successfully, but the UI retained its draft after
        // losing the acknowledgement. Retrying changed input cannot create twice.
        let receipt = repo
            .create_feature(
                &CreateFeature {
                    name: "Original creation".into(),
                    body: "".into(),
                    directory: None,
                    fields: BTreeMap::from([
                        ("decision".into(), json!("proposed")),
                        ("maturity".into(), json!("draft")),
                        ("availability".into(), json!("unavailable")),
                        ("custom".into(), json!({})),
                    ]),
                },
                &request,
            )
            .unwrap();
        draft.form.fields[0].value = "Edited after lost acknowledgement".into();
        assert_eq!(
            view.submit().unwrap_err().code,
            workdeck_pm::ErrorCode::IdempotencyConflict
        );
        assert_eq!(view.drafts[""].request.as_ref(), Some(&request));
        assert_eq!(repo.list_features().unwrap().len(), 1);
        view.drafts.get_mut("").unwrap().form.fields[0].value = "Original creation".into();
        view.submit().unwrap();
        assert_eq!(view.last_receipt, Some(receipt));
        assert!(view.active.is_none());
        assert!(view.drafts.is_empty());
        assert_eq!(repo.list_features().unwrap().len(), 1);
    }

    #[test]
    fn feature_archive_retry_after_lost_acknowledgement_does_not_invert_the_saved_action() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let outcome: FeatureOutcome = serde_json::from_value(
            repo.create_feature(&CreateFeature::new("Archive once"), &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        let id = outcome.record.metadata.id.to_string();
        let mut view = FeatureWorkspace::new(Some(repo.clone()));
        view.include_archived = true;
        view.open();
        let request = RequestId::new();
        view.archive = Some(ArchiveAttempt {
            id: id.clone(),
            expected: outcome.record.source.clone(),
            archived: true,
            request: request.clone(),
        });
        let receipt = repo
            .mutate_feature(
                &id,
                Some(&outcome.record.source),
                &FeatureMutation::Archive { archived: true },
                &request,
            )
            .unwrap();
        let archived = repo.feature(&id).unwrap();
        view.refresh().unwrap();
        view.archive().unwrap();
        assert_eq!(view.last_receipt, Some(receipt));
        assert!(view.archive.is_none());
        assert_eq!(repo.feature(&id).unwrap(), archived);
    }

    #[test]
    fn feature_workspace_never_rebinds_retained_drafts_after_repository_identity_replacement() {
        let root = tempfile::tempdir().unwrap();
        let repo = Repository::init(root.path(), "WD").unwrap();
        let outcome: FeatureOutcome = serde_json::from_value(
            repo.create_feature(
                &CreateFeature::new("Original repository"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
        )
        .unwrap();
        let id = outcome.record.metadata.id.to_string();
        let mut view = FeatureWorkspace::new(Some(repo.clone()));
        view.open();
        key(&mut view, KeyCode::Char('e'));
        view.drafts.get_mut(&id).unwrap().form.fields[0].value = "Retained edit".into();
        let expected = view.drafts[&id].expected.clone();
        let records = view.records.clone();
        let coverage = view.coverage.clone();
        let source_path = repo.root().join(&outcome.record.path);
        let original_source = std::fs::read(&source_path).unwrap();
        let config_path = repo.root().join("config.yml");
        let config = std::fs::read_to_string(&config_path).unwrap();
        let other_root = tempfile::tempdir().unwrap();
        let other = Repository::init(other_root.path(), "WD").unwrap();
        std::fs::write(
            &config_path,
            config.replace(repo.identity().as_str(), other.identity().as_str()),
        )
        .unwrap();
        view.bind_if_missing(Some(other.clone()));
        assert_eq!(view.repository().unwrap().identity(), repo.identity());
        assert_eq!(
            view.refresh().unwrap_err().code,
            workdeck_pm::ErrorCode::StaleSource
        );
        assert_eq!(
            view.submit().unwrap_err().code,
            workdeck_pm::ErrorCode::StaleSource
        );
        assert_eq!(view.records, records);
        assert_eq!(view.coverage, coverage);
        assert_eq!(view.drafts[&id].expected, expected);
        assert_eq!(view.drafts[&id].form.fields[0].value, "Retained edit");
        assert_eq!(std::fs::read(&source_path).unwrap(), original_source);
        assert!(other.list_features().unwrap().is_empty());
        std::fs::write(config_path, config).unwrap();
        view.submit().unwrap();
        assert_eq!(repo.feature(&id).unwrap().metadata.name, "Retained edit");
        assert!(other.list_features().unwrap().is_empty());
    }
    #[test]
    fn mounted_feature_collection_does_not_retain_every_native_document() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        for name in ["First capability", "Second capability", "Third capability"] {
            repository
                .create_feature(&CreateFeature::new(name), &RequestId::new())
                .unwrap();
        }
        let mut shell = crate::workbench::WorkbenchShell::open(
            crate::workbench::WorkbenchOptions::new(directory.path()),
            true,
        );
        shell.key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE));
        assert_eq!(shell.tab, crate::workbench::WorkbenchTab::Features);
        crate::workbench::test_pump::settle_shell(&mut shell);
        assert!(
            shell
                .features
                .index
                .as_ref()
                .is_some_and(|index| index.handle.is_some()),
            "feature reader failed: {:?}",
            shell.features
        );
        assert_eq!(
            shell
                .features
                .index
                .as_ref()
                .unwrap()
                .handle
                .as_ref()
                .unwrap()
                .total,
            3
        );
        let first = shell.features.selected.clone();
        shell.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        crate::workbench::test_pump::settle_shell(&mut shell);
        assert_ne!(shell.features.selected, first);
        assert_eq!(
            shell.features.selected.as_deref(),
            shell
                .features
                .index
                .as_ref()
                .unwrap()
                .selected_row()
                .map(|row| row.token.key.id.as_str())
        );
        assert_eq!(shell.features.records.len(), 1);
        assert!(
            shell.features.records.len() <= 1,
            "mounted collection retained {} full native documents; only the selected authoring record belongs in this controller",
            shell.features.records.len()
        );
    }

    #[test]
    fn indexed_feature_selection_defers_coverage_until_worker_completion() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        repository
            .create_feature(
                &CreateFeature::new("Background coverage"),
                &RequestId::new(),
            )
            .unwrap();
        let mut workspace = FeatureWorkspace::new_indexed(Some(repository));
        workspace.refresh().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let index = workspace.index.as_mut().unwrap();
            index.poll();
            if index.is_idle() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        workspace.select_index_record().unwrap();
        assert!(
            workspace.coverage.is_none(),
            "selection must enqueue coverage instead of evaluating it on the UI thread"
        );
        while workspace.coverage.is_none() {
            workspace.poll_index();
            assert!(
                std::time::Instant::now() < deadline,
                "background coverage did not settle: {:?}",
                workspace.error
            );
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert_eq!(
            workspace.coverage.as_ref().unwrap().feature.metadata.name,
            "Background coverage"
        );
    }

    #[test]
    fn maturity_control_uses_source_bound_acceptance_and_preserves_receipts() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        let outcome: FeatureOutcome = serde_json::from_value(
            repository
                .create_feature(
                    &CreateFeature {
                        name: "Policy capability".into(),
                        body: "Capability body".into(),
                        fields: BTreeMap::from([
                            ("decision".into(), json!("accepted")),
                            (
                                "criteria".into(),
                                json!([{"id":"works","description":"The capability works"}]),
                            ),
                        ]),
                        directory: None,
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
                        title: "Implement capability".into(),
                        body: "Implementation".into(),
                        fields: BTreeMap::from([(
                            "features".into(),
                            json!([outcome.record.metadata.id.clone()]),
                        )]),
                    },
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        let mut workspace = FeatureWorkspace::new(Some(repository.clone()));
        workspace.open();
        workspace.key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        assert!(workspace.policy.is_some());
        assert_eq!(
            workspace.maturity_draft.as_ref().unwrap().expected,
            outcome.record.source
        );
        let draft = workspace.maturity_draft.as_mut().unwrap();
        draft.form.fields[1].value = "local".into();
        draft.form.fields[2].value = "Accepted the specified capability".into();
        workspace.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(workspace.error.is_none(), "{:?}", workspace.error);
        assert!(workspace.maturity_draft.is_none());
        assert_eq!(
            repository
                .feature(outcome.record.metadata.id.as_str())
                .unwrap()
                .metadata
                .maturity,
            FeatureMaturity::Specified
        );
        repository
            .complete_issue(
                issue.metadata.id.as_str(),
                &issue.source,
                None,
                &RequestId::new(),
            )
            .unwrap();
        workspace.key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE));
        let draft = workspace.maturity_draft.as_mut().unwrap();
        draft.form.fields[1].value = "local".into();
        draft.form.fields[2].value = "Accepted the implemented capability".into();
        workspace.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(workspace.error.is_none(), "{:?}", workspace.error);
        assert!(workspace.last_receipt.is_some());
        assert_eq!(
            repository
                .feature(outcome.record.metadata.id.as_str())
                .unwrap()
                .metadata
                .maturity,
            FeatureMaturity::Implemented
        );
    }
}
