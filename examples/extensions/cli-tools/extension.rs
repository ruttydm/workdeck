//! Compiled native port of Hunk's CLI-tools extension example.

use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use workdeck_extension_api::{
    API_VERSION, Capability, CliCommandExecution, CliCommandInvocation, CliCommandRegistration,
    CliCommandResult, CliOutputNotification, CliOutputStream, HandshakeResponse, JsonRpcError,
    JsonRpcRequest, JsonRpcResponse, Registration,
};

const COMMAND_NAME: &str = "cli-tools";
const SUMMARY: &str = "Demonstrate extension-provided CLI workflows";
const USAGE: &str = "<status|review> [args...]";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliToolsUserError {
    pub message: String,
    pub suggestions: Vec<String>,
}

/// Wait for the same 100 ms preparation window while remaining cancellation-aware.
pub fn prepare_review(cancelled: &AtomicBool) -> Result<(), CliToolsUserError> {
    let deadline = Instant::now() + Duration::from_millis(100);
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err(CliToolsUserError {
                message: "Extension CLI command interrupted.".into(),
                suggestions: Vec::new(),
            });
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(());
        }
        thread::park_timeout(remaining.min(Duration::from_millis(2)));
    }
}

/// Execute the extension's status/review command tree against host-owned output callbacks.
pub fn execute_cli_tools(
    invocation: &CliCommandInvocation,
    cancelled: &AtomicBool,
    mut emit: impl FnMut(CliOutputStream, &[u8]) -> io::Result<()>,
) -> Result<CliCommandExecution, CliToolsUserError> {
    let Some((action, rest)) = invocation.args.split_first() else {
        return Err(choose_action_error());
    };
    match action.as_str() {
        "status" => {
            emit(
                CliOutputStream::Stdout,
                format!("cli-tools is ready in {}\n", invocation.cwd.display()).as_bytes(),
            )
            .map_err(output_error)?;
            Ok(CliCommandExecution {
                result: CliCommandResult::Exit { code: 0 },
                stdin_read_started: false,
                stdin_consumed: false,
            })
        }
        "review" => {
            emit(
                CliOutputStream::Stderr,
                "Preparing review input…\n".as_bytes(),
            )
            .map_err(output_error)?;
            prepare_review(cancelled)?;
            Ok(CliCommandExecution {
                result: CliCommandResult::Delegate {
                    argv: std::iter::once("diff".into())
                        .chain(rest.iter().cloned())
                        .collect(),
                },
                stdin_read_started: false,
                stdin_consumed: false,
            })
        }
        _ => Err(choose_action_error()),
    }
}

fn choose_action_error() -> CliToolsUserError {
    CliToolsUserError {
        message: "Choose a cli-tools action.".into(),
        suggestions: vec!["Run `workdeck cli-tools status` or `workdeck cli-tools review`.".into()],
    }
}

fn output_error(error: io::Error) -> CliToolsUserError {
    CliToolsUserError {
        message: format!("CLI output failed: {error}"),
        suggestions: Vec::new(),
    }
}

type SharedWriter<W> = Arc<Mutex<W>>;
type ActiveRequest = Arc<Mutex<Option<(u64, Arc<AtomicBool>)>>>;

/// Serve the newline-delimited JSON-RPC extension protocol until the host closes stdin.
pub fn serve<R, W>(mut input: R, output: W) -> io::Result<()>
where
    R: BufRead,
    W: Write + Send + 'static,
{
    let output = Arc::new(Mutex::new(output));
    let active: ActiveRequest = Arc::new(Mutex::new(None));
    loop {
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            return Ok(());
        }
        let value: Value = serde_json::from_str(&line).map_err(io::Error::other)?;
        if value.get("id").is_none() {
            handle_notification(&value, &active);
            continue;
        }
        let request: JsonRpcRequest = serde_json::from_value(value).map_err(io::Error::other)?;
        match request.method.as_str() {
            "workdeck/handshake" => write_result(
                &output,
                request.id,
                &HandshakeResponse {
                    extension_api_version: API_VERSION,
                    extension_version: env!("CARGO_PKG_VERSION").into(),
                    registrations: vec![Registration::CliCommand(CliCommandRegistration {
                        name: COMMAND_NAME.into(),
                        summary: SUMMARY.into(),
                        usage: Some(USAGE.into()),
                    })],
                },
            )?,
            "workdeck/cli/invoke" => {
                let invocation: CliCommandInvocation =
                    serde_json::from_value(request.params).map_err(io::Error::other)?;
                if invocation.command_name != COMMAND_NAME {
                    write_error(
                        &output,
                        request.id,
                        -32601,
                        format!("Unknown CLI command: {}", invocation.command_name),
                        None,
                    )?;
                    continue;
                }
                let cancelled = Arc::new(AtomicBool::new(false));
                *active
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) =
                    Some((request.id, Arc::clone(&cancelled)));
                let output = Arc::clone(&output);
                let active = Arc::clone(&active);
                thread::spawn(move || {
                    let result = execute_cli_tools(&invocation, &cancelled, |stream, bytes| {
                        write_cli_output(&output, request.id, stream, bytes)
                    });
                    match result {
                        Ok(execution) => {
                            let _ = write_result(&output, request.id, &execution);
                        }
                        Err(error) => {
                            let data = (!error.suggestions.is_empty())
                                .then(|| json!({ "suggestions": error.suggestions }));
                            let _ = write_error(&output, request.id, -32000, error.message, data);
                        }
                    }
                    let mut current = active
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if current.as_ref().is_some_and(|(id, _)| *id == request.id) {
                        *current = None;
                    }
                });
            }
            _ => write_error(
                &output,
                request.id,
                -32601,
                format!("Unknown method: {}", request.method),
                None,
            )?,
        }
    }
}

