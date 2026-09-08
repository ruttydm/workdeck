//! Executable native line-highlighter protocol example and lifecycle fixture.

use serde::Serialize;
use serde_json::Value;
use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, HandshakeRequest, HandshakeResponse, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse, LineHighlightRequest, Registration,
};

pub fn serve<R: BufRead, W: Write>(mut incoming: R, mut output: W) -> io::Result<()> {
    let mut require_cleanup = false;
    let mut mark_annotations = false;
    let mut skip_documents = false;
    let mut last_annotation_width: Option<usize> = None;
    let mut active_request = None;
    'requests: loop {
        let mut line = String::new();
        if incoming.read_line(&mut line)? == 0 {
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
            "example/last-annotation-width" => {
                write_result(&mut output, request.id, last_annotation_width)?;
            }
            "workdeck/handshake" => {
                let input: HandshakeRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                require_cleanup = input
                    .config
                    .get("requireCleanup")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                mark_annotations = input
                    .config
                    .get("markAnnotations")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                skip_documents = input
                    .config
                    .get("skipDocuments")
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
                let mut input: LineHighlightRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                let lazy = input.document_reader;
                if skip_documents {
                    write_result(&mut output, request.id, Vec::<Value>::new())?;
                    continue;
                }
                if input.highlighter_id == "hang" {
                    // Leave the request unresolved while continuing to service
                    // lifecycle notifications from the host.
                    continue;
                }
                if lazy {
                    for child_id in [1, 2] {
                        match read_document(&mut incoming, &mut output, request.id, child_id) {
                            Ok(text) => {
                                input
                                    .documents
                                    .insert(workdeck_extension_api::ExtensionFileSide::New, text);
                            }
                            Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                                active_request = None;
                                continue 'requests;
                            }
                            Err(error) => {
                                write_error(&mut output, request.id, -32602, &error.to_string())?;
                                continue 'requests;
                            }
                        }
                    }
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
                    || (!lazy && old.as_deref() != Some("old\n"))
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
                let width = if mark_annotations {
                    input
                        .file
                        .agent
                        .as_ref()
                        .map_or(0, |agent| {
                            agent
                                .annotations
                                .iter()
                                .map(|note| note.summary.chars().count())
                                .sum::<usize>()
                        })
                        .min(3)
                } else {
                    3
                };
                last_annotation_width = Some(width);
                if width == 0 {
                    write_result(&mut output, request.id, serde_json::json!([]))?;
                    continue;
                }
                write_result(
                    &mut output,
                    request.id,
                    serde_json::json!([{
                        "side": "new",
                        "line": 1,
                        "range": [0, width],
                        "tone": "warning"
                    }]),
                )?;
            }
            _ => write_error(&mut output, request.id, -32601, "method not found")?,
        }
    }
}

fn read_document(
    input: &mut impl BufRead,
    output: &mut impl Write,
    parent_id: u64,
    child_id: u64,
) -> io::Result<Option<String>> {
    serde_json::to_writer(
        &mut *output,
        &serde_json::json!({ "jsonrpc": "2.0", "id": child_id,
        "method": workdeck_extension_api::EXTENSION_DOCUMENT_READ_METHOD,
        "params": { "parentRequestId": parent_id, "side": "new" } }),
    )
    .map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()?;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if value.get("method").and_then(Value::as_str) == Some("$/cancelRequest") {
            if value.pointer("/params/id").and_then(Value::as_u64) == Some(parent_id) {
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            continue;
        }
        if value.get("id").and_then(Value::as_u64) != Some(child_id) {
            return Err(io::Error::other("unexpected document response ID"));
        }
        if let Some(error) = value.get("error") {
            return Err(io::Error::other(error.to_string()));
        }
        return match value.get("result") {
            Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            _ => Err(io::Error::other("invalid document response")),
        };
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
