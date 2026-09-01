//! VCS adapters that normalize provider output into one Workdeck changeset model.

mod catalog;
mod large_file;
mod platform;
mod source_text;
mod untracked;

pub use catalog::*;
pub use large_file::{
    LARGE_DIFF_FILE_MAX_BYTES, LARGE_DIFF_FILE_MAX_LINES, LargeFileCheck,
    inspect_large_untracked_file,
};
pub use platform::{normalize_path_for_os, normalize_path_for_platform};
pub use source_text::{
    DEFAULT_SOURCE_TEXT_MAX_BYTES, LimitedSourceTextResult, SourceSubprocess, SourceTextError,
    log_source_diagnostic, read_file_text_with_limit, read_stream_text_with_limit,
    terminate_source_subprocess,
};
pub use untracked::build_filesystem_untracked_diff_file;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use thiserror::Error;
use workdeck_core::{
    Changeset, ChangesetSource, FileSourceSnapshots, SourceOrigin, SourceSnapshot,
};
use workdeck_diff::{PatchError, parse_patch};

const MAX_PATCH_BYTES: usize = 64 * 1024 * 1024;
const MAX_SOURCE_BYTES: u64 = DEFAULT_SOURCE_TEXT_MAX_BYTES as u64;
const BINARY_SNIFF_BYTES: usize = 8_000;

