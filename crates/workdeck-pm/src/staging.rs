//! Explicit, receipt-scoped Git staging. The real index is published only after
//! an alternate index and every affected source precondition have been checked.
//! Cooperating Git writers honor index.lock. This does not protect against a
//! hostile process swapping paths in the remaining check/rename interval.

use crate::{
    ContentHash, ErrorCode, OperationId, PmError, Repository, RepositoryId, Result, SourceLink,
    transactions::{MAX_TRANSACTION_FILE_BYTES, MutationReceipt},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    process::{Command, Output, Stdio},
};

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagingReport {
    pub repository: RepositoryId,
    pub operation_id: OperationId,
    /// Literal paths relative to the Git worktree root, including the receipt.
    pub paths: Vec<PathBuf>,
    pub index_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagingFaultPoint {
    AfterLock,
    BeforePublish,
}

impl Repository {
    pub fn stage_operation(&self, receipt: &MutationReceipt) -> Result<StagingReport> {
        self.stage_operation_with_faults(receipt, |_| Ok(()))
    }

    /// Deterministic preparation/interruption seam; never bypasses validation.
    #[doc(hidden)]
    pub fn stage_operation_with_faults(
        &self,
        receipt: &MutationReceipt,
        mut fault: impl FnMut(StagingFaultPoint) -> Result<()>,
    ) -> Result<StagingReport> {
        if !cfg!(unix) {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "atomic staging requires qualified Unix index and directory durability",
            ));
        }
        self.store()?.with_snapshot(|snapshot| {
            let receipt_path = PathBuf::from(format!("operations/{}.yml", receipt.operation_id));
            let bytes = snapshot.read(&receipt_path)?.ok_or_else(|| PmError::new(ErrorCode::NotFound, "durable operation receipt was not found").at(self.root().join(&receipt_path)))?;
            let stored: MutationReceipt = serde_yaml_ng::from_slice(&bytes).map_err(|error| PmError::new(ErrorCode::CorruptStore, format!("durable receipt is invalid: {error}")).at(self.root().join(&receipt_path)))?;
            if &stored != receipt || receipt.repository.as_ref() != Some(self.identity()) {
                return Err(PmError::new(ErrorCode::Conflict, "provided receipt does not match this repository's durable operation"));
            }
            crate::transactions::validate_receipt(receipt)?;
            crate::graph::validate_receipt(receipt)?;
            crate::wiki::validate_receipt(receipt)?;
            crate::saved_views::validate_receipt(receipt)?;
            crate::features::validate_receipt(receipt)?;
            crate::gates::store::validate_receipt(receipt)?;
            crate::evidence::store::validate_receipt(receipt)?;
            crate::questions::validate_receipt(receipt)?;
            crate::handoffs::validate_receipt(receipt)?;
            crate::execution::records::validate_receipt(receipt)?;
                crate::claims::validate_receipt(receipt)?;
                crate::completion::validate_receipt(receipt)?;
            let restoration = if receipt.operation == "snapshot.restore" {
                Some(crate::restore::validate_restore_receipt(receipt)?)
            } else {
                None
            };
            let changes = restoration.as_ref().map_or(receipt.changed.as_slice(), |restore| restore.restored.as_slice());
            let git = Git::for_source(self.root())?;
            let index = git.index_path()?;
            let mut lock = IndexLock::acquire(&index)?;
            fault(StagingFaultPoint::AfterLock)?;
            let original_index = read_regular(&index)?;
            let head = git.head()?;
            let config = snapshot.read(Path::new("config.yml"))?.ok_or_else(|| PmError::new(ErrorCode::CorruptStore, "planning config disappeared"))?;
            let mut paths = BTreeMap::<PathBuf, PlannedPath>::new();
            for changed in changes {
                let original_operation = if restoration.is_some() && changed.path.starts_with("operations") {
                    if changed.path.parent() != Some(Path::new("operations")) {
                        return Err(PmError::new(ErrorCode::CorruptStore, "restored operation path must be canonical").at(&changed.path));
                    }
                    Some(changed.path.file_name().and_then(OsStr::to_str).and_then(|name| name.strip_suffix(".yml"))
                        .ok_or_else(|| PmError::new(ErrorCode::CorruptStore, "restored receipt filename is invalid").at(&changed.path))?
                        .parse::<OperationId>()?)
                } else {
                    validate_application_path(&changed.path)?;
                    None
                };
                let source = snapshot.read(&changed.path)?;
                if source.as_deref().map(ContentHash::of) != changed.after {
                    return Err(stale(self.root().join(&changed.path), "published planning content changed before staging"));
                }
                if let Some(operation) = original_operation {
                    let original: MutationReceipt = serde_yaml_ng::from_slice(source.as_deref().ok_or_else(|| PmError::new(ErrorCode::CorruptStore,"restored original receipt is missing").at(&changed.path))?)
                        .map_err(|error| PmError::new(ErrorCode::CorruptStore, error.to_string()).at(&changed.path))?;
                    crate::transactions::validate_receipt(&original)?;
                    crate::graph::validate_receipt(&original)?;
                    crate::wiki::validate_receipt(&original)?;
                    crate::saved_views::validate_receipt(&original)?;
                    crate::features::validate_receipt(&original)?;
                    crate::gates::store::validate_receipt(&original)?;
                    crate::evidence::store::validate_receipt(&original)?;
            crate::questions::validate_receipt(&original)?;
            crate::handoffs::validate_receipt(&original)?;
            crate::execution::records::validate_receipt(&original)?;
                crate::claims::validate_receipt(&original)?;
                crate::completion::validate_receipt(&original)?;
                    if original.operation_id != operation || original.repository.as_ref() != Some(self.identity()) {
                        return Err(PmError::new(ErrorCode::CorruptStore,"restored receipt source or identity is invalid").at(&changed.path));
                    }
                }
                let git_path = Path::new(".workdeck").join(&changed.path);
                if paths.insert(git_path, PlannedPath { source, before: changed.before.clone(), mode: String::new(), oid: None }).is_some() {
                    return Err(PmError::new(ErrorCode::CorruptStore, "receipt repeats an application path"));
                }
            }
            let git_receipt_path = Path::new(".workdeck").join(&receipt_path);
            if paths.insert(git_receipt_path, PlannedPath { source: Some(bytes), before: None, mode: String::new(), oid: None }).is_some() {
                return Err(PmError::new(ErrorCode::CorruptStore, "receipt lists itself as an application change"));
            }
            let selected = paths.keys().cloned().collect::<Vec<_>>();
            let indexed = git.index_entries(&selected)?;
            let committed = git.head_entries(head.as_deref(), &selected)?;
            let mut changed = false;
            for (path, planned) in &mut paths {
                let current = indexed.get(path);
                let baseline = committed.get(path);
                for entry in [current, baseline].into_iter().flatten() {
                    if !matches!(entry.mode.as_str(), "100644" | "100755") { return Err(PmError::new(ErrorCode::UnsafePath, "affected Git entries must be regular files").at(path)); }
                }
                planned.mode = baseline.map(|entry| entry.mode.clone()).unwrap_or_else(|| "100644".into());
                let current_hash = current.map(|entry| git.blob_hash(&entry.oid)).transpose()?;
                let desired_hash = planned.source.as_deref().map(ContentHash::of);
                let matches_after = current_hash == desired_hash && current.is_none_or(|entry| entry.mode == planned.mode);
                let matches_before = current_hash == planned.before && current.is_none_or(|entry| entry.mode == planned.mode);
                if current != baseline && !matches_before && !matches_after {
                    return Err(PmError::new(ErrorCode::Conflict, "affected path already has unrelated staged content; preserve or reconcile it before staging this operation").at(path));
                }
                changed |= !matches_after;
            }
            let mut alternate = None;
            if changed {
                let prepared = AlternateIndex::new(&index, original_index.as_deref(), &git)?;
                for planned in paths.values_mut() {
                    if let Some(source) = &planned.source { planned.oid = Some(git.hash_object(source)?); }
                }
                git.update_index(&prepared.path, &paths)?;
                // Expand copied split indexes before publication so the new
                // index does not depend on a newly generated auxiliary file.
                git.run(&[OsString::from("update-index"), OsString::from("--no-split-index")], None, Some(&prepared.path))?;
                alternate = Some(prepared);
            }
            fault(StagingFaultPoint::BeforePublish)?;
            // Snapshot reads are cached; re-open the actual source here to catch
            // direct editors which do not acquire the planning writer lock.
            for (path, planned) in &paths {
                if read_regular(&git.root.join(path))? != planned.source {
                    return Err(stale(git.root.join(path), "planning content changed during staging preparation"));
                }
            }
            if read_regular(&self.root().join("config.yml"))? != Some(config) {
                return Err(stale(self.root().join("config.yml"), "planning configuration changed during staging preparation"));
            }
            if read_regular(&index)? != original_index || git.head()? != head {
                return Err(stale(&index, "Git index or HEAD changed during staging preparation"));
            }
            lock.verify()?;
            if let Some(alternate) = alternate {
                let bytes = read_regular(&alternate.path)?.ok_or_else(|| stale(&alternate.path, "prepared index disappeared"))?;
                lock.publish(&bytes)?;
            }
            Ok(StagingReport { repository: self.identity().clone(), operation_id: receipt.operation_id.clone(), paths: selected, index_changed: changed })
        })
    }
}

