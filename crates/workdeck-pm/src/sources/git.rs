//! One bounded Git object/index reader. It never checks out or publishes a developer index.
use super::{GitOid, GitRefName, IndexSelection, SourceCaptureLimits, SourceEntry, fs, process};
use crate::{ContentHash, ErrorCode, PmError, Result};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub(crate) struct BoundGit {
    root: PathBuf,
    supplied_root: PathBuf,
    directory: PathBuf,
    common: PathBuf,
    identities: (fs::Identity, fs::Identity, fs::Identity),
    limits: SourceCaptureLimits,
    deadline: Instant,
    isolated_config: bool,
}
#[derive(Debug, Clone)]
pub(crate) struct CapturedIndex {
    pub path: PathBuf,
    pub bytes: Option<Vec<u8>>,
    pub identity: Option<fs::Identity>,
    pub selection: IndexSelection,
    pub entries: Vec<SourceEntry>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookTarget {
    pub directory: PathBuf,
    pub configuration: ContentHash,
    pub worktree: PathBuf,
}

fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "bound Git source changed; capture a new source view",
    )
}
impl BoundGit {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        Self::with_limits(root, &SourceCaptureLimits::default())
    }
    pub(crate) fn with_limits(root: &Path, limits: &SourceCaptureLimits) -> Result<Self> {
        limits.validate()?;
        Self::with_deadline(
            root,
            limits,
            Instant::now() + Duration::from_secs(limits.timeout_seconds),
            false,
        )
    }
    pub(super) fn with_deadline(
        root: &Path,
        limits: &SourceCaptureLimits,
        deadline: Instant,
        isolated_config: bool,
    ) -> Result<Self> {
        let supplied_root = if root.is_absolute() {
            root.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|e| PmError::io(root, e))?
                .join(root)
        };
        let root = root.canonicalize().map_err(|e| PmError::io(root, e))?;
        let root_identity = fs::directory(&root)?;
        let run = |args: &[&str]| -> Result<String> {
            let mut command = command(&root, None, true, isolated_config);
            command.args(args);
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(PmError::new(
                    ErrorCode::Io,
                    "Git source operation exceeded its total timeout",
                ));
            }
            let output = process::run(command, None, 64 * 1024, remaining)?;
            if Instant::now() > deadline {
                return Err(PmError::new(
                    ErrorCode::Io,
                    "Git source operation exceeded its total timeout",
                ));
            }
            if !output.status.success() {
                return Err(PmError::new(
                    ErrorCode::Unsupported,
                    "source requires the selected non-bare Git worktree",
                ));
            }
            String::from_utf8(output.stdout).map_err(|_| invalid("Git path must be UTF8"))
        };
        // The same Git process observes all three paths. Preserve the individual
        // path protocol when embedded newlines make this framing ambiguous.
        let paths = run(&[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--absolute-git-dir",
            "--git-common-dir",
        ])?;
        let framed = paths
            .strip_suffix('\n')
            .map(|body| body.split('\n').collect::<Vec<_>>());
        let (actual, directory, common) = match framed.as_deref() {
            Some([actual, directory, common])
                if !actual.is_empty() && !directory.is_empty() && !common.is_empty() =>
            {
                (
                    PathBuf::from(actual),
                    PathBuf::from(directory),
                    PathBuf::from(common),
                )
            }
            _ => {
                let path = |args: &[&str]| {
                    run(args).map(|output| PathBuf::from(output.trim_end_matches('\n')))
                };
                (
                    path(&["rev-parse", "--show-toplevel"])?,
                    path(&["rev-parse", "--absolute-git-dir"])?,
                    path(&["rev-parse", "--path-format=absolute", "--git-common-dir"])?,
                )
            }
        };
        if actual.canonicalize().map_err(|e| PmError::io(&actual, e))? != root {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "source must bind the exact Git worktree root",
            ));
        }
        let identities = (
            root_identity,
            fs::directory(&directory)?,
            fs::directory(&common)?,
        );
        Ok(Self {
            root,
            supplied_root,
            directory,
            common,
            identities,
            limits: limits.clone(),
            deadline,
            isolated_config,
        })
        .and_then(|git| {
            if isolated_config {
                git.validate_shared_profile()?;
            }
            Ok(git)
        })
    }
    pub(crate) fn open_shared(root: &Path) -> Result<Self> {
        Self::with_shared_limits(root, &SourceCaptureLimits::default())
    }
    pub(crate) fn with_shared_limits(root: &Path, limits: &SourceCaptureLimits) -> Result<Self> {
        limits.validate()?;
        Self::with_deadline(
            root,
            limits,
            Instant::now() + Duration::from_secs(limits.timeout_seconds),
            true,
        )
    }
    fn validate_shared_profile(&self) -> Result<()> {
        for scope in ["--local", "--worktree"] {
            let result = self.run(
                &[
                    "config".into(),
                    scope.into(),
                    "--no-includes".into(),
                    "--get-regexp".into(),
                    "^[iI][nN][cC][lL][uU][dD][eE]".into(),
                ],
                None,
                None,
                64 * 1024,
            )?;
            if result.status.success() {
                return Err(PmError::new(
                    ErrorCode::Unsupported,
                    "shared Git operations require direct repository/worktree configuration; include/includeIf files are not admitted",
                ));
            }
            if result.status.code() != Some(1) {
                return Err(invalid("cannot validate direct shared Git configuration"));
            }
        }
        Ok(())
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(super) fn local_identity(&self) -> serde_json::Value {
        serde_json::json!({"root":self.root,"git_dir":self.directory,"common_dir":self.common,"root_inode":[self.identities.0.0,self.identities.0.1],"git_inode":[self.identities.1.0,self.identities.1.1],"common_inode":[self.identities.2.0,self.identities.2.1]})
    }
    /// A phase boundary checks the original budget and never renews it.
    pub(crate) fn check_deadline(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            Err(PmError::new(
                ErrorCode::Io,
                "Git source operation exceeded its total timeout",
            ))
        } else {
            Ok(())
        }
    }
    /// Explicit retained-view revalidation is a new bounded read operation.
    pub(super) fn for_revalidation(&self) -> Self {
        let mut next = self.clone();
        next.deadline = Instant::now() + Duration::from_secs(self.limits.timeout_seconds);
        next
    }
    pub(super) fn for_revalidation_before(&self, deadline: Instant) -> Self {
        let mut next = self.clone();
        next.deadline = deadline;
        next
    }
    pub(crate) fn verify(&self) -> Result<()> {
        if (
            fs::directory(&self.root)?,
            fs::directory(&self.directory)?,
            fs::directory(&self.common)?,
        ) != self.identities
        {
            return Err(stale());
        }
        let reopened = Self::with_deadline(
            &self.root,
            &self.limits,
            self.deadline,
            self.isolated_config,
        )?;
        if reopened.directory != self.directory
            || reopened.common != self.common
            || reopened.identities != self.identities
        {
            return Err(stale());
        }
        Ok(())
    }
    pub(super) fn run(
        &self,
        args: &[OsString],
        input: Option<Vec<u8>>,
        index: Option<&Path>,
        bound: usize,
    ) -> Result<process::Output> {
        self.run_environment(args, input, index, bound, &[])
    }
    pub(super) fn run_environment(
        &self,
        args: &[OsString],
        input: Option<Vec<u8>>,
        index: Option<&Path>,
        bound: usize,
        environment: &[(&str, &str)],
    ) -> Result<process::Output> {
        let remaining = self.deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(PmError::new(
                ErrorCode::Io,
                "Git source capture exceeded its timeout",
            ));
        }
        let mut command = command(&self.root, index, true, self.isolated_config);
        command.args(args).envs(environment.iter().copied());
        let result = process::run(command, input, bound, remaining)?;
        if Instant::now() > self.deadline {
            return Err(PmError::new(
                ErrorCode::Io,
                "Git source capture exceeded its timeout",
            ));
        }
        Ok(result)
    }
    pub(crate) fn output(
        &self,
        args: &[OsString],
        index: Option<&Path>,
        bound: usize,
    ) -> Result<Vec<u8>> {
        let result = self.run(args, None, index, bound)?;
        if !result.status.success() {
            return Err(PmError::new(
                ErrorCode::CorruptStore,
                "Git could not read the selected source",
            ));
        }
        Ok(result.stdout)
    }
    pub(crate) fn head(&self) -> Result<Option<GitOid>> {
        self.resolve("HEAD")
    }
    pub(crate) fn head_ref(&self) -> Result<Option<GitRefName>> {
        let result = self.run(
            &["symbolic-ref".into(), "--quiet".into(), "HEAD".into()],
            None,
            None,
            4096,
        )?;
        if result.status.code() == Some(1) {
            return Ok(None);
        }
        if !result.status.success() {
            return Err(invalid("cannot determine HEAD reference"));
        }
        Ok(Some(text(&result.stdout)?.parse()?))
    }
    pub(crate) fn resolve(&self, reference: &str) -> Result<Option<GitOid>> {
        if reference != "HEAD" {
            reference.parse::<GitRefName>()?;
        }
        self.resolve_commit_name(reference)
    }
    /// Exact commit IDs are admitted separately from publication ref names.
    pub(crate) fn resolve_commit(&self, oid: &GitOid) -> Result<Option<GitOid>> {
        let resolved = self.resolve_commit_name(oid.as_str())?;
        if resolved.as_ref().is_some_and(|commit| commit != oid) {
            return Err(invalid("CI object identity must name a commit, not a tag"));
        }
        Ok(resolved)
    }
    fn resolve_commit_name(&self, reference: &str) -> Result<Option<GitOid>> {
        let result = self.run(
            &[
                "rev-parse".into(),
                "--verify".into(),
                "--quiet".into(),
                "--end-of-options".into(),
                format!("{reference}^{{commit}}").into(),
            ],
            None,
            None,
            4096,
        )?;
        if result.status.code() == Some(1) {
            return Ok(None);
        }
        if !result.status.success() {
            return Err(invalid("cannot resolve source reference"));
        }
        Ok(Some(text(&result.stdout)?.parse()?))
    }
    pub(crate) fn tree(&self, commit: &GitOid) -> Result<GitOid> {
        text(&self.output(
            &[
                "rev-parse".into(),
                "--verify".into(),
                format!("{commit}^{{tree}}").into(),
            ],
            None,
            4096,
        )?)?
        .parse()
    }
    pub(crate) fn tree_entries(&self, tree: &GitOid) -> Result<Vec<SourceEntry>> {
        let bytes = self.output(
            &[
                "ls-tree".into(),
                "-r".into(),
                "-z".into(),
                "--full-tree".into(),
                tree.as_str().into(),
                "--".into(),
                ".workdeck".into(),
            ],
            None,
            self.list_bound(),
        )?;
        self.parse_entries(&bytes, false)
    }
    /// Literal repository paths for evaluator inputs, without changing planning
    /// namespace parsing or enabling checkout, filters, replacement objects or fetch.
    pub(super) fn evaluator_tree_entries(
        &self,
        tree: &GitOid,
        paths: &[PathBuf],
    ) -> Result<Vec<SourceEntry>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let mut args: Vec<OsString> = ["ls-tree", "-r", "-t", "-z", "--full-tree"]
            .into_iter()
            .map(Into::into)
            .collect();
        args.push(tree.as_str().into());
        args.push("--".into());
        for path in paths {
            crate::commands::validation::relative(path, true)?;
            args.push(path.as_os_str().into());
        }
        let bytes = self.output(&args, None, self.list_bound())?;
        self.parse_entries_with_root(&bytes, false, false)
    }
    fn list_bound(&self) -> usize {
        self.limits
            .max_entries
            .saturating_mul(2048)
            .min(self.limits.max_total_bytes)
            .max(4096)
    }
    pub(super) fn blobs(&self, oids: &[GitOid]) -> Result<Vec<Vec<u8>>> {
        super::batch::read(self, oids, &self.limits)
    }
    pub(crate) fn index_path(&self, selection: &IndexSelection) -> Result<PathBuf> {
        let explicit = match selection {
            IndexSelection::Default => None,
            IndexSelection::EffectiveHook => std::env::var_os("GIT_INDEX_FILE").map(PathBuf::from),
            IndexSelection::Explicit { path } => Some(path.clone()),
        };
        // Normalize only the caller's already-admitted root spelling (for
        // example macOS /var -> /private/var). Every suffix remains no-follow.
        let path = explicit.unwrap_or_else(|| self.directory.join("index"));
        if path.components().any(|part| {
            matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        }) {
            return Err(fs::unsafe_path(&path));
        }
        if !path.is_absolute() {
            return Ok(self.root.join(path));
        }
        if let Ok(suffix) = path.strip_prefix(&self.supplied_root) {
            return Ok(self.root.join(suffix));
        }
        // A hook may supply macOS /var while current_dir already returned
        // /private/var. Admit only an ancestor resolving to this exact bound
        // worktree; never resolve the candidate file or its remaining suffix.
        for ancestor in path.ancestors().skip(1) {
            if ancestor
                .canonicalize()
                .is_ok_and(|resolved| resolved == self.root)
            {
                return Ok(self.root.join(
                    path.strip_prefix(ancestor)
                        .map_err(|_| fs::unsafe_path(&path))?,
                ));
            }
        }
        Ok(path)
    }
    pub(crate) fn capture_index(&self, selection: &IndexSelection) -> Result<CapturedIndex> {
        let path = self.index_path(selection)?;
        let (bytes, identity) = match std::fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::directory(path.parent().ok_or_else(|| fs::unsafe_path(&path))?)?;
                (None, None)
            }
            Err(e) => return Err(PmError::io(&path, e)),
            Ok(_) => {
                let (bytes, id) = fs::read(&path, self.limits.max_index_bytes)?;
                (Some(bytes), Some(id))
            }
        };
        let listing = self.output(
            &[
                "ls-files".into(),
                "--stage".into(),
                "-z".into(),
                "--".into(),
                ".workdeck".into(),
            ],
            Some(&path),
            self.list_bound(),
        )?;
        self.reject_intent_to_add(&path)?;
        let captured = CapturedIndex {
            path,
            bytes,
            identity,
            selection: selection.clone(),
            entries: self.parse_entries(&listing, true)?,
        };
        self.verify_index(&captured)?;
        Ok(captured)
    }
    fn reject_intent_to_add(&self, path: &Path) -> Result<()> {
        let bytes = self.output(
            &[
                "ls-files".into(),
                "--debug".into(),
                "-z".into(),
                "--".into(),
                ".workdeck".into(),
            ],
            Some(path),
            self.list_bound(),
        )?;
        let mut remaining = bytes.as_slice();
        while !remaining.is_empty() {
            let end = remaining
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| invalid("invalid Git debug index path"))?;
            let path = std::str::from_utf8(&remaining[..end])
                .map_err(|_| invalid("index paths must be UTF8"))?;
            remaining = &remaining[end + 1..];
            let mut flags = None;
            for _ in 0..5 {
                let end = remaining
                    .iter()
                    .position(|&b| b == b'\n')
                    .ok_or_else(|| invalid("invalid Git index flags"))?;
                let line = std::str::from_utf8(&remaining[..end])
                    .map_err(|_| invalid("invalid index debug response"))?;
                if let Some((_, value)) = line.split_once("flags: ") {
                    flags = Some(
                        u32::from_str_radix(value.trim(), 16)
                            .map_err(|_| invalid("invalid index flags"))?,
                    );
                }
                remaining = &remaining[end + 1..];
            }
            let relative = Path::new(path)
                .strip_prefix(".workdeck")
                .map_err(|_| invalid("index path escaped planning namespace"))?;
            if super::capture_core::authoritative(relative)?
                && flags.ok_or_else(|| invalid("missing Git index flags"))? & 0x2000_0000 != 0
            {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "intent-to-add planning entries have no staged authority",
                )
                .at(relative));
            }
        }
        Ok(())
    }
    pub(crate) fn verify_index(&self, captured: &CapturedIndex) -> Result<()> {
        if self.index_path(&captured.selection)? != captured.path {
            return Err(stale());
        }
        let current = match std::fs::symlink_metadata(&captured.path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (None, None),
            Err(e) => return Err(PmError::io(&captured.path, e)),
            Ok(_) => {
                let (bytes, id) = fs::read(&captured.path, self.limits.max_index_bytes)?;
                (Some(bytes), Some(id))
            }
        };
        if current != (captured.bytes.clone(), captured.identity) {
            return Err(stale());
        }
        Ok(())
    }
    fn parse_entries(&self, bytes: &[u8], index: bool) -> Result<Vec<SourceEntry>> {
        self.parse_entries_with_root(bytes, index, true)
    }
    fn parse_entries_with_root(
        &self,
        bytes: &[u8],
        index: bool,
        planning: bool,
    ) -> Result<Vec<SourceEntry>> {
        let mut entries = Vec::new();
        for row in bytes.split(|&b| b == 0).filter(|row| !row.is_empty()) {
            if entries.len() >= self.limits.max_entries {
                return Err(invalid("Git namespace exceeds source entry bound"));
            }
            let row =
                std::str::from_utf8(row).map_err(|_| invalid("planning paths must be UTF8"))?;
            let (header, path) = row
                .split_once('\t')
                .ok_or_else(|| invalid("malformed Git entry"))?;
            let columns = header.split_whitespace().collect::<Vec<_>>();
            if columns.len() != 3 {
                return Err(invalid("malformed Git entry header"));
            }
            let path = if planning {
                Path::new(path)
                    .strip_prefix(".workdeck")
                    .map_err(|_| invalid("Git source entry is outside planning namespace"))?
                    .to_path_buf()
            } else {
                PathBuf::from(path)
            };
            let (oid, stage) = if index {
                (
                    columns[1].parse()?,
                    columns[2]
                        .parse()
                        .map_err(|_| invalid("invalid index stage"))?,
                )
            } else {
                (columns[2].parse()?, 0)
            };
            entries.push(SourceEntry {
                path,
                mode: columns[0].into(),
                oid: Some(oid),
                stage,
            });
        }
        Ok(entries)
    }
    pub(crate) fn hook_target(&self) -> Result<HookTarget> {
        let mut command = command(&self.root, None, false, self.isolated_config);
        command.args(["config", "--path", "--get", "core.hooksPath"]);
        let output = process::run(
            command,
            None,
            64 * 1024,
            self.deadline.saturating_duration_since(Instant::now()),
        )?;
        let directory = if output.status.code() == Some(1) {
            self.common.join("hooks")
        } else if output.status.success() {
            let path = PathBuf::from(text(&output.stdout)?);
            if path.is_absolute() {
                path
            } else {
                self.root.join(path)
            }
        } else {
            return Err(invalid("cannot resolve configured Git hook directory"));
        };
        Ok(HookTarget {
            directory,
            configuration: ContentHash::of(&output.stdout),
            worktree: self.root.clone(),
        })
    }
}
fn text(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes)
        .map(str::trim)
        .map_err(|_| invalid("Git response must be UTF8"))
}
fn command(
    root: &Path,
    index: Option<&Path>,
    disable_hooks: bool,
    isolated_config: bool,
) -> Command {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    command
        .current_dir(root)
        .args([
            "--no-lazy-fetch",
            "--literal-pathspecs",
            "--no-replace-objects",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.splitIndex=false",
            "-c",
            "maintenance.auto=false",
            "-c",
            "gc.auto=0",
        ])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0");
    if isolated_config {
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
    }
    if disable_hooks {
        command.args(["-c", "core.hooksPath=/dev/null"]);
    }
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    command
}

