//! Native port of Hunk's MIT-licensed test/pty/fixtures/current-line-lens/index.tsx.

use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, ExtensionFileSide, ExtensionPaneSize, HandshakeResponse, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, PaneAvailabilityRequest, PaneAvailabilityResponse,
    PanePlacement, PaneRegistration, PaneRenderRequest, PaneRenderResponse, Registration, ViewNode,
    ViewStyle,
};

pub const PANE_ID: &str = "current-line";

pub fn registration() -> PaneRegistration {
    PaneRegistration {
        id: PANE_ID.into(),
        title: "Current-line test fixture".into(),
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
        current_line: true,
        available: true,
    }
}

pub fn render(request: &PaneRenderRequest) -> PaneRenderResponse {
    let Some(current_line) = request.current_line else {
        return PaneRenderResponse {
            content: ViewNode::Empty,
        };
    };
    let width = usize::from(request.width);
    let label: String = "─ Current line · old above, new below "
        .chars()
        .take(width)
        .collect();
    let rule = format!(
        "{label}{}",
        "─".repeat(width.saturating_sub(label.chars().count()))
    );
    PaneRenderResponse {
        content: ViewNode::Column {
            children: vec![
                ViewNode::Text {
                    text: rule,
                    style: ViewStyle {
                        foreground: Some(request.theme.border.clone()),
                        background: Some(request.theme.panel.clone()),
                        ..ViewStyle::default()
                    },
                },
                current_line.render(ExtensionFileSide::Old, request.width),
                current_line.render(ExtensionFileSide::New, request.width),
            ],
            gap: 0,
        },
    }
}

pub fn serve(input: impl BufRead, mut output: impl Write) -> io::Result<()> {
    for line in input.lines() {
        let request: JsonRpcRequest = serde_json::from_str(&line?).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => serde_json::to_value(HandshakeResponse {
                extension_api_version: API_VERSION,
                extension_version: env!("CARGO_PKG_VERSION").into(),
                registrations: vec![Registration::Pane(registration())],
            }),
            "workdeck/pane/available" => serde_json::from_value::<PaneAvailabilityRequest>(
                request.params,
            )
            .and_then(|request| {
                serde_json::to_value(PaneAvailabilityResponse {
                    available: request.current_line.is_some(),
                })
            }),
            "workdeck/pane/render" => serde_json::from_value::<PaneRenderRequest>(request.params)
                .and_then(|request| serde_json::to_value(render(&request))),
            _ => {
                serde_json::to_writer(
                    &mut output,
                    &JsonRpcResponse {
                        jsonrpc: "2.0".into(),
                        id: request.id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32601,
                            message: "Unknown method".into(),
                            data: None,
                        }),
                    },
                )
                .map_err(io::Error::other)?;
                output.write_all(b"\n")?;
                output.flush()?;
                continue;
            }
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
    Ok(())
}