struct PlannedPath {
    source: Option<Vec<u8>>,
    before: Option<ContentHash>,
    mode: String,
    oid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    mode: String,
    oid: String,
}

struct Git {
    root: PathBuf,
}
impl Git {
    fn for_source(source: &Path) -> Result<Self> {
        if source.file_name() != Some(OsStr::new(".workdeck")) {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "staging requires the Git root's .workdeck source",
            ));
        }
        let root = source
            .parent()
            .ok_or_else(|| {
                PmError::new(ErrorCode::UnsafePath, "planning source has no project root")
            })?
            .to_owned();
        check_path(&root.join(".git"), false)?;
        let git = Self { root };
        if git.text(&["rev-parse", "--is-inside-work-tree"])? != "true" {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "staging requires a non-bare Git worktree",
            ));
        }
        let actual = PathBuf::from(git.text(&["rev-parse", "--show-toplevel"])?);
        if actual
            .canonicalize()
            .map_err(|error| PmError::io(&actual, error))?
            != git.root
        {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "planning source is not directly under the Git worktree root",
            ));
        }
        Ok(git)
    }

    fn command(&self, index: Option<&Path>) -> Command {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
                command.env_remove(key);
            }
        }
        command
            .current_dir(&self.root)
            .arg("--literal-pathspecs")
            .arg("--no-replace-objects")
            .arg("-c")
            .arg("core.hooksPath=/dev/null")
            .arg("-c")
            .arg("core.fsmonitor=false")
            .arg("-c")
            .arg("core.splitIndex=false")
            .arg("-c")
            .arg("core.untrackedCache=false")
            .env("GIT_OPTIONAL_LOCKS", "0");
        if let Some(index) = index {
            command.env("GIT_INDEX_FILE", index);
        }
        command
    }

    fn run_raw(
        &self,
        args: &[OsString],
        input: Option<&[u8]>,
        index: Option<&Path>,
    ) -> Result<Output> {
        let mut command = self.command(index);
        command
            .args(args)
            .stdin(if input.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|error| PmError::io(&self.root, error))?;
        if let Some(input) = input
            && let Err(error) = child.stdin.take().expect("piped input").write_all(input)
        {
            let _ = child.kill();
            let _ = child.wait();
            return Err(PmError::io(&self.root, error));
        }
        child
            .wait_with_output()
            .map_err(|error| PmError::io(&self.root, error))
    }

    fn run(
        &self,
        args: &[OsString],
        input: Option<&[u8]>,
        index: Option<&Path>,
    ) -> Result<Vec<u8>> {
        let output = self.run_raw(args, input, index)?;
        if !output.status.success() {
            return Err(PmError::new(
                ErrorCode::Io,
                format!(
                    "Git staging preparation failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            )
            .at(&self.root));
        }
        Ok(output.stdout)
    }

    fn text(&self, args: &[&str]) -> Result<String> {
        String::from_utf8(self.run(
            &args.iter().map(OsString::from).collect::<Vec<_>>(),
            None,
            None,
        )?)
        .map(|text| text.trim().into())
        .map_err(|_| {
            PmError::new(
                ErrorCode::Unsupported,
                "Git reported a non-UTF-8 path or object identity",
            )
        })
    }

    fn index_path(&self) -> Result<PathBuf> {
        let path = PathBuf::from(self.text(&[
            "rev-parse",
            "--path-format=absolute",
            "--git-path",
            "index",
        ])?);
        let directory = PathBuf::from(self.text(&["rev-parse", "--absolute-git-dir"])?);
        if path != directory.join("index") {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "Git index is redirected outside the selected worktree's Git directory",
            ));
        }
        check_path(&path, true)?;
        Ok(path)
    }

    fn head(&self) -> Result<Option<String>> {
        let output = self.run_raw(
            &[
                "rev-parse".into(),
                "--verify".into(),
                "--quiet".into(),
                "HEAD".into(),
            ],
            None,
            None,
        )?;
        if output.status.code() == Some(1) {
            return Ok(None);
        }
        if !output.status.success() {
            return Err(PmError::new(ErrorCode::Io, "cannot resolve Git HEAD"));
        }
        Ok(Some(parse_oid(&output.stdout)?))
    }

    fn index_entries(&self, paths: &[PathBuf]) -> Result<BTreeMap<PathBuf, Entry>> {
        let mut args = vec![
            "ls-files".into(),
            "--stage".into(),
            "-z".into(),
            "--".into(),
        ];
        args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
        parse_entries(&self.run(&args, None, None)?, true)
    }

    fn head_entries(
        &self,
        head: Option<&str>,
        paths: &[PathBuf],
    ) -> Result<BTreeMap<PathBuf, Entry>> {
        let Some(head) = head else {
            return Ok(BTreeMap::new());
        };
        let mut args = vec![
            "ls-tree".into(),
            "-r".into(),
            "-z".into(),
            "--full-tree".into(),
            head.into(),
            "--".into(),
        ];
        args.extend(paths.iter().map(|path| path.as_os_str().to_owned()));
        parse_entries(&self.run(&args, None, None)?, false)
    }

    fn blob_hash(&self, oid: &str) -> Result<ContentHash> {
        let size = self
            .text(&["cat-file", "-s", oid])?
            .parse::<usize>()
            .map_err(|_| PmError::new(ErrorCode::CorruptStore, "Git blob size is invalid"))?;
        if size > MAX_TRANSACTION_FILE_BYTES {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "affected staged blob exceeds the staging read limit",
            ));
        }
        Ok(ContentHash::of(&self.run(
            &["cat-file".into(), "blob".into(), oid.into()],
            None,
            None,
        )?))
    }

    fn hash_object(&self, bytes: &[u8]) -> Result<String> {
        parse_oid(&self.run(
            &[
                "hash-object".into(),
                "-w".into(),
                "--stdin".into(),
                "--no-filters".into(),
            ],
            Some(bytes),
            None,
        )?)
    }

    fn update_index(&self, index: &Path, paths: &BTreeMap<PathBuf, PlannedPath>) -> Result<()> {
        let oid_length = paths
            .values()
            .find_map(|planned| planned.oid.as_ref().map(String::len))
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::CorruptStore,
                    "staging has no durable receipt blob",
                )
            })?;
        let mut input = Vec::new();
        for (path, planned) in paths {
            let prefix = if let Some(oid) = &planned.oid {
                format!("{} {oid}\t", planned.mode)
            } else {
                format!("0 {}\t", "0".repeat(oid_length))
            };
            input.extend_from_slice(prefix.as_bytes());
            input.extend_from_slice(
                path.to_str()
                    .ok_or_else(|| {
                        PmError::new(ErrorCode::Unsupported, "planning Git paths must be UTF-8")
                    })?
                    .as_bytes(),
            );
            input.push(0);
        }
        self.run(
            &["update-index".into(), "-z".into(), "--index-info".into()],
            Some(&input),
            Some(index),
        )?;
        Ok(())
    }
}