#[cfg(all(test, unix))]
mod deadline_tests {
    use super::*;
    #[test]
    fn ordinary_git_binding_uses_one_probe_and_keeps_exact_root_guards() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .current_dir(temp.path())
                .args(["init", "-b", "main"])
                .output()
                .unwrap()
                .status
                .success()
        );
        process::take_invocations();
        let git = BoundGit::open(temp.path()).unwrap();
        assert_eq!(
            process::take_invocations(),
            1,
            "Git binding repeats process setup for each path"
        );
        git.verify().unwrap();
        assert_eq!(process::take_invocations(), 1);
        let child = temp.path().join("subdirectory");
        std::fs::create_dir(&child).unwrap();
        assert_eq!(
            BoundGit::open(&child).unwrap_err().code,
            ErrorCode::UnsafePath
        );
    }
    #[test]
    fn newline_worktree_paths_keep_unambiguous_individual_path_observations() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("root\nwith-newline");
        std::fs::create_dir(&root).unwrap();
        assert!(
            Command::new("git")
                .current_dir(&root)
                .args(["init", "-b", "main"])
                .output()
                .unwrap()
                .status
                .success()
        );
        let git = BoundGit::open(&root).unwrap();
        assert_eq!(git.root, root.canonicalize().unwrap());
        assert_eq!(git.directory, root.canonicalize().unwrap().join(".git"));
        assert_eq!(git.common, git.directory);
        git.verify().unwrap();
    }
    #[test]
    fn phase_restart_cannot_extend_the_original_git_operation_deadline() {
        let temp = tempfile::tempdir().unwrap();
        assert!(
            Command::new("git")
                .current_dir(temp.path())
                .args(["init", "-b", "main"])
                .output()
                .unwrap()
                .status
                .success()
        );
        let git = BoundGit::with_limits(
            temp.path(),
            &SourceCaptureLimits {
                timeout_seconds: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let delay = ["-c".into(), "alias.pause=!sleep 0.6".into(), "pause".into()];
        git.output(&delay, None, 4096).unwrap();
        git.check_deadline().unwrap();
        assert!(
            git.output(&delay, None, 4096).is_err(),
            "per-phase restart extended the admitted whole-operation deadline"
        );
    }
}

#[derive(Clone)]
pub(super) struct LocalGitGuard {
    directories: Vec<(PathBuf, fs::Identity)>,
    files: Vec<(PathBuf, LocalPin)>,
    deadline: Instant,
}
#[derive(Clone, PartialEq, Eq)]
enum LocalPin {
    Missing,
    Directory(fs::Identity),
    File(fs::Identity, ContentHash),
}
fn local_pin(path: &Path) -> Result<LocalPin> {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(LocalPin::Missing),
        Err(error) => Err(PmError::io(path, error)),
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {
            Ok(LocalPin::Directory(fs::directory(path)?))
        }
        Ok(_) => {
            let (bytes, identity) = fs::read(path, 1024 * 1024)?;
            Ok(LocalPin::File(identity, ContentHash::of(&bytes)))
        }
    }
}
impl BoundGit {
    /// Supported shared profile: global/system config is disabled and every
    /// local/worktree include directive is rejected before remote operations.
    /// These direct files therefore cover all configurable source routing.
    pub(super) fn local_guard(&self) -> Result<LocalGitGuard> {
        if !self.isolated_config {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "locked Git source guards require the isolated shared configuration profile",
            ));
        }
        let paths = [
            self.root.join(".git"),
            self.common.join("config"),
            self.directory.join("config.worktree"),
            self.directory.join("commondir"),
            self.directory.join("HEAD"),
            self.common.join("HEAD"),
        ];
        let mut files = Vec::new();
        for path in paths {
            files.push((path.clone(), local_pin(&path)?));
        }
        let guard = LocalGitGuard {
            directories: vec![
                (self.root.clone(), self.identities.0),
                (self.directory.clone(), self.identities.1),
                (self.common.clone(), self.identities.2),
            ],
            files,
            deadline: self.deadline,
        };
        guard.verify()?;
        Ok(guard)
    }
}
impl LocalGitGuard {
    pub(super) fn verify(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            return Err(stale());
        }
        for (path, identity) in &self.directories {
            if fs::directory(path)? != *identity {
                return Err(stale());
            }
        }
        for (path, pin) in &self.files {
            if local_pin(path)? != *pin {
                return Err(stale());
            }
        }
        Ok(())
    }
}
