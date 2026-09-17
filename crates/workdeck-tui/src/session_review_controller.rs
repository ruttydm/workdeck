//! Main-thread mutations exposed to the authenticated live-session bridge.
//!
//! Hunk's React hook closes over mutable review callbacks. The native port keeps
//! those operations on the Ratatui owner thread and returns protocol values only
//! after the corresponding review mutation has committed.

use chrono::{SecondsFormat, Utc};
use workdeck_core::{
    CliInput, DiffFile, InputCursorLine, InputLayoutMode, ReviewNoteSource, ReviewSide,
    SidebarVisibility,
};
use workdeck_extension_api::{HighlightTone, ValidatedLineHighlight};
use workdeck_extension_host::{MAX_LINE_HIGHLIGHTS_PER_FILE, MAX_LINE_HIGHLIGHTS_PER_LINE};
use workdeck_review::{
    CommentTargetInput as ReviewCommentTargetInput, ReviewComment, ReviewNoteResolution,
    ReviewPublication, build_live_comment, classify_review_note_source, find_diff_file_by_path,
    resolve_comment_target,
};
use workdeck_session::{
    AppliedCommentBatchResult, AppliedCommentResult, AppliedHighlightResult, ClearedCommentsResult,
    ClearedHighlightsResult, CommentDirection, CommentTargetInput, CommentToolInput,
    HighlightToolInput, NavigateToHunkToolInput, NavigatedSelectionResult, ReloadSessionOptions,
    RemovedCommentResult, RevealedTarget, SelectedHunkSummary, SessionLineHighlightTone,
    SessionLiveCommentSummary, SessionRegistrationBootstrap,
    SessionReloadReason as BrokerReloadReason, SessionReviewNoteSummary, WorkdeckSessionSnapshot,
    create_initial_session_snapshot, no_diff_file_matches_message, update_session_registration,
};

use crate::{
    DEFAULT_STARTUP_NOTICE_DURATION, DynamicReviewHostOptions, DynamicReviewLoad,
    ExtensionPaneRuntime, ProvisionalExtensionPaneRuntime, ReviewApp, ReviewSelectionScope,
    ThemeController, agent_note_markup_width, build_selected_hunk_summary,
    resolve_review_navigation_target, session_input_kind,
};

fn protocol_side(side: ReviewSide) -> &'static str {
    match side {
        ReviewSide::Old => "old",
        ReviewSide::New => "new",
    }
}

const fn highlight_tone(tone: SessionLineHighlightTone) -> HighlightTone {
    match tone {
        SessionLineHighlightTone::Match => HighlightTone::Match,
        SessionLineHighlightTone::Current => HighlightTone::Current,
        SessionLineHighlightTone::Info => HighlightTone::Info,
        SessionLineHighlightTone::Warning => HighlightTone::Warning,
        SessionLineHighlightTone::Error => HighlightTone::Error,
        SessionLineHighlightTone::Dim => HighlightTone::Dim,
    }
}

const fn session_highlight_tone(tone: HighlightTone) -> SessionLineHighlightTone {
    match tone {
        HighlightTone::Match => SessionLineHighlightTone::Match,
        HighlightTone::Current => SessionLineHighlightTone::Current,
        HighlightTone::Info => SessionLineHighlightTone::Info,
        HighlightTone::Warning => SessionLineHighlightTone::Warning,
        HighlightTone::Error => SessionLineHighlightTone::Error,
        HighlightTone::Dim => SessionLineHighlightTone::Dim,
    }
}

fn review_comment_input(input: &CommentTargetInput) -> Result<ReviewCommentTargetInput, String> {
    Ok(ReviewCommentTargetInput {
        file_path: input.file_path.clone(),
        hunk_index: input
            .hunk_index
            .map(usize::try_from)
            .transpose()
            .map_err(|_| "Comment hunk index exceeds this platform's address space.".to_owned())?,
        side: input.side,
        line: input
            .line
            .map(u32::try_from)
            .transpose()
            .map_err(|_| "Comment line exceeds the supported source range.".to_owned())?,
        summary: input.summary.clone(),
        rationale: input.rationale.clone(),
        markup: input.markup.clone(),
        author: input.author.clone(),
    })
}

fn note_body(comment: &ReviewComment) -> String {
    match comment
        .rationale
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        Some(rationale) if !comment.summary.is_empty() => {
            format!("{}\n\n{rationale}", comment.summary)
        }
        Some(rationale) => rationale.to_owned(),
        None => comment.summary.clone(),
    }
}

fn selected_hunk(file: &DiffFile, hunk_index: usize) -> Option<SelectedHunkSummary> {
    file.hunks
        .get(hunk_index)
        .map(|_| build_selected_hunk_summary(file, hunk_index))
}

impl ReviewApp {
    pub(crate) fn session_apply_input_options(&mut self, input: &CliInput) {
        let options = input.options();
        if let Some(mode) = options.mode {
            self.with_state(|state| {
                state.set_layout(match mode {
                    InputLayoutMode::Auto => workdeck_review::LayoutMode::Auto,
                    InputLayoutMode::Split => workdeck_review::LayoutMode::Split,
                    InputLayoutMode::Stack => workdeck_review::LayoutMode::Stack,
                });
            });
        }
        if let Some(value) = options.line_numbers {
            self.options.line_numbers = value;
        }
        if let Some(value) = options.tab_width {
            self.options.tab_width = value;
        }
        if let Some(value) = options.file_gap {
            self.options.file_gap = value;
        }
        if let Some(value) = options.hunk_gap {
            self.options.hunk_gap = value;
        }
        if let Some(value) = options.wrap_lines {
            self.options.wrap_lines = value;
        }
        if let Some(value) = options.hunk_headers {
            self.options.hunk_headers = value;
        }
        if let Some(value) = options.menu_bar {
            self.show_menu_bar = value;
        }
        if let Some(value) = options.sidebar {
            self.options.sidebar_visibility = value;
            self.options.sidebar = value != SidebarVisibility::Hidden;
        }
        if let Some(value) = options.agent_notes {
            self.options.agent_notes = value;
        }
        if let Some(value) = options.copy_decorations {
            self.copy_decorations = value;
        }
        if let Some(value) = options.pager {
            self.options.pager = value;
        }
        if let Some(value) = options.prompt_save_view_preferences {
            self.options.prompt_save_view_preferences = value;
        }
        if let Some(value) = options.transparent_background {
            self.options.transparent_background = value;
            let theme_id = self.options.theme.id.clone();
            self.apply_theme_id(&theme_id);
        }
        if let Some(value) = options.cursor_line {
            self.options.cursor_line = match value {
                InputCursorLine::Row => crate::CursorLineMode::Row,
                InputCursorLine::Number => crate::CursorLineMode::Number,
                InputCursorLine::Off => crate::CursorLineMode::Off,
            };
        }
        self.options.watch = options.watch.unwrap_or(false);
        self.options.review_input = Some(input.clone());
    }