fn parse_entries(bytes: &[u8], index: bool) -> Result<BTreeMap<PathBuf, Entry>> {
    let mut entries = BTreeMap::new();
    for row in bytes.split(|byte| *byte == 0).filter(|row| !row.is_empty()) {
        let row = std::str::from_utf8(row).map_err(|_| {
            PmError::new(ErrorCode::Unsupported, "affected Git paths must be UTF-8")
        })?;
        let (header, path) = row
            .split_once('\t')
            .ok_or_else(|| PmError::new(ErrorCode::CorruptStore, "Git index entry is malformed"))?;
        let parts = header.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 || (index && parts[2] != "0") {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "affected path has unresolved merge stages",
            ));
        }
        let entry = Entry {
            mode: parts[0].into(),
            oid: if index { parts[1] } else { parts[2] }.into(),
        };
        parse_oid(entry.oid.as_bytes())?;
        if entries.insert(PathBuf::from(path), entry).is_some() {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "affected path has multiple Git index entries",
            ));
        }
    }
    Ok(entries)
}

fn parse_oid(bytes: &[u8]) -> Result<String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| PmError::new(ErrorCode::CorruptStore, "Git object identity is not UTF-8"))?
        .trim();
    if !matches!(text.len(), 40 | 64)
        || !text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PmError::new(
            ErrorCode::CorruptStore,
            "Git object identity is malformed",
        ));
    }
    Ok(text.into())
}

