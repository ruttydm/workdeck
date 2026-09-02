//! Compiled native port of Hunk's pane-layout extension example.

use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionHostAction, ExtensionPaneSize, HandshakeResponse, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse, PanePlacement, PaneRegistration, PaneRenderRequest, PaneRenderResponse,
    Registration, ViewNode, ViewStyle,
};

const EXTENSION_ID: &str = "example.pane-layout";
const PANE_IDS: [&str; 3] = ["side", "top", "bottom"];

fn pane_registration(id: &str, placement: PanePlacement) -> PaneRegistration {
    let fixed = ExtensionPaneSize {
        preferred: 2,
        min: Some(2),
        max: Some(2),
        fraction: None,
    };
    PaneRegistration {
        id: id.into(),
        title: format!("Pane example · {id}"),
        placement,
        default_open: false,
        preferred_size: None,
        width: (id == "side").then_some(ExtensionPaneSize {
            preferred: 28,
            min: Some(18),
            max: Some(44),
            fraction: None,
        }),
        height: (id != "side").then_some(fixed),
    }
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::Pane(pane_registration("side", PanePlacement::Right)),
        Registration::Pane(pane_registration("top", PanePlacement::Top)),
        Registration::Pane(pane_registration("bottom", PanePlacement::Bottom)),
        Registration::Command(CommandRegistration {
            id: "toggle".into(),
            title: "Toggle pane layout example".into(),
            description: None,
            default_keys: vec!["ctrl+p".into()],
        }),
    ]
}

pub fn render_pane(request: &PaneRenderRequest) -> Result<PaneRenderResponse, String> {
    let expected = match request.pane_id.as_str() {
        "side" => PanePlacement::Right,
        "top" => PanePlacement::Top,
        "bottom" => PanePlacement::Bottom,
        id => return Err(format!("Unknown pane: {id}")),
    };
    if request.placement != expected {
        return Err(format!("Unexpected placement for pane {}", request.pane_id));
    }
    let placement = match request.placement {
        PanePlacement::Left => "LEFT",
        PanePlacement::Right => "RIGHT",
        PanePlacement::Top => "TOP",
        PanePlacement::Bottom => "BOTTOM",
    };
    let selected = request
        .snapshot
        .changeset
        .files
        .get(request.snapshot.selection.file_index)
        .map(|file| file.path.as_str())
        .map_or_else(
            || format!("{} visible files", request.snapshot.changeset.files.len()),
            str::to_owned,
        );
    Ok(PaneRenderResponse {
        content: ViewNode::Column {
            children: vec![
                ViewNode::Text {
                    text: format!("{placement} PANE · {}×{}", request.width, request.height),
                    style: ViewStyle {
                        foreground: Some(request.theme.accent.clone()),
                        ..ViewStyle::default()
                    },
                },
                ViewNode::Text {
                    text: selected,
                    style: ViewStyle {
                        foreground: Some(request.theme.text.clone()),
                        ..ViewStyle::default()
                    },
                },
            ],
            gap: 0,
        },
    })
}

pub fn invoke_toggle(invocation: &CommandInvocation) -> Result<CommandExecution, String> {
    if invocation.command_id != "toggle" {
        return Err(format!("Unknown command: {}", invocation.command_id));
    }
    let close = PANE_IDS.iter().any(|id| {
        invocation
            .open_panes
            .iter()
            .any(|key| key == &format!("{EXTENSION_ID}:{id}"))
    });
    Ok(CommandExecution {
        actions: PANE_IDS
            .iter()
            .map(|id| {
                if close {
                    ExtensionHostAction::ClosePane { id: (*id).into() }
                } else {
                    ExtensionHostAction::OpenPane { id: (*id).into() }
                }
            })
            .collect(),
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
            "workdeck/pane/render" => {
                let request: PaneRenderRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                render_pane(&request)
                    .and_then(|response| {
                        serde_json::to_value(response).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/command/invoke" => {
                let invocation: CommandInvocation =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                invoke_toggle(&invocation)
                    .and_then(|execution| {
                        serde_json::to_value(execution).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
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

#[must_use]
pub fn required_capabilities() -> Vec<Capability> {
    vec![Capability::Commands, Capability::Panes]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_closes_all_when_any_example_pane_is_open() {
        let result = invoke_toggle(&CommandInvocation {
            command_id: "toggle".into(),
            snapshot: workdeck_review_snapshot(),
            open_panes: vec!["example.pane-layout:top".into()],
        })
        .unwrap();
        assert!(
            result
                .actions
                .iter()
                .all(|action| matches!(action, ExtensionHostAction::ClosePane { .. }))
        );
    }

    fn workdeck_review_snapshot() -> workdeck_core::ReviewSnapshot {
        workdeck_core::ReviewSnapshot {
            generation: 0,
            changeset: workdeck_core::Changeset {
                id: "test".into(),
                title: "test".into(),
                source: workdeck_core::ChangesetSource::Patch {
                    label: "test".into(),
                },
                files: Vec::new(),
            },
            selection: workdeck_core::ReviewSelection::default(),
        }
    }
}
