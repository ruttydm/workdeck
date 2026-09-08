//! Session CLI command composition, daemon compatibility checks, and stable output routing.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use thiserror::Error;
use workdeck_core::{
    CliInput, CommonOptions, HighlightTone, InputCursorLine, InputLayoutMode, NavigationDirection,
    RevealMode, ReviewNoteSource, SessionCommandInput, SessionCommandOutput,
    SessionCommentApplyItemInput, SessionCommentListType, SessionSelectorInput, SidebarVisibility,
};

use crate::{
    DaemonCliInput, DaemonCommentApplyItem, DaemonCommentDirection, DaemonCommentListType,
    DaemonCommonOptions, DaemonCursorLine, DaemonLayoutMode, DaemonRangeEndpoints,
    DaemonRevealMode, DaemonSidebarAuto, DaemonSidebarVisibility, HttpWorkdeckSessionCliClient,
    SessionCommentAddCliInput, SessionCommentApplyCliInput, SessionCommentClearCliInput,
    SessionCommentListCliInput, SessionCommentRemoveCliInput, SessionDaemonAction,
    SessionHighlightAddCliInput, SessionHighlightClearCliInput, SessionLineHighlightTone,
    SessionNavigateCliInput, SessionReloadCliInput, SessionReviewCliInput, SessionSelector,
    WORKDECK_SESSION_API_VERSION, WorkdeckSessionCliClient, WorkdeckSessionCliClientError,
    format_clear_comments_output, format_clear_highlights_output, format_comment_apply_output,
    format_comment_list_output, format_comment_output, format_context_output,
    format_highlight_output, format_list_output, format_navigation_output, format_note_list_output,
    format_reload_output, format_remove_comment_output, format_review_output,
    format_session_output, is_loopback_port_reachable, is_session_broker_healthy,
    normalize_session_selector, resolve_session_broker_config, stringify_json,
};

const AVAILABILITY_TIMEOUT: Duration = Duration::from_millis(500);