    fn session_apply_host_options(
        &mut self,
        input: &CliInput,
        host: DynamicReviewHostOptions,
        reset_app: bool,
    ) {
        self.options.command_cwd = Some(host.command_cwd);
        self.options.repo = host.repo_root;
        if let Some(provider) = host.repository_panels {
            self.options.repository_panels = provider;
        }
        self.options.startup_notices = host.startup_notices;
        self.options.custom_themes = host.custom_themes;
        self.options.keybindings = host.keybindings;
        self.options.keybinding_notices = host.keybinding_notices;
        self.options.view_preferences_config_path = host.view_preferences_config_path;
        self.options.view_preferences_write_policy = host.view_preferences_write_policy;
        self.options.prompt_save_view_preferences = host.prompt_save_view_preferences;
        if let Some(transient) = host.transient_view_preferences {
            self.options.transient_view_preferences = transient;
        }
        if let Some(pending) = host.pending_extension_trust_repo_root {
            self.options.pending_extension_trust_repo_root = pending;
        }
        if let Some(directory) = host.pending_extension_trust_directory {
            self.options.pending_extension_trust_directory = directory;
        }
        if let Some(handler) = host.extension_trust_handler {
            self.options.extension_trust_handler = Some(handler);
        }

        let requested_theme = input.options().theme.as_deref();
        if reset_app {
            self.themes = ThemeController::from_options(
                requested_theme,
                None,
                self.options.custom_themes.clone(),
                self.options.transparent_background,
            );
        } else {
            self.themes.replace_options(
                self.options.custom_themes.clone(),
                requested_theme,
                None,
                self.options.transparent_background,
            );
        }
        self.options.theme = self.themes.active_theme();
        self.startup_notices.restart(
            !self.options.pager,
            DEFAULT_STARTUP_NOTICE_DURATION,
            self.options.startup_notices.iter().cloned(),
            std::time::Instant::now(),
        );
        self.view_preference_quit.replace_inputs(
            self.options.view_preferences_config_path.clone(),
            self.options.pager,
            self.options.prompt_save_view_preferences,
            self.options.transient_view_preferences,
            self.options.view_preferences_home_directory.clone(),
        );
        self.view_preference_quit
            .set_write_policy(self.options.view_preferences_write_policy);
        self.reconcile_extension_trust_repo_root(
            self.options.pending_extension_trust_repo_root.clone(),
        );

        let mut command_defaults = crate::builtin_command_key_defaults();
        let mut runtime = self
            .extension_pane_runtime
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        command_defaults.extend(crate::bundled_search_command_defaults());
        command_defaults.extend(crate::extension_command_key_defaults(&runtime.commands));
        self.resolved_command_keys =
            crate::resolve_command_keys(&command_defaults, &self.options.keybindings);
        let table = crate::build_extension_app_commands(
            &runtime.commands,
            &crate::builtin_command_match_probes(Some(&self.resolved_command_keys)),
            Some(&self.resolved_command_keys),
            &crate::bundled_search_command_claims(&self.resolved_command_keys),
        );
        runtime.app_commands = table.commands;
        runtime.command_conflicts = table.conflicts;
    }

    pub(crate) fn session_commit_dynamic_reload(
        &mut self,
        mut loaded: DynamicReviewLoad,
        options: &ReloadSessionOptions,
    ) -> Result<workdeck_session::ReloadedSessionResult, String> {
        self.prepare_workbench_checkout_host(&mut loaded.host_options)?;
        let DynamicReviewLoad {
            input,
            changeset,
            replacement_extensions,
            replacement_vcs_catalog: _,
            host_options,
        } = loaded;
        let replacement = replacement_extensions.map(|extensions| {
            ProvisionalExtensionPaneRuntime::new(ExtensionPaneRuntime::new(
                extensions,
                &changeset.files,
            ))
        });
        let client = self.session_broker_client.clone();
        self.session_commit_reload_with_runtime(
            &input,
            changeset,
            options,
            Some(host_options),
            replacement,
            move |bootstrap, publication, snapshot| {
                let Some(client) = client else {
                    return Ok("local-session".into());
                };
                let registration =
                    update_session_registration(&client.get_registration(), bootstrap, publication)
                        .map_err(|error| error.to_string())?;
                let session_id = registration.session_id.clone();
                client
                    .replace_session(registration, snapshot.clone())
                    .map_err(|error| error.to_string())?;
                Ok(session_id)
            },
        )
    }

    #[cfg(test)]
    fn session_commit_reload_with<F>(
        &mut self,
        input: &CliInput,
        changeset: workdeck_core::Changeset,
        options: &ReloadSessionOptions,
        replace_broker_session: F,
    ) -> Result<workdeck_session::ReloadedSessionResult, String>
    where
        F: FnOnce(
            &SessionRegistrationBootstrap,
            &std::sync::Arc<ReviewPublication>,
            &WorkdeckSessionSnapshot,
        ) -> Result<String, String>,
    {
        self.session_commit_reload_with_runtime(
            input,
            changeset,
            options,
            None,
            None,
            replace_broker_session,
        )
    }

    fn session_commit_reload_with_runtime<F>(
        &mut self,
        input: &CliInput,
        changeset: workdeck_core::Changeset,
        options: &ReloadSessionOptions,
        mut host_options: Option<DynamicReviewHostOptions>,
        mut replacement: Option<ProvisionalExtensionPaneRuntime>,
        replace_broker_session: F,
    ) -> Result<workdeck_session::ReloadedSessionResult, String>
    where
        F: FnOnce(
            &SessionRegistrationBootstrap,
            &std::sync::Arc<ReviewPublication>,
            &WorkdeckSessionSnapshot,
        ) -> Result<String, String>,
    {
        if self.shutdown_requested() {
            return Err("The Workdeck review is shutting down and cannot reload.".into());
        }
        self.validate_workbench_checkout(host_options.as_ref())?;
        if let Some(host) = &host_options
            && let Some(navigation) = &host.registry_navigation
        {
            navigation.revalidate().map_err(|error| error.message)?;
            if host.repo_root.as_ref() != Some(&navigation.checkout().checkout) {
                return Err(
                    "Registered navigation does not match the loaded repository root".into(),
                );
            }
            // Resolve symlinks and parent components before accepting a nested
            // command directory. The selected review must not dispatch commands
            // through another checkout's host context.
            let command_cwd = host.command_cwd.canonicalize().map_err(|error| {
                format!("Registered navigation command cwd is unavailable: {error}")
            })?;
            if !command_cwd.starts_with(&navigation.checkout().checkout) {
                return Err("Registered navigation command cwd is outside its checkout".into());
            }
        }
        if let Some(host) = &host_options
            && let Some(Some(provider)) = &host.repository_panels
            && host.repo_root.as_ref() != Some(&provider.source().root)
        {
            return Err("Reload panel provider does not match the selected repository root".into());
        }
        let source_capabilities = host_options
            .as_mut()
            .and_then(|options| options.source_capabilities.as_mut());
        let changeset = match &mut replacement {
            Some(replacement) => replacement
                .runtime_mut()
                .apply_to_changeset_with_sources(changeset, source_capabilities),
            None => self.prepare_reloaded_changeset_with_sources(changeset, source_capabilities),
        };
        let reason = match options.reason.unwrap_or(BrokerReloadReason::Daemon) {
            BrokerReloadReason::Watch => workdeck_extension_api::SessionReloadReason::Watch,
            BrokerReloadReason::Daemon => workdeck_extension_api::SessionReloadReason::Daemon,
            BrokerReloadReason::Manual => workdeck_extension_api::SessionReloadReason::Manual,
        };
        let registration_bootstrap = SessionRegistrationBootstrap {
            input_kind: session_input_kind(Some(input), &changeset.source),
            source_label: changeset.effective_source_label().to_owned(),
            experimental: input.options().experimental.unwrap_or(false),
            initial_show_agent_notes: input
                .options()
                .agent_notes
                .unwrap_or(self.options.agent_notes),
            changeset: changeset.clone(),
        };
        let prepared = self
            .review_producer
            .prepare_publication_with_source_loader(
                &workdeck_review::PublishReviewInput {
                    files: changeset.files.clone(),
                    source_label: Some(changeset.effective_source_label().to_owned()),
                },
                crate::source_controller::publication_source_loader(
                    host_options
                        .as_ref()
                        .and_then(|host| host.source_capabilities.clone()),
                ),
            )
            .map_err(|error| error.to_string())?;
        let initial_snapshot =
            create_initial_session_snapshot(&registration_bootstrap, &prepared.publication);
        let reservation = self
            .review_producer
            .reserve_publication(prepared.clone())
            .map_err(|error| error.to_string())?;
        if let Err(error) = self.validate_workbench_checkout(host_options.as_ref()) {
            reservation.cancel();
            return Err(error);
        }
        let session_id = match replace_broker_session(
            &registration_bootstrap,
            &prepared.publication,
            &initial_snapshot,
        ) {
            Ok(session_id) => session_id,
            Err(error) => {
                reservation.cancel();
                return Err(error);
            }
        };
        let _ = reservation
            .commit(workdeck_review::ReviewPublicationCommitOptions { detach_store: true });
        self.mark_workbench_checkout_committed();
        let reset_app = options.reset_app != Some(false);
        let source_capabilities = host_options
            .as_ref()
            .and_then(|host| host.source_capabilities.clone());
        self.session_apply_input_options(input);
        if let Some(host_options) = host_options {
            self.session_apply_host_options(input, host_options, reset_app);
        }
        let replacement_installed = replacement.is_some();
        if let Some(mut replacement) = replacement {
            self.install_extension_runtime(replacement.adopt(), &changeset);
        }
        self.commit_reloaded_changeset(changeset.clone(), reason, replacement_installed, reset_app);
        if let Some(source_capabilities) = source_capabilities {
            self.install_vcs_source_capabilities(&source_capabilities);
        }
        let source_label = changeset.effective_source_label().to_owned();
        Ok(workdeck_session::ReloadedSessionResult {
            session_id,
            input_kind: registration_bootstrap.input_kind,
            title: changeset.title,
            source_label,
            file_count: u64::try_from(changeset.files.len()).unwrap_or(u64::MAX),
            selected_file_path: initial_snapshot.state.selected_file_path,
            selected_hunk_index: initial_snapshot.state.selected_hunk_index,
        })
    }