fn validate_application_path(path: &Path) -> Result<()> {
    SourceLink {
        path: path
            .to_str()
            .ok_or_else(|| PmError::new(ErrorCode::UnsafePath, "receipt path must be UTF-8"))?
            .into(),
        line: None,
        end_line: None,
    }
    .validate()?;
    if path.components().next().is_some_and(|part| {
        matches!(
            part.as_os_str().to_str(),
            Some(".git" | ".tmp" | ".index" | ".local" | "operations")
        )
    }) {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "receipt application path is not stageable planning authority",
        )
        .at(path));
    }
    Ok(())
}

fn stale(path: impl AsRef<Path>, message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message).at(path)
}

fn check_path(path: &Path, regular_leaf: bool) -> Result<()> {
    let mut candidate = PathBuf::new();
    for component in path.components() {
        if matches!(component, Component::ParentDir | Component::CurDir) {
            return Err(
                PmError::new(ErrorCode::UnsafePath, "staging path is not canonical").at(path),
            );
        }
        candidate.push(component.as_os_str());
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "staging does not follow symbolic links",
                )
                .at(&candidate));
            }
            Ok(metadata) if candidate != path && !metadata.is_dir() => {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "staging path parent must be a directory",
                )
                .at(&candidate));
            }
            Ok(metadata)
                if candidate == path
                    && (regular_leaf && !metadata.is_file()
                        || !regular_leaf && !metadata.is_file() && !metadata.is_dir()) =>
            {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "staging path must be a regular file or Git directory",
                )
                .at(&candidate));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PmError::io(&candidate, error)),
        }
    }
    Ok(())
}

