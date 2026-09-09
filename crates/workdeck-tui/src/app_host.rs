//! Native ownership boundary for the mounted review application.
//!
//! Broker transport threads enqueue typed requests here. The Ratatui owner
//! drains them serially, applies mutations, publishes the resulting snapshot,
//! and only then answers the requester. This is the framework-free equivalent
//! of Hunk's AppHost callback, reload-tail, and session-hook lifetimes.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::Value;
use workdeck_core::{
    Changeset, CliInput, NamedCustomThemeConfig, StartupNotice, UserKeyBindingEntry,
};
use workdeck_extension_host::LoadedExtension;
use workdeck_review::ReviewProducer;
use workdeck_session::{
    AppliedCommentBatchResult, AppliedCommentResult, AppliedHighlightResult, ClearedCommentsResult,
    ClearedHighlightsResult, CommentTargetInput, CommentToolInput, HighlightToolInput,
    NavigateToHunkToolInput, NavigatedSelectionResult, ReloadSessionOptions, ReloadedSessionResult,
    RemovedCommentResult, SessionBridgeSnapshotFacts, SessionBrokerConnectionBridge,
    SessionReloadBounds, SessionServerMessage, WorkdeckSessionAppBridge,
    WorkdeckSessionBridgeBinding, WorkdeckSessionBridgeHandlers, WorkdeckSessionBrokerClient,
    WorkdeckSessionCommandInput, WorkdeckSessionCommandResult, WorkdeckSessionState,
    core_cli_input_to_daemon, create_session_reload_bounds, daemon_cli_input_to_core,
    dispatch_workdeck_session_command, project_session_bridge_snapshot,
    validate_session_reload_within_bounds,
};
use workdeck_vcs::VcsCatalog;

use crate::{ExtensionTrustHandler, ReviewApp, agent_note_markup_width};

pub const APP_HOST_COMMAND_TIMEOUT: Duration = Duration::from_secs(30);
const APP_HOST_COMMAND_BURST: usize = 64;
const APP_HOST_RETIRED_MESSAGE: &str = "The Workdeck review is shutting down.";

/// Fully resolved content returned by the composition root after it reapplies
/// runtime defaults and configuration for the validated working directory.
pub struct DynamicReviewLoad {
    pub input: CliInput,
    pub changeset: Changeset,
    pub replacement_extensions: Option<Vec<LoadedExtension>>,
    pub replacement_vcs_catalog: Option<VcsCatalog>,
    pub host_options: DynamicReviewHostOptions,
}

/// Non-content bootstrap values that change when AppHost re-resolves config or
/// extension discovery for a new mounted review.
#[derive(Clone, Default)]
pub struct DynamicReviewHostOptions {
    /// Replacement runtime source authority, installed only after publication commits.
    pub source_capabilities: Option<workdeck_vcs::VcsSourceCapabilities>,
    pub command_cwd: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub startup_notices: Vec<StartupNotice>,
    pub custom_themes: Vec<NamedCustomThemeConfig>,
    pub keybindings: Vec<UserKeyBindingEntry>,
    pub keybinding_notices: Vec<String>,
    pub view_preferences_config_path: Option<PathBuf>,
    pub prompt_save_view_preferences: bool,
    pub transient_view_preferences: Option<bool>,
    /// `None` preserves the existing discovery result; `Some(None)` clears it.
    pub pending_extension_trust_repo_root: Option<Option<PathBuf>>,
    pub extension_trust_handler: Option<ExtensionTrustHandler>,
}

/// Validated content authority for one daemon-driven reload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppHostReloadPlan {
    pub input: CliInput,
    pub cwd: PathBuf,
    pub options: ReloadSessionOptions,
}

/// Immutable launch authority plus the latest successfully mounted input.
#[derive(Debug, Clone)]
pub struct AppHostReloadCoordinator {
    bounds: SessionReloadBounds,
    current_input: CliInput,
    current_cwd: PathBuf,
    launch_experimental: bool,
    launch_fast: bool,
    launch_extensions: Option<bool>,
    launch_extension_paths: Vec<String>,
}

impl AppHostReloadCoordinator {
    pub fn new(
        input: CliInput,
        cwd: impl AsRef<Path>,
        repo_root: Option<&Path>,
    ) -> Result<Self, String> {
        let bounds = create_session_reload_bounds(
            &core_cli_input_to_daemon(input.clone()),
            repo_root,
            cwd.as_ref(),
        )
        .map_err(|error| error.to_string())?;
        let options = input.options();
        let current_cwd = bounds.default_cwd.clone();
        Ok(Self {
            bounds,
            current_input: input.clone(),
            current_cwd,
            launch_experimental: options.experimental.unwrap_or(false),
            launch_fast: options.fast.unwrap_or(false),
            launch_extensions: options.extensions,
            launch_extension_paths: options.extension_paths.clone(),
        })
    }

    /// Parse the strict wire shape, restore launch-only authority, then check
    /// every path and VCS revision before the loader can perform I/O.
    pub fn plan(
        &self,
        next_input: &Value,
        options: ReloadSessionOptions,
    ) -> Result<AppHostReloadPlan, String> {
        let mut daemon =
            serde_json::from_value::<workdeck_session::DaemonCliInput>(next_input.clone())
                .map_err(|error| format!("Invalid session reload input: {error}"))?;
        let mut input =
            daemon_cli_input_to_core(daemon.clone()).map_err(|error| error.to_string())?;
        {
            let next = input.options_mut();
            next.experimental = Some(self.launch_experimental);
            next.fast = Some(self.launch_fast);
            next.extensions = self.launch_extensions;
            next.extension_paths
                .clone_from(&self.launch_extension_paths);
        }
        daemon = core_cli_input_to_daemon(input.clone());
        let validated = validate_session_reload_within_bounds(
            &self.bounds,
            &daemon,
            options.source_path.as_deref(),
        )
        .map_err(|error| error.to_string())?;
        Ok(AppHostReloadPlan {
            input,
            cwd: validated.cwd,
            options,
        })
    }

    pub fn commit(&mut self, plan: &AppHostReloadPlan) {
        self.current_input.clone_from(&plan.input);
        self.current_cwd.clone_from(&plan.cwd);
    }

    #[must_use]
    pub fn requires_extension_reload(&self, plan: &AppHostReloadPlan) -> bool {
        plan.options.reload_extensions.unwrap_or(false) || plan.cwd != self.current_cwd
    }

    #[must_use]
    pub fn current_input(&self) -> &CliInput {
        &self.current_input
    }

    #[must_use]
    pub fn current_cwd(&self) -> &Path {
        &self.current_cwd
    }
}

struct PendingAppHostCommand {
    message: SessionServerMessage<String, WorkdeckSessionCommandInput>,
    reply: mpsc::SyncSender<Result<WorkdeckSessionCommandResult, String>>,
}

struct PendingNativeQuit {
    request_id: String,
    deadline: Instant,
    result_queued: bool,
}

/// Thread-safe broker endpoint; it owns no mutable UI state.
struct AppHostCommandBridge {
    pending_quit: Arc<Mutex<Option<PendingNativeQuit>>>,
    sender: mpsc::Sender<PendingAppHostCommand>,
    active: Arc<AtomicBool>,
    timeout: Duration,
}