    fn markup_feedback(
        &self,
        markup: Option<&str>,
        side: ReviewSide,
    ) -> Result<(Option<u64>, Option<Vec<String>>), String> {
        let Some(markup) = markup else {
            return Ok((None, None));
        };
        if !self
            .options
            .review_input
            .as_ref()
            .and_then(|input| input.options().experimental)
            .unwrap_or(false)
        {
            return Err(
                "STML markup is disabled for this session. Relaunch Workdeck with --experimental, or omit markup."
                    .into(),
            );
        }
        let width = if self.review_geometry_published.get() {
            agent_note_markup_width(
                Some(side),
                self.layout(),
                usize::from(self.review_width.get()),
                0,
            )
        } else {
            workdeck_markup::STML_REFERENCE_WIDTH
        };
        let notes = workdeck_markup::validate_stml_markup(markup, width);
        Ok((
            Some(u64::try_from(width).unwrap_or(u64::MAX)),
            (!notes.is_empty()).then_some(notes),
        ))
    }

    fn prepare_live_comment(
        &self,
        input: &CommentTargetInput,
        comment_id: String,
        created_at: String,
    ) -> Result<(ReviewComment, AppliedCommentResult, usize), String> {
        let requested = review_comment_input(input)?;
        let Some((file, file_index)) = self.with_state(|state| {
            find_diff_file_by_path(&state.changeset().files, &input.file_path).and_then(|file| {
                state
                    .changeset()
                    .files
                    .iter()
                    .position(|candidate| candidate.runtime_id == file.runtime_id)
                    .map(|index| (file.clone(), index))
            })
        }) else {
            return Err(no_diff_file_matches_message(&input.file_path));
        };
        let target =
            resolve_comment_target(&file, &requested).map_err(|error| error.to_string())?;
        let (markup_width, markup_notes) =
            self.markup_feedback(requested.markup.as_deref(), target.side)?;
        let comment = build_live_comment(&file, requested, comment_id.clone(), created_at, target);
        Ok((
            comment,
            AppliedCommentResult {
                comment_id,
                file_id: file.runtime_id,
                file_path: file.path,
                hunk_index: u64::try_from(target.hunk_index).unwrap_or(u64::MAX),
                side: target.side,
                line: u64::from(target.line),
                markup_width,
                markup_notes,
            },
            file_index,
        ))
    }

    fn publish_session_note_mutation(&mut self) {
        self.commit_extension_runtime_bridge();
        let events = self.update_extension_review_events(std::time::Instant::now());
        self.publish_extension_lifecycle_events(events);
    }

    pub(crate) fn session_add_live_comment(
        &mut self,
        input: &CommentToolInput,
        comment_id: &str,
        reveal: bool,
    ) -> Result<AppliedCommentResult, String> {
        let (comment, result, file_index) = self.prepare_live_comment(
            &input.target,
            comment_id.to_owned(),
            Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        )?;
        self.with_state(|state| state.add_comment(comment))
            .map_err(|error| error.to_string())?;
        if reveal {
            let hunk_index = usize::try_from(result.hunk_index).unwrap_or(usize::MAX);
            self.navigate(|state| state.select_hunk(file_index, hunk_index).is_ok());
        }
        self.publish_session_note_mutation();
        Ok(result)
    }

    pub(crate) fn session_add_live_comment_batch(
        &mut self,
        comments: &[CommentTargetInput],
        request_id: &str,
        reveal_first: bool,
    ) -> Result<AppliedCommentBatchResult, String> {
        let created_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
        let mut prepared = Vec::with_capacity(comments.len());
        for (index, input) in comments.iter().enumerate() {
            prepared.push(self.prepare_live_comment(
                input,
                format!("mcp:{request_id}:{index}"),
                created_at.clone(),
            )?);
        }
        let existing_ids = self.with_state(|state| {
            state
                .comments()
                .iter()
                .map(|comment| comment.id.clone())
                .collect::<std::collections::BTreeSet<_>>()
        });
        if let Some((comment, _, _)) = prepared
            .iter()
            .find(|(comment, _, _)| existing_ids.contains(&comment.id))
        {
            return Err(format!("comment {:?} already exists", comment.id));
        }
        self.with_state(|state| {
            for (comment, _, _) in &prepared {
                state
                    .add_comment(comment.clone())
                    .expect("all batch comment identities were preflighted");
            }
        });
        if reveal_first && let Some((_, result, file_index)) = prepared.first() {
            let hunk_index = usize::try_from(result.hunk_index).unwrap_or(usize::MAX);
            let file_index = *file_index;
            self.navigate(|state| state.select_hunk(file_index, hunk_index).is_ok());
        }
        if !prepared.is_empty() {
            self.publish_session_note_mutation();
        }
        Ok(AppliedCommentBatchResult {
            applied: prepared.into_iter().map(|(_, result, _)| result).collect(),
        })
    }

    pub(crate) fn session_remove_live_comment(
        &mut self,
        comment_id: &str,
    ) -> Result<RemovedCommentResult, String> {
        let has_replies = self.with_state(|state| {
            state.comments().iter().any(|note| note.id == comment_id)
                && state
                    .comments()
                    .iter()
                    .any(|note| note.parent_id.as_deref() == Some(comment_id))
        });
        if has_replies {
            return Err(format!(
                "Review note {comment_id} cannot be removed while it has replies."
            ));
        }
        let removed = self
            .with_state(|state| state.remove_comment(comment_id))
            .ok_or_else(|| {
                if comment_id.starts_with("user:") {
                    format!("No user note matches id {comment_id}.")
                } else {
                    format!("No live comment matches id {comment_id}.")
                }
            })?;
        let source = classify_review_note_source(&removed.source);
        let remaining_comment_count =
            self.with_state(|state| u64::try_from(state.comments().len()).unwrap_or(u64::MAX));
        self.publish_session_note_mutation();
        Ok(RemovedCommentResult {
            comment_id: comment_id.to_owned(),
            removed: true,
            remaining_comment_count,
            source: Some(source),
        })
    }

