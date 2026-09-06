//! Compiled native port of Hunk's CLI-tools extension example.

use serde_json::{Value, json};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::{Duration, Instant};
use workdeck_extension_api::{
    API_VERSION, Capability, CliCommandExecution, CliCommandInvocation, CliCommandRegistration,
    CliCommandResult, CliOutputNotification, CliOutputStream, CliStdinChunk, CliStdinReadRequest,
    HandshakeResponse, JsonRpcError, JsonRpcRequest, JsonRpcResponse, Registration,
};

const COMMAND_NAME: &str = "cli-tools";
const SUMMARY: &str = "Demonstrate extension-provided CLI workflows";
const USAGE: &str = "<status|review|stdin> [args...]";

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
    mut read_stdin: impl FnMut(usize) -> Result<Option<Vec<u8>>, CliToolsUserError>,
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
        "stdin" => {
            let mut consumed = false;
            while let Some(bytes) = read_stdin(8 * 1024)? {
                consumed |= !bytes.is_empty();
                emit(CliOutputStream::Stdout, &bytes).map_err(output_error)?;
            }
            Ok(CliCommandExecution {
                result: CliCommandResult::Exit { code: 7 },
                stdin_read_started: true,
                stdin_consumed: consumed,
            })
        }
        "touch-stdin" => {
            let consumed = read_stdin(8 * 1024)?.is_some_and(|bytes| !bytes.is_empty());
            Ok(CliCommandExecution {
                result: CliCommandResult::Delegate {
                    argv: vec!["diff".into()],
                },
                stdin_read_started: true,
                stdin_consumed: consumed,
            })
        }
        "write-twice" => {
            emit(CliOutputStream::Stdout, b"first").map_err(output_error)?;
            emit(CliOutputStream::Stdout, b"second").map_err(output_error)?;
            Ok(CliCommandExecution {
                result: CliCommandResult::Exit { code: 0 },
                stdin_read_started: false,
                stdin_consumed: false,
            })
        }
        "late-output" => Ok(CliCommandExecution {
            result: CliCommandResult::Exit { code: 0 },
            stdin_read_started: false,
            stdin_consumed: false,
        }),
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
#[derive(Clone)]
struct ActiveCliRequest {
    id: u64,
    cancelled: Arc<AtomicBool>,
    stdin_chunks: mpsc::Sender<CliStdinChunk>,
}

type ActiveRequest = Arc<Mutex<Option<ActiveCliRequest>>>;

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
                let (stdin_chunks, stdin_responses) = mpsc::channel();
                *active
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(ActiveCliRequest {
                    id: request.id,
                    cancelled: Arc::clone(&cancelled),
                    stdin_chunks,
                });
                let output = Arc::clone(&output);
                let active = Arc::clone(&active);
                thread::spawn(move || {
                    let emit_late_output =
                        invocation.args.first().map(String::as_str) == Some("late-output");
                    let mut next_read_id = 1_u64;
                    let result = if invocation.args.first().map(String::as_str)
                        == Some("pending-stdin")
                    {
                        write_cli_stdin_read(&output, request.id, next_read_id, 8 * 1024)
                            .map_err(output_error)
                            .map(|()| CliCommandExecution {
                                result: CliCommandResult::Exit { code: 0 },
                                stdin_read_started: true,
                                stdin_consumed: false,
                            })
                    } else {
                        execute_cli_tools(
                            &invocation,
                            &cancelled,
                            |stream, bytes| write_cli_output(&output, request.id, stream, bytes),
                            |max_bytes| {
                                let read_id = next_read_id;
                                next_read_id = next_read_id.saturating_add(1);
                                write_cli_stdin_read(&output, request.id, read_id, max_bytes)
                                    .map_err(output_error)?;
                                let chunk = stdin_responses
                                    .recv_timeout(Duration::from_secs(30))
                                    .map_err(|error| CliToolsUserError {
                                    message: format!("CLI stdin failed: {error}"),
                                    suggestions: Vec::new(),
                                })?;
                                if chunk.request_id != request.id || chunk.read_id != read_id {
                                    return Err(CliToolsUserError {
                                        message:
                                            "CLI stdin response identity did not match its request."
                                                .into(),
                                        suggestions: Vec::new(),
                                    });
                                }
                                if let Some(error) = chunk.error {
                                    return Err(CliToolsUserError {
                                        message: format!("CLI stdin failed: {error}"),
                                        suggestions: Vec::new(),
                                    });
                                }
                                Ok((!chunk.done).then_some(chunk.bytes))
                            },
                        )
                    };
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
                    if emit_late_output {
                        thread::sleep(Duration::from_millis(5));
                        let _ = write_cli_output(
                            &output,
                            request.id,
                            CliOutputStream::Stdout,
                            b"revoked late output",
                        );
                    }
                    let mut current = active
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    if current
                        .as_ref()
                        .is_some_and(|active| active.id == request.id)
                    {
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
    if value.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        return;
    }
    if value.get("method").and_then(Value::as_str) == Some("workdeck/cli/stdin/chunk") {
        let Some(params) = value.get("params") else {
            return;
        };
        let Ok(chunk) = serde_json::from_value::<CliStdinChunk>(params.clone()) else {
            return;
        };
        if let Some(current) = active
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .cloned()
            && current.id == chunk.request_id
        {
            let _ = current.stdin_chunks.send(chunk);
        }
        return;
    }
    if value.get("method").and_then(Value::as_str) != Some("$/cancelRequest") {
        return;
    }
    let Some(id) = value
        .get("params")
        .and_then(|params| params.get("id"))
        .and_then(Value::as_u64)
    else {
        return;
    };
    if let Some(current) = active
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .as_ref()
        && current.id == id
    {
        current.cancelled.store(true, Ordering::Release);
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

fn write_cli_stdin_read<W: Write>(
    output: &SharedWriter<W>,
    request_id: u64,
    read_id: u64,
    max_bytes: usize,
) -> io::Result<()> {
    write_line(
        output,
        &json!({
            "jsonrpc": "2.0",
            "method": "workdeck/cli/stdin/read",
            "params": CliStdinReadRequest { request_id, read_id, max_bytes },
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
            |_| Ok(None),
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
            |_| Ok(None),
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
        let error = execute_cli_tools(
            &invocation(&["review"]),
            &cancelled,
            |_, _| Ok(()),
            |_| Ok(None),
        )
        .unwrap_err();
        assert!(started.elapsed() < Duration::from_millis(90));
        assert_eq!(error.message, "Extension CLI command interrupted.");
    }

    #[test]
    fn invalid_actions_are_user_errors_with_workdeck_guidance() {
        let error = execute_cli_tools(
            &invocation(&["wat"]),
            &AtomicBool::new(false),
            |_, _| Ok(()),
            |_| Ok(None),
        )
        .unwrap_err();
        assert_eq!(error.message, "Choose a cli-tools action.");
        assert_eq!(
            error.suggestions,
            ["Run `workdeck cli-tools status` or `workdeck cli-tools review`."]
        );
    }
}
