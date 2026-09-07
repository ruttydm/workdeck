//! Bounded source expansion from already-resolved Jujutsu commits.

use crate::{
    DEFAULT_SOURCE_TEXT_MAX_BYTES, LimitedSourceTextResult, SourceTextError, log_source_diagnostic,
    read_stream_text_with_limit, terminate_source_subprocess,
};
use std::fmt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

const JJ_SOURCE_DIAGNOSTIC_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JujutsuFileSourceSpec {
    pub repo_root: PathBuf,
    pub commit_id: String,
    pub path: String,
}

pub type JujutsuSourceDiagnostic = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct JujutsuFileSourceOptions {
    pub jj_executable: PathBuf,
    pub max_source_bytes: usize,
    pub diagnostic: Option<JujutsuSourceDiagnostic>,
}

impl fmt::Debug for JujutsuFileSourceOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("JujutsuFileSourceOptions")
            .field("jj_executable", &self.jj_executable)
            .field("max_source_bytes", &self.max_source_bytes)
            .field(
                "diagnostic",
                &self.diagnostic.as_ref().map(|_| "<callback>"),
            )
            .finish()
    }
}

impl Default for JujutsuFileSourceOptions {
    fn default() -> Self {
        Self {
            jj_executable: PathBuf::from("jj"),
            max_source_bytes: DEFAULT_SOURCE_TEXT_MAX_BYTES,
            diagnostic: None,
        }
    }
}

/// Encode a repository-relative path as an alias-insensitive literal Jujutsu fileset.
#[must_use]
pub fn jj_file_path_fileset(path: &str) -> String {
    let mut literal = String::new();
    for character in path.chars() {
        match character {
            '*' => literal.push_str("[*]"),
            '?' => literal.push_str("[?]"),
            '[' => literal.push_str("[[]"),
            ']' => literal.push_str("[]]"),
            '{' => literal.push_str("[{]"),
            '}' => literal.push_str("[}]"),
            '\\' if cfg!(windows) => literal.push('/'),
            '\\' => literal.push_str("[\\\\]"),
            _ => literal.push(character),
        }
    }
    serde_json::to_string(&literal).expect("a Rust string always has a JSON representation")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JujutsuStream {
    Stdout,
    Stderr,
}

type StreamResult = (JujutsuStream, Result<String, SourceTextError>);

/// Read one complete file from the immutable commit that produced the review patch.
pub fn read_jj_file_source(
    spec: &JujutsuFileSourceSpec,
    options: &JujutsuFileSourceOptions,
) -> LimitedSourceTextResult {
    let arguments = jj_source_arguments(spec);
    let mut command = jj_source_command(options, spec, &arguments);
    let mut process = match crate::source_text::spawn_source_subprocess(&mut command) {
        Ok(process) => process,
        Err(error) => {
            emit_diagnostic(
                options,
                &format!(
                    "failed to run Jujutsu while reading source {}:{}",
                    spec.commit_id, spec.path
                ),
                &error,
            );
            return LimitedSourceTextResult::Missing;
        }
    };

    let stdout = process.stdout.take();
    let stderr = process.stderr.take();
    let (sender, receiver) = mpsc::channel::<StreamResult>();
    let stdout_sender = sender.clone();
    let stdout_limit = options.max_source_bytes;
    let stdout_reader = thread::spawn(move || {
        let result = read_stream_text_with_limit(stdout, stdout_limit, None);
        let _ = stdout_sender.send((JujutsuStream::Stdout, result));
    });
    let stderr_reader = thread::spawn(move || {
        let result = read_stream_text_with_limit(stderr, JJ_SOURCE_DIAGNOSTIC_MAX_BYTES, None);
        let _ = sender.send((JujutsuStream::Stderr, result));
    });

    let mut stdout_text = None;
    let mut stderr_text = None;
    let mut exit_status = None;
    let mut collection_error = None;
    while stdout_text.is_none() || stderr_text.is_none() || exit_status.is_none() {
        match receiver.recv_timeout(Duration::from_millis(2)) {
            Ok((stream, Ok(text))) => match stream {
                JujutsuStream::Stdout => stdout_text = Some(text),
                JujutsuStream::Stderr => stderr_text = Some(text),
            },
            Ok((stream, Err(error))) => {
                collection_error = Some((stream, error));
                terminate_source_subprocess(&mut process);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected)
                if stdout_text.is_some() && stderr_text.is_some() => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                collection_error = Some((
                    JujutsuStream::Stdout,
                    SourceTextError::Io(std::io::Error::other(
                        "Jujutsu source reader stopped before collecting both streams",
                    )),
                ));
                terminate_source_subprocess(&mut process);
                break;
            }
        }
        match process.try_wait() {
            Ok(status) => exit_status = status.or(exit_status),
            Err(error) => {
                collection_error = Some((JujutsuStream::Stdout, SourceTextError::Io(error)));
                terminate_source_subprocess(&mut process);
                break;
            }
        }
    }
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();

    if let Some((stream, error)) = collection_error {
        if stream == JujutsuStream::Stdout && matches!(error, SourceTextError::TooLarge { .. }) {
            return LimitedSourceTextResult::TooLarge {
                max_bytes: options.max_source_bytes,
            };
        }
        let detail = match (stream, &error) {
            (JujutsuStream::Stderr, SourceTextError::TooLarge { .. }) => format!(
                "Jujutsu source diagnostics exceeded {JJ_SOURCE_DIAGNOSTIC_MAX_BYTES} bytes."
            ),
            _ => error.to_string(),
        };
        emit_diagnostic(
            options,
            &format!(
                "failed to collect Jujutsu source {}:{}",
                spec.commit_id, spec.path
            ),
            &detail,
        );
        return LimitedSourceTextResult::Missing;
    }

    let status = exit_status.expect("Jujutsu exit status collected before loop completion");
    let stdout = stdout_text.expect("Jujutsu stdout collected before loop completion");
    let stderr = stderr_text.expect("Jujutsu stderr collected before loop completion");
    if !status.success() {
        if !is_expected_missing_jj_source(&stderr) {
            emit_diagnostic(
                options,
                &format!(
                    "failed to read Jujutsu source {}:{} in {}",
                    spec.commit_id,
                    spec.path,
                    spec.repo_root.display()
                ),
                &stderr,
            );
        }
        return LimitedSourceTextResult::Missing;
    }
    LimitedSourceTextResult::Text(stdout)
}