    pub(crate) fn session_clear_live_comments(
        &mut self,
        file_path: Option<&str>,
        include_user: Option<bool>,
    ) -> Result<ClearedCommentsResult, String> {
        let file_key = match file_path {
            Some(path) => Some(
                self.with_state(|state| {
                    find_diff_file_by_path(&state.changeset().files, path)
                        .map(|file| file.key.clone())
                })
                .ok_or_else(|| no_diff_file_matches_message(path))?,
            ),
            None => None,
        };
        let include_user_value = include_user.unwrap_or(false);
        let (removed_live, removed_user, remaining_live, remaining_user) =
            self.with_state(|state| {
                let ids = state
                    .comments()
                    .iter()
                    .filter(|comment| {
                        comment.resolution != ReviewNoteResolution::Orphaned
                            && file_key
                                .as_ref()
                                .is_none_or(|file_key| &comment.anchor.file_key == file_key)
                            && (classify_review_note_source(&comment.source)
                                != ReviewNoteSource::User
                                || include_user_value)
                    })
                    .map(|comment| comment.id.clone())
                    .collect::<Vec<_>>();
                let mut removed_live = 0_usize;
                let mut removed_user = 0_usize;
                for id in ids {
                    if let Some(comment) = state.remove_comment(&id) {
                        if classify_review_note_source(&comment.source) == ReviewNoteSource::User {
                            removed_user += 1;
                        } else {
                            removed_live += 1;
                        }
                    }
                }
                let remaining_live = state
                    .comments()
                    .iter()
                    .filter(|comment| {
                        classify_review_note_source(&comment.source) != ReviewNoteSource::User
                    })
                    .count();
                let remaining_user = state.comments().len().saturating_sub(remaining_live);
                (removed_live, removed_user, remaining_live, remaining_user)
            });
        if removed_live + removed_user > 0 {
            self.publish_session_note_mutation();
        }
        let to_u64 = |value| u64::try_from(value).unwrap_or(u64::MAX);
        Ok(ClearedCommentsResult {
            removed_count: to_u64(removed_live + removed_user),
            remaining_comment_count: to_u64(remaining_live + remaining_user),
            file_path: file_path.map(str::to_owned),
            include_user,
            removed_live_comment_count: Some(to_u64(removed_live)),
            removed_user_note_count: Some(to_u64(removed_user)),
            remaining_live_comment_count: Some(to_u64(remaining_live)),
            remaining_user_note_count: Some(to_u64(remaining_user)),
        })
    }

    fn current_navigation_result(&self) -> Result<NavigatedSelectionResult, String> {
        self.with_state(|state| {
            let selection = state.selection();
            let file = state
                .changeset()
                .files
                .get(selection.file_index)
                .ok_or_else(|| "The current review has no selected file.".to_owned())?;
            let hunk_index = selection.hunk_index.unwrap_or(0);
            Ok(NavigatedSelectionResult {
                file_id: file.runtime_id.clone(),
                file_path: file.path.clone(),
                hunk_index: u64::try_from(hunk_index).unwrap_or(u64::MAX),
                selected_hunk: selected_hunk(file, hunk_index),
                revealed: None,
                side: None,
                line: None,
            })
        })
    }

    pub(crate) fn session_navigate_to_location(
        &mut self,
        input: &NavigateToHunkToolInput,
    ) -> Result<NavigatedSelectionResult, String> {
        if let Some(direction) = input.comment_direction {
            let before = self.with_state(|state| state.selection());
            self.move_selection(
                ReviewSelectionScope::AnnotatedHunk,
                match direction {
                    CommentDirection::Next => 1,
                    CommentDirection::Prev => -1,
                },
            );
            if self.with_state(|state| state.selection()) == before {
                return Err("No annotated hunks found in the current review.".into());
            }
            return self.current_navigation_result();
        }

        let (file_index, file, hunk_index) = self
            .with_state(|state| {
                let target = resolve_review_navigation_target(&state.changeset().files, input)?;
                let file_index = state
                    .changeset()
                    .files
                    .iter()
                    .position(|file| file.runtime_id == target.file.runtime_id)
                    .expect("resolved navigation file belongs to this changeset");
                Ok::<_, crate::ReviewNavigationTargetError>((
                    file_index,
                    target.file.clone(),
                    target.hunk_index,
                ))
            })
            .map_err(|error| error.to_string())?;

        if input.hunk_index.is_none()
            && let (Some(side), Some(line)) = (input.side, input.line)
        {
            let line = u32::try_from(line)
                .map_err(|_| "Navigation line exceeds the supported source range.".to_owned())?;
            let visible = workdeck_review::review_file_matches_filter(
                &workdeck_core::project_review_file(&file, "terminal-review", file_index),
                &self.filter,
            );
            if visible
                && self
                    .with_state(|state| state.reveal_line(file_index, side, line))
                    .is_ok()
            {
                let revealed = if !self.has_measured_review_line(file_index, side, line) {
                    self.with_state(|state| state.select_hunk(file_index, hunk_index))
                        .map_err(|error| error.to_string())?;
                    self.scroll_to_selection();
                    RevealedTarget::Hunk
                } else {
                    self.scroll_to_selected_line();
                    RevealedTarget::Line
                };
                self.publish_extension_selection_events();
                let selected_hunk = selected_hunk(&file, hunk_index);
                return Ok(NavigatedSelectionResult {
                    file_id: file.runtime_id,
                    file_path: file.path,
                    hunk_index: u64::try_from(hunk_index).unwrap_or(u64::MAX),
                    selected_hunk,
                    revealed: Some(revealed),
                    side: Some(side),
                    line: Some(u64::from(line)),
                });
            }
        }
        self.navigate(|state| state.select_hunk(file_index, hunk_index).is_ok());
        let selected_hunk = selected_hunk(&file, hunk_index);
        Ok(NavigatedSelectionResult {
            file_id: file.runtime_id,
            file_path: file.path,
            hunk_index: u64::try_from(hunk_index).unwrap_or(u64::MAX),
            selected_hunk,
            revealed: input.line.map(|_| RevealedTarget::Hunk),
            side: None,
            line: None,
        })
    }

    pub(crate) fn session_add_agent_line_highlight(
        &mut self,
        input: &HighlightToolInput,
    ) -> Result<AppliedHighlightResult, String> {
        let (file_index, file, hunk_index) = self.with_state(|state| {
            let file = find_diff_file_by_path(&state.changeset().files, &input.file_path)
                .ok_or_else(|| no_diff_file_matches_message(&input.file_path))?;
            let file_index = state
                .changeset()
                .files
                .iter()
                .position(|candidate| candidate.runtime_id == file.runtime_id)
                .expect("matched file belongs to this changeset");
            let line = u32::try_from(input.line)
                .map_err(|_| "Highlight line exceeds the supported source range.".to_owned())?;
            let hunk_index = file.hunk_at_line(input.side, line).ok_or_else(|| {
                format!(
                    "No {} diff hunk in {} covers line {}.",
                    protocol_side(input.side),
                    input.file_path,
                    input.line
                )
            })?;
            Ok::<_, String>((file_index, file.clone(), hunk_index))
        })?;
        if input.start >= input.end {
            return Err(format!(
                "Highlight range [{}, {}) is not a valid [start, end) character range.",
                input.start, input.end
            ));
        }
        let existing = self
            .agent_line_highlights
            .get(&file.runtime_id)
            .unwrap_or_default();
        let existing_count = existing.len();
        if existing_count >= MAX_LINE_HIGHLIGHTS_PER_FILE {
            return Err(format!(
                "{} already carries {MAX_LINE_HIGHLIGHTS_PER_FILE} attention marks. Clear some first.",
                input.file_path
            ));
        }
        let line_count = existing
            .iter()
            .filter(|mark| mark.side == input.side && mark.line == input.line)
            .count();
        if line_count >= MAX_LINE_HIGHLIGHTS_PER_LINE {
            return Err(format!(
                "{}:{} already carries {MAX_LINE_HIGHLIGHTS_PER_LINE} attention marks. Clear some first.",
                input.file_path, input.line
            ));
        }
        let mark = ValidatedLineHighlight {
            side: input.side,
            line: input.line,
            start: input.start,
            end: input.end,
            tone: highlight_tone(input.tone.unwrap_or(SessionLineHighlightTone::Match)),
        };
        let mut next = existing.to_vec();
        next.push(mark.clone());
        self.agent_line_highlights = self
            .agent_line_highlights
            .with_file_marks(file.runtime_id.clone(), next);

        let revealed = if input.reveal.unwrap_or(false) {
            let line = u32::try_from(input.line).expect("line conversion was preflighted");
            let visible = workdeck_review::review_file_matches_filter(
                &workdeck_core::project_review_file(&file, "terminal-review", file_index),
                &self.filter,
            );
            if visible
                && self.has_measured_review_line(file_index, input.side, line)
                && self
                    .with_state(|state| state.reveal_line(file_index, input.side, line))
                    .is_ok()
            {
                self.scroll_to_selected_line();
                self.publish_extension_selection_events();
                Some(RevealedTarget::Line)
            } else {
                self.navigate(|state| state.select_hunk(file_index, hunk_index).is_ok());
                Some(RevealedTarget::Hunk)
            }
        } else {
            None
        };
        Ok(AppliedHighlightResult {
            file_id: file.runtime_id,
            file_path: file.path,
            hunk_index: u64::try_from(hunk_index).unwrap_or(u64::MAX),
            side: mark.side,
            line: mark.line,
            start: mark.start,
            end: mark.end,
            tone: session_highlight_tone(mark.tone),
            file_mark_count: u64::try_from(existing_count + 1).unwrap_or(u64::MAX),
            revealed,
        })
    }

