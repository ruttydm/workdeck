//! Exact Jujutsu command construction, immutable endpoint resolution, and failure translation.

use crate::{VcsCatalogError, describe_diff_targets, normalize_path_for_os};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use workdeck_core::{
    VcsDiffCommandInput, VcsRangeEndpoints, VcsShowCommandInput, WorkdeckUserError,
};

const JJ_COMMIT_ID_TEMPLATE: &str = "self.commit_id() ++ \"\\n\"";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JujutsuBackedInput {
    Diff(VcsDiffCommandInput),
    Show(VcsShowCommandInput),
}

impl From<&VcsDiffCommandInput> for JujutsuBackedInput {
    fn from(input: &VcsDiffCommandInput) -> Self {
        Self::Diff(input.clone())
    }
}

impl From<&VcsShowCommandInput> for JujutsuBackedInput {
    fn from(input: &VcsShowCommandInput) -> Self {
        Self::Show(input.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JujutsuCommandContext {
    pub cwd: PathBuf,
    pub jj_executable: PathBuf,
}

impl Default for JujutsuCommandContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            jj_executable: PathBuf::from("jj"),
        }
    }
}

/// Immutable new commit and every commit used to construct the old side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JujutsuDiffEndpoints {
    pub new_commit_id: String,
    pub old_commit_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JujutsuPinnedDiff {
    Revision(String),
    Range(VcsRangeEndpoints),
}

fn parse_jj_commit_ids(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && line.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .map(str::to_owned)
        .collect()
}

pub fn require_jj_revision_arg<'a>(
    input: &JujutsuBackedInput,
    value: &'a str,
) -> Result<&'a str, WorkdeckUserError> {
    if value.is_empty() {
        return Err(WorkdeckUserError::new(
            format!(
                "`{}` refused an empty revision.",
                format_jj_command_label(input)
            ),
            vec!["Pass a non-empty revision or revset and try again.".into()],
        ));
    }
    if value.starts_with('-') {
        return Err(WorkdeckUserError::new(
            format!(
                "`{}` refused revision `{value}` because it looks like a Jujutsu option.",
                format_jj_command_label(input)
            ),
            vec!["Pass a plain revision or revset and try again.".into()],
        ));
    }
    Ok(value)
}

fn append_jj_filesets(arguments: &mut Vec<String>, pathspecs: &[String]) {
    if !pathspecs.is_empty() {
        arguments.push("--".into());
        arguments.extend(pathspecs.iter().cloned());
    }
}

pub fn build_jj_diff_args(
    input: &VcsDiffCommandInput,
    pinned: Option<&JujutsuPinnedDiff>,
    snapshot_working_copy: bool,
) -> Result<Vec<String>, WorkdeckUserError> {
    let mut arguments = vec!["diff".into(), "--git".into()];
    let endpoints = match pinned {
        Some(JujutsuPinnedDiff::Range(endpoints)) => Some(endpoints),
        _ => input.range_endpoints.as_ref(),
    };
    if let Some(endpoints) = endpoints {
        let backed = JujutsuBackedInput::from(input);
        let from = require_jj_revision_arg(&backed, &endpoints.from)?;
        let to = require_jj_revision_arg(&backed, &endpoints.to)?;
        if !snapshot_working_copy {
            arguments.push("--ignore-working-copy".into());
        }
        arguments.extend(["--from".into(), from.into(), "--to".into(), to.into()]);
    } else if let Some(revision) = match pinned {
        Some(JujutsuPinnedDiff::Revision(revision)) => Some(revision),
        _ => input.range.as_ref(),
    } {
        arguments.extend(["-r".into(), revision.clone()]);
    }
    append_jj_filesets(&mut arguments, &input.pathspecs);
    Ok(arguments)
}

#[must_use]
pub fn build_jj_show_args(
    input: &VcsShowCommandInput,
    pinned_revision: Option<&str>,
) -> Vec<String> {
    let mut arguments = vec![
        "diff".into(),
        "--git".into(),
        "-r".into(),
        pinned_revision
            .map(str::to_owned)
            .or_else(|| input.reference.clone())
            .unwrap_or_else(|| "@".into()),
    ];
    append_jj_filesets(&mut arguments, &input.pathspecs);
    arguments
}

