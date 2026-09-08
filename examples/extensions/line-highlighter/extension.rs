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
    let mut expect_missing = false;
    let mut exit_on_highlight = false;
    let mut batch_four = false;
    let mut exit_batch = false;
    let mut batch_documents = false;
    let mut document_parents = std::collections::BTreeMap::<u64, (u64, String)>::new();
    let mut cancel_batch = false;
    let mut batch: Vec<(u64, String)> = Vec::new();
    let mut last_annotation_width: Option<usize> = None;
    let mut active_request = None;
    let mut callbacks = workdeck_extension_api::ExtensionDocumentCallbacks::default();
    let mut pending_documents =
        std::collections::BTreeMap::<u64, (LineHighlightRequest, bool)>::new();
    loop {
        let mut line = String::new();
        if incoming.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if batch_documents
            && value.get("method").is_none()
            && let Some(child_id) = value.get("id").and_then(Value::as_u64)
            && let Some((parent, path)) = document_parents.remove(&child_id)
        {
            let text = value
                .get("result")
                .cloned()
                .ok_or_else(|| io::Error::other("missing document result"))?;
            write_result(
                &mut output,
                parent,
                serde_json::json!({"path":path,"text":text,"simultaneous":4}),
            )?;
            continue;
        }
        if let Some(reply) = callbacks.accept(&value)? {
            let Some((mut input, second_read)) = pending_documents.remove(&reply.parent_id) else {
                continue;
            };
            match reply.result {
                Ok(text) => {
                    input.documents.insert(reply.side, text);
                }
                Err(error) => {
                    write_error(&mut output, reply.parent_id, error.code, &error.message)?;
                    continue;
                }
            }
            if !second_read {
                callbacks.request(
                    &mut output,
                    reply.parent_id,
                    workdeck_extension_api::ExtensionFileSide::New,
                )?;
                pending_documents.insert(reply.parent_id, (input, true));
                continue;
            }
            finish_highlight(
                &mut output,
                reply.parent_id,
                input,
                expect_missing,
                mark_annotations,
                &mut last_annotation_width,
            )?;
            continue;
        } else if value.get("method").is_none() {
            // Late responses from retired parents must not become requests.
            continue;
        }
        if value.get("id").is_none() {
            if value.get("method").and_then(Value::as_str) == Some("$/cancelRequest")
                && let Some(parent) = value.pointer("/params/id").and_then(Value::as_u64)
            {
                callbacks.retire(parent);
                pending_documents.remove(&parent);
            }
            if cancel_batch
                && value.get("method").and_then(Value::as_str) == Some("$/cancelRequest")
                && batch.iter().any(|(id, path)| {
                    path == "file-1.rs"
                        && Some(*id) == value.pointer("/params/id").and_then(Value::as_u64)
                })
            {
                batch.retain(|(_, path)| path != "file-1.rs");
                for (id, path) in batch.drain(..).rev() {
                    write_result(
                        &mut output,
                        id,
                        serde_json::json!({"path":path,"simultaneous":4}),
                    )?;
                }
            }
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
            "example/stderr-flood" => {
                // Native host resource-limit fixture: write more than a pipe can
                // buffer before replying, then exceed the retained entry limit.
                let mut diagnostics = io::stderr().lock();
                let chunk = [b'x'; 8192];
                for _ in 0..512 {
                    diagnostics.write_all(&chunk)?;
                }
                writeln!(diagnostics)?;
                for index in 0..1100 {
                    writeln!(diagnostics, "diagnostic {index}")?;
                }
                writeln!(diagnostics, "stderr flood finished")?;
                diagnostics.flush()?;
                write_result(
                    &mut output,
                    request.id,
                    serde_json::json!({"completed":true}),
                )?;
            }
            "example/last-annotation-width" => {
                write_result(&mut output, request.id, last_annotation_width)?;
            }
            "workdeck/handshake" => {
                let input: HandshakeRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                exit_on_highlight = input
                    .config
                    .get("exitOnHighlight")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                batch_four = input
                    .config
                    .get("batchFour")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                exit_batch = input
                    .config
                    .get("exitBatch")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                batch_documents = input
                    .config
                    .get("batchDocuments")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                cancel_batch = input
                    .config
                    .get("cancelBatch")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
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
                expect_missing = input
                    .config
                    .get("expectMissing")
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
                if exit_on_highlight {
                    return Ok(());
                }
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
                let lazy = input.document_reader;
                if batch_four {
                    batch.push((request.id, input.file.path.clone()));
                    if batch.len() == 4 {
                        if exit_batch {
                            return Ok(());
                        }
                        if batch_documents {
                            for (parent, path) in batch.drain(..).rev() {
                                let child = parent + 100_000;
                                document_parents.insert(child, (parent, path));
                                serde_json::to_writer(&mut output, &serde_json::json!({"jsonrpc":"2.0","id":child,"method":"workdeck/document/read","params":{"parentRequestId":parent,"side":"new"}})).map_err(io::Error::other)?;
                                output.write_all(b"\n")?;
                            }
                            output.flush()?;
                            continue;
                        }
                        if cancel_batch {
                            let first = batch
                                .iter()
                                .position(|(_, path)| path == "file-0.rs")
                                .unwrap();
                            let (id, path) = batch.remove(first);
                            write_result(
                                &mut output,
                                id,
                                serde_json::json!({"path":path,"simultaneous":4}),
                            )?;
                            continue;
                        }
                        for (id, path) in batch.drain(..).rev() {
                            write_result(
                                &mut output,
                                id,
                                serde_json::json!({"path":path,"simultaneous":4}),
                            )?;
                        }
                    }
                    continue;
                }
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
                    callbacks.request(
                        &mut output,
                        request.id,
                        workdeck_extension_api::ExtensionFileSide::New,
                    )?;
                    pending_documents.insert(request.id, (input, false));
                    continue;
                }
                finish_highlight(
                    &mut output,
                    request.id,
                    input,
                    expect_missing,
                    mark_annotations,
                    &mut last_annotation_width,
                )?;
            }
            _ => write_error(&mut output, request.id, -32601, "method not found")?,
        }
    }
}

fn finish_highlight(
    output: &mut impl Write,
    parent: u64,
    input: LineHighlightRequest,
    expect_missing: bool,
    mark_annotations: bool,
    last_annotation_width: &mut Option<usize>,
) -> io::Result<()> {
    let old = input
        .documents
        .get(&workdeck_extension_api::ExtensionFileSide::Old)
        .and_then(|text| text.as_deref());
    let new = input
        .documents
        .get(&workdeck_extension_api::ExtensionFileSide::New)
        .and_then(|text| text.as_deref());
    if expect_missing && input.document_reader {
        return if new.is_none() {
            write_result(output, parent, Vec::<Value>::new())
        } else {
            write_error(output, parent, -32602, "expected unreadable document")
        };
    }
    if input.aborted || (!input.document_reader && old != Some("old\n")) || new != Some("new\n") {
        return write_error(
            output,
            parent,
            -32602,
            "expected immutable old/new documents",
        );
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
    *last_annotation_width = Some(width);
    if width == 0 {
        return write_result(output, parent, serde_json::json!([]));
    }
    write_result(
        output,
        parent,
        serde_json::json!([{
            "side":"new", "line":1, "range":[0,width], "tone":"warning"
        }]),
    )
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
