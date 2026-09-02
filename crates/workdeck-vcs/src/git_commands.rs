//! Exact Git command construction, discovery, and user-facing failure translation.

use crate::{
    LARGE_DIFF_FILE_MAX_BYTES, LARGE_DIFF_FILE_MAX_LINES, VcsCatalogError, describe_diff_range,
    describe_diff_targets, normalize_path_for_os,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use workdeck_core::{
    CommonOptions, VcsDiffCommandInput, VcsShowCommandInput, VcsStashShowCommandInput,
    WorkdeckUserError,
};

const DIFF_PREFIX_NORMALIZATION_ARGS: &[&str] = &[
    "-c",
    "core.quotePath=true",
    "-c",
    "diff.noprefix=false",
    "-c",
    "diff.mnemonicPrefix=false",
    "-c",
    "diff.srcPrefix=a/",
    "-c",
    "diff.dstPrefix=b/",
];

const GIT_MOVED_LINE_COLOR_CONFIG: &[&str] = &[
    "-c",
    "color.diff.oldMoved=magenta bold",
    "-c",
    "color.diff.oldMovedAlternative=magenta bold",
    "-c",
    "color.diff.oldMovedDimmed=magenta dim",
    "-c",
    "color.diff.oldMovedAlternativeDimmed=magenta dim",
    "-c",
    "color.diff.newMoved=cyan bold",
    "-c",
    "color.diff.newMovedAlternative=cyan bold",
    "-c",
    "color.diff.newMovedDimmed=cyan dim",
    "-c",
    "color.diff.newMovedAlternativeDimmed=cyan dim",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitBackedInput {
    Diff(VcsDiffCommandInput),
    Show(VcsShowCommandInput),
    StashShow(VcsStashShowCommandInput),
}

impl GitBackedInput {
    fn options(&self) -> &CommonOptions {
        match self {
            Self::Diff(input) => &input.options,
            Self::Show(input) => &input.options,
            Self::StashShow(input) => &input.options,
        }
    }
}

impl From<&VcsDiffCommandInput> for GitBackedInput {
    fn from(input: &VcsDiffCommandInput) -> Self {
        Self::Diff(input.clone())
    }
}

impl From<&VcsShowCommandInput> for GitBackedInput {
    fn from(input: &VcsShowCommandInput) -> Self {
        Self::Show(input.clone())
    }
}

impl From<&VcsStashShowCommandInput> for GitBackedInput {
    fn from(input: &VcsStashShowCommandInput) -> Self {
        Self::StashShow(input.clone())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommandContext {
    pub cwd: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub git_executable: PathBuf,
    pub prevent_optional_locks: bool,
}

impl Default for GitCommandContext {
    fn default() -> Self {
        Self {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            repo_root: None,
            git_executable: PathBuf::from("git"),
            prevent_optional_locks: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitColorMovedOptions {
    pub mode: String,
    pub whitespace_mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitNumstatFile {
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitMetadata {
    pub repo_root: PathBuf,
    pub git_dir: PathBuf,
    pub common_dir: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitDiffEndpoint {
    None,
    GitRef(String),
    Index,
    Worktree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitDiffEndpoints {
    pub old: GitDiffEndpoint,
    pub new: GitDiffEndpoint,
}

#[derive(Debug)]
struct RunGitCommandResult {
    stderr: String,
    stdout: String,
    exit_code: Option<i32>,
}

pub fn append_git_pathspecs(arguments: &mut Vec<String>, pathspecs: &[String]) {
    if !pathspecs.is_empty() {
        arguments.push("--".into());
        arguments.extend(pathspecs.iter().cloned());
    }
}

pub fn require_git_revision_arg<'a>(
    input: &GitBackedInput,
    value: &'a str,
) -> Result<&'a str, WorkdeckUserError> {
    if value.is_empty() {
        return Err(WorkdeckUserError::new(
            format!(
                "`{}` refused an empty revision.",
                format_git_command_label(input)
            ),
            vec!["Pass a non-empty revision or range and try again.".into()],
        ));
    }
    if value.starts_with('-') {
        return Err(WorkdeckUserError::new(
            format!(
                "`{}` refused revision `{value}` because it looks like a Git option.",
                format_git_command_label(input)
            ),
            vec!["Pass a plain revision or range, such as `HEAD` or `main..feature`.".into()],
        ));
    }
    Ok(value)
}

fn require_git_diff_range_arg(
    input: &VcsDiffCommandInput,
) -> Result<Option<String>, WorkdeckUserError> {
    let backed = GitBackedInput::from(input);
    if let Some(endpoints) = &input.range_endpoints {
        let from = require_git_revision_arg(&backed, &endpoints.from)?;
        let to = require_git_revision_arg(&backed, &endpoints.to)?;
        return Ok(Some(format!("{from}..{to}")));
    }
    input
        .range
        .as_deref()
        .map(|range| require_git_revision_arg(&backed, range).map(str::to_owned))
        .transpose()
}

fn with_normalized_diff_prefixes(arguments: Vec<String>) -> Vec<String> {
    DIFF_PREFIX_NORMALIZATION_ARGS
        .iter()
        .map(|argument| (*argument).to_owned())
        .chain(arguments)
        .collect()
}

fn git_patch_color_args(color_moved: Option<&GitColorMovedOptions>) -> Vec<String> {
    let Some(color_moved) = color_moved else {
        return vec!["--no-color".into()];
    };
    let mut arguments = vec![
        "--color=always".into(),
        format!("--color-moved={}", color_moved.mode),
    ];
    if let Some(whitespace_mode) = &color_moved.whitespace_mode {
        arguments.push(format!("--color-moved-ws={whitespace_mode}"));
    }
    arguments
}

fn with_git_moved_line_color_config(
    arguments: Vec<String>,
    color_moved: Option<&GitColorMovedOptions>,
) -> Vec<String> {
    if color_moved.is_none() {
        return arguments;
    }
    GIT_MOVED_LINE_COLOR_CONFIG
        .iter()
        .map(|argument| (*argument).to_owned())
        .chain(arguments)
        .collect()
}

pub fn build_git_diff_args(
    input: &VcsDiffCommandInput,
    excluded_pathspecs: &[String],
    color_moved: Option<&GitColorMovedOptions>,
) -> Result<Vec<String>, WorkdeckUserError> {
    let mut arguments = vec![
        "diff".into(),
        "--no-ext-diff".into(),
        "--find-renames".into(),
    ];
    arguments.extend(git_patch_color_args(color_moved));
    if input.staged {
        arguments.push("--staged".into());
    }
    if let Some(range) = require_git_diff_range_arg(input)? {
        arguments.push(range);
    }
    if excluded_pathspecs.is_empty() {
        append_git_pathspecs(&mut arguments, &input.pathspecs);
    } else {
        arguments.push("--".into());
        arguments.extend(input.pathspecs.iter().cloned());
        arguments.extend(
            excluded_pathspecs
                .iter()
                .map(|path| format!(":(exclude){path}")),
        );
    }
    Ok(with_normalized_diff_prefixes(
        with_git_moved_line_color_config(arguments, color_moved),
    ))
}

pub fn build_git_diff_numstat_args(
    input: &VcsDiffCommandInput,
) -> Result<Vec<String>, WorkdeckUserError> {
    let mut arguments = [
        "diff",
        "--no-ext-diff",
        "--find-renames",
        "--no-color",
        "--numstat",
        "-z",
    ]
    .map(str::to_owned)
    .to_vec();
    if input.staged {
        arguments.push("--staged".into());
    }
    if let Some(range) = require_git_diff_range_arg(input)? {
        arguments.push(range);
    }
    append_git_pathspecs(&mut arguments, &input.pathspecs);
    Ok(with_normalized_diff_prefixes(arguments))
}

pub fn parse_git_numstat(text: &str) -> Vec<GitNumstatFile> {
    text.split('\0')
        .filter_map(|entry| {
            let mut fields = entry.split('\t');
            let additions = fields.next()?.parse().ok()?;
            let deletions = fields.next()?.parse().ok()?;
            let path = fields.next()?.to_owned();
            (!path.is_empty()).then_some(GitNumstatFile {
                path,
                additions,
                deletions,
            })
        })
        .collect()
}

pub fn should_skip_large_tracked_diff(file: &GitNumstatFile, repo_root: &Path) -> bool {
    if file.additions.saturating_add(file.deletions) > LARGE_DIFF_FILE_MAX_LINES {
        return true;
    }
    fs::metadata(repo_root.join(&file.path))
        .is_ok_and(|metadata| metadata.len() > LARGE_DIFF_FILE_MAX_BYTES)
}

pub fn build_git_status_args(input: &VcsDiffCommandInput) -> Vec<String> {
    let mut arguments = [
        "--no-optional-locks",
        "status",
        "--porcelain=v1",
        "-z",
        "--untracked-files=all",
    ]
    .map(str::to_owned)
    .to_vec();
    append_git_pathspecs(&mut arguments, &input.pathspecs);
    arguments
}

pub fn build_git_ignored_directory_args() -> Vec<String> {
    [
        "ls-files",
        "--full-name",
        "--others",
        "--ignored",
        "--exclude-standard",
        "--directory",
        "-z",
    ]
    .map(str::to_owned)
    .to_vec()
}

pub fn build_git_show_args(
    input: &VcsShowCommandInput,
    color_moved: Option<&GitColorMovedOptions>,
) -> Result<Vec<String>, WorkdeckUserError> {
    let mut arguments = ["show", "--format=", "--no-ext-diff", "--find-renames"]
        .map(str::to_owned)
        .to_vec();
    arguments.extend(git_patch_color_args(color_moved));
    if let Some(reference) = &input.reference {
        arguments.push(require_git_revision_arg(&input.into(), reference)?.to_owned());
    }
    append_git_pathspecs(&mut arguments, &input.pathspecs);
    Ok(with_normalized_diff_prefixes(
        with_git_moved_line_color_config(arguments, color_moved),
    ))
}

pub fn build_git_stash_show_args(
    input: &VcsStashShowCommandInput,
    color_moved: Option<&GitColorMovedOptions>,
) -> Result<Vec<String>, WorkdeckUserError> {
    let mut arguments = ["stash", "show", "-p", "--no-ext-diff", "--find-renames"]
        .map(str::to_owned)
        .to_vec();
    arguments.extend(git_patch_color_args(color_moved));
    if let Some(reference) = &input.reference {
        arguments.push(require_git_revision_arg(&input.into(), reference)?.to_owned());
    }
    Ok(with_normalized_diff_prefixes(
        with_git_moved_line_color_config(arguments, color_moved),
    ))
}

pub fn format_git_command_label(input: &GitBackedInput) -> String {
    match input {
        GitBackedInput::Diff(input) if input.staged => "workdeck diff --staged".into(),
        GitBackedInput::Diff(input) => describe_diff_targets(input).map_or_else(
            || "workdeck diff".into(),
            |targets| format!("workdeck diff {targets}"),
        ),
        GitBackedInput::Show(input) => input.reference.as_ref().map_or_else(
            || "workdeck show".into(),
            |reference| format!("workdeck show {reference}"),
        ),
        GitBackedInput::StashShow(input) => input.reference.as_ref().map_or_else(
            || "workdeck stash show".into(),
            |reference| format!("workdeck stash show {reference}"),
        ),
    }
}

fn missing_repo_help(input: &GitBackedInput) -> Vec<String> {
    if matches!(input, GitBackedInput::Diff(_)) {
        vec![
            "Run the command from a Git checkout, or compare files directly instead:".into(),
            "  workdeck diff --files <before-file> <after-file>".into(),
            "  workdeck patch <file.patch>".into(),
        ]
    } else {
        vec!["Run the command from a Git checkout.".into()]
    }
}

fn first_git_error_line(stderr: &str) -> String {
    let message = stderr
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| stderr.trim());
    let lower = message.to_ascii_lowercase();
    for prefix in ["fatal:", "error:"] {
        if lower.starts_with(prefix) {
            return message[prefix.len()..].trim().to_owned();
        }
    }
    if message.is_empty() {
        "Git command failed.".into()
    } else {
        message.into()
    }
}

fn is_missing_git_repo_message(stderr: &str) -> bool {
    stderr.contains("not a git repository")
}

fn is_unknown_revision_message(stderr: &str) -> bool {
    [
        "bad revision",
        "unknown revision or path not in the working tree",
        "ambiguous argument",
        "Needed a single revision",
    ]
    .iter()
    .any(|fragment| stderr.contains(fragment))
}

fn is_no_stash_entries_message(stderr: &str) -> bool {
    ["No stash entries found.", "log for 'stash' only has"]
        .iter()
        .any(|fragment| stderr.contains(fragment))
}

fn missing_git_executable_error(
    input: &GitBackedInput,
    git_executable: &Path,
) -> WorkdeckUserError {
    WorkdeckUserError::new(
        format!(
            "Git is required for `{}`, but `{}` was not found in PATH.",
            format_git_command_label(input),
            git_executable.display()
        ),
        vec!["Install Git or make it available on PATH, then try again.".into()],
    )
}

fn missing_repo_error(input: &GitBackedInput) -> WorkdeckUserError {
    WorkdeckUserError::new(
        format!(
            "`{}` must be run inside a Git repository.",
            format_git_command_label(input)
        ),
        missing_repo_help(input),
    )
}

fn invalid_revision_error(input: &GitBackedInput) -> WorkdeckUserError {
    match input {
        GitBackedInput::Diff(diff) => {
            let mut suggestions = vec!["Check the revision or range and try again.".into()];
            if let Some(endpoints) = &diff.range_endpoints {
                suggestions.push(format!(
                    "To limit the review to a path, separate it: `workdeck diff {} -- {}`.",
                    endpoints.from, endpoints.to
                ));
            }
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Git revision or range `{}`.",
                    format_git_command_label(input),
                    describe_diff_range(diff).unwrap_or_default()
                ),
                suggestions,
            )
        }
        GitBackedInput::Show(show) => {
            let reference = show.reference.as_deref().unwrap_or("HEAD");
            WorkdeckUserError::new(
                format!(
                    "`{}` could not resolve Git ref `{reference}`.",
                    format_git_command_label(input)
                ),
                vec!["Check the ref name and try again.".into()],
            )
        }
        GitBackedInput::StashShow(_) => unreachable!("stash errors use stash translation"),
    }
}

fn missing_stash_error(input: &GitBackedInput) -> WorkdeckUserError {
    let GitBackedInput::StashShow(stash) = input else {
        unreachable!("only stash input can produce missing stash errors");
    };
    if let Some(reference) = &stash.reference {
        WorkdeckUserError::new(
            format!(
                "`{}` could not resolve stash entry `{reference}`.",
                format_git_command_label(input)
            ),
            vec!["List available stashes with `git stash list`, then try again.".into()],
        )
    } else {
        WorkdeckUserError::new(
            "`workdeck stash show` could not find a stash entry to show.",
            vec![
                "Create one with `git stash push`, or pass an explicit stash ref like `workdeck stash show stash@{0}`."
                    .into(),
            ],
        )
    }
}

fn translate_git_exit_failure(input: &GitBackedInput, stderr: &str) -> WorkdeckUserError {
    if is_missing_git_repo_message(stderr) {
        return missing_repo_error(input);
    }
    if matches!(input, GitBackedInput::StashShow(_))
        && (is_no_stash_entries_message(stderr) || is_unknown_revision_message(stderr))
    {
        return missing_stash_error(input);
    }
    if matches!(input, GitBackedInput::Diff(diff) if describe_diff_range(diff).is_some())
        && is_unknown_revision_message(stderr)
    {
        return invalid_revision_error(input);
    }
    if matches!(input, GitBackedInput::Show(_)) && is_unknown_revision_message(stderr) {
        return invalid_revision_error(input);
    }
    WorkdeckUserError::new(
        format!("`{}` failed.", format_git_command_label(input)),
        vec![first_git_error_line(stderr)],
    )
}

fn run_git_command(
    input: &GitBackedInput,
    arguments: &[String],
    context: &GitCommandContext,
    accepted_exit_codes: &[i32],
) -> Result<RunGitCommandResult, VcsCatalogError> {
    let mut command = Command::new(&context.git_executable);
    command
        .args(arguments)
        .current_dir(&context.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if context.prevent_optional_locks {
        command.env("GIT_OPTIONAL_LOCKS", "0");
    }
    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            VcsCatalogError::User(missing_git_executable_error(input, &context.git_executable))
        } else {
            VcsCatalogError::Operation(error.to_string())
        }
    })?;
    command_result(input, arguments, context, output, accepted_exit_codes)
}

fn command_result(
    input: &GitBackedInput,
    arguments: &[String],
    context: &GitCommandContext,
    output: Output,
    accepted_exit_codes: &[i32],
) -> Result<RunGitCommandResult, VcsCatalogError> {
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code();
    if !exit_code.is_some_and(|code| accepted_exit_codes.contains(&code)) {
        let detail = if stderr.trim().is_empty() {
            format!(
                "Command failed: {} {}",
                context.git_executable.display(),
                arguments.join(" ")
            )
        } else {
            stderr.trim().to_owned()
        };
        return Err(VcsCatalogError::User(translate_git_exit_failure(
            input, &detail,
        )));
    }
    Ok(RunGitCommandResult {
        stderr,
        stdout,
        exit_code,
    })
}

pub fn run_git_text(
    input: &GitBackedInput,
    arguments: &[String],
    context: &GitCommandContext,
) -> Result<String, VcsCatalogError> {
    Ok(run_git_command(input, arguments, context, &[0])?.stdout)
}

fn read_optional_git_config(
    input: &GitBackedInput,
    key: &str,
    context: &GitCommandContext,
) -> Result<Option<String>, VcsCatalogError> {
    let result = run_git_command(
        input,
        &["config".into(), "--get".into(), key.into()],
        context,
        &[0, 1],
    )?;
    Ok((result.exit_code == Some(0))
        .then(|| result.stdout.trim().to_owned())
        .filter(|value| !value.is_empty()))
}

fn normalize_git_color_moved_mode(value: Option<String>) -> Option<Option<String>> {
    let value = value?;
    let normalized = value.to_ascii_lowercase();
    if ["false", "no", "off", "0", "never"].contains(&normalized.as_str()) {
        return Some(None);
    }
    if ["true", "yes", "on", "1", "always"].contains(&normalized.as_str()) {
        return Some(Some("zebra".into()));
    }
    Some(Some(value))
}

pub fn resolve_git_color_moved_options(
    input: &GitBackedInput,
    context: &GitCommandContext,
) -> Result<Option<GitColorMovedOptions>, VcsCatalogError> {
    let configured = normalize_git_color_moved_mode(read_optional_git_config(
        input,
        "diff.colorMoved",
        context,
    )?);
    let mode = match configured {
        Some(None) => return Ok(None),
        Some(Some(mode)) => mode,
        None if input.options().color_moved == Some(true) => "zebra".into(),
        None => return Ok(None),
    };
    Ok(Some(GitColorMovedOptions {
        mode,
        whitespace_mode: read_optional_git_config(input, "diff.colorMovedWS", context)?,
    }))
}

fn working_tree_cache() -> &'static Mutex<HashMap<String, bool>> {
    static CACHE: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn is_working_tree_git_diff_input(
    input: &VcsDiffCommandInput,
    context: &GitCommandContext,
) -> Result<bool, VcsCatalogError> {
    if input.staged {
        return Ok(false);
    }
    let Some(range) = require_git_diff_range_arg(input).map_err(VcsCatalogError::User)? else {
        return Ok(true);
    };
    let root = context.repo_root.as_deref().unwrap_or(&context.cwd);
    let cache_key = format!(
        "{}\0{}\0{range}",
        context.git_executable.display(),
        root.display()
    );
    if let Some(cached) = working_tree_cache().lock().unwrap().get(&cache_key) {
        return Ok(*cached);
    }
    let backed = GitBackedInput::from(input);
    let revisions = run_git_text(
        &backed,
        &["rev-parse".into(), "--revs-only".into(), range],
        context,
    )?;
    let (positive, negative) = count_revision_polarity(&revisions);
    let includes_working_tree = positive == 1 && negative == 0;
    working_tree_cache()
        .lock()
        .unwrap()
        .insert(cache_key, includes_working_tree);
    Ok(includes_working_tree)
}

fn count_revision_polarity(revisions: &str) -> (usize, usize) {
    revisions
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .fold((0, 0), |(positive, negative), revision| {
            if revision.starts_with('^') {
                (positive, negative + 1)
            } else {
                (positive + 1, negative)
            }
        })
}

fn parse_untracked_file_paths(status: &str) -> Vec<PathBuf> {
    status
        .split('\0')
        .filter_map(|entry| entry.strip_prefix("?? ").map(PathBuf::from))
        .collect()
}

pub fn parse_git_ignored_directory_roots(output: &str, repo_root: &Path) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    output
        .split('\0')
        .filter_map(|entry| entry.strip_suffix('/'))
        .map(|entry| {
            PathBuf::from(normalize_path_for_os(
                &repo_root.join(entry).to_string_lossy(),
            ))
        })
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

pub fn list_git_ignored_directory_roots(
    input: &GitBackedInput,
    context: &GitCommandContext,
) -> Vec<PathBuf> {
    let result = (|| {
        let repo_root = context
            .repo_root
            .clone()
            .map_or_else(|| resolve_git_repo_root(input, context), Ok)?;
        let command_context = GitCommandContext {
            cwd: repo_root.clone(),
            prevent_optional_locks: true,
            ..context.clone()
        };
        let output = run_git_text(input, &build_git_ignored_directory_args(), &command_context)?;
        Ok::<_, VcsCatalogError>(parse_git_ignored_directory_roots(&output, &repo_root))
    })();
    result.unwrap_or_default()
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

pub fn list_git_untracked_files(
    input: &VcsDiffCommandInput,
    context: &GitCommandContext,
) -> Result<Vec<PathBuf>, VcsCatalogError> {
    if input.options.exclude_untracked == Some(true)
        || !is_working_tree_git_diff_input(input, context)?
    {
        return Ok(Vec::new());
    }
    let backed = GitBackedInput::from(input);
    let status = run_git_text(&backed, &build_git_status_args(input), context)?;
    let untracked = parse_untracked_file_paths(&status);
    if untracked.is_empty() {
        return Ok(Vec::new());
    }
    let repo_root = context
        .repo_root
        .clone()
        .map_or_else(|| resolve_git_repo_root(&backed, context), Ok)?;
    Ok(untracked
        .into_iter()
        .filter(|path| is_reviewable_untracked_path(&repo_root, path))
        .collect())
}

pub fn resolve_git_repo_root(
    input: &GitBackedInput,
    context: &GitCommandContext,
) -> Result<PathBuf, VcsCatalogError> {
    let output = run_git_text(
        input,
        &["rev-parse".into(), "--show-toplevel".into()],
        context,
    )?;
    Ok(PathBuf::from(normalize_path_for_os(output.trim())))
}

pub fn resolve_git_metadata(
    input: &GitBackedInput,
    context: &GitCommandContext,
) -> Result<GitMetadata, VcsCatalogError> {
    let repo_root = resolve_git_repo_root(input, context)?;
    let git_dir = PathBuf::from(normalize_path_for_os(
        run_git_text(
            input,
            &["rev-parse".into(), "--absolute-git-dir".into()],
            context,
        )?
        .trim(),
    ));
    let absolute_common = run_git_command(
        input,
        &[
            "rev-parse".into(),
            "--path-format=absolute".into(),
            "--git-common-dir".into(),
        ],
        context,
        &[0, 128, 129],
    )?;
    let common_output = if absolute_common.exit_code == Some(0) {
        absolute_common.stdout.trim().to_owned()
    } else {
        run_git_text(
            input,
            &["rev-parse".into(), "--git-common-dir".into()],
            context,
        )?
        .trim()
        .to_owned()
    };
    let common = PathBuf::from(normalize_path_for_os(&common_output));
    let common_dir = if common.is_absolute() {
        common
    } else {
        context.cwd.join(common)
    };
    Ok(GitMetadata {
        repo_root,
        git_dir,
        common_dir,
    })
}

pub fn resolve_git_commit_ref(
    input: &GitBackedInput,
    reference: &str,
    context: &GitCommandContext,
) -> Result<String, VcsCatalogError> {
    Ok(run_git_text(
        input,
        &[
            "rev-parse".into(),
            "--verify".into(),
            "--end-of-options".into(),
            format!("{reference}^{{commit}}"),
        ],
        context,
    )?
    .lines()
    .next()
    .unwrap_or_default()
    .trim()
    .to_owned())
}

fn try_resolve_git_commit_ref(
    input: &GitBackedInput,
    reference: &str,
    context: &GitCommandContext,
) -> Result<Option<String>, VcsCatalogError> {
    let result = run_git_command(
        input,
        &[
            "rev-parse".into(),
            "--verify".into(),
            "--end-of-options".into(),
            format!("{reference}^{{commit}}"),
        ],
        context,
        &[0, 1, 128],
    )?;
    if result.exit_code == Some(0) {
        return Ok(Some(
            result
                .stdout
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .to_owned(),
        ));
    }
    if is_unknown_revision_message(&result.stderr) {
        return Ok(None);
    }
    let detail = if result.stderr.trim().is_empty() {
        format!("Could not resolve Git ref {reference}.")
    } else {
        result.stderr.trim().to_owned()
    };
    Err(VcsCatalogError::User(translate_git_exit_failure(
        input, &detail,
    )))
}

fn parse_symmetric_diff_range(range: &str) -> Option<(&str, &str)> {
    if range.contains("....") {
        return None;
    }
    let mut parts = range.split("...");
    let left = parts.next()?;
    let right = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    Some((
        if left.is_empty() { "HEAD" } else { left },
        if right.is_empty() { "HEAD" } else { right },
    ))
}

fn resolve_range_revisions(
    input: &VcsDiffCommandInput,
    range: &str,
    context: &GitCommandContext,
) -> Result<(Vec<String>, Vec<String>), VcsCatalogError> {
    let backed = GitBackedInput::from(input);
    let command_context = GitCommandContext {
        cwd: context
            .repo_root
            .clone()
            .unwrap_or_else(|| context.cwd.clone()),
        ..context.clone()
    };
    let output = run_git_text(
        &backed,
        &["rev-parse".into(), "--revs-only".into(), range.into()],
        &command_context,
    )?;
    let mut positives = Vec::new();
    let mut negatives = Vec::new();
    for revision in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(revision) = revision.strip_prefix('^') {
            negatives.push(revision.to_owned());
        } else {
            positives.push(revision.to_owned());
        }
    }
    Ok((positives, negatives))
}

pub fn resolve_git_diff_endpoints(
    input: &VcsDiffCommandInput,
    context: &GitCommandContext,
) -> Result<Option<GitDiffEndpoints>, VcsCatalogError> {
    let range = require_git_diff_range_arg(input).map_err(VcsCatalogError::User)?;
    let backed = GitBackedInput::from(input);
    let command_context = GitCommandContext {
        cwd: context
            .repo_root
            .clone()
            .unwrap_or_else(|| context.cwd.clone()),
        ..context.clone()
    };
    if input.staged {
        let Some(range) = range else {
            let head = try_resolve_git_commit_ref(&backed, "HEAD", &command_context)?;
            return Ok(Some(GitDiffEndpoints {
                old: head.map_or(GitDiffEndpoint::None, GitDiffEndpoint::GitRef),
                new: GitDiffEndpoint::Index,
            }));
        };
        let (positives, negatives) = resolve_range_revisions(input, &range, context)?;
        return Ok(
            (positives.len() == 1 && negatives.is_empty()).then(|| GitDiffEndpoints {
                old: GitDiffEndpoint::GitRef(positives[0].clone()),
                new: GitDiffEndpoint::Index,
            }),
        );
    }
    let Some(range) = range else {
        return Ok(Some(GitDiffEndpoints {
            old: GitDiffEndpoint::Index,
            new: GitDiffEndpoint::Worktree,
        }));
    };
    if let Some((left, right)) = parse_symmetric_diff_range(&range) {
        let merge_base = run_git_text(
            &backed,
            &["merge-base".into(), left.into(), right.into()],
            &command_context,
        )?
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
        if merge_base.is_empty() {
            return Ok(None);
        }
        let right = resolve_git_commit_ref(&backed, right, &command_context)?;
        return Ok(Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(merge_base),
            new: GitDiffEndpoint::GitRef(right),
        }));
    }
    let (positives, negatives) = resolve_range_revisions(input, &range, context)?;
    if positives.len() == 1 && negatives.is_empty() {
        return Ok(Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(positives[0].clone()),
            new: GitDiffEndpoint::Worktree,
        }));
    }
    if positives.len() == 1 && negatives.len() == 1 {
        return Ok(Some(GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(negatives[0].clone()),
            new: GitDiffEndpoint::GitRef(positives[0].clone()),
        }));
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
