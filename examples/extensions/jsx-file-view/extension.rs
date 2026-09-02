//! Native Ratatui port of Hunk's fixed-height JSX file-view proof of concept.

use std::io::{self, BufRead, Write};

use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionDiffFile, ExtensionDiffHunk, ExtensionFileViewHunkRows, ExtensionFileViewLayout,
    ExtensionFileViewRow, ExtensionFileViewRowComponent, ExtensionFileViewSelectionPrefix,
    ExtensionFileViewSourceRange, ExtensionFileViewSpan, ExtensionFileViewTone,
    ExtensionHostAction, FileViewLayoutRequest, FileViewMatchRequest, HandshakeResponse,
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, Registration, ViewNode, ViewStyle,
};

pub const VIEW_ID: &str = "jsx-cards";
pub const COMMAND_ID: &str = "toggle-jsx-cards";

#[must_use]
pub fn matches_jsx_file_view(file: &ExtensionDiffFile) -> bool {
    file.hunks.len() >= 2
}

fn source_ranges(hunk: &ExtensionDiffHunk) -> Vec<ExtensionFileViewSourceRange> {
    [
        (
            workdeck_extension_api::ExtensionFileSide::Old,
            hunk.old_range,
        ),
        (
            workdeck_extension_api::ExtensionFileSide::New,
            hunk.new_range,
        ),
    ]
    .into_iter()
    .filter_map(|(side, range)| {
        let range = range?;
        (range[0] >= 1).then_some(ExtensionFileViewSourceRange {
            side,
            range: [range[0] as usize, range[1] as usize],
        })
    })
    .collect()
}

fn hunk_card(
    title: String,
    collapsed_detail: String,
    expanded_detail: String,
    foreground: &str,
) -> ExtensionFileViewRowComponent {
    let card = |detail: String| ViewNode::Column {
        children: vec![
            ViewNode::Text {
                text: title.clone(),
                style: ViewStyle {
                    foreground: Some(foreground.into()),
                    ..ViewStyle::default()
                },
            },
            ViewNode::Text {
                text: format!("  {detail}"),
                style: ViewStyle::default(),
            },
        ],
        gap: 0,
    };
    ExtensionFileViewRowComponent {
        height: 2,
        content: card(collapsed_detail),
        selected_content: None,
        expanded_content: Some(card(expanded_detail)),
        selected_expanded_content: None,
        toggle_expanded_on_left_mouse_up: true,
        selection_prefix: Some(ExtensionFileViewSelectionPrefix {
            selected: "▶ ".into(),
            unselected: "  ".into(),
        }),
    }
}

/// Build the deterministic two-row-per-hunk layout from the pinned Hunk example.
#[must_use]
pub fn create_jsx_file_view_layout(file: &ExtensionDiffFile) -> Option<ExtensionFileViewLayout> {
    if !matches_jsx_file_view(file) {
        return None;
    }
    let mut rows = Vec::with_capacity(file.hunks.len() * 2);
    for hunk in &file.hunks {
        let range = hunk.new_range.or(hunk.old_range);
        let range_label = range.map_or_else(
            || "unknown lines".into(),
            |range| format!("lines {}–{}", range[0], range[1]),
        );
        let summary_row_index = rows.len();
        rows.push(ExtensionFileViewRow {
            id: format!("hunk-{}-summary", hunk.index),
            spans: vec![ExtensionFileViewSpan {
                text: format!("Hunk {}: {}", hunk.index + 1, hunk.header),
                tone: Some(ExtensionFileViewTone::Accent),
                attributes: Vec::new(),
            }],
            source_ranges: source_ranges(hunk),
            component: Some(hunk_card(
                format!("Hunk {}", hunk.index + 1),
                format!("row {summary_row_index} · click for detail"),
                format!("{range_label} · {}", hunk.header),
                "file-new",
            )),
        });
        let detail_row_index = rows.len();
        rows.push(ExtensionFileViewRow {
            id: format!("hunk-{}-detail", hunk.index),
            spans: vec![ExtensionFileViewSpan {
                text: format!("{range_label} (symbolic fallback)"),
                tone: Some(ExtensionFileViewTone::Muted),
                attributes: Vec::new(),
            }],
            source_ranges: Vec::new(),
            component: Some(hunk_card(
                "Changed range".into(),
                format!("row {detail_row_index} · click for detail"),
                range_label,
                "accent",
            )),
        });
    }
    Some(ExtensionFileViewLayout {
        hunk_rows: file
            .hunks
            .iter()
            .enumerate()
            .map(|(position, _)| ExtensionFileViewHunkRows {
                start_row: position * 2,
                end_row: position * 2 + 1,
            })
            .collect(),
        rows,
    })
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::FileView {
            id: VIEW_ID.into(),
            title: "JSX hunk cards (POC)".into(),
            priority: 0,
            interactive_mode: false,
        },
        Registration::Command(CommandRegistration {
            id: COMMAND_ID.into(),
            title: "Toggle JSX hunk cards (POC)".into(),
            description: None,
            default_keys: vec!["f8".into()],
        }),
    ]
}

#[must_use]
pub fn required_capabilities() -> Vec<Capability> {
    vec![Capability::Commands, Capability::FileViews]
}

fn invoke_command(invocation: &CommandInvocation) -> Result<CommandExecution, String> {
    if invocation.command_id != COMMAND_ID {
        return Err(format!("Unknown command: {}", invocation.command_id));
    }
    Ok(CommandExecution {
        actions: vec![ExtensionHostAction::ToggleFileView { id: VIEW_ID.into() }],
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
            "workdeck/handshake" => serde_json::to_value(HandshakeResponse {
                extension_api_version: API_VERSION,
                extension_version: env!("CARGO_PKG_VERSION").into(),
                registrations: registrations(),
            })
            .map_err(io::Error::other),
            "workdeck/command/invoke" => {
                let invocation: CommandInvocation =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                invoke_command(&invocation)
                    .and_then(|execution| {
                        serde_json::to_value(execution).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/file-view/matches" => {
                let request: FileViewMatchRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                if request.view_id != VIEW_ID {
                    Err(io::Error::other(format!(
                        "Unknown file view: {}",
                        request.view_id
                    )))
                } else {
                    serde_json::to_value(matches_jsx_file_view(&request.file))
                        .map_err(io::Error::other)
                }
            }
            "workdeck/file-view/layout" => {
                let request: FileViewLayoutRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                if request.view_id != VIEW_ID {
                    Err(io::Error::other(format!(
                        "Unknown file view: {}",
                        request.view_id
                    )))
                } else {
                    serde_json::to_value(create_jsx_file_view_layout(&request.file))
                        .map_err(io::Error::other)
                }
            }
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
