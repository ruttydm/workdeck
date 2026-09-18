//! Compiled native parity fixture for Hunk's AppHost keybinding lifecycle tests.

use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration, ExtensionHostAction,
    HandshakeRequest, HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse,
    Registration, ReviewEvent,
};

pub const EXTENSION_ID: &str = "coach";

pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut log_path = None;
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if value.get("id").is_none() {
            if value.get("method").and_then(Value::as_str) == Some("workdeck/shutdown") {
                if let Some(path) = log_path.as_deref() {
                    append_log(path, "shutdown")?;
                }
                return Ok(());
            }
            continue;
        }
        let request: JsonRpcRequest = serde_json::from_value(value).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => {
                let handshake: HandshakeRequest = parse(&request)?;
                log_path = handshake
                    .config
                    .get("logPath")
                    .and_then(Value::as_str)
                    .map(PathBuf::from);
                to_value(HandshakeResponse {
                    extension_api_version: API_VERSION,
                    extension_version: env!("CARGO_PKG_VERSION").into(),
                    registrations: vec![
                        Registration::Command(CommandRegistration {
                            id: "toggle-lines".into(),
                            title: "Toggle lines".into(),
                            description: None,
                            default_keys: vec!["y".into()],
                        }),
                        Registration::EventSubscription {
                            names: vec!["command_executed".into(), "shutdown".into()],
                        },
                    ],
                })
            }
            "workdeck/command/invoke" => {
                let invocation: CommandInvocation = parse(&request)?;
                if invocation.command_id != "toggle-lines" {
                    Err(io::Error::other(format!(
                        "Unknown command: {}",
                        invocation.command_id
                    )))
                } else {
                    to_value(CommandExecution {
                        actions: vec![ExtensionHostAction::ExecuteReviewCommand {
                            id: "workdeck.view.toggleLineNumbers".into(),
                            count: None,
                        }],
                    })
                }
            }
            "workdeck/event" => {
                let event: ReviewEvent = parse(&request)?;
                if event.name == "command_executed"
                    && let Some(path) = log_path.as_deref()
                {
                    let command_id = event
                        .payload
                        .get("commandId")
                        .and_then(Value::as_str)
                        .ok_or_else(|| io::Error::other("command event omitted commandId"))?;
                    append_log(path, &format!("command:{command_id}"))?;
                }
                to_value(CommandExecution::default())
            }
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        write_response(&mut output, request.id, result)?;
    }
}

fn append_log(path: &Path, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(file, "{line}")
}

fn parse<T: serde::de::DeserializeOwned>(request: &JsonRpcRequest) -> io::Result<T> {
    serde_json::from_value(request.params.clone()).map_err(io::Error::other)
}

fn to_value(value: impl Serialize) -> io::Result<Value> {
    serde_json::to_value(value).map_err(io::Error::other)
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
