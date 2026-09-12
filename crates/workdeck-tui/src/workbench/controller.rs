use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf};
use workdeck_pm::{
    ArchiveFilter, CommentRecord, CreateIssue, ErrorCode, IssueId, IssueMutation, IssueQuery,
    IssueQuerySnapshot, IssueRecord, IssueSort, PmError, Priority, Repository, RepositoryId,
    RequestId, SourceLink, SourceToken, TargetMatch, UpdateIssue, transactions::MutationReceipt,
};

type Result<T> = std::result::Result<T, PmError>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IssueFilter {
    pub query: String,
    pub status: Option<String>,
    pub priority: Option<Priority>,
    pub assignee: Option<String>,
    pub label: Option<String>,
    pub project: Option<String>,
    pub cycle: Option<String>,
    pub include_archived: bool,
    pub archived_only: bool,
    pub milestone: Option<String>,
    pub targets: Vec<String>,
    pub target_match: TargetMatch,
    pub due_at: Option<String>,
    pub sort: Vec<IssueSort>,
}

impl IssueFilter {
    pub fn query(&self) -> IssueQuery {
        IssueQuery {
            query: self.query.clone(),
            ids: None,
            status: self.status.clone(),
            priority: self.priority,
            assignee: self.assignee.clone(),
            reviewer: None,
            workflow_categories: Vec::new(),
            due_before: None,
            label: self.label.clone(),
            project: self.project.clone(),
            cycle: self.cycle.clone(),
            milestone: self.milestone.clone(),
            targets: self.targets.clone(),
            target_match: self.target_match,
            due_at: self.due_at.clone(),
            archive: if self.archived_only {
                ArchiveFilter::Archived
            } else if self.include_archived {
                ArchiveFilter::All
            } else {
                ArchiveFilter::Active
            },
            sort: if self.sort.is_empty() {
                IssueQuery::default().sort
            } else {
                self.sort.clone()
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkbenchPane {
    #[default]
    List,
    Detail,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkbenchViewState {
    pub pane: WorkbenchPane,
    pub list_offset: usize,
    pub detail_scroll: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum DraftKey {
    Create,
    Edit(IssueId),
    Comment(IssueId),
}

#[derive(Debug, Clone)]
pub enum DraftInput {
    Create(CreateIssue),
    Edit(UpdateIssue),
    Comment { author: String, body: String },
}

impl DraftInput {
    fn fingerprint(&self) -> Result<Value> {
        let result = match self {
            Self::Create(input) => serde_json::to_value(input),
            Self::Edit(input) => serde_json::to_value(input),
            Self::Comment { author, body } => Ok(json!({"author":author,"body":body})),
        };
        result.map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))
    }
}

/// Editable fields are retained independently from the current issue snapshot.
/// Refresh never rebases the captured source or overwrites an unfinished draft.
#[derive(Debug, Clone)]
pub struct WorkbenchDraft {
    pub input: DraftInput,
    /// Raw form text survives temporary invalid JSON and reopening the draft.
    pub custom_input: Option<String>,
    source: Option<SourceToken>,
    submission: Option<(Value, RequestId)>,
}

impl WorkbenchDraft {
    pub fn source(&self) -> Option<&SourceToken> {
        self.source.as_ref()
    }

    /// The request already associated with this draft, if it was submitted.
    pub fn request_id(&self) -> Option<&RequestId> {
        self.submission.as_ref().map(|(_, request)| request)
    }

    fn request(&mut self) -> Result<RequestId> {
        // An acknowledgement may have been lost after publication. Keep the
        // request even if the retained draft changes: the shared engine must
        // detect changed input instead of publishing a second operation.
        if let Some((_, request)) = &self.submission {
            return Ok(request.clone());
        }
        let input = json!({"input":self.input.fingerprint()?,"source":self.source});
        let request = RequestId::new();
        self.submission = Some((input, request.clone()));
        Ok(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationTarget {
    pub repository: RepositoryId,
    pub issue: IssueId,
    pub source: SourceToken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueAction {
    Status(String),
    Assign(Option<String>),
    Priority(Priority),
    Labels(Vec<String>),
    LinkFile(SourceLink),
    Complete,
    Reopen,
    Archive(bool),
}

impl IssueAction {
    fn mutation(&self) -> IssueMutation {
        match self {
            Self::Status(status) => update_field("status", json!(status)),
            Self::Assign(assignee) => update_field("assignee", json!(assignee)),
            Self::Priority(priority) => update_field("priority", json!(priority)),
            Self::Labels(labels) => update_field("labels", json!(labels)),
            Self::LinkFile(link) => IssueMutation::Add {
                field: workdeck_pm::IssueCollection::Files,
                value: json!(link),
            },
            Self::Complete => IssueMutation::Complete { manual: None },
            Self::Reopen => IssueMutation::Reopen,
            Self::Archive(archived) => IssueMutation::Archive {
                archived: *archived,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkbenchErrorContext {
    Refresh,
    Mutation,
    Navigation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkbenchError {
    pub error: PmError,
    pub context: WorkbenchErrorContext,
    pub draft: Option<DraftKey>,
    /// A successful write must never be reported as failed because the next
    /// read failed. The receipt remains available in `last_receipt`.
    pub after_commit: bool,
}

#[derive(Debug, Clone)]
struct PendingAction {
    target: MutationTarget,
    action: IssueAction,
    request: RequestId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IssueReviewTarget {
    File(SourceLink),
    Commit(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanningReturnContext {
    pub selected_issue: Option<IssueId>,
    pub filter: IssueFilter,
    pub view: WorkbenchViewState,
    pub active_draft: Option<DraftKey>,
}

/// The native shell supplies its actual review source/selection/viewport state
/// as R. Keeping it opaque avoids manufacturing a second review state model.
#[derive(Debug, Clone)]
pub struct IssueNavigation<R> {
    pub repository: RepositoryId,
    pub repository_root: PathBuf,
    pub issue: IssueId,
    pub target: IssueReviewTarget,
    pub planning: PlanningReturnContext,
    pub review: R,
}

#[derive(Debug)]
pub struct WorkbenchController {
    repository: Option<Repository>,
    issues: Vec<IssueRecord>,
    query_snapshot: Option<IssueQuerySnapshot>,
    indexed: bool,
    projected_selection: Option<workdeck_pm::projection::ProjectionRowToken>,
    visible: Vec<usize>,
    selected_issue: Option<IssueId>,
    filter: IssueFilter,
    view: WorkbenchViewState,
    comments: Option<(IssueId, Vec<CommentRecord>)>,
    drafts: BTreeMap<DraftKey, WorkbenchDraft>,
    active_draft: Option<DraftKey>,
    pending_action: Option<PendingAction>,
    error: Option<WorkbenchError>,
    last_receipt: Option<MutationReceipt>,
}

impl WorkbenchController {
    /// Construction is read-only; call refresh explicitly from the host's I/O
    /// dispatch boundary. This controller never owns an event loop or watcher.
    pub fn new(repository: Repository) -> Self {
        Self::empty(Some(repository), None)
    }

    /// The indexed reader owns collection queries. This controller retains only
    /// the selected native record and independently captured authoring drafts.
    pub(super) fn new_indexed(repository: Repository) -> Self {
        let mut controller = Self::new(repository);
        controller.indexed = true;
        controller
    }

    pub(super) fn select_projection(
        &mut self,
        token: &workdeck_pm::projection::ProjectionRowToken,
    ) -> Result<()> {
        if !self.indexed {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "indexed selection requires an indexed controller",
            ));
        }
        let issue = match self
            .repository()
            .and_then(|repository| repository.issue_from_projection(token))
        {
            Ok(issue) => issue,
            Err(error) => return self.fail(error, WorkbenchErrorContext::Refresh, None, false),
        };
        if self.selected_issue.as_ref() != Some(&issue.metadata.id) {
            self.comments = None;
        }
        self.selected_issue = Some(issue.metadata.id.clone());
        self.issues = vec![issue];
        self.visible = vec![0];
        self.query_snapshot = None;
        self.projected_selection = Some(token.clone());
        // Never clear a failed mutation, replace draft input, or rebase its source.
        if self
            .error
            .as_ref()
            .is_some_and(|error| error.context == WorkbenchErrorContext::Refresh)
        {
            self.error = None;
        }
        Ok(())
    }

    pub(super) fn clear_index_selection(&mut self) {
        if self.indexed {
            self.selected_issue = None;
            self.projected_selection = None;
            self.issues.clear();
            self.visible.clear();
            self.comments = None;
        }
    }

    pub fn unavailable(error: PmError) -> Self {
        Self::empty(None, Some(error))
    }

    fn empty(repository: Option<Repository>, error: Option<PmError>) -> Self {
        Self {
            repository,
            issues: Vec::new(),
            query_snapshot: None,
            indexed: false,
            projected_selection: None,
            visible: Vec::new(),
            selected_issue: None,
            filter: IssueFilter::default(),
            view: WorkbenchViewState::default(),
            comments: None,
            drafts: BTreeMap::new(),
            active_draft: None,
            pending_action: None,
            last_receipt: None,
            error: error.map(|error| WorkbenchError {
                error,
                context: WorkbenchErrorContext::Refresh,
                draft: None,
                after_commit: false,
            }),
        }
    }

    pub fn issues(&self) -> &[IssueRecord] {
        &self.issues
    }
    pub fn visible_issues(&self) -> impl ExactSizeIterator<Item = &IssueRecord> {
        self.visible.iter().map(|index| &self.issues[*index])
    }
    pub fn selected_id(&self) -> Option<&IssueId> {
        self.selected_issue.as_ref()
    }
    pub fn selected_issue(&self) -> Option<&IssueRecord> {
        self.selected_issue
            .as_ref()
            .and_then(|id| self.issues.iter().find(|issue| &issue.metadata.id == id))
    }
    pub fn selected_target(&self) -> Option<MutationTarget> {
        let repository = self.repository.as_ref()?.identity().clone();
        self.selected_issue().map(|issue| MutationTarget {
            repository,
            issue: issue.metadata.id.clone(),
            source: issue.source.clone(),
        })
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.selected_issue.as_ref().and_then(|id| {
            self.visible
                .iter()
                .position(|index| &self.issues[*index].metadata.id == id)
        })
    }
    pub fn filter(&self) -> &IssueFilter {
        &self.filter
    }
    pub fn view(&self) -> &WorkbenchViewState {
        &self.view
    }
    pub fn view_mut(&mut self) -> &mut WorkbenchViewState {
        &mut self.view
    }
    pub fn error(&self) -> Option<&WorkbenchError> {
        self.error.as_ref()
    }
    pub fn dismiss_error(&mut self) {
        self.error = None;
    }
    pub fn last_receipt(&self) -> Option<&MutationReceipt> {
        self.last_receipt.as_ref()
    }
    pub fn drafts(&self) -> &BTreeMap<DraftKey, WorkbenchDraft> {
        &self.drafts
    }
    pub fn draft_mut(&mut self, key: &DraftKey) -> Option<&mut WorkbenchDraft> {
        self.drafts.get_mut(key)
    }
    pub fn active_draft(&self) -> Option<(&DraftKey, &WorkbenchDraft)> {
        self.active_draft
            .as_ref()
            .and_then(|key| self.drafts.get_key_value(key))
    }
    pub fn comments(&self) -> &[CommentRecord] {
        self.comments
            .as_ref()
            .filter(|(id, _)| Some(id) == self.selected_issue.as_ref())
            .map_or(&[], |(_, comments)| comments.as_slice())
    }

    pub(super) fn repository(&self) -> Result<&Repository> {
        self.repository.as_ref().ok_or_else(|| {
            self.error
                .as_ref()
                .map(|error| error.error.clone())
                .unwrap_or_else(|| {
                    PmError::new(
                        ErrorCode::NotInitialized,
                        "project management is not initialized",
                    )
                })
        })
    }

    pub fn refresh(&mut self) -> Result<()> {
        if self.indexed {
            return match self.projected_selection.clone() {
                Some(token) => self.select_projection(&token),
                None => Ok(()),
            };
        }
        let previous_index = self.selected_index().unwrap_or_default();
        let result = self
            .repository()
            .and_then(Repository::issue_query_snapshot)
            .and_then(|snapshot| {
                snapshot
                    .select_indices(&self.filter.query())
                    .map(|visible| (snapshot, visible))
            });
        let (snapshot, visible) = match result {
            Ok(result) => result,
            Err(error) => return self.fail(error, WorkbenchErrorContext::Refresh, None, false),
        };
        self.issues = snapshot.issues().to_vec();
        self.query_snapshot = Some(snapshot);
        self.visible = visible;
        self.reconcile_selection(previous_index);
        // Reloading current records is not an acknowledgement or rebase of a
        // failed mutation. Its captured source and retained draft stay intact.
        if self
            .error
            .as_ref()
            .is_some_and(|error| error.context == WorkbenchErrorContext::Refresh)
        {
            self.error = None;
        }
        self.refresh_detail()
    }

    pub fn refresh_detail(&mut self) -> Result<()> {
        let Some(id) = self.selected_issue.clone() else {
            self.comments = None;
            return Ok(());
        };
        let result = self
            .repository()
            .and_then(|repository| repository.comments(id.as_str()));
        match result {
            Ok(comments) => {
                self.comments = Some((id, comments));
                Ok(())
            }
            Err(error) => self.fail(error, WorkbenchErrorContext::Refresh, None, false),
        }
    }

    pub fn set_filter(&mut self, filter: IssueFilter) -> Result<()> {
        if self.indexed {
            filter.query().validate()?;
            self.filter = filter;
            return Ok(());
        }
        let indices = self
            .query_snapshot
            .as_ref()
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::NotInitialized,
                    "Refresh planning before changing its query",
                )
            })
            .and_then(|snapshot| snapshot.select_indices(&filter.query()));
        let indices = match indices {
            Ok(indices) => indices,
            Err(error) => return self.fail(error, WorkbenchErrorContext::Navigation, None, false),
        };
        let index = self.selected_index().unwrap_or_default();
        self.filter = filter;
        self.visible = indices;
        self.reconcile_selection(index);
        if self
            .error
            .as_ref()
            .is_some_and(|error| error.context == WorkbenchErrorContext::Navigation)
        {
            self.error = None;
        }
        Ok(())
    }

    pub fn select(&mut self, id: &IssueId) -> bool {
        if !self
            .visible
            .iter()
            .any(|index| &self.issues[*index].metadata.id == id)
        {
            return false;
        }
        if self.selected_issue.as_ref() != Some(id) {
            self.selected_issue = Some(id.clone());
            self.view.detail_scroll = 0;
            self.comments = None;
            self.active_draft = None;
        }
        true
    }

    pub fn move_selection(&mut self, delta: isize) {
        if self.visible.is_empty() {
            return;
        }
        let index = self
            .selected_index()
            .unwrap_or_default()
            .saturating_add_signed(delta)
            .min(self.visible.len() - 1);
        let id = self.issues[self.visible[index]].metadata.id.clone();
        self.select(&id);
    }

    fn reconcile_selection(&mut self, previous_index: usize) {
        if self.selected_index().is_some() {
            return;
        }
        let selected = self
            .visible
            .get(previous_index.min(self.visible.len().saturating_sub(1)))
            .map(|index| self.issues[*index].metadata.id.clone());
        if selected != self.selected_issue {
            self.comments = None;
            self.view.detail_scroll = 0;
            if matches!(
                self.active_draft,
                Some(DraftKey::Edit(_) | DraftKey::Comment(_))
            ) {
                self.active_draft = None;
            }
        }
        self.selected_issue = selected;
        self.view.list_offset = self
            .view
            .list_offset
            .min(self.visible.len().saturating_sub(1));
    }

    /// Reopening an existing draft preserves its text and captured source.
    pub fn begin_create(&mut self, input: CreateIssue) -> DraftKey {
        let key = DraftKey::Create;
        self.drafts.entry(key.clone()).or_insert(WorkbenchDraft {
            custom_input: input.fields.get("custom").map(|value| value.to_string()),
            input: DraftInput::Create(input),
            source: None,
            submission: None,
        });
        self.activate_draft(&key);
        key
    }

    pub fn begin_create_from_file(
        &mut self,
        link: SourceLink,
        title: String,
        body: String,
    ) -> Result<DraftKey> {
        link.validate()?;
        let mut input = CreateIssue::new(title, body);
        input.fields.insert("files".into(), json!([link]));
        Ok(self.begin_create(input))
    }

    pub fn begin_edit(&mut self) -> Result<DraftKey> {
        let issue = self.selected_issue().cloned().ok_or_else(no_selection)?;
        let key = DraftKey::Edit(issue.metadata.id);
        self.drafts.entry(key.clone()).or_insert(WorkbenchDraft {
            custom_input: Some(
                serde_json::to_string(&issue.metadata.custom).expect("custom mapping"),
            ),
            input: DraftInput::Edit(UpdateIssue {
                fields: BTreeMap::from([("title".into(), json!(issue.metadata.title))]),
                body: Some(issue.body),
            }),
            source: Some(issue.source),
            submission: None,
        });
        self.activate_draft(&key);
        Ok(key)
    }

    pub fn begin_comment(&mut self, author: String) -> Result<DraftKey> {
        let target = self.selected_target().ok_or_else(no_selection)?;
        let key = DraftKey::Comment(target.issue);
        self.drafts.entry(key.clone()).or_insert(WorkbenchDraft {
            custom_input: None,
            input: DraftInput::Comment {
                author,
                body: String::new(),
            },
            source: Some(target.source),
            submission: None,
        });
        self.activate_draft(&key);
        Ok(key)
    }

    pub fn activate_draft(&mut self, key: &DraftKey) -> bool {
        if !self.drafts.contains_key(key) {
            return false;
        }
        if let DraftKey::Edit(id) | DraftKey::Comment(id) = key {
            self.select(id);
        }
        self.active_draft = Some(key.clone());
        self.view.pane = WorkbenchPane::Detail;
        self.view.detail_scroll = 0;
        true
    }

    pub fn discard_draft(&mut self, key: &DraftKey) {
        self.drafts.remove(key);
        if self.active_draft.as_ref() == Some(key) {
            self.active_draft = None;
        }
        if self.error.as_ref().and_then(|error| error.draft.as_ref()) == Some(key) {
            self.error = None;
        }
    }

    pub fn submit_draft(&mut self, key: &DraftKey) -> Result<MutationReceipt> {
        let result = (|| {
            let draft = self
                .drafts
                .get_mut(key)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "draft was not found"))?;
            if let Some(text) = &draft.custom_input {
                let value: Value = serde_json::from_str(text).map_err(|error| {
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
                match &mut draft.input {
                    DraftInput::Create(input) => {
                        input.fields.insert("custom".into(), value);
                    }
                    DraftInput::Edit(input) => {
                        input.fields.insert("custom".into(), value);
                    }
                    DraftInput::Comment { .. } => {}
                }
            }
            let request = draft.request()?;
            let draft = draft.clone();
            let repository = self.repository()?;
            match (key, &draft.input) {
                (DraftKey::Create, DraftInput::Create(input)) => {
                    repository.create_issue(input, &request)
                }
                (DraftKey::Edit(id), DraftInput::Edit(input)) => repository.mutate_issue(
                    id.as_str(),
                    draft.source.as_ref(),
                    &IssueMutation::Update {
                        input: input.clone(),
                    },
                    &request,
                ),
                (DraftKey::Comment(id), DraftInput::Comment { author, body }) => repository
                    .mutate_issue(
                        id.as_str(),
                        draft.source.as_ref(),
                        &IssueMutation::Comment {
                            author: author.clone(),
                            body: body.clone(),
                        },
                        &request,
                    ),
                _ => Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "draft fields do not match the draft kind",
                )),
            }
        })();
        match result {
            Ok(receipt) => {
                self.discard_draft(key);
                self.after_commit(&receipt);
                Ok(receipt)
            }
            Err(error) => self.fail(
                error,
                WorkbenchErrorContext::Mutation,
                Some(key.clone()),
                false,
            ),
        }
    }

    /// The target is captured when the host opens an action dialog, not replaced
    /// by a newer selection or refresh when the user submits it.
    pub fn perform(
        &mut self,
        target: MutationTarget,
        action: IssueAction,
    ) -> Result<MutationReceipt> {
        if self.repository()?.identity() != &target.repository {
            return self.fail(
                PmError::new(
                    ErrorCode::StaleSource,
                    "the action belongs to a different repository",
                ),
                WorkbenchErrorContext::Mutation,
                None,
                false,
            );
        }
        let request = self
            .pending_action
            .as_ref()
            .filter(|pending| pending.target == target && pending.action == action)
            .map_or_else(RequestId::new, |pending| pending.request.clone());
        self.pending_action = Some(PendingAction {
            target,
            action,
            request,
        });
        self.retry_pending_action()
    }

    pub fn retry_pending_action(&mut self) -> Result<MutationReceipt> {
        let pending = self
            .pending_action
            .clone()
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "there is no pending action"))?;
        let result = self.repository().and_then(|repository| {
            repository.mutate_issue(
                pending.target.issue.as_str(),
                Some(&pending.target.source),
                &pending.action.mutation(),
                &pending.request,
            )
        });
        match result {
            Ok(receipt) => {
                self.pending_action = None;
                self.after_commit(&receipt);
                Ok(receipt)
            }
            Err(error) => self.fail(error, WorkbenchErrorContext::Mutation, None, false),
        }
    }

    fn after_commit(&mut self, receipt: &MutationReceipt) {
        self.last_receipt = Some(receipt.clone());
        self.error = None;
        let result = receipt.result.get("issue").unwrap_or(&receipt.result);
        let selected = result
            .get("metadata")
            .and_then(|metadata| metadata.get("id"))
            .and_then(Value::as_str)
            .and_then(|id| id.parse().ok());
        if let Some(selected) = selected {
            self.selected_issue = Some(selected);
        }
        if self.indexed {
            self.projected_selection = None;
            self.issues.clear();
            self.visible.clear();
            self.comments = None;
            return;
        }
        if self.refresh().is_err()
            && let Some(error) = &mut self.error
        {
            error.after_commit = true;
        }
    }

    pub fn return_context(&self) -> PlanningReturnContext {
        PlanningReturnContext {
            selected_issue: self.selected_issue.clone(),
            filter: self.filter.clone(),
            view: self.view.clone(),
            active_draft: self.active_draft.clone(),
        }
    }

    pub fn restore_context(&mut self, context: &PlanningReturnContext) -> Result<()> {
        self.set_filter(context.filter.clone())?;
        self.selected_issue = context.selected_issue.clone();
        self.view = context.view.clone();
        if !self.indexed {
            self.reconcile_selection(context.view.list_offset);
        }
        self.active_draft = context
            .active_draft
            .as_ref()
            .filter(|key| self.drafts.contains_key(*key))
            .cloned();
        Ok(())
    }

    pub fn navigate_file<R>(&mut self, index: usize, review: R) -> Result<IssueNavigation<R>> {
        let target = self
            .selected_issue()
            .and_then(|issue| issue.metadata.files.get(index))
            .cloned()
            .map(IssueReviewTarget::File)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "issue file link was not found"));
        self.navigation(target, review)
    }

    pub fn navigate_commit<R>(&mut self, index: usize, review: R) -> Result<IssueNavigation<R>> {
        let target = self
            .selected_issue()
            .and_then(|issue| issue.metadata.commits.get(index))
            .cloned()
            .map(IssueReviewTarget::Commit)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "issue commit link was not found"));
        self.navigation(target, review)
    }

    fn navigation<R>(
        &mut self,
        target: Result<IssueReviewTarget>,
        review: R,
    ) -> Result<IssueNavigation<R>> {
        let result: Result<IssueNavigation<R>> = (|| {
            let repository = self.repository()?;
            Ok(IssueNavigation {
                repository: repository.identity().clone(),
                repository_root: repository
                    .root()
                    .parent()
                    .unwrap_or(repository.root())
                    .to_owned(),
                issue: self.selected_id().cloned().ok_or_else(no_selection)?,
                target: target?,
                planning: self.return_context(),
                review,
            })
        })();
        result.inspect_err(|error| {
            self.error = Some(WorkbenchError {
                error: error.clone(),
                context: WorkbenchErrorContext::Navigation,
                draft: None,
                after_commit: false,
            });
        })
    }

    fn fail<T>(
        &mut self,
        error: PmError,
        context: WorkbenchErrorContext,
        draft: Option<DraftKey>,
        after_commit: bool,
    ) -> Result<T> {
        self.error = Some(WorkbenchError {
            error: error.clone(),
            context,
            draft,
            after_commit,
        });
        Err(error)
    }
}

fn no_selection() -> PmError {
    PmError::new(ErrorCode::NotFound, "select an issue first")
}
fn update_field(field: &str, value: Value) -> IssueMutation {
    IssueMutation::Update {
        input: UpdateIssue {
            fields: BTreeMap::from([(field.into(), value)]),
            body: None,
        },
    }
}