#[must_use]
pub fn format_jj_command_label(input: &JujutsuBackedInput) -> String {
    match input {
        JujutsuBackedInput::Diff(input) if input.staged => "workdeck diff --staged".into(),
        JujutsuBackedInput::Diff(input) => describe_diff_targets(input).map_or_else(
            || "workdeck diff".into(),
            |targets| format!("workdeck diff {targets}"),
        ),
        JujutsuBackedInput::Show(input) => input.reference.as_ref().map_or_else(
            || "workdeck show".into(),
            |reference| format!("workdeck show {reference}"),
        ),
    }
}

fn first_jj_error_line(stderr: &str) -> String {
    let line = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| stderr.trim());
    if line.to_ascii_lowercase().starts_with("error:") {
        return line["error:".len()..].trim().to_owned();
    }
    if line.is_empty() {
        "Jujutsu command failed.".into()
    } else {
        line.into()
    }
}

fn missing_jj_executable_error(
    input: &JujutsuBackedInput,
    context: &JujutsuCommandContext,
) -> WorkdeckUserError {
    WorkdeckUserError::new(
        format!(
            "Jujutsu is required for `{}` when `vcs = \"jj\"`, but `{}` was not found in PATH.",
            format_jj_command_label(input),
            context.jj_executable.display()
        ),
        vec!["Install Jujutsu or set `vcs = \"git\"` in Workdeck config, then try again.".into()],
    )
}

fn missing_jj_repo_error(input: &JujutsuBackedInput) -> WorkdeckUserError {
    WorkdeckUserError::new(
        format!(
            "`{}` must be run inside a Jujutsu repository when `vcs = \"jj\"`.",
            format_jj_command_label(input)
        ),
        vec![
            "Run the command from a Jujutsu checkout, or set `vcs = \"git\"` in Workdeck config."
                .into(),
        ],
    )
}

#[must_use]
pub fn create_jj_staged_error(input: &VcsDiffCommandInput) -> WorkdeckUserError {
    WorkdeckUserError::new(
        format!(
            "`{}` requires Git VCS mode because Jujutsu has no staging area.",
            format_jj_command_label(&JujutsuBackedInput::from(input))
        ),
        vec!["Remove `--staged`, or set `vcs = \"git\"` in Workdeck config.".into()],
    )
}

fn invalid_revset_error(input: &JujutsuBackedInput) -> WorkdeckUserError {
    match input {
        JujutsuBackedInput::Diff(input) if input.range_endpoints.is_some() => {
            let endpoints = input.range_endpoints.as_ref().unwrap();
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Jujutsu revisions `{}` and `{}`.",
                    format_jj_command_label(&JujutsuBackedInput::Diff(input.clone())),
                    endpoints.from,
                    endpoints.to
                ),
                vec!["Check both revisions and try again.".into()],
            )
        }
        JujutsuBackedInput::Diff(input) => {
            let revset = input.range.as_deref().unwrap_or("");
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Jujutsu revset `{revset}`.",
                    format_jj_command_label(&JujutsuBackedInput::Diff(input.clone()))
                ),
                vec!["Check the revset and try again.".into()],
            )
        }
        JujutsuBackedInput::Show(input) => {
            let revset = input.reference.as_deref().unwrap_or("@");
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Jujutsu revset `{revset}`.",
                    format_jj_command_label(&JujutsuBackedInput::Show(input.clone()))
                ),
                vec!["Check the revset and try again.".into()],
            )
        }
    }
}

fn translate_jj_exit_failure(input: &JujutsuBackedInput, stderr: &str) -> WorkdeckUserError {
    if ["There is no jj repo in", "not in a workspace"]
        .iter()
        .any(|fragment| stderr.contains(fragment))
    {
        return missing_jj_repo_error(input);
    }
    if [
        "Failed to parse revset",
        "Revision not found",
        "No such revision",
        "doesn't exist",
        "is ambiguous",
        "Revset expression resolved to no revisions",
    ]
    .iter()
    .any(|fragment| stderr.contains(fragment))
    {
        return invalid_revset_error(input);
    }
    WorkdeckUserError::new(
        format!("`{}` failed.", format_jj_command_label(input)),
        vec![first_jj_error_line(stderr)],
    )
}

