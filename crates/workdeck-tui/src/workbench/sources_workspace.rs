//! Read-only planning views retain immutable source identity across selection and refresh.
use super::input::{FormAction, FormKind, TextField, WorkbenchForm};
use crossterm::event::{KeyCode, KeyEvent};
use std::{collections::BTreeMap, path::PathBuf};
use workdeck_pm::*;

const MAX_SLOTS: usize = 8;
const MAX_RETAINED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub(super) struct SourceIssue {
    pub id: IssueId,
    pub title: String,
    pub status: String,
}
#[derive(Debug, Clone)]
pub(super) struct OpenedSource {
    pub observation: SourceObservation,
    pub issue: IssueRecord,
    pub document: String,
    pub contract: Result<ClaimWorkContract>,
    pub binding: Option<ContentHash>,
}
#[derive(Debug)]
pub(super) struct SourceState {
    pub view: PlanningSourceView,
    pub issues: Vec<SourceIssue>,
    pub selected: Option<IssueId>,
    pub opened: Option<OpenedSource>,
    pub scroll: u16,
    pub offset: usize,
}
#[derive(Debug)]
pub(super) struct SourcesWorkspace {
    pub root: PathBuf,
    repository: Option<RepositoryId>,
    pub selector: SourceSelector,
    pub slots: BTreeMap<String, SourceState>,
    pub error: Option<PmError>,
    pub proposal_draft: String,
    pub proposal_form: Option<WorkbenchForm>,
}
impl SourcesWorkspace {
    pub fn new(root: PathBuf, repository: Option<RepositoryId>) -> Self {
        Self {
            root,
            repository,
            selector: SourceSelector::WorkingTree,
            slots: BTreeMap::new(),
            error: None,
            proposal_draft: "refs/heads/workdeck-proposals/".into(),
            proposal_form: None,
        }
    }
    fn key_for(selector: &SourceSelector) -> String {
        serde_json::to_string(selector).expect("typed source selector serializes")
    }
    fn slot_key(&self) -> String {
        Self::key_for(&self.selector)
    }
    pub fn state(&self) -> Option<&SourceState> {
        self.slots.get(&self.slot_key())
    }
    pub fn state_mut(&mut self) -> Option<&mut SourceState> {
        let key = self.slot_key();
        self.slots.get_mut(&key)
    }
    pub fn open(&mut self) {
        if self.state().is_none()
            && let Err(error) = self.refresh()
        {
            self.error = Some(error);
        }
    }
    pub fn select(&mut self, selector: SourceSelector) {
        self.selector = selector;
        self.error = None;
        self.open();
    }
    pub fn refresh(&mut self) -> Result<()> {
        let key = self.slot_key();
        if !self.slots.contains_key(&key) && self.slots.len() >= MAX_SLOTS {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "Eight captured source slots are retained; close this workbench to release them before opening another source.",
            ));
        }
        let limits = SourceCaptureLimits {
            max_total_bytes: 32 * 1024 * 1024,
            ..SourceCaptureLimits::default()
        };
        let view = sources::capture(&self.root, &self.selector, &limits)?;
        if self
            .repository
            .as_ref()
            .is_some_and(|repository| repository != &view.observation.identity.repository)
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Planning repository identity changed; prior source slots and drafts were retained.",
            ));
        }
        let other_bytes: usize = self
            .slots
            .iter()
            .filter(|(slot, _)| *slot != &key)
            .flat_map(|(_, state)| state.view.snapshot.files().values())
            .map(Vec::len)
            .sum();
        let new_bytes: usize = view.snapshot.files().values().map(Vec::len).sum();
        if other_bytes.saturating_add(new_bytes) > MAX_RETAINED_BYTES {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "Captured planning sources exceed the workbench's 64 MiB retained-source budget; existing views were preserved.",
            ));
        }
        let issues = view
            .snapshot
            .query_issues(&IssueQuery::all())?
            .into_iter()
            .map(|record| SourceIssue {
                id: record.metadata.id,
                title: record.metadata.title,
                status: record.metadata.status,
            })
            .collect::<Vec<_>>();
        view.revalidate()?;
        self.repository
            .get_or_insert_with(|| view.observation.identity.repository.clone());
        let previous = self.slots.remove(&key);
        let selected = previous
            .as_ref()
            .and_then(|state| state.selected.clone())
            .filter(|id| issues.iter().any(|issue| &issue.id == id))
            .or_else(|| issues.first().map(|issue| issue.id.clone()));
        let opened = previous.as_ref().and_then(|state| state.opened.clone());
        let scroll = previous.as_ref().map_or(0, |state| state.scroll);
        let offset = previous.as_ref().map_or(0, |state| state.offset);
        self.slots.insert(
            key,
            SourceState {
                view,
                issues,
                selected,
                opened,
                scroll,
                offset,
            },
        );
        self.error = None;
        Ok(())
    }
    pub fn open_selected(&mut self) -> Result<()> {
        let state = self.state_mut().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "Capture a source before opening an issue",
            )
        })?;
        let id = state.selected.as_ref().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "This source contains no selected issue",
            )
        })?;
        let issue = state.view.snapshot.show_issue(id.as_str())?;
        let bytes = state
            .view
            .snapshot
            .files()
            .get(&issue.path)
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::CorruptStore,
                    "Captured issue document is missing from its source",
                )
            })?;
        if ContentHash::of(bytes) != issue.source.content {
            return Err(PmError::new(
                ErrorCode::CorruptStore,
                "Captured issue citation differs from its source identity",
            ));
        }
        let document = String::from_utf8(bytes.clone()).map_err(|_| {
            PmError::new(
                ErrorCode::CorruptStore,
                "Captured issue document is not UTF-8",
            )
        })?;
        let contract = state.view.snapshot.claim_contract(&issue.metadata.id);
        state.opened = Some(OpenedSource {
            observation: state.view.observation.clone(),
            issue,
            document,
            contract,
            binding: state.view.publication_binding().cloned(),
        });
        state.scroll = 0;
        Ok(())
    }
    pub fn selected_contract(&self) -> Result<ClaimWorkContract> {
        let state = self
            .state()
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "Capture a planning source first"))?;
        let issue = state.selected.as_ref().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "Select an issue to inspect its claim contract",
            )
        })?;
        if let Some(opened) = &state.opened
            && &opened.issue.metadata.id == issue
        {
            return opened.contract.clone();
        }
        state.view.snapshot.claim_contract(issue)
    }
    pub fn selected_claim(&self) -> Result<(ClaimWorkContract, Option<ContentHash>)> {
        let contract = self.selected_contract()?;
        let state = self.state().expect("selected contract has captured state");
        let binding = match &state.opened {
            Some(opened) if Some(&opened.issue.metadata.id) == state.selected.as_ref() => {
                opened.binding.clone()
            }
            _ => state.view.publication_binding().cloned(),
        };
        Ok((contract, binding))
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if let Some(form) = &mut self.proposal_form {
            form.fields[0].insert(text);
        }
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if let Some(form) = &mut self.proposal_form {
            let action = form.key(key);
            self.proposal_draft = form.fields[0].value.clone();
            match action {
                FormAction::Submit => match self.proposal_draft.parse() {
                    Ok(reference) => {
                        self.proposal_form = None;
                        self.select(SourceSelector::Proposal { reference });
                    }
                    Err(error) => self.error = Some(error),
                },
                FormAction::Close => self.proposal_form = None,
                FormAction::Edited => {}
            }
            return true;
        }
        if !key.modifiers.is_empty() {
            return false;
        }
        let result = match key.code {
            KeyCode::Char('l') => {
                self.select(SourceSelector::WorkingTree);
                Ok(())
            }
            KeyCode::Char('a') => {
                self.select(SourceSelector::Accepted);
                Ok(())
            }
            KeyCode::Char('c') => {
                self.select(SourceSelector::Coordination);
                Ok(())
            }
            KeyCode::Char('p') => {
                self.proposal_form = Some(WorkbenchForm::new(
                    FormKind::Planning,
                    "Inspect proposal source",
                    vec![TextField::new(
                        "Full Git ref",
                        self.proposal_draft.clone(),
                        false,
                    )],
                ));
                Ok(())
            }
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Enter => self.open_selected(),
            KeyCode::Esc if self.state().is_some_and(|state| state.opened.is_some()) => {
                if let Some(state) = self.state_mut() {
                    state.opened = None;
                }
                Ok(())
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                if let Some(state) = self.state_mut() {
                    let index = state
                        .issues
                        .iter()
                        .position(|issue| Some(&issue.id) == state.selected.as_ref())
                        .unwrap_or(0);
                    let index = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                        index.saturating_sub(1)
                    } else {
                        index
                            .saturating_add(1)
                            .min(state.issues.len().saturating_sub(1))
                    };
                    state.selected = state.issues.get(index).map(|issue| issue.id.clone());
                }
                Ok(())
            }
            KeyCode::PageDown | KeyCode::PageUp => {
                if let Some(state) = self.state_mut() {
                    state.scroll = if key.code == KeyCode::PageDown {
                        state.scroll.saturating_add(10)
                    } else {
                        state.scroll.saturating_sub(10)
                    };
                }
                Ok(())
            }
            _ => return false,
        };
        if let Err(error) = result {
            self.error = Some(error);
        }
        true
    }
}
