//! Integration methods are installed on the existing mounted ReviewApp.

use super::shell::ShellEffect;
use super::{IssueReviewTarget, PanelTarget, WorkbenchTab};
use crate::*;
use workdeck_core::{CliInput, FileCommandInput, VcsShowCommandInput};
use workdeck_pm::SourceLink;

#[derive(Debug, Clone)]
pub(super) struct NativeReturnContext {
    pub input: Option<workdeck_core::CliInput>,
    pub path: Option<String>,
    pub selection: workdeck_core::ReviewSelection,
    pub scroll: usize,
    pub current_line_row: usize,
    pub filter: String,
    pub focus: Focus,
    pub sources: BTreeMap<String, (String, Option<String>)>,
    pub expanded_gaps: BTreeSet<(String, usize)>,
    pub gap_cursor_restore: BTreeMap<(String, usize), GapCursorRestorePoint>,
    pub planning: super::PlanningReturnContext,
}

impl ReviewApp {
    /// A clean file has no diff hunks. For an explicit planning file jump only,
    /// present its host-loaded immutable snapshot as context rows before the
    /// shared producer publishes it. This adds no claimed changes or file I/O.
    pub(crate) fn prepare_workbench_source_view(
        &self,
        input: &CliInput,
        changeset: &mut Changeset,
    ) -> Result<(), String> {
        let Some(shell) = &self.workbench else {
            return Ok(());
        };
        let shell = shell.lock().unwrap_or_else(|error| error.into_inner());
        let Some(path) = &shell.source_file else {
            return Ok(());
        };
        if !matches!(input,CliInput::Files(input) if &input.left==path && &input.right==path) {
            return Ok(());
        }
        let Some(file) = changeset.files.first_mut() else {
            if shell.context_source.is_some() {
                return Err("The cited source snapshot is unavailable".into());
            }
            return Ok(());
        };
        if let Some((expected_path, expected)) = &shell.context_source {
            let source = file
                .sources
                .new
                .as_ref()
                .ok_or("The cited source snapshot is unavailable")?;
            if expected_path != path
                || workdeck_pm::ContentHash::of(source.content.as_bytes()) != *expected
            {
                return Err("The cited source changed after context inspection; refresh context before opening it".into());
            }
        }
        if !file.hunks.is_empty() || file.flags.binary {
            return Ok(());
        }
        let Some(source) = file.sources.new.as_ref() else {
            return Err("The file source snapshot is unavailable".into());
        };
        if source.content.is_empty() {
            return Ok(());
        }
        let lines = source.content.lines().collect::<Vec<_>>();
        let count = lines.len();
        let mut patch = format!(
            "diff --git a/source b/source\n--- a/source\n+++ b/source\n@@ -1,{count} +1,{count} @@ Source\n"
        );
        for line in lines {
            patch.push(' ');
            patch.push_str(line);
            patch.push('\n');
        }
        let mut projected = workdeck_diff::parse_single_file_patch(&patch, path, Some(path))
            .map_err(|error| error.to_string())?;
        projected.set_sources(file.sources.clone());
        *file = projected;
        changeset.refresh_review_identities();
        Ok(())
    }

    pub(crate) fn render_workbench_nav(&self, area: Rect, buffer: &mut Buffer) -> Rect {
        let Some(shell) = &self.workbench else {
            return area;
        };
        let nav_height = {
            let shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            1 + u16::from(shell.panels.is_some()) + u16::from(shell.checkout_label.is_some())
        };
        let rows =
            Layout::vertical([Constraint::Length(nav_height), Constraint::Min(0)]).split(area);
        shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .render_nav(rows[0], buffer, &self.options.theme);
        rows[1]
    }

    pub(crate) fn render_workbench_body(&self, area: Rect, buffer: &mut Buffer) -> bool {
        let Some(shell) = &self.workbench else {
            return false;
        };
        let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
        if !matches!(shell.tab, WorkbenchTab::Review | WorkbenchTab::MyWork)
            && let Some(source) = &mut shell.readonly
        {
            source.render(area, buffer, &self.options.theme);
            return true;
        }
        if let WorkbenchTab::Repository(page) = shell.tab {
            shell.render_repository_panel(page, area, buffer, &self.options.theme);
            return true;
        }
        if shell.tab == WorkbenchTab::Planning {
            shell.planning.render(area, buffer, &self.options.theme);
            return true;
        }
        if shell.tab == WorkbenchTab::MyWork {
            if let Some(view) = &mut shell.my_work {
                view.render(area, buffer, &self.options.theme);
            }
            return true;
        }
        if shell.tab == WorkbenchTab::Activity {
            shell.render_activity(area, buffer, &self.options.theme);
            return true;
        }
        if shell.tab == WorkbenchTab::Features {
            shell.planning_bounds = Some(area);
            shell.features.render(area, buffer, &self.options.theme);
            return true;
        }
        if shell.tab != WorkbenchTab::Issues {
            shell.planning_bounds = None;
            return false;
        }
        let planning = if area.width >= 150 {
            let columns =
                Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(area);
            render_body(columns[1], buffer, self);
            columns[0]
        } else {
            area
        };
        shell.render_planning(planning, buffer, &self.options.theme);
        true
    }