#[derive(Debug, Error)]
pub enum SessionCommandError {
    #[error(transparent)]
    Client(#[from] WorkdeckSessionCliClientError),
    #[error("{0}")]
    Message(String),
    #[error("session selector normalization failed: {0}")]
    Selector(#[from] std::io::Error),
    #[error("session output JSON failed: {0}")]
    Json(#[from] serde_json::Error),
}

type ClientFactory =
    dyn Fn() -> Result<Arc<dyn WorkdeckSessionCliClient>, SessionCommandError> + Send + Sync;
type AvailabilityProbe =
    dyn Fn(SessionDaemonAction) -> Result<bool, SessionCommandError> + Send + Sync;

/// Injectable command runner; production defaults stay state-free until a daemon-backed action runs.
#[derive(Clone)]
pub struct SessionCommandRunner {
    client_factory: Arc<ClientFactory>,
    availability: Arc<AvailabilityProbe>,
}

impl std::fmt::Debug for SessionCommandRunner {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionCommandRunner")
            .finish_non_exhaustive()
    }
}

impl SessionCommandRunner {
    #[must_use]
    pub fn with_hooks(
        client_factory: Arc<ClientFactory>,
        availability: Arc<AvailabilityProbe>,
    ) -> Self {
        Self {
            client_factory,
            availability,
        }
    }

    pub fn from_environment(env: BTreeMap<String, String>) -> Self {
        let client_env = env.clone();
        let availability_env = env;
        Self::with_hooks(
            Arc::new(move || {
                Ok(Arc::new(HttpWorkdeckSessionCliClient::from_environment(
                    client_env.clone(),
                    Duration::from_millis(crate::WORKDECK_SESSION_DAEMON_HTTP_TIMEOUT_MS),
                )?) as Arc<dyn WorkdeckSessionCliClient>)
            }),
            Arc::new(move |action| resolve_daemon_availability(action, &availability_env)),
        )
    }

    pub fn from_process_environment() -> Self {
        Self::from_environment(std::env::vars().collect())
    }

    pub fn run(&self, input: SessionCommandInput) -> Result<String, SessionCommandError> {
        let action = command_action(&input);
        let daemon_available = (self.availability)(action)?;
        let output = command_output(&input);
        if !daemon_available && action == SessionDaemonAction::List {
            return render_output(output, &json!({"sessions": []}), || format_list_output(&[]));
        }

        let original_selector = command_selector(&input).map(selector_from_input);
        let normalized_selector = original_selector
            .as_ref()
            .map(normalize_session_selector)
            .transpose()?;
        let client = (self.client_factory)()?;
        ensure_required_action(action, client.as_ref())?;

        match input {
            SessionCommandInput::List { output } => {
                let sessions = client.list_sessions()?;
                render_output(output, &json!({"sessions": sessions}), || {
                    format_list_output(&sessions)
                })
            }
            SessionCommandInput::Get {
                context,
                output,
                selector: _,
            } => {
                let selector = required_selector(normalized_selector)?;
                if context {
                    let context = client.get_selected_context(selector)?;
                    render_output(output, &json!({"context": context}), || {
                        format_context_output(&context)
                    })
                } else {
                    let session = client.get_session(selector)?;
                    render_output(output, &json!({"session": session}), || {
                        format_session_output(&session)
                    })
                }
            }
            SessionCommandInput::Review {
                output,
                selector: _,
                include_patch,
                include_notes,
            } => {
                let review = client.get_session_review(SessionReviewCliInput {
                    selector: required_selector(normalized_selector)?,
                    include_patch,
                    include_notes,
                })?;
                render_output(output, &json!({"review": review}), || {
                    format_review_output(&review)
                })
            }
            SessionCommandInput::Navigate {
                output,
                selector: _,
                file_path,
                hunk_number,
                side,
                line,
                comment_direction,
                comment_id,
            } => {
                let result = client.navigate_to_hunk(SessionNavigateCliInput {
                    selector: required_selector(normalized_selector)?,
                    file_path,
                    hunk_number,
                    side,
                    line,
                    comment_direction: comment_direction.map(navigation_direction),
                    comment_id,
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_navigation_output(&display_selector, &result)
                })
            }
            SessionCommandInput::Reload {
                output,
                selector: _,
                next_input,
                source_path,
            } => {
                let result = client.reload_session(SessionReloadCliInput {
                    selector: required_selector(normalized_selector)?,
                    next_input: daemon_cli_input(*next_input),
                    source_path,
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_reload_output(&display_selector, &result)
                })
            }
            SessionCommandInput::CommentAdd {
                output,
                selector: _,
                file_path,
                side,
                line,
                summary,
                rationale,
                markup,
                author,
                reveal,
            } => {
                let result = client.add_comment(SessionCommentAddCliInput {
                    selector: required_selector(normalized_selector)?,
                    file_path,
                    side,
                    line,
                    summary,
                    rationale,
                    markup,
                    author,
                    reveal,
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_comment_output(&display_selector, &result)
                })
            }
            SessionCommandInput::CommentApply {
                output,
                selector: _,
                comments,
                reveal_mode,
            } => {
                let result = client.apply_comments(SessionCommentApplyCliInput {
                    selector: required_selector(normalized_selector)?,
                    comments: comments.into_iter().map(daemon_comment).collect(),
                    reveal_mode: daemon_reveal_mode(reveal_mode),
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_comment_apply_output(&display_selector, &result)
                })
            }
            SessionCommandInput::CommentList {
                output,
                selector: _,
                file_path,
                list_type,
            } => {
                let comments = client.list_comments(SessionCommentListCliInput {
                    selector: required_selector(normalized_selector)?,
                    file_path,
                    list_type: list_type.map(daemon_comment_list_type),
                })?;
                if output == SessionCommandOutput::Json {
                    return render_output(output, &json!({"comments": comments}), String::new);
                }
                let display_selector = required_selector(original_selector)?;
                if list_type.is_some_and(|kind| kind != SessionCommentListType::Live) {
                    let notes = comments
                        .into_iter()
                        .map(|comment| match comment {
                            crate::SessionCommentSummary::Review(note) => Ok(note),
                            crate::SessionCommentSummary::Live(_) => Err(SessionCommandError::Message(
                                "Session daemon returned a live comment for a review-note listing.".into(),
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(format_note_list_output(&display_selector, &notes))
                } else {
                    let comments = comments
                        .into_iter()
                        .map(|comment| match comment {
                            crate::SessionCommentSummary::Live(comment) => Ok(comment),
                            crate::SessionCommentSummary::Review(_) => Err(SessionCommandError::Message(
                                "Session daemon returned a review note for a live-comment listing.".into(),
                            )),
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    Ok(format_comment_list_output(&display_selector, &comments))
                }
            }
            SessionCommandInput::CommentRemove {
                output,
                selector: _,
                comment_id,
            } => {
                let result = client.remove_comment(SessionCommentRemoveCliInput {
                    selector: required_selector(normalized_selector)?,
                    comment_id,
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_remove_comment_output(&display_selector, &result)
                })
            }
            SessionCommandInput::CommentClear {
                output,
                selector: _,
                file_path,
                include_user,
                confirmed: _,
            } => {
                let result = client.clear_comments(SessionCommentClearCliInput {
                    selector: required_selector(normalized_selector)?,
                    file_path,
                    include_user,
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_clear_comments_output(&display_selector, &result)
                })
            }
            SessionCommandInput::HighlightAdd {
                output,
                selector: _,
                file_path,
                side,
                line,
                start,
                end,
                tone,
                reveal,
            } => {
                let result = client.add_highlight(SessionHighlightAddCliInput {
                    selector: required_selector(normalized_selector)?,
                    file_path,
                    side,
                    line,
                    start,
                    end,
                    tone: tone.map(daemon_highlight_tone),
                    reveal,
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_highlight_output(&display_selector, &result)
                })
            }
            SessionCommandInput::HighlightClear {
                output,
                selector: _,
                file_path,
            } => {
                let result = client.clear_highlights(SessionHighlightClearCliInput {
                    selector: required_selector(normalized_selector)?,
                    file_path,
                })?;
                let display_selector = required_selector(original_selector)?;
                render_output(output, &json!({"result": result}), || {
                    format_clear_highlights_output(&display_selector, &result)
                })
            }
        }
    }
}

pub fn run_session_command(input: SessionCommandInput) -> Result<String, SessionCommandError> {
    SessionCommandRunner::from_process_environment().run(input)
}

fn ensure_required_action(
    action: SessionDaemonAction,
    client: &dyn WorkdeckSessionCliClient,
) -> Result<(), SessionCommandError> {
    let capabilities = match client.get_capabilities() {
        Ok(capabilities) => capabilities,
        Err(WorkdeckSessionCliClientError::Authentication) => None,
        Err(error) => return Err(error.into()),
    };
    if capabilities.as_ref().is_some_and(|capabilities| {
        capabilities.version == WORKDECK_SESSION_API_VERSION
            && capabilities.actions.contains(&action)
    }) {
        return Ok(());
    }
    Err(SessionCommandError::Message(format!(
        "The running Workdeck session daemon is incompatible or missing required support for {}. Close older Workdeck windows, wait for the daemon to become idle, then retry this command.",
        action_name(action)
    )))
}

pub fn resolve_daemon_availability(
    action: SessionDaemonAction,
    env: &BTreeMap<String, String>,
) -> Result<bool, SessionCommandError> {
    let config = resolve_session_broker_config(env)
        .map_err(|error| SessionCommandError::Message(error.to_string()))?;
    if is_session_broker_healthy(&config, AVAILABILITY_TIMEOUT) {
        return Ok(true);
    }
    if is_loopback_port_reachable(&config, AVAILABILITY_TIMEOUT) {
        return Err(SessionCommandError::Message(format!(
            "Workdeck session daemon port {}:{} is already in use by another process. Stop the conflicting process or set WORKDECK_MCP_PORT to a different loopback port.",
            config.host, config.port
        )));
    }
    if action == SessionDaemonAction::List {
        Ok(false)
    } else {
        Err(SessionCommandError::Message(
            crate::NO_ACTIVE_SESSIONS_MESSAGE.into(),
        ))
    }
}

fn render_output<T: Serialize>(
    output: SessionCommandOutput,
    value: &T,
    format_text: impl FnOnce() -> String,
) -> Result<String, SessionCommandError> {
    match output {
        SessionCommandOutput::Json => Ok(stringify_json(value)?),
        SessionCommandOutput::Text => Ok(format_text()),
    }
}

const fn command_action(input: &SessionCommandInput) -> SessionDaemonAction {
    match input {
        SessionCommandInput::List { .. } => SessionDaemonAction::List,
        SessionCommandInput::Get { context: false, .. } => SessionDaemonAction::Get,
        SessionCommandInput::Get { context: true, .. } => SessionDaemonAction::Context,
        SessionCommandInput::Review { .. } => SessionDaemonAction::Review,
        SessionCommandInput::Navigate { .. } => SessionDaemonAction::Navigate,
        SessionCommandInput::Reload { .. } => SessionDaemonAction::Reload,
        SessionCommandInput::CommentAdd { .. } => SessionDaemonAction::CommentAdd,
        SessionCommandInput::CommentApply { .. } => SessionDaemonAction::CommentApply,
        SessionCommandInput::CommentList { .. } => SessionDaemonAction::CommentList,
        SessionCommandInput::CommentRemove { .. } => SessionDaemonAction::CommentRm,
        SessionCommandInput::CommentClear { .. } => SessionDaemonAction::CommentClear,
        SessionCommandInput::HighlightAdd { .. } => SessionDaemonAction::HighlightAdd,
        SessionCommandInput::HighlightClear { .. } => SessionDaemonAction::HighlightClear,
    }
}

const fn command_output(input: &SessionCommandInput) -> SessionCommandOutput {
    match input {
        SessionCommandInput::List { output }
        | SessionCommandInput::Get { output, .. }
        | SessionCommandInput::Review { output, .. }
        | SessionCommandInput::Navigate { output, .. }
        | SessionCommandInput::Reload { output, .. }
        | SessionCommandInput::CommentAdd { output, .. }
        | SessionCommandInput::CommentApply { output, .. }
        | SessionCommandInput::CommentList { output, .. }
        | SessionCommandInput::CommentRemove { output, .. }
        | SessionCommandInput::CommentClear { output, .. }
        | SessionCommandInput::HighlightAdd { output, .. }
        | SessionCommandInput::HighlightClear { output, .. } => *output,
    }
}

fn command_selector(input: &SessionCommandInput) -> Option<&SessionSelectorInput> {
    match input {
        SessionCommandInput::List { .. } => None,
        SessionCommandInput::Get { selector, .. }
        | SessionCommandInput::Review { selector, .. }
        | SessionCommandInput::Navigate { selector, .. }
        | SessionCommandInput::Reload { selector, .. }
        | SessionCommandInput::CommentAdd { selector, .. }
        | SessionCommandInput::CommentApply { selector, .. }
        | SessionCommandInput::CommentList { selector, .. }
        | SessionCommandInput::CommentRemove { selector, .. }
        | SessionCommandInput::CommentClear { selector, .. }
        | SessionCommandInput::HighlightAdd { selector, .. }
        | SessionCommandInput::HighlightClear { selector, .. } => Some(selector),
    }
}

fn selector_from_input(input: &SessionSelectorInput) -> SessionSelector {
    SessionSelector {
        session_id: input.session_id.clone(),
        session_path: input.session_path.as_ref().map(PathBuf::from),
        repo_root: input.repo_root.as_ref().map(PathBuf::from),
        repo_boundary: input.repo_boundary.as_ref().map(PathBuf::from),
    }
}

fn required_selector(
    selector: Option<SessionSelector>,
) -> Result<SessionSelector, SessionCommandError> {
    selector
        .ok_or_else(|| SessionCommandError::Message("session command requires a selector".into()))
}

fn daemon_comment(comment: SessionCommentApplyItemInput) -> DaemonCommentApplyItem {
    DaemonCommentApplyItem {
        file_path: comment.file_path,
        hunk_number: comment.hunk_number,
        side: comment.side,
        line: comment.line,
        summary: comment.summary,
        rationale: comment.rationale,
        markup: comment.markup,
        author: comment.author,
    }
}

fn daemon_common_options(options: CommonOptions) -> DaemonCommonOptions {
    DaemonCommonOptions {
        mode: options.mode.map(|mode| match mode {
            InputLayoutMode::Auto => DaemonLayoutMode::Auto,
            InputLayoutMode::Split => DaemonLayoutMode::Split,
            InputLayoutMode::Stack => DaemonLayoutMode::Stack,
        }),
        cursor_line: options.cursor_line.map(|cursor| match cursor {
            InputCursorLine::Row => DaemonCursorLine::Row,
            InputCursorLine::Number => DaemonCursorLine::Number,
            InputCursorLine::Off => DaemonCursorLine::Off,
        }),
        vcs: options.vcs,
        theme: options.theme,
        agent_context: options.agent_context,
        pager: options.pager,
        watch: options.watch,
        experimental: options.experimental,
        fast: options.fast,
        exclude_untracked: options.exclude_untracked,
        line_numbers: options.line_numbers,
        tab_width: options.tab_width.map(u64::from),
        file_gap: options.file_gap.map(u64::from),
        hunk_gap: options.hunk_gap.map(u64::from),
        wrap_lines: options.wrap_lines,
        hunk_headers: options.hunk_headers,
        menu_bar: options.menu_bar,
        sidebar: options.sidebar.map(|sidebar| match sidebar {
            SidebarVisibility::Auto => DaemonSidebarVisibility::Auto(DaemonSidebarAuto::Auto),
            SidebarVisibility::Visible => DaemonSidebarVisibility::Visible(true),
            SidebarVisibility::Hidden => DaemonSidebarVisibility::Visible(false),
        }),
        agent_notes: options.agent_notes,
        copy_decorations: options.copy_decorations,
        prompt_save_view_preferences: options.prompt_save_view_preferences,
        transparent_background: options.transparent_background,
        color_moved: options.color_moved,
        extensions: options.extensions,
        extension_paths: (!options.extension_paths.is_empty()).then_some(options.extension_paths),
    }
}

fn daemon_cli_input(input: CliInput) -> DaemonCliInput {
    match input {
        CliInput::Vcs(input) => DaemonCliInput::Vcs {
            range: input.range,
            range_endpoints: input.range_endpoints.map(|range| DaemonRangeEndpoints {
                from: range.from,
                to: range.to,
            }),
            staged: input.staged,
            pathspecs: (!input.pathspecs.is_empty()).then_some(input.pathspecs),
            options: daemon_common_options(input.options),
        },
        CliInput::Show(input) => DaemonCliInput::Show {
            reference: input.reference,
            pathspecs: (!input.pathspecs.is_empty()).then_some(input.pathspecs),
            options: daemon_common_options(input.options),
        },
        CliInput::StashShow(input) => DaemonCliInput::StashShow {
            reference: input.reference,
            options: daemon_common_options(input.options),
        },
        CliInput::Files(input) => DaemonCliInput::Diff {
            left: input.left,
            right: input.right,
            options: daemon_common_options(input.options),
        },
        CliInput::Patch(input) => DaemonCliInput::Patch {
            file: input.file,
            text: input.text,
            options: daemon_common_options(input.options),
        },
        CliInput::DiffTool(input) => DaemonCliInput::Difftool {
            left: input.left,
            right: input.right,
            path: input.path,
            options: daemon_common_options(input.options),
        },
    }
}

const fn navigation_direction(direction: NavigationDirection) -> DaemonCommentDirection {
    match direction {
        NavigationDirection::Next => DaemonCommentDirection::Next,
        NavigationDirection::Previous => DaemonCommentDirection::Prev,
    }
}

const fn daemon_reveal_mode(mode: RevealMode) -> DaemonRevealMode {
    match mode {
        RevealMode::None => DaemonRevealMode::None,
        RevealMode::First => DaemonRevealMode::First,
    }
}

const fn daemon_comment_list_type(kind: SessionCommentListType) -> DaemonCommentListType {
    match kind {
        SessionCommentListType::Live => DaemonCommentListType::Live,
        SessionCommentListType::All => DaemonCommentListType::All,
        SessionCommentListType::Source(ReviewNoteSource::Ai) => DaemonCommentListType::Ai,
        SessionCommentListType::Source(ReviewNoteSource::Agent) => DaemonCommentListType::Agent,
        SessionCommentListType::Source(ReviewNoteSource::User) => DaemonCommentListType::User,
    }
}

const fn daemon_highlight_tone(tone: HighlightTone) -> SessionLineHighlightTone {
    match tone {
        HighlightTone::Match => SessionLineHighlightTone::Match,
        HighlightTone::Current => SessionLineHighlightTone::Current,
        HighlightTone::Info => SessionLineHighlightTone::Info,
        HighlightTone::Warning => SessionLineHighlightTone::Warning,
        HighlightTone::Error => SessionLineHighlightTone::Error,
        HighlightTone::Dim => SessionLineHighlightTone::Dim,
    }
}

const fn action_name(action: SessionDaemonAction) -> &'static str {
    match action {
        SessionDaemonAction::List => "list",
        SessionDaemonAction::Get => "get",
        SessionDaemonAction::Context => "context",
        SessionDaemonAction::Review => "review",
        SessionDaemonAction::Navigate => "navigate",
        SessionDaemonAction::Reload => "reload",
        SessionDaemonAction::CommentAdd => "comment-add",
        SessionDaemonAction::CommentApply => "comment-apply",
        SessionDaemonAction::CommentList => "comment-list",
        SessionDaemonAction::CommentRm => "comment-rm",
        SessionDaemonAction::CommentClear => "comment-clear",
        SessionDaemonAction::HighlightAdd => "highlight-add",
        SessionDaemonAction::HighlightClear => "highlight-clear",
        SessionDaemonAction::Quit => "quit",
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    use serde_json::Value;
    use tempfile::TempDir;
    use workdeck_core::{
        FileCommandInput, ReviewSide, SessionCommentApplyItemInput, VcsDiffCommandInput,
        VcsRangeEndpoints, VcsShowCommandInput,
    };

    use super::*;
    use crate::{
        AppliedCommentBatchResult, AppliedCommentResult, AppliedHighlightResult,
        ClearedCommentsResult, ClearedHighlightsResult, ListedSession, NavigatedSelectionResult,
        ReloadedSessionResult, RemovedCommentResult, RevealedTarget, SelectedSessionContext,
        SessionDaemonCapabilities, SessionFileSummary, SessionLiveCommentSummary, SessionReview,
        SessionReviewFile, SessionReviewHunk, SessionReviewNoteSummary, SessionSnapshot,
        SessionTerminalLocation, SessionTerminalMetadata, WORKDECK_SESSION_DAEMON_VERSION,
        WorkdeckSessionInputKind, WorkdeckSessionState,
    };

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        List,
        Get(SessionSelector),
        Context(SessionSelector),
        Review(SessionReviewCliInput),
        Navigate(SessionNavigateCliInput),
        Reload(SessionReloadCliInput),
        CommentAdd(SessionCommentAddCliInput),
        CommentApply(SessionCommentApplyCliInput),
        CommentList(SessionCommentListCliInput),
        CommentRemove(SessionCommentRemoveCliInput),
        CommentClear(SessionCommentClearCliInput),
        HighlightAdd(SessionHighlightAddCliInput),
        HighlightClear(SessionHighlightClearCliInput),
    }

    struct FakeState {
        capabilities: Result<Option<SessionDaemonCapabilities>, WorkdeckSessionCliClientError>,
        calls: Vec<Call>,
        sessions: Vec<ListedSession>,
        session: ListedSession,
        context: SelectedSessionContext,
        review: SessionReview,
        navigation: NavigatedSelectionResult,
        reload: ReloadedSessionResult,
        comment: AppliedCommentResult,
        comment_batch: AppliedCommentBatchResult,
        comments: Vec<crate::SessionCommentSummary>,
        removed: RemovedCommentResult,
        cleared: ClearedCommentsResult,
        highlight: AppliedHighlightResult,
        highlights_cleared: ClearedHighlightsResult,
    }

    #[derive(Clone)]
    struct FakeClient(Arc<Mutex<FakeState>>);

    impl FakeClient {
        fn new() -> Self {
            let session = listed_session(None);
            Self(Arc::new(Mutex::new(FakeState {
                capabilities: Ok(Some(all_capabilities())),
                calls: Vec::new(),
                sessions: vec![session.clone()],
                session,
                context: selected_context(),
                review: review(false, false),
                navigation: NavigatedSelectionResult {
                    file_id: "file-1".into(),
                    file_path: "README.md".into(),
                    hunk_index: 0,
                    selected_hunk: None,
                    revealed: None,
                    side: None,
                    line: None,
                },
                reload: ReloadedSessionResult {
                    session_id: "session-1".into(),
                    input_kind: WorkdeckSessionInputKind::Show,
                    title: "repo show HEAD~1".into(),
                    source_label: "/repo".into(),
                    file_count: 1,
                    selected_file_path: Some("README.md".into()),
                    selected_hunk_index: 0,
                },
                comment: comment(),
                comment_batch: AppliedCommentBatchResult {
                    applied: vec![comment()],
                },
                comments: Vec::new(),
                removed: RemovedCommentResult {
                    comment_id: "comment-1".into(),
                    removed: true,
                    remaining_comment_count: 0,
                    source: None,
                },
                cleared: ClearedCommentsResult {
                    removed_count: 0,
                    remaining_comment_count: 0,
                    file_path: None,
                    include_user: None,
                    removed_live_comment_count: None,
                    removed_user_note_count: None,
                    remaining_live_comment_count: None,
                    remaining_user_note_count: None,
                },
                highlight: AppliedHighlightResult {
                    file_id: "file-1".into(),
                    file_path: "README.md".into(),
                    hunk_index: 0,
                    side: ReviewSide::New,
                    line: 2,
                    start: 4,
                    end: 11,
                    tone: SessionLineHighlightTone::Info,
                    file_mark_count: 1,
                    revealed: Some(RevealedTarget::Line),
                },
                highlights_cleared: ClearedHighlightsResult {
                    removed_count: 1,
                    remaining_count: 0,
                    file_path: None,
                },
            })))
        }

        fn edit(&self, edit: impl FnOnce(&mut FakeState)) {
            edit(&mut self.0.lock().unwrap_or_else(|error| error.into_inner()));
        }

        fn calls(&self) -> Vec<Call> {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .calls
                .clone()
        }
    }

    impl WorkdeckSessionCliClient for FakeClient {
        fn get_capabilities(
            &self,
        ) -> Result<Option<SessionDaemonCapabilities>, WorkdeckSessionCliClientError> {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .capabilities
                .clone()
        }

        fn list_sessions(&self) -> Result<Vec<ListedSession>, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::List);
            Ok(state.sessions.clone())
        }

        fn get_session(
            &self,
            selector: SessionSelector,
        ) -> Result<ListedSession, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::Get(selector));
            Ok(state.session.clone())
        }

        fn get_selected_context(
            &self,
            selector: SessionSelector,
        ) -> Result<SelectedSessionContext, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::Context(selector));
            Ok(state.context.clone())
        }

        fn get_session_review(
            &self,
            input: SessionReviewCliInput,
        ) -> Result<SessionReview, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::Review(input));
            Ok(state.review.clone())
        }

