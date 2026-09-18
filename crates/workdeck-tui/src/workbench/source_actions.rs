//! Explicit source operations retain the inspected plan and original request.
use super::{
    ForegroundRunSignal,
    input::{FormAction, FormKind, TextField, WorkbenchForm},
    owned_publication::OwnedPublication,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::sync::Arc;
use workdeck_pm::{
    sources::{ProposalOutcome, ProposalPlan, ProposalRequest},
    *,
};

#[derive(Debug, Clone)]
pub(super) enum ReviewedSourceAction {
    Refresh {
        sync: bool,
        input: SourceFetchRequest,
        source: Box<SourceObservation>,
        shared: SharedSources,
        errors: Vec<PmError>,
    },
    Proposal(Box<ProposalPlan>),
}
#[derive(Debug)]
pub(super) enum SourceActionOutcome {
    Refresh(SourceFetchOutcome),
    Proposal(Box<ProposalOutcome>),
}
#[derive(Debug, Clone, Copy)]
enum FormPurpose {
    Proposal,
    Request,
}
#[derive(Debug)]
struct SourceForm {
    purpose: FormPurpose,
    form: WorkbenchForm,
}
#[derive(Debug)]
pub(super) struct SourceActions {
    repository: Option<Repository>,
    pub visible: bool,
    pub review: Option<ReviewedSourceAction>,
    pub request: Option<RequestId>,
    pub outcome: Option<SourceActionOutcome>,
    pub error: Option<PmError>,
    pub notice: Option<String>,
    pub scroll: u16,
    pub page: usize,
    form: Option<SourceForm>,
    pub signal: Arc<ForegroundRunSignal>,
    worker: Option<OwnedPublication<Result<SourceActionOutcome>>>,
}
impl SourceActions {
    pub fn new(repository: Option<Repository>, signal: Arc<ForegroundRunSignal>) -> Self {
        Self {
            repository,
            visible: false,
            review: None,
            request: None,
            outcome: None,
            error: None,
            notice: None,
            scroll: 0,
            page: 0,
            form: None,
            signal,
            worker: None,
        }
    }
    pub fn bind_if_missing(&mut self, repository: Option<Repository>) {
        if self.repository.is_none() {
            self.repository = repository;
        }
    }
    pub fn attach_signal(&mut self, signal: Arc<ForegroundRunSignal>) {
        if !self.busy() {
            self.signal = signal;
        }
    }
    fn repository(&self) -> Result<&Repository> {
        self.repository.as_ref().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotInitialized,
                "Finish workdeck init or migration before explicit source operations",
            )
        })
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn form(&self) -> Option<&WorkbenchForm> {
        self.form.as_ref().map(|form| &form.form)
    }
    fn allow_new_review(&self) -> Result<()> {
        if self.busy() {
            return Err(PmError::new(
                ErrorCode::Locked,
                "A source operation is pending; its original request remains owned",
            ));
        }
        if self.request.is_some() {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "An original source request is retained. Inspect or retry it, or explicitly Ctrl-D discard before reviewing another intent.",
            ));
        }
        Ok(())
    }
    pub fn inspect_refresh(&mut self, sync: bool) -> Result<()> {
        self.allow_new_review()?;
        let repository = self.repository()?;
        let status = repository.source_status()?;
        if &status.repository != repository.identity()
            || &status.working.identity.repository != repository.identity()
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Source operation belongs to a replaced repository",
            ));
        }
        let shared = status.shared.ok_or_else(|| {
            PmError::new(
                ErrorCode::PolicyBlocked,
                "Configure shared planning sources before fetching or syncing",
            )
        })?;
        let input = SourceFetchRequest {
            expected_config: status.config,
            expected_binding: status.binding.ok_or_else(|| {
                PmError::new(
                    ErrorCode::StaleSource,
                    "Source publication binding is unavailable; inspect a new source status",
                )
            })?,
        };
        self.review = Some(ReviewedSourceAction::Refresh {
            sync,
            input,
            source: Box::new(status.working),
            shared,
            errors: status.errors,
        });
        self.error = None;
        self.notice = None;
        self.scroll = 0;
        self.page = 0;
        Ok(())
    }
    pub fn begin_proposal(&mut self) -> Result<()> {
        self.allow_new_review()?;
        let config = self.repository()?.config()?;
        let shared = config.sources.ok_or_else(|| {
            PmError::new(
                ErrorCode::PolicyBlocked,
                "Configure shared planning sources before proposing changes",
            )
        })?;
        let mut form = WorkbenchForm::new(
            FormKind::Planning,
            "Preview planning proposal",
            vec![
                TextField::new(
                    "Full proposal ref",
                    format!("{}/", shared.proposal_namespace),
                    false,
                ),
                TextField::new("Title", String::new(), false),
            ],
        );
        form.help.push(
            "Ctrl-S inspects a plan without publishing. x separately publishes the reviewed plan."
                .into(),
        );
        self.form = Some(SourceForm {
            purpose: FormPurpose::Proposal,
            form,
        });
        Ok(())
    }
    pub fn begin_request(&mut self) -> Result<()> {
        self.allow_new_review()?;
        self.form = Some(SourceForm {
            purpose: FormPurpose::Request,
            form: WorkbenchForm::new(
                FormKind::Planning,
                "Inspect original proposal request",
                vec![TextField::new("Request ID", String::new(), false)],
            ),
        });
        Ok(())
    }
    fn submit_form(&mut self) -> Result<()> {
        let form = self.form.as_ref().ok_or_else(|| {
            PmError::new(ErrorCode::InvalidInput, "No source action form is open")
        })?;
        match form.purpose {
            FormPurpose::Proposal => {
                let request = ProposalRequest {
                    reference: form.form.fields[0].value.trim().parse()?,
                    title: form.form.fields[1].value.clone(),
                };
                let plan = self.repository()?.preview_proposal(&request)?;
                if &plan.repository != self.repository()?.identity() {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "Proposal belongs to a replaced repository",
                    ));
                }
                plan.validate()?;
                self.repository()?.save_proposal_plan(&plan)?;
                self.review = Some(ReviewedSourceAction::Proposal(Box::new(plan)));
                self.request = None;
                self.outcome = None;
                self.error = None;
                self.form = None;
                self.scroll = 0;
                self.page = 0;
                Ok(())
            }
            FormPurpose::Request => {
                let request = form.form.fields[0].value.trim().parse()?;
                self.request = Some(request);
                self.review = None;
                self.form = None;
                self.inspect_request(false)
            }
        }
    }
    pub fn execute(&mut self) -> Result<()> {
        let action = self.review.clone().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "Inspect a fetch, sync, or proposal plan before executing",
            )
        })?;
        let request = self.request.get_or_insert_with(RequestId::new).clone();
        self.start(move |repository| match action {
            ReviewedSourceAction::Refresh { sync, input, .. } => {
                let result = if sync {
                    repository.sync_sources(&input, &request)
                } else {
                    repository.fetch_sources(&input, &request)
                }?;
                Ok(SourceActionOutcome::Refresh(result))
            }
            ReviewedSourceAction::Proposal(plan) => repository
                .publish_proposal(&plan, &request)
                .map(|outcome| SourceActionOutcome::Proposal(Box::new(outcome))),
        })
    }
    pub fn inspect_request(&mut self, resume: bool) -> Result<()> {
        if matches!(self.review, Some(ReviewedSourceAction::Refresh { .. })) {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "For retained fetch/sync, x retries its original request; proposal status/resume requires a proposal request",
            ));
        }
        let request = self.request.clone().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "Use t to inspect an original proposal request ID",
            )
        })?;
        self.start(move |repository| {
            let outcome = if resume {
                repository.resume_proposal(&request)
            } else {
                repository.proposal_status(&request)
            }?;
            Ok(SourceActionOutcome::Proposal(Box::new(outcome)))
        })
    }
    fn start(
        &mut self,
        task: impl FnOnce(Repository) -> Result<SourceActionOutcome> + Send + 'static,
    ) -> Result<()> {
        if self.busy() {
            return Err(PmError::new(
                ErrorCode::Locked,
                "The source operation is still pending",
            ));
        }
        let repository = self.repository()?.clone();
        self.worker = Some(
            OwnedPublication::start(self.signal.clone(), move || task(repository))
                .map_err(|error| PmError::new(ErrorCode::Locked, error.to_string()))?,
        );
        self.error = None;
        self.notice = Some(
            "Explicit source operation pending; exit and suspension wait for the owned worker."
                .into(),
        );
        Ok(())
    }
    pub fn poll(&mut self) {
        if let Some(result) = self.worker.as_mut().and_then(|worker| worker.poll()) {
            self.worker = None;
            self.finish(result);
        }
    }
    pub fn take_shutdown(&mut self) -> Option<OwnedPublication<Result<SourceActionOutcome>>> {
        self.worker.take()
    }
    pub fn finish(&mut self, result: std::thread::Result<Result<SourceActionOutcome>>) {
        self.notice = None;
        self.scroll = 0;
        match result {
            Ok(Ok(outcome)) => { self.outcome = Some(outcome); self.error = None; },
            Ok(Err(error)) => self.error = Some(error),
            Err(_) => self.error = Some(PmError::new(ErrorCode::RecoveryRequired, "Source operation outcome is unknown after its worker failed; inspect or retry the retained original request").details(serde_json::json!({"request_id":self.request,"outcome":"unknown"}))),
        }
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if let Some(form) = &mut self.form {
            let selected = form.form.selected;
            form.form.fields[selected].insert(text);
        }
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if self.busy() {
            self.poll();
            if self.busy() {
                self.notice = Some(
                    "Source operation pending; no cancellation or publication result is inferred."
                        .into(),
                );
                return true;
            }
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('d') {
            self.review = None;
            self.request = None;
            self.form = None;
            self.error = None;
            self.outcome = None;
            self.scroll = 0;
            return true;
        }
        if let Some(form) = &mut self.form {
            let action = form.form.key(key);
            match action {
                FormAction::Submit => {
                    if let Err(error) = self.submit_form() {
                        self.error = Some(error);
                    }
                }
                // Keep draft fields when leaving to Sources or Review.
                FormAction::Close => self.visible = false,
                FormAction::Edited => {}
            }
            return true;
        }
        if !key.modifiers.is_empty() {
            return false;
        }
        let result = match key.code {
            KeyCode::Char('f') => self.inspect_refresh(false),
            KeyCode::Char('s') => self.inspect_refresh(true),
            KeyCode::Char('n') => self.begin_proposal(),
            KeyCode::Char('t') => self.begin_request(),
            KeyCode::Char('x') => self.execute(),
            KeyCode::Char('v') => self.inspect_request(false),
            KeyCode::Char('u') => self.inspect_request(true),
            KeyCode::PageUp => {
                self.scroll = self.scroll.saturating_sub(8);
                Ok(())
            }
            KeyCode::PageDown => {
                self.scroll = self.scroll.saturating_add(8);
                Ok(())
            }
            KeyCode::Char(']') => {
                let pages = match &self.review {
                    Some(ReviewedSourceAction::Proposal(plan)) => (plan.changed.len()
                        + plan.proposal_changed.len())
                    .max(1)
                    .div_ceil(64),
                    _ => 1,
                };
                self.page = self.page.saturating_add(1).min(pages - 1);
                self.scroll = 0;
                Ok(())
            }
            KeyCode::Char('[') => {
                self.page = self.page.saturating_sub(1);
                self.scroll = 0;
                Ok(())
            }
            KeyCode::Esc => {
                self.visible = false;
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
