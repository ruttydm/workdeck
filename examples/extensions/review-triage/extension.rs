//! Compiled native port of Hunk's session-local review triage board.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, BufRead, Write};
use workdeck_core::Changeset;
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ConfirmDialogSubmission, ExtensionDiffFile, ExtensionHostAction, ExtensionNotifyType,
    HandshakeResponse, InputDialogSubmission, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    PaneActionInvocation, PanePlacement, PaneRegistration, PaneRenderRequest, PaneRenderResponse,
    Registration, ReviewEvent, SelectDialogSubmission, ViewNode, ViewStyle,
};

const PANE_ID: &str = "triage";
const STATUS_DIALOG_ID: &str = "triage-status";
const RATIONALE_DIALOG_ID: &str = "triage-rationale";
const FOCUS_DIALOG_ID: &str = "triage-focus";
const CLEAR_DIALOG_ID: &str = "triage-clear";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriageStatus {
    Approved,
    Investigate,
    Blocked,
}

impl TriageStatus {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "approved" => Some(Self::Approved),
            "investigate" => Some(Self::Investigate),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Investigate => "investigate",
            Self::Blocked => "blocked",
        }
    }

    const fn marker(self) -> &'static str {
        match self {
            Self::Approved => "✓",
            Self::Investigate => "!",
            Self::Blocked => "×",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageDecision {
    pub status: TriageStatus,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingDecision {
    file_id: String,
    path: String,
    hunk_index: usize,
    status: Option<TriageStatus>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PaneAction {
    SelectFile { file_id: String },
    SelectHunk { file_id: String, hunk_index: usize },
}

#[derive(Debug, Default)]
pub struct ReviewTriageState {
    pub decisions: BTreeMap<String, TriageDecision>,
    pub viewed: BTreeSet<String>,
    pub note_counts: BTreeMap<String, usize>,
    pub current: Option<(String, usize)>,
    pub focus: String,
    pub filter: String,
    pub reload_pending: bool,
    pane_actions: BTreeMap<String, PaneAction>,
    pending_decision: Option<PendingDecision>,
}

#[must_use]
pub fn hunk_key(file_id: &str, hunk_index: usize) -> String {
    format!("{file_id}:{hunk_index}")
}

pub fn reconcile_changeset(state: &mut ReviewTriageState, changeset: &Changeset) {
    let known_hunks = changeset
        .files
        .iter()
        .flat_map(|file| {
            file.hunks
                .iter()
                .map(|hunk| hunk_key(&file.runtime_id, hunk.index))
        })
        .collect::<BTreeSet<_>>();
    state.decisions.retain(|key, _| known_hunks.contains(key));
    state.viewed.retain(|key| known_hunks.contains(key));
    state.note_counts.retain(|key, _| known_hunks.contains(key));
    if state
        .current
        .as_ref()
        .is_some_and(|(file_id, hunk_index)| !known_hunks.contains(&hunk_key(file_id, *hunk_index)))
    {
        state.current = None;
    }
    if state.pending_decision.as_ref().is_some_and(|pending| {
        !known_hunks.contains(&hunk_key(&pending.file_id, pending.hunk_index))
    }) {
        state.pending_decision = None;
    }
    state.reload_pending = false;
}

fn mark_viewed(state: &mut ReviewTriageState, file_id: &str, hunk_index: Option<usize>) {
    if let Some(hunk_index) = hunk_index {
        state.viewed.insert(hunk_key(file_id, hunk_index));
    }
}

fn record_note(state: &mut ReviewTriageState, file_id: &str, hunk_index: Option<usize>) {
    if let Some(hunk_index) = hunk_index {
        *state
            .note_counts
            .entry(hunk_key(file_id, hunk_index))
            .or_default() += 1;
    }
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::Pane(PaneRegistration {
            id: PANE_ID.into(),
            title: "Review triage".into(),
            placement: PanePlacement::Right,
            default_open: false,
            preferred_size: None,
            width: None,
            height: None,
            replaces: None,
            current_line: false,
            available: false,
        }),
        command("toggle", "Toggle review triage", &["y"]),
        command("center", "Center current review line", &[]),
        command("mark", "Mark selected hunk…", &["x"]),
        command("focus", "Set review focus…", &[]),
        command("clear", "Clear triage decisions", &[]),
        Registration::EventSubscription {
            names: vec![
                "changeset_loaded".into(),
                "session_reload".into(),
                "selection_changed".into(),
                "hunk_viewed".into(),
                "note_created".into(),
                "filter_changed".into(),
                "watch_reload_pending".into(),
                "review-triage:open".into(),
            ],
        },
    ]
}

fn command(id: &str, title: &str, default_keys: &[&str]) -> Registration {
    Registration::Command(CommandRegistration {
        id: id.into(),
        title: title.into(),
        description: None,
        default_keys: default_keys.iter().map(|key| (*key).into()).collect(),
    })
}

fn pane_refresh() -> ExtensionHostAction {
    ExtensionHostAction::RefreshPane { id: PANE_ID.into() }
}

fn notify(
    message: impl Into<String>,
    notification_type: ExtensionNotifyType,
) -> ExtensionHostAction {
    ExtensionHostAction::Notify {
        message: message.into(),
        notification_type,
    }
}

fn selected_hunk(invocation: &CommandInvocation) -> Option<(&ExtensionDiffFile, usize)> {
    let hunk_index = invocation.selection.hunk_index?;
    let file = invocation.selection.file.as_ref()?;
    file.hunks.get(hunk_index)?;
    Some((file, hunk_index))
}

pub fn invoke_command(
    invocation: &CommandInvocation,
    state: &mut ReviewTriageState,
) -> Result<CommandExecution, String> {
    let actions = match invocation.command_id.as_str() {
        "toggle" => {
            let is_open = invocation
                .open_panes
                .iter()
                .any(|pane| pane == PANE_ID || pane.ends_with(":triage"));
            vec![if is_open {
                ExtensionHostAction::ClosePane { id: PANE_ID.into() }
            } else {
                ExtensionHostAction::OpenPane { id: PANE_ID.into() }
            }]
        }
        "center" => vec![
            invocation
                .commands
                .execute("workdeck.review.align-current-line-center", None)
                .expect("the built-in command id and count are valid")
                .unwrap_or_else(|| {
                    notify(
                        "Enable the current-line marker before centering it",
                        ExtensionNotifyType::Warning,
                    )
                }),
        ],
        "mark" => {
            let Some((file, hunk_index)) = selected_hunk(invocation) else {
                return Ok(CommandExecution {
                    actions: vec![notify(
                        "Select a hunk before triaging it",
                        ExtensionNotifyType::Warning,
                    )],
                });
            };
            state.pending_decision = Some(PendingDecision {
                file_id: file.id.clone(),
                path: file.path.clone(),
                hunk_index,
                status: None,
            });
            vec![ExtensionHostAction::OpenSelectDialog {
                id: STATUS_DIALOG_ID.into(),
                title: format!("Triage {}, hunk {}", file.path, hunk_index + 1),
                options: vec!["approved".into(), "investigate".into(), "blocked".into()],
            }]
        }
        "focus" => vec![ExtensionHostAction::OpenInputDialog {
            id: FOCUS_DIALOG_ID.into(),
            title: "Review focus".into(),
            placeholder: "What are you looking for in this changeset?".into(),
            initial: Some(state.focus.clone()),
        }],
        "clear" => vec![ExtensionHostAction::OpenConfirmDialog {
            id: CLEAR_DIALOG_ID.into(),
            title: "Clear review triage?".into(),
            body: "This only clears this extension's session-local decisions.".into(),
            confirm_label: "clear".into(),
            cancel_label: Some("keep".into()),
        }],
        command => return Err(format!("Unknown command: {command}")),
    };
    Ok(CommandExecution { actions })
}

pub fn submit_select(
    submission: &SelectDialogSubmission,
    state: &mut ReviewTriageState,
) -> CommandExecution {
    if submission.action_id != STATUS_DIALOG_ID {
        return CommandExecution::default();
    }
    let Some(selected) = submission.value.as_deref() else {
        state.pending_decision = None;
        return CommandExecution::default();
    };
    let Some(status) = TriageStatus::parse(selected) else {
        state.pending_decision = None;
        return CommandExecution::default();
    };
    let Some(pending) = &mut state.pending_decision else {
        return CommandExecution::default();
    };
    pending.status = Some(status);
    CommandExecution {
        actions: vec![ExtensionHostAction::OpenInputDialog {
            id: RATIONALE_DIALOG_ID.into(),
            title: format!("{}: optional rationale", status.as_str()),
            placeholder: "Why should a reviewer care?".into(),
            initial: None,
        }],
    }
}

pub fn submit_input(
    submission: &InputDialogSubmission,
    state: &mut ReviewTriageState,
) -> CommandExecution {
    match submission.action_id.as_str() {
        FOCUS_DIALOG_ID => {
            let Some(value) = submission.value.as_deref() else {
                return CommandExecution::default();
            };
            state.focus = value.trim().into();
            CommandExecution {
                actions: vec![
                    pane_refresh(),
                    ExtensionHostAction::OpenPane { id: PANE_ID.into() },
                ],
            }
        }
        RATIONALE_DIALOG_ID => {
            let pending = state.pending_decision.take();
            let Some(value) = submission.value.as_deref() else {
                return CommandExecution::default();
            };
            let Some(pending) = pending else {
                return CommandExecution::default();
            };
            let Some(status) = pending.status else {
                return CommandExecution::default();
            };
            let key = hunk_key(&pending.file_id, pending.hunk_index);
            let rationale = value.trim();
            state.decisions.insert(
                key,
                TriageDecision {
                    status,
                    rationale: (!rationale.is_empty()).then(|| rationale.into()),
                },
            );
            CommandExecution {
                actions: vec![
                    pane_refresh(),
                    ExtensionHostAction::EmitEvent {
                        name: "review-triage:decision".into(),
                        payload: serde_json::json!({
                            "fileId": pending.file_id,
                            "hunkIndex": pending.hunk_index,
                            "status": status.as_str(),
                        }),
                    },
                    notify(
                        format!("Marked hunk {} {}", pending.hunk_index + 1, status.as_str()),
                        ExtensionNotifyType::Info,
                    ),
                ],
            }
        }
        _ => CommandExecution::default(),
    }
}

pub fn submit_confirm(
    submission: &ConfirmDialogSubmission,
    state: &mut ReviewTriageState,
) -> CommandExecution {
    if submission.action_id != CLEAR_DIALOG_ID || !submission.confirmed {
        return CommandExecution::default();
    }
    state.decisions.clear();
    CommandExecution {
        actions: vec![
            pane_refresh(),
            notify("Cleared review triage decisions", ExtensionNotifyType::Info),
        ],
    }
}

pub fn handle_event(event: &ReviewEvent, state: &mut ReviewTriageState) -> CommandExecution {
    let changed = match event.name.as_str() {
        "changeset_loaded" | "session_reload" => {
            reconcile_changeset(state, &event.snapshot.changeset);
            true
        }
        "selection_changed" => {
            state.current = event_file_and_hunk(&event.payload)
                .and_then(|(file_id, hunk_index)| hunk_index.map(|index| (file_id, index)));
            true
        }
        "hunk_viewed" => {
            if let Some((file_id, hunk_index)) = event_file_and_hunk(&event.payload) {
                mark_viewed(state, &file_id, hunk_index);
                true
            } else {
                false
            }
        }
        "note_created" => {
            if let Some((file_id, hunk_index)) = event_file_and_hunk(&event.payload) {
                record_note(state, &file_id, hunk_index);
                true
            } else {
                false
            }
        }
        "filter_changed" => {
            state.filter = event
                .payload
                .get("filter")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into();
            true
        }
        "watch_reload_pending" => {
            state.reload_pending = true;
            true
        }
        "review-triage:open" => {
            return CommandExecution {
                actions: vec![if event.context.panes.is_open(PANE_ID) {
                    ExtensionHostAction::ClosePane { id: PANE_ID.into() }
                } else {
                    ExtensionHostAction::OpenPane { id: PANE_ID.into() }
                }],
            };
        }
        _ => false,
    };
    CommandExecution {
        actions: changed.then(pane_refresh).into_iter().collect(),
    }
}

fn event_file_and_hunk(payload: &Value) -> Option<(String, Option<usize>)> {
    let file_id = payload.get("fileId")?.as_str()?.to_owned();
    let hunk_index = payload
        .get("hunkIndex")
        .and_then(Value::as_u64)
        .and_then(|value| usize::try_from(value).ok());
    Some((file_id, hunk_index))
}

pub fn invoke_pane_action(
    invocation: &PaneActionInvocation,
    state: &ReviewTriageState,
) -> CommandExecution {
    if invocation.pane_id != PANE_ID {
        return CommandExecution::default();
    }
    let action = state.pane_actions.get(&invocation.action_id);
    let action = match action {
        Some(PaneAction::SelectFile { file_id }) => ExtensionHostAction::SelectReviewFile {
            file_id: file_id.clone(),
        },
        Some(PaneAction::SelectHunk {
            file_id,
            hunk_index,
        }) => ExtensionHostAction::SelectReviewHunk {
            file_id: file_id.clone(),
            hunk_index: *hunk_index,
        },
        None => return CommandExecution::default(),
    };
    CommandExecution {
        actions: vec![action],
    }
}

pub fn render_pane(
    request: &PaneRenderRequest,
    state: &mut ReviewTriageState,
) -> Result<PaneRenderResponse, String> {
    if request.pane_id != PANE_ID {
        return Err(format!("Unknown pane: {}", request.pane_id));
    }
    state.pane_actions.clear();
    let files = &request.snapshot.changeset.files;
    let total = files.iter().map(|file| file.hunks.len()).sum::<usize>();
    let approved = state
        .decisions
        .values()
        .filter(|decision| decision.status == TriageStatus::Approved)
        .count();
    let investigate = state
        .decisions
        .values()
        .filter(|decision| decision.status == TriageStatus::Investigate)
        .count();
    let blocked = state
        .decisions
        .values()
        .filter(|decision| decision.status == TriageStatus::Blocked)
        .count();
    let mut children = vec![
        text(
            " Review triage (session only)",
            &request.theme.accent,
            &request.theme.panel,
        ),
        text(
            format!(" {approved}/{total} reviewed · !{investigate} · ×{blocked}"),
            &request.theme.muted,
            &request.theme.panel,
        ),
    ];
    if state.reload_pending {
        children.push(text(
            " Reload pending…",
            &request.theme.accent_muted,
            &request.theme.panel,
        ));
    }
    if !state.focus.is_empty() {
        children.push(text(
            format!(" Focus: {}", state.focus),
            &request.theme.muted,
            &request.theme.panel,
        ));
    }
    if !state.filter.is_empty() {
        children.push(text(
            format!(" Filter: {}", state.filter),
            &request.theme.muted,
            &request.theme.panel,
        ));
    }
    for (file_index, file) in files.iter().enumerate() {
        let file_action = format!("file:{file_index}");
        state.pane_actions.insert(
            file_action.clone(),
            PaneAction::SelectFile {
                file_id: file.runtime_id.clone(),
            },
        );
        children.push(ViewNode::Action {
            id: file_action,
            child: Box::new(text(
                format!(" {} ({})", file.path, file.hunks.len()),
                &request.theme.text,
                &request.theme.panel,
            )),
        });
        for hunk in &file.hunks {
            let key = hunk_key(&file.runtime_id, hunk.index);
            let decision = state.decisions.get(&key);
            let note_count = state.note_counts.get(&key).copied().unwrap_or_default();
            let selected = request.snapshot.selection.file_index == file_index
                && request.snapshot.selection.hunk_index == Some(hunk.index);
            let marker = decision.map_or_else(
                || {
                    if state.viewed.contains(&key) {
                        "·"
                    } else {
                        "○"
                    }
                },
                |decision| decision.status.marker(),
            );
            let notes = if note_count == 0 {
                String::new()
            } else {
                format!(
                    " [{note_count} note{}]",
                    if note_count == 1 { "" } else { "s" }
                )
            };
            let rationale = decision
                .and_then(|decision| decision.rationale.as_deref())
                .map_or_else(String::new, |rationale| format!(" — {rationale}"));
            let foreground = match decision.map(|decision| decision.status) {
                Some(TriageStatus::Blocked) => &request.theme.badge_removed,
                Some(TriageStatus::Investigate) => &request.theme.accent,
                _ => &request.theme.text,
            };
            let background = if selected {
                &request.theme.selected_hunk
            } else {
                &request.theme.panel
            };
            let action_id = format!("hunk:{file_index}:{}", hunk.index);
            state.pane_actions.insert(
                action_id.clone(),
                PaneAction::SelectHunk {
                    file_id: file.runtime_id.clone(),
                    hunk_index: hunk.index,
                },
            );
            children.push(ViewNode::Action {
                id: action_id,
                child: Box::new(text(
                    format!("   {marker} hunk {}{notes}{rationale}", hunk.index + 1),
                    foreground,
                    background,
                )),
            });
        }
    }
    Ok(PaneRenderResponse {
        content: ViewNode::Column { children, gap: 0 },
    })
}

fn text(text: impl Into<String>, foreground: &str, background: &str) -> ViewNode {
    ViewNode::Text {
        text: text.into(),
        style: ViewStyle {
            foreground: Some(foreground.into()),
            background: Some(background.into()),
            ..ViewStyle::default()
        },
    }
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut state = ReviewTriageState::default();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if value.get("id").is_none() {
            // Notifications have no response. The host may revoke this process with a shutdown
            // notification even when a deliberately uncooperative test runtime does not exit.
            continue;
        }
        let request: JsonRpcRequest = serde_json::from_value(value).map_err(io::Error::other)?;
        let result = dispatch_request(&request.method, request.params, &mut state);
        let response = match result {
            Ok(result) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: Some(result),
                error: None,
            },
            Err(error) => JsonRpcResponse {
                jsonrpc: "2.0".into(),
                id: request.id,
                result: None,
                error: Some(JsonRpcError {
                    code: -32602,
                    message: error,
                    data: None,
                }),
            },
        };
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

fn dispatch_request(
    method: &str,
    params: Value,
    state: &mut ReviewTriageState,
) -> Result<Value, String> {
    match method {
        "workdeck/handshake" => {
            apply_loader_fixture_delay()?;
            serde_json::to_value(HandshakeResponse {
                extension_api_version: API_VERSION,
                extension_version: env!("CARGO_PKG_VERSION").into(),
                registrations: loader_fixture_registrations()?,
            })
            .map_err(|error| error.to_string())
        }
        "workdeck/command/invoke" => {
            let invocation = serde_json::from_value(params).map_err(|error| error.to_string())?;
            serde_json::to_value(invoke_command(&invocation, state)?)
                .map_err(|error| error.to_string())
        }
        "workdeck/dialog/select" => {
            let submission = serde_json::from_value(params).map_err(|error| error.to_string())?;
            serde_json::to_value(submit_select(&submission, state))
                .map_err(|error| error.to_string())
        }
        "workdeck/dialog/input" => {
            let submission = serde_json::from_value(params).map_err(|error| error.to_string())?;
            serde_json::to_value(submit_input(&submission, state))
                .map_err(|error| error.to_string())
        }
        "workdeck/dialog/confirm" => {
            let submission = serde_json::from_value(params).map_err(|error| error.to_string())?;
            serde_json::to_value(submit_confirm(&submission, state))
                .map_err(|error| error.to_string())
        }
        "workdeck/pane/render" => {
            let request = serde_json::from_value(params).map_err(|error| error.to_string())?;
            serde_json::to_value(render_pane(&request, state)?).map_err(|error| error.to_string())
        }
        "workdeck/pane/action" => {
            let invocation = serde_json::from_value(params).map_err(|error| error.to_string())?;
            serde_json::to_value(invoke_pane_action(&invocation, state))
                .map_err(|error| error.to_string())
        }
        "workdeck/event" => {
            let event = serde_json::from_value(params).map_err(|error| error.to_string())?;
            serde_json::to_value(handle_event(&event, state)).map_err(|error| error.to_string())
        }
        method => Err(format!("Unknown method: {method}")),
    }
}

/// Integration-fixture seam proving that the host awaits a handshake and starts the child in the
/// manifest directory. Ordinary examples never contain this sentinel file.
fn apply_loader_fixture_delay() -> Result<(), String> {
    let path = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join(".workdeck-test-handshake-delay-ms");
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let milliseconds = source
        .trim()
        .parse::<u64>()
        .map_err(|error| error.to_string())?
        .min(1_000);
    std::thread::sleep(std::time::Duration::from_millis(milliseconds));
    Ok(())
}

/// Return the ordinary declarations unless a copied integration fixture asks for a deliberately
/// invalid final declaration. This proves the host never publishes the valid prefix of a failed
/// native factory-equivalent handshake.
fn loader_fixture_registrations() -> Result<Vec<Registration>, String> {
    let path = std::env::current_dir()
        .map_err(|error| error.to_string())?
        .join(".workdeck-test-invalid-registration");
    let mut values = registrations();
    if path.exists() {
        values.push(Registration::Command(CommandRegistration {
            id: "invalid-tail".into(),
            title: " ".into(),
            description: None,
            default_keys: Vec::new(),
        }));
    }
    Ok(values)
}

#[must_use]
pub fn required_capabilities() -> Vec<Capability> {
    vec![
        Capability::Commands,
        Capability::Panes,
        Capability::Events,
        Capability::Dialogs,
        Capability::Notifications,
        Capability::ReviewNavigation,
    ]
}