    pub(crate) fn session_clear_agent_line_highlights(
        &mut self,
        file_path: Option<&str>,
    ) -> Result<ClearedHighlightsResult, String> {
        let total = self.agent_line_highlights.mark_count();
        let Some(file_path) = file_path else {
            self.agent_line_highlights = crate::LineHighlightMap::default();
            return Ok(ClearedHighlightsResult {
                removed_count: u64::try_from(total).unwrap_or(u64::MAX),
                remaining_count: 0,
                file_path: None,
            });
        };
        let file = self
            .with_state(|state| {
                find_diff_file_by_path(&state.changeset().files, file_path).cloned()
            })
            .ok_or_else(|| no_diff_file_matches_message(file_path))?;
        let removed = self
            .agent_line_highlights
            .get(&file.runtime_id)
            .map_or(0, <[ValidatedLineHighlight]>::len);
        if removed > 0 {
            self.agent_line_highlights = self
                .agent_line_highlights
                .with_file_marks(file.runtime_id, Vec::new());
        }
        Ok(ClearedHighlightsResult {
            removed_count: u64::try_from(removed).unwrap_or(u64::MAX),
            remaining_count: u64::try_from(total.saturating_sub(removed)).unwrap_or(u64::MAX),
            file_path: Some(file_path.to_owned()),
        })
    }

    pub(crate) fn session_open_agent_notes(&mut self) {
        self.options.agent_notes = true;
    }

