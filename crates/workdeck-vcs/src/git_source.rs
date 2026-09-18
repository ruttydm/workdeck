//! Bounded Git source expansion for revision, index, and working-tree endpoints.

use crate::git_commands::GitDiffEndpoint;
use crate::{
    DEFAULT_SOURCE_TEXT_MAX_BYTES, LimitedSourceTextResult, SourceTextError,
    force_terminate_source_subprocess, log_source_diagnostic, read_file_text_with_limit,
    read_stream_text_with_limit, terminate_source_subprocess,
};
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

const GIT_SOURCE_DIAGNOSTIC_MAX_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitFileSourceSpec {
    None,
    FileSystem {
        absolute_path: PathBuf,
    },
    GitBlob {
        repo_root: PathBuf,
        reference: String,
        path: PathBuf,
    },
    GitIndex {
        repo_root: PathBuf,
        path: PathBuf,
    },
}

pub type GitSourceDiagnostic = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct GitFileSourceOptions {
    pub git_executable: PathBuf,
    pub max_source_bytes: usize,
    pub diagnostic: Option<GitSourceDiagnostic>,
}

impl fmt::Debug for GitFileSourceOptions {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GitFileSourceOptions")
            .field("git_executable", &self.git_executable)
            .field("max_source_bytes", &self.max_source_bytes)
            .field(
                "diagnostic",
                &self.diagnostic.as_ref().map(|_| "<callback>"),
            )
            .finish()
    }
}

impl Default for GitFileSourceOptions {
    fn default() -> Self {
        Self {
            git_executable: PathBuf::from("git"),
            max_source_bytes: DEFAULT_SOURCE_TEXT_MAX_BYTES,
            diagnostic: None,
        }
    }
}

/// Convert a provider-local Git diff endpoint into its source lookup.
pub fn git_endpoint_source_spec(
    endpoint: &GitDiffEndpoint,
    repo_root: &Path,
    file_path: &Path,
) -> GitFileSourceSpec {
    match endpoint {
        GitDiffEndpoint::None => GitFileSourceSpec::None,
        GitDiffEndpoint::GitRef(reference) => GitFileSourceSpec::GitBlob {
            repo_root: repo_root.to_path_buf(),
            reference: reference.clone(),
            path: file_path.to_path_buf(),
        },
        GitDiffEndpoint::Index => GitFileSourceSpec::GitIndex {
            repo_root: repo_root.to_path_buf(),
            path: file_path.to_path_buf(),
        },
        GitDiffEndpoint::Worktree => GitFileSourceSpec::FileSystem {
            absolute_path: repo_root.join(file_path),
        },
    }
}

