//! Compiled native parity fixture for Hunk's nested keyboard-mode tests.

use std::io::{self, BufRead, Write};

use serde::Serialize;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewSpan, ExtensionHostAction, ExtensionKeyEvent, ExtensionNotifyType,
    FileViewLayoutRequest, FileViewMatchRequest, FileViewModeKeyRequest,
    FileViewModeLifecycleExecution, FileViewModeLifecycleRequest, HandshakeResponse, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, KeyRoutingResult, KeyboardModeExecution,
    KeyboardModeKeyRequest, KeyboardModeLifecycleRequest, KeyboardModeRegistration, Registration,
};

pub const EXTENSION_ID: &str = "test.keyboard-probe";
pub const MODE_ID: &str = "normal";
pub const VIEW_ID: &str = "focused";
pub const MODE_TITLE: &str = "Probe normal";

fn notification(message: impl Into<String>) -> ExtensionHostAction {
    ExtensionHostAction::Notify {
        message: message.into(),
        notification_type: ExtensionNotifyType::Info,
    }
}

fn pressed(key: &ExtensionKeyEvent) -> &str {
    if key.sequence.is_empty() {
        key.name.as_str()
    } else {
        key.sequence.as_str()
    }
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::KeyboardMode(KeyboardModeRegistration {
            id: MODE_ID.into(),
            title: MODE_TITLE.into(),
        }),
        Registration::FileView {
            id: VIEW_ID.into(),
            title: "Focused".into(),
            priority: 0,
            interactive_mode: true,
        },
        Registration::Command(CommandRegistration {
            id: "session".into(),
            title: "Toggle probe mode".into(),
            description: None,
            default_keys: vec!["f8".into()],
        }),
        Registration::Command(CommandRegistration {
            id: "file".into(),
            title: "Enter focused view".into(),
            description: None,
            default_keys: vec!["f9".into()],
        }),
        Registration::Command(CommandRegistration {
            id: "passed".into(),
            title: "Passed command".into(),
            description: None,
            default_keys: vec!["p".into()],
        }),
    ]
}

fn invoke(invocation: CommandInvocation) -> io::Result<CommandExecution> {
    let actions = match invocation.command_id.as_str() {
        "session"
            if invocation.active_keyboard_mode.as_deref() == Some("test.keyboard-probe:normal") =>
        {
            vec![ExtensionHostAction::ExitKeyboardMode]
        }
        "session" => vec![ExtensionHostAction::EnterKeyboardMode { id: MODE_ID.into() }],
        "file" => vec![ExtensionHostAction::EnterFileViewMode { id: VIEW_ID.into() }],
        "passed" => vec![notification("COMMAND P")],
        id => return Err(io::Error::other(format!("Unknown command: {id}"))),
    };
    Ok(CommandExecution { actions })
}

fn session_lifecycle(
    method: &str,
    request: KeyboardModeLifecycleRequest,
) -> io::Result<CommandExecution> {
    if request.mode_id != MODE_ID {
        return Err(io::Error::other(format!(
            "Unknown keyboard mode: {}",
            request.mode_id
        )));
    }
    let actions = if method.ends_with("enter") {
        vec![notification("SESSION ENTER")]
    } else {
        vec![
            notification("SESSION EXIT"),
            // Lifecycle actions cannot reclaim ownership while the activation is
            // retiring. The trailing notice freezes Hunk's synchronous false result.
            ExtensionHostAction::EnterKeyboardMode { id: MODE_ID.into() },
            notification("SESSION REENTER false"),
        ]
    };
    Ok(CommandExecution { actions })
}

fn session_key(request: KeyboardModeKeyRequest) -> io::Result<KeyboardModeExecution> {
    if request.mode_id != MODE_ID {
        return Err(io::Error::other(format!(
            "Unknown keyboard mode: {}",
            request.mode_id
        )));
    }
    let key = pressed(&request.key);
    let result = match key {
        "j" | "?" => KeyRoutingResult::Handled,
        "x" => KeyRoutingResult::Exit,
        _ => KeyRoutingResult::Pass,
    };
    Ok(KeyboardModeExecution {
        result,
        actions: vec![notification(format!("SESSION KEY {key}"))],
    })
}

fn focused_layout(request: FileViewLayoutRequest) -> io::Result<ExtensionFileViewLayout> {
    if request.view_id != VIEW_ID {
        return Err(io::Error::other(format!(
            "Unknown file view: {}",
            request.view_id
        )));
    }
    Ok(ExtensionFileViewLayout {
        rows: vec![ExtensionFileViewRow {
            id: "focused".into(),
            spans: vec![ExtensionFileViewSpan {
                text: "FOCUSED VIEW".into(),
                tone: None,
                attributes: Vec::new(),
            }],
            source_ranges: Vec::new(),
            component: None,
        }],
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

fn file_lifecycle(
    request: FileViewModeLifecycleRequest,
    entering: bool,
) -> io::Result<FileViewModeLifecycleExecution> {
    if request.view_id != VIEW_ID {
        return Err(io::Error::other(format!(
            "Unknown file view: {}",
            request.view_id
        )));
    }
    Ok(FileViewModeLifecycleExecution {
        actions: vec![notification(if entering {
            "FILE ENTER"
        } else {
            "FILE EXIT"
        })],
        failure: None,
    })
}

fn file_key(request: FileViewModeKeyRequest) -> io::Result<KeyboardModeExecution> {
    if request.view_id != VIEW_ID {
        return Err(io::Error::other(format!(
            "Unknown file view: {}",
            request.view_id
        )));
    }
    let key = pressed(&request.key);
    Ok(KeyboardModeExecution {
        result: if key == "?" {
            KeyRoutingResult::Handled
        } else {
            KeyRoutingResult::Pass
        },
        actions: vec![notification(format!("FILE KEY {key}"))],
    })
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => to_value(HandshakeResponse {
                extension_api_version: API_VERSION,
                extension_version: env!("CARGO_PKG_VERSION").into(),
                registrations: registrations(),
            }),
            "workdeck/command/invoke" => parse(&request).and_then(invoke).and_then(to_value),
            "workdeck/keyboard-mode/enter" | "workdeck/keyboard-mode/exit" => {
                let method = request.method.clone();
                parse(&request)
                    .and_then(|value| session_lifecycle(&method, value))
                    .and_then(to_value)
            }
            "workdeck/keyboard-mode/key" => {
                parse(&request).and_then(session_key).and_then(to_value)
            }
            "workdeck/file-view/matches" => {
                parse::<FileViewMatchRequest>(&request).and_then(|value| {
                    if value.view_id == VIEW_ID {
                        to_value(true)
                    } else {
                        Err(io::Error::other(format!(
                            "Unknown file view: {}",
                            value.view_id
                        )))
                    }
                })
            }
            "workdeck/file-view/layout" => {
                parse(&request).and_then(focused_layout).and_then(to_value)
            }
            "workdeck/file-view-mode/enter" | "workdeck/file-view-mode/exit" => {
                let entering = request.method.ends_with("enter");
                parse(&request)
                    .and_then(|value| file_lifecycle(value, entering))
                    .and_then(to_value)
            }
            "workdeck/file-view-mode/key" => parse(&request).and_then(file_key).and_then(to_value),
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
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
                    message: error.to_string(),
                    data: None,
                }),
            },
        };
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
}

fn parse<T: serde::de::DeserializeOwned>(request: &JsonRpcRequest) -> io::Result<T> {
    serde_json::from_value(request.params.clone()).map_err(io::Error::other)
}

fn to_value<T: Serialize>(value: T) -> io::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}
