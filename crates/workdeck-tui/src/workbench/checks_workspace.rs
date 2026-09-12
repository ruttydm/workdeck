//! Source-bound catalog, plan and result state. Execution stays in the shared runner.
use super::owned_run::{ForegroundRunSignal, OwnedRun};
use crossterm::event::{KeyCode, KeyEvent};
use std::{collections::BTreeMap, sync::Arc};
use workdeck_pm::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ChecksPage {
    #[default]
    Definitions,
    Plan,
    Results,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ResultFilter {
    #[default]
    All,
    Attention,
    Passed,
}
impl ResultFilter {
    pub fn contains(self, state: RunState) -> bool {
        match self {
            Self::All => true,
            Self::Passed => state == RunState::Passed,
            Self::Attention => !matches!(state, RunState::Passed | RunState::Skipped),
        }
    }
    fn next(self) -> Self {
        match self {
            Self::All => Self::Attention,
            Self::Attention => Self::Passed,
            Self::Passed => Self::All,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CheckTarget {
    Profile(String),
    Check(String),
    Command(String),
    Run(LocalRunId),
    Result(LocalRunId, String),
}

#[derive(Debug, Clone)]
pub(super) struct CheckRow {
    pub id: String,
    pub title: String,
    pub detail: String,
    pub target: Option<CheckTarget>,
}

#[derive(Debug)]
pub(super) struct InspectedPlan {
    pub plan: CheckPlan,
    pub request: Option<RequestId>,
}

#[derive(Debug, Default)]
pub(super) struct CheckState {
    pub catalog: Option<CommandCatalogSnapshot>,
    pub plan: Option<InspectedPlan>,
    pub page: ChecksPage,
    pub selected: [Option<String>; 3],
    pub offset: [usize; 3],
    pub scroll: [u16; 3],
    pub runs: Vec<RunOutcome>,
    pub filter: ResultFilter,
    pub error: Option<PmError>,
    pub notice: Option<String>,
}

#[derive(Debug)]
pub(super) struct Completion {
    key: String,
    request: RequestId,
    result: Result<RunOutcome>,
}

#[derive(Debug)]
pub(super) struct ChecksWorkspace {
    repository: Option<Repository>,
    pub author: String,
    pub current: Option<IssueId>,
    pub states: BTreeMap<String, CheckState>,
    pub signal: Arc<ForegroundRunSignal>,
    worker: Option<OwnedRun<Completion>>,
    pub running_key: Option<String>,
}

impl ChecksWorkspace {
    pub fn new(repository: Option<Repository>, author: String) -> Self {
        Self {
            repository,
            author,
            current: None,
            states: BTreeMap::new(),
            signal: Arc::default(),
            worker: None,
            running_key: None,
        }
    }
    pub fn attach_signal(&mut self, signal: Option<Arc<ForegroundRunSignal>>) {
        if self.worker.is_none()
            && let Some(signal) = signal
        {
            self.signal = signal;
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
                "Run workdeck init or finish migration, then refresh check definitions",
            )
        })
    }
    fn state_key(&self) -> String {
        self.current
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    }
    pub fn state(&self) -> Option<&CheckState> {
        self.states.get(&self.state_key())
    }
    pub fn state_mut(&mut self) -> &mut CheckState {
        self.states.entry(self.state_key()).or_default()
    }
    pub fn open(&mut self, issue: Option<IssueId>) {
        self.current = issue;
        if self.state().is_none_or(|state| state.catalog.is_none())
            && let Err(error) = self.refresh()
        {
            self.state_mut().error = Some(error);
        }
    }
    pub fn refresh(&mut self) -> Result<()> {
        let repository = self.repository()?;
        let catalog = repository.command_catalog()?;
        if catalog.repository != *repository.identity() {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Check definitions belong to another repository",
            ));
        }
        let runs = repository.check_results(&RunQuery {
            issue: self.current.clone(),
            state: None,
            limit: Some(100),
        })?;
        let state = self.state_mut();
        state.catalog = Some(catalog);
        state.runs = runs;
        state.error = None;
        // Refresh deliberately does not replace an inspected plan or its request.
        self.keep_selection();
        Ok(())
    }
    pub fn open_result(
        &mut self,
        id: &LocalRunId,
        intent: &SourcePin,
        result: Option<&SourcePin>,
    ) -> Result<()> {
        let outcome = self.repository()?.check_status(id)?;
        if outcome.run.path != intent.path
            || outcome.run.content != intent.content
            || outcome
                .results
                .as_ref()
                .map(|record| SourcePin {
                    path: record.path.clone(),
                    content: record.content.clone(),
                })
                .as_ref()
                != result
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Check result changed since context inspection",
            ));
        }
        let state = self.state_mut();
        state.runs.retain(|run| run.run.intent.id != *id);
        state.runs.insert(0, outcome);
        state.page = ChecksPage::Results;
        state.filter = ResultFilter::All;
        state.selected[2] = Some(format!("run:{id}"));
        state.error = None;
        Ok(())
    }
    pub fn plan_selected(&mut self) -> Result<()> {
        if self
            .state()
            .and_then(|state| state.plan.as_ref())
            .is_some_and(|plan| plan.request.is_some())
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "This plan has an attempted run; retry its original request or press D to discard the inspected plan before planning again",
            ));
        }
        let target = self
            .selected_row()
            .and_then(|row| row.target)
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::NotFound,
                    "Select a profile, check or command definition first",
                )
            })?;
        let repository = self.repository()?;
        let mut request = CheckPlanRequest {
            issue: self.current.as_ref().map(ToString::to_string),
            ..Default::default()
        };
        let plan = match target {
            CheckTarget::Profile(id) => {
                request.profiles.push(id);
                repository.check_plan(&request)?
            }
            CheckTarget::Check(id) => {
                request.checks.push(id);
                repository.check_plan(&request)?
            }
            CheckTarget::Command(command) => repository.command_plan(&CommandPlanRequest {
                command,
                arguments: BTreeMap::new(),
            })?,
            _ => {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "Open definitions before building a new plan",
                ));
            }
        };
        if plan.repository != *repository.identity() {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Check plan belongs to another repository",
            ));
        }
        if self
            .state()
            .and_then(|state| state.catalog.as_ref())
            .is_none_or(|catalog| catalog.fingerprint != plan.definitions.fingerprint)
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Check definitions changed since inspection; refresh definitions before building a plan",
            ));
        }
        let state = self.state_mut();
        state.plan = Some(InspectedPlan {
            plan,
            request: None,
        });
        state.page = ChecksPage::Plan;
        state.error = None;
        state.notice = Some(
            "Plan inspected. Press x explicitly to execute; discovery never starts a process."
                .into(),
        );
        self.keep_selection();
        Ok(())
    }
    pub fn begin_run(&mut self) -> Result<()> {
        self.begin_run_using(|repository, input, request, control| {
            repository.run_check_plan(&input, &request, &control)
        })
    }
    pub(super) fn begin_run_using(
        &mut self,
        execute: impl FnOnce(Repository, CheckRunRequest, RequestId, RunControl) -> Result<RunOutcome>
        + Send
        + 'static,
    ) -> Result<()> {
        if self.worker.is_some() || self.signal.is_active() {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "A foreground run is active or its cleanup acknowledgement is missing",
            ));
        }
        let repository = self.repository()?.clone();
        let author = self.author.clone();
        let key = self.state_key();
        let inspected = self.state_mut().plan.as_mut().ok_or_else(|| {
            PmError::new(
                ErrorCode::InvalidInput,
                "Inspect a plan with p before running checks",
            )
        })?;
        let request = inspected.request.get_or_insert_with(RequestId::new).clone();
        let input = CheckRunRequest {
            expected_plan: inspected.plan.fingerprint.clone(),
            plan: inspected.plan.clone(),
            actor: author,
        };
        let completion_key = key.clone();
        self.worker = Some(
            OwnedRun::start(self.signal.clone(), move |control| Completion {
                key: completion_key,
                request: request.clone(),
                result: execute(repository, input, request, control),
            })
            .map_err(|error| PmError::new(ErrorCode::Io, error.to_string()))?,
        );
        self.running_key = Some(key);
        self.state_mut().notice =
            Some("Running inspected plan · Ctrl-C cancels · repeated Ctrl-C forces cleanup".into());
        self.state_mut().error = None;
        Ok(())
    }
    pub fn poll(&mut self) {
        let result = self.worker.as_mut().and_then(OwnedRun::poll);
        if let Some(result) = result {
            self.complete(result);
        }
    }
    fn complete(&mut self, result: std::thread::Result<Completion>) {
        let cleaned = self.worker.as_ref().is_some_and(OwnedRun::cleanup_complete);
        self.worker = None;
        let key = self.running_key.take().unwrap_or_default();
        match result {
            Ok(completion) => {
                let state = self.states.entry(completion.key).or_default();
                match completion.result {
                    Ok(outcome) => {
                        state
                            .runs
                            .retain(|run| run.run.intent.id != outcome.run.intent.id);
                        state.selected[2] = Some(format!("run:{}", outcome.run.intent.id));
                        state.notice = Some(format!(
                            "Run {} · request {} · {:?} · local feedback",
                            outcome.run.intent.id, completion.request, outcome.assessment.state
                        ));
                        state.runs.insert(0, outcome);
                        state.page = ChecksPage::Results;
                        state.error = None;
                    }
                    Err(error) => {
                        state.error = Some(error);
                        state.notice = Some(format!(
                            "Original request {} retained. x retries publication; it never silently starts another invocation.",
                            completion.request
                        ));
                    }
                }
            }
            Err(_) => {
                self.states.entry(key.clone()).or_default().error = Some(PmError::new(
                    ErrorCode::Io,
                    "Foreground runner worker panicked; inspect its durable run status before continuing",
                ))
            }
        }
        if !cleaned {
            self.states.entry(key).or_default().error = Some(PmError::new(
                ErrorCode::Io,
                "Runner did not acknowledge process cleanup; another run is blocked",
            ));
        }
        self.keep_selection();
    }
    pub fn take_shutdown_task(&mut self) -> Option<OwnedRun<Completion>> {
        self.worker.take()
    }
    pub fn finish_shutdown(
        &mut self,
        task: OwnedRun<Completion>,
        result: std::thread::Result<Completion>,
    ) {
        self.worker = Some(task);
        self.complete(result);
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if !key.modifiers.is_empty() {
            return false;
        }
        let result = match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return false,
            KeyCode::Char('p') => self.plan_selected(),
            KeyCode::Char('x') => self.begin_run(),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('d') => {
                self.state_mut().page = ChecksPage::Definitions;
                self.keep_selection();
                Ok(())
            }
            KeyCode::Char('v') => {
                self.state_mut().page = ChecksPage::Results;
                self.keep_selection();
                Ok(())
            }
            KeyCode::Char('b') => {
                self.state_mut().page = ChecksPage::Plan;
                self.keep_selection();
                Ok(())
            }
            KeyCode::Char('f') => {
                let state = self.state_mut();
                state.filter = state.filter.next();
                self.keep_selection();
                Ok(())
            }
            KeyCode::Char('D') if self.running_key.as_ref() != Some(&self.state_key()) => {
                self.state_mut().plan = None;
                self.state_mut().notice = Some(
                    "Inspected plan discarded. Existing run receipts remain available.".into(),
                );
                Ok(())
            }
            KeyCode::Enter | KeyCode::Char('e') => self.explain_selected(),
            KeyCode::Up | KeyCode::Down | KeyCode::Char('j' | 'k') => {
                let rows = self.rows();
                let state = self.state_mut();
                let index = state.page as usize;
                let at = rows
                    .iter()
                    .position(|row| Some(&row.id) == state.selected[index].as_ref())
                    .unwrap_or_default();
                let next = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                    at.saturating_sub(1)
                } else {
                    at.saturating_add(1).min(rows.len().saturating_sub(1))
                };
                state.selected[index] = rows.get(next).map(|row| row.id.clone());
                state.scroll[index] = 0;
                Ok(())
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                let state = self.state_mut();
                let scroll = &mut state.scroll[state.page as usize];
                *scroll = if key.code == KeyCode::PageUp {
                    scroll.saturating_sub(10)
                } else {
                    scroll.saturating_add(10)
                };
                Ok(())
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.state_mut().error = Some(error);
        }
        true
    }
    fn explain_selected(&mut self) -> Result<()> {
        let target = self.selected_row().and_then(|row| row.target);
        if let Some(CheckTarget::Run(id) | CheckTarget::Result(id, _)) = target {
            let fresh = self.repository()?.check_status(&id)?;
            let state = self.state_mut();
            if let Some(run) = state.runs.iter_mut().find(|run| run.run.intent.id == id) {
                *run = fresh;
            }
            state.error = None;
        }
        Ok(())
    }
    pub fn keep_selection(&mut self) {
        let rows = self.rows();
        let state = self.state_mut();
        let index = state.page as usize;
        if !rows
            .iter()
            .any(|row| Some(&row.id) == state.selected[index].as_ref())
        {
            state.selected[index] = rows.first().map(|row| row.id.clone());
            state.scroll[index] = 0;
        }
    }
    pub fn selected_row(&self) -> Option<CheckRow> {
        let state = self.state()?;
        let rows = self.rows();
        rows.iter()
            .find(|row| Some(&row.id) == state.selected[state.page as usize].as_ref())
            .or_else(|| rows.first())
            .cloned()
    }
}
