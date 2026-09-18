//! VCS adapters that normalize provider output into one Workdeck changeset model.

mod bundled;
mod catalog;
mod file_comparison;
mod file_source;
mod git_adapter;
mod git_commands;
mod git_source;
mod jujutsu_adapter;
mod jujutsu_commands;
mod jujutsu_source;
mod large_file;
mod materialize;
mod platform;
mod sapling_adapter;
mod sapling_commands;
mod source_capabilities;
mod source_text;
mod untracked;
mod watch_controller;
mod watch_observer;
mod watch_plan;
mod watch_runtime;
mod watch_signature;

pub use bundled::*;
pub use catalog::*;
pub use file_comparison::*;
pub use file_source::*;
pub use git_adapter::*;
pub use git_commands::*;
pub use git_source::*;
pub use jujutsu_adapter::*;
pub use jujutsu_commands::*;
pub use jujutsu_source::*;
pub use large_file::{
    LARGE_DIFF_FILE_MAX_BYTES, LARGE_DIFF_FILE_MAX_LINES, LargeFileCheck,
    inspect_large_untracked_file,
};
pub use materialize::*;
pub use platform::{normalize_path_for_os, normalize_path_for_platform};
pub use sapling_adapter::*;
pub use sapling_commands::*;
pub use source_capabilities::*;
pub use source_text::{
    DEFAULT_SOURCE_TEXT_MAX_BYTES, LimitedSourceTextResult, SourceSubprocess, SourceTextError,
    force_terminate_source_subprocess, log_source_diagnostic, read_file_text_with_limit,
    read_stream_text_with_limit, terminate_source_subprocess,
};
pub use untracked::build_filesystem_untracked_diff_file;
pub use watch_controller::*;
pub use watch_observer::*;
pub use watch_plan::*;
pub use watch_runtime::*;
pub use watch_signature::*;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use thiserror::Error;
use workdeck_core::{Changeset, ChangesetSource};
use workdeck_diff::PatchError;

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
    #[error("{0}")]
    Adapter(String),
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
                if detect_sapling_repo(cwd).is_some() {
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

    pub fn stash(&self, reference: Option<&str>) -> Result<Changeset, VcsError> {
        load_git_changeset(
            &VcsReviewInput::StashShow(workdeck_core::VcsStashShowCommandInput {
                reference: reference.map(str::to_owned),
                options: workdeck_core::CommonOptions::default(),
            }),
            &VcsLoadContext {
                cwd: self.root.clone(),
            },
            &GitVcsAdapterOptions::default(),
        )
        .map_err(|error| VcsError::Adapter(error.to_string()))
    }

    pub fn files(&self, left: &Path, right: &Path) -> Result<Changeset, VcsError> {
        load_file_comparison(&self.root, left, right)
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
        load_git_changeset(
            &VcsReviewInput::Diff(workdeck_core::VcsDiffCommandInput {
                range: request
                    .from
                    .is_none()
                    .then(|| request.target.clone())
                    .flatten(),
                range_endpoints: request
                    .from
                    .clone()
                    .zip(request.target.clone())
                    .map(|(from, to)| workdeck_core::VcsRangeEndpoints { from, to }),
                staged: request.staged,
                pathspecs: request.pathspec.clone(),
                options: workdeck_core::CommonOptions {
                    exclude_untracked: Some(request.exclude_untracked),
                    color_moved: request.color_moved,
                    ..workdeck_core::CommonOptions::default()
                },
            }),
            &VcsLoadContext {
                cwd: self.root.clone(),
            },
            &GitVcsAdapterOptions::default(),
        )
        .map_err(|error| VcsError::Adapter(error.to_string()))
    }

    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError> {
        load_git_changeset(
            &VcsReviewInput::Show(workdeck_core::VcsShowCommandInput {
                reference: target.map(str::to_owned),
                pathspecs: pathspec.to_vec(),
                options: workdeck_core::CommonOptions::default(),
            }),
            &VcsLoadContext {
                cwd: self.root.clone(),
            },
            &GitVcsAdapterOptions::default(),
        )
        .map_err(|error| VcsError::Adapter(error.to_string()))
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
        if request.from.is_some() && request.target.is_none() {
            return Err(VcsError::InvalidRevision(
                "a from revision requires a to revision".into(),
            ));
        }
        load_jujutsu_changeset(
            &VcsReviewInput::Diff(workdeck_core::VcsDiffCommandInput {
                range: request
                    .from
                    .is_none()
                    .then(|| request.target.clone())
                    .flatten(),
                range_endpoints: request
                    .from
                    .clone()
                    .zip(request.target.clone())
                    .map(|(from, to)| workdeck_core::VcsRangeEndpoints { from, to }),
                staged: request.staged,
                pathspecs: request.pathspec.clone(),
                options: workdeck_core::CommonOptions::default(),
            }),
            &VcsLoadContext {
                cwd: self.root.clone(),
            },
            &JujutsuVcsAdapterOptions::default(),
        )
        .map_err(|error| VcsError::Adapter(error.to_string()))
    }

    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError> {
        load_jujutsu_changeset(
            &VcsReviewInput::Show(workdeck_core::VcsShowCommandInput {
                reference: target.map(str::to_owned),
                pathspecs: pathspec.to_vec(),
                options: workdeck_core::CommonOptions::default(),
            }),
            &VcsLoadContext {
                cwd: self.root.clone(),
            },
            &JujutsuVcsAdapterOptions::default(),
        )
        .map_err(|error| VcsError::Adapter(error.to_string()))
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
}

impl VcsProvider for SaplingProvider {
    fn name(&self) -> &'static str {
        "sl"
    }

    fn working_tree(&self, request: &DiffRequest) -> Result<Changeset, VcsError> {
        if request.from.is_some() && request.target.is_none() {
            return Err(VcsError::InvalidRevision(
                "a from revision requires a to revision".into(),
            ));
        }
        load_sapling_changeset(
            &VcsReviewInput::Diff(workdeck_core::VcsDiffCommandInput {
                range: request
                    .from
                    .is_none()
                    .then(|| request.target.clone())
                    .flatten(),
                range_endpoints: request
                    .from
                    .clone()
                    .zip(request.target.clone())
                    .map(|(from, to)| workdeck_core::VcsRangeEndpoints { from, to }),
                staged: request.staged,
                pathspecs: request.pathspec.clone(),
                options: workdeck_core::CommonOptions {
                    exclude_untracked: Some(request.exclude_untracked),
                    ..workdeck_core::CommonOptions::default()
                },
            }),
            &VcsLoadContext {
                cwd: self.root.clone(),
            },
            &SaplingVcsAdapterOptions::default(),
        )
        .map_err(|error| VcsError::Adapter(error.to_string()))
    }

    fn show(&self, target: Option<&str>, pathspec: &[String]) -> Result<Changeset, VcsError> {
        load_sapling_changeset(
            &VcsReviewInput::Show(workdeck_core::VcsShowCommandInput {
                reference: target.map(str::to_owned),
                pathspecs: pathspec.to_vec(),
                options: workdeck_core::CommonOptions::default(),
            }),
            &VcsLoadContext {
                cwd: self.root.clone(),
            },
            &SaplingVcsAdapterOptions::default(),
        )
        .map_err(|error| VcsError::Adapter(error.to_string()))
    }
}