fn handle_notification(value: &Value, active: &ActiveRequest) {
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || value.get("method").and_then(Value::as_str) != Some("$/cancelRequest")
    {
        return;
    }
    let Some(id) = value
        .get("params")
        .and_then(|params| params.get("id"))
        .and_then(Value::as_u64)
    else {
        return;
    };
    if let Some((active_id, cancelled)) = active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        && *active_id == id
    {
        cancelled.store(true, Ordering::Release);
    }
}

fn write_cli_output<W: Write>(
    output: &SharedWriter<W>,
    request_id: u64,
    stream: CliOutputStream,
    bytes: &[u8],
) -> io::Result<()> {
    write_line(
        output,
        &json!({
            "jsonrpc": "2.0",
            "method": "workdeck/cli/output",
            "params": CliOutputNotification { request_id, stream, bytes: bytes.to_vec() },
        }),
    )
}

fn write_result<W: Write>(
    output: &SharedWriter<W>,
    id: u64,
    result: &impl serde::Serialize,
) -> io::Result<()> {
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

fn write_error<W: Write>(
    output: &SharedWriter<W>,
    id: u64,
    code: i32,
    message: String,
    data: Option<Value>,
) -> io::Result<()> {
    write_line(
        output,
        &JsonRpcResponse {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
        },
    )
}

fn write_line<W: Write>(output: &SharedWriter<W>, value: &impl serde::Serialize) -> io::Result<()> {
    let mut output = output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    serde_json::to_writer(&mut *output, value).map_err(io::Error::other)?;
    output.write_all(b"\n")?;
    output.flush()
}

#[must_use]
pub fn required_capabilities() -> Vec<Capability> {
    vec![Capability::CliCommands]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invocation(args: &[&str]) -> CliCommandInvocation {
        CliCommandInvocation {
            command_name: COMMAND_NAME.into(),
            args: args.iter().map(ToString::to_string).collect(),
            cwd: "/tmp/work deck".into(),
        }
    }

    #[test]
    fn status_streams_stdout_and_exits_zero() {
        let mut output = Vec::new();
        let execution = execute_cli_tools(
            &invocation(&["status"]),
            &AtomicBool::new(false),
            |stream, bytes| {
                output.push((stream, bytes.to_vec()));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(
            output,
            [(
                CliOutputStream::Stdout,
                b"cli-tools is ready in /tmp/work deck\n".to_vec()
            )]
        );
        assert_eq!(execution.result, CliCommandResult::Exit { code: 0 });
    }

    #[test]
    fn review_streams_stderr_waits_and_delegates_raw_args() {
        let started = Instant::now();
        let mut output = Vec::new();
        let execution = execute_cli_tools(
            &invocation(&["review", "--", "-leading", "two words"]),
            &AtomicBool::new(false),
            |stream, bytes| {
                output.push((stream, bytes.to_vec()));
                Ok(())
            },
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(95));
        assert_eq!(
            output,
            [(
                CliOutputStream::Stderr,
                "Preparing review input…\n".as_bytes().to_vec()
            )]
        );
        assert_eq!(
            execution.result,
            CliCommandResult::Delegate {
                argv: vec![
                    "diff".into(),
                    "--".into(),
                    "-leading".into(),
                    "two words".into()
                ]
            }
        );
    }

    #[test]
    fn cancellation_interrupts_preparation_without_waiting_the_full_window() {
        let cancelled = Arc::new(AtomicBool::new(false));
        let trigger = Arc::clone(&cancelled);
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            trigger.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let error =
            execute_cli_tools(&invocation(&["review"]), &cancelled, |_, _| Ok(())).unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(90));
        assert_eq!(error.message, "Extension CLI command interrupted.");
    }

    #[test]
    fn invalid_actions_are_user_errors_with_workdeck_guidance() {
        let error = execute_cli_tools(
            &invocation(&["wat"]),
            &AtomicBool::new(false),
            |_, _| Ok(()),
        )
        .unwrap_err();
        assert_eq!(error.message, "Choose a cli-tools action.");
        assert_eq!(
            error.suggestions,
            ["Run `workdeck cli-tools status` or `workdeck cli-tools review`."]
        );
    }
}
