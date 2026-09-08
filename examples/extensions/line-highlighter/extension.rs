//! Executable native line-highlighter protocol example and lifecycle fixture.

use serde::Serialize;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, HandshakeRequest, HandshakeResponse, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse, LineHighlightRequest, Registration,
};

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut require_cleanup = false;
    let mut active_request = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if value.get("id").is_none() {
            if value.get("method").and_then(Value::as_str) == Some("$/cancelRequest")
                && value.pointer("/params/id").and_then(Value::as_u64) == active_request
            {
                active_request = None;
            }
            if value.get("method").and_then(Value::as_str) == Some("workdeck/shutdown") {
                return Ok(());
            }
            continue;
        }
        let request: JsonRpcRequest = serde_json::from_value(value).map_err(io::Error::other)?;
        match request.method.as_str() {
            "workdeck/handshake" => {
                let input: HandshakeRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                require_cleanup = input
                    .config
                    .get("requireCleanup")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                let mut registrations = vec![Registration::LineHighlighter {
                    id: "attention".into(),
                }];
                if input
                    .config
                    .get("includeHang")
                    .and_then(Value::as_bool)
                    .unwrap_or(true)
                {
                    registrations.push(Registration::LineHighlighter { id: "hang".into() });
                }
                write_result(
                    &mut output,
                    request.id,
                    HandshakeResponse {
                        extension_api_version: API_VERSION,
                        extension_version: env!("CARGO_PKG_VERSION").into(),
                        registrations,
                    },
                )?;
            }
            "workdeck/line-highlighter/highlight" => {
                if require_cleanup && active_request.is_some() {
                    write_error(
                        &mut output,
                        request.id,
                        -32602,
                        "previous request signal was not cleaned up",
                    )?;
                    continue;
                }
                active_request = Some(request.id);
                let input: LineHighlightRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                if input.highlighter_id == "hang" {
                    // Leave the request unresolved while continuing to service
                    // lifecycle notifications from the host.
                    continue;
                }
                let old = input
                    .documents
                    .get(&workdeck_extension_api::ExtensionFileSide::Old)
                    .cloned()
                    .flatten();
                let new = input
                    .documents
                    .get(&workdeck_extension_api::ExtensionFileSide::New)
                    .cloned()
                    .flatten();
                if input.aborted
                    || old.as_deref() != Some("old\n")
                    || new.as_deref() != Some("new\n")
                {
                    write_error(
                        &mut output,
                        request.id,
                        -32602,
                        "expected immutable old/new documents",
                    )?;
                    continue;
                }
                write_result(
                    &mut output,
                    request.id,
                    serde_json::json!([{
                        "side": "new",
                        "line": 1,
                        "range": [0, 3],
                        "tone": "warning"
                    }]),
                )?;
            }
            _ => write_error(&mut output, request.id, -32601, "method not found")?,
        }
    }
}

fn write_result(output: &mut impl Write, id: u64, result: impl Serialize) -> io::Result<()> {
    serde_json::to_writer(
        &mut *output,
        &JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::to_value(result).map_err(io::Error::other)?),
            error: None,
        },
    )
    .map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

fn write_error(output: &mut impl Write, id: u64, code: i32, message: &str) -> io::Result<()> {
    serde_json::to_writer(
        &mut *output,
        &JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.into(),
                data: None,
            }),
        },
    )
    .map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}
