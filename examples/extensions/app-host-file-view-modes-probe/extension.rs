//! Native subprocess oracle for Hunk's AppHost interactive file-view mode tests.

use std::io::{self, BufRead, Write};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration, ConfirmDialogSubmission,
    ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewSpan, ExtensionHostAction, ExtensionNotifyType, FileViewLayoutRequest,
    FileViewMatchRequest, FileViewModeKeyRequest, FileViewModeLifecycleExecution,
    FileViewModeLifecycleRequest, HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    KeyRoutingResult, KeyboardModeExecution, Registration,
};

pub const EXTENSION_ID: &str = "app-host-file-view-modes-probe";
const ALPHA: &str = "alpha";
const BETA: &str = "beta";
const PLAIN: &str = "plain";
const PICKY: &str = "picky";
const BOOM: &str = "boom";

#[derive(Debug, Default)]
struct State {
    cursor: usize,
    events: Vec<String>,
}

impl State {
    fn record(&mut self, event: impl Into<String>) {
        self.events.push(event.into());
        if self.events.len() > 16 {
            self.events.remove(0);
        }
    }
}

fn command(id: &str, title: &str, key: &str) -> Registration {
    Registration::Command(CommandRegistration {
        id: id.into(),
        title: title.into(),
        description: None,
        default_keys: vec![key.into()],
    })
}

fn file_view(id: &str, interactive_mode: bool) -> Registration {
    Registration::FileView {
        id: id.into(),
        title: format!("{} view", id.to_ascii_uppercase()),
        priority: 0,
        interactive_mode,
    }
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        file_view(ALPHA, true),
        file_view(BETA, true),
        file_view(PLAIN, false),
        file_view(PICKY, true),
        file_view(BOOM, true),
        command("toggle-alpha", "Toggle alpha", "f8"),
        command("enter-alpha", "Enter alpha mode", "f9"),
        command("enter-beta", "Enter beta mode", "f7"),
        command("enter-missing", "Enter unknown mode", "f6"),
        command("enter-picky", "Enter picky mode", "f4"),
        command("enter-plain", "Enter plain mode", "f3"),
        command("leave", "Leave mode twice", "f5"),
        command("deferred-enter", "Deferred enter", "f2"),
        command("enter-boom", "Enter broken mode", "f1"),
        command("answered", "Answered command", "n"),
        command("declined", "Declined command", "p"),
        command("escaped", "Escaped command", "escape"),
    ]
}

fn refresh(view_id: &str) -> ExtensionHostAction {
    ExtensionHostAction::RefreshFileView {
        id: view_id.into(),
        file_id: None,
    }
}

fn refresh_every_view() -> Vec<ExtensionHostAction> {
    [ALPHA, BETA, PLAIN, PICKY, BOOM]
        .into_iter()
        .map(refresh)
        .collect()
}

fn notify(message: impl Into<String>) -> ExtensionHostAction {
    ExtensionHostAction::Notify {
        message: message.into(),
        notification_type: ExtensionNotifyType::Info,
    }
}

fn invoke_command(
    invocation: CommandInvocation,
    state: &mut State,
) -> io::Result<CommandExecution> {
    let actions = match invocation.command_id.as_str() {
        "toggle-alpha" => vec![ExtensionHostAction::ToggleFileView { id: ALPHA.into() }],
        "enter-alpha" => vec![ExtensionHostAction::EnterFileViewMode { id: ALPHA.into() }],
        "enter-beta" => vec![ExtensionHostAction::EnterFileViewMode { id: BETA.into() }],
        "enter-missing" => vec![ExtensionHostAction::EnterFileViewMode {
            id: "not-a-view".into(),
        }],
        "enter-picky" => vec![ExtensionHostAction::EnterFileViewMode { id: PICKY.into() }],
        "enter-plain" => vec![ExtensionHostAction::EnterFileViewMode { id: PLAIN.into() }],
        "enter-boom" => vec![ExtensionHostAction::EnterFileViewMode { id: BOOM.into() }],
        "leave" => vec![
            ExtensionHostAction::ExitFileViewMode,
            ExtensionHostAction::ExitFileViewMode,
        ],
        "deferred-enter" => vec![ExtensionHostAction::OpenConfirmDialog {
            id: "deferred-enter".into(),
            title: "Ready to edit?".into(),
            body: String::new(),
            confirm_label: "Confirm".into(),
            cancel_label: None,
        }],
        "answered" | "declined" | "escaped" => {
            let event = match invocation.command_id.as_str() {
                "answered" => "COMMAND N RAN",
                "declined" => "COMMAND P RAN",
                "escaped" => "ESCAPE COMMAND RAN",
                _ => unreachable!(),
            };
            state.record(event);
            let mut actions = refresh_every_view();
            actions.push(notify(event));
            actions
        }
        id => return Err(io::Error::other(format!("Unknown command: {id}"))),
    };
    Ok(CommandExecution { actions })
}

fn submit_confirm(
    submission: ConfirmDialogSubmission,
    state: &mut State,
) -> io::Result<CommandExecution> {
    if submission.action_id != "deferred-enter" {
        return Err(io::Error::other(format!(
            "Unknown confirmation: {}",
            submission.action_id
        )));
    }
    state.record(format!("DIALOG ANSWER {}", submission.confirmed));
    state.record("LATE RESULT true");
    Ok(CommandExecution {
        actions: vec![
            notify(format!("DIALOG ANSWER {}", submission.confirmed)),
            ExtensionHostAction::EnterFileViewMode { id: ALPHA.into() },
            notify("LATE RESULT true"),
        ],
    })
}