        fn navigate_to_hunk(
            &self,
            input: SessionNavigateCliInput,
        ) -> Result<NavigatedSelectionResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::Navigate(input));
            Ok(state.navigation.clone())
        }

        fn reload_session(
            &self,
            input: SessionReloadCliInput,
        ) -> Result<ReloadedSessionResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::Reload(input));
            Ok(state.reload.clone())
        }

        fn add_comment(
            &self,
            input: SessionCommentAddCliInput,
        ) -> Result<AppliedCommentResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::CommentAdd(input));
            Ok(state.comment.clone())
        }

        fn apply_comments(
            &self,
            input: SessionCommentApplyCliInput,
        ) -> Result<AppliedCommentBatchResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::CommentApply(input));
            Ok(state.comment_batch.clone())
        }

        fn list_comments(
            &self,
            input: SessionCommentListCliInput,
        ) -> Result<Vec<crate::SessionCommentSummary>, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::CommentList(input));
            Ok(state.comments.clone())
        }

        fn remove_comment(
            &self,
            input: SessionCommentRemoveCliInput,
        ) -> Result<RemovedCommentResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::CommentRemove(input));
            Ok(state.removed.clone())
        }

        fn clear_comments(
            &self,
            input: SessionCommentClearCliInput,
        ) -> Result<ClearedCommentsResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::CommentClear(input));
            Ok(state.cleared.clone())
        }

        fn add_highlight(
            &self,
            input: SessionHighlightAddCliInput,
        ) -> Result<AppliedHighlightResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::HighlightAdd(input));
            Ok(state.highlight.clone())
        }

        fn clear_highlights(
            &self,
            input: SessionHighlightClearCliInput,
        ) -> Result<ClearedHighlightsResult, WorkdeckSessionCliClientError> {
            let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
            state.calls.push(Call::HighlightClear(input));
            Ok(state.highlights_cleared.clone())
        }
    }

    fn all_capabilities() -> SessionDaemonCapabilities {
        SessionDaemonCapabilities {
            version: WORKDECK_SESSION_API_VERSION,
            daemon_version: WORKDECK_SESSION_DAEMON_VERSION,
            actions: vec![
                SessionDaemonAction::List,
                SessionDaemonAction::Get,
                SessionDaemonAction::Context,
                SessionDaemonAction::Review,
                SessionDaemonAction::Navigate,
                SessionDaemonAction::Reload,
                SessionDaemonAction::CommentAdd,
                SessionDaemonAction::CommentApply,
                SessionDaemonAction::CommentList,
                SessionDaemonAction::CommentRm,
                SessionDaemonAction::CommentClear,
                SessionDaemonAction::HighlightAdd,
                SessionDaemonAction::HighlightClear,
            ],
        }
    }

    fn selector_input() -> SessionSelectorInput {
        SessionSelectorInput {
            session_id: Some("session-1".into()),
            ..SessionSelectorInput::default()
        }
    }

    fn selector() -> SessionSelector {
        selector_from_input(&selector_input())
    }

    fn session_file() -> SessionFileSummary {
        SessionFileSummary {
            id: "file-1".into(),
            path: "README.md".into(),
            previous_path: None,
            additions: 1,
            deletions: 0,
            hunk_count: 1,
        }
    }

    fn listed_session(terminal: Option<SessionTerminalMetadata>) -> ListedSession {
        ListedSession {
            session_id: "session-1".into(),
            pid: 42,
            cwd: "/repo".into(),
            repo_root: Some("/repo".into()),
            launched_at: "2026-01-01T00:00:00Z".into(),
            terminal,
            input_kind: WorkdeckSessionInputKind::Diff,
            title: "repo diff".into(),
            source_label: "/repo".into(),
            experimental_features: None,
            file_count: 1,
            files: vec![session_file()],
            snapshot: SessionSnapshot {
                updated_at: "2026-01-01T00:00:00Z".into(),
                state: WorkdeckSessionState {
                    selected_file_id: Some("file-1".into()),
                    selected_file_path: Some("README.md".into()),
                    selected_hunk_index: 0,
                    selected_hunk_old_range: Some([1, 1]),
                    selected_hunk_new_range: Some([1, 2]),
                    show_agent_notes: false,
                    note_markup_width: None,
                    live_comment_count: 0,
                    live_comments: Vec::new(),
                    review_note_count: None,
                    review_notes: None,
                    review_publication: None,
                },
            },
        }
    }

    fn selected_context() -> SelectedSessionContext {
        SelectedSessionContext {
            session_id: "session-1".into(),
            title: "repo diff".into(),
            source_label: "/repo".into(),
            cwd: Some("/repo".into()),
            repo_root: Some("/repo".into()),
            input_kind: WorkdeckSessionInputKind::Diff,
            experimental_features: None,
            selected_file: Some(session_file()),
            selected_hunk: None,
            show_agent_notes: false,
            note_markup_width: None,
            live_comment_count: 0,
        }
    }

    fn review(include_patch: bool, include_notes: bool) -> SessionReview {
        let hunk = SessionReviewHunk {
            index: 0,
            header: "@@ -1,1 +1,2 @@".into(),
            old_range: Some([1, 1]),
            new_range: Some([1, 2]),
        };
        let file = SessionReviewFile {
            summary: session_file(),
            patch: include_patch.then(|| "@@ -1,1 +1,2 @@".into()),
            hunks: vec![hunk.clone()],
        };
        let notes = include_notes.then(|| vec![review_note()]);
        SessionReview {
            session_id: "session-1".into(),
            title: "repo diff".into(),
            source_label: "/repo".into(),
            cwd: None,
            repo_root: Some("/repo".into()),
            input_kind: WorkdeckSessionInputKind::Diff,
            experimental_features: None,
            selected_file: Some(file.clone()),
            selected_hunk: Some(hunk),
            show_agent_notes: false,
            live_comment_count: 0,
            review_note_count: include_notes.then_some(1),
            review_notes: notes,
            files: vec![file],
        }
    }

    fn comment() -> AppliedCommentResult {
        AppliedCommentResult {
            comment_id: "comment-1".into(),
            file_id: "file-1".into(),
            file_path: "README.md".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 1,
            markup_width: None,
            markup_notes: None,
        }
    }

    fn live_comment() -> SessionLiveCommentSummary {
        SessionLiveCommentSummary {
            comment_id: "comment-1".into(),
            file_path: "README.md".into(),
            hunk_index: 0,
            side: ReviewSide::New,
            line: 2,
            summary: "Explain this line".into(),
            rationale: None,
            author: Some("agent".into()),
            created_at: "2026-05-10T00:00:00.000Z".into(),
        }
    }

    fn review_note() -> SessionReviewNoteSummary {
        SessionReviewNoteSummary {
            note_id: "user:1".into(),
            parent_id: None,
            source: ReviewNoteSource::User,
            file_path: "README.md".into(),
            hunk_index: Some(0),
            old_range: None,
            new_range: None,
            body: "Human note".into(),
            title: None,
            author: Some("user".into()),
            created_at: "2026-05-10T00:00:00.000Z".into(),
            updated_at: None,
            editable: true,
        }
    }

    fn runner(client: FakeClient, available: bool) -> SessionCommandRunner {
        SessionCommandRunner::with_hooks(
            Arc::new(move || Ok(Arc::new(client.clone()))),
            Arc::new(move |_| Ok(available)),
        )
    }

    fn json_output(output: &str) -> Value {
        serde_json::from_str(output).unwrap()
    }

    #[test]
    fn incompatible_daemon_fails_before_executing_the_action() {
        let client = FakeClient::new();
        client.edit(|state| state.capabilities = Ok(None));
        let error = runner(client.clone(), true)
            .run(SessionCommandInput::Get {
                context: true,
                output: SessionCommandOutput::Json,
                selector: selector_input(),
            })
            .unwrap_err();
        assert!(error.to_string().contains("Close older Workdeck windows"));
        assert!(client.calls().is_empty());
    }

    #[test]
    fn signed_negotiation_failure_maps_to_quiescent_upgrade_guidance() {
        let client = FakeClient::new();
        client
            .edit(|state| state.capabilities = Err(WorkdeckSessionCliClientError::Authentication));
        let error = runner(client, true)
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Json,
            })
            .unwrap_err();
        assert!(error.to_string().contains("Close older Workdeck windows"));
    }

    #[test]
    fn local_credential_store_failures_are_preserved() {
        let client = FakeClient::new();
        client.edit(|state| {
            state.capabilities = Err(WorkdeckSessionCliClientError::Request(
                "owner-private credential store is unsafe".into(),
            ));
        });
        let error = runner(client, true)
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Json,
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("owner-private credential store is unsafe")
        );
    }

    #[test]
    fn compatible_daemon_missing_required_action_fails_before_dispatch() {
        let client = FakeClient::new();
        client.edit(|state| {
            state.capabilities = Ok(Some(SessionDaemonCapabilities {
                version: WORKDECK_SESSION_API_VERSION,
                daemon_version: WORKDECK_SESSION_DAEMON_VERSION,
                actions: vec![SessionDaemonAction::Get],
            }));
        });
        let error = runner(client.clone(), true)
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Json,
            })
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("missing required support for list")
        );
        assert!(client.calls().is_empty());
    }

    #[test]
    fn review_defaults_forward_no_patch_and_no_notes() {
        let client = FakeClient::new();
        let output = runner(client.clone(), true)
            .run(SessionCommandInput::Review {
                output: SessionCommandOutput::Json,
                selector: selector_input(),
                include_patch: false,
                include_notes: Some(false),
            })
            .unwrap();
        assert_eq!(
            json_output(&output)["review"]["files"][0].get("patch"),
            None
        );
        assert_eq!(
            client.calls(),
            [Call::Review(SessionReviewCliInput {
                selector: selector(),
                include_patch: false,
                include_notes: Some(false),
            })]
        );
    }

    #[test]
    fn review_can_request_raw_patch_text() {
        let client = FakeClient::new();
        client.edit(|state| state.review = review(true, false));
        let output = runner(client.clone(), true)
            .run(SessionCommandInput::Review {
                output: SessionCommandOutput::Json,
                selector: selector_input(),
                include_patch: true,
                include_notes: Some(false),
            })
            .unwrap();
        assert_eq!(
            json_output(&output)["review"]["files"][0]["patch"],
            "@@ -1,1 +1,2 @@"
        );
        assert!(matches!(
            client.calls().as_slice(),
            [Call::Review(SessionReviewCliInput {
                include_patch: true,
                ..
            })]
        ));
    }

    #[test]
    fn review_can_request_review_notes() {
        let client = FakeClient::new();
        client.edit(|state| state.review = review(false, true));
        let output = runner(client.clone(), true)
            .run(SessionCommandInput::Review {
                output: SessionCommandOutput::Json,
                selector: selector_input(),
                include_patch: false,
                include_notes: Some(true),
            })
            .unwrap();
        assert_eq!(json_output(&output)["review"]["reviewNoteCount"], 1);
        assert!(matches!(
            client.calls().as_slice(),
            [Call::Review(SessionReviewCliInput {
                include_notes: Some(true),
                ..
            })]
        ));
    }

    #[test]
    fn typed_comment_listing_routes_to_review_note_output() {
        let client = FakeClient::new();
        client.edit(|state| {
            state.comments = vec![crate::SessionCommentSummary::Review(review_note())]
        });
        let output = runner(client.clone(), true)
            .run(SessionCommandInput::CommentList {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: Some("README.md".into()),
                list_type: Some(SessionCommentListType::Source(ReviewNoteSource::User)),
            })
            .unwrap();
        assert!(output.contains("user:1  README.md [user]"));
        assert!(output.contains("body: Human note"));
        assert!(matches!(
            client.calls().as_slice(),
            [Call::CommentList(SessionCommentListCliInput {
                list_type: Some(DaemonCommentListType::User),
                ..
            })]
        ));
    }

    #[test]
    fn reload_returns_the_replacement_session_summary() {
        let client = FakeClient::new();
        let output = runner(client.clone(), true)
            .run(SessionCommandInput::Reload {
                output: SessionCommandOutput::Json,
                selector: selector_input(),
                next_input: Box::new(CliInput::Show(VcsShowCommandInput {
                    reference: Some("HEAD~1".into()),
                    pathspecs: Vec::new(),
                    options: CommonOptions::default(),
                })),
                source_path: None,
            })
            .unwrap();
        assert_eq!(json_output(&output)["result"]["title"], "repo show HEAD~1");
        assert!(matches!(
            client.calls().as_slice(),
            [Call::Reload(SessionReloadCliInput {
                next_input: DaemonCliInput::Show { reference: Some(reference), .. },
                ..
            })] if reference == "HEAD~1"
        ));
    }

    #[test]
    fn reload_forwards_structured_endpoints_and_separate_source_path() {
        let client = FakeClient::new();
        let input_path = std::env::current_dir().unwrap().join("live-session");
        let input = SessionCommandInput::Reload {
            output: SessionCommandOutput::Json,
            selector: SessionSelectorInput {
                session_path: Some(input_path.to_string_lossy().into_owned()),
                ..SessionSelectorInput::default()
            },
            next_input: Box::new(CliInput::Vcs(VcsDiffCommandInput {
                range: None,
                range_endpoints: Some(VcsRangeEndpoints {
                    from: "main".into(),
                    to: "feature".into(),
                }),
                staged: false,
                pathspecs: Vec::new(),
                options: CommonOptions::default(),
            })),
            source_path: Some("/source-repo".into()),
        };
        runner(client.clone(), true).run(input).unwrap();
        assert!(matches!(
            client.calls().as_slice(),
            [Call::Reload(SessionReloadCliInput {
                source_path: Some(source),
                next_input: DaemonCliInput::Vcs { range_endpoints: Some(DaemonRangeEndpoints { from, to }), .. },
                ..
            })] if source == "/source-repo" && from == "main" && to == "feature"
        ));
    }

    #[test]
    fn comment_apply_forwards_batch_and_formats_applied_result() {
        let client = FakeClient::new();
        client.edit(|state| state.comment_batch.applied[0].hunk_index = 1);
        let output = runner(client.clone(), true)
            .run(SessionCommandInput::CommentApply {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                comments: vec![SessionCommentApplyItemInput {
                    file_path: "README.md".into(),
                    hunk_number: Some(2),
                    side: None,
                    line: None,
                    summary: "Explain the hunk".into(),
                    rationale: None,
                    markup: None,
                    author: None,
                }],
                reveal_mode: RevealMode::First,
            })
            .unwrap();
        assert!(output.starts_with("Applied 1 live comments to session session-1:"));
        assert!(matches!(
            client.calls().as_slice(),
            [Call::CommentApply(SessionCommentApplyCliInput {
                reveal_mode: DaemonRevealMode::First,
                comments,
                ..
            })] if comments[0].hunk_number == Some(2)
        ));
    }

    #[test]
    fn daemon_with_required_action_runs_the_command() {
        let client = FakeClient::new();
        let output = runner(client.clone(), true)
            .run(SessionCommandInput::CommentList {
                output: SessionCommandOutput::Json,
                selector: selector_input(),
                file_path: None,
                list_type: None,
            })
            .unwrap();
        assert_eq!(json_output(&output), json!({"comments": []}));
        assert!(matches!(client.calls().as_slice(), [Call::CommentList(_)]));
    }

    #[test]
    fn reload_normalizes_session_path_before_client_dispatch() {
        let client = FakeClient::new();
        runner(client.clone(), true)
            .run(SessionCommandInput::Reload {
                output: SessionCommandOutput::Json,
                selector: SessionSelectorInput {
                    session_path: Some(".".into()),
                    ..SessionSelectorInput::default()
                },
                next_input: Box::new(CliInput::Vcs(VcsDiffCommandInput {
                    range: None,
                    range_endpoints: None,
                    staged: false,
                    pathspecs: Vec::new(),
                    options: CommonOptions::default(),
                })),
                source_path: None,
            })
            .unwrap();
        let expected = std::env::current_dir().unwrap();
        assert!(matches!(
            client.calls().as_slice(),
            [Call::Reload(SessionReloadCliInput { selector: SessionSelector { session_path: Some(path), .. }, .. })]
                if path == &expected
        ));
    }

    #[test]
    fn unavailable_daemon_list_returns_empty_without_creating_a_client() {
        let created = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = Arc::clone(&created);
        let runner = SessionCommandRunner::with_hooks(
            Arc::new(move || {
                observed.store(true, std::sync::atomic::Ordering::Release);
                Err(SessionCommandError::Message(
                    "must not create client".into(),
                ))
            }),
            Arc::new(|_| Ok(false)),
        );
        let output = runner
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Text,
            })
            .unwrap();
        assert_eq!(output, "No active Workdeck sessions.\n");
        assert!(!created.load(std::sync::atomic::Ordering::Acquire));
    }

    #[test]
    fn remaining_session_actions_dispatch_and_keep_text_output_stable() {
        let client = FakeClient::new();
        client.edit(|state| {
            state.comments = vec![crate::SessionCommentSummary::Live(live_comment())];
            state.removed.remaining_comment_count = 1;
            state.cleared = ClearedCommentsResult {
                removed_count: 2,
                remaining_comment_count: 0,
                file_path: Some("README.md".into()),
                include_user: None,
                removed_live_comment_count: None,
                removed_user_note_count: None,
                remaining_live_comment_count: None,
                remaining_user_note_count: None,
            };
        });
        let runner = runner(client.clone(), true);
        let navigate = runner
            .run(SessionCommandInput::Navigate {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: Some("README.md".into()),
                hunk_number: Some(1),
                side: None,
                line: None,
                comment_direction: None,
                comment_id: None,
            })
            .unwrap();
        assert_eq!(navigate, "Focused README.md hunk 1 in session session-1.\n");
        let listed = runner
            .run(SessionCommandInput::CommentList {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: None,
                list_type: None,
            })
            .unwrap();
        assert!(listed.contains("comment-1  README.md:2 (new)"));
        let removed = runner
            .run(SessionCommandInput::CommentRemove {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                comment_id: "comment-1".into(),
            })
            .unwrap();
        assert!(removed.contains("Remaining comments: 1"));
        let cleared = runner
            .run(SessionCommandInput::CommentClear {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: Some("README.md".into()),
                include_user: None,
                confirmed: true,
            })
            .unwrap();
        assert!(cleared.contains("Cleared 2 live comments from README.md"));
        assert_eq!(client.calls().len(), 4);
    }

    #[test]
    fn highlight_actions_dispatch_and_keep_text_output_stable() {
        let client = FakeClient::new();
        let runner = runner(client.clone(), true);
        let added = runner
            .run(SessionCommandInput::HighlightAdd {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: "README.md".into(),
                side: ReviewSide::New,
                line: 2,
                start: 4,
                end: 11,
                tone: Some(HighlightTone::Info),
                reveal: true,
            })
            .unwrap();
        assert_eq!(
            added,
            "Marked README.md:2 (new) [4, 11) as info in session session-1 and revealed its line. File marks: 1.\n"
        );
        let cleared = runner
            .run(SessionCommandInput::HighlightClear {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: None,
            })
            .unwrap();
        assert_eq!(
            cleared,
            "Cleared 1 attention marks from session session-1. Remaining marks: 0.\n"
        );
        assert!(matches!(
            client.calls().as_slice(),
            [
                Call::HighlightAdd(SessionHighlightAddCliInput {
                    tone: Some(SessionLineHighlightTone::Info),
                    ..
                }),
                Call::HighlightClear(SessionHighlightClearCliInput {
                    file_path: None,
                    ..
                })
            ]
        ));
    }

    fn terminal() -> SessionTerminalMetadata {
        SessionTerminalMetadata {
            program: Some("iTerm.app".into()),
            locations: vec![
                SessionTerminalLocation {
                    source: "tty".into(),
                    tty: Some("/dev/ttys003".into()),
                    window_id: None,
                    tab_id: None,
                    pane_id: None,
                    terminal_id: None,
                    session_id: None,
                },
                SessionTerminalLocation {
                    source: "tmux".into(),
                    tty: None,
                    window_id: None,
                    tab_id: None,
                    pane_id: Some("%2".into()),
                    terminal_id: None,
                    session_id: None,
                },
                SessionTerminalLocation {
                    source: "iterm2".into(),
                    tty: None,
                    window_id: Some("1".into()),
                    tab_id: Some("2".into()),
                    pane_id: Some("3".into()),
                    terminal_id: None,
                    session_id: None,
                },
            ],
        }
    }

    #[test]
    fn list_text_includes_generic_terminal_and_location_lines() {
        let client = FakeClient::new();
        client.edit(|state| state.sessions = vec![listed_session(Some(terminal()))]);
        let output = runner(client, true)
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Text,
            })
            .unwrap();
        assert!(output.contains("terminal: iTerm.app"));
        assert!(output.contains("location[tty]: /dev/ttys003"));
        assert!(output.contains("location[tmux]: pane %2"));
        assert!(output.contains("location[iterm2]: window 1, tab 2, pane 3"));
    }

    #[test]
    fn list_text_omits_terminal_lines_when_absent() {
        let output = runner(FakeClient::new(), true)
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Text,
            })
            .unwrap();
        assert!(!output.contains("terminal:"));
        assert!(!output.contains("location["));
    }

    #[test]
    fn get_text_includes_generic_terminal_location_lines() {
        let client = FakeClient::new();
        client.edit(|state| state.session = listed_session(Some(terminal())));
        let output = runner(client, true)
            .run(SessionCommandInput::Get {
                context: false,
                output: SessionCommandOutput::Text,
                selector: selector_input(),
            })
            .unwrap();
        assert!(output.contains("Terminal: iTerm.app"));
        assert!(output.contains("Location[tty]: /dev/ttys003"));
        assert!(output.contains("Location[tmux]: pane %2"));
    }

    #[test]
    fn list_json_includes_structured_terminal_metadata_without_legacy_fields() {
        let client = FakeClient::new();
        client.edit(|state| state.sessions = vec![listed_session(Some(terminal()))]);
        let output = runner(client, true)
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Json,
            })
            .unwrap();
        let session = &json_output(&output)["sessions"][0];
        assert_eq!(session["terminal"]["program"], "iTerm.app");
        assert!(session.get("tty").is_none());
        assert!(session.get("tmuxPane").is_none());
    }

    fn free_port() -> u16 {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.local_addr().unwrap().port()
    }

    fn isolated_env(root: &TempDir, port: u16) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("WORKDECK_MCP_PORT".into(), port.to_string()),
            (
                "XDG_RUNTIME_DIR".into(),
                root.path().to_string_lossy().into_owned(),
            ),
        ])
    }

    #[test]
    fn real_availability_probe_returns_empty_list_when_no_daemon_listens() {
        let root = TempDir::new().unwrap();
        let output = SessionCommandRunner::from_environment(isolated_env(&root, free_port()))
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Json,
            })
            .unwrap();
        assert_eq!(json_output(&output), json!({"sessions": []}));
        assert!(!root.path().join("workdeck-mcp").exists());
    }

    #[test]
    fn real_availability_probe_reports_no_active_sessions_for_non_list() {
        let root = TempDir::new().unwrap();
        let error = SessionCommandRunner::from_environment(isolated_env(&root, free_port()))
            .run(SessionCommandInput::Get {
                context: false,
                output: SessionCommandOutput::Json,
                selector: selector_input(),
            })
            .unwrap_err();
        assert!(error.to_string().contains("No active Workdeck sessions"));
    }

    #[test]
    fn real_availability_probe_reports_foreign_process_port_conflict() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut health, _) = listener.accept().unwrap();
            let mut request = [0_u8; 1_024];
            let _ = health.read(&mut request).unwrap();
            health
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope",
                )
                .unwrap();
            let _ = listener.accept().unwrap();
        });
        let root = TempDir::new().unwrap();
        let error = SessionCommandRunner::from_environment(isolated_env(&root, port))
            .run(SessionCommandInput::List {
                output: SessionCommandOutput::Json,
            })
            .unwrap_err();
        assert!(error.to_string().contains("already in use"));
        server.join().unwrap();
    }

    #[test]
    fn text_output_for_reload_comment_add_and_clear_is_nonempty() {
        let client = FakeClient::new();
        let runner = runner(client, true);
        let reload = runner
            .run(SessionCommandInput::Reload {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                next_input: Box::new(CliInput::Show(VcsShowCommandInput {
                    reference: Some("HEAD~1".into()),
                    pathspecs: Vec::new(),
                    options: CommonOptions::default(),
                })),
                source_path: None,
            })
            .unwrap();
        let added = runner
            .run(SessionCommandInput::CommentAdd {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: "README.md".into(),
                side: ReviewSide::New,
                line: 1,
                summary: "note".into(),
                rationale: None,
                markup: None,
                author: None,
                reveal: false,
            })
            .unwrap();
        let cleared = runner
            .run(SessionCommandInput::CommentClear {
                output: SessionCommandOutput::Text,
                selector: selector_input(),
                file_path: None,
                include_user: None,
                confirmed: true,
            })
            .unwrap();
        assert!(!reload.is_empty());
        assert!(!added.is_empty());
        assert!(!cleared.is_empty());
    }

    #[test]
    fn command_input_conversion_covers_direct_diff_without_losing_options() {
        let input = daemon_cli_input(CliInput::Files(FileCommandInput {
            left: "old".into(),
            right: "new".into(),
            options: CommonOptions {
                mode: Some(InputLayoutMode::Split),
                ..CommonOptions::default()
            },
        }));
        assert!(matches!(
            input,
            DaemonCliInput::Diff {
                left,
                right,
                options: DaemonCommonOptions {
                    mode: Some(DaemonLayoutMode::Split),
                    ..
                }
            } if left == "old" && right == "new"
        ));
    }

    #[test]
    fn health_parser_accepts_minimal_and_bounded_rich_shapes_only() {
        assert!(crate::parse_session_broker_health(&json!({"ok": true})).is_some());
        assert!(
            crate::parse_session_broker_health(&json!({
                "ok": true,
                "pid": 42,
                "paths": {"health": "/health", "socket": "/session", "api": "/session-api"}
            }))
            .is_some()
        );
        assert!(
            crate::parse_session_broker_health(&json!({"ok": true, "unknown": true})).is_none()
        );
        assert!(crate::parse_session_broker_health(&json!({"ok": true, "pid": -1})).is_none());
        assert!(
            crate::parse_session_broker_health(
                &json!({"ok": true, "paths": {"health": "/health"}})
            )
            .is_none()
        );
    }
}
