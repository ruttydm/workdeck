//! Compiled native port of Hunk's authoritative saved-note navigator.

use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionHostAction, ExtensionNotifyType, ExtensionReviewNoteResolution,
    ExtensionReviewSnapshot, ExtensionReviewSnapshotFile, ExtensionReviewSnapshotNote,
    HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Registration,
    SelectDialogSubmission,
};

const DIALOG_ID: &str = "saved-review-note";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewNoteChoice {
    pub label: String,
    pub note_id: String,
}

#[derive(Debug, Default)]
pub struct ReviewNoteNavigatorState {
    choices: Vec<ReviewNoteChoice>,
}

/// Resolve a host-sanitized dialog answer through the unique ordinal prefix we own.
#[must_use]
pub fn selected_review_note_choice<'a>(
    choices: &'a [ReviewNoteChoice],
    selected: &str,
) -> Option<&'a ReviewNoteChoice> {
    let (ordinal, suffix) = selected.split_once('.')?;
    if ordinal.is_empty()
        || !ordinal.bytes().all(|byte| byte.is_ascii_digit())
        || !suffix.chars().next().is_some_and(char::is_whitespace)
    {
        return None;
    }
    let index = ordinal.parse::<usize>().ok()?.checked_sub(1)?;
    choices.get(index)
}

fn one_line_summary(summary: &str) -> String {
    let summary = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    if summary.is_empty() {
        "Untitled note".into()
    } else {
        summary
    }
}

fn resolution_label(resolution: ExtensionReviewNoteResolution) -> &'static str {
    match resolution {
        ExtensionReviewNoteResolution::Active => "active",
        ExtensionReviewNoteResolution::Stale => "stale",
        ExtensionReviewNoteResolution::Orphaned => "orphaned",
    }
}

/// Build unique selector choices in authoritative saved-note order.
#[must_use]
pub fn build_review_note_choices(snapshot: &ExtensionReviewSnapshot) -> Vec<ReviewNoteChoice> {
    snapshot
        .notes
        .iter()
        .enumerate()
        .map(|(index, note)| {
            let path = snapshot
                .files
                .iter()
                .find(|file| file.file_key == note.file_key)
                .map_or_else(
                    || format!("retired file {}", note.file_key),
                    |file| file.path.clone(),
                );
            let location = note.anchor.preferred.map_or_else(String::new, |preferred| {
                let side = match preferred.side {
                    workdeck_core::ReviewSide::Old => "old",
                    workdeck_core::ReviewSide::New => "new",
                };
                format!(":{} ({side})", preferred.line)
            });
            ReviewNoteChoice {
                label: format!(
                    "{}. [{}] {path}{location} — {}",
                    index + 1,
                    resolution_label(note.resolution),
                    one_line_summary(&note.summary)
                ),
                note_id: note.id.clone(),
            }
        })
        .collect()
}

/// Preserve the authoritative hunk fallback while requesting a note's exact line.
#[must_use]
pub fn navigate_to_saved_review_note(
    file: &ExtensionReviewSnapshotFile,
    note: &ExtensionReviewSnapshotNote,
) -> Vec<ExtensionHostAction> {
    let mut actions = Vec::new();
    if let Some(hunk_index) = note.anchor.owner_hunk_index {
        actions.push(ExtensionHostAction::SelectReviewHunk {
            file_id: file.runtime_id.clone(),
            hunk_index,
        });
    }
    if let Some(preferred) = note.anchor.preferred {
        actions.push(ExtensionHostAction::RevealReviewLine {
            file_id: file.runtime_id.clone(),
            side: preferred.side,
            line: preferred.line,
        });
    } else if note.anchor.owner_hunk_index.is_none() {
        actions.push(ExtensionHostAction::SelectReviewFile {
            file_id: file.runtime_id.clone(),
        });
    }
    actions
}

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![Registration::Command(CommandRegistration {
        id: "navigate".into(),
        title: "Navigate saved review note…".into(),
        description: None,
        default_keys: vec!["f8".into()],
    })]
}

fn invoke_navigate(
    invocation: &CommandInvocation,
    state: &mut ReviewNoteNavigatorState,
) -> Result<CommandExecution, String> {
    if invocation.command_id != "navigate" {
        return Err(format!("Unknown command: {}", invocation.command_id));
    }
    let Some(snapshot) = invocation.review.as_ref() else {
        return Ok(notify(
            "The current review is unavailable to this command",
            ExtensionNotifyType::Warning,
        ));
    };
    let choices = build_review_note_choices(snapshot);
    if choices.is_empty() {
        return Ok(notify(
            "This review has no saved notes",
            ExtensionNotifyType::Info,
        ));
    }
    let options = choices.iter().map(|choice| choice.label.clone()).collect();
    state.choices = choices;
    Ok(CommandExecution {
        actions: vec![ExtensionHostAction::OpenSelectDialog {
            id: DIALOG_ID.into(),
            title: "Navigate saved review note".into(),
            options,
        }],
    })
}

fn submit_selection(
    submission: &SelectDialogSubmission,
    state: &mut ReviewNoteNavigatorState,
) -> CommandExecution {
    if submission.action_id != DIALOG_ID {
        return CommandExecution::default();
    }
    let choices = std::mem::take(&mut state.choices);
    let Some(selected) = submission.value.as_deref() else {
        return CommandExecution::default();
    };
    let Some(choice) = selected_review_note_choice(&choices, selected) else {
        return CommandExecution::default();
    };
    let Some(current) = submission.review.as_ref() else {
        return notify(
            "The review changed; open the note navigator again",
            ExtensionNotifyType::Warning,
        );
    };
    let Some(note) = current
        .notes
        .iter()
        .find(|candidate| candidate.id == choice.note_id)
    else {
        return notify(
            "That saved note no longer exists",
            ExtensionNotifyType::Warning,
        );
    };
    if note.resolution == ExtensionReviewNoteResolution::Orphaned {
        return notify(
            "That note is orphaned and has no current review location",
            ExtensionNotifyType::Warning,
        );
    }
    let Some(file) = current
        .files
        .iter()
        .find(|candidate| candidate.file_key == note.file_key)
    else {
        return notify(
            "That note's file is no longer in the review",
            ExtensionNotifyType::Warning,
        );
    };
    CommandExecution {
        actions: navigate_to_saved_review_note(file, note),
    }
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
    let mut state = ReviewNoteNavigatorState::default();
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
                invoke_navigate(&invocation, &mut state)
                    .and_then(|value| {
                        serde_json::to_value(value).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/dialog/select" => {
                let submission: SelectDialogSubmission =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                serde_json::to_value(submit_selection(&submission, &mut state))
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
        Capability::ReviewNavigation,
    ]
}
