use super::input::{FormAction, FormKind, TextField, WorkbenchForm};
use super::{
    DraftInput, DraftKey, IssueAction, IssueReviewTarget, WorkbenchController, WorkbenchPane,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
use workdeck_pm::{CreateIssue, ErrorCode, PmError, Repository};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkbenchTab {
    #[default]
    Review,
    Issues,
    Planning,
    Features,
    Activity,
    MyWork,
    Repository(super::PanelPage),
}

/// Opt-in normal-startup planning source. Specialized review entrypoints leave
/// ReviewOptions.workbench unset and retain their existing geometry and keys.
#[derive(Debug, Clone)]
pub struct WorkbenchOptions {
    pub root: PathBuf,
    pub author: String,
}

impl WorkbenchOptions {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            author: "local".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum ShellEffect {
    ContextNavigate(Box<super::context_workspace::ContextNavigation>),
    Navigate(Box<super::IssueNavigation<()>>),
    Return,
    PreviousCheckout,
    LaunchCheckout,
    PanelNavigate(super::PanelTarget),
    PlanningMembers {
        target: super::PanelTarget,
        scope: workdeck_pm::ArchiveFilter,
    },
    CreateFromReview,
    LinkFromReview,
    CopyReference(String),
}

#[derive(Debug)]
pub(crate) struct WorkbenchShell {
    pub controller: WorkbenchController,
    pub(super) readonly: Option<super::readonly::ReadonlyPlanning>,
    pub(super) checkout_binding: Option<workdeck_pm::registry::RegisteredCheckout>,
    pub(super) context: super::context_workspace::ContextWorkspace,
    pub(super) context_visible: bool,
    pub(super) context_source: Option<(String, workdeck_pm::ContentHash)>,
    pub(super) my_work: Option<super::my_work::MyWorkWorkspace>,
    pub(super) activity: Option<super::indexed_workspace::IndexedWorkspace>,
    pub(super) graph: Option<super::graph_view::GraphView>,
    pub(super) features: super::feature_workspace::FeatureWorkspace,
    pub(super) planning: super::planning_workspace::PlanningWorkspace,
    pub(super) panels: Option<super::panel_controller::RepositoryPanels>,
    pub(super) panel_hits: Vec<(ratatui::layout::Rect, String)>,
    pub(super) panel_list_bounds: Option<ratatui::layout::Rect>,
    pub(super) panel_preview_bounds: Option<ratatui::layout::Rect>,
    pub(super) nav_hits: Vec<(ratatui::layout::Rect, KeyCode)>,
    pub tab: WorkbenchTab,
    pub(super) my_work_return_tab: WorkbenchTab,
    pub(super) checkout_label: Option<String>,
    pub(super) form: Option<WorkbenchForm>,
    pub(super) effect: Option<ShellEffect>,
    pub(super) notice: Option<String>,
    /// Set by the host while an application modal (help, agent skill, menu,
    /// or extension dialog) owns input; queued planning input must not replay
    /// underneath it.
    pub(super) input_paused: bool,
    pub(super) options: WorkbenchOptions,
    pub(super) navigation: Option<super::host::NativeReturnContext>,
    pub(super) source_file: Option<String>,
    pub(super) nav_bounds: ratatui::layout::Rect,
    pub(super) planning_bounds: Option<ratatui::layout::Rect>,
    pub(super) rendered: Option<super::RenderedWorkbench>,
    pub(super) index: Option<super::indexed_workspace::IndexedWorkspace>,
    pub(super) index_rendered: Option<super::indexed_view::RenderedIndex>,
    pub(super) index_attempted: Option<workdeck_pm::projection::ProjectionRowToken>,
    pub(super) index_pending_selection: Option<workdeck_pm::projection::ProjectionRecordKey>,
    pub(super) index_receipt: Option<workdeck_pm::OperationId>,
    pub(super) index_input: Option<super::indexed_shell::PendingInput>,
    pub(super) replaying_index_input: bool,
    refreshed: Instant,
    pub(super) available: bool,
}

impl WorkbenchShell {
    pub(crate) fn attach_run_signal(
        &mut self,
        signal: Option<std::sync::Arc<super::ForegroundRunSignal>>,
    ) {
        self.context.checks.attach_signal(signal);
        self.context
            .source_actions
            .attach_signal(self.context.checks.signal.clone());
        self.context
            .claims
            .attach_signal(self.context.checks.signal.clone());
    }

    pub fn open(options: WorkbenchOptions, empty_review: bool) -> Self {
        let (controller, available) = Self::discover(&options);
        let checkout_binding = workdeck_pm::registry::inspect_checkout(
            "retained",
            &options.root,
            workdeck_pm::SourceSelector::WorkingTree,
        )
        .ok();
        Self::with_controller(
            options,
            empty_review,
            controller,
            available,
            checkout_binding,
        )
    }

    pub(super) fn open_readonly(
        options: WorkbenchOptions,
        binding: workdeck_pm::registry::RegisteredCheckout,
    ) -> Result<Self, String> {
        let readonly = super::readonly::ReadonlyPlanning::new(binding.clone())?;
        let controller = WorkbenchController::unavailable(PmError::new(
            ErrorCode::Unsupported,
            "Read-only planning source has no native authoring controller",
        ));
        let mut shell = Self::with_controller(options, true, controller, false, Some(binding));
        shell.readonly = Some(readonly);
        Ok(shell)
    }

    fn with_controller(
        options: WorkbenchOptions,
        empty_review: bool,
        controller: WorkbenchController,
        available: bool,
        checkout_binding: Option<workdeck_pm::registry::RegisteredCheckout>,
    ) -> Self {
        let planning = super::planning_workspace::PlanningWorkspace::new_indexed(
            controller.repository().ok().cloned(),
        );
        let features = super::feature_workspace::FeatureWorkspace::new_indexed(
            controller.repository().ok().cloned(),
        );
        let mut context = super::context_workspace::ContextWorkspace::new(
            controller.repository().ok().cloned(),
            options.author.clone(),
        );
        context.sources.root = options.root.clone();
        let mut shell = Self {
            controller,
            readonly: None,
            checkout_binding,
            context,
            context_visible: false,
            context_source: None,
            activity: None,
            my_work: None,
            graph: None,
            features,
            planning,
            panels: None,
            panel_hits: Vec::new(),
            panel_list_bounds: None,
            panel_preview_bounds: None,
            nav_hits: Vec::new(),
            available,
            options,
            my_work_return_tab: WorkbenchTab::Issues,
            checkout_label: None,
            tab: if empty_review {
                WorkbenchTab::Issues
            } else {
                WorkbenchTab::Review
            },
            form: None,
            effect: None,
            notice: None,
            input_paused: false,
            navigation: None,
            source_file: None,
            nav_bounds: ratatui::layout::Rect::default(),
            planning_bounds: None,
            rendered: None,
            index: None,
            index_rendered: None,
            index_attempted: None,
            index_pending_selection: None,
            index_receipt: None,
            index_input: None,
            replaying_index_input: false,
            refreshed: Instant::now(),
        };
        shell.poll_index(false);
        shell
    }

    pub fn attach_panels(
        &mut self,
        provider: Option<std::sync::Arc<dyn super::RepositoryPanelProvider>>,
    ) {
        if self.readonly.is_some() {
            self.panels = None;
            return;
        }
        self.panels = provider.and_then(|provider| {
            if provider.source().root != self.options.root {
                self.notice = Some(
                    "Repository panel provider does not match the workbench repository".into(),
                );
                None
            } else {
                Some(super::panel_controller::RepositoryPanels::new(provider))
            }
        });
    }

    fn discover(options: &WorkbenchOptions) -> (WorkbenchController, bool) {
        match Repository::discover(&options.root) {
            Ok(repository) => {
                let mut controller = WorkbenchController::new_indexed(repository);
                let _ = controller.refresh();
                (controller, true)
            }
            Err(error) => (WorkbenchController::unavailable(error), false),
        }
    }

    fn bind_planning_source(&mut self) -> bool {
        if !self.planning.needs_source() {
            return false;
        }
        // Only an unavailable source may be discovered again. Once bound, both
        // controllers retain their original repository identity through errors.
        if !self.available {
            (self.controller, self.available) = Self::discover(&self.options);
            if self.available && self.checkout_binding.is_none() {
                self.checkout_binding = workdeck_pm::registry::inspect_checkout(
                    "retained",
                    &self.options.root,
                    workdeck_pm::SourceSelector::WorkingTree,
                )
                .ok();
            }
        }
        self.planning
            .bind_if_missing(self.controller.repository().ok().cloned());
        !self.planning.needs_source()
    }

    pub fn refresh(&mut self, now: Instant, force: bool) {
        if let Some(readonly) = &mut self.readonly {
            readonly.poll();
            if let Some(my_work) = &mut self.my_work {
                my_work.poll();
            }
            return;
        }
        self.poll_index(false);
        if let Some(my_work) = &mut self.my_work {
            my_work.poll();
        }
        if let Some(activity) = &mut self.activity {
            activity.poll();
        }
        self.features.poll_index();
        self.planning.poll_index();
        if self.tab == WorkbenchTab::Issues && self.context_visible {
            return;
        }
        if self.tab == WorkbenchTab::Issues && self.graph.is_some() {
            return;
        }
        if self.tab == WorkbenchTab::Planning {
            if force || now.duration_since(self.refreshed) >= Duration::from_secs(1) {
                self.refreshed = now;
                let newly_bound = self.bind_planning_source();
                if force || newly_bound {
                    if let Err(error) = self.planning.refresh() {
                        self.planning.error = Some(error);
                    }
                } else {
                    self.planning.auto_refresh();
                }
            }
            return;
        }
        if !force
            && (self.tab != WorkbenchTab::Issues
                || now.duration_since(self.refreshed) < Duration::from_secs(1))
        {
            return;
        }
        self.refreshed = now;
        if !self.available {
            (self.controller, self.available) = Self::discover(&self.options);
            if self.available && self.checkout_binding.is_none() {
                self.checkout_binding = workdeck_pm::registry::inspect_checkout(
                    "retained",
                    &self.options.root,
                    workdeck_pm::SourceSelector::WorkingTree,
                )
                .ok();
            }
        }
        self.poll_index(true);
    }

    pub fn open_draft(&mut self, key: &DraftKey) {
        self.context_visible = false;
        self.controller.activate_draft(key);
        let Some(draft) = self.controller.drafts().get(key) else {
            return;
        };
        let (title, fields) = match &draft.input {
            DraftInput::Create(input) => (
                "Create issue",
                vec![
                    TextField::new("Title", input.title.clone(), false),
                    TextField::new("Body", input.body.clone(), true),
                    TextField::new(
                        "Custom fields (JSON object)",
                        draft.custom_input.clone().unwrap_or_else(|| "{}".into()),
                        true,
                    ),
                ],
            ),
            DraftInput::Edit(input) => (
                "Edit issue",
                vec![
                    TextField::new(
                        "Title",
                        input
                            .fields
                            .get("title")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .into(),
                        false,
                    ),
                    TextField::new("Body", input.body.clone().unwrap_or_default(), true),
                    TextField::new(
                        "Custom fields (JSON object)",
                        draft.custom_input.clone().unwrap_or_else(|| "{}".into()),
                        true,
                    ),
                ],
            ),
            DraftInput::Comment { author, body } => (
                "Comment",
                vec![
                    TextField::new("Author", author.clone(), false),
                    TextField::new("Comment", body.clone(), true),
                ],
            ),
        };
        self.form = Some(WorkbenchForm::new(
            FormKind::Draft(key.clone()),
            title,
            fields,
        ));
        self.tab = WorkbenchTab::Issues;
    }

    fn sync_draft(&mut self) {
        let Some(form) = &self.form else {
            return;
        };
        let FormKind::Draft(key) = &form.kind else {
            return;
        };
        let Some(draft) = self.controller.draft_mut(key) else {
            return;
        };
        if matches!(draft.input, DraftInput::Create(_) | DraftInput::Edit(_)) {
            draft.custom_input = Some(form.fields[2].value.clone());
        }
        match &mut draft.input {
            DraftInput::Create(input) => {
                input.title = form.fields[0].value.clone();
                input.body = form.fields[1].value.clone();
            }
            DraftInput::Edit(input) => {
                input
                    .fields
                    .insert("title".into(), serde_json::json!(form.fields[0].value));
                input.body = Some(form.fields[1].value.clone());
            }
            DraftInput::Comment { author, body } => {
                *author = form.fields[0].value.clone();
                *body = form.fields[1].value.clone();
            }
        }
    }

    fn submit(&mut self) {
        self.sync_draft();
        let Some(form) = self.form.clone() else {
            return;
        };
        let result = match form.kind {
            FormKind::Draft(key) => self.controller.submit_draft(&key).map(|_| ()),
            FormKind::Status(target) => self
                .controller
                .perform(target, IssueAction::Status(form.fields[0].value.clone()))
                .map(|_| ()),
            FormKind::Assign(target) => self
                .controller
                .perform(
                    target,
                    IssueAction::Assign(
                        (!form.fields[0].value.is_empty()).then(|| form.fields[0].value.clone()),
                    ),
                )
                .map(|_| ()),
            FormKind::Priority(target) => workdeck_pm::Priority::parse_input(&form.fields[0].value)
                .and_then(|priority| {
                    self.controller
                        .perform(target, IssueAction::Priority(priority))
                })
                .map(|_| ()),
            FormKind::Labels(target) => self
                .controller
                .perform(
                    target,
                    IssueAction::Labels(
                        form.fields[0]
                            .value
                            .lines()
                            .filter(|label| !label.trim().is_empty())
                            .map(str::to_owned)
                            .collect(),
                    ),
                )
                .map(|_| ()),
            FormKind::LinkFile { target, link } => self
                .controller
                .perform(target, IssueAction::LinkFile(link))
                .map(|_| ()),
            FormKind::Links(links) => form.fields[0]
                .value
                .parse::<usize>()
                .ok()
                .and_then(|number| number.checked_sub(1))
                .and_then(|index| links.get(index).cloned())
                .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "Choose a listed link number"))
                .map(|navigation| self.effect = Some(ShellEffect::Navigate(Box::new(navigation)))),
            FormKind::Planning => unreachable!("planning forms have a separate controller"),
            FormKind::Filter => (|| {
                let mut filter = self.controller.filter().clone();
                filter.query = form.fields[0].value.clone();
                filter.status =
                    (!form.fields[1].value.is_empty()).then(|| form.fields[1].value.clone());
                filter.assignee =
                    (!form.fields[2].value.is_empty()).then(|| form.fields[2].value.clone());
                filter.label =
                    (!form.fields[3].value.is_empty()).then(|| form.fields[3].value.clone());
                filter.project =
                    (!form.fields[4].value.is_empty()).then(|| form.fields[4].value.clone());
                filter.cycle =
                    (!form.fields[5].value.is_empty()).then(|| form.fields[5].value.clone());
                filter.milestone =
                    (!form.fields[6].value.is_empty()).then(|| form.fields[6].value.clone());
                filter.targets = form.fields[7]
                    .value
                    .lines()
                    .map(str::trim)
                    .filter(|line| !line.is_empty())
                    .map(str::to_owned)
                    .collect();
                filter.target_match = match form.fields[8].value.as_str() {
                    "all" => workdeck_pm::TargetMatch::All,
                    "any" => workdeck_pm::TargetMatch::Any,
                    _ => {
                        return Err(PmError::new(
                            ErrorCode::InvalidInput,
                            "Target match must be all or any",
                        ));
                    }
                };
                match form.fields[9].value.as_str() {
                    "active" => {
                        filter.include_archived = false;
                        filter.archived_only = false;
                    }
                    "all" => {
                        filter.include_archived = true;
                        filter.archived_only = false;
                    }
                    "archived" => {
                        filter.include_archived = false;
                        filter.archived_only = true;
                    }
                    _ => {
                        return Err(PmError::new(
                            ErrorCode::InvalidInput,
                            "Archive scope must be active, all, or archived",
                        ));
                    }
                }
                self.controller.set_filter(filter)
            })(),
        };
        match result {
            Ok(()) => {
                self.form = None;
                self.notice = None;
            }
            Err(error) => self.notice = Some(error.message),
        }
    }

    pub fn paste(&mut self, text: &str) -> bool {
        if !matches!(self.tab, WorkbenchTab::Review | WorkbenchTab::MyWork)
            && let Some(source) = &mut self.readonly
        {
            return source.paste(text);
        }
        if self.tab == WorkbenchTab::MyWork {
            return self.my_work.as_mut().is_some_and(|view| view.paste(text));
        }
        if self.tab == WorkbenchTab::Activity {
            return true;
        }
        if self.queue_index_paste(text) {
            return true;
        }
        if self.tab == WorkbenchTab::Issues && self.context_visible {
            return self.context.paste(text);
        }
        if self.tab == WorkbenchTab::Features {
            return self.features.paste(text);
        }
        if self.tab == WorkbenchTab::Planning {
            return self.planning.paste(text);
        }
        if let WorkbenchTab::Repository(page) = self.tab {
            return self.panel_paste(page, text);
        }
        if self.tab != WorkbenchTab::Issues {
            return false;
        }
        if self.graph.is_some() {
            return true;
        }
        let Some(form) = &mut self.form else {
            return false;
        };
        if let Some(field) = form.fields.get_mut(form.selected) {
            field.insert(text);
        }
        self.sync_draft();
        true
    }

    pub fn key(&mut self, key: KeyEvent) -> bool {
        if (matches!(key.code, KeyCode::Esc | KeyCode::F(_))
            || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')))
            && self.index_input.take().is_some()
        {
            self.notice = None;
        }
        if key.code == KeyCode::F(9) && key.modifiers == KeyModifiers::SHIFT {
            if self.tab != WorkbenchTab::MyWork {
                self.my_work_return_tab = self.tab;
            }
            if self.my_work.is_none() {
                self.my_work = Some(super::my_work::MyWorkWorkspace::new(
                    self.options.root.clone(),
                    self.controller.repository().cloned(),
                    self.options.author.clone(),
                ));
            }
            self.tab = WorkbenchTab::MyWork;
            return true;
        }
        if self.readonly.is_some()
            && (self.tab != WorkbenchTab::MyWork || matches!(key.code, KeyCode::F(_)))
        {
            return self.readonly_key(key);
        }
        if key.code == KeyCode::F(12) && key.modifiers == KeyModifiers::SHIFT {
            self.open_activity();
            return true;
        }
        if key.code == KeyCode::F(11) && key.modifiers == KeyModifiers::SHIFT {
            self.open_features();
            return true;
        }
        if key.code == KeyCode::F(4)
            && key.modifiers == KeyModifiers::SHIFT
            && self.tab == WorkbenchTab::Review
        {
            self.effect = Some(ShellEffect::LinkFromReview);
            return true;
        }
        if key.modifiers.is_empty() {
            match key.code {
                KeyCode::F(2) => {
                    self.tab = WorkbenchTab::Review;
                    return true;
                }
                KeyCode::F(3) => {
                    self.effect = Some(ShellEffect::Return);
                    self.tab = WorkbenchTab::Issues;
                    self.refresh(Instant::now(), true);
                    return true;
                }
                KeyCode::F(number @ 11..=12) => {
                    self.effect = Some(ShellEffect::Return);
                    self.tab = WorkbenchTab::Planning;
                    self.planning
                        .bind_if_missing(self.controller.repository().ok().cloned());
                    self.planning.open(if number == 11 {
                        workdeck_pm::PlanningKind::Project
                    } else {
                        workdeck_pm::PlanningKind::Cycle
                    });
                    return true;
                }
                KeyCode::F(number @ 5..=9) if self.panels.is_some() => {
                    let page = [
                        super::PanelPage::Changes,
                        super::PanelPage::Git,
                        super::PanelPage::Files,
                        super::PanelPage::Agents,
                        super::PanelPage::Search,
                    ][usize::from(number - 5)];
                    self.effect = Some(ShellEffect::Return);
                    self.tab = WorkbenchTab::Repository(page);
                    self.panels.as_mut().unwrap().open(page);
                    return true;
                }
                KeyCode::F(4) if self.tab == WorkbenchTab::Review => {
                    self.effect = Some(ShellEffect::CreateFromReview);
                    return true;
                }
                _ => {}
            }
        }
        if self.tab == WorkbenchTab::MyWork {
            if matches!(key.code, KeyCode::Char('b' | 'h'))
                && key.modifiers.is_empty()
                && self.my_work.as_ref().is_none_or(|view| view.form.is_none())
            {
                self.effect = Some(if key.code == KeyCode::Char('h') {
                    ShellEffect::LaunchCheckout
                } else {
                    ShellEffect::PreviousCheckout
                });
                return true;
            }
            if key.code == KeyCode::Esc
                && self.my_work.as_ref().is_none_or(|view| view.form.is_none())
            {
                self.tab = WorkbenchTab::Issues;
                if let Some(source) = &mut self.readonly
                    && let Err(error) = source.select(self.tab, self.planning.kind)
                {
                    source.error = Some(error);
                }
                return true;
            }
            return self.my_work.as_mut().is_some_and(|view| view.key(key));
        }
        if self.tab == WorkbenchTab::Activity {
            if key.code == KeyCode::Esc {
                self.tab = WorkbenchTab::Issues;
                return true;
            }
            return self
                .activity
                .as_mut()
                .is_some_and(|activity| activity.key(key));
        }
        if self.defer_index_key(key) {
            return true;
        }
        if self.tab == WorkbenchTab::Planning {
            if key.modifiers.is_empty() && key.code == KeyCode::Char('r') {
                self.bind_planning_source();
            }
            let (handled, target) = self.planning.handle_key(key);
            if let Some(target) = target {
                self.effect = Some(ShellEffect::PlanningMembers {
                    target,
                    scope: self.planning.issue_scope(),
                });
            }
            return handled;
        }
        if self.tab == WorkbenchTab::Features {
            if key.modifiers.is_empty() && key.code == KeyCode::Char('r') {
                self.bind_planning_source();
                self.features
                    .bind_if_missing(self.controller.repository().ok().cloned());
            }
            return self.features.key(key);
        }
        if let WorkbenchTab::Repository(page) = self.tab {
            return self.panel_key(page, key);
        }
        if self.tab != WorkbenchTab::Issues {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return false;
        }
        if self.context_visible {
            if key.modifiers.is_empty() && key.code == KeyCode::Char('r') {
                self.notice = None;
                self.bind_planning_source();
                self.context
                    .bind_if_missing(self.controller.repository().ok().cloned());
            }
            let (handled, effect) = self.context.key(key);
            if let Some(effect) = effect {
                self.effect = Some(match effect {
                    super::context_workspace::ContextEffect::Navigate(navigation) => {
                        ShellEffect::ContextNavigate(navigation)
                    }
                    super::context_workspace::ContextEffect::Issue(id) => {
                        self.context_visible = false;
                        ShellEffect::PanelNavigate(super::PanelTarget::Issue { id: id.to_string() })
                    }
                });
            }
            if !handled && key.code == KeyCode::Esc {
                self.context_visible = false;
                return true;
            }
            return handled;
        }
        if let Some(graph) = &mut self.graph {
            if key.modifiers.is_empty() && key.code == KeyCode::Esc {
                self.graph = None;
            } else if key.modifiers.is_empty() && key.code == KeyCode::Char('q') {
                return false;
            } else {
                graph.key(key);
            }
            return true;
        }
        if let Some(form) = &mut self.form {
            match form.key(key) {
                FormAction::Submit => self.submit(),
                FormAction::Close => {
                    self.sync_draft();
                    self.form = None;
                }
                FormAction::Edited => self.sync_draft(),
            }
            return true;
        }
        if key.modifiers == KeyModifiers::SHIFT
            && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown)
        {
            return self.index.as_mut().is_some_and(|index| index.key(key));
        }
        if !key.modifiers.is_empty() {
            return false;
        }
        // defer_index_key already polled this event's reader state. Keep its
        // selection/readiness decision intact through action dispatch.
        if let Some(index) = &mut self.index
            && index.key(key)
        {
            self.sync_index_selection();
            return true;
        }
        if matches!(
            key.code,
            KeyCode::Char('e' | 'c' | 's' | 'a' | 'p' | 'l' | 'f' | 'g' | 'b' | 'd' | 'o' | 'y')
        ) && !self.prepare_index_selection()
        {
            return true;
        }
        let result = match key.code {
            KeyCode::Char('i') => {
                self.notice = None;
                self.context
                    .bind_if_missing(self.controller.repository().ok().cloned());
                self.context.open(self.controller.selected_id().cloned());
                self.context_visible = true;
                Ok(())
            }
            KeyCode::Char('q') => return false,
            KeyCode::Char('x') => self.controller.set_filter(super::IssueFilter::default()),
            KeyCode::Up | KeyCode::Char('k') => {
                self.controller.move_selection(-1);
                Ok(())
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.controller.move_selection(1);
                Ok(())
            }
            KeyCode::PageUp => {
                self.controller.view_mut().detail_scroll =
                    self.controller.view().detail_scroll.saturating_sub(10);
                Ok(())
            }
            KeyCode::PageDown => {
                self.controller.view_mut().detail_scroll =
                    self.controller.view().detail_scroll.saturating_add(10);
                Ok(())
            }
            KeyCode::Enter => {
                self.controller.view_mut().pane = WorkbenchPane::Detail;
                self.controller.refresh_detail()
            }
            KeyCode::Esc => {
                self.controller.view_mut().pane = WorkbenchPane::List;
                Ok(())
            }
            KeyCode::Char('r') => {
                self.refresh(Instant::now(), true);
                Ok(())
            }
            KeyCode::Char('b') => (|| {
                let repository = self.controller.repository()?.clone();
                let anchor = self.controller.selected_id().cloned().ok_or_else(|| {
                    PmError::new(ErrorCode::NotFound, "Select an issue to inspect its graph")
                })?;
                self.graph = Some(super::graph_view::GraphView::open(repository, anchor)?);
                Ok(())
            })(),
            KeyCode::Char('v') => {
                self.open_features();
                Ok(())
            }
            KeyCode::Char('n') => {
                let key = self.controller.begin_create(CreateIssue::new("", ""));
                self.open_draft(&key);
                Ok(())
            }
            KeyCode::Char('e') => self
                .controller
                .begin_edit()
                .map(|key| self.open_draft(&key)),
            KeyCode::Char('c') => self
                .controller
                .begin_comment(self.options.author.clone())
                .map(|key| self.open_draft(&key)),
            KeyCode::Char('/') => {
                let f = self.controller.filter();
                self.form = Some(WorkbenchForm::new(
                    FormKind::Filter,
                    "Filter issues",
                    vec![
                        TextField::new("Search", f.query.clone(), false),
                        TextField::new("Status", f.status.clone().unwrap_or_default(), false),
                        TextField::new("Assignee", f.assignee.clone().unwrap_or_default(), false),
                        TextField::new("Label", f.label.clone().unwrap_or_default(), false),
                        TextField::new("Project ID", f.project.clone().unwrap_or_default(), false),
                        TextField::new("Cycle ID", f.cycle.clone().unwrap_or_default(), false),
                        TextField::new(
                            "Milestone ID",
                            f.milestone.clone().unwrap_or_default(),
                            false,
                        ),
                        TextField::new("Target IDs (one per line)", f.targets.join("\n"), true),
                        TextField::new(
                            "Target match (all/any)",
                            match f.target_match {
                                workdeck_pm::TargetMatch::All => "all",
                                workdeck_pm::TargetMatch::Any => "any",
                            }
                            .into(),
                            false,
                        ),
                        TextField::new(
                            "Archive (active/all/archived)",
                            if f.archived_only {
                                "archived"
                            } else if f.include_archived {
                                "all"
                            } else {
                                "active"
                            }
                            .into(),
                            false,
                        ),
                    ],
                ));
                Ok(())
            }
            KeyCode::Char('s' | 'a' | 'p' | 'l') => {
                if let (Some(target), Some(issue)) = (
                    self.controller.selected_target(),
                    self.controller.selected_issue(),
                ) {
                    let (kind, title, label, value) = match key.code {
                        KeyCode::Char('s') => (
                            FormKind::Status(target),
                            "Change status",
                            "Status",
                            issue.metadata.status.clone(),
                        ),
                        KeyCode::Char('a') => (
                            FormKind::Assign(target),
                            "Assign issue",
                            "Assignee (empty clears)",
                            issue.metadata.assignee.clone().unwrap_or_default(),
                        ),
                        KeyCode::Char('p') => (
                            FormKind::Priority(target),
                            "Change priority",
                            "Priority (none, low, medium, high, urgent)",
                            format!("{:?}", issue.metadata.priority).to_ascii_lowercase(),
                        ),
                        _ => (
                            FormKind::Labels(target),
                            "Edit labels",
                            "Labels (one per line; empty clears)",
                            issue.metadata.labels.join("\n"),
                        ),
                    };
                    self.form = Some(WorkbenchForm::new(
                        kind,
                        title,
                        vec![TextField::new(label, value, key.code == KeyCode::Char('l'))],
                    ));
                    Ok(())
                } else {
                    Err(PmError::new(ErrorCode::NotFound, "Select an issue first"))
                }
            }
            KeyCode::Char('d') | KeyCode::Char('o') => {
                if let Some(target) = self.controller.selected_target() {
                    self.controller
                        .perform(
                            target,
                            if key.code == KeyCode::Char('d') {
                                IssueAction::Complete
                            } else {
                                IssueAction::Reopen
                            },
                        )
                        .map(|_| ())
                } else {
                    Err(PmError::new(ErrorCode::NotFound, "Select an issue first"))
                }
            }
            KeyCode::Char('D') => {
                if let Some((key, _)) = self.controller.active_draft() {
                    let key = key.clone();
                    self.controller.discard_draft(&key);
                }
                Ok(())
            }
            KeyCode::Char('f') => self.choose_link(false),
            KeyCode::Char('g') => self.choose_link(true),
            KeyCode::Char('y') => {
                if let Some(issue) = self.controller.selected_id() {
                    self.effect = Some(ShellEffect::CopyReference(issue.to_string()));
                    Ok(())
                } else {
                    Err(PmError::new(ErrorCode::NotFound, "Select an issue to copy"))
                }
            }
            _ => Ok(()),
        };
        if let Err(error) = result {
            self.notice = Some(error.message);
        }
        true
    }

    fn open_features(&mut self) {
        self.effect = Some(ShellEffect::Return);
        self.tab = WorkbenchTab::Features;
        self.bind_planning_source();
        self.features
            .bind_if_missing(self.controller.repository().ok().cloned());
        self.features.open();
    }

    fn choose_link(&mut self, commits: bool) -> workdeck_pm::Result<()> {
        let issue = self
            .controller
            .selected_issue()
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "Select an issue first"))?;
        let count = if commits {
            issue.metadata.commits.len()
        } else {
            issue.metadata.files.len()
        };
        if count == 0 {
            return Err(PmError::new(
                ErrorCode::NotFound,
                if commits {
                    "This issue has no commit links"
                } else {
                    "This issue has no file links"
                },
            ));
        }
        let mut links = (0..count)
            .map(|index| {
                if commits {
                    self.controller.navigate_commit(index, ())
                } else {
                    self.controller.navigate_file(index, ())
                }
            })
            .collect::<workdeck_pm::Result<Vec<_>>>()?;
        if count == 1 {
            self.effect = Some(ShellEffect::Navigate(Box::new(links.remove(0))));
            return Ok(());
        }
        let help = links
            .iter()
            .enumerate()
            .map(|(index, nav)| {
                format!(
                    "{}. {}",
                    index + 1,
                    match &nav.target {
                        IssueReviewTarget::File(link) => format!(
                            "{}{}",
                            link.path,
                            link.line.map(|line| format!(":{line}")).unwrap_or_default()
                        ),
                        IssueReviewTarget::Commit(commit) => commit.clone(),
                    }
                )
            })
            .collect();
        let mut form = WorkbenchForm::new(
            FormKind::Links(links),
            if commits {
                "Choose commit"
            } else {
                "Choose file"
            },
            vec![TextField::new("Link number", "1".into(), false)],
        );
        form.help = help;
        self.form = Some(form);
        Ok(())
    }
}