#[derive(Debug, Error)]
pub enum VcsError {
    #[error("{0}")]
    InvalidRevision(String),
    #[error("{program} failed with exit code {code:?}: {stderr}")]
    Command {
        program: String,
        code: Option<i32>,
        stderr: String,
    },
    #[error("patch output exceeded {0} bytes")]
    PatchTooLarge(usize),
    #[error("failed to read {path}: {source}")]
    ReadFile {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Patch(#[from] PatchError),
}

#[derive(Debug, Clone, Default)]
pub struct DiffRequest {
    pub target: Option<String>,
    pub from: Option<String>,
    pub staged: bool,
    pub exclude_untracked: bool,
    pub pathspec: Vec<String>,
    /// Git's `diff.colorMoved` wins when present; `Some(true)` enables zebra mode otherwise.
    pub color_moved: Option<bool>,
}

pub trait VcsProvider {
    fn name(&self) -> &'static str;
    fn working_tree(&self, request: &DiffRequest) -> Result<Changeset, VcsError>;
    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError>;
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProviderPreference {
    #[default]
    Auto,
    Git,
    Jujutsu,
    Sapling,
}

impl ProviderPreference {
    pub fn parse(value: &str) -> Result<Self, VcsError> {
        match value {
            "auto" => Ok(Self::Auto),
            "git" => Ok(Self::Git),
            "jj" => Ok(Self::Jujutsu),
            "sl" => Ok(Self::Sapling),
            other => Err(VcsError::InvalidRevision(format!(
                "unsupported VCS provider {other:?}; expected auto, git, jj, or sl"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub enum AnyProvider {
    Git(GitProvider),
    Jujutsu(JujutsuProvider),
    Sapling(SaplingProvider),
}

impl AnyProvider {
    pub fn discover(cwd: &Path, preference: ProviderPreference) -> Result<Self, VcsError> {
        match preference {
            ProviderPreference::Git => GitProvider::discover(cwd).map(Self::Git),
            ProviderPreference::Jujutsu => JujutsuProvider::discover(cwd).map(Self::Jujutsu),
            ProviderPreference::Sapling => SaplingProvider::discover(cwd).map(Self::Sapling),
            ProviderPreference::Auto => {
                if find_marker(cwd, ".jj").is_some() {
                    return JujutsuProvider::discover(cwd).map(Self::Jujutsu);
                }
                if find_marker(cwd, ".sl").is_some() {
                    return SaplingProvider::discover(cwd).map(Self::Sapling);
                }
                GitProvider::discover(cwd).map(Self::Git)
            }
        }
    }

    pub fn root(&self) -> &Path {
        match self {
            Self::Git(provider) => provider.root(),
            Self::Jujutsu(provider) => provider.root(),
            Self::Sapling(provider) => provider.root(),
        }
    }
}

impl VcsProvider for AnyProvider {
    fn name(&self) -> &'static str {
        match self {
            Self::Git(provider) => provider.name(),
            Self::Jujutsu(provider) => provider.name(),
            Self::Sapling(provider) => provider.name(),
        }
    }

    fn working_tree(&self, request: &DiffRequest) -> Result<Changeset, VcsError> {
        match self {
            Self::Git(provider) => provider.working_tree(request),
            Self::Jujutsu(provider) => provider.working_tree(request),
            Self::Sapling(provider) => provider.working_tree(request),
        }
    }

    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError> {
        match self {
            Self::Git(provider) => provider.show(target, pathspec),
            Self::Jujutsu(provider) => provider.show(target, pathspec),
            Self::Sapling(provider) => provider.show(target, pathspec),
        }
    }
}

#[derive(Debug, Clone)]
pub struct GitProvider {
    root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GitColorMovedOptions {
    mode: String,
    whitespace_mode: Option<String>,
}

const GIT_DIFF_PREFIX_CONFIG: &[&str] = &[
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

const GIT_MOVED_COLOR_CONFIG: &[&str] = &[
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

impl GitProvider {
    pub fn discover(cwd: &Path) -> Result<Self, VcsError> {
        let output = run(cwd, "git", &["rev-parse", "--show-toplevel"], &[0])?;
        let root = String::from_utf8_lossy(&output.stdout);
        Ok(Self {
            root: PathBuf::from(normalize_path_for_os(root.trim())),
        })
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn read_optional_config(&self, key: &str) -> Option<String> {
        let output = Command::new("git")
            .args(["config", "--get", key])
            .current_dir(&self.root)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        (!value.is_empty()).then_some(value)
    }

    fn color_moved_options(&self, requested: Option<bool>) -> Option<GitColorMovedOptions> {
        let configured = self.read_optional_config("diff.colorMoved");
        let mode = match configured.as_deref().map(str::to_ascii_lowercase) {
            Some(value) if ["false", "no", "off", "0", "none"].contains(&value.as_str()) => {
                return None;
            }
            Some(value) if ["true", "yes", "on", "1"].contains(&value.as_str()) => "zebra".into(),
            Some(_) => configured.expect("configured value was present"),
            None if requested == Some(true) => "zebra".into(),
            None => return None,
        };
        Some(GitColorMovedOptions {
            mode,
            whitespace_mode: self.read_optional_config("diff.colorMovedWS"),
        })
    }

    fn patch_command_prefix(&self, moved: Option<&GitColorMovedOptions>) -> Vec<String> {
        let mut arguments = GIT_DIFF_PREFIX_CONFIG
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();
        if moved.is_some() {
            arguments.extend(
                GIT_MOVED_COLOR_CONFIG
                    .iter()
                    .map(|value| (*value).to_owned()),
            );
        }
        arguments
    }

    fn append_patch_color_args(arguments: &mut Vec<String>, moved: Option<&GitColorMovedOptions>) {
        if let Some(moved) = moved {
            arguments.push("--color=always".into());
            arguments.push(format!("--color-moved={}", moved.mode));
            if let Some(whitespace) = &moved.whitespace_mode {
                arguments.push(format!("--color-moved-ws={whitespace}"));
            }
        } else {
            arguments.push("--no-color".into());
        }
    }

    pub fn stash(&self, reference: Option<&str>) -> Result<Changeset, VcsError> {
        let reference = reference.unwrap_or("stash@{0}");
        validate_revision(reference)?;
        let moved = self.color_moved_options(None);
        let mut arguments = self.patch_command_prefix(moved.as_ref());
        arguments.extend([
            "stash".into(),
            "show".into(),
            "--patch".into(),
            "--find-renames".into(),
        ]);
        Self::append_patch_color_args(&mut arguments, moved.as_ref());
        arguments.push(reference.into());
        let borrowed = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        let patch = output_text(run(&self.root, "git", &borrowed, &[0])?)?;
        let mut changeset = parse_patch(
            &patch,
            format!("git:stash:{reference}"),
            format!("Stash {reference}"),
            ChangesetSource::Stash {
                reference: reference.to_owned(),
            },
        )?;
        self.hydrate_revision_sources(&mut changeset, &format!("{reference}^1"), reference);
        Ok(changeset)
    }

    pub fn files(&self, left: &Path, right: &Path) -> Result<Changeset, VcsError> {
        let left = absolute_or_join(&self.root, left);
        let right = absolute_or_join(&self.root, right);
        let left_text = left.to_string_lossy();
        let right_text = right.to_string_lossy();
        let args = [
            "diff",
            "--no-index",
            "--no-ext-diff",
            "--no-color",
            "--",
            left_text.as_ref(),
            right_text.as_ref(),
        ];
        let patch = output_text(run(&self.root, "git", &args, &[0, 1])?)?;
        let mut changeset = parse_patch(
            &patch,
            "git:files",
            format!("{} ↔ {}", left.display(), right.display()),
            ChangesetSource::Files {
                left: left.display().to_string(),
                right: right.display().to_string(),
            },
        )?;
        if let Some(file) = changeset.files.first_mut() {
            file.set_sources(FileSourceSnapshots {
                old: read_file_snapshot(&left),
                new: read_file_snapshot(&right),
            });
        }
        Ok(changeset)
    }

    fn tracked_diff(&self, request: &DiffRequest) -> Result<String, VcsError> {
        if let Some(from) = request.from.as_deref() {
            validate_revision(from)?;
        }
        if let Some(target) = request.target.as_deref() {
            validate_revision(target)?;
        }

        let moved = self.color_moved_options(request.color_moved);
        let mut arguments = self.patch_command_prefix(moved.as_ref());
        arguments.extend([
            "diff".to_owned(),
            "--no-ext-diff".to_owned(),
            "--find-renames".to_owned(),
            "--binary".to_owned(),
        ]);
        Self::append_patch_color_args(&mut arguments, moved.as_ref());
        if request.staged {
            arguments.push("--cached".to_owned());
        }
        match (&request.from, &request.target) {
            (Some(from), Some(to)) => arguments.push(format!("{from}..{to}")),
            (None, Some(target)) => arguments.push(target.clone()),
            (Some(_), None) => unreachable!("from requires a target at the CLI boundary"),
            (None, None) if !request.staged && has_head(&self.root) => {
                arguments.push("HEAD".to_owned());
            }
            _ => {}
        }
        arguments.push("--".to_owned());
        arguments.extend(request.pathspec.iter().cloned());
        let borrowed = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        output_text(run(&self.root, "git", &borrowed, &[0])?)
    }

    fn untracked_patch(&self, pathspec: &[String]) -> Result<UntrackedPatch, VcsError> {
        let mut args = vec!["ls-files", "--others", "--exclude-standard", "-z", "--"];
        args.extend(pathspec.iter().map(String::as_str));
        let output = run(&self.root, "git", &args, &[0])?;
        let mut untracked = UntrackedPatch::default();
        for raw_path in output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
        {
            let path = String::from_utf8_lossy(raw_path);
            let absolute = self.root.join(path.as_ref());
            let metadata =
                fs::symlink_metadata(&absolute).map_err(|source| VcsError::ReadFile {
                    path: absolute.clone(),
                    source,
                })?;
            if !metadata.file_type().is_file() && !metadata.file_type().is_symlink() {
                continue;
            }
            let file = build_filesystem_untracked_diff_file(
                &self.root,
                Path::new(path.as_ref()),
                untracked.files.len(),
                "git:working",
            )?;
            append_untracked_transport_patch(&mut untracked.patch, &file);
            untracked.files.push(file);
        }
        Ok(untracked)
    }

    fn hydrate_working_sources(&self, changeset: &mut Changeset, request: &DiffRequest) {
        for file in &mut changeset.files {
            if file.flags.untracked
                && (file.flags.binary
                    || file.flags.too_large
                    || file.patch.contains("new file mode 120000"))
            {
                continue;
            }
            let old_path = file.previous_path.as_deref().unwrap_or(&file.path);
            let sources = match (&request.from, &request.target) {
                (Some(from), Some(to)) => FileSourceSnapshots {
                    old: self.git_blob_snapshot(from, old_path),
                    new: self.git_blob_snapshot(to, &file.path),
                },
                (None, Some(target)) => FileSourceSnapshots {
                    old: self.git_blob_snapshot(target, old_path),
                    new: if request.staged {
                        self.index_snapshot(&file.path)
                    } else {
                        read_working_tree_snapshot(&self.root.join(&file.path))
                    },
                },
                (None, None) => FileSourceSnapshots {
                    old: self.git_blob_snapshot("HEAD", old_path),
                    new: if request.staged {
                        self.index_snapshot(&file.path)
                    } else {
                        read_working_tree_snapshot(&self.root.join(&file.path))
                    },
                },
                (Some(_), None) => continue,
            };
            if sources.old.is_some() || sources.new.is_some() {
                file.set_sources(sources);
            }
        }
    }

    fn hydrate_revision_sources(&self, changeset: &mut Changeset, old: &str, new: &str) {
        for file in &mut changeset.files {
            let old_path = file.previous_path.as_deref().unwrap_or(&file.path);
            let sources = FileSourceSnapshots {
                old: self.git_blob_snapshot(old, old_path),
                new: self.git_blob_snapshot(new, &file.path),
            };
            if sources.old.is_some() || sources.new.is_some() {
                file.set_sources(sources);
            }
        }
    }

    fn git_blob_snapshot(&self, revision: &str, path: &str) -> Option<SourceSnapshot> {
        let spec = format!("{revision}:{path}");
        let output = Command::new("git")
            .args(["cat-file", "blob", &spec])
            .current_dir(&self.root)
            .output()
            .ok()?;
        if !output.status.success() || output.stdout.len() as u64 > MAX_SOURCE_BYTES {
            return None;
        }
        let content = String::from_utf8(output.stdout).ok()?;
        Some(SourceSnapshot::new(
            content,
            SourceOrigin::Revision {
                revision: revision.into(),
            },
            true,
        ))
    }

    fn index_snapshot(&self, path: &str) -> Option<SourceSnapshot> {
        let spec = format!(":{path}");
        let output = Command::new("git")
            .args(["cat-file", "blob", &spec])
            .current_dir(&self.root)
            .output()
            .ok()?;
        if !output.status.success() || output.stdout.len() as u64 > MAX_SOURCE_BYTES {
            return None;
        }
        Some(SourceSnapshot::new(
            String::from_utf8(output.stdout).ok()?,
            SourceOrigin::Index,
            true,
        ))
    }
}

impl VcsProvider for GitProvider {
    fn name(&self) -> &'static str {
        "git"
    }

    fn working_tree(&self, request: &DiffRequest) -> Result<Changeset, VcsError> {
        if request.from.is_some() && request.target.is_none() {
            return Err(VcsError::InvalidRevision(
                "a from revision requires a to revision".into(),
            ));
        }
        let mut patch = self.tracked_diff(request)?;
        let mut untracked = UntrackedPatch::default();
        if !request.exclude_untracked
            && !request.staged
            && request.target.is_none()
            && request.from.is_none()
        {
            untracked = self.untracked_patch(&request.pathspec)?;
            patch.push_str(&untracked.patch);
        }
        let title = match (&request.from, &request.target, request.staged) {
            (Some(from), Some(to), _) => format!("{from} → {to}"),
            (None, Some(target), _) => format!("Changes from {target}"),
            (_, _, true) => "Staged changes".to_owned(),
            _ => "Working tree".to_owned(),
        };
        let source = match (&request.from, &request.target) {
            (from, Some(to)) => ChangesetSource::Revision {
                from: from.clone(),
                to: to.clone(),
            },
            _ => ChangesetSource::WorkingTree {
                staged: request.staged,
            },
        };
        if patch.trim().is_empty() {
            return Ok(Changeset {
                id: "git:working".into(),
                title,
                source,
                files: Vec::new(),
            });
        }
        let mut changeset = parse_patch(&patch, "git:working", title, source)?;
        apply_untracked_metadata(&mut changeset, &untracked.files);
        self.hydrate_working_sources(&mut changeset, request);
        Ok(changeset)
    }

    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError> {
        let target = target.unwrap_or("HEAD");
        validate_revision(target)?;
        let moved = self.color_moved_options(None);
        let mut arguments = self.patch_command_prefix(moved.as_ref());
        arguments.extend([
            "show".into(),
            "--format=".into(),
            "--no-ext-diff".into(),
            "--find-renames".into(),
            "--binary".into(),
        ]);
        Self::append_patch_color_args(&mut arguments, moved.as_ref());
        arguments.extend([target.into(), "--".into()]);
        arguments.extend(pathspec.iter().cloned());
        let borrowed = arguments.iter().map(String::as_str).collect::<Vec<_>>();
        let patch = output_text(run(&self.root, "git", &borrowed, &[0])?)?;
        if patch.trim().is_empty() {
            return Ok(Changeset {
                id: format!("git:show:{target}"),
                title: format!("Commit {target}"),
                source: ChangesetSource::Revision {
                    from: None,
                    to: target.to_owned(),
                },
                files: Vec::new(),
            });
        }
        let mut changeset = parse_patch(
            &patch,
            format!("git:show:{target}"),
            format!("Commit {target}"),
            ChangesetSource::Revision {
                from: None,
                to: target.to_owned(),
            },
        )?;
        self.hydrate_revision_sources(&mut changeset, &format!("{target}^"), target);
        Ok(changeset)
    }
}

#[derive(Debug, Clone)]
pub struct JujutsuProvider {
    root: PathBuf,
}

impl JujutsuProvider {
    pub fn discover(cwd: &Path) -> Result<Self, VcsError> {
        let output = run_prefixed(cwd, "jj", &["--no-pager", "--color", "never"], &["root"])?;
        Ok(Self {
            root: PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl VcsProvider for JujutsuProvider {
    fn name(&self) -> &'static str {
        "jj"
    }

    fn working_tree(&self, request: &DiffRequest) -> Result<Changeset, VcsError> {
        if request.staged {
            return Err(VcsError::InvalidRevision(
                "Jujutsu has no staging area; remove --staged or select Git".into(),
            ));
        }
        let mut arguments = vec!["diff", "--git"];
        match (&request.from, &request.target) {
            (Some(from), Some(to)) => {
                validate_revision(from)?;
                validate_revision(to)?;
                arguments.extend(["--ignore-working-copy", "--from", from, "--to", to]);
            }
            (None, Some(target)) => {
                validate_revision(target)?;
                arguments.extend(["-r", target]);
            }
            (Some(_), None) => {
                return Err(VcsError::InvalidRevision(
                    "a from revision requires a to revision".into(),
                ));
            }
            (None, None) => {}
        }
        append_paths(&mut arguments, &request.pathspec);
        let patch = output_text(run_prefixed(
            &self.root,
            "jj",
            &["--no-pager", "--color", "never"],
            &arguments,
        )?)?;
        let (title, source) = review_title_and_source(request);
        parse_or_empty(&patch, "jj:diff", title, source)
    }

    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError> {
        let target = target.unwrap_or("@");
        validate_revision(target)?;
        let mut arguments = vec!["diff", "--git", "-r", target];
        append_paths(&mut arguments, pathspec);
        let patch = output_text(run_prefixed(
            &self.root,
            "jj",
            &["--no-pager", "--color", "never"],
            &arguments,
        )?)?;
        parse_or_empty(
            &patch,
            &format!("jj:show:{target}"),
            format!("Commit {target}"),
            ChangesetSource::Revision {
                from: None,
                to: target.into(),
            },
        )
    }
}

#[derive(Debug, Clone)]
pub struct SaplingProvider {
    root: PathBuf,
}

impl SaplingProvider {
    pub fn discover(cwd: &Path) -> Result<Self, VcsError> {
        let output = run_prefixed(
            cwd,
            "sl",
            &["--noninteractive", "--color", "never"],
            &["root"],
        )?;
        Ok(Self {
            root: PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn untracked_patch(&self, pathspec: &[String]) -> Result<UntrackedPatch, VcsError> {
        let mut arguments = vec!["status", "--unknown", "--print0", "--root-relative"];
        append_paths(&mut arguments, pathspec);
        let output = run_prefixed(
            &self.root,
            "sl",
            &["--noninteractive", "--color", "never"],
            &arguments,
        )?;
        let mut untracked = UntrackedPatch::default();
        for entry in output.stdout.split(|byte| *byte == 0) {
            let entry = String::from_utf8_lossy(entry);
            let Some(path) = entry.strip_prefix("? ") else {
                continue;
            };
            let absolute = self.root.join(path);
            let Ok(metadata) = fs::symlink_metadata(&absolute) else {
                continue;
            };
            if metadata.is_dir() {
                continue;
            }
            let file = build_filesystem_untracked_diff_file(
                &self.root,
                Path::new(path),
                untracked.files.len(),
                "sl:diff",
            )?;
            append_untracked_transport_patch(&mut untracked.patch, &file);
            untracked.files.push(file);
        }
        Ok(untracked)
    }
}

impl VcsProvider for SaplingProvider {
    fn name(&self) -> &'static str {
        "sl"
    }

    fn working_tree(&self, request: &DiffRequest) -> Result<Changeset, VcsError> {
        if request.staged {
            return Err(VcsError::InvalidRevision(
                "Sapling has no staging area; remove --staged or select Git".into(),
            ));
        }
        let mut arguments = vec!["diff", "--git"];
        match (&request.from, &request.target) {
            (Some(from), Some(to)) => {
                validate_revision(from)?;
                validate_revision(to)?;
                arguments.extend(["-r", from, "-r", to]);
            }
            (None, Some(target)) => {
                validate_revision(target)?;
                arguments.extend(["-r", target]);
            }
            (Some(_), None) => {
                return Err(VcsError::InvalidRevision(
                    "a from revision requires a to revision".into(),
                ));
            }
            (None, None) => {}
        }
        append_paths(&mut arguments, &request.pathspec);
        let mut patch = output_text(run_prefixed(
            &self.root,
            "sl",
            &["--noninteractive", "--color", "never"],
            &arguments,
        )?)?;
        let mut untracked = UntrackedPatch::default();
        if !request.exclude_untracked && request.target.is_none() && request.from.is_none() {
            untracked = self.untracked_patch(&request.pathspec)?;
            patch.push_str(&untracked.patch);
        }
        let (title, source) = review_title_and_source(request);
        let mut changeset = parse_or_empty(&patch, "sl:diff", title, source)?;
        apply_untracked_metadata(&mut changeset, &untracked.files);
        Ok(changeset)
    }

    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError> {
        let target = target.unwrap_or(".");
        validate_revision(target)?;
        let mut arguments = vec!["diff", "--git", "--change", target];
        append_paths(&mut arguments, pathspec);
        let patch = output_text(run_prefixed(
            &self.root,
            "sl",
            &["--noninteractive", "--color", "never"],
            &arguments,
        )?)?;
        parse_or_empty(
            &patch,
            &format!("sl:show:{target}"),
            format!("Commit {target}"),
            ChangesetSource::Revision {
                from: None,
                to: target.into(),
            },
        )
    }
}

pub fn parse_patch_input(patch: &str, label: impl Into<String>) -> Result<Changeset, VcsError> {
    let label = label.into();
    parse_patch(
        patch,
        format!("patch:{label}"),
        label.clone(),
        ChangesetSource::Patch { label },
    )
    .map_err(Into::into)
}

pub fn validate_revision(revision: &str) -> Result<(), VcsError> {
    if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
        return Err(VcsError::InvalidRevision(format!(
            "refusing option-like or empty VCS revision {revision:?}"
        )));
    }
    Ok(())
}

fn append_paths<'a>(arguments: &mut Vec<&'a str>, pathspec: &'a [String]) {
    if !pathspec.is_empty() {
        arguments.push("--");
        arguments.extend(pathspec.iter().map(String::as_str));
    }
}

fn review_title_and_source(request: &DiffRequest) -> (String, ChangesetSource) {
    let title = match (&request.from, &request.target) {
        (Some(from), Some(to)) => format!("{from} → {to}"),
        (None, Some(target)) => format!("Changes from {target}"),
        _ => "Working tree".into(),
    };
    let source = match (&request.from, &request.target) {
        (from, Some(to)) => ChangesetSource::Revision {
            from: from.clone(),
            to: to.clone(),
        },
        _ => ChangesetSource::WorkingTree { staged: false },
    };
    (title, source)
}

fn parse_or_empty(
    patch: &str,
    id: &str,
    title: String,
    source: ChangesetSource,
) -> Result<Changeset, VcsError> {
    if patch.trim().is_empty() {
        Ok(Changeset {
            id: id.into(),
            title,
            source,
            files: Vec::new(),
        })
    } else {
        parse_patch(patch, id, title, source).map_err(Into::into)
    }
}

fn run_prefixed(
    cwd: &Path,
    program: &str,
    prefix: &[&str],
    args: &[&str],
) -> Result<Output, VcsError> {
    let output = Command::new(program)
        .args(prefix)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| VcsError::Command {
            program: program.into(),
            code: None,
            stderr: error.to_string(),
        })?;
    if !output.status.success() {
        return Err(VcsError::Command {
            program: program.into(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().into(),
        });
    }
    Ok(output)
}

fn find_marker(cwd: &Path, marker: &str) -> Option<PathBuf> {
    let mut current = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_owned());
    loop {
        if current.join(marker).exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn run(
    cwd: &Path,
    program: &str,
    args: &[&str],
    success_codes: &[i32],
) -> Result<Output, VcsError> {
    let output = Command::new(program)
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| VcsError::Command {
            program: program.to_owned(),
            code: None,
            stderr: error.to_string(),
        })?;
    if !output
        .status
        .code()
        .is_some_and(|code| success_codes.contains(&code))
    {
        return Err(VcsError::Command {
            program: program.to_owned(),
            code: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        });
    }
    Ok(output)
}

fn output_text(output: Output) -> Result<String, VcsError> {
    if output.stdout.len() > MAX_PATCH_BYTES {
        return Err(VcsError::PatchTooLarge(output.stdout.len()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn has_head(root: &Path) -> bool {
    Command::new("git")
        .args(["rev-parse", "--verify", "HEAD"])
        .current_dir(root)
        .output()
        .is_ok_and(|output| output.status.success())
}

#[derive(Debug, Default)]
struct UntrackedPatch {
    patch: String,
    files: Vec<workdeck_core::DiffFile>,
}

fn append_untracked_transport_patch(output: &mut String, file: &workdeck_core::DiffFile) {
    if file.patch.starts_with("diff --git ") {
        output.push_str(&file.patch);
        return;
    }
    let path = &file.path;
    let old_path = quote_git_path(&format!("a/{path}"));
    let new_path = quote_git_path(&format!("b/{path}"));
    output.push_str(&format!("diff --git {old_path} {new_path}\n"));
    output.push_str("new file mode 100644\n");
    output.push_str(&format!("Binary files /dev/null and {new_path} differ\n"));
}

fn apply_untracked_metadata(changeset: &mut Changeset, untracked: &[workdeck_core::DiffFile]) {
    for record in untracked {
        let Some(file) = changeset
            .files
            .iter_mut()
            .find(|file| file.path == record.path)
        else {
            continue;
        };
        *file = record.clone();
    }
}

fn quote_git_path(path: &str) -> String {
    if path
        .bytes()
        .all(|byte| !byte.is_ascii_whitespace() && byte != b'"' && byte != b'\\')
    {
        return path.to_owned();
    }
    let mut quoted = String::from("\"");
    for byte in path.as_bytes() {
        match byte {
            b'"' => quoted.push_str("\\\""),
            b'\\' => quoted.push_str("\\\\"),
            b'\t' => quoted.push_str("\\t"),
            b'\n' => quoted.push_str("\\n"),
            0x20..=0x7e => quoted.push(char::from(*byte)),
            _ => quoted.push_str(&format!("\\{byte:03o}")),
        }
    }
    quoted.push('"');
    quoted
}

fn absolute_or_join(root: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        root.join(path)
    }
}

fn read_file_snapshot(path: &Path) -> Option<SourceSnapshot> {
    read_source_snapshot(
        path,
        SourceOrigin::File {
            path: path.display().to_string(),
        },
    )
}

fn read_working_tree_snapshot(path: &Path) -> Option<SourceSnapshot> {
    read_source_snapshot(path, SourceOrigin::WorkingTree)
}

fn read_source_snapshot(path: &Path, origin: SourceOrigin) -> Option<SourceSnapshot> {
    match read_file_text_with_limit(path, DEFAULT_SOURCE_TEXT_MAX_BYTES) {
        LimitedSourceTextResult::Text(content) => Some(SourceSnapshot::new(content, origin, true)),
        LimitedSourceTextResult::Missing | LimitedSourceTextResult::TooLarge { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;
    use untracked::is_probably_binary;

    #[test]
    fn rejects_option_like_revisions() {
        let error = validate_revision("--output=/tmp/owned").unwrap_err();
        assert!(error.to_string().contains("option-like"));
        validate_revision("main~2").unwrap();
    }

    #[test]
    fn parses_provider_preferences_and_detects_checkout_markers() {
        assert_eq!(
            ProviderPreference::parse("jj").unwrap(),
            ProviderPreference::Jujutsu
        );
        assert!(ProviderPreference::parse("mercurial").is_err());
        let directory = TempDir::new().unwrap();
        fs::create_dir(directory.path().join(".jj")).unwrap();
        fs::create_dir(directory.path().join("nested")).unwrap();
        assert_eq!(
            find_marker(&directory.path().join("nested"), ".jj"),
            Some(fs::canonicalize(directory.path()).unwrap())
        );
    }

    #[test]
    fn non_git_providers_reject_staging_without_spawning() {
        let request = DiffRequest {
            staged: true,
            ..DiffRequest::default()
        };
        let jj = JujutsuProvider {
            root: PathBuf::from("/definitely/not/a/repository"),
        };
        let sl = SaplingProvider {
            root: PathBuf::from("/definitely/not/a/repository"),
        };
        assert!(
            jj.working_tree(&request)
                .unwrap_err()
                .to_string()
                .contains("staging")
        );
        assert!(
            sl.working_tree(&request)
                .unwrap_err()
                .to_string()
                .contains("staging")
        );
    }

    #[test]
    fn binary_sniffing_matches_hunks_control_byte_threshold() {
        assert!(!is_probably_binary(
            b"const value = 1;\n\tconst other = 2;\r\n"
        ));
        assert!(!is_probably_binary(b""));
        assert!(is_probably_binary(&[b'a', b'b', 0, b'c']));
        assert!(is_probably_binary(&[
            b'a', b'b', b'c', b'd', b'e', b'f', b'g', 1, 2, 3,
        ]));
        assert!(!is_probably_binary(&[
            b'a', b'b', b'c', b'd', b'e', b'f', b'g', b'h', b'i', 1,
        ]));
    }

    #[test]
    fn loads_tracked_and_untracked_working_tree_files() {
        let directory = TempDir::new().unwrap();
        run_git(directory.path(), &["init", "-q"]);
        run_git(
            directory.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(directory.path(), &["config", "user.name", "Test"]);
        fs::write(directory.path().join("tracked.txt"), "old\n").unwrap();
        run_git(directory.path(), &["add", "tracked.txt"]);
        run_git(directory.path(), &["commit", "-qm", "initial"]);
        fs::write(directory.path().join("tracked.txt"), "new\n").unwrap();
        fs::write(directory.path().join("untracked file.txt"), "hello\n").unwrap();

        let provider = GitProvider::discover(directory.path()).unwrap();
        let changeset = provider.working_tree(&DiffRequest::default()).unwrap();
        assert_eq!(changeset.files.len(), 2);
        let tracked = changeset
            .files
            .iter()
            .find(|file| file.path == "tracked.txt")
            .unwrap();
        assert_eq!(tracked.sources.old.as_ref().unwrap().content, "old\n");
        assert_eq!(tracked.sources.new.as_ref().unwrap().content, "new\n");
        assert!(matches!(
            tracked.sources.old.as_ref().unwrap().origin,
            SourceOrigin::Revision { .. }
        ));
        assert_eq!(
            tracked.sources.new.as_ref().unwrap().origin,
            SourceOrigin::WorkingTree
        );
        let untracked = changeset
            .files
            .iter()
            .find(|file| file.path == "untracked file.txt")
            .unwrap();
        assert!(untracked.flags.untracked);
        assert!(untracked.sources.old.is_none());
        assert_eq!(untracked.sources.new.as_ref().unwrap().content, "hello\n");
        assert!(tracked.source_attested && untracked.source_attested);
    }

    #[test]
    fn keeps_large_untracked_files_as_bounded_review_placeholders() {
        let directory = TempDir::new().unwrap();
        run_git(directory.path(), &["init", "-q"]);
        fs::write(
            directory.path().join("large.txt"),
            vec![b'x'; LARGE_DIFF_FILE_MAX_BYTES as usize + 1],
        )
        .unwrap();

        let provider = GitProvider::discover(directory.path()).unwrap();
        let changeset = provider.working_tree(&DiffRequest::default()).unwrap();
        let file = changeset
            .files
            .iter()
            .find(|file| file.path == "large.txt")
            .unwrap();
        assert!(file.flags.untracked);
        assert!(file.flags.too_large);
        assert!(!file.flags.binary);
        assert_eq!(file.stats.additions, 1);
        assert!(file.stats.truncated);
        assert!(file.hunks.is_empty());
        assert!(file.sources.new.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn git_provider_keeps_empty_executable_and_symlink_untracked_semantics() {
        use std::os::unix::fs::{PermissionsExt, symlink};

        let directory = TempDir::new().unwrap();
        run_git(directory.path(), &["init", "-q"]);
        fs::write(directory.path().join("empty.txt"), "").unwrap();
        let executable = directory.path().join("run.sh");
        fs::write(&executable, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(directory.path().join("target.txt"), "target\n").unwrap();
        symlink("target.txt", directory.path().join("link")).unwrap();

        let provider = GitProvider::discover(directory.path()).unwrap();
        let changeset = provider.working_tree(&DiffRequest::default()).unwrap();

        let empty = changeset
            .files
            .iter()
            .find(|file| file.path == "empty.txt")
            .unwrap();
        assert!(empty.hunks.is_empty());
        let executable = changeset
            .files
            .iter()
            .find(|file| file.path == "run.sh")
            .unwrap();
        assert!(executable.patch.contains("new file mode 100755"));
        let link = changeset
            .files
            .iter()
            .find(|file| file.path == "link")
            .unwrap();
        assert!(link.patch.contains("new file mode 120000"));
        assert!(link.sources.new.is_none());
    }

    #[test]
    fn captures_staged_and_commit_source_pairs() {
        let directory = TempDir::new().unwrap();
        run_git(directory.path(), &["init", "-q"]);
        run_git(
            directory.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(directory.path(), &["config", "user.name", "Test"]);
        fs::write(directory.path().join("file.txt"), "one\n").unwrap();
        run_git(directory.path(), &["add", "file.txt"]);
        run_git(directory.path(), &["commit", "-qm", "one"]);
        fs::write(directory.path().join("file.txt"), "two\n").unwrap();
        run_git(directory.path(), &["add", "file.txt"]);

        let provider = GitProvider::discover(directory.path()).unwrap();
        let staged = provider
            .working_tree(&DiffRequest {
                staged: true,
                ..DiffRequest::default()
            })
            .unwrap();
        assert_eq!(
            staged.files[0].sources.old.as_ref().unwrap().content,
            "one\n"
        );
        assert_eq!(
            staged.files[0].sources.new.as_ref().unwrap().content,
            "two\n"
        );
        assert_eq!(
            staged.files[0].sources.new.as_ref().unwrap().origin,
            SourceOrigin::Index
        );

        run_git(directory.path(), &["commit", "-qm", "two"]);
        let shown = provider.show(Some("HEAD"), &[]).unwrap();
        assert_eq!(
            shown.files[0].sources.old.as_ref().unwrap().content,
            "one\n"
        );
        assert_eq!(
            shown.files[0].sources.new.as_ref().unwrap().content,
            "two\n"
        );
    }

    #[test]
    fn captures_git_color_moved_lines_with_deterministic_palette() {
        let directory = TempDir::new().unwrap();
        run_git(directory.path(), &["init", "-q"]);
        run_git(
            directory.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(directory.path(), &["config", "user.name", "Test"]);
        let before = concat!(
            "start anchor\n",
            "relocated block first line has many chars\n",
            "relocated block second line has many chars\n",
            "relocated block third line has many chars\n",
            "middle unchanged one has many chars\n",
            "middle unchanged two has many chars\n",
            "end anchor\n",
        );
        fs::write(directory.path().join("example.txt"), before).unwrap();
        run_git(directory.path(), &["add", "example.txt"]);
        run_git(directory.path(), &["commit", "-qm", "initial"]);
        run_git(
            directory.path(),
            &["config", "--local", "diff.colorMoved", "zebra"],
        );
        let after = concat!(
            "start anchor\n",
            "middle unchanged one has many chars\n",
            "middle unchanged two has many chars\n",
            "relocated block first line has many chars\n",
            "relocated block second line has many chars\n",
            "relocated block third line has many chars\n",
            "end anchor\n",
        );
        fs::write(directory.path().join("example.txt"), after).unwrap();

        let changeset = GitProvider::discover(directory.path())
            .unwrap()
            .working_tree(&DiffRequest::default())
            .unwrap();
        let moved = changeset.files[0]
            .hunks
            .iter()
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| line.moved)
            .collect::<Vec<_>>();
        assert!(moved.iter().any(|line| {
            line.kind == workdeck_core::DiffLineKind::Addition
                && line.content.contains("middle unchanged one")
        }));
        assert!(moved.iter().any(|line| {
            line.kind == workdeck_core::DiffLineKind::Deletion
                && line.content.contains("middle unchanged one")
        }));
    }

    fn run_git(cwd: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .args(args)
                .current_dir(cwd)
                .status()
                .unwrap()
                .success()
        );
    }
}
