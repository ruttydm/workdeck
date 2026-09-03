//! Minimal compiled VCS adapter demonstrating the complete native protocol.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, ExtensionVcsAdapterRegistration, ExtensionVcsDetectRequest, ExtensionVcsFileSide,
    ExtensionVcsFileSourceInvocation, ExtensionVcsFileSourceResult, ExtensionVcsOperationKind,
    ExtensionVcsOperationRegistration, ExtensionVcsOperationRequest, ExtensionVcsPatchResult,
    ExtensionVcsWatchCoverage, ExtensionVcsWatchPlan, ExtensionVcsWatchTarget,
    ExtensionVcsWatchTargetSource, HandshakeResponse, JsonRpcError, JsonRpcRequest,
    JsonRpcResponse, Registration,
};

const ADAPTER_ID: &str = "example-vcs";

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut source_reads = BTreeMap::<String, usize>::new();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if value.get("id").is_none() {
            if value.get("method").and_then(Value::as_str) == Some("workdeck/shutdown") {
                return Ok(());
            }
            continue;
        }
        let request: JsonRpcRequest = serde_json::from_value(value).map_err(io::Error::other)?;
        let request_id = request.id;
        let result = match request.method.as_str() {
            "workdeck/handshake" => serde_json::to_value(handshake()),
            "workdeck/vcs/detect" => {
                let request: ExtensionVcsDetectRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                serde_json::to_value(serde_json::json!({
                    "id": "mistyped-example-vcs",
                    "repoRoot": request.cwd,
                }))
            }
            "workdeck/vcs/load" => {
                let request: ExtensionVcsOperationRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                serde_json::to_value(load(request))
            }
            "workdeck/vcs/source/read" => {
                let request: ExtensionVcsFileSourceInvocation =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                let read_key = format!("{}:{:?}", request.request.path, request.request.side);
                let attempts = source_reads.entry(read_key).or_default();
                *attempts += 1;
                if (request.request.path == "tracked.txt" && *attempts > 1)
                    || (request.request.path == "retry.txt" && *attempts == 1)
                    || (request.request.path == "too-large.txt" && *attempts > 1)
                {
                    write_error(&mut output, request_id, "synthetic source failure".into())?;
                    continue;
                }
                if request.request.path == "too-large.txt" {
                    serde_json::to_value(ExtensionVcsFileSourceResult::TooLarge {
                        max_bytes: Some(42),
                    })
                } else {
                    let text = match request.request.side {
                        ExtensionVcsFileSide::Old => "old\n",
                        ExtensionVcsFileSide::New => "new\n",
                    };
                    serde_json::to_value(ExtensionVcsFileSourceResult::Source(text.into()))
                }
            }
            "workdeck/vcs/watch-signature" => {
                let request: ExtensionVcsOperationRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                serde_json::to_value(format!("{}:{:?}", request.adapter_id, request.operation))
            }
            "workdeck/vcs/watch-plan" => {
                let request: ExtensionVcsOperationRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                serde_json::to_value(ExtensionVcsWatchPlan {
                    coverage: ExtensionVcsWatchCoverage::Hybrid,
                    targets: vec![ExtensionVcsWatchTarget::DirectoryEntries {
                        directory: request.context.cwd,
                        entries: vec![".example-vcs".into()],
                        sources: vec![ExtensionVcsWatchTargetSource::VcsMetadata],
                    }],
                })
            }
            _ => {
                write_error(
                    &mut output,
                    request.id,
                    format!("unknown method {}", request.method),
                )?;
                continue;
            }
        }
        .map_err(io::Error::other)?;
        write_result(&mut output, request.id, result)?;
    }
}

fn handshake() -> HandshakeResponse {
    let callbacks = ExtensionVcsOperationRegistration {
        watch_signature: true,
        watch_plan: true,
    };
    HandshakeResponse {
        extension_api_version: API_VERSION,
        extension_version: env!("CARGO_PKG_VERSION").into(),
        registrations: vec![Registration::VcsAdapter(ExtensionVcsAdapterRegistration {
            id: ADAPTER_ID.into(),
            name: "Example VCS".into(),
            operations: BTreeMap::from([
                (ExtensionVcsOperationKind::WorkingTreeDiff, callbacks),
                (ExtensionVcsOperationKind::RevisionShow, callbacks),
                (ExtensionVcsOperationKind::StashShow, callbacks),
            ]),
            detection_priority: Some(50),
        })],
    }
}

fn load(request: ExtensionVcsOperationRequest) -> ExtensionVcsPatchResult {
    ExtensionVcsPatchResult {
        repo_root: request.context.cwd.clone(),
        source_label: request.context.cwd.display().to_string(),
        title: format!("Example VCS {:?}", request.operation),
        patch_text: concat!(
            "diff --git a/tracked.txt b/tracked.txt\n",
            "--- a/tracked.txt\n",
            "+++ b/tracked.txt\n",
            "@@ -1 +1 @@\n",
            "-old\n",
            "+new\n"
        )
        .into(),
        untracked_paths: Vec::new(),
        read_file_source: true,
        load_token: Some("example-snapshot-1".into()),
        source_cache_key: Some("example-cache-1".into()),
        extra_files: Vec::new(),
    }
}

fn write_result(output: &mut impl Write, id: u64, result: impl Serialize) -> io::Result<()> {
    write_line(
        output,
        &JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(serde_json::to_value(result).map_err(io::Error::other)?),
            error: None,
        },
    )
}

fn write_error(output: &mut impl Write, id: u64, message: String) -> io::Result<()> {
    write_line(
        output,
        &JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32601,
                message,
                data: None,
            }),
        },
    )
}

fn write_line(output: &mut impl Write, value: &impl Serialize) -> io::Result<()> {
    serde_json::to_writer(&mut *output, value).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}