    pub(crate) fn workbench_issues_visible(&self) -> bool {
        self.workbench.as_ref().is_some_and(|shell| {
            shell.lock().unwrap_or_else(|error| error.into_inner()).tab != WorkbenchTab::Review
        })
    }

    pub(crate) fn handle_workbench_key(&mut self, key: KeyEvent) -> bool {
        self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .key(key)
        })
    }

    pub(crate) fn handle_workbench_paste(&mut self, text: &str) -> bool {
        if self.show_help
            || self.show_agent_skill
            || self.has_extension_dialog()
            || self.extension_trust_controller.prompt_open()
            || self.view_preference_quit.save_config_prompt_open()
        {
            return false;
        }
        self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .paste(text)
        })
    }

    /// Application modals that own keyboard input ahead of the workbench.
    /// Queued planning input must not replay while one is open, because the
    /// replay path drives the shell directly and skips the modal owners in
    /// `handle_key`.
    fn workbench_input_blocked(&self) -> bool {
        if self.show_help || self.show_agent_skill {
            return true;
        }
        // try_lock keeps the poll path non-blocking: a busy runtime pauses
        // replay for a frame instead of stalling the interface thread.
        self.extension_pane_runtime
            .try_lock()
            .map(|runtime| runtime.menu.is_open() || runtime.dialogs.current().is_some())
            .unwrap_or(true)
    }

    pub(crate) fn poll_workbench(&mut self) {
        let publication = self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .context
                .checks
                .signal
                .is_publication()
        });
        for retained in &self.workbench_checkouts.retained {
            let mut shell = retained
                .shell
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(source) = &mut shell.readonly {
                source.poll();
            }
            shell.input_paused = self.workbench_input_blocked();
            shell.context.checks.poll();
            shell.context.claims.poll();
            shell.context.source_actions.poll();
            shell.poll_index(false);
            if let Some(activity) = &mut shell.activity {
                activity.poll();
            }
            shell.features.poll_index();
            shell.planning.poll_index();
            if let Some(panels) = &mut shell.panels {
                panels.poll();
            }
        }
        if let Some(shell) = &self.workbench {
            let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            shell.context.checks.poll();
            shell.context.claims.poll();
            shell.context.source_actions.poll();
            if publication && !shell.context.checks.signal.is_active() {
                self.status = Some("Planning publication joined; inspect Claims or Source operations for its outcome".into());
            } else if publication && shell.context.checks.signal.interruption_requested() {
                self.status =
                    Some("Publication pending; exit is deferred until its worker joins".into());
            }
            if let Some(panels) = &mut shell.panels {
                panels.poll();
            }
            shell.input_paused = self.workbench_input_blocked();
            shell.refresh(Instant::now(), false);
        }
    }

    /// Signal ownership remains in the composition root. Keyboard interrupts
    /// use the same run signal so active checks cancel without quitting Review.
    pub(crate) fn interrupt_foreground_run(&mut self) -> bool {
        let publication = self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .context
                .checks
                .signal
                .is_publication()
        });
        let consumed = self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .context
                .checks
                .signal
                .interrupt_active()
        });
        if consumed {
            self.status = Some(if publication {
                "Publication pending; exit is deferred until its worker joins. No cancellation is inferred."
            } else { "Canceling foreground checks; waiting for owned process cleanup" }.into());
        }
        consumed
    }

    pub(crate) fn defer_publication_exit(&mut self) -> bool {
        let publication = self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .context
                .checks
                .signal
                .is_publication()
        });
        publication && self.interrupt_foreground_run()
    }

    pub(crate) fn defer_foreground_run_suspend(&mut self) -> bool {
        let active = self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .context
                .checks
                .signal
                .is_active()
        });
        if active {
            let publication = self.workbench.as_ref().is_some_and(|shell| {
                shell
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .context
                    .checks
                    .signal
                    .is_publication()
            });
            let message = if publication {
                "Suspension deferred until planning publication joins; publication is not canceled"
            } else {
                "Suspension deferred while a foreground check is active; Ctrl-C cancels it"
            };
            self.status = Some(message.into());
            if let Some(shell) = &self.workbench {
                shell
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .context
                    .checks
                    .state_mut()
                    .notice = Some(message.into());
            }
        }
        active
    }

    pub(crate) fn shutdown_foreground_run(&mut self) -> std::result::Result<(), String> {
        let retained = std::mem::take(&mut self.workbench_checkouts.retained);
        for entry in &retained {
            Self::shutdown_checkout_workers(&entry.shell);
        }
        let Some(shell) = &self.workbench else {
            return Ok(());
        };
        Self::shutdown_checkout_workers(shell);
        if shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .context
            .checks
            .signal
            .is_active()
        {
            Err(
                "Foreground check cleanup was not acknowledged; inspect its durable run status"
                    .into(),
            )
        } else {
            Ok(())
        }
    }

    fn shutdown_checkout_workers(shell: &Mutex<super::WorkbenchShell>) {
        let mut readonly = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .readonly
            .take();
        if let Some(source) = &mut readonly {
            source.begin_shutdown();
        }

        let mut my_work = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .my_work
            .take();
        if let Some(view) = &mut my_work {
            view.begin_shutdown();
        }
        let mut activity = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .activity
            .take();
        if let Some(activity) = &mut activity {
            activity.begin_shutdown();
        }
        let _planning_index = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .planning
            .take_index_shutdown();
        let _feature_index = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .features
            .take_index_shutdown();
        let mut index = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .index
            .take();
        if let Some(index) = &mut index {
            index.begin_shutdown();
        }
        // Keep the reader owned through foreground cleanup; its Drop joins outside
        // the shell lock before this shutdown boundary returns.
        let publication = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .context
            .claims
            .take_shutdown();
        if let Some((issue, request, mut task)) = publication
            && let Some(result) = task.shutdown()
        {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .context
                .claims
                .finish_shutdown(issue, request, result);
        }
        let publication = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .context
            .source_actions
            .take_shutdown();
        if let Some(mut task) = publication
            && let Some(result) = task.shutdown()
        {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .context
                .source_actions
                .finish(result);
        }
        let task = shell
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .context
            .checks
            .take_shutdown_task();
        if let Some(mut task) = task {
            // Do not hold the shell mutex while the core cleans up or publishes.
            if let Some(result) = task.shutdown() {
                shell
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .context
                    .checks
                    .finish_shutdown(task, result);
            }
        }
    }

    pub(crate) fn handle_workbench_mouse(&mut self, event: &MouseEvent) -> bool {
        let Some(shell) = &self.workbench else {
            return false;
        };
        let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
        if rect_contains(shell.nav_bounds, event.column, event.row) {
            if event.kind == MouseEventKind::Up(MouseButton::Left)
                && let Some((_, key)) = shell
                    .nav_hits
                    .iter()
                    .find(|(rect, _)| rect_contains(*rect, event.column, event.row))
                    .copied()
            {
                shell.key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            return true;
        }
        if !matches!(shell.tab, WorkbenchTab::Review | WorkbenchTab::MyWork)
            && let Some(source) = &mut shell.readonly
        {
            return source.mouse(event);
        }
        if shell.tab == WorkbenchTab::MyWork {
            return shell.my_work.as_mut().is_some_and(|view| view.mouse(event));
        }
        if shell.tab == WorkbenchTab::Activity {
            return shell.activity_mouse(event);
        }
        if shell.tab == WorkbenchTab::Issues && shell.graph.is_some() {
            if !shell
                .planning_bounds
                .is_some_and(|area| rect_contains(area, event.column, event.row))
            {
                return false;
            }
            if let Some(graph) = &mut shell.graph {
                let key = match event.kind {
                    MouseEventKind::ScrollDown => Some(KeyCode::PageDown),
                    MouseEventKind::ScrollUp => Some(KeyCode::PageUp),
                    _ => None,
                };
                if let Some(key) = key {
                    graph.key(KeyEvent::new(key, KeyModifiers::NONE));
                }
            }
            return true;
        }
        if shell.tab == WorkbenchTab::Issues && shell.context_visible {
            if !shell
                .planning_bounds
                .is_some_and(|area| rect_contains(area, event.column, event.row))
            {
                return false;
            }
            let key = match event.kind {
                MouseEventKind::ScrollDown => Some(KeyCode::PageDown),
                MouseEventKind::ScrollUp => Some(KeyCode::PageUp),
                _ => None,
            };
            if let Some(key) = key {
                shell.context.key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            return true;
        }
        if shell.tab == WorkbenchTab::Features {
            if !shell
                .planning_bounds
                .is_some_and(|area| rect_contains(area, event.column, event.row))
            {
                return false;
            }
            let key = match event.kind {
                MouseEventKind::ScrollDown => Some(KeyCode::PageDown),
                MouseEventKind::ScrollUp => Some(KeyCode::PageUp),
                _ => None,
            };
            if let Some(key) = key {
                shell.features.key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            return true;
        }
        if let WorkbenchTab::Repository(page) = shell.tab {
            if !shell
                .planning_bounds
                .is_some_and(|rect| rect_contains(rect, event.column, event.row))
            {
                return false;
            }
            let in_preview = shell
                .panel_preview_bounds
                .is_some_and(|rect| rect_contains(rect, event.column, event.row));
            match event.kind {
                MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
                    let delta = if event.kind == MouseEventKind::ScrollDown {
                        1
                    } else {
                        -1
                    };
                    if let Some(panels) = &mut shell.panels {
                        if in_preview {
                            let state = panels.state(page);
                            state.location.preview_scroll = state
                                .location
                                .preview_scroll
                                .saturating_add_signed(delta as i16 * 3);
                        } else {
                            panels.move_selection(page, delta);
                        }
                    }
                }
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(id) = shell
                        .panel_hits
                        .iter()
                        .find(|(rect, _)| rect_contains(*rect, event.column, event.row))
                        .map(|(_, id)| id.clone())
                        && let Some(panels) = &mut shell.panels
                    {
                        panels.select(page, &id);
                    }
                }
                _ => {}
            }
            return true;
        }
        if shell.tab == WorkbenchTab::Planning {
            if !shell
                .planning
                .bounds
                .is_some_and(|area| rect_contains(area, event.column, event.row))
            {
                return false;
            }
            let key = match event.kind {
                MouseEventKind::ScrollDown => Some(KeyCode::Down),
                MouseEventKind::ScrollUp => Some(KeyCode::Up),
                _ => None,
            };
            if let Some(key) = key {
                shell
                    .planning
                    .handle_key(KeyEvent::new(key, KeyModifiers::NONE));
            }
            return true;
        }
        if shell.tab != WorkbenchTab::Issues {
            return false;
        }
        if !shell
            .planning_bounds
            .is_some_and(|area| rect_contains(area, event.column, event.row))
        {
            return false;
        }
        if shell.form.is_some() {
            return true;
        }
        if !shell.context_visible && shell.graph.is_none() && shell.index.is_some() {
            let clicked = shell.index_rendered.as_ref().and_then(|rendered| {
                rendered
                    .rows
                    .iter()
                    .find(|(area, _)| rect_contains(*area, event.column, event.row))
                    .map(|(_, token)| token.clone())
            });
            let over_list = shell
                .index_rendered
                .as_ref()
                .is_some_and(|rendered| rect_contains(rendered.list, event.column, event.row));
            let over_detail = shell
                .index_rendered
                .as_ref()
                .is_some_and(|rendered| rect_contains(rendered.detail, event.column, event.row));
            let index = shell.index.as_mut().expect("indexed issue view");
            match event.kind {
                MouseEventKind::ScrollDown if over_detail => {
                    index.detail_scroll = index
                        .detail_scroll
                        .saturating_add(3)
                        .min(index.opened_lines.len().saturating_sub(1));
                }
                MouseEventKind::ScrollUp if over_detail => {
                    index.detail_scroll = index.detail_scroll.saturating_sub(3);
                }
                MouseEventKind::ScrollDown if over_list => index.move_by(1),
                MouseEventKind::ScrollUp if over_list => index.move_by(-1),
                MouseEventKind::Up(MouseButton::Left) => {
                    if let Some(token) = clicked
                        && index.select_token(&token)
                    {
                        index.open_selected();
                    }
                }
                _ => {}
            }
            shell.sync_index_selection();
            return true;
        }
        match event.kind {
            MouseEventKind::ScrollDown => shell.controller.move_selection(1),
            MouseEventKind::ScrollUp => shell.controller.move_selection(-1),
            MouseEventKind::Up(MouseButton::Left) => {
                if let Some(rendered) = &shell.rendered
                    && let Some(list) = rendered.layout.list
                    && rect_contains(list, event.column, event.row)
                {
                    let index = usize::from(event.row.saturating_sub(list.y + 1)) / 2;
                    if let Some(id) = rendered.visible_issue_ids.get(index).cloned() {
                        shell.controller.select(&id);
                        shell.controller.view_mut().pane = super::WorkbenchPane::Detail;
                        let _ = shell.controller.refresh_detail();
                    }
                }
            }
            _ => {}
        }
        true
    }

    /// Process planning navigation on the same serial AppHost commit boundary
    /// as broker reloads. The callback owns VCS I/O, publication and watch reset.
    pub(crate) fn process_workbench_effect<R>(&mut self, reload: &mut R)
    where
        R: FnMut(&mut ReviewApp, &CliInput, &std::path::Path) -> Result<(), String>,
    {
        let navigation = self.workbench.as_ref().and_then(|shell| {
            let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            if shell.tab == WorkbenchTab::MyWork {
                shell
                    .my_work
                    .as_mut()
                    .and_then(|view| view.ready_navigation.take())
            } else {
                None
            }
        });
        if let Some(navigation) = navigation {
            if let Err(error) = self.workbench_registered_checkout(navigation, reload)
                && let Some(shell) = &self.workbench
            {
                let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
                shell.notice = Some(error.clone());
                if let Some(view) = &mut shell.my_work {
                    view.error = Some(error);
                }
            }
            return;
        }
        let effect = self.workbench.as_ref().and_then(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .effect
                .take()
        });
        let Some(effect) = effect else {
            return;
        };
        let result = match effect {
            ShellEffect::ContextNavigate(navigation) => {
                self.workbench_context_navigate(*navigation, reload)
            }
            ShellEffect::CreateFromReview => self.workbench_create_from_review(),
            ShellEffect::LinkFromReview => self.workbench_link_from_review(),
            ShellEffect::CopyReference(reference) => {
                if self.clipboard_copy_supported {
                    self.clipboard_copy_request = Some(reference);
                    self.workbench_clipboard_notice(
                        "Copy requested through terminal clipboard".into(),
                    );
                } else {
                    self.workbench_clipboard_notice(
                        "Clipboard copy unsupported in this terminal (enable OSC 52)".into(),
                    );
                }
                Ok(())
            }
            ShellEffect::Navigate(target) => self.workbench_navigate(*target, reload),
            ShellEffect::Return => self.workbench_return(reload),
            ShellEffect::PreviousCheckout => self.workbench_previous_checkout(reload),
            ShellEffect::LaunchCheckout => self.workbench_launch_checkout(reload),
            ShellEffect::PanelNavigate(target) => {
                self.workbench_panel_navigate(target, None, reload)
            }
            ShellEffect::PlanningMembers { target, scope } => {
                self.workbench_panel_navigate(target, Some(scope), reload)
            }
        };
        if let Err(error) = result
            && let Some(shell) = &self.workbench
        {
            let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            shell.notice = Some(error.clone());
            if shell.tab == WorkbenchTab::MyWork
                && let Some(view) = &mut shell.my_work
            {
                view.error = Some(error);
            }
            if shell.tab == WorkbenchTab::Review {
                shell.tab = WorkbenchTab::Issues;
            }
        }
    }

    fn workbench_context_navigate<R>(
        &mut self,
        navigation: super::context_workspace::ContextNavigation,
        reload: &mut R,
    ) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &std::path::Path) -> Result<(), String>,
    {
        use workdeck_pm::{ContentHash, ContextTarget};
        let (root, planning, link, expected) = {
            let shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            shell
                .context
                .validate_navigation(&navigation)
                .map_err(|error| error.message)?;
            let repository = shell.context.repository().map_err(|error| error.message)?;
            let expected = navigation
                .citation
                .source
                .clone()
                .ok_or("This citation has no captured content; refresh before opening it")?;
            let planning_path = |path: std::path::PathBuf,
                                 hash: ContentHash|
             -> Result<SourceLink, String> {
                if hash != expected {
                    return Err("The cited planning record changed; refresh context".into());
                }
                let relative = repository
                    .root()
                    .join(path)
                    .strip_prefix(&shell.options.root)
                    .map_err(|_| "This planning source is not bound to the reviewed repository")?
                    .to_path_buf();
                let link = SourceLink {
                    path: relative.to_string_lossy().into_owned(),
                    line: None,
                    end_line: None,
                };
                link.validate().map_err(|error| error.message)?;
                Ok(link)
            };
            let link = match &navigation.citation.target {
                ContextTarget::WorktreeSource { link } => link.clone(),
                ContextTarget::PlanningSource { path } => {
                    planning_path(path.clone(), expected.clone())?
                }
                ContextTarget::Issue { id } => {
                    let record = repository
                        .show_issue(id.as_str())
                        .map_err(|error| error.message)?;
                    planning_path(record.path, record.source.content)?
                }
                ContextTarget::Feature { id } => {
                    let record = repository
                        .feature(id.as_str())
                        .map_err(|error| error.message)?;
                    planning_path(record.path, record.source.content)?
                }
                ContextTarget::Question { id } => {
                    let id = id
                        .parse()
                        .map_err(|error: workdeck_pm::PmError| error.message)?;
                    let record = repository.question(&id).map_err(|error| error.message)?;
                    planning_path(record.path, record.source.content)?
                }
                ContextTarget::Handoff { issue, id } => {
                    let id = id
                        .parse()
                        .map_err(|error: workdeck_pm::PmError| error.message)?;
                    let record = repository
                        .handoff(issue, &id)
                        .map_err(|error| error.message)?;
                    planning_path(record.path, record.content)?
                }
                ContextTarget::Evidence { id } => {
                    let record = repository.evidence(id).map_err(|error| error.message)?;
                    planning_path(record.path, record.content)?
                }
            };
            link.validate().map_err(|error| error.message)?;
            (
                shell.options.root.clone(),
                shell.controller.return_context(),
                link,
                expected,
            )
        };
        self.workbench_capture_return(planning);
        let options = self
            .options
            .review_input
            .as_ref()
            .map(|input| input.options().clone())
            .unwrap_or_default();
        let input = CliInput::Files(FileCommandInput {
            left: link.path.clone(),
            right: link.path.clone(),
            options,
        });
        {
            let mut shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            shell.source_file = Some(link.path.clone());
            shell.context_source = Some((link.path.clone(), expected));
        }
        // The host callback reads immutable bytes. prepare_workbench_source_view
        // checks their captured hash before the producer may publish them.
        let result = self.workbench_reload_input(input, &root, reload);
        self.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .context_source = None;
        result?;
        self.filter.clear();
        self.focus = Focus::Review;
        self.workbench_reveal_link(&link)?;
        let mut shell = self
            .workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shell.tab = WorkbenchTab::Review;
        shell.notice = None;
        Ok(())
    }

    pub(crate) fn workbench_clipboard_notice(&mut self, notice: String) {
        self.status = Some(notice.clone());
        if let Some(shell) = &self.workbench {
            let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            if shell.tab != WorkbenchTab::Review {
                shell.notice = Some(notice);
            }
        }
    }

    fn workbench_review_scope(&self) -> Result<(), String> {
        if let Some(shell) = &self.workbench {
            let shell = shell.lock().unwrap_or_else(|error| error.into_inner());
            if self.options.repo.as_deref() != Some(shell.options.root.as_path()) {
                return Err("This review belongs to another repository or its repository is unavailable. Return to the workbench repository before linking a review file.".into());
            }
        }
        Ok(())
    }

    fn workbench_link_from_review(&mut self) -> Result<(), String> {
        self.workbench_review_scope()?;
        let link = self.with_state(|state| {
            let file = state
                .selected_file()
                .ok_or_else(|| "Select a review file to link".to_owned())?;
            Ok::<_, String>(SourceLink {
                path: file.path.clone(),
                line: state.selection().line,
                end_line: None,
            })
        })?;
        link.validate().map_err(|error| error.message)?;
        let shell = self
            .workbench
            .as_ref()
            .expect("workbench effect has a shell");
        let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
        let target = shell.controller.selected_target().ok_or_else(|| "Select an issue in Issues, return to Review, then press Shift-F4 to link this file.".to_owned())?;
        let issue = shell
            .controller
            .selected_issue()
            .expect("selected target has an issue");
        let mut form = super::input::WorkbenchForm::new(
            super::input::FormKind::LinkFile {
                target,
                link: link.clone(),
            },
            "Link current file",
            Vec::new(),
        );
        form.help = vec![
            format!("Issue: {} · {}", issue.metadata.id, issue.metadata.title),
            format!("File: {}{}", link.path, link.line.map(|line| format!(":{line}")).unwrap_or_default()),
            "Enter links this file to the selected issue. Choose another issue in Issues before opening this form.".into(),
        ];
        shell.form = Some(form);
        shell.context_visible = false;
        shell.notice = None;
        shell.tab = WorkbenchTab::Issues;
        Ok(())
    }

    fn workbench_create_from_review(&mut self) -> Result<(), String> {
        self.workbench_review_scope()?;
        let note = self.active_note_for_composer(false);
        let (path, line) = self.with_state(|state| {
            let file = state
                .selected_file()
                .ok_or_else(|| "Select a review file before creating an issue".to_owned())?;
            Ok::<_, String>((file.path.clone(), state.selection().line))
        })?;
        let (line, title, body) = note.map_or_else(
            || (line, format!("Review {path}"), String::new()),
            |(note, target)| {
                let body = note
                    .markup
                    .or(note.rationale)
                    .unwrap_or_else(|| note.summary.clone());
                (Some(target.line), note.title.unwrap_or(note.summary), body)
            },
        );
        let shell = self
            .workbench
            .as_ref()
            .expect("workbench effect has a shell");
        let mut shell = shell.lock().unwrap_or_else(|error| error.into_inner());
        let key = shell
            .controller
            .begin_create_from_file(
                SourceLink {
                    path,
                    line,
                    end_line: None,
                },
                title,
                body,
            )
            .map_err(|error| error.message)?;
        shell.open_draft(&key);
        Ok(())
    }

    pub(super) fn capture_native_return(
        &self,
        planning: super::PlanningReturnContext,
    ) -> NativeReturnContext {
        NativeReturnContext {
            input: self.options.review_input.clone(),
            path: self.with_state(|state| state.selected_file().map(|file| file.path.clone())),
            selection: self.with_state(|state| state.selection()),
            scroll: self.scroll,
            current_line_row: self.current_line_row,
            filter: self.filter.clone(),
            focus: self.focus,
            sources: self.with_state(|state| {
                state
                    .changeset()
                    .files
                    .iter()
                    .map(|file| {
                        (
                            file.key.clone(),
                            (file.content_identity.clone(), file.source_identity.clone()),
                        )
                    })
                    .collect()
            }),
            expanded_gaps: self.expanded_gaps.clone(),
            gap_cursor_restore: self.gap_cursor_restore.clone(),
            planning,
        }
    }

    fn workbench_capture_return(&mut self, planning: super::PlanningReturnContext) {
        let context = self.capture_native_return(planning);
        {
            let mut shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            shell.navigation.get_or_insert(context);
        }
    }

    fn workbench_navigate<R>(
        &mut self,
        navigation: super::IssueNavigation<()>,
        reload: &mut R,
    ) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &std::path::Path) -> Result<(), String>,
    {
        let root = navigation.repository_root;
        if self.workbench.as_ref().is_some_and(|shell| {
            shell
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .controller
                .selected_target()
                .is_none_or(|target| target.repository != navigation.repository)
        }) {
            return Err(
                "The issue navigation source does not match the workbench repository".into(),
            );
        }
        let planning = navigation.planning;
        let target = navigation.target;
        self.workbench_capture_return(planning);
        let options = self
            .options
            .review_input
            .as_ref()
            .map(|input| input.options().clone())
            .unwrap_or_default();
        let next = match &target {
            IssueReviewTarget::Commit(reference) => Some(CliInput::Show(VcsShowCommandInput {
                reference: Some(reference.clone()),
                pathspecs: Vec::new(),
                options,
            })),
            IssueReviewTarget::File(link) => {
                link.validate().map_err(|error| error.message)?;
                let present = self.with_state(|state| {
                    state
                        .changeset()
                        .files
                        .iter()
                        .any(|file| file.path == link.path)
                });
                (!present).then(|| {
                    CliInput::Files(FileCommandInput {
                        left: link.path.clone(),
                        right: link.path.clone(),
                        options,
                    })
                })
            }
        };
        {
            let mut shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            shell.source_file = match &next {
                Some(CliInput::Files(input)) => Some(input.right.clone()),
                _ => None,
            };
        }
        if let Some(next) = next {
            self.workbench_reload_input(next, &root, reload)?;
        }
        self.filter.clear();
        self.focus = Focus::Review;
        if let IssueReviewTarget::File(link) = target {
            self.workbench_reveal_link(&link)?;
        }
        let mut shell = self
            .workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shell.tab = WorkbenchTab::Review;
        shell.notice = None;
        Ok(())
    }

    fn workbench_panel_navigate<R>(
        &mut self,
        target: PanelTarget,
        scope: Option<workdeck_pm::ArchiveFilter>,
        reload: &mut R,
    ) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &std::path::Path) -> Result<(), String>,
    {
        if let PanelTarget::Issue { id } = &target {
            let mut shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let id: workdeck_pm::IssueId = id
                .parse()
                .map_err(|error: workdeck_pm::PmError| error.message)?;
            let repository = shell
                .controller
                .repository()
                .map_err(|error| error.message)?
                .identity()
                .clone();
            shell.form = None;
            shell.context_visible = false;
            shell
                .controller
                .set_filter(super::IssueFilter::default())
                .map_err(|error| error.message)?;
            shell.index_pending_selection = Some(workdeck_pm::projection::ProjectionRecordKey {
                repository,
                kind: workdeck_pm::SnapshotKind::Issue,
                id: id.to_string(),
            });
            shell.tab = WorkbenchTab::Issues;
            shell.poll_index(true);
            return Ok(());
        }
        if matches!(
            target,
            PanelTarget::Project { .. }
                | PanelTarget::Cycle { .. }
                | PanelTarget::Label { .. }
                | PanelTarget::Milestone { .. }
                | PanelTarget::Target { .. }
        ) {
            let mut shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let mut filter = super::IssueFilter {
                include_archived: scope == Some(workdeck_pm::ArchiveFilter::All),
                archived_only: scope == Some(workdeck_pm::ArchiveFilter::Archived),
                ..super::IssueFilter::default()
            };
            match target {
                PanelTarget::Project { id } => filter.project = Some(id),
                PanelTarget::Cycle { id } => filter.cycle = Some(id),
                PanelTarget::Label { id } => filter.label = Some(id),
                PanelTarget::Milestone { id } => filter.milestone = Some(id),
                PanelTarget::Target { id } => filter.targets = vec![id],
                _ => unreachable!(),
            }
            shell.form = None;
            shell.context_visible = false;
            shell
                .controller
                .set_filter(filter)
                .map_err(|error| error.message)?;
            shell.controller.view_mut().pane = super::WorkbenchPane::List;
            shell.tab = WorkbenchTab::Issues;
            shell.index_pending_selection = None;
            shell.poll_index(true);
            return Ok(());
        }
        let (root, planning) = {
            let shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (
                shell
                    .panels
                    .as_ref()
                    .ok_or("Repository panels are unavailable")?
                    .source
                    .root
                    .clone(),
                shell.controller.return_context(),
            )
        };
        let options = self
            .options
            .review_input
            .as_ref()
            .map(|input| input.options().clone())
            .unwrap_or_default();
        let (next, link) = match target {
            PanelTarget::File { path, line } => {
                let link = SourceLink {
                    path: path.clone(),
                    line,
                    end_line: None,
                };
                link.validate().map_err(|error| error.message)?;
                (
                    CliInput::Files(FileCommandInput {
                        left: path.clone(),
                        right: path,
                        options,
                    }),
                    Some(link),
                )
            }
            PanelTarget::Change { path, staged } => {
                SourceLink {
                    path: path.clone(),
                    line: None,
                    end_line: None,
                }
                .validate()
                .map_err(|error| error.message)?;
                (
                    CliInput::Vcs(workdeck_core::VcsDiffCommandInput {
                        range: None,
                        range_endpoints: None,
                        staged,
                        pathspecs: vec![path.clone()],
                        options,
                    }),
                    Some(SourceLink {
                        path,
                        line: None,
                        end_line: None,
                    }),
                )
            }
            PanelTarget::Commit { reference } | PanelTarget::Tag { reference } => (
                CliInput::Show(VcsShowCommandInput {
                    reference: Some(reference),
                    pathspecs: Vec::new(),
                    options,
                }),
                None,
            ),
            PanelTarget::Branch { reference } => (
                CliInput::Vcs(workdeck_core::VcsDiffCommandInput {
                    range: Some(format!("HEAD...{reference}")),
                    range_endpoints: None,
                    staged: false,
                    pathspecs: Vec::new(),
                    options,
                }),
                None,
            ),
            PanelTarget::Stash { reference } => (
                CliInput::StashShow(workdeck_core::VcsStashShowCommandInput {
                    reference: Some(reference),
                    options,
                }),
                None,
            ),
            _ => {
                let mut shell = self
                    .workbench
                    .as_ref()
                    .unwrap()
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                shell.notice=Some("This entry has a read-only preview. Select a file, change, commit, branch, stash, or issue to open it.".into());
                return Ok(());
            }
        };
        self.workbench_capture_return(planning);
        {
            let mut shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            shell.source_file = if let CliInput::Files(input) = &next {
                Some(input.right.clone())
            } else {
                None
            };
        }
        self.workbench_reload_input(next, &root, reload)?;
        self.filter.clear();
        self.focus = Focus::Review;
        if let Some(link) = link {
            self.workbench_reveal_link(&link)?;
        }
        let mut shell = self
            .workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shell.tab = WorkbenchTab::Review;
        shell.notice = None;
        Ok(())
    }

    fn workbench_reload_input<R>(
        &mut self,
        next: CliInput,
        root: &std::path::Path,
        reload: &mut R,
    ) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &std::path::Path) -> Result<(), String>,
    {
        reload(self, &next, root)
    }

    pub(super) fn restore_native_review(&mut self, context: &NativeReturnContext) {
        self.scroll = context.scroll;
        self.current_line_row = context.current_line_row;
        self.filter = context.filter.clone();
        self.focus = context.focus;
        let retained = self.with_state(|state| {
            state
                .changeset()
                .files
                .iter()
                .filter(|file| {
                    context.sources.get(&file.key)
                        == Some(&(file.content_identity.clone(), file.source_identity.clone()))
                })
                .map(|file| file.key.clone())
                .collect::<BTreeSet<_>>()
        });
        self.expanded_gaps = context
            .expanded_gaps
            .iter()
            .filter(|(key, _)| retained.contains(key))
            .cloned()
            .collect();
        self.gap_cursor_restore = context
            .gap_cursor_restore
            .iter()
            .filter(|((key, _), restore)| {
                retained.contains(key) && retained.contains(&restore.file_key)
            })
            .map(|(key, restore)| (key.clone(), restore.clone()))
            .collect();
        if let Some(path) = &context.path {
            let index = self.with_state(|state| {
                state
                    .changeset()
                    .files
                    .iter()
                    .position(|file| &file.path == path)
            });
            if let Some(index) = index {
                self.with_state(|state| {
                    let _ = state.select_file(index);
                    if let Some(hunk) = context.selection.hunk_index {
                        let _ = state.select_hunk(index, hunk);
                    }
                    if let (Some(side), Some(line)) =
                        (context.selection.side, context.selection.line)
                    {
                        let _ = state.reveal_line(index, side, line).or_else(|_| {
                            state.reveal_source_line(
                                index,
                                context.selection.hunk_index.unwrap_or(0),
                                side,
                                line,
                            )
                        });
                    }
                });
                self.current_line_row = context.current_line_row;
                self.scroll = context.scroll;
            }
        }
        self.publish_extension_selection_events();
    }

    fn workbench_return<R>(&mut self, reload: &mut R) -> Result<(), String>
    where
        R: FnMut(&mut ReviewApp, &CliInput, &std::path::Path) -> Result<(), String>,
    {
        let (context, root) = {
            let shell = self
                .workbench
                .as_ref()
                .unwrap()
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            (shell.navigation.clone(), shell.options.root.clone())
        };
        let Some(context) = context else {
            return Ok(());
        };
        if let Some(input) = context.input.clone()
            && self.options.review_input.as_ref() != Some(&input)
        {
            self.workbench_reload_input(input, &root, reload)?;
        }
        self.restore_native_review(&context);
        let mut shell = self
            .workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        shell
            .controller
            .restore_context(&context.planning)
            .map_err(|error| error.message)?;
        shell.index_pending_selection = context.planning.selected_issue.as_ref().map(|id| {
            workdeck_pm::projection::ProjectionRecordKey {
                repository: shell
                    .controller
                    .repository()
                    .expect("bound return context")
                    .identity()
                    .clone(),
                kind: workdeck_pm::SnapshotKind::Issue,
                id: id.to_string(),
            }
        });
        shell.poll_index(false);
        shell.navigation = None;
        shell.source_file = None;
        Ok(())
    }

    fn workbench_reveal_link(&mut self, link: &SourceLink) -> Result<(), String> {
        let file_pair = matches!(&self.options.review_input,Some(CliInput::Files(input)) if input.right==link.path && input.left==link.path);
        let (index, file) = self
            .with_state(|state| {
                state
                    .changeset()
                    .files
                    .iter()
                    .enumerate()
                    .find(|(index, file)| file.path == link.path || (file_pair && *index == 0))
                    .map(|(index, file)| (index, file.clone()))
            })
            .ok_or_else(|| "The linked file is unavailable in this review".to_owned())?;
        self.with_state(|state| state.select_file(index))
            .map_err(|error| error.to_string())?;
        let line = link.line.unwrap_or(1);
        let side = review_expansion_side(file.change_kind);
        let visible = self
            .with_state(|state| state.reveal_line(index, side, line))
            .is_ok();
        if !visible {
            let geometry = workdeck_review::review_gap_geometry_for_file(&file);
            let gap = (0..file.hunks.len())
                .filter_map(|slot| geometry.leading_gap(slot).map(|gap| (slot, gap)))
                .chain(geometry.trailing_gap().map(|gap| (file.hunks.len(), gap)))
                .find(|(_, gap)| {
                    let range = if side == ReviewSide::Old {
                        gap.old_range
                    } else {
                        gap.new_range
                    };
                    line >= range.start && line <= range.end
                });
            if let Some((slot, gap)) = gap {
                let key = (file.key.clone(), slot);
                if !self.expanded_gaps.contains(&key)
                    && let Err(error) = self.toggle_source_gap_for_file(&file.key, slot)
                {
                    self.status = Some(error.message);
                }
                let target = ReviewNoteTarget {
                    file_index: index,
                    hunk_index: gap.hunk_index,
                    side,
                    line,
                };
                if let Some(cursor) = review_line_cursors(&self.current_review_rows())
                    .into_iter()
                    .find(|cursor| cursor.target == target)
                {
                    self.apply_review_line_cursor(cursor);
                } else {
                    self.pending_source_reveal = Some(source_controller::PendingSourceReveal {
                        runtime_id: file.runtime_id,
                        gap: key,
                        target,
                    });
                }
            }
        }
        self.scroll_to_selected_line();
        self.publish_extension_selection_events();
        Ok(())
    }
}