fn jj_source_arguments(spec: &JujutsuFileSourceSpec) -> Vec<String> {
    vec![
        "--no-pager".to_owned(),
        "--color".into(),
        "never".into(),
        "file".into(),
        "show".into(),
        "--ignore-working-copy".into(),
        "-r".into(),
        spec.commit_id.clone(),
        "-T".into(),
        "\"\"".into(),
        "--".into(),
        jj_file_path_fileset(&spec.path),
    ]
}

fn jj_source_command(
    options: &JujutsuFileSourceOptions,
    spec: &JujutsuFileSourceSpec,
    arguments: &[String],
) -> Command {
    let mut command = Command::new(&options.jj_executable);
    command
        .args(arguments)
        .current_dir(&spec.repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn is_expected_missing_jj_source(stderr: &str) -> bool {
    let normalized = stderr.to_ascii_lowercase();
    [
        "no such path:",
        "path does not exist:",
        "path doesn't exist:",
    ]
    .iter()
    .any(|fragment| normalized.contains(fragment))
}

fn emit_diagnostic(options: &JujutsuFileSourceOptions, message: &str, detail: &dyn fmt::Display) {
    if let Some(diagnostic) = &options.diagnostic {
        let detail = detail
            .to_string()
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or_default()
            .to_owned();
        if detail.is_empty() {
            diagnostic(message.to_owned());
        } else {
            diagnostic(format!("{message}: {detail}"));
        }
    } else {
        log_source_diagnostic(message, detail);
    }
}

#[cfg(test)]
mod tests;
