//! Compiled native parity fixture for Hunk's AppHost file-view refresh tests.

use std::collections::BTreeSet;
use std::io::{self, BufRead, Write};

use serde::Serialize;
use serde_json::Value;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration, ExtensionDiffFile,
    ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewSpan, ExtensionHostAction, FileViewLayoutRequest, FileViewMatchRequest,
    HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Registration,
};

pub const STATEFUL_VIEW_ID: &str = "stateful";
pub const BULK_VIEW_ID: &str = "preview";

fn command(id: &str, title: &str, key: &str) -> Registration {
    Registration::Command(CommandRegistration {
        id: id.into(),
        title: title.into(),
        description: None,
        default_keys: vec![key.into()],
    })
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::FileView {
            id: STATEFUL_VIEW_ID.into(),
            title: "Stateful view".into(),
            priority: 0,
            interactive_mode: false,
        },
        Registration::FileView {
            id: BULK_VIEW_ID.into(),
            title: "Bulk preview".into(),
            priority: 0,
            interactive_mode: false,
        },
        command("toggle-stateful", "Toggle stateful view", "f8"),
        command("expand-stateful", "Expand stateful view", "f9"),
        command("mark-stateful", "Mark this file", "f6"),
        command("refresh-unknown", "Refresh unknown view", "f7"),
        command("refresh-unknown-file", "Refresh missing file", "f4"),
        command("mark-hidden", "Mark hidden file", "f3"),
        command("toggle-preview", "Toggle bulk preview", "f2"),
    ]
}

fn one_row_layout(file: &ExtensionDiffFile, id: &str, text: String) -> ExtensionFileViewLayout {
    ExtensionFileViewLayout {
        rows: vec![ExtensionFileViewRow {
            id: id.into(),
            spans: vec![ExtensionFileViewSpan {
                text,
                tone: None,
                attributes: Vec::new(),
            }],
            source_ranges: Vec::new(),
            component: None,
        }],
        hunk_rows: file
            .hunks
            .iter()
            .map(|_| ExtensionFileViewHunkRows {
                start_row: 0,
                end_row: 0,
            })
            .collect(),
    }
}

fn response(output: &mut impl Write, id: u64, result: io::Result<Value>) -> io::Result<()> {
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

fn value(value: impl Serialize) -> io::Result<Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}

/// Serve a deliberately stateful pair of file views over the real JSON-RPC boundary.
pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut expanded = false;
    let mut pending = false;
    let mut marked = BTreeSet::new();
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
            "workdeck/file-view/matches" => {
                let request: FileViewMatchRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                value(match request.view_id.as_str() {
                    STATEFUL_VIEW_ID => true,
                    BULK_VIEW_ID => request.file.path.ends_with(".ts"),
                    id => return Err(io::Error::other(format!("Unknown file view: {id}"))),
                })
            }
            "workdeck/file-view/layout" => {
                let request: FileViewLayoutRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                let layout = match request.view_id.as_str() {
                    STATEFUL_VIEW_ID => {
                        let text = format!(
                            "STATE {}{}{}",
                            if expanded { "EXPANDED" } else { "COLLAPSED" },
                            if marked.contains(&request.file.id) {
                                " MARKED"
                            } else {
                                ""
                            },
                            if pending { " PENDING" } else { "" }
                        );
                        one_row_layout(&request.file, "state", text)
                    }
                    BULK_VIEW_ID => one_row_layout(
                        &request.file,
                        "preview",
                        format!("PREVIEW {}", request.file.path),
                    ),
                    id => return Err(io::Error::other(format!("Unknown file view: {id}"))),
                };
                value(Some(layout))
            }
            "workdeck/command/invoke" => {
                let invocation: CommandInvocation =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                let actions = match invocation.command_id.as_str() {
                    "toggle-stateful" => vec![ExtensionHostAction::ToggleFileView {
                        id: STATEFUL_VIEW_ID.into(),
                    }],
                    "expand-stateful" => {
                        expanded = !expanded;
                        vec![ExtensionHostAction::RefreshFileView {
                            id: STATEFUL_VIEW_ID.into(),
                            file_id: None,
                        }]
                    }
                    "mark-stateful" => invocation
                        .snapshot
                        .changeset
                        .files
                        .get(invocation.snapshot.selection.file_index)
                        .map(|file| {
                            marked.insert(file.runtime_id.clone());
                            ExtensionHostAction::RefreshFileView {
                                id: STATEFUL_VIEW_ID.into(),
                                file_id: Some(file.runtime_id.clone()),
                            }
                        })
                        .into_iter()
                        .collect(),
                    "refresh-unknown" => vec![ExtensionHostAction::RefreshFileView {
                        id: "not-a-view".into(),
                        file_id: None,
                    }],
                    "refresh-unknown-file" => {
                        pending = true;
                        vec![ExtensionHostAction::RefreshFileView {
                            id: STATEFUL_VIEW_ID.into(),
                            file_id: Some("no-such-file".into()),
                        }]
                    }
                    "mark-hidden" => invocation
                        .snapshot
                        .changeset
                        .files
                        .get(1)
                        .map(|file| {
                            marked.insert(file.runtime_id.clone());
                            ExtensionHostAction::RefreshFileView {
                                id: STATEFUL_VIEW_ID.into(),
                                file_id: Some(file.runtime_id.clone()),
                            }
                        })
                        .into_iter()
                        .collect(),
                    "toggle-preview" => vec![ExtensionHostAction::ToggleFileView {
                        id: BULK_VIEW_ID.into(),
                    }],
                    id => return Err(io::Error::other(format!("Unknown command: {id}"))),
                };
                value(CommandExecution { actions })
            }
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        response(&mut output, request.id, result)?;
    }
}
