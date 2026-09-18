//! Issue-scoped, source-bound continuity. Reads and mutations belong to workdeck-pm.
use super::input::{FormAction, FormKind, TextField, WorkbenchForm};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::BTreeMap;
use workdeck_pm::{transactions::MutationReceipt, *};

pub(super) const DEFAULT_BUDGET: usize = 64 * 1024;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Section {
    #[default]
    Context,
    Next,
    Questions,
    Handoffs,
    Checks,
    Sources,
    Claims,
}
impl Section {
    pub fn index(self) -> usize {
        match self {
            Self::Context => 0,
            Self::Next => 1,
            Self::Questions => 2,
            Self::Handoffs => 3,
            Self::Checks => 4,
            Self::Sources => 5,
            Self::Claims => 6,
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum RowTarget {
    Citation(ContextCitation),
    Action(SuggestedAction),
    Ready(NextIssueCandidate),
    ReadyList,
    Question(Box<(QuestionRecord, QuestionApplicability)>),
    Handoff,
    CheckRun(Box<ContextCheckRun>),
}
#[derive(Debug, Clone)]
pub(super) struct Row {
    pub id: String,
    pub title: String,
    pub body: String,
    pub target: Option<RowTarget>,
}
#[derive(Debug, Clone)]
pub(super) struct ContextNavigation {
    pub anchor: ContextAnchor,
    pub budget: usize,
    pub citation: ContextCitation,
}
#[derive(Debug, Clone)]
pub(super) enum ContextEffect {
    Navigate(Box<ContextNavigation>),
    Issue(IssueId),
}
#[derive(Debug)]
enum DraftInput {
    Question {
        subjects: Vec<QuestionSubject>,
    },
    Answer {
        question: QuestionRecord,
    },
    Supersede {
        question: QuestionRecord,
        replacements: Vec<QuestionRecord>,
    },
    Handoff {
        anchor: ContextAnchor,
        evidence: Vec<HandoffEvidenceRef>,
        questions: Vec<QuestionId>,
    },
}
#[derive(Debug)]
struct Draft {
    input: DraftInput,
    form: WorkbenchForm,
    request: Option<RequestId>,
}
#[derive(Debug, Default)]
pub(super) struct TaskState {
    pub review: super::review_authority::ReviewAssessment,
    pub packet: Option<ContextPacket>,
    pub section: Section,
    pub selected: [Option<String>; 7],
    pub scroll: [u16; 7],
    pub offset: [usize; 7],
    pub ready: Option<NextIssueSelection>,
    pub ready_visible: bool,
    drafts: BTreeMap<String, Draft>,
    active: Option<String>,
    pub error: Option<PmError>,
    pub receipt: Option<MutationReceipt>,
}
#[derive(Debug)]
pub(super) struct ContextWorkspace {
    pub checks: super::checks_workspace::ChecksWorkspace,
    pub sources: super::sources_workspace::SourcesWorkspace,
    pub claims: super::claims_workspace::ClaimsWorkspace,
    pub source_actions: super::source_actions::SourceActions,
    repository: Option<Repository>,
    pub author: String,
    pub current: Option<IssueId>,
    pub states: BTreeMap<String, TaskState>,
    pub budget: usize,
}
impl ContextWorkspace {
    pub fn new(repository: Option<Repository>, author: String) -> Self {
        let checks =
            super::checks_workspace::ChecksWorkspace::new(repository.clone(), author.clone());
        let claims = super::claims_workspace::ClaimsWorkspace::new(
            repository.clone(),
            author.clone(),
            checks.signal.clone(),
        );
        let source_actions =
            super::source_actions::SourceActions::new(repository.clone(), checks.signal.clone());
        Self {
            source_actions,
            sources: super::sources_workspace::SourcesWorkspace::new(
                repository
                    .as_ref()
                    .and_then(|repository| repository.root().parent())
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .into(),
                repository
                    .as_ref()
                    .map(|repository| repository.identity().clone()),
            ),
            checks,
            claims,
            repository,
            author,
            current: None,
            states: BTreeMap::new(),
            budget: DEFAULT_BUDGET,
        }
    }
    pub fn bind_if_missing(&mut self, repository: Option<Repository>) {
        self.checks.bind_if_missing(repository.clone());
        self.claims.bind_if_missing(repository.clone());
        self.source_actions.bind_if_missing(repository.clone());
        if self.repository.is_none() {
            self.repository = repository;
        }
    }
    pub fn repository(&self) -> Result<&Repository> {
        self.repository.as_ref().ok_or_else(|| {
            PmError::new(
                ErrorCode::NotInitialized,
                "Run workdeck init or finish migration, then press r to inspect task context",
            )
        })
    }
    fn state_key(&self) -> String {
        self.current
            .as_ref()
            .map(ToString::to_string)
            .unwrap_or_default()
    }
    pub fn state(&self) -> Option<&TaskState> {
        self.states.get(&self.state_key())
    }
    pub fn state_mut(&mut self) -> &mut TaskState {
        self.states.entry(self.state_key()).or_default()
    }
    pub fn form(&self) -> Option<&WorkbenchForm> {
        let state = self.state()?;
        if state.review.visible {
            return state.review.form.as_ref();
        }
        state
            .active
            .as_ref()
            .and_then(|key| state.drafts.get(key))
            .map(|draft| &draft.form)
    }
    pub fn open(&mut self, issue: Option<IssueId>) {
        self.current = issue;
        if self.current.is_none() {
            let state = self.state_mut();
            state.section = Section::Next;
            state.ready_visible = true;
        }
        // An inspected packet and drafts survive tab changes. Explicit refresh is deliberate.
        if self
            .state()
            .is_none_or(|state| state.packet.is_none() && state.ready.is_none())
            && let Err(error) = self.refresh()
        {
            self.state_mut().error = Some(error);
        }
        if self
            .state()
            .is_some_and(|state| state.section == Section::Checks)
        {
            self.checks.open(self.current.clone());
        }
    }
    pub fn refresh(&mut self) -> Result<()> {
        self.state_mut().review.report = None;
        let repository = self.repository()?.clone();
        let ready = self
            .state()
            .is_some_and(|state| state.section == Section::Next && state.ready_visible);
        if ready || self.current.is_none() {
            let result = repository.next_issue(&NextIssueRequest::default())?;
            let state = self.state_mut();
            state.ready = Some(result);
            state.error = None;
        } else if let Some(issue) = &self.current {
            let packet = repository.context(&ContextRequest::new(issue.as_str(), self.budget))?;
            if packet.anchor.repository != *repository.identity() || packet.anchor.issue != *issue {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "Context belongs to a different repository or issue",
                ));
            }
            let state = self.state_mut();
            state.packet = Some(packet);
            state.error = None;
        }
        self.refresh_review_authentication()?;
        self.keep_selection();
        Ok(())
    }
    fn keep_selection(&mut self) {
        let rows = self.rows();
        let state = self.state_mut();
        let index = state.section.index();
        if !rows
            .iter()
            .any(|row| Some(&row.id) == state.selected[index].as_ref())
        {
            state.selected[index] = rows.first().map(|row| row.id.clone());
        }
    }
    pub fn selected_row(&self) -> Option<Row> {
        let state = self.state()?;
        let rows = self.rows();
        rows.iter()
            .find(|row| Some(&row.id) == state.selected[state.section.index()].as_ref())
            .or_else(|| rows.first())
            .cloned()
    }
    fn anchor(&self) -> Result<ContextAnchor> {
        self.state()
            .and_then(|state| state.packet.as_ref())
            .map(|packet| packet.anchor.clone())
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::NotFound,
                    "Select an issue and refresh its context first",
                )
            })
    }
    fn insert_draft(
        &mut self,
        key: String,
        input: DraftInput,
        title: &str,
        fields: Vec<TextField>,
    ) {
        let state = self.state_mut();
        state.drafts.entry(key.clone()).or_insert_with(|| Draft {
            input,
            form: WorkbenchForm::new(FormKind::Planning, title, fields),
            request: None,
        });
        state.active = Some(key);
    }
    pub fn begin_question(&mut self) -> Result<()> {
        let anchor = self.anchor()?;
        self.insert_draft(
            "question:new".into(),
            DraftInput::Question {
                subjects: vec![QuestionSubject {
                    subject: SubjectRef::Issue(anchor.issue),
                    source: anchor.issue_source,
                }],
            },
            "Create question",
            vec![
                TextField::new("Author", self.author.clone(), false),
                TextField::new("Question", String::new(), true),
                TextField::new("Blocks implementation (true/false)", "false".into(), false),
            ],
        );
        Ok(())
    }
    fn begin_answer(&mut self, supersede: bool) -> Result<()> {
        let Some(Row {
            target: Some(RowTarget::Question(record)),
            ..
        }) = self.selected_row()
        else {
            return Err(PmError::new(ErrorCode::NotFound, "Select a question first"));
        };
        let (question, applicability) = *record;
        let key = format!(
            "{}:{}",
            if supersede { "supersede" } else { "answer" },
            question.metadata.id
        );
        if self
            .state()
            .is_some_and(|state| state.drafts.contains_key(&key))
        {
            self.state_mut().active = Some(key);
            return Ok(());
        }
        let allowed = if supersede {
            &applicability.supersede
        } else {
            &applicability.answer
        };
        if !allowed.allowed {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                allowed
                    .reasons
                    .iter()
                    .map(|reason| reason.message.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            ));
        }
        let fields = vec![
            TextField::new("Author", self.author.clone(), false),
            TextField::new(
                if supersede { "Reason" } else { "Answer" },
                String::new(),
                true,
            ),
        ];
        if supersede {
            let replacements = self
                .rows()
                .into_iter()
                .filter_map(|row| match row.target {
                    Some(RowTarget::Question(pair))
                        if pair.0.metadata.id != question.metadata.id =>
                    {
                        Some(pair.0)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            let first = replacements.first().ok_or_else(|| PmError::new(ErrorCode::NotFound,
                "Create and inspect a replacement question first; refresh or increase the packet budget if it was omitted"))?;
            let mut fields = fields;
            fields.push(TextField::new(
                "Inspected replacement question ID",
                first.metadata.id.to_string(),
                false,
            ));
            self.insert_draft(
                key,
                DraftInput::Supersede {
                    question,
                    replacements,
                },
                "Supersede question",
                fields,
            );
        } else {
            self.insert_draft(
                key,
                DraftInput::Answer { question },
                "Answer question",
                fields,
            );
        }
        Ok(())
    }
    pub fn begin_handoff(&mut self) -> Result<()> {
        let anchor = self.anchor()?;
        let mut evidence = Vec::new();
        let mut questions = Vec::new();
        if let Some(packet) = self.state().and_then(|state| state.packet.as_ref()) {
            for entry in packet.sections.iter().flat_map(|section| &section.entries) {
                match &entry.content {
                    ContextContent::Evidence { record, .. } => evidence.push(HandoffEvidenceRef {
                        id: record.reference.id.clone(),
                        content: record.content.clone(),
                    }),
                    ContextContent::Question { record, .. } => {
                        questions.push(record.metadata.id.clone())
                    }
                    _ => {}
                }
            }
        }
        self.insert_draft(
            "handoff:new".into(),
            DraftInput::Handoff {
                anchor,
                evidence,
                questions,
            },
            "Create handoff · declared continuity",
            vec![
                TextField::new("Author", self.author.clone(), false),
                TextField::new("Summary", String::new(), true),
                TextField::new("Attempted (one per line)", String::new(), true),
                TextField::new("Uncertainties (one per line)", String::new(), true),
                TextField::new("Next steps (one per line)", String::new(), true),
                TextField::new("Inspected evidence IDs (one per line)", String::new(), true),
                TextField::new("Inspected question IDs (one per line)", String::new(), true),
                TextField::new("Pending request IDs (one per line)", String::new(), true),
            ],
        );
        Ok(())
    }
    fn submit(&mut self) -> Result<()> {
        let repository = self.repository()?.clone();
        let state = self.state_mut();
        let key = state
            .active
            .clone()
            .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "No context draft is open"))?;
        let draft = state.drafts.get_mut(&key).expect("active draft exists");
        let value = |index: usize| draft.form.fields[index].value.clone();
        let actor = value(0);
        let body = value(1);
        // Input parsing does not re-read or replace captured subject/record/anchor pins.
        let mutation = match &draft.input {
            DraftInput::Question { subjects } => PendingMutation::Question(CreateQuestion {
                actor,
                body,
                subjects: subjects.clone(),
                requirements: Vec::new(),
                blocks_work: value(2)
                    .parse::<bool>()
                    .map_err(|_| invalid("Blocks implementation must be true or false"))?,
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            }),
            DraftInput::Answer { question } => PendingMutation::MutateQuestion(
                question.metadata.id.clone(),
                question.source.clone(),
                QuestionMutation::Answer {
                    actor,
                    body,
                    decision_refs: Vec::new(),
                },
            ),
            DraftInput::Supersede {
                question,
                replacements,
            } => {
                let id = value(2);
                let replacement = replacements.iter().find(|record| record.metadata.id.as_str() == id.trim())
                    .ok_or_else(|| invalid("Select a replacement from the inspected questions; discard this draft to capture a different source"))?;
                PendingMutation::MutateQuestion(
                    question.metadata.id.clone(),
                    question.source.clone(),
                    QuestionMutation::Supersede {
                        actor,
                        reason: body,
                        replacement: replacement.metadata.id.clone(),
                        replacement_source: replacement.source.clone(),
                    },
                )
            }
            DraftInput::Handoff {
                anchor,
                evidence,
                questions,
            } => {
                let evidence_refs = lines(&value(5))
                    .into_iter()
                    .map(|id| {
                        evidence
                            .iter()
                            .find(|item| item.id.as_str() == id)
                            .cloned()
                            .ok_or_else(|| {
                                invalid("Evidence must be included in the inspected packet")
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let selected_questions = lines(&value(6))
                    .into_iter()
                    .map(|id| {
                        questions
                            .iter()
                            .find(|item| item.as_str() == id)
                            .cloned()
                            .ok_or_else(|| {
                                invalid("Question must be included in the inspected packet")
                            })
                    })
                    .collect::<Result<Vec<_>>>()?;
                let pending_operations = lines(&value(7))
                    .into_iter()
                    .map(|id| {
                        Ok(HandoffOperationRef {
                            request_id: id.parse()?,
                            operation_id: None,
                            receipt_content: None,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                PendingMutation::Handoff(Box::new(CreateHandoff {
                    actor,
                    anchor: anchor.clone(),
                    body,
                    attempted: lines(&value(2)),
                    uncertainties: lines(&value(3)),
                    next_steps: lines(&value(4)),
                    evidence_refs,
                    questions: selected_questions,
                    pending_operations,
                    custom: BTreeMap::new(),
                    extra: BTreeMap::new(),
                }))
            }
        };
        let request = draft.request.get_or_insert_with(RequestId::new).clone();
        let receipt = mutation.apply(&repository, &request)?;
        // Keep the acknowledgement before any subsequent read, including decoding/display.
        state.receipt = Some(receipt);
        state.drafts.remove(&key);
        state.active = None;
        state.error = None;
        if let Err(mut error) = self.refresh() {
            error.message = format!("Saved; refresh failed: {}", error.message);
            self.state_mut().error = Some(error);
        }
        Ok(())
    }
    pub fn paste(&mut self, text: &str) -> bool {
        if self.state().is_some_and(|s| s.review.visible) {
            let review = &mut self.state_mut().review;
            review.edited();
            if let Some(form) = &mut review.form
                && let Some(field) = form.fields.get_mut(form.selected)
            {
                field.insert(text);
            }
            return true;
        }
        if self
            .state()
            .is_some_and(|state| state.section == Section::Claims)
        {
            return self.claims.paste(text);
        }
        if self
            .state()
            .is_some_and(|state| state.section == Section::Sources)
        {
            return if self.source_actions.visible {
                self.source_actions.paste(text)
            } else {
                self.sources.paste(text)
            };
        }
        let state = self.state_mut();
        if let Some(draft) = state
            .active
            .as_ref()
            .and_then(|key| state.drafts.get_mut(key))
            && let Some(field) = draft.form.fields.get_mut(draft.form.selected)
        {
            field.insert(text);
        }
        true
    }
    pub fn key(&mut self, key: KeyEvent) -> (bool, Option<ContextEffect>) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return (false, None);
        }
        if self.state().is_some_and(|s| s.review.visible) {
            if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('d') {
                self.state_mut().review = Default::default();
                self.state_mut().error = None;
                self.keep_selection();
                return (true, None);
            }
            let review = &mut self.state_mut().review;
            let before = review
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .map(|f| f.value.clone())
                .collect::<Vec<_>>();
            let action = review.form.as_mut().unwrap().key(key);
            if review
                .form
                .as_ref()
                .unwrap()
                .fields
                .iter()
                .map(|f| &f.value)
                .ne(before.iter())
            {
                review.edited();
            }
            match action {
                FormAction::Submit => {
                    if let Err(error) = self.submit_review_authentication() {
                        self.state_mut().error = Some(error);
                    }
                }
                FormAction::Close => self.state_mut().review.visible = false,
                FormAction::Edited => {}
            }
            return (true, None);
        }
        if self
            .state()
            .is_some_and(|state| state.section == Section::Sources)
            && self.source_actions.visible
            && (self.source_actions.form().is_some()
                || !matches!(key.code, KeyCode::Char('1'..='5' | '7')))
        {
            return (self.source_actions.key(key), None);
        }
        if self
            .state()
            .is_some_and(|state| state.section == Section::Sources)
            && (self.sources.proposal_form.is_some()
                || !matches!(key.code, KeyCode::Char('1'..='5' | '7')))
        {
            if self.sources.proposal_form.is_none()
                && key.modifiers.is_empty()
                && key.code == KeyCode::Char('o')
            {
                self.source_actions.visible = true;
                return (true, None);
            }
            return (self.sources.key(key), None);
        }
        if self
            .state()
            .is_some_and(|state| state.section == Section::Claims)
            && (self.claims.form().is_some() || !matches!(key.code, KeyCode::Char('1'..='6')))
        {
            return (self.claims.key(key), None);
        }
        if key.modifiers == KeyModifiers::CONTROL && key.code == KeyCode::Char('d') {
            let state = self.state_mut();
            if let Some(key) = state.active.take() {
                state.drafts.remove(&key);
            }
            state.error = None;
            return (true, None);
        }
        let state = self.state_mut();
        if let Some(draft) = state
            .active
            .as_ref()
            .and_then(|key| state.drafts.get_mut(key))
        {
            let action = draft.form.key(key);
            match action {
                FormAction::Submit => {
                    if let Err(error) = self.submit() {
                        self.state_mut().error = Some(error);
                    }
                }
                FormAction::Close => self.state_mut().active = None,
                FormAction::Edited => {}
            }
            return (true, None);
        }
        if !key.modifiers.is_empty() {
            return (false, None);
        }
        if self
            .state()
            .is_some_and(|state| state.section == Section::Checks)
            && !matches!(key.code, KeyCode::Char('1'..='4' | '6' | '7'))
        {
            return (self.checks.key(key), None);
        }
        let result = match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return (false, None),
            KeyCode::Char(number @ '1'..='7') => {
                let from_sources = self
                    .state()
                    .is_some_and(|state| state.section == Section::Sources);
                if number == '2' && self.current.is_some() {
                    self.state_mut().ready_visible = false;
                }
                self.state_mut().section = [
                    Section::Context,
                    Section::Next,
                    Section::Questions,
                    Section::Handoffs,
                    Section::Checks,
                    Section::Sources,
                    Section::Claims,
                ][number as usize - '1' as usize];
                if number == '5' {
                    self.checks.open(self.current.clone());
                    return (true, None);
                }
                if number == '6' {
                    self.sources.open();
                    return (true, None);
                }
                if number == '7' {
                    if from_sources {
                        let issue = self
                            .sources
                            .state()
                            .and_then(|state| state.selected.clone());
                        let inspected = self.sources.selected_claim();
                        self.claims.open(issue, Some(inspected));
                    } else {
                        self.claims.open(self.current.clone(), None);
                    }
                    return (true, None);
                }
                if self.current.is_some()
                    && self.state().is_some_and(|state| state.packet.is_none())
                {
                    self.state_mut().ready_visible = false;
                    self.refresh().map(|()| None)
                } else {
                    self.keep_selection();
                    Ok(None)
                }
            }
            KeyCode::Char('v') if self.state().is_some_and(|s| s.section == Section::Context) => {
                self.begin_review_authentication().map(|()| None)
            }
            KeyCode::Char('r') => self.refresh().map(|()| None),
            KeyCode::Char('+') | KeyCode::Char('-') => {
                let previous = self.budget;
                self.budget = if key.code == KeyCode::Char('+') {
                    self.budget.saturating_mul(2).min(MAX_CONTEXT_BUDGET_BYTES)
                } else {
                    (self.budget / 2).max(4096)
                };
                let result = self.refresh().map(|()| None);
                if result.is_err() {
                    self.budget = previous;
                }
                result
            }
            KeyCode::Char('w')
                if self
                    .state()
                    .is_some_and(|state| state.section == Section::Next) =>
            {
                self.open_ready().map(|()| None)
            }
            KeyCode::Char('n') => match self.state().map(|state| state.section) {
                Some(Section::Questions) => self.begin_question().map(|()| None),
                Some(Section::Handoffs) => self.begin_handoff().map(|()| None),
                _ => Ok(None),
            },
            KeyCode::Char('a')
                if self
                    .state()
                    .is_some_and(|state| state.section == Section::Questions) =>
            {
                self.begin_answer(false).map(|()| None)
            }
            KeyCode::Char('s')
                if self
                    .state()
                    .is_some_and(|state| state.section == Section::Questions) =>
            {
                self.begin_answer(true).map(|()| None)
            }
            KeyCode::Enter => self.activate(),
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Down | KeyCode::Char('j') => {
                let rows = self.rows();
                let state = self.state_mut();
                let section = state.section.index();
                let at = rows
                    .iter()
                    .position(|row| Some(&row.id) == state.selected[section].as_ref())
                    .unwrap_or(0);
                let next = if matches!(key.code, KeyCode::Up | KeyCode::Char('k')) {
                    at.saturating_sub(1)
                } else {
                    at.saturating_add(1).min(rows.len().saturating_sub(1))
                };
                state.selected[section] = rows.get(next).map(|row| row.id.clone());
                state.scroll[section] = 0;
                Ok(None)
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                let state = self.state_mut();
                let scroll = &mut state.scroll[state.section.index()];
                *scroll = if key.code == KeyCode::PageUp {
                    scroll.saturating_sub(10)
                } else {
                    scroll.saturating_add(10)
                };
                Ok(None)
            }
            KeyCode::Char(']') if self.state().is_some_and(|state| state.ready_visible) => {
                self.next_ready_page().map(|()| None)
            }
            _ => Ok(None),
        };
        match result {
            Ok(effect) => (true, effect),
            Err(error) => {
                self.state_mut().error = Some(error);
                (true, None)
            }
        }
    }
    fn open_ready(&mut self) -> Result<()> {
        let selection = self
            .repository()?
            .next_issue(&NextIssueRequest::default())?;
        let state = self.state_mut();
        state.section = Section::Next;
        state.ready_visible = true;
        state.ready = Some(selection);
        state.error = None;
        self.keep_selection();
        Ok(())
    }
    fn next_ready_page(&mut self) -> Result<()> {
        let cursor = self
            .state()
            .and_then(|state| state.ready.as_ref())
            .and_then(|ready| ready.next_cursor.clone());
        if let Some(cursor) = cursor {
            let selection = self.repository()?.next_issue(&NextIssueRequest {
                cursor: Some(cursor),
                ..NextIssueRequest::default()
            })?;
            self.state_mut().ready = Some(selection);
            self.keep_selection();
        }
        Ok(())
    }
    fn activate(&mut self) -> Result<Option<ContextEffect>> {
        let Some(row) = self.selected_row() else {
            return Ok(None);
        };
        match row.target {
            Some(RowTarget::CheckRun(summary)) => {
                let anchor = self.anchor()?;
                let current = self.repository()?.context(&ContextRequest {
                    issue: anchor.issue.to_string(),
                    budget_bytes: self.budget,
                    as_of: None,
                    expected_context: Some(anchor.fingerprint.clone()),
                })?;
                let present = current.sections.iter().flat_map(|section| &section.entries).any(|entry| {
                    matches!(&entry.content, ContextContent::CheckRun { summary: current } if current == summary.as_ref())
                });
                if !present {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "Inspected check result is no longer in this context packet",
                    ));
                }
                self.checks.open(self.current.clone());
                self.checks
                    .open_result(&summary.id, &summary.intent, summary.result.as_ref())?;
                self.state_mut().section = Section::Checks;
                Ok(None)
            }
            Some(RowTarget::ReadyList) => {
                self.open_ready()?;
                Ok(None)
            }
            Some(RowTarget::Ready(candidate)) => {
                // Opening a candidate inspects its task; it does not start work or claim eligibility.
                let packet = self
                    .repository()?
                    .context(&ContextRequest::new(candidate.issue.as_str(), self.budget))?;
                if packet.anchor.issue != candidate.issue
                    || packet.anchor.issue_source != candidate.source
                {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "Ready-work candidate changed; refresh the selection",
                    ));
                }
                self.current = Some(candidate.issue);
                let state = self.state_mut();
                state.packet = Some(packet);
                state.section = Section::Context;
                state.ready_visible = false;
                state.error = None;
                self.keep_selection();
                Ok(None)
            }
            Some(RowTarget::Citation(citation)) => {
                Ok(Some(ContextEffect::Navigate(Box::new(ContextNavigation {
                    anchor: self.anchor()?,
                    budget: self.budget,
                    citation,
                }))))
            }
            Some(RowTarget::Action(action)) => {
                if !action.available {
                    return Err(PmError::new(ErrorCode::Unsupported, action.explanation));
                }
                let next = self.repository()?.next_actions(&NextActionRequest {
                    issue: action.preconditions.issue.to_string(),
                    expected_context: Some(action.preconditions.context.clone()),
                })?;
                if !next.actions.contains(&action) {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "Suggested action changed; refresh context before continuing",
                    ));
                }
                match action.kind {
                    NextActionKind::RunChecks => {
                        self.state_mut().section = Section::Checks;
                        self.checks.open(self.current.clone());
                        Ok(None)
                    }
                    NextActionKind::RecordHandoff => {
                        self.state_mut().section = Section::Handoffs;
                        self.begin_handoff()?;
                        Ok(None)
                    }
                    NextActionKind::ClarifyRequirements => {
                        self.state_mut().section = Section::Questions;
                        self.begin_question()?;
                        Ok(None)
                    }
                    NextActionKind::ResolveQuestion => {
                        if let ContextTarget::Question { id } = action.target {
                            let present = self.state().and_then(|state| state.packet.as_ref()).into_iter()
                                .flat_map(|packet| &packet.sections).flat_map(|section| &section.entries)
                                .any(|entry| matches!(&entry.content, ContextContent::Question { record, .. } if record.metadata.id.as_str() == id));
                            if !present {
                                return Err(PmError::new(
                                    ErrorCode::NotFound,
                                    "The question was omitted; increase the context budget before resolving it",
                                ));
                            }
                            self.state_mut().section = Section::Questions;
                            self.state_mut().selected[Section::Questions.index()] =
                                Some(format!("question:{id}"));
                        }
                        Ok(None)
                    }
                    _ => match action.target {
                        ContextTarget::Issue { id } => Ok(Some(ContextEffect::Issue(id))),
                        target => {
                            let citation = self.state().and_then(|state| state.packet.as_ref()).into_iter().flat_map(|packet| &packet.sections)
                                .flat_map(|section| &section.entries).flat_map(|entry| &entry.citations).find(|citation| citation.target == target).cloned()
                                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "Action source was omitted; increase the context budget before opening it"))?;
                            Ok(Some(ContextEffect::Navigate(Box::new(ContextNavigation {
                                anchor: self.anchor()?,
                                budget: self.budget,
                                citation,
                            }))))
                        }
                    },
                }
            }
            _ => Ok(None),
        }
    }
    pub fn validate_navigation(&self, navigation: &ContextNavigation) -> Result<ContextPacket> {
        let packet = self.repository()?.context(&ContextRequest {
            issue: navigation.anchor.issue.to_string(),
            budget_bytes: navigation.budget,
            as_of: None,
            expected_context: Some(navigation.anchor.fingerprint.clone()),
        })?;
        if packet.anchor != navigation.anchor {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Context source changed before navigation",
            ));
        }
        if !packet
            .sections
            .iter()
            .flat_map(|section| &section.entries)
            .flat_map(|entry| &entry.citations)
            .any(|citation| citation == &navigation.citation)
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Citation is unavailable or changed; refresh or increase the packet budget",
            ));
        }
        Ok(packet)
    }
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn lines(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}
enum PendingMutation {
    Question(CreateQuestion),
    MutateQuestion(QuestionId, SourceToken, QuestionMutation),
    Handoff(Box<CreateHandoff>),
}
impl PendingMutation {
    fn apply(self, repository: &Repository, request: &RequestId) -> Result<MutationReceipt> {
        match self {
            Self::Question(input) => repository.create_question(&input, request),
            Self::MutateQuestion(id, source, mutation) => {
                repository.mutate_question(&id, &source, &mutation, request)
            }
            Self::Handoff(input) => repository.create_handoff(&input, request),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn seed() -> (tempfile::TempDir, Repository, IssueRecord, ContextWorkspace) {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        let issue = serde_json::from_value::<IssueRecord>(
            repository
                .create_issue(
                    &CreateIssue::new("Task", "Original scope"),
                    &RequestId::new(),
                )
                .unwrap()
                .result,
        )
        .unwrap();
        let mut workspace = ContextWorkspace::new(Some(repository.clone()), "local".into());
        workspace.open(Some(issue.metadata.id.clone()));
        assert!(
            workspace.state().unwrap().error.is_none(),
            "{:?}",
            workspace.state().unwrap().error
        );
        (directory, repository, issue, workspace)
    }
    fn field(workspace: &mut ContextWorkspace, index: usize, text: &str) {
        let state = workspace.state_mut();
        let key = state.active.clone().unwrap();
        let field = &mut state.drafts.get_mut(&key).unwrap().form.fields[index];
        field.value = text.into();
        field.cursor = text.chars().count();
    }
    fn key(workspace: &mut ContextWorkspace, code: KeyCode) {
        workspace.key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    fn question(workspace: &mut ContextWorkspace, body: &str) -> QuestionRecord {
        workspace.state_mut().section = Section::Questions;
        workspace.begin_question().unwrap();
        field(workspace, 1, body);
        workspace.submit().unwrap();
        serde_json::from_value::<QuestionMutationResult>(
            workspace
                .state()
                .unwrap()
                .receipt
                .as_ref()
                .unwrap()
                .result
                .clone(),
        )
        .unwrap()
        .question
    }
    fn select_question(workspace: &mut ContextWorkspace, id: &QuestionId) {
        workspace.state_mut().section = Section::Questions;
        workspace.state_mut().selected[Section::Questions.index()] = Some(format!("question:{id}"));
    }
    fn update(repository: &Repository, issue: &IssueId) {
        repository
            .mutate_issue(
                issue.as_str(),
                None,
                &IssueMutation::Update {
                    input: UpdateIssue {
                        fields: BTreeMap::new(),
                        body: Some("Changed requirements".into()),
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    #[test]
    fn question_draft_retains_subject_source_across_refresh_and_stale_failure() {
        let (_directory, repository, issue, mut workspace) = seed();
        workspace.begin_question().unwrap();
        field(&mut workspace, 1, "Which behavior is intended?");
        key(&mut workspace, KeyCode::Esc);
        update(&repository, &issue.metadata.id);
        workspace.refresh().unwrap();
        workspace.begin_question().unwrap();
        assert_eq!(
            workspace.form().unwrap().fields[1].value,
            "Which behavior is intended?"
        );
        assert_eq!(workspace.submit().unwrap_err().code, ErrorCode::StaleSource);
        let request = workspace.state().unwrap().drafts["question:new"]
            .request
            .clone();
        workspace.refresh().unwrap();
        assert_eq!(workspace.submit().unwrap_err().code, ErrorCode::StaleSource);
        assert_eq!(
            workspace.state().unwrap().drafts["question:new"].request,
            request
        );
        assert!(
            repository
                .questions(&QuestionQuery::default())
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn question_creation_lost_ack_retries_original_input_and_receipt() {
        let (_directory, repository, issue, mut workspace) = seed();
        workspace.begin_question().unwrap();
        field(&mut workspace, 1, "Original question");
        let request = RequestId::new();
        workspace
            .state_mut()
            .drafts
            .get_mut("question:new")
            .unwrap()
            .request = Some(request.clone());
        let receipt = repository
            .create_question(
                &CreateQuestion {
                    actor: "local".into(),
                    body: "Original question".into(),
                    subjects: vec![QuestionSubject {
                        subject: SubjectRef::Issue(issue.metadata.id.clone()),
                        source: issue.source.clone(),
                    }],
                    requirements: Vec::new(),
                    blocks_work: false,
                    custom: BTreeMap::new(),
                    extra: BTreeMap::new(),
                },
                &request,
            )
            .unwrap();
        field(&mut workspace, 1, "Changed attempted retry");
        assert_eq!(
            workspace.submit().unwrap_err().code,
            ErrorCode::IdempotencyConflict
        );
        field(&mut workspace, 1, "Original question");
        workspace.submit().unwrap();
        assert_eq!(
            workspace.state().unwrap().receipt.as_ref().unwrap(),
            &receipt
        );
        assert!(workspace.form().is_none());
        assert_eq!(
            repository
                .questions(&QuestionQuery::default())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            repository
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .source,
            issue.source
        );
    }
    #[test]
    fn answered_question_lost_ack_reopens_retained_draft_despite_current_action_state() {
        let (_directory, repository, _issue, mut workspace) = seed();
        let question = question(&mut workspace, "Question awaiting answer");
        select_question(&mut workspace, &question.metadata.id);
        workspace.begin_answer(false).unwrap();
        field(&mut workspace, 1, "Original decision");
        let key = workspace.state().unwrap().active.clone().unwrap();
        let request = RequestId::new();
        workspace.state_mut().drafts.get_mut(&key).unwrap().request = Some(request.clone());
        let receipt = repository
            .mutate_question(
                &question.metadata.id,
                &question.source,
                &QuestionMutation::Answer {
                    actor: "local".into(),
                    body: "Original decision".into(),
                    decision_refs: Vec::new(),
                },
                &request,
            )
            .unwrap();
        self::key(&mut workspace, KeyCode::Esc);
        workspace.refresh().unwrap();
        select_question(&mut workspace, &question.metadata.id);
        workspace.begin_answer(false).unwrap();
        field(&mut workspace, 1, "Changed decision");
        assert_eq!(
            workspace.submit().unwrap_err().code,
            ErrorCode::IdempotencyConflict
        );
        field(&mut workspace, 1, "Original decision");
        workspace.submit().unwrap();
        assert_eq!(
            workspace.state().unwrap().receipt.as_ref().unwrap(),
            &receipt
        );
    }
    #[test]
    fn supersession_uses_inspected_replacement_and_preserves_both_records() {
        let (_directory, repository, issue, mut workspace) = seed();
        let old = question(&mut workspace, "Original uncertain contract");
        let replacement = question(&mut workspace, "Revised question");
        select_question(&mut workspace, &old.metadata.id);
        workspace.begin_answer(true).unwrap();
        field(&mut workspace, 1, "Scope clarified");
        field(&mut workspace, 2, replacement.metadata.id.as_str());
        workspace.submit().unwrap();
        let updated = repository.question(&old.metadata.id).unwrap();
        assert_eq!(updated.metadata.state, QuestionState::Superseded);
        assert_eq!(
            updated.metadata.supersession.unwrap().replacement,
            replacement.metadata.id
        );
        assert_eq!(
            repository.question(&replacement.metadata.id).unwrap(),
            replacement
        );
        assert_eq!(
            repository
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .source,
            issue.source
        );
    }
    #[test]
    fn handoff_authoring_and_later_refresh_keep_stale_summary_explicit() {
        let (_directory, repository, issue, mut workspace) = seed();
        workspace.state_mut().section = Section::Handoffs;
        workspace.begin_handoff().unwrap();
        field(&mut workspace, 1, "Investigated the behavior");
        field(&mut workspace, 2, "Read source\nCompared current tests");
        field(&mut workspace, 3, "Runtime check pending");
        field(&mut workspace, 4, "Add a focused regression");
        workspace.submit().unwrap();
        let record = serde_json::from_value::<HandoffRecord>(
            workspace
                .state()
                .unwrap()
                .receipt
                .as_ref()
                .unwrap()
                .result
                .clone(),
        )
        .unwrap();
        assert_eq!(
            record.metadata.attempted,
            vec!["Read source", "Compared current tests"]
        );
        assert_eq!(repository.handoffs(&issue.metadata.id).unwrap().len(), 1);
        assert_eq!(
            repository
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .source,
            issue.source
        );
        update(&repository, &issue.metadata.id);
        workspace.refresh().unwrap();
        let rows = workspace.rows();
        assert!(
            rows.iter()
                .any(|row| row.title.contains("Stale")
                    && row.body.contains("Investigated the behavior")),
            "{rows:?}"
        );
        assert_eq!(
            repository
                .handoff(&issue.metadata.id, &record.metadata.id)
                .unwrap(),
            record
        );
    }
    #[test]
    fn repository_identity_replacement_keeps_original_context_and_draft() {
        let (_directory, repository, issue, mut workspace) = seed();
        workspace.begin_question().unwrap();
        field(&mut workspace, 1, "Keep this draft");
        let original = workspace.state().unwrap().packet.clone().unwrap();
        let other = tempfile::tempdir().unwrap();
        let replacement = Repository::init(other.path(), "WD").unwrap();
        let config = repository.root().join("config.yml");
        let original_config = std::fs::read(&config).unwrap();
        std::fs::write(
            &config,
            std::fs::read(replacement.root().join("config.yml")).unwrap(),
        )
        .unwrap();
        workspace.bind_if_missing(Some(replacement.clone()));
        assert_eq!(
            workspace.refresh().unwrap_err().code,
            ErrorCode::StaleSource
        );
        assert_eq!(workspace.submit().unwrap_err().code, ErrorCode::StaleSource);
        assert_eq!(workspace.state().unwrap().packet.as_ref(), Some(&original));
        assert_eq!(workspace.form().unwrap().fields[1].value, "Keep this draft");
        assert_eq!(
            workspace.repository().unwrap().identity(),
            repository.identity()
        );
        assert!(
            replacement
                .questions(&QuestionQuery::default())
                .unwrap()
                .is_empty()
        );
        std::fs::write(config, original_config).unwrap();
        workspace.submit().unwrap();
        assert_eq!(
            repository
                .show_issue(issue.metadata.id.as_str())
                .unwrap()
                .source,
            issue.source
        );
    }
    #[test]
    fn ready_list_can_return_to_same_issue_suggestions_and_changed_candidates_stay_unselected() {
        let (_directory, repository, issue, mut workspace) = seed();
        key(&mut workspace, KeyCode::Char('2'));
        key(&mut workspace, KeyCode::Char('w'));
        assert!(workspace.state().unwrap().ready_visible);
        key(&mut workspace, KeyCode::Char('2'));
        assert!(
            !workspace.state().unwrap().ready_visible,
            "2 returns from ready work to this issue's suggestions"
        );
        assert!(
            workspace
                .rows()
                .iter()
                .any(|row| matches!(row.target, Some(RowTarget::Action(_))))
        );
        workspace.open_ready().unwrap();
        let before = workspace.state().unwrap().packet.clone();
        workspace.state_mut().selected[1] = Some(format!("ready:{}", issue.metadata.id));
        update(&repository, &issue.metadata.id);
        assert_eq!(
            workspace.activate().unwrap_err().code,
            ErrorCode::StaleSource
        );
        assert!(workspace.state().unwrap().ready_visible);
        assert_eq!(workspace.state().unwrap().packet, before);
    }
    #[test]
    fn suggested_actions_are_revalidated_and_question_resolution_uses_shared_action_state() {
        let (_directory, repository, issue, mut workspace) = seed();
        let question = question(&mut workspace, "Blocking question");
        workspace.state_mut().section = Section::Next;
        let row = workspace.rows().into_iter().find(|row| matches!(&row.target, Some(RowTarget::Action(action)) if action.kind == NextActionKind::RecordHandoff)).unwrap();
        workspace.state_mut().selected[1] = Some(row.id);
        update(&repository, &issue.metadata.id);
        assert_eq!(
            workspace.activate().unwrap_err().code,
            ErrorCode::StaleSource
        );
        assert!(workspace.form().is_none());
        workspace.refresh().unwrap();
        select_question(&mut workspace, &question.metadata.id);
        assert_eq!(
            workspace.begin_answer(false).unwrap_err().code,
            ErrorCode::PolicyBlocked
        );
        assert!(workspace.form().is_none());
    }
    #[test]
    fn handoff_lost_ack_replays_original_anchor_after_later_issue_edit() {
        let (_directory, repository, issue, mut workspace) = seed();
        workspace.state_mut().section = Section::Handoffs;
        workspace.begin_handoff().unwrap();
        field(&mut workspace, 1, "Original handoff");
        let anchor = workspace.anchor().unwrap();
        let request = RequestId::new();
        workspace
            .state_mut()
            .drafts
            .get_mut("handoff:new")
            .unwrap()
            .request = Some(request.clone());
        let receipt = repository
            .create_handoff(
                &CreateHandoff {
                    actor: "local".into(),
                    anchor,
                    body: "Original handoff".into(),
                    attempted: Vec::new(),
                    uncertainties: Vec::new(),
                    evidence_refs: Vec::new(),
                    questions: Vec::new(),
                    pending_operations: Vec::new(),
                    next_steps: Vec::new(),
                    custom: BTreeMap::new(),
                    extra: BTreeMap::new(),
                },
                &request,
            )
            .unwrap();
        key(&mut workspace, KeyCode::Esc);
        update(&repository, &issue.metadata.id);
        workspace.refresh().unwrap();
        workspace.begin_handoff().unwrap();
        field(&mut workspace, 1, "Changed retry");
        assert_eq!(
            workspace.submit().unwrap_err().code,
            ErrorCode::IdempotencyConflict
        );
        field(&mut workspace, 1, "Original handoff");
        workspace.submit().unwrap();
        assert_eq!(
            workspace.state().unwrap().receipt.as_ref().unwrap(),
            &receipt
        );
        assert_eq!(repository.handoffs(&issue.metadata.id).unwrap().len(), 1);
    }
    #[test]
    fn empty_ready_selection_does_not_manufacture_a_task_or_authority() {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        let mut workspace = ContextWorkspace::new(Some(repository.clone()), "local".into());
        workspace.open(None);
        assert!(
            workspace.state().unwrap().error.is_none(),
            "{:?}",
            workspace.state().unwrap().error
        );
        assert!(
            workspace
                .rows()
                .iter()
                .any(|row| row.body.contains("No eligible work"))
        );
        assert!(workspace.activate().unwrap().is_none());
        assert!(workspace.current.is_none());
        assert!(repository.list_issues().unwrap().is_empty());
    }
}
