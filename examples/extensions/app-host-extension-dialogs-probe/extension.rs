//! Native subprocess fixture for Hunk's mounted AppHost dialog tests.

use std::io::{self, BufRead, Write};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration, ConfirmDialogSubmission,
    ExtensionHostAction, ExtensionNotifyType, HandshakeRequest, HandshakeResponse,
    InputDialogSubmission, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Registration,
    ReviewEvent, SelectDialogSubmission,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Scenario {
    #[default]
    Confirm,
    ShortConfirm,
    EscapeConfirm,
    Select,
    Input,
    ReloadCancel,
    ReloadLifecycle,
}

impl Scenario {
    fn parse(value: Option<&str>) -> io::Result<Self> {
        match value.unwrap_or("confirm") {
            "confirm" => Ok(Self::Confirm),
            "short-confirm" => Ok(Self::ShortConfirm),
            "escape-confirm" => Ok(Self::EscapeConfirm),
            "select" => Ok(Self::Select),
            "input" => Ok(Self::Input),
            "reload-cancel" => Ok(Self::ReloadCancel),
            "reload-lifecycle" => Ok(Self::ReloadLifecycle),
            value => Err(io::Error::other(format!("Unknown scenario: {value}"))),
        }
    }
}

fn registrations() -> Vec<Registration> {
    vec![
        Registration::Command(CommandRegistration {
            id: "ask".into(),
            title: "Ask".into(),
            description: None,
            default_keys: vec!["y".into()],
        }),
        Registration::EventSubscription {
            names: vec!["session_reload".into()],
        },
    ]
}

fn confirm(id: &str, title: &str, body: &str, confirm_label: &str) -> ExtensionHostAction {
    ExtensionHostAction::OpenConfirmDialog {
        id: id.into(),
        title: title.into(),
        body: body.into(),
        confirm_label: confirm_label.into(),
        cancel_label: None,
    }
}

fn invoke_command(
    invocation: CommandInvocation,
    scenario: Scenario,
) -> io::Result<CommandExecution> {
    if invocation.command_id != "ask" {
        return Err(io::Error::other(format!(
            "Unknown command: {}",
            invocation.command_id
        )));
    }
    let action = match scenario {
        Scenario::Confirm => confirm(
            "confirm",
            "Reformat the file?",
            "This rewrites it in place.",
            "reformat",
        ),
        Scenario::ShortConfirm => confirm(
            "short-confirm",
            "Short terminal",
            "This deliberately long explanation wraps across many rows but must never displace the primary actions from the modal footer.",
            "confirm",
        ),
        Scenario::EscapeConfirm => confirm("escape-confirm", "Discard the draft?", "", "confirm"),
        Scenario::Select => ExtensionHostAction::OpenSelectDialog {
            id: "select".into(),
            title: "Where to?".into(),
            options: vec!["staging".into(), "production".into(), "canary".into()],
        },
        Scenario::Input => ExtensionHostAction::OpenInputDialog {
            id: "input".into(),
            title: "Branch name?".into(),
            placeholder: "feature/...".into(),
            initial: None,
        },
        Scenario::ReloadCancel => confirm("reload-cancel", "Still relevant?", "", "confirm"),
        Scenario::ReloadLifecycle => {
            return Ok(CommandExecution::default());
        }
    };
    Ok(CommandExecution {
        actions: vec![action],
    })
}

fn answer(message: String) -> CommandExecution {
    CommandExecution {
        actions: vec![ExtensionHostAction::Notify {
            message,
            notification_type: ExtensionNotifyType::Info,
        }],
    }
}

fn submit_confirm(submission: ConfirmDialogSubmission) -> CommandExecution {
    answer(format!("answer {}", submission.confirmed))
}

fn submit_select(submission: SelectDialogSubmission) -> CommandExecution {
    answer(format!(
        "answer {}",
        submission.value.as_deref().unwrap_or("null")
    ))
}

fn submit_input(submission: InputDialogSubmission) -> CommandExecution {
    answer(format!(
        "answer {}",
        submission.value.as_deref().unwrap_or("null")
    ))
}

fn handle_event(event: ReviewEvent, scenario: Scenario) -> CommandExecution {
    if scenario == Scenario::ReloadLifecycle && event.name == "session_reload" {
        return CommandExecution {
            actions: vec![confirm(
                "reload-lifecycle",
                "Review reloaded",
                "",
                "confirm",
            )],
        };
    }
    CommandExecution::default()
}

fn value(value: impl Serialize) -> io::Result<Value> {
    serde_json::to_value(value).map_err(io::Error::other)
}

fn parse<T: DeserializeOwned>(request: &JsonRpcRequest) -> io::Result<T> {
    serde_json::from_value(request.params.clone()).map_err(io::Error::other)
}

fn write_response(output: &mut impl Write, id: u64, result: io::Result<Value>) -> io::Result<()> {
    let response = match result {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        },
        Err(error) => JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32_000,
                message: error.to_string(),
                data: None,
            }),
        },
    };
    serde_json::to_writer(&mut *output, &response).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

/// Serve one configured dialog scenario over JSON-RPC 2.0 JSONL.
pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut scenario = Scenario::default();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => parse::<HandshakeRequest>(&request).and_then(|handshake| {
                scenario =
                    Scenario::parse(handshake.config.get("scenario").and_then(Value::as_str))?;
                value(HandshakeResponse {
                    extension_api_version: API_VERSION,
                    extension_version: env!("CARGO_PKG_VERSION").into(),
                    registrations: registrations(),
                })
            }),
            "workdeck/command/invoke" => parse(&request)
                .and_then(|invocation| invoke_command(invocation, scenario))
                .and_then(value),
            "workdeck/dialog/confirm" => parse(&request).map(submit_confirm).and_then(value),
            "workdeck/dialog/select" => parse(&request).map(submit_select).and_then(value),
            "workdeck/dialog/input" => parse(&request).map(submit_input).and_then(value),
            "workdeck/event" => parse(&request)
                .map(|event| handle_event(event, scenario))
                .and_then(value),
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        write_response(&mut output, request.id, result)?;
    }
}