fn read_regular(path: &Path) -> Result<Option<Vec<u8>>> {
    check_path(path, true)?;
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PmError::io(path, error)),
    };
    if !file
        .metadata()
        .map_err(|error| PmError::io(path, error))?
        .is_file()
    {
        return Err(
            PmError::new(ErrorCode::UnsafePath, "staging requires a regular file").at(path),
        );
    }
    let mut bytes = Vec::new();
    file.take(MAX_TRANSACTION_FILE_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| PmError::io(path, error))?;
    if bytes.len() > MAX_TRANSACTION_FILE_BYTES {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "staging source or index exceeds the bounded read limit",
        )
        .at(path));
    }
    Ok(Some(bytes))
}

struct IndexLock {
    path: PathBuf,
    index: PathBuf,
    file: File,
    identity: (u64, u64),
    published: bool,
}
impl IndexLock {
    fn acquire(index: &Path) -> Result<Self> {
        let path = index.with_file_name("index.lock");
        check_path(&path, true)?;
        let file = OpenOptions::new().create_new(true).read(true).write(true).open(&path).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists { PmError::new(ErrorCode::Locked, "Git index.lock already exists; another operation or interrupted preparation owns it").at(&path) } else { PmError::io(&path, error) }
        })?;
        let identity = file_identity(&file.metadata().map_err(|error| PmError::io(&path, error))?);
        let lock = Self {
            path,
            index: index.into(),
            file,
            identity,
            published: false,
        };
        match fs::symlink_metadata(index) {
            Ok(metadata) if metadata.is_file() => lock
                .file
                .set_permissions(metadata.permissions())
                .map_err(|error| PmError::io(&lock.path, error))?,
            Ok(_) => {
                return Err(PmError::new(
                    ErrorCode::UnsafePath,
                    "Git index must be a regular file",
                )
                .at(index));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PmError::io(index, error)),
        }
        Ok(lock)
    }
    fn verify(&self) -> Result<()> {
        let metadata =
            fs::symlink_metadata(&self.path).map_err(|error| PmError::io(&self.path, error))?;
        if !metadata.is_file() || file_identity(&metadata) != self.identity {
            return Err(stale(&self.path, "Git index lock ownership changed"));
        }
        Ok(())
    }
    fn publish(&mut self, bytes: &[u8]) -> Result<()> {
        self.verify()?;
        self.file
            .write_all(bytes)
            .map_err(|error| PmError::io(&self.path, error))?;
        self.file
            .sync_all()
            .map_err(|error| PmError::io(&self.path, error))?;
        self.verify()?;
        fs::rename(&self.path, &self.index).map_err(|error| PmError::io(&self.index, error))?;
        self.published = true;
        File::open(self.index.parent().expect("index parent"))
            .and_then(|directory| directory.sync_all())
            .map_err(|error| PmError::io(&self.index, error))?;
        Ok(())
    }
}
impl Drop for IndexLock {
    fn drop(&mut self) {
        if !self.published && self.verify().is_ok() {
            let _ = fs::remove_file(&self.path);
        }
    }
}

