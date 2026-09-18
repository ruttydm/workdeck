//! Compiled native parity fixture for Hunk's AppHost key-ownership tests.

use std::io::{self, BufRead, Write};

use serde::Serialize;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewSpan, ExtensionHostAction, ExtensionKeyEvent, ExtensionNotifyType,
    ExtensionPaneSize, FileViewLayoutRequest, FileViewMatchRequest, FileViewModeKeyRequest,
    HandshakeRequest, HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    KeyRoutingResult, KeyboardModeExecution, PaneInputInvocation, PanePlacement, PaneRegistration,
    PaneRenderRequest, PaneRenderResponse, Registration, ViewNode,
};

pub const EXTENSION_ID: &str = "test.key-routing-probe";
pub const VIEW_ID: &str = "tall";
pub const PANE_ID: &str = "prompt";
pub const INPUT_ID: &str = "prompt";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Surface {
    #[default]
    FileView,
    Pane,
}

impl Surface {
    fn from_handshake(request: &HandshakeRequest) -> io::Result<Self> {
        match request
            .config
            .get("surface")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("file-view")
        {
            "file-view" => Ok(Self::FileView),
            "pane" => Ok(Self::Pane),
            value => Err(io::Error::other(format!("Unknown surface: {value}"))),
        }
    }
}

#[must_use]
fn registrations(surface: Surface) -> Vec<Registration> {
    match surface {
        Surface::FileView => vec![
            Registration::FileView {
                id: VIEW_ID.into(),
                title: "Tall".into(),
                priority: 0,
                interactive_mode: true,
            },
            command("toggle", "Toggle tall", "f8"),
            command("enter", "Enter tall mode", "f9"),
        ],
        Surface::Pane => vec![
            Registration::Pane(PaneRegistration {
                id: PANE_ID.into(),
                title: "Prompt".into(),
                placement: PanePlacement::Bottom,
                default_open: true,
                preferred_size: None,
                width: None,
                height: Some(ExtensionPaneSize {
                    preferred: 3,
                    min: Some(3),
                    max: Some(3),
                    fraction: None,
                }),
                replaces: None,
                current_line: false,
                available: false,
            }),
            command("letter", "Letter command", "j"),
        ],
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

fn invoke_command(invocation: CommandInvocation) -> io::Result<CommandExecution> {
    let actions = match invocation.command_id.as_str() {
        "toggle" => vec![ExtensionHostAction::ToggleFileView { id: VIEW_ID.into() }],
        "enter" => vec![ExtensionHostAction::EnterFileViewMode { id: VIEW_ID.into() }],
        "letter" => vec![ExtensionHostAction::Notify {
            message: "COMMAND FIRED".into(),
            notification_type: ExtensionNotifyType::Info,
        }],
        id => return Err(io::Error::other(format!("Unknown command: {id}"))),
    };
    Ok(CommandExecution { actions })
}

fn render_pane(request: PaneRenderRequest, value: &str) -> io::Result<PaneRenderResponse> {
    if request.pane_id != PANE_ID || request.placement != PanePlacement::Bottom {
        return Err(io::Error::other(format!(
            "Unknown pane or placement: {} {:?}",
            request.pane_id, request.placement
        )));
    }
    Ok(PaneRenderResponse {
        content: ViewNode::Input {
            id: INPUT_ID.into(),
            value: value.into(),
            placeholder: None,
            focused: true,
        },
    })
}

fn update_pane_input(
    request: PaneInputInvocation,
    value: &mut String,
) -> io::Result<CommandExecution> {
    if request.pane_id != PANE_ID || request.input_id != INPUT_ID {
        return Err(io::Error::other(format!(
            "Unknown pane input: {}:{}",
            request.pane_id, request.input_id
        )));
    }
    *value = request.value;
    Ok(CommandExecution::default())
}

fn tall_layout(request: FileViewLayoutRequest) -> io::Result<ExtensionFileViewLayout> {
    if request.view_id != VIEW_ID {
        return Err(io::Error::other(format!(
            "Unknown file view: {}",
            request.view_id
        )));
    }
    Ok(ExtensionFileViewLayout {
        rows: (0..200)
            .map(|index| ExtensionFileViewRow {
                id: format!("row:{index}"),
                spans: vec![ExtensionFileViewSpan {
                    text: format!("TALL ROW {}", index + 1),
                    tone: None,
                    attributes: Vec::new(),
                }],
                source_ranges: Vec::new(),
                component: None,
            })
            .collect(),
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

fn pressed(key: &ExtensionKeyEvent) -> &str {
    if key.sequence.is_empty() {
        key.name.as_str()
    } else {
        key.sequence.as_str()
    }
}

fn route_file_view_key(request: FileViewModeKeyRequest) -> io::Result<KeyboardModeExecution> {
    if request.view_id != VIEW_ID {
        return Err(io::Error::other(format!(
            "Unknown file view: {}",
            request.view_id
        )));
    }
    Ok(KeyboardModeExecution {
        result: if pressed(&request.key) == "n" {
            KeyRoutingResult::Handled
        } else {
            KeyRoutingResult::Pass
        },
        actions: Vec::new(),
    })
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut surface = Surface::default();
    let mut pane_value = String::new();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => {
                let handshake: HandshakeRequest = parse(&request)?;
                surface = Surface::from_handshake(&handshake)?;
                to_value(HandshakeResponse {
                    extension_api_version: API_VERSION,
                    extension_version: env!("CARGO_PKG_VERSION").into(),
                    registrations: registrations(surface),
                })
            }
            "workdeck/command/invoke" => {
                parse(&request).and_then(invoke_command).and_then(to_value)
            }
            "workdeck/pane/render" if surface == Surface::Pane => parse(&request)
                .and_then(|request| render_pane(request, &pane_value))
                .and_then(to_value),
            "workdeck/pane/input" if surface == Surface::Pane => parse(&request)
                .and_then(|request| update_pane_input(request, &mut pane_value))
                .and_then(to_value),
            "workdeck/file-view/matches" if surface == Surface::FileView => {
                parse::<FileViewMatchRequest>(&request).and_then(|request| {
                    if request.view_id == VIEW_ID {
                        to_value(true)
                    } else {
                        Err(io::Error::other(format!(
                            "Unknown file view: {}",
                            request.view_id
                        )))
                    }
                })
            }
            "workdeck/file-view/layout" if surface == Surface::FileView => {
                parse(&request).and_then(tall_layout).and_then(to_value)
            }
            "workdeck/file-view-mode/enter" | "workdeck/file-view-mode/exit"
                if surface == Surface::FileView =>
            {
                to_value(CommandExecution::default())
            }
            "workdeck/file-view-mode/key" if surface == Surface::FileView => parse(&request)
                .and_then(route_file_view_key)
                .and_then(to_value),
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