pub fn parse_patch_input(patch: &str, label: impl Into<String>) -> Result<Changeset, VcsError> {
    let label = label.into();
    Ok(workdeck_diff::changeset_from_patch(
        patch,
        format!("patch:{label}"),
        format!("Patch review: {}", display_basename(&label)),
        label.clone(),
        ChangesetSource::Patch { label },
        None,
    ))
}

pub fn validate_revision(revision: &str) -> Result<(), VcsError> {
    if revision.is_empty() || revision.starts_with('-') || revision.contains('\0') {
        return Err(VcsError::InvalidRevision(format!(
            "refusing option-like or empty VCS revision {revision:?}"
        )));
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::TempDir;
    use untracked::is_probably_binary;
    use workdeck_core::SourceOrigin;

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
    fn patch_input_uses_hunk_empty_review_semantics_and_separate_source_label() {
        let changeset = parse_patch_input(
            "\x1b]0;title\x07not really a patch\n--- separator only",
            "stdin patch",
        )
        .unwrap();

        assert_eq!(changeset.id, "patch:stdin patch");
        assert_eq!(changeset.source_label, "stdin patch");
        assert_eq!(changeset.title, "Patch review: stdin patch");
        assert_eq!(
            changeset.summary.as_deref(),
            Some("not really a patch\n--- separator only")
        );
        assert!(changeset.files.is_empty());
    }

    #[test]
    fn patch_loader_projection_matches_both_pinned_hunk_oracles() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/loader-bootstrap.json"
        ))
        .unwrap();
        let expected = &oracle["patch_cases"];
        let patch = concat!(
            "diff --git a/a.txt b/a.txt\n",
            "--- a/a.txt\n",
            "+++ b/a.txt\n",
            "@@ -1 +1 @@\n",
            "-old\n",
            "+new\n",
        );
        let file = parse_patch_input(patch, "nested/input.patch").unwrap();
        assert_eq!(
            serde_json::json!({
                "sourceLabel": file.source_label,
                "title": file.title,
                "path": file.files[0].path,
                "hasSource": file.files[0].source_identity.is_some(),
            }),
            expected["file"]
        );

        let malformed = parse_patch_input(
            "\u{1b}]0;title\u{7}not really a patch\n--- separator only\n@@ section heading",
            "stdin patch",
        )
        .unwrap();
        assert_eq!(
            serde_json::json!({
                "sourceLabel": malformed.source_label,
                "title": malformed.title,
                "summary": malformed.summary,
                "fileCount": malformed.files.len(),
                "hasInitialWatchSignature": false,
            }),
            expected["malformed"]
        );
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
        assert_eq!(
            tracked.sources.old.as_ref().unwrap().origin,
            SourceOrigin::Index
        );
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

    #[test]
    fn hunk_loader_marks_tracked_binary_and_large_files_as_bounded_placeholders() {
        let binary_repo = initialized_git_repo();
        fs::write(binary_repo.path().join("image.png"), [0, 1, 2, 3, 4]).unwrap();
        run_git(binary_repo.path(), &["add", "image.png"]);
        run_git(binary_repo.path(), &["commit", "-qm", "initial"]);
        fs::write(binary_repo.path().join("image.png"), [0, 1, 9, 3, 4, 5]).unwrap();

        let binary = GitProvider::discover(binary_repo.path())
            .unwrap()
            .working_tree(&DiffRequest::default())
            .unwrap();
        assert_eq!(binary.files.len(), 1);
        assert_eq!(binary.files[0].path, "image.png");
        assert!(binary.files[0].flags.binary);
        assert!(binary.files[0].hunks.is_empty());
        assert!(binary.files[0].sources.old.is_none());
        assert!(binary.files[0].sources.new.is_none());

        let tracked_repo = initialized_git_repo();
        fs::write(tracked_repo.path().join("large.txt"), "original\n").unwrap();
        run_git(tracked_repo.path(), &["add", "large.txt"]);
        run_git(tracked_repo.path(), &["commit", "-qm", "initial"]);
        let mut generated = "x\n".repeat(100_000);
        generated.push_str("widest generated line\n");
        fs::write(tracked_repo.path().join("large.txt"), generated).unwrap();

        let tracked = GitProvider::discover(tracked_repo.path())
            .unwrap()
            .working_tree(&DiffRequest::default())
            .unwrap();
        assert_eq!(tracked.files.len(), 1);
        assert!(tracked.files[0].flags.too_large);
        assert_eq!(tracked.files[0].stats.additions, 100_001);
        assert_eq!(tracked.files[0].stats.deletions, 1);
        assert!(tracked.files[0].hunks.is_empty());
        assert!(tracked.files[0].sources.old.is_none());
        assert!(tracked.files[0].sources.new.is_none());

        let untracked_repo = initialized_git_repo();
        fs::write(
            untracked_repo.path().join("large.txt"),
            "x\n".repeat(100_001),
        )
        .unwrap();
        fs::write(
            untracked_repo.path().join("large-single-line.txt"),
            "x".repeat(1_000_001),
        )
        .unwrap();
        let untracked = GitProvider::discover(untracked_repo.path())
            .unwrap()
            .working_tree(&DiffRequest::default())
            .unwrap();
        let line_limited = untracked
            .files
            .iter()
            .find(|file| file.path == "large.txt")
            .unwrap();
        assert!(line_limited.flags.too_large && line_limited.flags.untracked);
        assert_eq!(line_limited.stats.additions, 100_001);
        assert!(!line_limited.stats.truncated);
        let byte_limited = untracked
            .files
            .iter()
            .find(|file| file.path == "large-single-line.txt")
            .unwrap();
        assert!(byte_limited.flags.too_large && byte_limited.flags.untracked);
        assert_eq!(byte_limited.stats.additions, 1);
        assert!(byte_limited.stats.truncated);
    }

    #[test]
    fn hunk_loader_applies_worktree_range_pathspec_and_repository_config_semantics() {
        let directory = initialized_git_repo();
        for (path, body) in [("alpha.ts", "alpha one\n"), ("beta.ts", "beta one\n")] {
            fs::write(directory.path().join(path), body).unwrap();
        }
        run_git(directory.path(), &["add", "."]);
        run_git(directory.path(), &["commit", "-qm", "initial"]);
        run_git(directory.path(), &["branch", "base-branch"]);
        fs::write(directory.path().join("alpha.ts"), "alpha two\n").unwrap();
        fs::write(directory.path().join("beta.ts"), "beta two\n").unwrap();
        run_git(directory.path(), &["add", "."]);
        run_git(directory.path(), &["commit", "-qm", "second"]);
        fs::write(directory.path().join("alpha.ts"), "alpha three\n").unwrap();
        fs::write(directory.path().join("beta.ts"), "beta three\n").unwrap();
        fs::write(directory.path().join("new-alpha.ts"), "new alpha\n").unwrap();
        fs::write(directory.path().join("new-beta.ts"), "new beta\n").unwrap();
        run_git(
            directory.path(),
            &["config", "diff.external", "git --version"],
        );
        run_git(directory.path(), &["config", "diff.noprefix", "true"]);
        run_git(directory.path(), &["config", "diff.mnemonicPrefix", "true"]);

        let provider = GitProvider::discover(directory.path()).unwrap();
        let default = provider.working_tree(&DiffRequest::default()).unwrap();
        assert_eq!(
            default
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["alpha.ts", "beta.ts", "new-alpha.ts", "new-beta.ts"]
        );

        let excluded = provider
            .working_tree(&DiffRequest {
                exclude_untracked: true,
                ..DiffRequest::default()
            })
            .unwrap();
        assert_eq!(
            excluded
                .files
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["alpha.ts", "beta.ts"]
        );

        let one_ref = provider
            .working_tree(&DiffRequest {
                target: Some("base-branch".into()),
                ..DiffRequest::default()
            })
            .unwrap();
        assert!(one_ref.files.iter().any(|file| file.path == "new-alpha.ts"));

        let revisions = provider
            .working_tree(&DiffRequest {
                target: Some("HEAD".into()),
                from: Some("base-branch".into()),
                ..DiffRequest::default()
            })
            .unwrap();
        assert!(!revisions.files.iter().any(|file| file.flags.untracked));

        let parent_bang = provider
            .working_tree(&DiffRequest {
                target: Some("HEAD^!".into()),
                ..DiffRequest::default()
            })
            .unwrap();
        assert!(!parent_bang.files.iter().any(|file| file.flags.untracked));

        let pathspec = provider
            .working_tree(&DiffRequest {
                pathspec: vec!["new-beta.ts".into()],
                ..DiffRequest::default()
            })
            .unwrap();
        assert_eq!(pathspec.files.len(), 1);
        assert_eq!(pathspec.files[0].path, "new-beta.ts");

        fs::create_dir(directory.path().join("nested")).unwrap();
        let nested = GitProvider::discover(&directory.path().join("nested")).unwrap();
        assert_eq!(
            fs::canonicalize(nested.root()).unwrap(),
            fs::canonicalize(directory.path()).unwrap()
        );
        assert!(
            nested
                .working_tree(&DiffRequest::default())
                .unwrap()
                .files
                .iter()
                .any(|file| file.path == "new-alpha.ts")
        );
    }

    #[test]
    fn hunk_loader_show_stash_and_unicode_sources_are_immutable_snapshots() {
        let directory = initialized_git_repo();
        fs::write(directory.path().join("value.txt"), "first\n").unwrap();
        run_git(directory.path(), &["add", "value.txt"]);
        run_git(directory.path(), &["commit", "-qm", "first"]);
        fs::write(directory.path().join("value.txt"), "second\n").unwrap();
        run_git(directory.path(), &["commit", "-qam", "second"]);

        let provider = GitProvider::discover(directory.path()).unwrap();
        let shown = provider.show(Some("HEAD"), &[]).unwrap();
        assert_eq!(
            shown.files[0].sources.old.as_ref().unwrap().content,
            "first\n"
        );
        assert_eq!(
            shown.files[0].sources.new.as_ref().unwrap().content,
            "second\n"
        );
        fs::write(directory.path().join("value.txt"), "third\n").unwrap();
        run_git(directory.path(), &["commit", "-qam", "third"]);
        assert_eq!(
            shown.files[0].sources.old.as_ref().unwrap().content,
            "first\n"
        );
        assert_eq!(
            shown.files[0].sources.new.as_ref().unwrap().content,
            "second\n"
        );

        fs::write(directory.path().join("value.txt"), "first stash\n").unwrap();
        run_git(directory.path(), &["stash", "push", "-qm", "first stash"]);
        let stashed = provider.stash(None).unwrap();
        fs::write(directory.path().join("value.txt"), "second stash\n").unwrap();
        run_git(directory.path(), &["stash", "push", "-qm", "second stash"]);
        assert_eq!(
            stashed.files[0].sources.old.as_ref().unwrap().content,
            "third\n"
        );
        assert_eq!(
            stashed.files[0].sources.new.as_ref().unwrap().content,
            "first stash\n"
        );

        let unicode_repo = initialized_git_repo();
        fs::write(
            unicode_repo.path().join("日本語.txt"),
            "shared\nold-only\nshared\n",
        )
        .unwrap();
        run_git(unicode_repo.path(), &["add", "日本語.txt"]);
        run_git(unicode_repo.path(), &["commit", "-qm", "before"]);
        run_git(unicode_repo.path(), &["mv", "日本語.txt", "한국어🧪.txt"]);
        fs::write(
            unicode_repo.path().join("한국어🧪.txt"),
            "shared\nnew-only\nshared\n",
        )
        .unwrap();
        run_git(unicode_repo.path(), &["add", "한국어🧪.txt"]);
        run_git(unicode_repo.path(), &["commit", "-qm", "rename"]);
        let renamed = GitProvider::discover(unicode_repo.path())
            .unwrap()
            .show(Some("HEAD"), &[])
            .unwrap();
        assert_eq!(renamed.files[0].path, "한국어🧪.txt");
        assert_eq!(
            renamed.files[0].previous_path.as_deref(),
            Some("日本語.txt")
        );
        assert_eq!(
            renamed.files[0].sources.old.as_ref().unwrap().content,
            "shared\nold-only\nshared\n"
        );
        assert_eq!(
            renamed.files[0].sources.new.as_ref().unwrap().content,
            "shared\nnew-only\nshared\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn hunk_loader_skips_directory_symlinks_and_preserves_exact_untracked_names() {
        use std::os::unix::fs::symlink;

        let directory = initialized_git_repo();
        fs::write(directory.path().join("tracked.ts"), "one\n").unwrap();
        run_git(directory.path(), &["add", "tracked.ts"]);
        run_git(directory.path(), &["commit", "-qm", "initial"]);
        fs::create_dir(directory.path().join("targetdir")).unwrap();
        symlink("targetdir", directory.path().join("linkdir")).unwrap();
        fs::write(directory.path().join("quote\"name.txt"), "quote\n").unwrap();
        fs::write(directory.path().join("tab\tname.txt"), "tab\n").unwrap();
        fs::write(directory.path().join("back\\slash.txt"), "backslash\n").unwrap();
        symlink("tracked.ts", directory.path().join("good-link")).unwrap();
        symlink("missing-file", directory.path().join("dangling-link")).unwrap();

        let changeset = GitProvider::discover(directory.path())
            .unwrap()
            .working_tree(&DiffRequest::default())
            .unwrap();
        let paths = changeset
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert!(!paths.contains(&"linkdir"));
        for path in [
            "quote\"name.txt",
            "tab\tname.txt",
            "back\\slash.txt",
            "good-link",
            "dangling-link",
        ] {
            assert!(
                paths.contains(&path),
                "missing exact untracked path {path:?}; loaded {paths:?}"
            );
        }
        let good = changeset
            .files
            .iter()
            .find(|file| file.path == "good-link")
            .unwrap();
        assert!(good.patch.contains("new file mode 120000"));
        assert!(good.patch.contains("+tracked.ts"));
        let dangling = changeset
            .files
            .iter()
            .find(|file| file.path == "dangling-link")
            .unwrap();
        assert!(dangling.patch.contains("+missing-file"));
    }

    #[test]
    fn frozen_hunk_loader_oracle_maps_every_baseline_source_test() {
        use std::collections::BTreeSet;

        let oracle: serde_json::Value = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../port/hunk/oracles/changeset-loaders.json"
        )))
        .unwrap();
        assert_eq!(oracle["schema_version"], 1);
        assert_eq!(
            oracle["source"]["baseline"]["commit"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        assert_eq!(
            oracle["source"]["baseline"]["source_blob"],
            "aece09bc494a7d5f56e6d70601c2fdfb967efaec"
        );
        assert_eq!(
            oracle["source"]["baseline"]["test_blob"],
            "9950f19daa506712c599567f68f3844390bdc7c0"
        );
        assert_eq!(oracle["source"]["baseline"]["result"]["passed"], 73);
        assert_eq!(oracle["source"]["baseline"]["result"]["skipped"], 3);
        assert_eq!(oracle["source"]["baseline"]["result"]["failed"], 0);
        assert_eq!(oracle["source"]["stable"]["result"]["passed"], 72);
        assert_eq!(oracle["source"]["stable"]["result"]["skipped"], 3);
        assert_eq!(oracle["source"]["stable"]["result"]["failed"], 0);

        let groups = oracle["test_groups"].as_array().unwrap();
        assert_eq!(groups.len(), 5);
        let mut tests = BTreeSet::new();
        for group in groups {
            let rust_tests = group["rust_tests"].as_array().unwrap();
            assert!(!rust_tests.is_empty());
            assert!(rust_tests.iter().all(|name| {
                name.as_str().is_some_and(|name| {
                    name.starts_with("workdeck_") && name.matches("::").count() >= 2
                })
            }));
            for test in group["hunk_tests"].as_array().unwrap() {
                assert!(tests.insert(test.as_str().unwrap()));
            }
        }
        assert_eq!(tests.len(), 76);
        assert!(
            tests.contains("preserves literal backslashes when matching exact Git-quoted paths")
        );
        assert!(tests.contains("`hunk stash show` pins expansion sources after stash@{0} moves"));
        assert!(tests.contains("includes Sapling unknown files in working copy reviews"));
    }

    fn initialized_git_repo() -> TempDir {
        let directory = TempDir::new().unwrap();
        run_git(
            directory.path(),
            &["init", "-q", "--initial-branch", "master"],
        );
        run_git(
            directory.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(directory.path(), &["config", "user.name", "Test"]);
        run_git(directory.path(), &["config", "commit.gpgsign", "false"]);
        directory
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