struct AlternateIndex {
    path: PathBuf,
    owned: bool,
}
impl AlternateIndex {
    fn new(index: &Path, original: Option<&[u8]>, git: &Git) -> Result<Self> {
        let path = index.with_file_name(format!(".workdeck-index-{}", OperationId::new()));
        let mut alternate = Self { path, owned: false };
        if let Some(bytes) = original {
            let mut file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&alternate.path)
                .map_err(|error| PmError::io(&alternate.path, error))?;
            alternate.owned = true;
            file.write_all(bytes)
                .map_err(|error| PmError::io(&alternate.path, error))?;
        } else {
            if fs::symlink_metadata(&alternate.path).is_ok() {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "alternate index path already exists",
                )
                .at(&alternate.path));
            }
            git.run(
                &["read-tree".into(), "--empty".into()],
                None,
                Some(&alternate.path),
            )?;
            alternate.owned = true;
        }
        Ok(alternate)
    }
}
impl Drop for AlternateIndex {
    fn drop(&mut self) {
        if self.owned {
            let _ = fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    (metadata.dev(), metadata.ino())
}
#[cfg(not(unix))]
fn file_identity(_: &fs::Metadata) -> (u64, u64) {
    (0, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        RequestId,
        transactions::{FileChange, PreparedOperation},
    };
    use serde_json::json;

    #[test]
    fn published_deletions_remove_only_the_receipted_index_entry() {
        let temp = tempfile::TempDir::new().unwrap();
        let git = Git {
            root: temp.path().canonicalize().unwrap(),
        };
        git.run(&["init".into(), "--quiet".into()], None, None)
            .unwrap();
        let repository = Repository::init(&git.root, "WD").unwrap();
        let path = PathBuf::from("documents/summary.md");
        fs::create_dir_all(repository.root().join("documents")).unwrap();
        fs::write(repository.root().join(&path), "obsolete\n").unwrap();
        git.run(
            &[
                "add".into(),
                "--".into(),
                ".workdeck/documents/summary.md".into(),
            ],
            None,
            None,
        )
        .unwrap();
        git.run(
            &[
                "-c".into(),
                "user.name=PM Test".into(),
                "-c".into(),
                "user.email=pm@example.invalid".into(),
                "commit".into(),
                "--quiet".into(),
                "--no-gpg-sign".into(),
                "-m".into(),
                "document".into(),
            ],
            None,
            None,
        )
        .unwrap();
        let receipt = repository
            .store()
            .unwrap()
            .transact(
                &RequestId::new(),
                "document.delete",
                &json!({"path":path}),
                |_| {
                    Ok(PreparedOperation {
                        changes: vec![FileChange {
                            path,
                            expected: Some(ContentHash::of(b"obsolete\n")),
                            content: None,
                        }],
                        result: json!({"deleted":true}),
                    })
                },
            )
            .unwrap();
        repository.stage_operation(&receipt).unwrap();
        let status = git.text(&["diff", "--cached", "--name-status"]).unwrap();
        assert!(status.contains("D\t.workdeck/documents/summary.md"));
        assert_eq!(status.lines().count(), 2);
        let before = read_regular(&git.index_path().unwrap()).unwrap();
        assert!(!repository.stage_operation(&receipt).unwrap().index_changed);
        assert_eq!(read_regular(&git.index_path().unwrap()).unwrap(), before);
    }
}
