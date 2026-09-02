//! Exact Sapling command construction, untracked discovery, and failure translation.

use crate::{VcsCatalogError, describe_diff_targets, normalize_path_for_os};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use workdeck_core::{VcsDiffCommandInput, VcsShowCommandInput, WorkdeckUserError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaplingBackedInput {
    Diff(VcsDiffCommandInput),
    Show(VcsShowCommandInput),
}

impl From<&VcsDiffCommandInput> for SaplingBackedInput {
    fn from(input: &VcsDiffCommandInput) -> Self {
        Self::Diff(input.clone())
    }
}

impl From<&VcsShowCommandInput> for SaplingBackedInput {
    fn from(input: &VcsShowCommandInput) -> Self {
        Self::Show(input.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaplingCommandContext {
    pub cwd: PathBuf,
    pub sl_executable: PathBuf,
}

impl Default for SaplingCommandContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            sl_executable: PathBuf::from("sl"),
        }
    }
}

/// Reject a Sapling revision that could be interpreted as an option.
pub fn require_sl_revision_arg<'a>(
    input: &SaplingBackedInput,
    value: &'a str,
) -> Result<&'a str, WorkdeckUserError> {
    if value.is_empty() {
        return Err(WorkdeckUserError::new(
            format!(
                "`{}` refused an empty revision.",
                format_sl_command_label(input)
            ),
            vec!["Pass a non-empty revision or revset and try again.".into()],
        ));
    }
    if value.starts_with('-') {
        return Err(WorkdeckUserError::new(
            format!(
                "`{}` refused revision `{value}` because it looks like a Sapling option.",
                format_sl_command_label(input)
            ),
            vec!["Pass a plain revision or revset and try again.".into()],
        ));
    }
    Ok(value)
}

fn validate_sl_diff_endpoints(input: &VcsDiffCommandInput) -> Result<(), WorkdeckUserError> {
    if let Some(endpoints) = &input.range_endpoints {
        let backed = SaplingBackedInput::from(input);
        require_sl_revision_arg(&backed, &endpoints.from)?;
        require_sl_revision_arg(&backed, &endpoints.to)?;
    }
    Ok(())
}

fn append_sl_pathspecs(arguments: &mut Vec<String>, pathspecs: &[String]) {
    if !pathspecs.is_empty() {
        arguments.push("--".into());
        arguments.extend(pathspecs.iter().cloned());
    }
}

/// Build `sl diff --git` arguments for working-copy and revset reviews.
pub fn build_sl_diff_args(input: &VcsDiffCommandInput) -> Result<Vec<String>, WorkdeckUserError> {
    validate_sl_diff_endpoints(input)?;
    let mut arguments = vec!["diff".into(), "--git".into()];
    if let Some(endpoints) = &input.range_endpoints {
        arguments.extend([
            "-r".into(),
            endpoints.from.clone(),
            "-r".into(),
            endpoints.to.clone(),
        ]);
    } else if let Some(range) = &input.range {
        arguments.extend(["-r".into(), range.clone()]);
    }
    append_sl_pathspecs(&mut arguments, &input.pathspecs);
    Ok(arguments)
}

/// Build `sl diff --git --change` arguments used for `workdeck show`.
#[must_use]
pub fn build_sl_show_args(input: &VcsShowCommandInput) -> Vec<String> {
    let mut arguments = vec![
        "diff".into(),
        "--git".into(),
        "--change".into(),
        input.reference.clone().unwrap_or_else(|| ".".into()),
    ];
    append_sl_pathspecs(&mut arguments, &input.pathspecs);
    arguments
}

fn build_sl_status_args(input: &VcsDiffCommandInput) -> Vec<String> {
    let mut arguments = vec![
        "status".into(),
        "--unknown".into(),
        "--print0".into(),
        "--root-relative".into(),
    ];
    append_sl_pathspecs(&mut arguments, &input.pathspecs);
    arguments
}

/// User-facing label for the Sapling operation being run.
#[must_use]
pub fn format_sl_command_label(input: &SaplingBackedInput) -> String {
    match input {
        SaplingBackedInput::Diff(input) if input.staged => "workdeck diff --staged".into(),
        SaplingBackedInput::Diff(input) => describe_diff_targets(input).map_or_else(
            || "workdeck diff".into(),
            |targets| format!("workdeck diff {targets}"),
        ),
        SaplingBackedInput::Show(input) => input.reference.as_ref().map_or_else(
            || "workdeck show".into(),
            |reference| format!("workdeck show {reference}"),
        ),
    }
}

fn first_sl_error_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| stderr.trim());
    let lower = line.to_ascii_lowercase();
    for prefix in ["abort:", "error:"] {
        if lower.starts_with(prefix) {
            return line[prefix.len()..].trim().to_owned();
        }
    }
    if line.is_empty() {
        "Sapling command failed.".into()
    } else {
        line.into()
    }
}

fn contains_case_insensitive(haystack: &str, needles: &[&str]) -> bool {
    let haystack = haystack.to_ascii_lowercase();
    needles
        .iter()
        .any(|needle| haystack.contains(&needle.to_ascii_lowercase()))
}

fn missing_sl_executable_error(
    input: &SaplingBackedInput,
    sl_executable: &Path,
) -> WorkdeckUserError {
    WorkdeckUserError::new(
        format!(
            "Sapling is required for `{}` when `vcs = \"sl\"`, but `{}` was not found in PATH.",
            format_sl_command_label(input),
            sl_executable.display()
        ),
        vec!["Install Sapling or set `vcs = \"git\"` in Workdeck config, then try again.".into()],
    )
}