    pub(crate) fn session_live_comment_summaries(&self) -> Vec<SessionLiveCommentSummary> {
        self.with_state(|state| {
            state
                .comments()
                .iter()
                .filter(|comment| {
                    classify_review_note_source(&comment.source) != ReviewNoteSource::User
                })
                .filter_map(|comment| {
                    Some(SessionLiveCommentSummary {
                        comment_id: comment.id.clone(),
                        file_path: comment.file_path.clone()?,
                        hunk_index: u64::try_from(comment.hunk_index?).unwrap_or(u64::MAX),
                        side: comment.side?,
                        line: u64::from(comment.line?),
                        summary: comment.summary.clone(),
                        rationale: comment.rationale.clone(),
                        author: comment.author.clone(),
                        created_at: comment
                            .created_at
                            .clone()
                            .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into()),
                    })
                })
                .collect()
        })
    }

    pub(crate) fn session_review_note_summaries(&self) -> Vec<SessionReviewNoteSummary> {
        self.with_state(|state| {
            let mut summaries = Vec::new();
            for file in &state.changeset().files {
                for (index, annotation) in file
                    .agent
                    .as_ref()
                    .map(|agent| agent.annotations.as_slice())
                    .unwrap_or_default()
                    .iter()
                    .enumerate()
                {
                    let source = crate::review_note_source(annotation);
                    summaries.push(SessionReviewNoteSummary {
                        note_id: annotation.id.clone().unwrap_or_else(|| {
                            format!(
                                "{}:{}:{index}",
                                match source {
                                    ReviewNoteSource::Ai => "ai",
                                    ReviewNoteSource::Agent => "agent",
                                    ReviewNoteSource::User => "user",
                                },
                                file.runtime_id
                            )
                        }),
                        parent_id: None,
                        source,
                        file_path: file.path.clone(),
                        hunk_index: None,
                        old_range: annotation
                            .old_range
                            .map(|range| [u64::from(range.start), u64::from(range.end)]),
                        new_range: annotation
                            .new_range
                            .map(|range| [u64::from(range.start), u64::from(range.end)]),
                        body: match annotation
                            .rationale
                            .as_deref()
                            .filter(|value| !value.is_empty())
                        {
                            Some(rationale) if !annotation.summary.is_empty() => {
                                format!("{}\n\n{rationale}", annotation.summary)
                            }
                            Some(rationale) => rationale.to_owned(),
                            None => annotation.summary.clone(),
                        },
                        title: annotation.title.clone(),
                        author: annotation.author.clone(),
                        created_at: annotation
                            .created_at
                            .clone()
                            .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into()),
                        updated_at: annotation.updated_at.clone(),
                        editable: false,
                    });
                }
                for comment in state.comments().iter().filter(|comment| {
                    comment.anchor.file_key == file.key
                        && comment.resolution != ReviewNoteResolution::Orphaned
                }) {
                    summaries.push(SessionReviewNoteSummary {
                        note_id: comment.id.clone(),
                        parent_id: comment.parent_id.clone(),
                        source: classify_review_note_source(&comment.source),
                        file_path: file.path.clone(),
                        hunk_index: comment
                            .hunk_index
                            .map(|index| u64::try_from(index).unwrap_or(u64::MAX)),
                        old_range: comment
                            .anchor
                            .old_range
                            .map(|range| [u64::from(range.start), u64::from(range.end)]),
                        new_range: comment
                            .anchor
                            .new_range
                            .map(|range| [u64::from(range.start), u64::from(range.end)]),
                        body: note_body(comment),
                        title: comment.title.clone(),
                        author: comment.author.clone(),
                        created_at: comment
                            .created_at
                            .clone()
                            .unwrap_or_else(|| "1970-01-01T00:00:00.000Z".into()),
                        updated_at: comment.updated_at.clone(),
                        editable: classify_review_note_source(&comment.source)
                            == ReviewNoteSource::User,
                    });
                }
            }
            summaries
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    fn vcs_load(
        text: &str,
    ) -> (
        workdeck_core::Changeset,
        workdeck_vcs::VcsSourceCapabilities,
    ) {
        let text = text.to_owned();
        let (changeset, capabilities) = workdeck_vcs::materialize_vcs_patch_result_deferred(
            workdeck_vcs::VcsPatchResult {
                repo_root: std::path::PathBuf::from("."),
                source_label: "reload-source".into(),
                title: "reload-source".into(),
                patch_text:
                    "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -3 +3 @@\n-old\n+new\n"
                        .into(),
                untracked_paths: vec![],
                extra_files: vec![],
                source_cache_key: None,
                source_reader: Some(Arc::new(move |_| {
                    Ok(workdeck_vcs::VcsFileSourceResult::Source(
                        workdeck_core::SourceSnapshot::new(
                            text.clone(),
                            workdeck_core::SourceOrigin::WorkingTree,
                            false,
                        ),
                    ))
                })),
            },
            "reload-source",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        (changeset, capabilities)
    }

    #[test]
    fn replacing_unattested_vcs_handles_refetches_an_open_gap_with_unchanged_identity() {
        fn published_source(app: &ReviewApp) -> String {
            let producer = app.review_producer();
            let descriptor = producer
                .describe_resources()
                .into_iter()
                .find(|descriptor| {
                    matches!(
                        descriptor,
                        workdeck_review::ReviewResourceDescriptor::Source {
                            side: ReviewSide::New,
                            ..
                        }
                    )
                })
                .unwrap();
            let id = descriptor.base().id.clone();
            let resources = producer
                .materialize_resources(std::slice::from_ref(&id))
                .unwrap();
            String::from_utf8(resources[&id].as_ref().unwrap().bytes.to_vec()).unwrap()
        }
        let (initial, initial_handles) = vcs_load("before-one\nbefore-two\nnew\n");
        let identity = initial.files[0].source_identity.clone();
        let mut app = ReviewApp::new(
            initial,
            ReviewOptions {
                source_capabilities: Some(initial_handles),
                highlight: false,
                ..ReviewOptions::default()
            },
        );
        assert_eq!(published_source(&app), "before-one\nbefore-two\nnew\n");
        app.toggle_source_gap();
        crate::source_controller::tests::drain_one(&mut app);
        assert!(crate::source_controller::tests::rows(&app).contains("before-one"));
        let (replacement, replacement_handles) = vcs_load("after-one\nafter-two\nnew\n");
        assert_eq!(replacement.files[0].source_identity, identity);
        assert!(!replacement.files[0].source_attested);
        let before = app.review_producer().get_publication_address();
        let failed = app.session_commit_reload_with_runtime(
            &patch_input("input.patch"),
            replacement.clone(),
            &ReloadSessionOptions {
                reset_app: Some(false),
                ..Default::default()
            },
            Some(crate::DynamicReviewHostOptions {
                source_capabilities: Some(replacement_handles.clone()),
                ..Default::default()
            }),
            None,
            |_, _, _| Err("registration unavailable".into()),
        );
        assert_eq!(failed.unwrap_err(), "registration unavailable");
        assert_eq!(app.review_producer().get_publication_address(), before);
        assert_eq!(published_source(&app), "before-one\nbefore-two\nnew\n");
        app.session_commit_dynamic_reload(
            crate::DynamicReviewLoad {
                input: workdeck_core::CliInput::Patch(workdeck_core::PatchCommandInput {
                    file: Some("input.patch".into()),
                    text: None,
                    options: Default::default(),
                }),
                changeset: replacement,
                replacement_extensions: None,
                replacement_vcs_catalog: None,
                host_options: crate::DynamicReviewHostOptions {
                    source_capabilities: Some(replacement_handles),
                    command_cwd: std::env::current_dir().unwrap(),
                    ..Default::default()
                },
            },
            &ReloadSessionOptions {
                reset_app: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(published_source(&app), "after-one\nafter-two\nnew\n");
        assert!(!app.expanded_gaps.is_empty());
        crate::source_controller::tests::drain_one(&mut app);
        let text = crate::source_controller::tests::rows(&app);
        assert!(text.contains("after-one"), "{text}");
        assert!(!text.contains("before-one"));
        app.install_vcs_source_capabilities(&workdeck_vcs::VcsSourceCapabilities::default());
        assert!(app.source_loaders.is_empty());
        assert!(!crate::source_controller::tests::rows(&app).contains("after-one"));
    }
    use workdeck_core::{
        ChangesetSource, CliInput, CommonOptions, PatchCommandInput, ReviewSide, StartupNotice,
        UserKeyBinding, UserKeyBindingEntry,
    };
    use workdeck_diff::{LanguageMatcher, LanguageRegistration, parse_patch};
    use workdeck_session::{
        CommentTargetInput, CommentToolInput, HighlightToolInput, ReloadSessionOptions,
        SessionLineHighlightTone, SessionSelector,
    };

    use super::*;
    use crate::ReviewOptions;

    fn changeset(path: &str, old: &str, new: &str) -> workdeck_core::Changeset {
        parse_patch(
            &format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-{old}\n+{new}\n"
            ),
            "test",
            "Working tree",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap()
    }

    fn patch_input(path: &str) -> CliInput {
        CliInput::Patch(PatchCommandInput {
            file: Some(path.into()),
            text: None,
            options: CommonOptions::default(),
        })
    }

    fn comment_target(file_path: &str, summary: &str) -> CommentTargetInput {
        CommentTargetInput {
            file_path: file_path.into(),
            hunk_index: Some(0),
            side: Some(ReviewSide::New),
            line: Some(1),
            summary: summary.into(),
            rationale: None,
            markup: None,
            author: Some("agent".into()),
        }
    }

    #[test]
    fn broker_failure_cancels_the_publication_and_preserves_the_mounted_review() {
        let initial_input = patch_input("initial.patch");
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions {
                review_input: Some(initial_input.clone()),
                ..ReviewOptions::default()
            },
        );
        let initial_generation = app.review_producer().get_publication_address();
        let initial_changeset = app.with_state(|state| state.changeset().clone());
        let replacement_input = patch_input("replacement.patch");

        let error = app
            .session_commit_reload_with(
                &replacement_input,
                changeset("after.rs", "old", "after"),
                &ReloadSessionOptions::default(),
                |_, _, _| Err("broker exploded".into()),
            )
            .unwrap_err();

        assert_eq!(error, "broker exploded");
        assert_eq!(
            app.review_producer().get_publication_address(),
            initial_generation
        );
        assert_eq!(
            app.with_state(|state| state.changeset().clone()),
            initial_changeset
        );
        assert_eq!(app.options.review_input.as_ref(), Some(&initial_input));

        let committed = app
            .session_commit_reload_with(
                &replacement_input,
                changeset("after.rs", "old", "after"),
                &ReloadSessionOptions::default(),
                |_, publication, snapshot| {
                    assert_eq!(publication.document.files[0].path, "after.rs");
                    assert_eq!(
                        snapshot.state.selected_file_path.as_deref(),
                        Some("after.rs")
                    );
                    Ok("session-1".into())
                },
            )
            .unwrap();
        assert_eq!(committed.session_id, "session-1");
        assert_eq!(
            app.with_state(|state| state.changeset().files[0].path.clone()),
            "after.rs"
        );
        assert_ne!(
            app.review_producer().get_publication_address(),
            initial_generation
        );
        assert_eq!(app.options.review_input.as_ref(), Some(&replacement_input));
    }

    #[test]
    fn shutdown_refuses_reload_before_loading_or_broker_commit() {
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions::default(),
        );
        app.should_quit = true;
        let mut broker_called = false;
        let error = app
            .session_commit_reload_with(
                &patch_input("replacement.patch"),
                changeset("after.rs", "old", "after"),
                &ReloadSessionOptions::default(),
                |_, _, _| {
                    broker_called = true;
                    Ok("session-1".into())
                },
            )
            .unwrap_err();
        assert_eq!(
            error,
            "The Workdeck review is shutting down and cannot reload."
        );
        assert!(!broker_called);
    }

    #[test]
    fn replacement_registry_is_adopted_only_after_the_broker_commit_gate() {
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions::default(),
        );
        let initial_registry_generation = app.extension_registry_generation;
        let failed_changeset = changeset("failed.rs", "old", "failed");
        let failed_replacement = ProvisionalExtensionPaneRuntime::new(ExtensionPaneRuntime::new(
            Vec::new(),
            &failed_changeset.files,
        ));
        assert_eq!(
            app.session_commit_reload_with_runtime(
                &patch_input("failed.patch"),
                failed_changeset,
                &ReloadSessionOptions::default(),
                None,
                Some(failed_replacement),
                |_, _, _| Err("broker refused replacement".into()),
            )
            .unwrap_err(),
            "broker refused replacement"
        );
        assert_eq!(
            app.extension_registry_generation,
            initial_registry_generation
        );
        assert_eq!(
            app.with_state(|state| state.changeset().files[0].path.clone()),
            "before.rs"
        );

        let committed_changeset = changeset("committed.rs", "old", "committed");
        let committed_replacement = ProvisionalExtensionPaneRuntime::new(
            ExtensionPaneRuntime::new(Vec::new(), &committed_changeset.files),
        );
        app.session_commit_reload_with_runtime(
            &patch_input("committed.patch"),
            committed_changeset,
            &ReloadSessionOptions::default(),
            None,
            Some(committed_replacement),
            |_, _, _| Ok("session-1".into()),
        )
        .unwrap();
        assert_eq!(
            app.extension_registry_generation,
            initial_registry_generation + 1
        );
        assert_eq!(
            app.with_state(|state| state.changeset().files[0].path.clone()),
            "committed.rs"
        );
    }

    #[test]
    fn failed_replacement_discards_its_session_local_file_language_registry() {
        let mut app = ReviewApp::new(
            changeset("before.currentlang", "old", "before"),
            ReviewOptions::default(),
        );
        {
            let mut runtime = app
                .extension_pane_runtime
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            runtime.file_languages = vec![LanguageRegistration {
                matcher: LanguageMatcher::Extension("currentlang".into()),
                language: "python".into(),
                reserved: false,
            }];
        }
        let initial_changeset = app.with_state(|state| state.changeset().clone());
        let failed_changeset = changeset("failed.replacementlang", "old", "failed");
        let mut replacement_runtime =
            ExtensionPaneRuntime::new(Vec::new(), &failed_changeset.files);
        replacement_runtime.file_languages = vec![LanguageRegistration {
            matcher: LanguageMatcher::Extension("replacementlang".into()),
            language: "ruby".into(),
            reserved: false,
        }];

        let error = app
            .session_commit_reload_with_runtime(
                &patch_input("failed.patch"),
                failed_changeset,
                &ReloadSessionOptions::default(),
                None,
                Some(ProvisionalExtensionPaneRuntime::new(replacement_runtime)),
                |bootstrap, publication, _| {
                    assert_eq!(
                        bootstrap.changeset.files[0].language.as_deref(),
                        Some("ruby")
                    );
                    assert_eq!(
                        publication.document.files[0].language.as_deref(),
                        Some("ruby")
                    );
                    Err("broker refused replacement".into())
                },
            )
            .unwrap_err();

        assert_eq!(error, "broker refused replacement");
        assert_eq!(
            app.with_state(|state| state.changeset().clone()),
            initial_changeset
        );
        assert_eq!(
            app.prepare_reloaded_changeset(changeset("still.currentlang", "old", "current"))
                .files[0]
                .language
                .as_deref(),
            Some("python")
        );
        assert_eq!(
            app.prepare_reloaded_changeset(changeset(
                "not-leaked.replacementlang",
                "old",
                "current"
            ))
            .files[0]
                .language,
            None
        );
    }

    #[test]
    fn replacement_lifecycle_is_published_only_after_the_broker_commit_gate() {
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions {
                command_cwd: Some("/repo/old".into()),
                ..ReviewOptions::default()
            },
        );
        app.observed_extension_events.clear();
        let initial_registry_generation = app.extension_registry_generation;
        let next_changeset = changeset("after.rs", "old", "after");
        let replacement = ProvisionalExtensionPaneRuntime::new(ExtensionPaneRuntime::new(
            Vec::new(),
            &next_changeset.files,
        ));
        let mut broker_committed = false;

        app.session_commit_reload_with_runtime(
            &patch_input("after.patch"),
            next_changeset,
            &ReloadSessionOptions {
                reason: Some(workdeck_session::SessionReloadReason::Watch),
                ..ReloadSessionOptions::default()
            },
            Some(DynamicReviewHostOptions {
                command_cwd: "/repo/new".into(),
                repo_root: Some("/repo/new".into()),
                ..DynamicReviewHostOptions::default()
            }),
            Some(replacement),
            |bootstrap, publication, snapshot| {
                assert_eq!(bootstrap.changeset.files[0].path, "after.rs");
                assert_eq!(publication.document.files[0].path, "after.rs");
                assert_eq!(
                    snapshot.state.selected_file_path.as_deref(),
                    Some("after.rs")
                );
                broker_committed = true;
                Ok("session-1".into())
            },
        )
        .unwrap();

        assert!(broker_committed);
        assert_eq!(
            app.extension_registry_generation,
            initial_registry_generation + 1
        );
        assert!(
            app.observed_extension_events
                .iter()
                .all(|(generation, _, _)| { *generation == initial_registry_generation + 1 })
        );
        assert_eq!(
            app.observed_extension_events
                .iter()
                .map(|(_, name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["startup", "changeset_loaded", "session_reload"]
        );
        assert_eq!(app.observed_extension_events[0].2["cwd"], "/repo/new");
        assert_eq!(app.observed_extension_events[2].2["reason"], "watch");
    }

    #[test]
    fn same_runtime_lifecycle_observes_the_committed_review_without_a_second_startup() {
        let initial_input = patch_input("before.patch");
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions {
                review_input: Some(initial_input),
                command_cwd: Some("/repo".into()),
                ..ReviewOptions::default()
            },
        );
        app.observed_extension_events.clear();
        let registry_generation = app.extension_registry_generation;
        let next_input = patch_input("after.patch");
        let mut broker_committed = false;

        app.session_commit_reload_with(
            &next_input,
            changeset("after.rs", "old", "after"),
            &ReloadSessionOptions {
                reason: Some(workdeck_session::SessionReloadReason::Daemon),
                reset_app: Some(false),
                ..ReloadSessionOptions::default()
            },
            |bootstrap, publication, snapshot| {
                assert_eq!(bootstrap.changeset.files[0].path, "after.rs");
                assert_eq!(publication.document.files[0].path, "after.rs");
                assert_eq!(
                    snapshot.state.selected_file_path.as_deref(),
                    Some("after.rs")
                );
                broker_committed = true;
                Ok("session-1".into())
            },
        )
        .unwrap();

        assert!(broker_committed);
        assert_eq!(app.extension_registry_generation, registry_generation);
        assert_eq!(
            app.with_state(|state| state.changeset().files[0].path.clone()),
            "after.rs"
        );
        assert_eq!(
            app.observed_extension_events
                .iter()
                .map(|(generation, name, _)| (*generation, name.as_str()))
                .collect::<Vec<_>>(),
            [
                (registry_generation, "changeset_loaded"),
                (registry_generation, "session_reload")
            ]
        );
        assert_eq!(app.observed_extension_events[1].2["reason"], "daemon");
    }

    #[test]
    fn reset_app_false_preserves_view_state_while_default_reload_resets_it() {
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions::default(),
        );
        app.filter = "needle".into();
        app.filter_cursor = app.filter.len();
        app.scroll = 9;
        app.session_commit_reload_with(
            &patch_input("soft.patch"),
            changeset("before.rs", "old", "soft"),
            &ReloadSessionOptions {
                reset_app: Some(false),
                ..ReloadSessionOptions::default()
            },
            |_, _, _| Ok("session-1".into()),
        )
        .unwrap();
        assert_eq!(app.filter, "needle");
        assert_eq!(app.filter_cursor, 6);
        assert_eq!(app.scroll, 9);

        app.session_commit_reload_with(
            &patch_input("hard.patch"),
            changeset("before.rs", "old", "hard"),
            &ReloadSessionOptions::default(),
            |_, _, _| Ok("session-1".into()),
        )
        .unwrap();
        assert!(app.filter.is_empty());
        assert_eq!(app.filter_cursor, 0);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn preference_source_policy_is_replaced_on_soft_reload_without_global_fallback() {
        let directory = tempfile::TempDir::new().unwrap();
        let path = directory.path().join("legacy.toml");
        let original = "# legacy\nmode='split'\n";
        std::fs::write(&path, original).unwrap();
        let input = patch_input("source.patch");
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions::default(),
        );
        app.session_apply_host_options(
            &input,
            DynamicReviewHostOptions {
                view_preferences_config_path: Some(path.clone()),
                view_preferences_write_policy:
                    workdeck_core::ViewPreferenceWritePolicy::LegacyReadOnly,
                prompt_save_view_preferences: true,
                ..DynamicReviewHostOptions::default()
            },
            false,
        );
        let current = app.current_view_preferences();
        let error = app
            .view_preference_quit
            .save_view_preferences_and_schedule_quit(&current, std::time::Instant::now())
            .unwrap_err();
        assert!(matches!(
            error,
            workdeck_core::ViewPreferencePersistenceError::LegacyReadOnly
        ));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
        let native = directory.path().join(".workdeck/config.toml");
        app.session_apply_host_options(
            &input,
            DynamicReviewHostOptions {
                view_preferences_config_path: Some(native.clone()),
                view_preferences_write_policy: workdeck_core::ViewPreferenceWritePolicy::Writable,
                prompt_save_view_preferences: true,
                ..DynamicReviewHostOptions::default()
            },
            false,
        );
        assert_eq!(
            app.view_preference_quit
                .save_view_preferences_and_schedule_quit(&current, std::time::Instant::now())
                .unwrap(),
            Some(native)
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), original);
    }

    #[test]
    fn dynamic_reload_commits_resolved_host_options_with_the_review() {
        let initial_input = patch_input("initial.patch");
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions {
                review_input: Some(initial_input),
                command_cwd: Some("/old/cwd".into()),
                repo: Some("/old/repo".into()),
                pending_extension_trust_repo_root: Some("/old/trust".into()),
                ..ReviewOptions::default()
            },
        );
        let replacement_input = CliInput::Patch(PatchCommandInput {
            file: Some("replacement.patch".into()),
            text: None,
            options: CommonOptions {
                pager: Some(true),
                prompt_save_view_preferences: Some(false),
                watch: Some(true),
                ..CommonOptions::default()
            },
        });
        let binding =
            UserKeyBindingEntry::new("workdeck.review.wrap", UserKeyBinding::Chord("w".into()));
        app.session_commit_dynamic_reload(
            DynamicReviewLoad {
                input: replacement_input.clone(),
                changeset: changeset("after.rs", "old", "after"),
                replacement_extensions: None,
                replacement_vcs_catalog: None,
                host_options: DynamicReviewHostOptions {
                    command_cwd: "/new/cwd".into(),
                    repo_root: Some("/new/repo".into()),
                    startup_notices: vec![StartupNotice::new("reload", "Reloaded config")],
                    keybindings: vec![binding.clone()],
                    keybinding_notices: vec!["binding notice".into()],
                    view_preferences_config_path: Some("/new/config.toml".into()),
                    prompt_save_view_preferences: false,
                    transient_view_preferences: Some(true),
                    pending_extension_trust_repo_root: Some(None),
                    ..DynamicReviewHostOptions::default()
                },
            },
            &ReloadSessionOptions {
                reset_app: Some(false),
                ..ReloadSessionOptions::default()
            },
        )
        .unwrap();

        assert_eq!(app.options.review_input.as_ref(), Some(&replacement_input));
        assert_eq!(
            app.options.command_cwd.as_deref(),
            Some(std::path::Path::new("/new/cwd"))
        );
        assert_eq!(
            app.options.repo.as_deref(),
            Some(std::path::Path::new("/new/repo"))
        );
        assert!(app.options.pager);
        assert!(app.options.watch);
        assert!(!app.options.prompt_save_view_preferences);
        assert!(app.options.transient_view_preferences);
        assert!(app.options.pending_extension_trust_repo_root.is_none());
        assert_eq!(app.options.keybindings, [binding]);
        assert_eq!(app.options.keybinding_notices, ["binding notice"]);
        assert_eq!(app.startup_notices.text(), None, "pager mode hides notices");
        assert!(!app.extension_trust_controller.prompt_open());
    }

    #[test]
    fn comment_batches_preflight_every_target_and_commit_atomically() {
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions::default(),
        );
        let error = app
            .session_add_live_comment_batch(
                &[
                    comment_target("before.rs", "valid"),
                    comment_target("missing.rs", "invalid"),
                ],
                "batch",
                true,
            )
            .unwrap_err();
        assert!(error.contains("missing.rs"), "{error}");
        assert!(app.with_state(|state| state.comments().is_empty()));

        let result = app
            .session_add_live_comment_batch(
                &[
                    comment_target("before.rs", "first"),
                    comment_target("before.rs", "second"),
                ],
                "batch",
                true,
            )
            .unwrap();
        assert_eq!(result.applied.len(), 2);
        assert_eq!(result.applied[0].comment_id, "mcp:batch:0");
        assert_eq!(result.applied[1].comment_id, "mcp:batch:1");
        assert_eq!(app.with_state(|state| state.comments().len()), 2);
    }

    #[test]
    fn live_comment_markup_requires_launch_experimental_authority() {
        let mut target = comment_target("before.rs", "markup");
        target.markup = Some("<strong>attention</strong>".into());
        let command = CommentToolInput {
            target_session: SessionSelector::default(),
            target,
            reveal: None,
        };
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions {
                review_input: Some(patch_input("review.patch")),
                ..ReviewOptions::default()
            },
        );
        assert!(
            app.session_add_live_comment(&command, "mcp:comment", false)
                .unwrap_err()
                .contains("--experimental")
        );
        app.options
            .review_input
            .as_mut()
            .unwrap()
            .options_mut()
            .experimental = Some(true);
        let result = app
            .session_add_live_comment(&command, "mcp:comment", false)
            .unwrap();
        assert_eq!(
            result.markup_width,
            Some(u64::try_from(workdeck_markup::STML_REFERENCE_WIDTH).unwrap())
        );
    }

    #[test]
    fn agent_highlights_share_validation_limits_and_clear_counts() {
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions::default(),
        );
        let input = HighlightToolInput {
            target_session: SessionSelector::default(),
            file_path: "before.rs".into(),
            side: ReviewSide::New,
            line: 1,
            start: 0,
            end: 3,
            tone: Some(SessionLineHighlightTone::Current),
            reveal: Some(true),
        };
        let result = app.session_add_agent_line_highlight(&input).unwrap();
        assert_eq!(result.file_mark_count, 1);
        assert_eq!(result.revealed, Some(RevealedTarget::Line));
        let invalid = HighlightToolInput {
            start: 3,
            end: 3,
            ..input.clone()
        };
        assert!(
            app.session_add_agent_line_highlight(&invalid)
                .unwrap_err()
                .contains("not a valid")
        );
        let cleared = app
            .session_clear_agent_line_highlights(Some("before.rs"))
            .unwrap();
        assert_eq!(cleared.removed_count, 1);
        assert_eq!(cleared.remaining_count, 0);
    }

    #[test]
    fn cursor_off_agent_highlight_reports_hunk_fallback() {
        let mut app = ReviewApp::new(
            changeset("before.rs", "old", "before"),
            ReviewOptions {
                cursor_line: crate::CursorLineMode::Off,
                ..Default::default()
            },
        );
        let result = app
            .session_add_agent_line_highlight(&HighlightToolInput {
                target_session: SessionSelector::default(),
                file_path: "before.rs".into(),
                side: ReviewSide::New,
                line: 1,
                start: 0,
                end: 3,
                tone: None,
                reveal: Some(true),
            })
            .unwrap();
        assert_eq!(result.revealed, Some(RevealedTarget::Hunk));
        assert_eq!(result.file_mark_count, 1);
        let navigation = app
            .session_navigate_to_location(&NavigateToHunkToolInput {
                target_session: SessionSelector::default(),
                file_path: Some("before.rs".into()),
                hunk_index: None,
                side: Some(ReviewSide::New),
                line: Some(1),
                comment_direction: None,
            })
            .unwrap();
        assert_eq!(navigation.revealed, Some(RevealedTarget::Hunk));
        assert_eq!(navigation.side, Some(ReviewSide::New));
        assert_eq!(navigation.line, Some(1));
        app.review_height.set(1);
        app.scroll_to_selection();
        let expected_scroll = app.scroll;
        app.scroll = app.current_review_geometry_rows().lines.len() - 1;
        assert_ne!(app.scroll, expected_scroll);
        let selection = app.with_state(|state| state.selection());
        let repeated = app
            .session_add_agent_line_highlight(&HighlightToolInput {
                target_session: SessionSelector::default(),
                file_path: "before.rs".into(),
                side: ReviewSide::New,
                line: 1,
                start: 0,
                end: 3,
                tone: None,
                reveal: Some(true),
            })
            .unwrap();
        assert_eq!(repeated.revealed, Some(RevealedTarget::Hunk));
        assert_eq!(app.with_state(|state| state.selection()), selection);
        assert_eq!(app.scroll, expected_scroll);
    }
}

#[cfg(test)]
#[path = "session_checkout_tests.rs"]
mod checkout_tests;
