//! Hunk MIT: native translation of createInteractiveViewExtension in
//! test/pty/file-views-integration.test.ts. The host owns every rendered cell.
use std::io::{self, BufRead, Write};
use workdeck_extension_api::*;

fn value(value: impl serde::Serialize) -> io::Result<serde_json::Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}

fn dispatch(request: &JsonRpcRequest, cursor: &mut usize) -> io::Result<serde_json::Value> {
    match request.method.as_str() {
        "workdeck/handshake" => value(HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: "0.1.0".into(),
            registrations: vec![
                Registration::FileView {
                    id: "cursor".into(),
                    title: "Cursor demo".into(),
                    priority: 0,
                    interactive_mode: true,
                },
                Registration::Command(CommandRegistration {
                    id: "toggle".into(),
                    title: "Toggle cursor demo".into(),
                    description: None,
                    default_keys: vec!["f8".into()],
                }),
                Registration::Command(CommandRegistration {
                    id: "enter".into(),
                    title: "Enter cursor mode".into(),
                    description: None,
                    default_keys: vec!["f9".into()],
                }),
            ],
        }),
        "workdeck/command/invoke" => {
            let invocation: CommandInvocation =
                serde_json::from_value(request.params.clone()).map_err(io::Error::other)?;
            let action = match invocation.command_id.as_str() {
                "toggle" => ExtensionHostAction::ToggleFileView {
                    id: "cursor".into(),
                },
                "enter" => ExtensionHostAction::EnterFileViewMode {
                    id: "cursor".into(),
                },
                _ => return Err(io::Error::other("unknown cursor command")),
            };
            value(CommandExecution {
                actions: vec![action],
            })
        }
        "workdeck/file-view/matches" => value(true),
        "workdeck/file-view/layout" => {
            let request: FileViewLayoutRequest =
                serde_json::from_value(request.params.clone()).map_err(io::Error::other)?;
            value(ExtensionFileViewLayout {
                rows: vec![ExtensionFileViewRow {
                    id: "cursor".into(),
                    spans: vec![ExtensionFileViewSpan {
                        text: format!("CURSOR AT {cursor}"),
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
        "workdeck/file-view-mode/enter" | "workdeck/file-view-mode/exit" => {
            value(FileViewModeLifecycleExecution {
                actions: Vec::new(),
                failure: None,
            })
        }
        "workdeck/file-view-mode/key" => {
            let request: FileViewModeKeyRequest =
                serde_json::from_value(request.params.clone()).map_err(io::Error::other)?;
            let (result, actions) = match request.key.name.as_str() {
                "j" => {
                    *cursor += 1;
                    (
                        KeyRoutingResult::Handled,
                        vec![ExtensionHostAction::RefreshFileView {
                            id: "cursor".into(),
                            file_id: None,
                        }],
                    )
                }
                "x" => (KeyRoutingResult::Exit, Vec::new()),
                _ => (KeyRoutingResult::Pass, Vec::new()),
            };
            value(KeyboardModeExecution { result, actions })
        }
        _ => Err(io::Error::other("unknown cursor protocol method")),
    }
}

fn main() -> io::Result<()> {
    let mut cursor = 0;
    let mut output = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let request: JsonRpcRequest = serde_json::from_str(&line?).map_err(io::Error::other)?;
        let response = match dispatch(&request, &mut cursor) {
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
                    code: -32000,
                    message: error.to_string(),
                    data: None,
                }),
            },
        };
        serde_json::to_writer(&mut output, &response).map_err(io::Error::other)?;
        output.write_all(b"\n")?;
        output.flush()?;
    }
    Ok(())
}
