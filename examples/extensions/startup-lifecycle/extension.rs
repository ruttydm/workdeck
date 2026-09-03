//! Compiled fixture for staged startup, configuration, and retirement semantics.

use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use workdeck_extension_api::{
    API_VERSION, HandshakeRequest, HandshakeResponse, JsonRpcRequest, JsonRpcResponse,
    Registration, ThemeRegistration,
};

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
        if request.method != "workdeck/handshake" {
            continue;
        }
        let handshake: HandshakeRequest =
            serde_json::from_value(request.params).map_err(io::Error::other)?;
        log_path = handshake
            .config
            .get("logPath")
            .and_then(Value::as_str)
            .map(PathBuf::from);
        let value = handshake
            .config
            .get("value")
            .map(ToString::to_string)
            .unwrap_or_else(|| "null".into());
        if let Some(path) = log_path.as_deref() {
            append_log(path, &format!("factory:{}:{value}", handshake.extension_id))?;
            if handshake
                .config
                .get("logCwd")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                append_log(path, &format!("cwd:{}", handshake.cwd.display()))?;
            }
        }
        if handshake
            .config
            .get("logStderr")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            eprintln!("factory log 🧭");
        }
        let theme_id = handshake
            .config
            .get("themeId")
            .and_then(Value::as_str)
            .unwrap_or("fallback");
        let response = HandshakeResponse {
            extension_api_version: API_VERSION,
            extension_version: env!("CARGO_PKG_VERSION").into(),
            registrations: vec![
                Registration::Theme(ThemeRegistration {
                    id: theme_id.into(),
                    base: None,
                    colors: BTreeMap::new(),
                }),
                Registration::EventSubscription {
                    names: vec!["shutdown".into()],
                },
            ],
        };
        write_result(&mut output, request.id, response)?;
    }
}

fn append_log(path: &Path, line: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(file, "{line}")
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