/// Read the complete text named by one resolved Git source specification.
pub fn read_git_file_source(
    spec: &GitFileSourceSpec,
    options: &GitFileSourceOptions,
) -> LimitedSourceTextResult {
    match spec {
        GitFileSourceSpec::None => LimitedSourceTextResult::Missing,
        GitFileSourceSpec::FileSystem { absolute_path } => {
            read_file_text_with_limit(absolute_path, options.max_source_bytes)
        }
        GitFileSourceSpec::GitBlob {
            repo_root,
            reference,
            path,
        } => read_git_object_spec(
            repo_root,
            &format!("{reference}:{}", path.to_string_lossy()),
            options,
        ),
        GitFileSourceSpec::GitIndex { repo_root, path } => {
            read_git_object_spec(repo_root, &format!(":{}", path.to_string_lossy()), options)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GitStream {
    Stdout,
    Stderr,
}

type StreamResult = (GitStream, Result<String, SourceTextError>);

fn read_git_object_spec(
    repo_root: &Path,
    object_name: &str,
    options: &GitFileSourceOptions,
) -> LimitedSourceTextResult {
    let mut command = git_source_command(options, repo_root, object_name);
    let mut process = match crate::source_text::spawn_source_subprocess(&mut command) {
        Ok(process) => process,
        Err(error) => {
            emit_diagnostic(
                options,
                &format!("failed to run Git while reading source {object_name}"),
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
        let _ = stdout_sender.send((GitStream::Stdout, result));
    });
    let stderr_reader = thread::spawn(move || {
        let result = read_stream_text_with_limit(stderr, GIT_SOURCE_DIAGNOSTIC_MAX_BYTES, None);
        let _ = sender.send((GitStream::Stderr, result));
    });

    let mut stdout_text = None;
    let mut stderr_text = None;
    let mut exit_status = None;
    let mut collection_error = None;

    while stdout_text.is_none() || stderr_text.is_none() || exit_status.is_none() {
        match receiver.recv_timeout(Duration::from_millis(2)) {
            Ok((stream, Ok(text))) => match stream {
                GitStream::Stdout => stdout_text = Some(text),
                GitStream::Stderr => stderr_text = Some(text),
            },
            Ok((stream, Err(error))) => {
                let force = matches!(&error, SourceTextError::TooLarge { .. });
                collection_error = Some((stream, error));
                if force {
                    force_terminate_source_subprocess(&mut process);
                } else {
                    terminate_source_subprocess(&mut process);
                }
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected)
                if stdout_text.is_some() && stderr_text.is_some() => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                collection_error = Some((
                    GitStream::Stdout,
                    SourceTextError::Io(std::io::Error::other(
                        "Git source reader stopped before collecting both streams",
                    )),
                ));
                terminate_source_subprocess(&mut process);
                break;
            }
        }

        match process.try_wait() {
            Ok(status) => exit_status = status.or(exit_status),
            Err(error) => {
                collection_error = Some((GitStream::Stdout, SourceTextError::Io(error)));
                terminate_source_subprocess(&mut process);
                break;
            }
        }
    }

    // Readers finish after normal EOF or after the bounded TERM/KILL cleanup above.
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();

    if let Some((stream, error)) = collection_error {
        if stream == GitStream::Stdout && matches!(error, SourceTextError::TooLarge { .. }) {
            return LimitedSourceTextResult::TooLarge {
                max_bytes: options.max_source_bytes,
            };
        }
        let detail = match (stream, &error) {
            (GitStream::Stderr, SourceTextError::TooLarge { .. }) => {
                format!("Git source diagnostics exceeded {GIT_SOURCE_DIAGNOSTIC_MAX_BYTES} bytes.")
            }
            _ => error.to_string(),
        };
        emit_diagnostic(
            options,
            &format!("failed to collect Git source {object_name}"),
            &detail,
        );
        return LimitedSourceTextResult::Missing;
    }

    let status = exit_status.expect("Git exit status collected before loop completion");
    let stdout = stdout_text.expect("Git stdout collected before loop completion");
    let stderr = stderr_text.expect("Git stderr collected before loop completion");
    if !status.success() {
        if !is_expected_missing_git_source(&stderr) {
            emit_diagnostic(
                options,
                &format!(
                    "failed to read Git source {object_name} in {}",
                    repo_root.display()
                ),
                &stderr,
            );
        }
        return LimitedSourceTextResult::Missing;
    }

    LimitedSourceTextResult::Text(stdout)
}

fn git_source_command(
    options: &GitFileSourceOptions,
    repo_root: &Path,
    object_name: &str,
) -> Command {
    let mut command = Command::new(&options.git_executable);
    command
        .args(["show", object_name])
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

fn is_expected_missing_git_source(stderr: &str) -> bool {
    let normalized = stderr.to_lowercase();
    [
        "exists on disk, but not in",
        "does not exist in",
        "invalid object name",
        "needed a single revision",
        "unknown revision or path not in the working tree",
    ]
    .iter()
    .any(|fragment| normalized.contains(fragment))
}

fn emit_diagnostic(options: &GitFileSourceOptions, message: &str, detail: &dyn fmt::Display) {
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
