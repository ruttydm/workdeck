use anyhow::{Context, Result, bail};
use chrono::Utc;
use git2::{BranchType, Repository, Status, StatusOptions};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeSet, VecDeque};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
use std::thread;
use std::time::{Duration, Instant};
use workdeck_domain::{
    CheckoutId, CheckoutRecord, RepositoryId, WorktreeAttention, WorktreeId, WorktreeRecord,
};

mod command_policy;
mod history;

pub use command_policy::validate_read_only_git_args;

pub use history::{
    GitChangedFile, GitCommit, GitCommitDetail, GitCommitPathMatches, GitCommitSearch,
    GitGraphEdge, GitGraphRow, GitGraphSnapshot, GitReference, GitReferenceKind,
    GitSearchCancellation, GitStatusSummary, commits_touching_path, load_commit_detail, load_graph,
    load_graph_without_status, search_commits,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepositoryDiscovery {
    pub root: PathBuf,
    pub git_common_dir: PathBuf,
    pub name: String,
    pub remotes: Vec<String>,
    pub provider: Option<ProviderCoordinates>,
    pub worktrees: Vec<DiscoveredWorktree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCoordinates {
    pub provider: String,
    pub owner: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredWorktree {
    pub path: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub locked: bool,
    pub prunable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Typechange,
    Untracked,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotFile {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub kind: SnapshotChangeKind,
    pub staged: bool,
    pub unstaged: bool,
    pub content_hash: Option<String>,
    pub base_content_hash: Option<String>,
    pub binary: bool,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSnapshotManifest {
    pub root: PathBuf,
    pub head: Option<String>,
    pub branch: Option<String>,
    pub index_tree: Option<String>,
    pub captured_at: chrono::DateTime<Utc>,
    pub files: Vec<SnapshotFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttentionScanOptions {
    pub max_concurrency: usize,
    pub per_worktree_timeout: Duration,
    pub overall_timeout: Duration,
    pub max_changes: usize,
}

impl Default for AttentionScanOptions {
    fn default() -> Self {
        Self {
            max_concurrency: 8,
            per_worktree_timeout: Duration::from_secs(8),
            overall_timeout: Duration::from_secs(30),
            max_changes: 100_000,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttentionScanEvent {
    Started {
        total: usize,
        concurrency: usize,
    },
    Updated(WorktreeAttention),
    Progress {
        completed: usize,
        total: usize,
        dirty: usize,
        errors: usize,
    },
    Finished {
        completed: usize,
        total: usize,
        dirty: usize,
        errors: usize,
        cancelled: bool,
        duration_ms: u64,
    },
}

#[derive(Debug, Clone)]
pub struct AttentionScanCancellation {
    cancelled: Arc<AtomicBool>,
}

impl AttentionScanCancellation {
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub struct AttentionScan {
    pub receiver: Receiver<AttentionScanEvent>,
    pub cancellation: AttentionScanCancellation,
}

/// Starts a bounded, incremental repository attention scan.
///
/// The scan invokes Git with optional locks disabled, counts porcelain records,
/// and reads the local commit graph relative to the repository's default base.
/// It never fetches, computes patches, or reads working files. Results arrive
/// one worktree at a time so callers can keep the last persisted result visible.
pub fn start_attention_scan(
    worktrees: Vec<WorktreeRecord>,
    options: AttentionScanOptions,
) -> AttentionScan {
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = AttentionScanCancellation {
        cancelled: Arc::clone(&cancelled),
    };
    let (events, receiver) = mpsc::channel();
    thread::Builder::new()
        .name("workdeck-inbox-coordinator".into())
        .spawn(move || {
            let started = Instant::now();
            let worktrees = worktrees
                .into_iter()
                .filter(|worktree| worktree.available)
                .collect::<Vec<_>>();
            let total = worktrees.len();
            let concurrency = options.max_concurrency.clamp(1, total.max(1));
            if events
                .send(AttentionScanEvent::Started { total, concurrency })
                .is_err()
            {
                cancelled.store(true, Ordering::Release);
                return;
            }

            let queue = Arc::new(Mutex::new(VecDeque::from(worktrees)));
            let (results, incoming) = mpsc::channel();
            let mut workers = Vec::with_capacity(concurrency);
            for index in 0..concurrency {
                let queue = Arc::clone(&queue);
                let results = results.clone();
                let cancelled = Arc::clone(&cancelled);
                let worker_options = options;
                if let Ok(worker) = thread::Builder::new()
                    .name(format!("workdeck-inbox-{index}"))
                    .spawn(move || {
                        loop {
                            if cancelled.load(Ordering::Acquire) {
                                break;
                            }
                            let worktree =
                                queue.lock().ok().and_then(|mut queue| queue.pop_front());
                            let Some(worktree) = worktree else {
                                break;
                            };
                            let attention =
                                scan_attention_worktree(&worktree, worker_options, &cancelled);
                            let Some(attention) = attention else {
                                break;
                            };
                            if results.send(attention).is_err() {
                                break;
                            }
                        }
                    })
                {
                    workers.push(worker);
                }
            }
            drop(results);

            let mut completed = 0;
            let mut dirty = 0;
            let mut errors = 0;
            while completed < total {
                if started.elapsed() >= options.overall_timeout {
                    cancelled.store(true, Ordering::Release);
                }
                match incoming.recv_timeout(Duration::from_millis(25)) {
                    Ok(attention) => {
                        completed += 1;
                        dirty += usize::from(attention.is_dirty());
                        errors += usize::from(attention.error.is_some());
                        let updated_sent =
                            events.send(AttentionScanEvent::Updated(attention)).is_ok();
                        let progress_sent = updated_sent
                            && events
                                .send(AttentionScanEvent::Progress {
                                    completed,
                                    total,
                                    dirty,
                                    errors,
                                })
                                .is_ok();
                        if !progress_sent {
                            cancelled.store(true, Ordering::Release);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if workers.iter().all(thread::JoinHandle::is_finished) {
                            break;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
                if cancelled.load(Ordering::Acquire)
                    && workers.iter().all(thread::JoinHandle::is_finished)
                {
                    break;
                }
            }
            cancelled.store(true, Ordering::Release);
            for worker in workers {
                let _ = worker.join();
            }
            let _ = events.send(AttentionScanEvent::Finished {
                completed,
                total,
                dirty,
                errors,
                cancelled: completed < total,
                duration_ms: elapsed_millis(started),
            });
        })
        .expect("failed to start Workdeck inbox coordinator");
    AttentionScan {
        receiver,
        cancellation,
    }
}

fn scan_attention_worktree(
    worktree: &WorktreeRecord,
    options: AttentionScanOptions,
    cancelled: &AtomicBool,
) -> Option<WorktreeAttention> {
    let started = Instant::now();
    let result = count_porcelain_changes(
        &worktree.path,
        options.max_changes.max(1),
        options.per_worktree_timeout,
        cancelled,
    );
    match result {
        Ok((change_count, truncated, _status_fingerprint)) => {
            let (commit_count, base_ref, live_head) = commit_range_attention(&worktree.path);
            let mut fingerprint = Sha256::new();
            // Updates is commit/PR based. Working-tree edits remain visible as
            // WIP in Git, but they must not reopen an unread commit branch.
            fingerprint.update(commit_count.to_le_bytes());
            if let Some(base_ref) = &base_ref {
                fingerprint.update(base_ref.as_bytes());
            }
            if let Some(head) = live_head.as_ref().or(worktree.head.as_ref()) {
                fingerprint.update([0]);
                fingerprint.update(head.as_bytes());
            }
            Some(WorktreeAttention {
                worktree_id: worktree.id.clone(),
                change_count,
                commit_count,
                base_ref,
                fingerprint: format!("{:x}", fingerprint.finalize()),
                truncated,
                error: None,
                scanned_at: Utc::now(),
                duration_ms: elapsed_millis(started),
            })
        }
        Err(AttentionScanFailure::Cancelled) => None,
        Err(AttentionScanFailure::Failed(error)) => Some(WorktreeAttention {
            worktree_id: worktree.id.clone(),
            change_count: 0,
            commit_count: 0,
            base_ref: None,
            fingerprint: String::new(),
            truncated: false,
            error: Some(error),
            scanned_at: Utc::now(),
            duration_ms: elapsed_millis(started),
        }),
    }
}

fn commit_range_attention(root: &Path) -> (usize, Option<String>, Option<String>) {
    let Ok(repository) = Repository::open(root).or_else(|_| Repository::discover(root)) else {
        return (0, None, None);
    };
    let Ok(head) = repository.head().and_then(|head| head.peel_to_commit()) else {
        return (0, None, None);
    };
    let head_oid = Some(head.id().to_string());
    let current_branch = repository
        .head()
        .ok()
        .and_then(|head| head.shorthand().ok().map(str::to_string));
    let mut candidates = Vec::new();
    if let Ok(origin_head) = repository.find_reference("refs/remotes/origin/HEAD")
        && let Ok(Some(target)) = origin_head.symbolic_target()
    {
        candidates.push(target.trim_start_matches("refs/remotes/").to_string());
    }
    candidates.extend(
        ["origin/main", "origin/master", "main", "master"]
            .into_iter()
            .map(str::to_string),
    );
    if let Ok(branches) = repository.branches(Some(BranchType::Local)) {
        candidates.extend(
            branches
                .filter_map(Result::ok)
                .filter_map(|(branch, _)| branch.name().ok().flatten().map(str::to_string))
                .take(256),
        );
    }
    let mut seen = BTreeSet::new();
    let mut best: Option<(usize, String)> = None;
    for candidate in candidates {
        if current_branch.as_deref() == Some(candidate.as_str()) || !seen.insert(candidate.clone())
        {
            continue;
        }
        let Ok(base) = repository
            .revparse_single(&candidate)
            .and_then(|object| object.peel_to_commit())
        else {
            continue;
        };
        if base.id() == head.id()
            || !repository
                .graph_descendant_of(head.id(), base.id())
                .unwrap_or(false)
        {
            continue;
        }
        if let Ok((ahead, _)) = repository.graph_ahead_behind(head.id(), base.id()) {
            let replace = best
                .as_ref()
                .is_none_or(|(best_ahead, _)| ahead < *best_ahead);
            if replace {
                best = Some((ahead, candidate));
            }
        }
    }
    best.map_or((0, None, head_oid.clone()), |(ahead, base)| {
        (ahead, Some(base), head_oid)
    })
}

#[derive(Debug)]
enum AttentionScanFailure {
    Cancelled,
    Failed(String),
}

fn count_porcelain_changes(
    root: &Path,
    max_changes: usize,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> std::result::Result<(usize, bool, String), AttentionScanFailure> {
    count_porcelain_changes_with_command(Path::new("git"), root, max_changes, timeout, cancelled)
}

fn count_porcelain_changes_with_command(
    git_command: &Path,
    root: &Path,
    max_changes: usize,
    timeout: Duration,
    cancelled: &AtomicBool,
) -> std::result::Result<(usize, bool, String), AttentionScanFailure> {
    if cancelled.load(Ordering::Acquire) {
        return Err(AttentionScanFailure::Cancelled);
    }
    let mut child = Command::new(git_command)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(root)
        .args([
            "-c",
            "color.ui=never",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignore-submodules=all",
            "--no-renames",
            "--no-ahead-behind",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| {
            AttentionScanFailure::Failed(format!("failed to inspect {}: {error}", root.display()))
        })?;
    let stdout = child.stdout.take().expect("piped Git stdout");
    let stderr = child.stderr.take().expect("piped Git stderr");
    let fingerprint_root = root.to_path_buf();
    let stdout_reader =
        thread::spawn(move || count_nul_records(stdout, max_changes, &fingerprint_root));
    let stderr_reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stderr.take(8 * 1024).read_to_end(&mut bytes);
        bytes
    });
    let started = Instant::now();
    let status = loop {
        if cancelled.load(Ordering::Acquire) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(AttentionScanFailure::Cancelled);
        }
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(AttentionScanFailure::Failed(format!(
                "scan timed out after {} ms",
                timeout.as_millis()
            )));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(AttentionScanFailure::Failed(format!(
                    "failed while inspecting {}: {error}",
                    root.display()
                )));
            }
        }
    };
    let (count, truncated, fingerprint) = stdout_reader
        .join()
        .map_err(|_| AttentionScanFailure::Failed("Git status reader panicked".into()))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| AttentionScanFailure::Failed("Git error reader panicked".into()))?;
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
        return Err(AttentionScanFailure::Failed(format!(
            "Git status failed: {}",
            detail.trim()
        )));
    }
    Ok((count, truncated, fingerprint))
}

fn count_nul_records(
    mut reader: impl Read,
    max_changes: usize,
    root: &Path,
) -> (usize, bool, String) {
    let mut buffer = [0_u8; 16 * 1024];
    let mut count = 0;
    let mut truncated = false;
    let mut fingerprint = Sha256::new();
    let mut record = Vec::new();
    while let Ok(read) = reader.read(&mut buffer) {
        if read == 0 {
            break;
        }
        fingerprint.update(&buffer[..read]);
        for byte in &buffer[..read] {
            if *byte == 0 {
                fingerprint_worktree_entry(root, &record, &mut fingerprint);
                record.clear();
                if count < max_changes {
                    count += 1;
                } else {
                    truncated = true;
                }
            } else {
                record.push(*byte);
            }
        }
    }
    (count, truncated, format!("{:x}", fingerprint.finalize()))
}

fn fingerprint_worktree_entry(root: &Path, record: &[u8], fingerprint: &mut Sha256) {
    let Some(path_bytes) = record.get(3..) else {
        return;
    };
    let path = root.join(String::from_utf8_lossy(path_bytes).as_ref());
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return;
    };
    fingerprint.update(metadata.len().to_le_bytes());
    if let Ok(modified) = metadata.modified()
        && let Ok(elapsed) = modified.duration_since(std::time::UNIX_EPOCH)
    {
        fingerprint.update(elapsed.as_nanos().to_le_bytes());
    }
    if metadata.is_file()
        && metadata.len() <= 64 * 1024
        && let Ok(content) = fs::read(path)
    {
        fingerprint.update(content);
    }
}

fn elapsed_millis(started: Instant) -> u64 {
    started.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}

pub trait ObjectSink {
    fn put(&mut self, bytes: &[u8]) -> Result<String>;
}

pub fn discover(path: &Path) -> Result<RepositoryDiscovery> {
    let repository = Repository::discover(path)
        .with_context(|| format!("{} is not inside a Git repository", path.display()))?;
    let root = repository
        .workdir()
        .context("bare repositories cannot be registered as local checkouts")?
        .to_path_buf();
    let git_common_dir = canonical_or_original(&git_common_dir(&root)?);
    let remotes = normalized_remotes(&root)?;
    let provider = remotes
        .iter()
        .find_map(|remote| provider_coordinates(remote));
    let name = provider
        .as_ref()
        .map(|value| value.name.clone())
        .or_else(|| {
            root.file_name()
                .map(|value| value.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| "repository".to_string());
    let worktrees =
        parse_worktree_porcelain(&git_output(&root, &["worktree", "list", "--porcelain"])?);
    Ok(RepositoryDiscovery {
        root: canonical_or_original(&root),
        git_common_dir,
        name,
        remotes,
        provider,
        worktrees,
    })
}

pub fn records_for_discovery(
    repository_id: &RepositoryId,
    discovery: &RepositoryDiscovery,
) -> (CheckoutRecord, Vec<WorktreeRecord>) {
    let now = Utc::now();
    let checkout_id = CheckoutId::new();
    let checkout = CheckoutRecord {
        id: checkout_id.clone(),
        repository_id: repository_id.clone(),
        path: discovery.root.clone(),
        git_common_dir: discovery.git_common_dir.clone(),
        available: true,
        last_seen_at: now,
    };
    let worktrees = discovery
        .worktrees
        .iter()
        .map(|worktree| WorktreeRecord {
            id: WorktreeId::new(),
            checkout_id: checkout_id.clone(),
            path: canonical_or_original(&worktree.path),
            head: worktree.head.clone(),
            branch: worktree.branch.clone(),
            locked: worktree.locked,
            prunable: worktree.prunable,
            available: worktree.path.exists(),
            last_seen_at: now,
        })
        .collect();
    (checkout, worktrees)
}

pub fn capture_worktree(
    root: &Path,
    sink: &mut dyn ObjectSink,
) -> Result<WorktreeSnapshotManifest> {
    let repository = Repository::open(root)
        .with_context(|| format!("failed to open Git repository {}", root.display()))?;
    let mut options = StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .renames_head_to_index(true)
        .renames_index_to_workdir(true);
    let statuses = repository.statuses(Some(&mut options))?;
    let head = repository
        .head()
        .ok()
        .and_then(|head| head.target())
        .map(|oid| oid.to_string());
    let branch = repository
        .head()
        .ok()
        .and_then(|head| head.shorthand().ok().map(str::to_string));
    // Fingerprint index entries directly. Creating a Git tree would write an object,
    // while checkpoint capture is deliberately read-only with respect to the repo.
    let index_tree = repository.index().ok().map(|index| {
        let mut digest = Sha256::new();
        for entry in index.iter() {
            digest.update(entry.id.as_bytes());
            digest.update(entry.mode.to_le_bytes());
            digest.update(entry.flags.to_le_bytes());
            digest.update(&entry.path);
        }
        format!("index:{:x}", digest.finalize())
    });
    let mut files = Vec::new();
    for entry in statuses.iter() {
        let path = PathBuf::from(entry.path()?);
        let status = entry.status();
        let kind = snapshot_change_kind(status);
        let absolute = root.join(&path);
        let (content_hash, binary, size) = if absolute.is_file() {
            let before = fs::metadata(&absolute)?;
            let bytes = fs::read(&absolute)
                .with_context(|| format!("failed to snapshot {}", absolute.display()))?;
            let after = fs::metadata(&absolute)?;
            if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
                bail!(
                    "{} changed while its snapshot was being captured",
                    path.display()
                );
            }
            let hash = sink.put(&bytes)?;
            (Some(hash), looks_binary(&bytes), bytes.len() as u64)
        } else {
            (None, false, 0)
        };
        let base_bytes = head
            .as_deref()
            .and_then(|revision| read_blob_at_revision(root, revision, &path).ok())
            .flatten();
        let base_content_hash = match base_bytes {
            Some(bytes) => Some(sink.put(&bytes)?),
            None => None,
        };
        files.push(SnapshotFile {
            path,
            old_path: None,
            kind,
            staged: is_staged(status),
            unstaged: is_unstaged(status),
            content_hash,
            base_content_hash,
            binary,
            size,
        });
    }
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(WorktreeSnapshotManifest {
        root: canonical_or_original(root),
        head,
        branch,
        index_tree,
        captured_at: Utc::now(),
        files,
    })
}

pub fn worktree_manifest_revision(manifest: &WorktreeSnapshotManifest) -> String {
    let mut digest = Sha256::new();
    digest.update(manifest.head.as_deref().unwrap_or("unborn").as_bytes());
    digest.update(manifest.branch.as_deref().unwrap_or("detached").as_bytes());
    digest.update(
        manifest
            .index_tree
            .as_deref()
            .unwrap_or("no-index")
            .as_bytes(),
    );
    for file in &manifest.files {
        digest.update(file.path.to_string_lossy().as_bytes());
        digest.update(format!("{:?}", file.kind).as_bytes());
        digest.update([u8::from(file.staged), u8::from(file.unstaged)]);
        digest.update(file.content_hash.as_deref().unwrap_or("deleted").as_bytes());
        digest.update(
            file.base_content_hash
                .as_deref()
                .unwrap_or("no-base")
                .as_bytes(),
        );
    }
    format!(
        "worktree:{}:{:x}",
        manifest.head.as_deref().unwrap_or("unborn"),
        digest.finalize()
    )
}

pub fn live_worktree_revision(root: &Path) -> Result<String> {
    #[derive(Default)]
    struct HashSink;
    impl ObjectSink for HashSink {
        fn put(&mut self, bytes: &[u8]) -> Result<String> {
            Ok(format!("{:x}", Sha256::digest(bytes)))
        }
    }
    let manifest = capture_worktree(root, &mut HashSink)?;
    Ok(worktree_manifest_revision(&manifest))
}

pub fn read_blob_at_revision(root: &Path, revision: &str, path: &Path) -> Result<Option<Vec<u8>>> {
    let repository = Repository::open(root)?;
    let object = match repository.revparse_single(revision) {
        Ok(object) => object,
        Err(_) => return Ok(None),
    };
    let commit = object.peel_to_commit()?;
    let tree = commit.tree()?;
    let entry = match tree.get_path(path) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    let blob = match repository.find_blob(entry.id()) {
        Ok(blob) => blob,
        Err(_) => return Ok(None),
    };
    Ok(Some(blob.content().to_vec()))
}

pub fn resolve_revision(root: &Path, revision: &str) -> Result<String> {
    let repository = Repository::open(root)
        .with_context(|| format!("failed to open Git repository {}", root.display()))?;
    let commit = repository
        .revparse_single(revision)
        .with_context(|| format!("revision {revision} does not exist"))?
        .peel_to_commit()
        .with_context(|| format!("revision {revision} does not resolve to a commit"))?;
    Ok(commit.id().to_string())
}

pub fn changed_paths_for_range(root: &Path, base: &str, head: &str) -> Result<Vec<PathBuf>> {
    let output = git_output(
        root,
        &["diff", "--name-only", "--no-renames", base, head, "--"],
    )?;
    Ok(output
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(PathBuf::from)
        .collect())
}

pub fn diff_for_range(root: &Path, base: &str, head: &str, path: Option<&Path>) -> Result<String> {
    let mut args = vec!["diff", "--no-ext-diff", "--color=never", base, head];
    let path_string;
    if let Some(path) = path {
        path_string = path.to_string_lossy().to_string();
        args.push("--");
        args.push(&path_string);
    }
    git_output(root, &args)
}

pub fn normalize_remote(value: &str) -> Option<String> {
    let value = value.trim().trim_end_matches('/').trim_end_matches(".git");
    if value.is_empty() {
        return None;
    }
    let without_scheme = value
        .strip_prefix("https://")
        .or_else(|| value.strip_prefix("http://"))
        .or_else(|| value.strip_prefix("ssh://"))
        .unwrap_or(value);
    let normalized = if let Some(path) = without_scheme.strip_prefix("git@") {
        path.replacen(':', "/", 1)
    } else if without_scheme.contains('@') && without_scheme.contains(':') {
        let (_, rest) = without_scheme.rsplit_once('@')?;
        rest.replacen(':', "/", 1)
    } else {
        without_scheme.to_string()
    };
    Some(normalized.trim_start_matches('/').to_ascii_lowercase())
}

pub fn provider_coordinates(remote: &str) -> Option<ProviderCoordinates> {
    let mut parts = remote.split('/');
    let host = parts.next()?;
    let owner = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let provider = match host {
        "github.com" => "github",
        "gitlab.com" => "gitlab",
        "bitbucket.org" => "bitbucket",
        _ => return None,
    };
    Some(ProviderCoordinates {
        provider: provider.to_string(),
        owner: owner.to_string(),
        name: name.to_string(),
    })
}

pub fn parse_worktree_porcelain(output: &str) -> Vec<DiscoveredWorktree> {
    let mut result = Vec::new();
    let mut current: Option<DiscoveredWorktree> = None;
    for line in output.lines().chain(std::iter::once("")) {
        if line.trim().is_empty() {
            if let Some(worktree) = current.take() {
                result.push(worktree);
            }
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "worktree" => {
                if let Some(worktree) = current.take() {
                    result.push(worktree);
                }
                current = Some(DiscoveredWorktree {
                    path: PathBuf::from(value),
                    head: None,
                    branch: None,
                    locked: false,
                    prunable: false,
                });
            }
            "HEAD" => {
                if let Some(worktree) = current.as_mut() {
                    worktree.head = Some(value.to_string());
                }
            }
            "branch" => {
                if let Some(worktree) = current.as_mut() {
                    worktree.branch = Some(
                        value
                            .strip_prefix("refs/heads/")
                            .unwrap_or(value)
                            .to_string(),
                    );
                }
            }
            "locked" => {
                if let Some(worktree) = current.as_mut() {
                    worktree.locked = true;
                }
            }
            "prunable" => {
                if let Some(worktree) = current.as_mut() {
                    worktree.prunable = true;
                }
            }
            _ => {}
        }
    }
    result
}

fn normalized_remotes(root: &Path) -> Result<Vec<String>> {
    let output = git_output(root, &["remote", "-v"])?;
    let mut remotes = BTreeSet::new();
    for line in output.lines() {
        let mut columns = line.split_whitespace();
        let _name = columns.next();
        if let Some(url) = columns.next().and_then(normalize_remote) {
            remotes.insert(url);
        }
    }
    Ok(remotes.into_iter().collect())
}

fn git_common_dir(root: &Path) -> Result<PathBuf> {
    let output = git_output(root, &["rev-parse", "--git-common-dir"])?;
    let path = PathBuf::from(output.trim());
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    validate_read_only_git_args(args)?;
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn snapshot_change_kind(status: Status) -> SnapshotChangeKind {
    if status.is_conflicted() {
        SnapshotChangeKind::Conflicted
    } else if status.intersects(Status::WT_RENAMED | Status::INDEX_RENAMED) {
        SnapshotChangeKind::Renamed
    } else if status.intersects(Status::WT_DELETED | Status::INDEX_DELETED) {
        SnapshotChangeKind::Deleted
    } else if status.intersects(Status::WT_TYPECHANGE | Status::INDEX_TYPECHANGE) {
        SnapshotChangeKind::Typechange
    } else if status.contains(Status::WT_NEW) {
        SnapshotChangeKind::Untracked
    } else if status.contains(Status::INDEX_NEW) {
        SnapshotChangeKind::Added
    } else {
        SnapshotChangeKind::Modified
    }
}

fn is_staged(status: Status) -> bool {
    status.intersects(
        Status::INDEX_NEW
            | Status::INDEX_MODIFIED
            | Status::INDEX_DELETED
            | Status::INDEX_RENAMED
            | Status::INDEX_TYPECHANGE,
    )
}

fn is_unstaged(status: Status) -> bool {
    status.intersects(
        Status::WT_NEW
            | Status::WT_MODIFIED
            | Status::WT_DELETED
            | Status::WT_RENAMED
            | Status::WT_TYPECHANGE
            | Status::CONFLICTED,
    )
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8_000).any(|byte| *byte == 0)
}

fn canonical_or_original(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn git(root: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn normalizes_remote_forms() {
        assert_eq!(
            normalize_remote("git@github.com:OpenAI/codex.git"),
            Some("github.com/openai/codex".to_string())
        );
        assert_eq!(
            normalize_remote("https://github.com/OpenAI/codex.git"),
            Some("github.com/openai/codex".to_string())
        );
    }

    #[test]
    fn parses_worktree_output() {
        let worktrees = parse_worktree_porcelain(
            "worktree /tmp/main\nHEAD abc\nbranch refs/heads/main\n\nworktree /tmp/feature\nHEAD def\nbranch refs/heads/feature\nlocked reason\n\n",
        );
        assert_eq!(worktrees.len(), 2);
        assert_eq!(worktrees[1].branch.as_deref(), Some("feature"));
        assert!(worktrees[1].locked);
    }

    #[test]
    fn provider_coordinates_require_known_host() {
        assert_eq!(
            provider_coordinates("github.com/openai/codex"),
            Some(ProviderCoordinates {
                provider: "github".to_string(),
                owner: "openai".to_string(),
                name: "codex".to_string(),
            })
        );
        assert!(provider_coordinates("example.test/a/b").is_none());
    }

    #[test]
    fn attention_scan_is_incremental_exact_and_truncatable() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        git(
            root.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        git(root.path(), &["config", "user.name", "Workdeck Tests"]);
        fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-qm", "initial"]);
        fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
        fs::write(root.path().join("one.txt"), "one\n").unwrap();
        fs::write(root.path().join("two.txt"), "two\n").unwrap();

        let worktree = WorktreeRecord {
            id: WorktreeId::new(),
            checkout_id: CheckoutId::new(),
            path: root.path().to_path_buf(),
            head: None,
            branch: Some("main".into()),
            locked: false,
            prunable: false,
            available: true,
            last_seen_at: Utc::now(),
        };
        let scan = start_attention_scan(
            vec![worktree.clone()],
            AttentionScanOptions {
                max_concurrency: 8,
                per_worktree_timeout: Duration::from_secs(2),
                overall_timeout: Duration::from_secs(3),
                max_changes: 2,
            },
        );
        let events = scan.receiver.iter().collect::<Vec<_>>();
        assert_eq!(
            events.first(),
            Some(&AttentionScanEvent::Started {
                total: 1,
                concurrency: 1
            })
        );
        let attention = events
            .iter()
            .find_map(|event| match event {
                AttentionScanEvent::Updated(attention) => Some(attention),
                _ => None,
            })
            .unwrap();
        assert_eq!(attention.worktree_id, worktree.id);
        assert_eq!(attention.change_count, 2);
        assert!(attention.truncated);
        assert_eq!(attention.fingerprint.len(), 64);
        assert!(attention.error.is_none());
        let original_fingerprint = attention.fingerprint.clone();
        fs::write(root.path().join("tracked.txt"), "later\n").unwrap();
        let refreshed = start_attention_scan(
            vec![worktree.clone()],
            AttentionScanOptions {
                max_concurrency: 1,
                per_worktree_timeout: Duration::from_secs(2),
                overall_timeout: Duration::from_secs(3),
                max_changes: 2,
            },
        )
        .receiver
        .iter()
        .find_map(|event| match event {
            AttentionScanEvent::Updated(attention) => Some(attention),
            _ => None,
        })
        .unwrap();
        assert_eq!(refreshed.fingerprint, original_fingerprint);
        git(root.path(), &["add", "tracked.txt", "one.txt", "two.txt"]);
        git(root.path(), &["commit", "-qm", "agent update"]);
        let committed = start_attention_scan(
            vec![worktree],
            AttentionScanOptions {
                max_concurrency: 1,
                per_worktree_timeout: Duration::from_secs(2),
                overall_timeout: Duration::from_secs(3),
                max_changes: 2,
            },
        )
        .receiver
        .iter()
        .find_map(|event| match event {
            AttentionScanEvent::Updated(attention) => Some(attention),
            _ => None,
        })
        .unwrap();
        assert_ne!(committed.fingerprint, original_fingerprint);
        assert!(matches!(
            events.last(),
            Some(AttentionScanEvent::Finished {
                completed: 1,
                total: 1,
                cancelled: false,
                ..
            })
        ));
    }

    #[test]
    fn attention_scan_detects_commits_ahead_of_default_branch() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        git(
            root.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        git(root.path(), &["config", "user.name", "Workdeck Tests"]);
        fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        git(
            root.path(),
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        git(root.path(), &["checkout", "-qb", "feat/review"]);
        fs::write(root.path().join("tracked.txt"), "base\nfeature\n").unwrap();
        git(root.path(), &["commit", "-qam", "feature"]);

        let worktree = WorktreeRecord {
            id: WorktreeId::new(),
            checkout_id: CheckoutId::new(),
            path: root.path().to_path_buf(),
            head: None,
            branch: Some("refs/heads/feat/review".into()),
            locked: false,
            prunable: false,
            available: true,
            last_seen_at: Utc::now(),
        };
        let attention = scan_attention_worktree(
            &worktree,
            AttentionScanOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();

        assert_eq!(attention.change_count, 0);
        assert_eq!(attention.commit_count, 1);
        assert_eq!(attention.base_ref.as_deref(), Some("origin/main"));
        assert!(attention.has_reviewable_work());
    }

    #[test]
    fn attention_scan_prefers_the_nearest_local_ancestor() {
        let root = tempfile::tempdir().unwrap();
        git(root.path(), &["init", "-q"]);
        git(
            root.path(),
            &["config", "user.email", "workdeck@example.test"],
        );
        git(root.path(), &["config", "user.name", "Workdeck Tests"]);
        fs::write(root.path().join("tracked.txt"), "base\n").unwrap();
        git(root.path(), &["add", "tracked.txt"]);
        git(root.path(), &["commit", "-qm", "base"]);
        git(
            root.path(),
            &["update-ref", "refs/remotes/origin/master", "HEAD"],
        );
        git(root.path(), &["checkout", "-qb", "tui"]);
        fs::write(root.path().join("tracked.txt"), "base\ntui\n").unwrap();
        git(root.path(), &["commit", "-qam", "tui base"]);
        git(root.path(), &["checkout", "-qb", "feature/streaming"]);
        fs::write(root.path().join("tracked.txt"), "base\ntui\nfeature\n").unwrap();
        git(root.path(), &["commit", "-qam", "feature"]);

        let (ahead, base, head) = commit_range_attention(root.path());
        assert_eq!((ahead, base), (1, Some("tui".into())));
        assert!(head.is_some());
    }

    #[test]
    fn attention_scan_cancellation_keeps_unstarted_work_out_of_results() {
        let cancellation = Arc::new(AtomicBool::new(true));
        let result = count_porcelain_changes(
            Path::new("/does/not/matter"),
            10,
            Duration::from_secs(1),
            &cancellation,
        );
        assert!(matches!(result, Err(AttentionScanFailure::Cancelled)));
    }

    #[cfg(unix)]
    #[test]
    fn attention_scan_cancels_an_in_flight_git_process() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        let slow_git = fixture.path().join("slow-git");
        fs::write(&slow_git, "#!/bin/sh\nexec sleep 10\n").unwrap();
        fs::set_permissions(&slow_git, fs::Permissions::from_mode(0o755)).unwrap();
        let cancellation = Arc::new(AtomicBool::new(false));
        let cancellation_for_thread = Arc::clone(&cancellation);
        let canceller = thread::spawn(move || {
            thread::sleep(Duration::from_millis(50));
            cancellation_for_thread.store(true, Ordering::Release);
        });
        let started = Instant::now();
        let result = count_porcelain_changes_with_command(
            &slow_git,
            fixture.path(),
            10,
            Duration::from_secs(5),
            &cancellation,
        );
        canceller.join().unwrap();
        assert!(matches!(result, Err(AttentionScanFailure::Cancelled)));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[cfg(unix)]
    #[test]
    fn attention_scan_enforces_the_per_worktree_timeout() {
        use std::os::unix::fs::PermissionsExt as _;

        let fixture = tempfile::tempdir().unwrap();
        let slow_git = fixture.path().join("slow-git");
        fs::write(&slow_git, "#!/bin/sh\nexec sleep 10\n").unwrap();
        fs::set_permissions(&slow_git, fs::Permissions::from_mode(0o755)).unwrap();
        let cancellation = AtomicBool::new(false);
        let started = Instant::now();
        let result = count_porcelain_changes_with_command(
            &slow_git,
            fixture.path(),
            10,
            Duration::from_millis(50),
            &cancellation,
        );
        assert!(matches!(result, Err(AttentionScanFailure::Failed(_))));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