fn missing_sl_repo_error(input: &SaplingBackedInput) -> WorkdeckUserError {
    WorkdeckUserError::new(
        format!(
            "`{}` must be run inside a Sapling repository when `vcs = \"sl\"`.",
            format_sl_command_label(input)
        ),
        vec![
            "Run the command from a Sapling checkout, or set `vcs = \"git\"` in Workdeck config."
                .into(),
        ],
    )
}

/// Return the user-facing error when `--staged` is used with Sapling.
#[must_use]
pub fn create_sl_staged_error(input: &VcsDiffCommandInput) -> WorkdeckUserError {
    let backed = SaplingBackedInput::from(input);
    WorkdeckUserError::new(
        format!(
            "`{}` requires Git VCS mode because Sapling has no staging area.",
            format_sl_command_label(&backed)
        ),
        vec!["Remove `--staged`, or set `vcs = \"git\"` in Workdeck config.".into()],
    )
}

fn invalid_revset_error(input: &SaplingBackedInput) -> WorkdeckUserError {
    match input {
        SaplingBackedInput::Diff(input) if input.range_endpoints.is_some() => {
            let endpoints = input.range_endpoints.as_ref().unwrap();
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Sapling revisions `{}` and `{}`.",
                    format_sl_command_label(&SaplingBackedInput::Diff(input.clone())),
                    endpoints.from,
                    endpoints.to
                ),
                vec!["Check both revisions and try again.".into()],
            )
        }
        SaplingBackedInput::Diff(input) => {
            let revset = input.range.as_deref().unwrap_or("");
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Sapling revset `{revset}`.",
                    format_sl_command_label(&SaplingBackedInput::Diff(input.clone()))
                ),
                vec!["Check the revset and try again.".into()],
            )
        }
        SaplingBackedInput::Show(input) => {
            let revset = input.reference.as_deref().unwrap_or(".");
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Sapling revset `{revset}`.",
                    format_sl_command_label(&SaplingBackedInput::Show(input.clone()))
                ),
                vec!["Check the revset and try again.".into()],
            )
        }
    }
}

fn translate_sl_exit_failure(input: &SaplingBackedInput, stderr: &str) -> WorkdeckUserError {
    if contains_case_insensitive(
        stderr,
        &[
            "is not inside a repository",
            "not in a repository",
            "no repository found",
        ],
    ) {
        return missing_sl_repo_error(input);
    }
    if contains_case_insensitive(
        stderr,
        &[
            "unknown revision",
            "ambiguous identifier",
            "can't find revision",
            "is not a valid revision",
            "revision not found",
            "syntax error in revset",
        ],
    ) {
        return invalid_revset_error(input);
    }
    WorkdeckUserError::new(
        format!("`{}` failed.", format_sl_command_label(input)),
        vec![first_sl_error_line(stderr)],
    )
}

/// Run one Sapling command and translate common failures into user-facing errors.
pub fn run_sl_text(
    input: &SaplingBackedInput,
    arguments: &[String],
    context: &SaplingCommandContext,
) -> Result<String, VcsCatalogError> {
    let output = Command::new(&context.sl_executable)
        .args(["--noninteractive", "--color", "never"])
        .args(arguments)
        .current_dir(&context.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                VcsCatalogError::User(missing_sl_executable_error(input, &context.sl_executable))
            } else {
                VcsCatalogError::Operation(error.to_string())
            }
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            format!(
                "Command failed: {} --noninteractive --color never {}",
                context.sl_executable.display(),
                arguments.join(" ")
            )
        } else {
            stderr.trim().to_owned()
        };
        return Err(VcsCatalogError::User(translate_sl_exit_failure(
            input, &detail,
        )));
    }
    Ok(stdout)
}

fn should_include_untracked_files(input: &VcsDiffCommandInput) -> bool {
    !input.staged
        && input.range_endpoints.is_none()
        && input.options.exclude_untracked != Some(true)
}

fn parse_untracked_file_paths(status: &str) -> Vec<PathBuf> {
    status
        .split('\0')
        .filter_map(|entry| entry.strip_prefix("? ").map(PathBuf::from))
        .collect()
}

fn is_reviewable_untracked_path(repo_root: &Path, file_path: &Path) -> bool {
    let absolute_path = repo_root.join(file_path);
    let Ok(metadata) = fs::symlink_metadata(&absolute_path) else {
        return true;
    };
    if metadata.is_dir() {
        return false;
    }
    if !metadata.file_type().is_symlink() {
        return true;
    }
    fs::metadata(absolute_path).map_or(true, |target| !target.is_dir())
}

/// Return repo-root-relative unknown files for one working-copy Sapling review.
pub fn list_sl_untracked_files(
    input: &VcsDiffCommandInput,
    context: &SaplingCommandContext,
    repo_root: Option<&Path>,
) -> Result<Vec<PathBuf>, VcsCatalogError> {
    validate_sl_diff_endpoints(input).map_err(VcsCatalogError::User)?;
    if !should_include_untracked_files(input) {
        return Ok(Vec::new());
    }
    let backed = SaplingBackedInput::from(input);
    let status = run_sl_text(&backed, &build_sl_status_args(input), context)?;
    let paths = parse_untracked_file_paths(&status);
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let resolved_root;
    let root = if let Some(repo_root) = repo_root {
        repo_root
    } else {
        resolved_root = resolve_sl_repo_root(&backed, context)?;
        &resolved_root
    };
    Ok(paths
        .into_iter()
        .filter(|path| is_reviewable_untracked_path(root, path))
        .collect())
}

/// Resolve the repository root with `sl root`.
pub fn resolve_sl_repo_root(
    input: &SaplingBackedInput,
    context: &SaplingCommandContext,
) -> Result<PathBuf, VcsCatalogError> {
    let root = run_sl_text(input, &["root".into()], context)?;
    Ok(PathBuf::from(normalize_path_for_os(root.trim())))
}

#[cfg(test)]
mod tests;