impl SessionBrokerConnectionBridge<WorkdeckSessionCommandInput, WorkdeckSessionCommandResult>
    for AppHostCommandBridge
{
    fn command_result_queued(&self, request_id: &str) {
        let mut pending = self
            .pending_quit
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(quit) = pending
            .as_mut()
            .filter(|quit| quit.request_id == request_id)
        {
            quit.result_queued = true;
        }
    }
    fn dispatch_command(
        &self,
        message: SessionServerMessage<String, WorkdeckSessionCommandInput>,
    ) -> Result<WorkdeckSessionCommandResult, String> {
        if !self.active.load(Ordering::Acquire) {
            return Err(APP_HOST_RETIRED_MESSAGE.into());
        }
        let (reply, response) = mpsc::sync_channel(1);
        self.sender
            .send(PendingAppHostCommand { message, reply })
            .map_err(|_| APP_HOST_RETIRED_MESSAGE.to_owned())?;
        response
            .recv_timeout(self.timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    "Timed out waiting for the Workdeck review event loop.".to_owned()
                }
                mpsc::RecvTimeoutError::Disconnected => APP_HOST_RETIRED_MESSAGE.to_owned(),
            })?
    }
}

struct AppHostCommandReceiver {
    pending_quit: Arc<Mutex<Option<PendingNativeQuit>>>,
    timeout: Duration,
    receiver: mpsc::Receiver<PendingAppHostCommand>,
    active: Arc<AtomicBool>,
}

impl AppHostCommandReceiver {
    fn retire(&self) {
        if !self.active.swap(false, Ordering::AcqRel) {
            return;
        }
        self.pending_quit
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
        while let Ok(pending) = self.receiver.try_recv() {
            let _ = pending.reply.send(Err(APP_HOST_RETIRED_MESSAGE.into()));
        }
    }
}

impl Drop for AppHostCommandReceiver {
    fn drop(&mut self) {
        self.retire();
    }
}

fn app_host_command_channel(
    timeout: Duration,
) -> (Arc<WorkdeckSessionAppBridge>, AppHostCommandReceiver) {
    let (sender, receiver) = mpsc::channel();
    let active = Arc::new(AtomicBool::new(true));
    let pending_quit = Arc::new(Mutex::new(None));
    let bridge: Arc<WorkdeckSessionAppBridge> = Arc::new(AppHostCommandBridge {
        pending_quit: Arc::clone(&pending_quit),
        sender,
        active: Arc::clone(&active),
        timeout,
    });
    (
        bridge,
        AppHostCommandReceiver {
            receiver,
            active,
            pending_quit,
            timeout,
        },
    )
}

struct MountedReviewHandlers<'a, R> {
    app: RefCell<&'a mut ReviewApp>,
    reload: RefCell<&'a mut R>,
}

