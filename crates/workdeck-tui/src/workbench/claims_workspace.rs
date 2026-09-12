use super::{
    ForegroundRunSignal,
    input::{FormAction, FormKind, TextField, WorkbenchForm},
    owned_publication::OwnedPublication,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Read,
    path::Path,
    sync::Arc,
};
use workdeck_pm::*;

const MAX_VERIFICATION_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ClaimAction {
    Acquire,
    Renew,
    Revalidate,
    Release,
    Cancel,
    Supersede,
    Recover,
    Complete,
    CompleteVerified,
}
impl ClaimAction {
    pub fn label(self) -> &'static str {
        match self {
            Self::Acquire => "Acquire",
            Self::Renew => "Renew",
            Self::Revalidate => "Revalidate",
            Self::Release => "Release",
            Self::Cancel => "Cancel",
            Self::Supersede => "Supersede",
            Self::Recover => "Recover expired claim",
            Self::Complete => "Complete and release",
            Self::CompleteVerified => "Complete with verification",
        }
    }
}
#[derive(Debug)]
pub(super) struct ClaimDraft {
    pub action: ClaimAction,
    pub form: WorkbenchForm,
    pub contract: ClaimWorkContract,
    pub binding: Option<ContentHash>,
    pub expected: Option<ClaimPrecondition>,
    pub request: Option<RequestId>,
    pub input: Option<ClaimRequest>,
    pub completion_input: Option<ContentHash>,
    pub verified_completion_input: Option<ContentHash>,
    pub verified_completion_path: Option<String>,
    pub verified_completion: Option<CompleteVerifiedIssue>,
    pub release_request: Option<RequestId>,
    pub visible: bool,
}
#[derive(Debug, Default)]
pub(super) struct ClaimTask {
    pub scroll: u16,
    pub contract: Option<ClaimWorkContract>,
    pub binding: Option<ContentHash>,
    pub status: Option<ClaimStatus>,
    pub draft: Option<ClaimDraft>,
    pub outcome: Option<ClaimOperationOutcome>,
    pub completion: Option<ClaimedCompletionOutcome>,
    pub completion_verified: bool,
    pub error: Option<PmError>,
    pub notice: Option<String>,
}
#[derive(Debug)]
pub(super) enum ClaimPublication {
    Claim(Box<ClaimOperationOutcome>),
    Completion(Box<ClaimedCompletionOutcome>),
    VerifiedCompletion(Box<ClaimedCompletionOutcome>),
}
type ClaimWorker = OwnedPublication<Result<ClaimPublication>>;

fn read_verification_file(repository: &Repository, value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value == "-" {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "A regular verification JSON path is required",
        ));
    }
    let supplied = Path::new(value);
    let path = if supplied.is_absolute() {
        supplied.to_owned()
    } else {
        repository
            .root()
            .parent()
            .unwrap_or_else(|| repository.root())
            .join(supplied)
    };
    let metadata = fs::symlink_metadata(&path).map_err(|error| PmError::io(&path, error))?;
    if !metadata.is_file() {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "verification input must be a regular file",
        )
        .at(path));
    }
    if metadata.len() > MAX_VERIFICATION_BYTES {
        return Err(
            PmError::new(ErrorCode::InvalidSchema, "verification input exceeds 2 MiB").at(path),
        );
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
    }
    let file = options
        .open(&path)
        .map_err(|error| PmError::io(&path, error))?;
    if !file
        .metadata()
        .map_err(|error| PmError::io(&path, error))?
        .is_file()
    {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "verification input must be a regular file",
        )
        .at(path));
    }
    let mut bytes = Vec::new();
    file.take(MAX_VERIFICATION_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PmError::io(&path, error))?;
    if bytes.len() as u64 > MAX_VERIFICATION_BYTES {
        return Err(
            PmError::new(ErrorCode::InvalidSchema, "verification input exceeds 2 MiB").at(path),
        );
    }
    String::from_utf8(bytes).map_err(|_| {
        PmError::new(ErrorCode::InvalidSchema, "verification input must be UTF-8").at(path)
    })
}

