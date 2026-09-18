//! Native subprocess fixture for Hunk's mounted AppHost workspace tests.

use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::time::Duration;

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use workdeck_extension_api::{
    API_VERSION, CommandExecution, CommandInvocation, CommandRegistration, ExtensionFileSide,
    ExtensionHostAction, ExtensionNotifyType, ExtensionWorkspaceReadCompletion,
    ExtensionWorkspaceWriteCompletion, HandshakeRequest, HandshakeResponse, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, Registration,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Scenario {
    #[default]
    Read,
    DeferredRead,
    ReadWrite,
    Write,
}

impl Scenario {
    fn parse(value: Option<&str>) -> io::Result<Self> {
        match value.unwrap_or("read") {
            "read" => Ok(Self::Read),
            "deferred-read" => Ok(Self::DeferredRead),
            "read-write" => Ok(Self::ReadWrite),
            "write" => Ok(Self::Write),
            value => Err(io::Error::other(format!("Unknown scenario: {value}"))),
        }
    }
}

#[derive(Debug, Default)]
struct Probe {
    scenario: Scenario,
    can_write: bool,
    file_id: String,
    reads: BTreeMap<String, Option<String>>,
}

impl Probe {
    fn invoke(&mut self, invocation: CommandInvocation) -> io::Result<CommandExecution> {
        if invocation.command_id != "probe" {
            return Err(io::Error::other(format!(
                "Unknown command: {}",
                invocation.command_id
            )));
        }
        let file_id = invocation
            .selection
            .file
            .as_ref()
            .map(|file| file.id.clone())
            .ok_or_else(|| io::Error::other("The review selection has no file"))?;
        self.can_write = invocation
            .workspace
            .as_ref()
            .is_some_and(|workspace| workspace.can_write_document(&file_id));
        self.file_id.clone_from(&file_id);
        self.reads.clear();

        let actions = match self.scenario {
            Scenario::Read => vec![
                read("new", &file_id, ExtensionFileSide::New),
                read("old", &file_id, ExtensionFileSide::Old),
                read("unknown", "no-such-file", ExtensionFileSide::New),
            ],
            Scenario::DeferredRead => {
                std::thread::sleep(Duration::from_millis(150));
                vec![read("deferred", &file_id, ExtensionFileSide::New)]
            }
            Scenario::ReadWrite => vec![read("transform", &file_id, ExtensionFileSide::New)],
            Scenario::Write => vec![
                notify(format!("can {}", self.can_write)),
                ExtensionHostAction::RequestWorkspaceWrite {
                    request_id: "write".into(),
                    file_id,
                    text: "rewritten\n".into(),
                },
            ],
        };
        Ok(CommandExecution { actions })
    }

    fn complete_read(&mut self, completion: ExtensionWorkspaceReadCompletion) -> CommandExecution {
        match self.scenario {
            Scenario::Read => {
                self.reads
                    .insert(completion.request_id.clone(), completion.value);
                if self.reads.len() != 3 {
                    return CommandExecution::default();
                }
                let encoded = |key: &str| {
                    serde_json::to_string(&self.reads.get(key).and_then(Option::as_deref)).unwrap()
                };
                execution(notify(format!(
                    "reads new={} old={} unknown={} can={}",
                    encoded("new"),
                    encoded("old"),
                    encoded("unknown"),
                    self.can_write
                )))
            }
            Scenario::DeferredRead => execution(notify(format!(
                "read {}",
                serde_json::to_string(&completion.value).unwrap()
            ))),
            Scenario::ReadWrite => completion.value.map_or_else(
                || execution(notify("read null".into())),
                |text| {
                    execution(ExtensionHostAction::RequestWorkspaceWrite {
                        request_id: "transform-write".into(),
                        file_id: self.file_id.clone(),
                        text: text.to_uppercase(),
                    })
                },
            ),
            Scenario::Write => CommandExecution::default(),
        }
    }

    fn complete_write(&self, completion: ExtensionWorkspaceWriteCompletion) -> CommandExecution {
        execution(notify(format!(
            "result {}",
            serde_json::to_string(&completion.result).unwrap()
        )))
    }
}

fn execution(action: ExtensionHostAction) -> CommandExecution {
    CommandExecution {
        actions: vec![action],
    }
}

fn notify(message: String) -> ExtensionHostAction {
    ExtensionHostAction::Notify {
        message,
        notification_type: ExtensionNotifyType::Info,
    }
}

fn read(request_id: &str, file_id: &str, side: ExtensionFileSide) -> ExtensionHostAction {
    ExtensionHostAction::RequestWorkspaceRead {
        request_id: request_id.into(),
        file_id: file_id.into(),
        side,
    }
}

fn registrations() -> Vec<Registration> {
    vec![Registration::Command(CommandRegistration {
        id: "probe".into(),
        title: "Probe workspace".into(),
        description: None,
        default_keys: vec!["y".into()],
    })]
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

/// Serve one configured workspace scenario over JSON-RPC 2.0 JSONL.
pub fn serve<R: BufRead, W: Write>(mut input: R, mut output: W) -> io::Result<()> {
    let mut probe = Probe::default();
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let request: JsonRpcRequest = serde_json::from_str(&line).map_err(io::Error::other)?;
        let result = match request.method.as_str() {
            "workdeck/handshake" => parse::<HandshakeRequest>(&request).and_then(|handshake| {
                probe.scenario =
                    Scenario::parse(handshake.config.get("scenario").and_then(Value::as_str))?;
                value(HandshakeResponse {
                    extension_api_version: API_VERSION,
                    extension_version: env!("CARGO_PKG_VERSION").into(),
                    registrations: registrations(),
                })
            }),
            "workdeck/command/invoke" => parse(&request)
                .and_then(|invocation| probe.invoke(invocation))
                .and_then(value),
            "workdeck/workspace/read-complete" => parse(&request)
                .map(|completion| probe.complete_read(completion))
                .and_then(value),
            "workdeck/workspace/write-complete" => parse(&request)
                .map(|completion| probe.complete_write(completion))
                .and_then(value),
            method => Err(io::Error::other(format!("Unknown method: {method}"))),
        };
        write_response(&mut output, request.id, result)?;
    }
}