impl<R> WorkdeckSessionBridgeHandlers for MountedReviewHandlers<'_, R>
where
    R: FnMut(&mut ReviewApp, &Value, ReloadSessionOptions) -> Result<ReloadedSessionResult, String>,
{
    fn add_live_comment(
        &self,
        input: &CommentToolInput,
        comment_id: &str,
        reveal: bool,
    ) -> Result<AppliedCommentResult, String> {
        self.app
            .borrow_mut()
            .session_add_live_comment(input, comment_id, reveal)
    }

    fn add_live_comment_batch(
        &self,
        comments: &[CommentTargetInput],
        request_id: &str,
        reveal_first: bool,
    ) -> Result<AppliedCommentBatchResult, String> {
        self.app
            .borrow_mut()
            .session_add_live_comment_batch(comments, request_id, reveal_first)
    }

    fn clear_live_comments(
        &self,
        file_path: Option<&str>,
        include_user: Option<bool>,
    ) -> Result<ClearedCommentsResult, String> {
        self.app
            .borrow_mut()
            .session_clear_live_comments(file_path, include_user)
    }

    fn navigate_to_location(
        &self,
        input: &NavigateToHunkToolInput,
    ) -> Result<NavigatedSelectionResult, String> {
        self.app.borrow_mut().session_navigate_to_location(input)
    }

    fn add_agent_line_highlight(
        &self,
        input: &HighlightToolInput,
    ) -> Result<AppliedHighlightResult, String> {
        self.app
            .borrow_mut()
            .session_add_agent_line_highlight(input)
    }

    fn clear_agent_line_highlights(
        &self,
        file_path: Option<&str>,
    ) -> Result<ClearedHighlightsResult, String> {
        self.app
            .borrow_mut()
            .session_clear_agent_line_highlights(file_path)
    }

    fn open_agent_notes(&self) {
        self.app.borrow_mut().session_open_agent_notes();
    }

    fn reload_session(
        &self,
        next_input: &Value,
        options: ReloadSessionOptions,
    ) -> Result<ReloadedSessionResult, String> {
        let mut app = self.app.borrow_mut();
        (self.reload.borrow_mut())(&mut app, next_input, options)
    }

    fn remove_live_comment(&self, comment_id: &str) -> Result<RemovedCommentResult, String> {
        self.app
            .borrow_mut()
            .session_remove_live_comment(comment_id)
    }

    fn review_producer(&self) -> Option<ReviewProducer> {
        Some(self.app.borrow().review_producer())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishedSnapshotIdentity {
    state: WorkdeckSessionState,
}

/// One bridge attachment and snapshot publication lifecycle for a mounted app.
pub struct AppHostController {
    receiver: AppHostCommandReceiver,
    binding: Option<WorkdeckSessionBridgeBinding>,
    last_snapshot: Option<PublishedSnapshotIdentity>,
    retired: bool,
}

impl AppHostController {
    #[must_use]
    pub fn attach(client: Option<WorkdeckSessionBrokerClient>) -> Self {
        Self::attach_with_timeout(client, APP_HOST_COMMAND_TIMEOUT)
    }

    #[must_use]
    fn attach_with_timeout(client: Option<WorkdeckSessionBrokerClient>, timeout: Duration) -> Self {
        let host = client
            .map(|client| Arc::new(client) as Arc<dyn workdeck_session::WorkdeckSessionBridgeHost>);
        Self::attach_to_host(host, timeout)
    }

    fn attach_to_host(
        host: Option<Arc<dyn workdeck_session::WorkdeckSessionBridgeHost>>,
        timeout: Duration,
    ) -> Self {
        let (bridge, receiver) = app_host_command_channel(timeout);
        Self {
            receiver,
            binding: Some(WorkdeckSessionBridgeBinding::attach(host, bridge)),
            last_snapshot: None,
            retired: false,
        }
    }

    /// Apply a bounded FIFO burst. Requests beyond the burst remain ordered for
    /// the next event-loop turn so remote traffic cannot starve terminal input.
    pub fn process_pending<R>(&mut self, app: &mut ReviewApp, reload: &mut R) -> usize
    where
        R: FnMut(
            &mut ReviewApp,
            &Value,
            ReloadSessionOptions,
        ) -> Result<ReloadedSessionResult, String>,
    {
        if self.retired {
            return 0;
        }
        let mut processed = 0;
        while processed < APP_HOST_COMMAND_BURST {
            let mut pending_quit = self
                .receiver
                .pending_quit
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if let Some(quit) = pending_quit.as_ref() {
                if quit.result_queued {
                    app.should_quit = true;
                    pending_quit.take();
                } else if Instant::now() >= quit.deadline {
                    pending_quit.take();
                    app.status =
                        Some("Native quit reply was not queued; the review remains open.".into());
                } else {
                    // Do not start later mutations while shutdown awaits its reply.
                    break;
                }
            }
            drop(pending_quit);
            // Quit linearizes between complete owner-thread commands. Once it
            // wins, every command still queued receives the stable retirement
            // error instead of starting mutation, loading, or filesystem I/O.
            if app.shutdown_requested() {
                self.retire();
                break;
            }
            let pending = match self.receiver.receiver.try_recv() {
                Ok(pending) => pending,
                Err(mpsc::TryRecvError::Empty | mpsc::TryRecvError::Disconnected) => break,
            };
            if matches!(
                &pending.message.input,
                WorkdeckSessionCommandInput::QuitSession(_)
            ) {
                *self
                    .receiver
                    .pending_quit
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(PendingNativeQuit {
                    request_id: pending.message.request_id,
                    deadline: Instant::now() + self.receiver.timeout,
                    result_queued: false,
                });
                if pending
                    .reply
                    .send(Ok(WorkdeckSessionCommandResult::QuitSession(
                        workdeck_session::QuitSessionResult { quitting: true },
                    )))
                    .is_err()
                {
                    self.receiver
                        .pending_quit
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take();
                }
                processed += 1;
                continue;
            }
            let handlers = MountedReviewHandlers {
                app: RefCell::new(app),
                reload: RefCell::new(reload),
            };
            let result = dispatch_workdeck_session_command(&handlers, pending.message);
            let _ = pending.reply.send(result);
            processed += 1;
        }
        processed
    }

    /// Publish only when a source-hook dependency changed, retaining the exact
    /// selected ranges, note lists, and review publication address.
    pub fn publish_snapshot(&mut self, app: &ReviewApp) -> Result<bool, String> {
        let Some(binding) = self.binding.as_ref() else {
            return Ok(false);
        };
        if !binding.is_attached() {
            return Ok(false);
        }
        let live_comments = app.session_live_comment_summaries();
        let review_notes = app.session_review_note_summaries();
        let publication_generation = app.review_producer().get_publication().generation.clone();
        let previous = self.last_snapshot.clone();
        let publication: Result<(PublishedSnapshotIdentity, bool), String> =
            app.with_state(|state| {
                let selection = state.selection();
                let selected_file = state.changeset().files.get(selection.file_index);
                let selected_hunk = selected_file
                    .and_then(|file| selection.hunk_index.and_then(|index| file.hunks.get(index)));
                let note_markup_width = app.review_geometry_published.get().then(|| {
                    u64::try_from(agent_note_markup_width(
                        selection.side,
                        state.resolved_layout(app.review_width.get()),
                        usize::from(app.review_width.get()),
                        0,
                    ))
                    .unwrap_or(u64::MAX)
                });
                let facts = SessionBridgeSnapshotFacts {
                    selected_file,
                    selected_hunk,
                    selected_hunk_index: selection.hunk_index.unwrap_or(0),
                    show_agent_notes: app.options.agent_notes,
                    note_markup_width,
                    live_comment_count: u64::try_from(live_comments.len()).unwrap_or(u64::MAX),
                    live_comments: live_comments.clone(),
                    review_note_count: u64::try_from(review_notes.len()).unwrap_or(u64::MAX),
                    review_notes: review_notes.clone(),
                    publication_generation: Some(&publication_generation),
                    review_state_revision: state.state_revision(),
                };
                let identity = PublishedSnapshotIdentity {
                    state: project_session_bridge_snapshot(
                        SessionBridgeSnapshotFacts {
                            selected_file: facts.selected_file,
                            selected_hunk: facts.selected_hunk,
                            selected_hunk_index: facts.selected_hunk_index,
                            show_agent_notes: facts.show_agent_notes,
                            note_markup_width: facts.note_markup_width,
                            live_comment_count: facts.live_comment_count,
                            live_comments: facts.live_comments.clone(),
                            review_note_count: facts.review_note_count,
                            review_notes: facts.review_notes.clone(),
                            publication_generation: facts.publication_generation,
                            review_state_revision: facts.review_state_revision,
                        },
                        "identity",
                    )
                    .state,
                };
                if previous.as_ref() == Some(&identity) {
                    return Ok((identity, false));
                }
                binding
                    .update_snapshot(facts)
                    .map_err(|error| error.to_string())?;
                Ok((identity, true))
            });
        let (identity, published) = publication?;
        self.last_snapshot = Some(identity);
        Ok(published)
    }

    /// Detach the bridge before extension and terminal teardown. Pending
    /// requesters are refused exactly once rather than timing out.
    pub fn retire(&mut self) {
        if self.retired {
            return;
        }
        self.retired = true;
        self.receiver.retire();
        self.binding.take();
    }
}

impl Drop for AppHostController {
    fn drop(&mut self) {
        self.retire();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Barrier, Mutex};
    use std::thread;
    use std::time::Instant;

    use workdeck_core::{
        ChangesetSource, CliInput, CommonOptions, ReviewSide, VcsDiffCommandInput,
    };
    use workdeck_diff::parse_patch;
    use workdeck_session::{
        ClearHighlightsToolInput, CommentTargetInput, ReloadSessionToolInput,
        SessionBrokerClientError, SessionSelector, WorkdeckSessionBridgeHost,
        WorkdeckSessionInputKind, WorkdeckSessionSnapshot, no_diff_file_matches_message,
    };

    use super::*;
    use crate::ReviewOptions;

    #[derive(Default)]
    struct MockHostState {
        bridge: Option<Arc<WorkdeckSessionAppBridge>>,
        attachments: Vec<bool>,
        snapshots: Vec<WorkdeckSessionSnapshot>,
    }

    #[derive(Default)]
    struct MockHost(Mutex<MockHostState>);

    impl WorkdeckSessionBridgeHost for MockHost {
        fn set_bridge(&self, bridge: Option<Arc<WorkdeckSessionAppBridge>>) {
            let mut state = self.0.lock().unwrap();
            state.attachments.push(bridge.is_some());
            state.bridge = bridge;
        }

        fn update_snapshot(
            &self,
            snapshot: WorkdeckSessionSnapshot,
        ) -> Result<(), SessionBrokerClientError> {
            self.0.lock().unwrap().snapshots.push(snapshot);
            Ok(())
        }
    }

    fn app() -> ReviewApp {
        ReviewApp::new(
            parse_patch(
                "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+new\n",
                "test",
                "Working tree",
                ChangesetSource::WorkingTree { staged: false },
            )
            .unwrap(),
            ReviewOptions::default(),
        )
    }

    fn vcs_input(options: CommonOptions) -> CliInput {
        CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options,
        })
    }

    fn message(
        request_id: &str,
        input: WorkdeckSessionCommandInput,
    ) -> SessionServerMessage<String, WorkdeckSessionCommandInput> {
        SessionServerMessage {
            request_id: request_id.into(),
            command: "test".into(),
            command_version: None,
            input,
        }
    }

    fn reload_message(sequence: u64) -> SessionServerMessage<String, WorkdeckSessionCommandInput> {
        message(
            &format!("reload-{sequence}"),
            WorkdeckSessionCommandInput::ReloadSession(ReloadSessionToolInput {
                target_session: SessionSelector::default(),
                next_input: serde_json::json!({"sequence": sequence}),
                source_path: None,
            }),
        )
    }

    fn queued_controller(
        messages: impl IntoIterator<Item = SessionServerMessage<String, WorkdeckSessionCommandInput>>,
    ) -> (
        AppHostController,
        Vec<mpsc::Receiver<Result<WorkdeckSessionCommandResult, String>>>,
    ) {
        let (sender, receiver) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let mut replies = Vec::new();
        for message in messages {
            let (reply, response) = mpsc::sync_channel(1);
            sender
                .send(PendingAppHostCommand { message, reply })
                .unwrap();
            replies.push(response);
        }
        drop(sender);
        (
            AppHostController {
                receiver: AppHostCommandReceiver {
                    receiver,
                    active,
                    pending_quit: Arc::new(Mutex::new(None)),
                    timeout: APP_HOST_COMMAND_TIMEOUT,
                },
                binding: None,
                last_snapshot: None,
                retired: false,
            },
            replies,
        )
    }

    fn reloaded_result(sequence: u64) -> ReloadedSessionResult {
        ReloadedSessionResult {
            session_id: format!("session-{sequence}"),
            input_kind: WorkdeckSessionInputKind::Vcs,
            title: format!("reload {sequence}"),
            source_label: "/repo".into(),
            file_count: 1,
            selected_file_path: Some("a.rs".into()),
            selected_hunk_index: 0,
        }
    }

    fn process_until_pending(controller: &mut AppHostController, app: &mut ReviewApp) -> usize {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let processed =
                controller.process_pending(app, &mut |_, _, _| Err("unexpected reload".into()));
            if processed > 0 || Instant::now() >= deadline {
                return processed;
            }
            thread::yield_now();
        }
    }

    #[test]
    fn broker_mutations_run_on_the_owner_thread_and_answer_after_commit() {
        let host = Arc::new(MockHost::default());
        let mut controller = AppHostController::attach_to_host(
            Some(host.clone() as Arc<dyn WorkdeckSessionBridgeHost>),
            Duration::from_secs(2),
        );
        let bridge = host.0.lock().unwrap().bridge.clone().unwrap();
        let barrier = Arc::new(Barrier::new(2));
        let worker_barrier = Arc::clone(&barrier);
        let worker = thread::spawn(move || {
            worker_barrier.wait();
            bridge.dispatch_command(message(
                "comment-1",
                WorkdeckSessionCommandInput::Comment(CommentToolInput {
                    target_session: SessionSelector::default(),
                    target: CommentTargetInput {
                        file_path: "a.rs".into(),
                        hunk_index: None,
                        side: Some(ReviewSide::New),
                        line: Some(1),
                        summary: "Inspect this".into(),
                        rationale: None,
                        markup: None,
                        author: Some("agent".into()),
                    },
                    reveal: Some(true),
                }),
            ))
        });
        barrier.wait();
        let mut app = app();
        assert_eq!(process_until_pending(&mut controller, &mut app), 1);
        let WorkdeckSessionCommandResult::AppliedComment(result) = worker.join().unwrap().unwrap()
        else {
            panic!("wrong broker result")
        };
        assert_eq!(result.comment_id, "mcp:comment-1");
        assert_eq!(app.with_state(|state| state.comments().len()), 1);
        assert!(app.options.agent_notes);
    }

    #[test]
    fn handler_failures_cross_the_queue_without_mutating_the_review() {
        let host = Arc::new(MockHost::default());
        let mut controller = AppHostController::attach_to_host(
            Some(host.clone() as Arc<dyn WorkdeckSessionBridgeHost>),
            Duration::from_secs(2),
        );
        let bridge = host.0.lock().unwrap().bridge.clone().unwrap();
        let worker = thread::spawn(move || {
            bridge.dispatch_command(message(
                "clear",
                WorkdeckSessionCommandInput::ClearHighlights(ClearHighlightsToolInput {
                    target_session: SessionSelector::default(),
                    file_path: Some("missing.rs".into()),
                }),
            ))
        });
        let mut app = app();
        assert_eq!(process_until_pending(&mut controller, &mut app), 1);
        assert_eq!(
            worker.join().unwrap().unwrap_err(),
            no_diff_file_matches_message("missing.rs")
        );
        assert!(app.agent_line_highlights.is_empty());
    }

    #[test]
    fn queued_reloads_run_to_completion_in_fifo_order() {
        let (mut controller, replies) = queued_controller([reload_message(1), reload_message(2)]);
        let mut app = app();
        let mut observed = Vec::new();
        let processed = controller.process_pending(&mut app, &mut |_, input, _| {
            let sequence = input["sequence"]
                .as_u64()
                .expect("test reload has a sequence");
            observed.push(sequence);
            Ok(reloaded_result(sequence))
        });

        assert_eq!(processed, 2);
        assert_eq!(observed, [1, 2]);
        for (sequence, reply) in (1..=2).zip(replies) {
            let WorkdeckSessionCommandResult::ReloadedSession(result) =
                reply.recv().unwrap().unwrap()
            else {
                panic!("queued reload returned the wrong result")
            };
            assert_eq!(result.session_id, format!("session-{sequence}"));
        }
    }

    #[test]
    fn current_review_refresh_uses_live_view_options_and_the_full_commit_gate() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        let input = vcs_input(CommonOptions::default());
        let initial = parse_patch(
            "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+initial\n",
            repo.path().to_string_lossy(),
            "test",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        let mut app = ReviewApp::new(
            initial,
            ReviewOptions {
                review_input: Some(input.clone()),
                repo: Some(repo.path().to_owned()),
                command_cwd: Some(repo.path().to_owned()),
                ..ReviewOptions::default()
            },
        );
        app.with_state(|state| state.set_layout(workdeck_review::LayoutMode::Stack));
        app.options.wrap_lines = true;
        app.options.line_numbers = false;
        app.filter = "keep-me".into();

        let mut coordinator =
            AppHostReloadCoordinator::new(input, repo.path(), Some(repo.path())).unwrap();
        let mut catalog = workdeck_vcs::bundled_vcs_catalog().clone();
        let mut observed = Vec::new();
        let mut loader = |input: &CliInput,
                          cwd: &Path,
                          reload_extensions: bool,
                          _: &VcsCatalog,
                          current_extensions: &[LoadedExtension]| {
            observed.push((
                input.clone(),
                cwd.to_owned(),
                reload_extensions,
                current_extensions.len(),
            ));
            Ok(DynamicReviewLoad {
                input: input.clone(),
                changeset: parse_patch(
                    "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+reloaded\n",
                    cwd.to_string_lossy(),
                    "test",
                    ChangesetSource::WorkingTree { staged: false },
                )
                .unwrap(),
                replacement_extensions: None,
                replacement_vcs_catalog: None,
                host_options: DynamicReviewHostOptions {
                    command_cwd: cwd.to_owned(),
                    repo_root: Some(cwd.to_owned()),
                    prompt_save_view_preferences: true,
                    ..DynamicReviewHostOptions::default()
                },
            })
        };

        let result = crate::commit_current_dynamic_review_reload(
            &mut app,
            &mut coordinator,
            &mut loader,
            &mut catalog,
            workdeck_extension_api::SessionReloadReason::Manual,
            false,
        )
        .unwrap();

        assert_eq!(result.session_id, "local-session");
        assert_eq!(observed.len(), 1);
        assert_eq!(observed[0].1, repo.path().canonicalize().unwrap());
        assert!(!observed[0].2);
        assert_eq!(observed[0].3, 0);
        assert_eq!(
            observed[0].0.options().mode,
            Some(workdeck_core::InputLayoutMode::Stack)
        );
        assert_eq!(observed[0].0.options().wrap_lines, Some(true));
        assert_eq!(observed[0].0.options().line_numbers, Some(false));
        assert_eq!(app.filter, "keep-me");
        assert_eq!(
            app.with_state(|state| state.changeset().files[0].hunks[0].lines[1].content.clone()),
            "reloaded"
        );
        assert_eq!(coordinator.current_input(), &observed[0].0);
    }

    #[test]
    fn terminal_quit_refuses_the_next_already_queued_reload() {
        let (mut controller, replies) = queued_controller([reload_message(1), reload_message(2)]);
        let mut app = app();
        let mut observed = Vec::new();
        let processed = controller.process_pending(&mut app, &mut |app, input, _| {
            let sequence = input["sequence"]
                .as_u64()
                .expect("test reload has a sequence");
            observed.push(sequence);
            app.should_quit = true;
            Ok(reloaded_result(sequence))
        });

        assert_eq!(processed, 1);
        assert_eq!(observed, [1]);
        let WorkdeckSessionCommandResult::ReloadedSession(first) =
            replies[0].recv().unwrap().unwrap()
        else {
            panic!("first queued reload returned the wrong result")
        };
        assert_eq!(first.session_id, "session-1");
        assert_eq!(
            replies[1].recv().unwrap().unwrap_err(),
            APP_HOST_RETIRED_MESSAGE
        );
        assert!(controller.retired);
    }

    fn quit_message() -> SessionServerMessage<String, WorkdeckSessionCommandInput> {
        message(
            "native-quit",
            WorkdeckSessionCommandInput::QuitSession(workdeck_session::QuitSessionToolInput {
                target_session: SessionSelector::default(),
            }),
        )
    }

    #[test]
    fn native_quit_waits_for_matching_queued_result_and_retires_later_commands() {
        let (mut controller, replies) = queued_controller([quit_message(), reload_message(1)]);
        let (sender, _receiver) = mpsc::channel();
        let bridge = AppHostCommandBridge {
            sender,
            active: Arc::clone(&controller.receiver.active),
            pending_quit: Arc::clone(&controller.receiver.pending_quit),
            timeout: APP_HOST_COMMAND_TIMEOUT,
        };
        let mut app = app();
        let mut reload = |_: &mut ReviewApp,
                          _: &Value,
                          _: ReloadSessionOptions|
         -> Result<ReloadedSessionResult, String> {
            panic!("commands following accepted quit must not start");
        };
        assert_eq!(controller.process_pending(&mut app, &mut reload), 1);
        assert!(!app.shutdown_requested());
        assert!(matches!(
            replies[0].recv().unwrap().unwrap(),
            WorkdeckSessionCommandResult::QuitSession(workdeck_session::QuitSessionResult {
                quitting: true
            })
        ));
        bridge.command_result_queued("unrelated-request");
        assert_eq!(controller.process_pending(&mut app, &mut reload), 0);
        assert!(!app.shutdown_requested());
        assert!(matches!(
            replies[1].try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        bridge.command_result_queued("native-quit");
        assert_eq!(controller.process_pending(&mut app, &mut reload), 0);
        assert!(app.shutdown_requested());
        assert!(controller.retired);
        assert_eq!(
            replies[1].recv().unwrap().unwrap_err(),
            APP_HOST_RETIRED_MESSAGE
        );
        assert!(controller.receiver.pending_quit.lock().unwrap().is_none());
    }

    #[test]
    fn native_quit_timeout_or_abandoned_request_keeps_review_open() {
        let (mut controller, replies) = queued_controller([quit_message()]);
        let mut app = app();
        let mut reload =
            |_: &mut ReviewApp,
             _: &Value,
             _: ReloadSessionOptions|
             -> Result<ReloadedSessionResult, String> { panic!("unexpected reload") };
        assert_eq!(controller.process_pending(&mut app, &mut reload), 1);
        assert!(replies[0].recv().unwrap().is_ok());
        controller
            .receiver
            .pending_quit
            .lock()
            .unwrap()
            .as_mut()
            .unwrap()
            .deadline = Instant::now();
        assert_eq!(controller.process_pending(&mut app, &mut reload), 0);
        assert!(!app.shutdown_requested());
        assert!(
            app.status
                .as_deref()
                .unwrap()
                .contains("review remains open")
        );
        assert!(controller.receiver.pending_quit.lock().unwrap().is_none());

        let (mut controller, replies) = queued_controller([quit_message()]);
        drop(replies);
        assert_eq!(controller.process_pending(&mut app, &mut reload), 1);
        assert!(!app.shutdown_requested());
        assert!(controller.receiver.pending_quit.lock().unwrap().is_none());
    }

    #[test]
    fn wheel_scroll_publishes_viewport_center_file_and_hunk() {
        use crate::tests::{navigation_changeset, numbered_exports, rendered_review_frame};
        use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
        use ratatui::{Terminal, backend::TestBackend};
        let first = numbered_exports(1, 12, 0, true);
        let second = numbered_exports(13, 50, 0, true);
        let mut second_after = second.clone();
        for line in [13, 42, 43, 44] {
            second_after = second_after.replace(
                &format!("line{line} = {line};"),
                &format!("line{line} = {};", line * 100),
            );
        }
        let mut review = navigation_changeset(vec![
            (
                "first.ts".into(),
                first.clone(),
                first.replace("line01 = 1;", "line01 = 101;"),
            ),
            ("second.ts".into(), second, second_after),
        ]);
        for file in &mut review.files {
            file.agent = Some(serde_json::from_value(serde_json::json!({
                "path":file.path, "summary":format!("{} note", file.path),
                "annotations":[{"new_range":{"start":2,"end":2}, "summary":format!("Annotation for {}", file.path), "rationale":format!("Why {} changed", file.path)}]
            })).unwrap());
        }
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let host = Arc::new(MockHost::default());
        let mut controller = AppHostController::attach_to_host(
            Some(host.clone() as Arc<dyn WorkdeckSessionBridgeHost>),
            Duration::from_secs(2),
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 12)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        controller.publish_snapshot(&app).unwrap();
        let selected = || {
            let guard = host.0.lock().unwrap();
            let state = &guard.snapshots.last().unwrap().state;
            (state.selected_file_path.clone(), state.selected_hunk_index)
        };
        assert_eq!(selected(), (Some("first.ts".into()), 0));
        for _ in 0..16 {
            app.handle_mouse_event(MouseEvent {
                kind: MouseEventKind::ScrollDown,
                column: 120,
                row: 7,
                modifiers: KeyModifiers::NONE,
            });
            rendered_review_frame(&mut terminal, &app);
            controller.publish_snapshot(&app).unwrap();
            if selected() == (Some("second.ts".into()), 1) {
                break;
            }
        }
        assert_eq!(selected(), (Some("second.ts".into()), 1));
    }

    #[test]
    fn file_shortcuts_publish_selection_and_filter_focus_retains_selected_file() {
        use crate::tests::{navigation_changeset, numbered_exports, rendered_review_frame};
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
        use ratatui::{Terminal, backend::TestBackend};
        let mut review = navigation_changeset(vec![
            (
                "first.ts".into(),
                numbered_exports(1, 16, 0, true),
                numbered_exports(1, 16, 100, true),
            ),
            (
                "second.ts".into(),
                numbered_exports(17, 16, 0, true),
                numbered_exports(17, 16, 100, true),
            ),
        ]);
        for (file, id) in review.files.iter_mut().zip(["first", "second"]) {
            file.runtime_id = id.into();
            file.agent = Some(serde_json::from_value(serde_json::json!({
                "path":file.path, "summary":format!("{} note", file.path),
                "annotations":[{"new_range":{"start":2,"end":2}, "summary":format!("Annotation for {}", file.path), "rationale":format!("Why {} changed", file.path)}]
            })).unwrap());
        }
        review.refresh_review_identities();
        let mut app = ReviewApp::new(review, ReviewOptions::default());
        let host = Arc::new(MockHost::default());
        let mut controller = AppHostController::attach_to_host(
            Some(host.clone() as Arc<dyn WorkdeckSessionBridgeHost>),
            Duration::from_secs(2),
        );
        let mut terminal = Terminal::new(TestBackend::new(220, 10)).unwrap();
        rendered_review_frame(&mut terminal, &app);
        controller.publish_snapshot(&app).unwrap();
        for _ in 0..10 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            rendered_review_frame(&mut terminal, &app);
        }
        for (key, expected) in [
            (KeyCode::Char('.'), "second"),
            (KeyCode::Char(','), "first"),
        ] {
            app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
            let frame = rendered_review_frame(&mut terminal, &app);
            controller.publish_snapshot(&app).unwrap();
            let snapshots = host.0.lock().unwrap();
            let state = &snapshots.snapshots.last().unwrap().state;
            assert_eq!(state.selected_file_id.as_deref(), Some(expected));
            assert_eq!(state.selected_hunk_index, 0);
            assert!(frame.contains(&format!("{expected}.ts")), "{frame}");
        }
        app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::NONE));
        let frame = rendered_review_frame(&mut terminal, &app);
        controller.publish_snapshot(&app).unwrap();
        assert!(frame.contains("filter:"), "{frame}");
        assert_eq!(
            host.0
                .lock()
                .unwrap()
                .snapshots
                .last()
                .unwrap()
                .state
                .selected_file_id
                .as_deref(),
            Some("first")
        );
    }

    #[test]
    fn snapshots_are_dependency_driven_and_retirement_detaches_once() {
        let host = Arc::new(MockHost::default());
        let mut controller = AppHostController::attach_to_host(
            Some(host.clone() as Arc<dyn WorkdeckSessionBridgeHost>),
            Duration::from_secs(2),
        );
        let mut app = app();
        assert!(controller.publish_snapshot(&app).unwrap());
        assert!(!controller.publish_snapshot(&app).unwrap());
        app.session_add_live_comment(
            &CommentToolInput {
                target_session: SessionSelector::default(),
                target: CommentTargetInput {
                    file_path: "a.rs".into(),
                    hunk_index: Some(0),
                    side: None,
                    line: None,
                    summary: "note".into(),
                    rationale: None,
                    markup: None,
                    author: None,
                },
                reveal: None,
            },
            "mcp:direct",
            false,
        )
        .unwrap();
        assert!(controller.publish_snapshot(&app).unwrap());
        let state = host.0.lock().unwrap();
        assert_eq!(state.snapshots.len(), 2);
        assert_eq!(state.snapshots[1].state.live_comment_count, 1);
        assert_eq!(state.snapshots[1].state.review_note_count, Some(1));
        drop(state);
        controller.retire();
        controller.retire();
        assert_eq!(host.0.lock().unwrap().attachments, [true, false]);
    }

    #[test]
    fn retirement_answers_already_queued_requests_and_refuses_new_ones() {
        let (sender, queue) = mpsc::channel();
        let active = Arc::new(AtomicBool::new(true));
        let bridge = AppHostCommandBridge {
            pending_quit: Arc::new(Mutex::new(None)),
            sender: sender.clone(),
            active: Arc::clone(&active),
            timeout: Duration::from_secs(2),
        };
        let receiver = AppHostCommandReceiver {
            pending_quit: Arc::clone(&bridge.pending_quit),
            timeout: APP_HOST_COMMAND_TIMEOUT,
            receiver: queue,
            active,
        };
        let (reply, response) = mpsc::sync_channel(1);
        sender
            .send(PendingAppHostCommand {
                message: message(
                    "queued",
                    WorkdeckSessionCommandInput::ClearHighlights(ClearHighlightsToolInput {
                        target_session: SessionSelector::default(),
                        file_path: None,
                    }),
                ),
                reply,
            })
            .unwrap();
        receiver.retire();
        assert_eq!(
            response.recv().unwrap().unwrap_err(),
            APP_HOST_RETIRED_MESSAGE
        );
        assert_eq!(
            bridge
                .dispatch_command(message(
                    "late",
                    WorkdeckSessionCommandInput::ClearHighlights(ClearHighlightsToolInput {
                        target_session: SessionSelector::default(),
                        file_path: None,
                    }),
                ))
                .unwrap_err(),
            APP_HOST_RETIRED_MESSAGE
        );
    }

    #[test]
    fn queued_file_reload_preserves_live_comment_in_updated_terminal_frame() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        let left = repo.path().join("before.ts");
        let right = repo.path().join("after.ts");
        fs::write(&left, "export const answer = 41;\n").unwrap();
        fs::write(&right, "export const answer = 42;\n").unwrap();
        let initial = CliInput::Files(workdeck_core::FileCommandInput {
            left: left.to_string_lossy().into_owned(),
            right: right.to_string_lossy().into_owned(),
            options: CommonOptions {
                mode: Some(workdeck_core::InputLayoutMode::Split),
                ..Default::default()
            },
        });
        let mut app = ReviewApp::new(
            workdeck_vcs::load_file_comparison(repo.path(), &left, &right).unwrap(),
            ReviewOptions {
                review_input: Some(initial.clone()),
                command_cwd: Some(repo.path().to_owned()),
                repo: Some(repo.path().to_owned()),
                layout: workdeck_review::LayoutMode::Split,
                ..Default::default()
            },
        );
        let frame = |app: &ReviewApp| {
            let area = ratatui::layout::Rect::new(0, 0, 220, 20);
            let mut buffer = ratatui::buffer::Buffer::empty(area);
            crate::render(area, &mut buffer, app);
            (0..area.height)
                .map(|y| {
                    (0..area.width)
                        .map(|x| buffer[(x, y)].symbol())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let note = "Keep this daemon review note";
        let comment = serde_json::from_value(serde_json::json!({
            "filePath":"after.ts", "side":"new", "line":1, "summary":note, "reveal":true,
        }))
        .unwrap();
        let (mut controller, replies) = queued_controller([message(
            "comment-1",
            WorkdeckSessionCommandInput::Comment(comment),
        )]);
        assert_eq!(
            controller.process_pending(&mut app, &mut |_, _, _| Err("unexpected reload".into())),
            1
        );
        replies[0].recv().unwrap().unwrap();
        assert!(frame(&app).contains(note));
        let comments = app.with_state(|state| state.comments().to_vec());
        let publication = app.review_producer().get_publication_address();
        fs::write(
            &right,
            "export const answer = 42;\nexport const added = true;\n",
        )
        .unwrap();
        let (mut controller, replies) = queued_controller([message(
            "reload-1",
            WorkdeckSessionCommandInput::ReloadSession(ReloadSessionToolInput {
                target_session: SessionSelector::default(),
                next_input: serde_json::to_value(core_cli_input_to_daemon(initial.clone()))
                    .unwrap(),
                source_path: None,
            }),
        )]);
        let mut coordinator =
            AppHostReloadCoordinator::new(initial, repo.path(), Some(repo.path())).unwrap();
        let mut loader = |input: &CliInput,
                          cwd: &Path,
                          _: bool,
                          _: &VcsCatalog,
                          _: &[LoadedExtension]|
         -> anyhow::Result<DynamicReviewLoad> {
            let CliInput::Files(files) = input else {
                anyhow::bail!("expected file comparison")
            };
            Ok(DynamicReviewLoad {
                input: input.clone(),
                changeset: workdeck_vcs::load_file_comparison(
                    cwd,
                    Path::new(&files.left),
                    Path::new(&files.right),
                )?,
                replacement_extensions: None,
                replacement_vcs_catalog: None,
                host_options: DynamicReviewHostOptions {
                    command_cwd: cwd.to_owned(),
                    repo_root: Some(cwd.to_owned()),
                    ..Default::default()
                },
            })
        };
        let mut catalog = workdeck_vcs::bundled_vcs_catalog().clone();
        assert_eq!(
            controller.process_pending(&mut app, &mut |app, input, options| {
                let plan = coordinator.plan(input, options.clone())?;
                crate::commit_dynamic_review_reload(
                    app,
                    &mut coordinator,
                    plan,
                    &mut loader,
                    &mut catalog,
                )
            }),
            1
        );
        assert!(matches!(
            replies[0].recv().unwrap().unwrap(),
            WorkdeckSessionCommandResult::ReloadedSession(_)
        ));
        let updated = frame(&app);
        assert!(updated.contains("export const added = true;"), "{updated}");
        assert!(updated.contains(note), "{updated}");
        assert_eq!(app.with_state(|state| state.comments().to_vec()), comments);
        assert_ne!(app.review_producer().get_publication_address(), publication);
    }

    #[test]
    fn queued_reload_outside_launch_root_is_rejected_before_loader_or_publication() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        for directory in [repo.path(), outside.path()] {
            fs::write(directory.join("before.ts"), "export const value = 1;\n").unwrap();
            fs::write(directory.join("after.ts"), "export const value = 2;\n").unwrap();
        }
        for (left_outside, right_outside, source_outside) in [
            (true, true, true),
            (true, false, false),
            (false, true, false),
            (false, false, true),
        ] {
            let initial = vcs_input(CommonOptions::default());
            let mut coordinator =
                AppHostReloadCoordinator::new(initial.clone(), repo.path(), Some(repo.path()))
                    .unwrap();
            let path = |external: bool, name: &str| {
                if external {
                    outside.path()
                } else {
                    repo.path()
                }
                .join(name)
            };
            let command = message(
                "outside-root",
                WorkdeckSessionCommandInput::ReloadSession(ReloadSessionToolInput {
                    target_session: SessionSelector::default(),
                    next_input: serde_json::json!({"kind":"diff",
                        "left":path(left_outside, "before.ts"),
                        "right":path(right_outside, "after.ts"),
                        "options":{"mode":"split"}}),
                    source_path: source_outside
                        .then(|| outside.path().to_string_lossy().into_owned()),
                }),
            );
            let (mut controller, replies) = queued_controller([command]);
            let mut app = app();
            app.options.review_input = Some(initial.clone());
            let publication = app.review_producer().get_publication_address();
            let before = app.with_state(|state| {
                (
                    state.generation(),
                    state.selection(),
                    state.state_revision(),
                )
            });
            let mut loader_calls = 0;
            let mut loader = |_: &CliInput,
                              _: &Path,
                              _: bool,
                              _: &VcsCatalog,
                              _: &[LoadedExtension]|
             -> anyhow::Result<DynamicReviewLoad> {
                loader_calls += 1;
                anyhow::bail!("outside input reached the loader")
            };
            let mut catalog = workdeck_vcs::bundled_vcs_catalog().clone();
            let processed = controller.process_pending(&mut app, &mut |app, input, options| {
                let plan = coordinator.plan(input, options.clone())?;
                crate::commit_dynamic_review_reload(
                    app,
                    &mut coordinator,
                    plan,
                    &mut loader,
                    &mut catalog,
                )
            });
            assert_eq!(processed, 1);
            let error = replies[0].recv().unwrap().unwrap_err();
            assert!(
                error.contains("outside the initial Workdeck root"),
                "{error}"
            );
            assert_eq!(loader_calls, 0);
            assert_eq!(app.review_producer().get_publication_address(), publication);
            assert_eq!(
                app.with_state(|state| (
                    state.generation(),
                    state.selection(),
                    state.state_revision()
                )),
                before
            );
            assert_eq!(coordinator.current_input(), &initial);
            assert_eq!(
                coordinator.current_cwd(),
                repo.path().canonicalize().unwrap()
            );
            assert_eq!(app.options.review_input.as_ref(), Some(&initial));
        }
    }

    #[test]
    fn plain_launch_reload_cannot_enable_markup_and_rejected_comments_leave_no_state() {
        for launch_experimental in [None, Some(false)] {
            for reset_app in [false, true] {
                let repo = tempfile::tempdir().unwrap();
                fs::create_dir(repo.path().join(".git")).unwrap();
                let left = repo.path().join("before.ts");
                let right = repo.path().join("after.ts");
                fs::write(&left, "export const answer = 41;\n").unwrap();
                fs::write(&right, "export const answer = 42;\n").unwrap();
                let file_input = |experimental| {
                    CliInput::Files(workdeck_core::FileCommandInput {
                        left: left.to_string_lossy().into_owned(),
                        right: right.to_string_lossy().into_owned(),
                        options: CommonOptions {
                            experimental,
                            mode: Some(workdeck_core::InputLayoutMode::Split),
                            ..Default::default()
                        },
                    })
                };
                let initial = file_input(launch_experimental);
                let mut app = ReviewApp::new(
                    workdeck_vcs::load_file_comparison(repo.path(), &left, &right).unwrap(),
                    ReviewOptions {
                        review_input: Some(initial.clone()),
                        command_cwd: Some(repo.path().to_owned()),
                        repo: Some(repo.path().to_owned()),
                        layout: workdeck_review::LayoutMode::Split,
                        ..Default::default()
                    },
                );
                // Keep a real, unstarted broker client: registration replacement is
                // exercised without discovering or launching a user daemon.
                let bootstrap = workdeck_session::SessionRegistrationBootstrap {
                    input_kind: workdeck_session::WorkdeckSessionInputKind::Diff,
                    changeset: app.with_state(|state| state.changeset().clone()),
                    source_label: "experimental reload regression".into(),
                    experimental: launch_experimental.unwrap_or(false),
                    initial_show_agent_notes: false,
                };
                let publication = app.review_producer().get_publication();
                let registration =
                    workdeck_session::create_session_registration(&bootstrap, &publication)
                        .unwrap();
                let session_id = registration.session_id.clone();
                let client = WorkdeckSessionBrokerClient::new(
                    registration,
                    workdeck_session::create_initial_session_snapshot(&bootstrap, &publication),
                );
                app.session_broker_client = Some(client.clone());
                let mut coordinator =
                    AppHostReloadCoordinator::new(initial, repo.path(), Some(repo.path())).unwrap();
                let (mut controller, replies) = queued_controller([message(
                    "reload-experimental",
                    WorkdeckSessionCommandInput::ReloadSession(ReloadSessionToolInput {
                        target_session: SessionSelector::default(),
                        next_input: serde_json::to_value(core_cli_input_to_daemon(file_input(
                            Some(true),
                        )))
                        .unwrap(),
                        source_path: None,
                    }),
                )]);
                let mut loader = |input: &CliInput,
                                  cwd: &Path,
                                  _: bool,
                                  _: &VcsCatalog,
                                  _: &[LoadedExtension]| {
                    assert_eq!(input.options().experimental, Some(false));
                    let CliInput::Files(files) = input else {
                        anyhow::bail!("expected file comparison")
                    };
                    Ok(DynamicReviewLoad {
                        input: input.clone(),
                        changeset: workdeck_vcs::load_file_comparison(
                            cwd,
                            Path::new(&files.left),
                            Path::new(&files.right),
                        )?,
                        replacement_extensions: None,
                        replacement_vcs_catalog: None,
                        host_options: DynamicReviewHostOptions {
                            command_cwd: cwd.to_owned(),
                            repo_root: Some(cwd.to_owned()),
                            ..Default::default()
                        },
                    })
                };
                let mut catalog = workdeck_vcs::bundled_vcs_catalog().clone();
                assert_eq!(
                    controller.process_pending(&mut app, &mut |app, input, options| {
                        let mut options = options.clone();
                        options.reset_app = Some(reset_app);
                        let plan = coordinator.plan(input, options)?;
                        crate::commit_dynamic_review_reload(
                            app,
                            &mut coordinator,
                            plan,
                            &mut loader,
                            &mut catalog,
                        )
                    }),
                    1
                );
                assert!(matches!(
                    replies[0].recv().unwrap().unwrap(),
                    WorkdeckSessionCommandResult::ReloadedSession(_)
                ));
                assert_eq!(
                    coordinator.current_input().options().experimental,
                    Some(false)
                );
                let registration = client.get_registration();
                assert_eq!(registration.session_id, session_id);
                assert_eq!(registration.info.experimental_features, Some(Vec::new()));
                assert_ne!(
                    registration.info.source_label, "experimental reload regression",
                    "the assertion must observe the replacement, not the launch registration"
                );
                assert_eq!(
                    app.options
                        .review_input
                        .as_ref()
                        .unwrap()
                        .options()
                        .experimental,
                    Some(false)
                );
                let mut comment: workdeck_session::CommentToolInput = serde_json::from_value(
                    serde_json::json!({"filePath":"after.ts","side":"new","line":1,
                        "summary":"Plain fallback","markup":"<badge>disabled</badge>"}),
                )
                .unwrap();
                let before = app.with_state(|state| state.state_revision());
                let (mut controller, replies) = queued_controller([message(
                    "comment-experimental",
                    WorkdeckSessionCommandInput::Comment(comment.clone()),
                )]);
                assert_eq!(
                    controller
                        .process_pending(&mut app, &mut |_, _, _| Err("unexpected reload".into())),
                    1
                );
                let error = replies[0].recv().unwrap().unwrap_err();
                assert!(error.contains("Relaunch Workdeck with --experimental"));
                assert_eq!(app.with_state(|state| state.state_revision()), before);
                assert!(app.with_state(|state| state.comments().is_empty()));
                comment.target.markup = None;
                app.session_add_live_comment(&comment, "plain-comment", false)
                    .unwrap();
                assert_eq!(app.with_state(|state| state.comments().len()), 1);
            }
        }
    }

    #[test]
    fn reload_plans_restore_launch_authority_validate_before_io_and_commit_explicitly() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join(".git")).unwrap();
        let initial = vcs_input(CommonOptions {
            experimental: Some(true),
            fast: Some(true),
            extensions: Some(false),
            extension_paths: vec!["launch-extension".into()],
            ..CommonOptions::default()
        });
        let mut coordinator =
            AppHostReloadCoordinator::new(initial.clone(), repo.path(), Some(repo.path())).unwrap();
        let requested = vcs_input(CommonOptions {
            experimental: Some(false),
            fast: Some(false),
            extensions: Some(true),
            extension_paths: vec!["remote-extension".into()],
            watch: Some(true),
            ..CommonOptions::default()
        });
        let value = serde_json::to_value(core_cli_input_to_daemon(requested)).unwrap();
        let plan = coordinator
            .plan(&value, ReloadSessionOptions::default())
            .unwrap();

        assert!(!coordinator.requires_extension_reload(&plan));
        assert_eq!(plan.input.options().experimental, Some(true));
        assert_eq!(plan.input.options().fast, Some(true));
        assert_eq!(plan.input.options().extensions, Some(false));
        assert_eq!(plan.input.options().extension_paths, ["launch-extension"]);
        assert_eq!(coordinator.current_input(), &initial);
        coordinator.commit(&plan);
        assert_eq!(coordinator.current_input(), &plan.input);
        assert_eq!(
            coordinator.current_cwd(),
            repo.path().canonicalize().unwrap()
        );
        let forced = coordinator
            .plan(
                &value,
                ReloadSessionOptions {
                    reload_extensions: Some(true),
                    ..ReloadSessionOptions::default()
                },
            )
            .unwrap();
        assert!(coordinator.requires_extension_reload(&forced));

        let option_like = CliInput::Vcs(VcsDiffCommandInput {
            range: Some("--output=/tmp/stolen".into()),
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions::default(),
        });
        let error = coordinator
            .plan(
                &serde_json::to_value(core_cli_input_to_daemon(option_like)).unwrap(),
                ReloadSessionOptions::default(),
            )
            .unwrap_err();
        assert!(error.contains("looks like a VCS option"), "{error}");
        assert_eq!(coordinator.current_input(), &plan.input);
    }

    #[test]
    fn frozen_app_host_oracle_covers_every_baseline_byte_and_both_pins() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../../port/hunk/oracles/app-host.json")).unwrap();
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["stable"], "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd");
        assert_eq!(oracle["source"]["baseline"]["bytes"], 23_127);
        assert_eq!(oracle["source"]["baseline"]["lines"], 538);
        assert_eq!(oracle["source"]["stable"]["bytes"], 22_525);
        assert_eq!(oracle["source"]["stable"]["lines"], 528);
        assert_eq!(oracle["applicationOracle"]["files"], 20);
        assert_eq!(oracle["applicationOracle"]["baseline"]["passed"], 220);
        assert_eq!(oracle["applicationOracle"]["stable"]["passed"], 212);
        assert_eq!(oracle["pinDelta"].as_array().unwrap().len(), 3);
        assert!(oracle["nativeTests"].as_array().unwrap().len() >= 20);

        let coverage = oracle["sourceCoverage"].as_array().unwrap();
        let mut next_line = 1;
        let mut next_byte = 0;
        for interval in coverage {
            let lines = interval["lines"].as_array().unwrap();
            let bytes = interval["bytes"].as_array().unwrap();
            assert_eq!(lines[0].as_u64().unwrap(), next_line);
            assert_eq!(bytes[0].as_u64().unwrap(), next_byte);
            next_line = lines[1].as_u64().unwrap() + 1;
            next_byte = bytes[1].as_u64().unwrap();
        }
        assert_eq!(next_line, 539);
        assert_eq!(next_byte, 23_127);
    }

    #[test]
    fn frozen_app_host_extensions_oracle_maps_both_pins_and_every_source_test() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/app-host-extensions.json"
        ))
        .unwrap();
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(oracle["source"]["baseline"]["bytes"], 52_443);
        assert_eq!(oracle["source"]["baseline"]["lines"], 1_382);
        assert_eq!(
            oracle["source"]["stable"]["commit"],
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        );
        assert_eq!(oracle["source"]["stable"]["bytes"], 51_599);
        assert_eq!(oracle["source"]["stable"]["lines"], 1_368);
        for pin in ["baseline", "stable"] {
            assert_eq!(oracle["oracleRuns"][pin]["passed"], 19);
            assert_eq!(oracle["oracleRuns"][pin]["failed"], 0);
            assert_eq!(
                oracle["oracleRuns"][pin]["caseMilliseconds"]
                    .as_array()
                    .unwrap()
                    .len(),
                19
            );
        }

        let expected = [
            "--no-extensions still disables extensions when a reload re-runs discovery",
            "a failed replacement keeps the visible extension instance running",
            "a reloaded files replacement does not take toggle control from the open fallback",
            "retires a prepared replacement when broker commit preparation throws",
            "refuses a queued replacement reload after quit becomes terminal",
            "owns and retires a replacement still inside its asynchronous factory",
            "retires an in-flight replacement instead of adopting it after quit",
            "waits for an adopted runtime's in-flight retirement before quit",
            "serializes concurrent reloads so every replacement receives a full lifecycle",
            "--extension paths survive a reload that re-runs discovery",
            "delivers same-runtime reload events after the new review commits",
            "fires again when a soft reload replaces the selected file with the same id",
            "fires when granting trust loads a repo extension for the first time",
            "fires when the session was launched through a non-canonical repo path",
            "starts a replacement only after its mounted sidebar controls are ready",
            "revokes retained panes and dialogs before a soft replacement shuts down",
            "shuts down and starts each replacement extension instance",
            "an extension backend keeps a checkout no built-in recognizes",
            "a nearer extension checkout inside a Git repository survives reload",
        ];
        let mappings = oracle["testMappings"].as_array().unwrap();
        assert_eq!(
            mappings
                .iter()
                .map(|mapping| mapping["upstream"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(mappings.iter().all(|mapping| {
            mapping["rustTests"].as_array().is_some_and(|tests| {
                !tests.is_empty()
                    && tests.iter().all(|test| {
                        test.as_str()
                            .is_some_and(|selector| selector.contains(".rs#"))
                    })
            })
        }));
        assert_eq!(oracle["baselineDelta"].as_array().unwrap().len(), 2);
    }
}
