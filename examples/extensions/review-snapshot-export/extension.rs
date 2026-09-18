//! Compiled native port of Hunk's authoritative review-snapshot exporter.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::{Component, Path, PathBuf};
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionHostAction, ExtensionNotifyType, ExtensionReviewSnapshot, HandshakeResponse,
    InputDialogSubmission, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Registration,
};

const DIALOG_ID: &str = "snapshot-export-path";

#[derive(Debug, Default)]
pub struct ReviewSnapshotExportState {
    captured: Option<ExtensionReviewSnapshot>,
}

/// Resolve one user-entered path from the command working directory.
#[must_use]
pub fn resolve_snapshot_export_path(cwd: &Path, input: &str) -> PathBuf {
    let input = Path::new(input.trim());
    clean_path(if input.is_absolute() {
        input.to_path_buf()
    } else {
        cwd.join(input)
    })
}

fn clean_path(path: PathBuf) -> PathBuf {
    let mut result = PathBuf::new();
    let mut rooted = false;
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                let can_pop_normal = result
                    .components()
                    .next_back()
                    .is_some_and(|component| matches!(component, Component::Normal(_)));
                if can_pop_normal {
                    result.pop();
                } else if !rooted {
                    result.push(component.as_os_str());
                }
            }
            Component::Prefix(_) => result.push(component.as_os_str()),
            Component::RootDir => {
                rooted = true;
                result.push(component.as_os_str());
            }
            Component::Normal(_) => {
                result.push(component.as_os_str());
            }
        }
    }
    result
}

/// Async work is current only at the exact generation and store revision it captured.
#[must_use]
pub fn snapshot_position_matches(
    captured: &ExtensionReviewSnapshot,
    current: Option<&ExtensionReviewSnapshot>,
) -> bool {
    current.is_some_and(|current| {
        current.generation == captured.generation
            && current.state_revision == captured.state_revision
    })
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![Registration::Command(CommandRegistration {
        id: "export".into(),
        title: "Export review snapshot…".into(),
        description: None,
        default_keys: vec!["f9".into()],
    })]
}

fn invoke_export(
    invocation: &CommandInvocation,
    state: &mut ReviewSnapshotExportState,
) -> Result<CommandExecution, String> {
    if invocation.command_id != "export" {
        return Err(format!("Unknown command: {}", invocation.command_id));
    }
    let Some(captured) = invocation.review.clone() else {
        return Ok(notify(
            "The current review is unavailable to this command",
            ExtensionNotifyType::Warning,
        ));
    };
    state.captured = Some(captured);
    Ok(CommandExecution {
        actions: vec![ExtensionHostAction::OpenInputDialog {
            id: DIALOG_ID.into(),
            title: "Export review snapshot".into(),
            placeholder: "workdeck-review-snapshot.json".into(),
            initial: None,
        }],
    })
}

fn submit_export(
    submission: &InputDialogSubmission,
    state: &mut ReviewSnapshotExportState,
) -> Result<CommandExecution, String> {
    if submission.action_id != DIALOG_ID {
        return Ok(CommandExecution::default());
    }
    let Some(captured) = state.captured.take() else {
        return Ok(CommandExecution::default());
    };
    let Some(input) = submission.value.as_deref() else {
        return Ok(CommandExecution::default());
    };
    if input.trim().is_empty() {
        return Ok(CommandExecution::default());
    }
    if !snapshot_position_matches(&captured, submission.review.as_ref()) {
        return Ok(notify(
            "The review changed while exporting; run the command again",
            ExtensionNotifyType::Warning,
        ));
    }

    let output_path = resolve_snapshot_export_path(&submission.cwd, input);
    let mut output = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
    {
        Ok(output) => output,
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            return Ok(notify(
                format!(
                    "Refusing to overwrite existing file {}",
                    output_path.display()
                ),
                ExtensionNotifyType::Warning,
            ));
        }
        Err(error) => {
            return Err(format!(
                "could not create review snapshot {}: {error}",
                output_path.display()
            ));
        }
    };
    let encoded = serde_json::to_string_pretty(&captured).map_err(|error| error.to_string())?;
    output
        .write_all(encoded.as_bytes())
        .and_then(|()| output.write_all(b"\n"))
        .and_then(|()| output.flush())
        .map_err(|error| {
            format!(
                "could not write review snapshot {}: {error}",
                output_path.display()
            )
        })?;
    let note_count = captured.notes.len();
    Ok(notify(
        format!(
            "Exported {note_count} saved {} to {}",
            if note_count == 1 { "note" } else { "notes" },
            output_path.display()
        ),
        ExtensionNotifyType::Info,
    ))
}

fn notify(message: impl Into<String>, notification_type: ExtensionNotifyType) -> CommandExecution {
    CommandExecution {
        actions: vec![ExtensionHostAction::Notify {
            message: message.into(),
            notification_type,
        }],
    }
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut state = ReviewSnapshotExportState::default();
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
                invoke_export(&invocation, &mut state)
                    .and_then(|value| {
                        serde_json::to_value(value).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/dialog/input" => {
                let submission: InputDialogSubmission =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                submit_export(&submission, &mut state)
                    .and_then(|value| {
                        serde_json::to_value(value).map_err(|error| error.to_string())
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
    vec![
        Capability::Commands,
        Capability::Dialogs,
        Capability::Notifications,
    ]
}