pub fn run_jj_text(
    input: &JujutsuBackedInput,
    arguments: &[String],
    context: &JujutsuCommandContext,
) -> Result<String, VcsCatalogError> {
    let output = Command::new(&context.jj_executable)
        .args(["--no-pager", "--color", "never"])
        .args(arguments)
        .current_dir(&context.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                VcsCatalogError::User(missing_jj_executable_error(input, context))
            } else {
                VcsCatalogError::Operation(error.to_string())
            }
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = if stderr.trim().is_empty() {
            format!(
                "Command failed: {} --no-pager --color never {}",
                context.jj_executable.display(),
                arguments.join(" ")
            )
        } else {
            stderr.trim().to_owned()
        };
        return Err(VcsCatalogError::User(translate_jj_exit_failure(
            input, &detail,
        )));
    }
    Ok(stdout)
}

pub fn resolve_jj_diff_endpoints(
    input: &JujutsuBackedInput,
    revset: &str,
    context: &JujutsuCommandContext,
) -> Result<Option<JujutsuDiffEndpoints>, VcsCatalogError> {
    let commit_ids = parse_jj_commit_ids(&run_jj_text(
        input,
        &[
            "log".into(),
            "--no-graph".into(),
            "-r".into(),
            revset.into(),
            "-T".into(),
            JJ_COMMIT_ID_TEMPLATE.into(),
        ],
        context,
    )?);
    if commit_ids.len() != 1 {
        return Ok(None);
    }
    let commit_id = commit_ids[0].clone();
    let mut parent_commit_ids = parse_jj_commit_ids(&run_jj_text(
        input,
        &[
            "log".into(),
            "--no-graph".into(),
            "--ignore-working-copy".into(),
            "-r".into(),
            format!("{commit_id}-"),
            "-T".into(),
            JJ_COMMIT_ID_TEMPLATE.into(),
        ],
        context,
    )?);
    parent_commit_ids.sort();
    Ok(Some(JujutsuDiffEndpoints {
        new_commit_id: commit_id,
        old_commit_ids: parent_commit_ids,
    }))
}

pub fn resolve_jj_range_endpoints(
    input: &VcsDiffCommandInput,
    endpoints: &VcsRangeEndpoints,
    context: &JujutsuCommandContext,
) -> Result<Option<JujutsuDiffEndpoints>, VcsCatalogError> {
    let backed = JujutsuBackedInput::from(input);
    let from = require_jj_revision_arg(&backed, &endpoints.from).map_err(VcsCatalogError::User)?;
    let to = require_jj_revision_arg(&backed, &endpoints.to).map_err(VcsCatalogError::User)?;
    let resolve_one = |revset: &str| {
        run_jj_text(
            &backed,
            &[
                "log".into(),
                "--no-graph".into(),
                "-r".into(),
                revset.into(),
                "-T".into(),
                JJ_COMMIT_ID_TEMPLATE.into(),
            ],
            context,
        )
        .map(|output| parse_jj_commit_ids(&output))
    };
    let from_commit_ids = resolve_one(from)?;
    let to_commit_ids = resolve_one(to)?;
    if from_commit_ids.len() != 1 || to_commit_ids.len() != 1 {
        return Ok(None);
    }
    Ok(Some(JujutsuDiffEndpoints {
        new_commit_id: to_commit_ids[0].clone(),
        old_commit_ids: vec![from_commit_ids[0].clone()],
    }))
}

pub fn resolve_jj_repo_root(
    input: &JujutsuBackedInput,
    context: &JujutsuCommandContext,
) -> Result<PathBuf, VcsCatalogError> {
    let root = run_jj_text(input, &["root".into()], context)?;
    Ok(PathBuf::from(normalize_path_for_os(root.trim())))
}

#[cfg(test)]
mod tests;