#[derive(Debug)]
struct ActivePublication {
    issue: IssueId,
    request: RequestId,
    worker: ClaimWorker,
}
#[derive(Debug)]
pub(super) struct ClaimsWorkspace {
    repository: Option<Repository>,
    pub current: Option<IssueId>,
    pub author: String,
    pub tasks: BTreeMap<String, ClaimTask>,
    pub signal: Arc<ForegroundRunSignal>,
    active: Option<ActivePublication>,
}
impl ClaimsWorkspace {
    pub fn new(
        repository: Option<Repository>,
        author: String,
        signal: Arc<ForegroundRunSignal>,
    ) -> Self {
        Self {
            repository,
            current: None,
            author,
            tasks: BTreeMap::new(),
            signal,
            active: None,
        }
    }
    pub fn bind_if_missing(&mut self, repository: Option<Repository>) {
        if self.repository.is_none() {
            self.repository = repository;
        }
    }
    pub fn attach_signal(&mut self, signal: Arc<ForegroundRunSignal>) {
        if self.active.is_none() {
            self.signal = signal;
        }
    }
    pub fn state(&self) -> Option<&ClaimTask> {
        self.tasks.get(
            &self
                .current
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
        )
    }
    pub fn state_mut(&mut self) -> &mut ClaimTask {
        self.tasks
            .entry(
                self.current
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
            )
            .or_default()
    }
    pub fn busy(&self) -> bool {
        self.active.is_some()
    }
    fn repository(&self) -> Result<&Repository> {
        self.repository.as_ref().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotInitialized,
                "Run workdeck init or finish migration before authoring claims",
            )
        })
    }
    pub fn open(
        &mut self,
        issue: Option<IssueId>,
        inspected: Option<Result<(ClaimWorkContract, Option<ContentHash>)>>,
    ) {
        self.current = issue;
        if let Some(inspected) = inspected {
            match inspected {
                Ok((contract, binding))
                    if self.current.as_ref() == Some(&contract.issue)
                        && self.repository.as_ref().is_some_and(|repository| {
                            repository.identity() == &contract.accepted_source.repository
                        }) =>
                {
                    self.state_mut().contract = Some(contract);
                    self.state_mut().binding = binding;
                }
                Ok(_) => {
                    self.state_mut().contract = None;
                    self.state_mut().binding = None;
                    self.state_mut().error = Some(PmError::new(
                        ErrorCode::StaleSource,
                        "Inspected claim contract belongs to another source or issue",
                    ));
                }
                Err(error) => {
                    self.state_mut().contract = None;
                    self.state_mut().binding = None;
                    self.state_mut().error = Some(error);
                }
            }
        }
        if self
            .state()
            .is_none_or(|state| state.contract.is_none() && state.error.is_none())
        {
            if let Err(error) = self.refresh() {
                self.state_mut().error = Some(error);
            }
        } else if let Err(error) = self.refresh_status() {
            self.state_mut().error = Some(error);
        }
    }
    fn refresh_status(&mut self) -> Result<()> {
        let Some(issue) = self.current.clone() else {
            return Ok(());
        };
        let status = self
            .repository()?
            .claims()?
            .into_iter()
            .find(|status| status.claim.metadata.issue == issue);
        self.state_mut().status = status;
        Ok(())
    }
    pub fn refresh(&mut self) -> Result<()> {
        let issue = self.current.clone().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "Select an issue in Issues or an accepted source before inspecting claims",
            )
        })?;
        let repository = self.repository()?;
        let status = repository.source_status()?;
        let contract = repository.claim_contract(&issue)?;
        if status.repository != contract.accepted_source.repository {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Claim source identity changed during inspection",
            ));
        }
        let binding = status.binding;
        if status.shared.is_some() && binding.is_none() {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Shared publication destination is unavailable; inspect source status before authoring a claim",
            ));
        }
        // A missing cached coordination ref does not invalidate the inspected
        // accepted contract. Keep its identity visible while status remains
        // explicitly unavailable; an explicit acquire still validates remotely.
        self.state_mut().contract = Some(contract);
        self.state_mut().binding = binding;
        let statuses = self.repository()?.claims()?;
        let state = self.state_mut();
        if state.draft.is_some() {
            state.notice = Some("Current status refreshed; retained draft contract, token, generation, and request were not rebased.".into());
        }
        state.status = statuses
            .into_iter()
            .find(|status| status.claim.metadata.issue == issue);
        state.error = None;
        Ok(())
    }
    pub fn begin(&mut self, action: ClaimAction) -> Result<()> {
        if self.busy() {
            return Err(PmError::new(
                ErrorCode::Locked,
                "A planning publication is still owned by this terminal",
            ));
        }
        let completed = self.state().is_some_and(|state| {
            state
                .draft
                .as_ref()
                .zip(state.outcome.as_ref())
                .is_some_and(|(draft, outcome)| {
                    draft.request.as_ref() == Some(&outcome.request_id)
                        && outcome.receipt.is_some()
                        && outcome.publication.as_ref().is_none_or(|publication| {
                            publication.state == PublicationState::Confirmed
                        })
                })
                || state
                    .draft
                    .as_ref()
                    .zip(state.completion.as_ref())
                    .is_some_and(|(draft, completion)| {
                        draft.request.as_ref() == Some(&completion.completion.request_id)
                            && (completion.release_recorded || state.completion_verified)
                    })
        });
        if !completed && let Some(draft) = self.state_mut().draft.as_mut() {
            draft.visible = true;
            return Ok(());
        }
        if completed {
            self.state_mut().draft = None;
        }
        let state = self
            .state()
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "Inspect a claim contract first"))?;
        let contract = state.contract.clone().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "Inspect an exact local or accepted work contract first",
            )
        })?;
        let binding = state.binding.clone();
        if contract.accepted_source.role == SourceRole::Accepted && binding.is_none() {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "The inspected accepted source has no publication binding; refresh its source before authoring a claim",
            ));
        }
        let expected = if action == ClaimAction::Acquire {
            None
        } else {
            Some(
                state
                    .status
                    .as_ref()
                    .ok_or_else(|| {
                        PmError::new(
                            ErrorCode::NotFound,
                            "No captured claim is available for this action",
                        )
                    })?
                    .claim
                    .precondition(),
            )
        };
        let mut fields = vec![TextField::new("Actor", self.author.clone(), false)];
        if matches!(
            action,
            ClaimAction::Acquire
                | ClaimAction::Renew
                | ClaimAction::Revalidate
                | ClaimAction::Recover
        ) {
            fields.push(TextField::new(
                "TTL seconds (optional)",
                String::new(),
                false,
            ));
        }
        if matches!(
            action,
            ClaimAction::Release
                | ClaimAction::Cancel
                | ClaimAction::Supersede
                | ClaimAction::Recover
                | ClaimAction::Complete
        ) {
            fields.push(TextField::new("Reason", String::new(), true));
        }
        if action == ClaimAction::CompleteVerified {
            fields.push(TextField::new(
                "Verification JSON path",
                String::new(),
                false,
            ));
        }
        let mut form = WorkbenchForm::new(
            FormKind::Planning,
            format!("{} claim", action.label()),
            fields,
        );
        form.help.push(format!(
            "Contract {} · source {:?} · issue revision {}",
            contract.requirements,
            contract.accepted_source.role,
            contract.issue_source.revision.get()
        ));
        if let Some(binding) = &binding {
            form.help
                .push(format!("Inspected publication binding {binding}"));
        }
        form.help.push("Explicit publication may contact the configured remote. It does not stop an external coding process.".into());
        self.state_mut().draft = Some(ClaimDraft {
            action,
            form,
            contract,
            binding,
            expected,
            request: None,
            input: None,
            completion_input: None,
            verified_completion_input: None,
            verified_completion_path: None,
            verified_completion: None,
            release_request: None,
            visible: true,
        });
        Ok(())
    }
    fn build_input(draft: &ClaimDraft) -> Result<ClaimRequest> {
        let actor = draft.form.fields[0].value.trim().to_owned();
        if actor.is_empty() {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "A claim actor is required",
            ));
        }
        let ttl = if matches!(
            draft.action,
            ClaimAction::Acquire
                | ClaimAction::Renew
                | ClaimAction::Revalidate
                | ClaimAction::Recover
        ) {
            let text = draft.form.fields[1].value.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.parse::<u64>().map_err(|_| {
                    PmError::new(
                        ErrorCode::InvalidInput,
                        "TTL must be a whole number of seconds",
                    )
                })?)
            }
        } else {
            None
        };
        let reason = draft
            .form
            .fields
            .iter()
            .find(|field| field.label == "Reason")
            .map(|field| field.value.trim().to_owned())
            .unwrap_or_default();
        if matches!(
            draft.action,
            ClaimAction::Release
                | ClaimAction::Cancel
                | ClaimAction::Supersede
                | ClaimAction::Recover
        ) && reason.is_empty()
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "An explicit reason is required",
            ));
        }
        let expected = || {
            draft.expected.clone().ok_or_else(|| {
                PmError::new(
                    ErrorCode::InvalidInput,
                    "The captured claim precondition is missing",
                )
            })
        };
        if matches!(draft.action, ClaimAction::Acquire | ClaimAction::Recover) {
            let recovery = (draft.action == ClaimAction::Recover)
                .then(|| expected().map(|expected| ClaimRecovery { expected, reason }))
                .transpose()?;
            return Ok(ClaimRequest::Acquire {
                input: Box::new(AcquireClaim {
                    actor,
                    contract: draft.contract.clone(),
                    ttl_seconds: ttl,
                    recovery,
                }),
            });
        }
        let mutation = match draft.action {
            ClaimAction::Renew => ClaimMutation::Renew {
                actor,
                ttl_seconds: ttl,
            },
            ClaimAction::Revalidate => ClaimMutation::Revalidate {
                actor,
                contract: draft.contract.clone().into(),
                ttl_seconds: ttl,
            },
            ClaimAction::Release => ClaimMutation::Release { actor, reason },
            ClaimAction::Cancel => ClaimMutation::Cancel { actor, reason },
            ClaimAction::Supersede => ClaimMutation::Supersede { actor, reason },
            ClaimAction::Acquire | ClaimAction::Recover => unreachable!(),
            ClaimAction::Complete => {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "Complete and release uses its separate retained completion input",
                ));
            }
            ClaimAction::CompleteVerified => {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "Verified completion uses its retained verification input",
                ));
            }
        };
        Ok(ClaimRequest::Mutate {
            issue: draft.contract.issue.clone(),
            expected: expected()?,
            mutation,
        })
    }
    pub fn submit(&mut self) -> Result<()> {
        if self.state().is_some_and(|state| {
            state
                .draft
                .as_ref()
                .is_some_and(|draft| draft.action == ClaimAction::Complete)
        }) {
            return self.submit_completion_with(|repository, input, request, release, reason| {
                repository.complete_claimed_issue_and_release(&input, &request, &release, &reason)
            });
        }
        if self.state().is_some_and(|state| {
            state
                .draft
                .as_ref()
                .is_some_and(|draft| draft.action == ClaimAction::CompleteVerified)
        }) {
            return self.submit_verified_completion();
        }
        let binding = self
            .state()
            .and_then(|state| state.draft.as_ref())
            .and_then(|draft| draft.binding.clone());
        self.submit_with(move |repository, input, request| match binding {
            Some(binding) => repository.mutate_claim_reviewed(&input, &request, &binding),
            None => repository.mutate_local_claim_outcome(&input, &request),
        })
    }

    fn submit_verified_completion(&mut self) -> Result<()> {
        if self.busy() {
            return Err(PmError::new(
                ErrorCode::Locked,
                "Publication is still pending",
            ));
        }
        let repository = self.repository()?.clone();
        let draft = self
            .state_mut()
            .draft
            .as_mut()
            .filter(|draft| draft.action == ClaimAction::CompleteVerified)
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::InvalidInput,
                    "Open Complete with verification first",
                )
            })?;
        let actor = draft.form.fields[0].value.trim().to_owned();
        if actor.is_empty() {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "A claim actor is required",
            ));
        }
        let path = draft.form.fields[1].value.trim().to_owned();
        let verification = if let Some(verification) = draft.verified_completion.clone() {
            if draft.verified_completion_path.as_deref() != Some(path.as_str()) {
                return Err(PmError::new(
                    ErrorCode::IdempotencyConflict,
                    "Retained verified completion input has a different file path; restore it or discard the draft before creating a new intent",
                ));
            }
            verification
        } else {
            let document = read_verification_file(&repository, &path)?;
            serde_json::from_str::<CompleteVerifiedIssue>(&document).map_err(|error| {
                PmError::new(
                    ErrorCode::InvalidSchema,
                    format!("invalid verified completion input: {error}"),
                )
                .at(path.clone())
            })?
        };
        let claim = CompleteClaimedIssue {
            issue: draft.contract.issue.clone(),
            actor: actor.clone(),
            expected_claim: draft.expected.clone().ok_or_else(|| {
                PmError::new(
                    ErrorCode::InvalidInput,
                    "Captured claim precondition is missing",
                )
            })?,
            expected_issue: draft.contract.issue_source.clone(),
            contract: draft.contract.clone(),
            expected_binding: draft.binding.clone(),
        };
        if verification.issue != claim.issue
            || verification.expected_issue != claim.expected_issue
            || verification.actor != claim.actor
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "verified completion file must match the claim issue, actor and source token",
            ));
        }
        let input = CompleteClaimedVerifiedIssue {
            claim,
            verification,
        };
        let fingerprint = ContentHash::of(
            &serde_json::to_vec(&(&path, &input))
                .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?,
        );
        if draft
            .verified_completion_input
            .as_ref()
            .is_some_and(|original| original != &fingerprint)
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "Retained verified completion input changed; restore the original file or explicitly discard the draft before creating a new intent",
            ));
        }
        draft.verified_completion_input.get_or_insert(fingerprint);
        draft.verified_completion_path.get_or_insert(path);
        draft
            .verified_completion
            .get_or_insert_with(|| input.verification.clone());
        let request = draft.request.get_or_insert_with(RequestId::new).clone();
        let worker_request = request.clone();
        let issue = input.claim.issue.clone();
        let repository_id = repository.identity().clone();
        let worker = OwnedPublication::start(self.signal.clone(), move || {
            repository
                .complete_claimed_verified_issue(&input, &worker_request)
                .map(|completion| {
                    ClaimPublication::VerifiedCompletion(Box::new(ClaimedCompletionOutcome {
                        repository: repository_id,
                        completion,
                        release: None,
                        release_error: None,
                        release_publication: None,
                        release_recorded: false,
                    }))
                })
        })
        .map_err(|error| PmError::new(ErrorCode::Locked, error.to_string()))?;
        self.active = Some(ActivePublication {
            issue,
            request,
            worker,
        });
        self.state_mut().error = None;
        self.state_mut().notice = Some(
            "Authenticated claimed completion pending; the current claim and verification proof are retained for exact retry.".into(),
        );
        Ok(())
    }

    pub(super) fn submit_with(
        &mut self,
        publish: impl FnOnce(Repository, ClaimRequest, RequestId) -> Result<ClaimOperationOutcome>
        + Send
        + 'static,
    ) -> Result<()> {
        if self.busy() {
            return Err(PmError::new(
                ErrorCode::Locked,
                "Publication is still pending; original request is retained",
            ));
        }
        let repository = self.repository()?.clone();
        let draft =
            self.state_mut().draft.as_mut().ok_or_else(|| {
                PmError::new(ErrorCode::InvalidInput, "Open a claim action first")
            })?;
        let input = Self::build_input(draft)?;
        if draft
            .input
            .as_ref()
            .is_some_and(|original| original != &input)
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "Retained claim request has different field values; restore them to retry or explicitly discard the draft before creating a new intent.",
            ));
        }
        let request = draft.request.get_or_insert_with(RequestId::new).clone();
        draft.input.get_or_insert_with(|| input.clone());
        let issue = input.issue().clone();
        let worker_request = request.clone();
        let worker = OwnedPublication::start(self.signal.clone(), move || {
            publish(repository, input, worker_request)
                .map(|outcome| ClaimPublication::Claim(Box::new(outcome)))
        })
        .map_err(|error| PmError::new(ErrorCode::Locked, error.to_string()))?;
        self.active = Some(ActivePublication {
            issue,
            request,
            worker,
        });
        self.state_mut().error = None;
        self.state_mut().notice = Some("Planning publication pending. Exit and suspension are deferred until its bounded operation joins; publication cannot be silently canceled.".into());
        Ok(())
    }
    pub(super) fn submit_completion_with(
        &mut self,
        publish: impl FnOnce(
            Repository,
            CompleteClaimedIssue,
            RequestId,
            RequestId,
            String,
        ) -> Result<ClaimedCompletionOutcome>
        + Send
        + 'static,
    ) -> Result<()> {
        if self.busy() {
            return Err(PmError::new(
                ErrorCode::Locked,
                "Publication is still pending",
            ));
        }
        let repository = self.repository()?.clone();
        let draft = self
            .state_mut()
            .draft
            .as_mut()
            .filter(|draft| draft.action == ClaimAction::Complete)
            .ok_or_else(|| {
                PmError::new(ErrorCode::InvalidInput, "Open Complete and release first")
            })?;
        let actor = draft.form.fields[0].value.trim().to_owned();
        let reason = draft.form.fields[1].value.trim().to_owned();
        if actor.is_empty() || reason.is_empty() {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "Actor and explicit release reason are required",
            ));
        }
        let input = CompleteClaimedIssue {
            issue: draft.contract.issue.clone(),
            actor,
            expected_claim: draft.expected.clone().ok_or_else(|| {
                PmError::new(
                    ErrorCode::InvalidInput,
                    "Captured claim precondition is missing",
                )
            })?,
            expected_issue: draft.contract.issue_source.clone(),
            contract: draft.contract.clone(),
            expected_binding: draft.binding.clone(),
        };
        let fingerprint = ContentHash::of(
            &serde_json::to_vec(&(&input, &reason))
                .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?,
        );
        if draft
            .completion_input
            .as_ref()
            .is_some_and(|original| original != &fingerprint)
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "Complete/release retry fields changed; restore the original fields or explicitly discard before a new intent",
            ));
        }
        draft.completion_input.get_or_insert(fingerprint);
        let request = draft.request.get_or_insert_with(RequestId::new).clone();
        let release = draft
            .release_request
            .get_or_insert_with(RequestId::new)
            .clone();
        let worker_request = request.clone();
        let issue = input.issue.clone();
        let worker = OwnedPublication::start(self.signal.clone(), move || {
            publish(repository, input, worker_request, release, reason)
                .map(|outcome| ClaimPublication::Completion(Box::new(outcome)))
        })
        .map_err(|error| PmError::new(ErrorCode::Locked, error.to_string()))?;
        self.active = Some(ActivePublication {
            issue,
            request,
            worker,
        });
        self.state_mut().error = None;
        self.state_mut().notice = Some("Claimed completion and the separate release are pending; both original request IDs are retained.".into());
        Ok(())
    }
    fn complete(
        &mut self,
        issue: IssueId,
        request: RequestId,
        result: std::thread::Result<Result<ClaimPublication>>,
    ) {
        let state = self.tasks.entry(issue.to_string()).or_default();
        state.notice = None;
        state.scroll = 0;
        match result {
            Ok(Ok(ClaimPublication::Claim(outcome))) => {
                state.status = outcome.current.clone(); state.outcome = Some(*outcome); state.completion = None; state.error = None;
                if let Some(draft) = &mut state.draft { draft.visible = false; }
                state.notice = Some("Publication joined. Historical receipt and current ownership are separate. t retries the retained request; a new action reviews a new intent after confirmed publication.".into());
            },
            Ok(Ok(ClaimPublication::Completion(outcome))) => {
                state.status = outcome.release_publication.as_ref().and_then(|publication| publication.current.clone());
                state.error = None; state.outcome = None; state.completion_verified = false;
                if let Some(draft) = &mut state.draft { draft.visible = false; }
                state.notice = Some("Completion receipt retained. Release is a separate result; t retries the same two requests, r inspects current ownership.".into());
                state.completion = Some(*outcome);
            },
            Ok(Ok(ClaimPublication::VerifiedCompletion(outcome))) => {
                state.error = None;
                state.outcome = None;
                state.completion_verified = true;
                if let Some(draft) = &mut state.draft {
                    draft.visible = false;
                }
                state.notice = Some(
                    "Authenticated claimed completion recorded; t retries the exact proof and r inspects current ownership.".into(),
                );
                state.completion = Some(*outcome);
            },
            Ok(Err(error)) => state.error = Some(error),
            Err(_) => state.error = Some(PmError::new(ErrorCode::RecoveryRequired, "Planning publication outcome is unknown after its worker failed; inspect or retry the exact retained request before creating another intent.")
                .details(serde_json::json!({"request_id":request,"outcome":"unknown"}))),
        }
    }
    pub fn poll(&mut self) {
        if let Some(result) = self.active.as_mut().and_then(|active| active.worker.poll()) {
            let active = self.active.take().expect("completed owned publication");
            self.complete(active.issue, active.request, result);
        }
    }
    pub fn take_shutdown(&mut self) -> Option<(IssueId, RequestId, ClaimWorker)> {
        self.active
            .take()
            .map(|active| (active.issue, active.request, active.worker))
    }
    pub fn finish_shutdown(
        &mut self,
        issue: IssueId,
        request: RequestId,
        result: std::thread::Result<Result<ClaimPublication>>,
    ) {
        self.complete(issue, request, result);
    }
    pub fn form(&self) -> Option<&WorkbenchForm> {
        self.state()?
            .draft
            .as_ref()
            .filter(|draft| draft.visible)
            .map(|draft| &draft.form)
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if !self.busy()
            && let Some(draft) = self
                .state_mut()
                .draft
                .as_mut()
                .filter(|draft| draft.visible)
        {
            let selected = draft.form.selected;
            draft.form.fields[selected].insert(text);
        }
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> bool {
        if self.busy() {
            self.poll();
            if self.busy() {
                self.state_mut().notice = Some("Publication pending; leave or retry only after its bounded worker joins. No cancellation or claim loss has been inferred.".into());
                return true;
            }
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('d') {
            self.state_mut().draft = None;
            self.state_mut().error = None;
            return true;
        }
        if let Some(draft) = self
            .state_mut()
            .draft
            .as_mut()
            .filter(|draft| draft.visible)
        {
            match draft.form.key(key) {
                FormAction::Submit => {
                    if let Err(error) = self.submit() {
                        self.state_mut().error = Some(error);
                    }
                }
                FormAction::Close => draft.visible = false,
                FormAction::Edited => {}
            }
            return true;
        }
        if !key.modifiers.is_empty() {
            return false;
        }
        let result = match key.code {
            KeyCode::PageDown => {
                let state = self.state_mut();
                state.scroll = state.scroll.saturating_add(8);
                Ok(())
            }
            KeyCode::PageUp => {
                let state = self.state_mut();
                state.scroll = state.scroll.saturating_sub(8);
                Ok(())
            }
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('t') => {
                if let Some(draft) = self.state_mut().draft.as_mut() {
                    draft.visible = true;
                    Ok(())
                } else {
                    Err(PmError::new(
                        ErrorCode::NotFound,
                        "No retained request is available to retry",
                    ))
                }
            }
            KeyCode::Char('n') => self.begin(ClaimAction::Acquire),
            KeyCode::Char('u') => self.begin(ClaimAction::Renew),
            KeyCode::Char('v') => self.begin(ClaimAction::Revalidate),
            KeyCode::Char('d') => self.begin(ClaimAction::Release),
            KeyCode::Char('c') => self.begin(ClaimAction::Cancel),
            KeyCode::Char('s') => self.begin(ClaimAction::Supersede),
            KeyCode::Char('x') => self.begin(ClaimAction::Recover),
            KeyCode::Char('e') => self.begin(ClaimAction::Complete),
            KeyCode::Char('g') => self.begin(ClaimAction::CompleteVerified),
            _ => return false,
        };
        if let Err(error) = result {
            self.state_mut().error = Some(error);
        }
        true
    }
}