fn matches(request: FileViewMatchRequest) -> io::Result<bool> {
    match request.view_id.as_str() {
        PICKY => Ok(false),
        ALPHA | BETA | PLAIN | BOOM => Ok(true),
        id => Err(io::Error::other(format!("Unknown file view: {id}"))),
    }
}

fn layout(request: FileViewLayoutRequest, state: &State) -> io::Result<ExtensionFileViewLayout> {
    if !matches!(
        request.view_id.as_str(),
        ALPHA | BETA | PLAIN | PICKY | BOOM
    ) {
        return Err(io::Error::other(format!(
            "Unknown file view: {}",
            request.view_id
        )));
    }
    let mut labels = vec![
        format!("{} VIEW", request.view_id.to_ascii_uppercase()),
        format!("CURSOR {}", state.cursor),
        format!("FILE RELOADED {}", request.file.patch.contains("reloaded")),
    ];
    labels.extend(
        state
            .events
            .iter()
            .enumerate()
            .map(|(index, event)| format!("EVENT {index:02} {event}")),
    );
    let rows = labels
        .into_iter()
        .enumerate()
        .map(|(index, text)| ExtensionFileViewRow {
            id: format!("row:{index}"),
            spans: vec![ExtensionFileViewSpan {
                text,
                tone: None,
                attributes: Vec::new(),
            }],
            source_ranges: Vec::new(),
            component: None,
        })
        .collect::<Vec<_>>();
    Ok(ExtensionFileViewLayout {
        rows,
        hunk_rows: request
            .file
            .hunks
            .iter()
            .map(|_| ExtensionFileViewHunkRows {
                start_row: 0,
                end_row: 0,
            })
            .collect(),
    })
}

fn lifecycle(
    request: FileViewModeLifecycleRequest,
    entering: bool,
    state: &mut State,
) -> FileViewModeLifecycleExecution {
    let event = if entering {
        format!(
            "ENTER {} {} RELOADED {}",
            request.view_id,
            request.file.path,
            request.file.patch.contains("reloaded")
        )
    } else if request.view_id == BOOM {
        format!("BOOM EXIT {}", request.file.path)
    } else {
        format!("EXIT {} {}", request.view_id, request.file.path)
    };
    state.record(event);
    FileViewModeLifecycleExecution {
        actions: vec![refresh(&request.view_id)],
        failure: None,
    }
}

fn route_mode_key(
    request: FileViewModeKeyRequest,
    state: &mut State,
) -> io::Result<KeyboardModeExecution> {
    if request.view_id == BOOM {
        return Err(io::Error::other("key handler exploded"));
    }
    let key = request.key.name.as_str();
    state.record(format!("KEY {} {key}", request.view_id));
    let (result, actions) = match (request.view_id.as_str(), key) {
        (ALPHA, "j") => {
            state.cursor = state.cursor.saturating_add(1);
            (KeyRoutingResult::Handled, vec![refresh(ALPHA)])
        }
        (ALPHA | BETA, "n") => (KeyRoutingResult::Handled, vec![refresh(&request.view_id)]),
        (ALPHA | BETA, "x") => (KeyRoutingResult::Exit, vec![refresh(&request.view_id)]),
        (ALPHA, "r") => (
            KeyRoutingResult::Exit,
            vec![ExtensionHostAction::EnterFileViewMode { id: BETA.into() }],
        ),
        _ => (KeyRoutingResult::Pass, Vec::new()),
    };
    Ok(KeyboardModeExecution { result, actions })
}

fn value(value: impl Serialize) -> io::Result<Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}

fn parse<T: DeserializeOwned>(request: &JsonRpcRequest) -> io::Result<T> {
    serde_json::from_value(request.params.clone()).map_err(io::Error::other)
}

fn write_response(output: &mut impl Write, id: u64, result: io::Result<Value>) -> io::Result<()> {
    let response = match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32_000,
                message: error.to_string(),
                data: None,
            }),
        },
    };
    serde_json::to_writer(&mut *output, &response).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

/// Serve the stateful file-view mode fixture over JSON-RPC 2.0 JSONL.
pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut state = State::default();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => value(HandshakeResponse {
                extension_api_version: API_VERSION,
                extension_version: env!("CARGO_PKG_VERSION").into(),
                registrations: registrations(),
            }),
            "workdeck/command/invoke" => parse(&request)
                .and_then(|invocation| invoke_command(invocation, &mut state))
                .and_then(value),
            "workdeck/dialog/confirm" => parse(&request)
                .and_then(|submission| submit_confirm(submission, &mut state))
                .and_then(value),
            "workdeck/file-view/matches" => parse(&request).and_then(matches).and_then(value),
            "workdeck/file-view/layout" => parse(&request)
                .and_then(|request| layout(request, &state))
                .map(Some)
                .and_then(value),
            "workdeck/file-view-mode/enter" => parse(&request)
                .map(|request| lifecycle(request, true, &mut state))
                .and_then(value),
            "workdeck/file-view-mode/exit" => parse(&request)
                .map(|request| lifecycle(request, false, &mut state))
                .and_then(value),
            "workdeck/file-view-mode/key" => parse(&request)
                .and_then(|request| route_mode_key(request, &mut state))
                .and_then(value),
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        write_response(&mut output, request.id, result)?;
    }
}
