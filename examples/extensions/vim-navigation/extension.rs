//! Compiled native port of Hunk's Vim-navigation extension example.

#[path = "state.rs"]
mod state;

use self::state::{VimCommandResult, VimNavigationState, execute_vim_command};
use std::io::{self, BufRead, Write};
use workdeck_extension_api::{
    API_VERSION, Capability, CommandExecution, CommandInvocation, CommandRegistration,
    ExtensionHostAction, ExtensionNotifyType, HandshakeResponse, InputDialogSubmission,
    JsonRpcError, JsonRpcRequest, JsonRpcResponse, KeyboardModeExecution, KeyboardModeKeyRequest,
    KeyboardModeLifecycleRequest, KeyboardModeRegistration, Registration,
};

const MODE_ID: &str = "normal";
const QUALIFIED_MODE_ID: &str = "example.vim-navigation:normal";

#[must_use]
pub fn registrations() -> Vec<Registration> {
    vec![
        Registration::KeyboardMode(KeyboardModeRegistration {
            id: MODE_ID.into(),
            title: "Vim navigation".into(),
        }),
        Registration::Command(CommandRegistration {
            id: "command-line".into(),
            title: "Open Vim command line".into(),
            description: None,
            default_keys: vec![":".into()],
        }),
        Registration::Command(CommandRegistration {
            id: "toggle".into(),
            title: "Toggle Vim navigation".into(),
            description: None,
            default_keys: vec!["f6".into()],
        }),
    ]
}

fn invoke_command(invocation: &CommandInvocation) -> Result<CommandExecution, String> {
    let active = invocation.active_keyboard_mode.as_deref() == Some(QUALIFIED_MODE_ID);
    let actions = match invocation.command_id.as_str() {
        "toggle" if active => vec![ExtensionHostAction::ExitKeyboardMode],
        "toggle" => vec![ExtensionHostAction::EnterKeyboardMode { id: MODE_ID.into() }],
        "command-line" if active => vec![ExtensionHostAction::OpenInputDialog {
            id: "vim-command".into(),
            title: "Vim command (:)".into(),
            placeholder: "top or bottom".into(),
            initial: None,
        }],
        "command-line" => vec![ExtensionHostAction::Notify {
            message: "Enter Vim navigation before opening its command line".into(),
            notification_type: ExtensionNotifyType::Info,
        }],
        command => return Err(format!("Unknown command: {command}")),
    };
    Ok(CommandExecution { actions })
}

fn lifecycle(
    request: &KeyboardModeLifecycleRequest,
    state: &mut VimNavigationState,
    entering: bool,
) -> Result<CommandExecution, String> {
    if request.mode_id != MODE_ID {
        return Err(format!("Unknown keyboard mode: {}", request.mode_id));
    }
    state.reset();
    Ok(CommandExecution {
        actions: entering
            .then(|| ExtensionHostAction::ExecuteReviewCommand {
                id: "workdeck.view.cursor-line-row".into(),
                count: None,
            })
            .into_iter()
            .collect(),
    })
}

fn route_key(
    request: &KeyboardModeKeyRequest,
    state: &mut VimNavigationState,
) -> Result<KeyboardModeExecution, String> {
    if request.mode_id != MODE_ID {
        return Err(format!("Unknown keyboard mode: {}", request.mode_id));
    }
    let (result, actions) = state.handle_key(&request.key);
    Ok(KeyboardModeExecution { result, actions })
}

fn submit_input(submission: &InputDialogSubmission) -> CommandExecution {
    let Some(input) = submission
        .value
        .as_deref()
        .filter(|_| submission.active_keyboard_mode.as_deref() == Some(QUALIFIED_MODE_ID))
    else {
        return CommandExecution::default();
    };
    if submission.action_id != "vim-command" {
        return CommandExecution::default();
    }
    let (result, mut actions) = execute_vim_command(input);
    if result == VimCommandResult::Unknown {
        actions.push(ExtensionHostAction::Notify {
            message: format!("Unknown Vim command \"{}\"", input.trim()),
            notification_type: ExtensionNotifyType::Warning,
        });
    }
    CommandExecution { actions }
}

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut navigation = VimNavigationState::default();
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
                    .and_then(|value| {
                        serde_json::to_value(value).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/keyboard-mode/enter" | "workdeck/keyboard-mode/exit" => {
                let entering = request.method.ends_with("enter");
                let lifecycle_request: KeyboardModeLifecycleRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                lifecycle(&lifecycle_request, &mut navigation, entering)
                    .and_then(|value| {
                        serde_json::to_value(value).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/keyboard-mode/key" => {
                let key_request: KeyboardModeKeyRequest =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                route_key(&key_request, &mut navigation)
                    .and_then(|value| {
                        serde_json::to_value(value).map_err(|error| error.to_string())
                    })
                    .map_err(io::Error::other)
            }
            "workdeck/dialog/input" => {
                let submission: InputDialogSubmission =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                serde_json::to_value(submit_input(&submission)).map_err(io::Error::other)
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
        Capability::KeyboardModes,
        Capability::ReviewNavigation,
        Capability::Dialogs,
        Capability::Notifications,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{Changeset, ChangesetSource, ReviewSelection, ReviewSnapshot};

    fn invocation(id: &str, active: bool) -> CommandInvocation {
        CommandInvocation {
            command_id: id.into(),
            snapshot: ReviewSnapshot {
                generation: 0,
                changeset: Changeset {
                    id: "test".into(),
                    title: "test".into(),
                    source: ChangesetSource::Patch {
                        label: "test".into(),
                    },
                    files: Vec::new(),
                },
                selection: ReviewSelection::default(),
            },
            cwd: std::path::PathBuf::new(),
            review: None,
            open_panes: Vec::new(),
            active_keyboard_mode: active.then(|| QUALIFIED_MODE_ID.into()),
            workspace: None,
        }
    }

    #[test]
    fn toggle_and_command_line_follow_active_mode_context() {
        assert!(matches!(
            invoke_command(&invocation("toggle", false))
                .unwrap()
                .actions[0],
            ExtensionHostAction::EnterKeyboardMode { .. }
        ));
        assert!(matches!(
            invoke_command(&invocation("toggle", true)).unwrap().actions[0],
            ExtensionHostAction::ExitKeyboardMode
        ));
        assert!(matches!(
            invoke_command(&invocation("command-line", false))
                .unwrap()
                .actions[0],
            ExtensionHostAction::Notify { .. }
        ));
        assert!(matches!(
            invoke_command(&invocation("command-line", true))
                .unwrap()
                .actions[0],
            ExtensionHostAction::OpenInputDialog { .. }
        ));
    }

    #[test]
    fn cancelled_or_inactive_dialog_submissions_do_nothing() {
        let snapshot = invocation("toggle", true).snapshot;
        let submission = InputDialogSubmission {
            action_id: "vim-command".into(),
            value: None,
            snapshot: snapshot.clone(),
            cwd: std::path::PathBuf::new(),
            review: None,
            active_keyboard_mode: Some(QUALIFIED_MODE_ID.into()),
        };
        assert!(submit_input(&submission).actions.is_empty());
        assert!(
            submit_input(&InputDialogSubmission {
                value: Some("top".into()),
                active_keyboard_mode: None,
                ..submission
            })
            .actions
            .is_empty()
        );
    }
}
