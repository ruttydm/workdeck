use super::{
    git::{BoundGit, CapturedIndex},
    *,
};
use crate::{Config, ContentHash, ErrorCode, PmError, Repository, Result, transactions::Snapshot};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
};

#[derive(Debug, Clone)]
pub(super) struct CaptureGuard {
    git: Option<BoundGit>,
    local_root: Option<(PathBuf, super::fs::Identity)>,
    head: Option<GitOid>,
    head_ref: Option<GitRefName>,
    reference: Option<(GitRefName, GitOid)>,
    index: Option<CapturedIndex>,
    working: Option<BTreeMap<PathBuf, Vec<u8>>>,
    working_stamps: Option<BTreeMap<PathBuf, super::fs::Stamp>>,
    working_hashes: Option<BTreeMap<PathBuf, ContentHash>>,
    configuration: Option<(PathBuf, ContentHash)>,
    remote_binding: Option<(crate::RepositoryId, SharedSources, ContentHash)>,
    limits: SourceCaptureLimits,
}
impl CaptureGuard {
    pub(super) fn publication_binding(&self) -> Option<&ContentHash> {
        self.remote_binding.as_ref().map(|(_, _, binding)| binding)
    }
    pub(super) fn verify_before(&self, deadline: std::time::Instant) -> Result<()> {
        if std::time::Instant::now() >= deadline {
            return Err(stale());
        }
        self.verify_with(
            self.git
                .as_ref()
                .map(|git| git.for_revalidation_before(deadline)),
        )?;
        if std::time::Instant::now() >= deadline {
            return Err(stale());
        }
        Ok(())
    }
    pub(super) fn verify(&self) -> Result<()> {
        self.verify_with(self.git.as_ref().map(BoundGit::for_revalidation))
    }
    fn verify_initial(&self) -> Result<()> {
        self.verify_with(self.git.clone())
    }
    fn verify_with(&self, git: Option<BoundGit>) -> Result<()> {
        let check = || -> Result<()> {
            if let Some(git) = &git {
                git.verify()?;
                if let Some((repository, shared, expected)) = &self.remote_binding {
                    let url = git.remote_url(&shared.remote)?;
                    if git.binding(repository, shared, &url)? != *expected {
                        return Err(stale());
                    }
                }
                if git.head()? != self.head || git.head_ref()? != self.head_ref {
                    return Err(stale());
                }
                if let Some((reference, oid)) = &self.reference
                    && git.resolve(reference.as_str())?.as_ref() != Some(oid)
                {
                    return Err(stale());
                }
                if let Some(index) = &self.index {
                    git.verify_index(index)?;
                }
            }
            if let Some((root, identity)) = &self.local_root
                && super::fs::directory(root)? != *identity
            {
                return Err(stale());
            }
            if let Some((path, expected)) = &self.configuration
                && ContentHash::of(&super::fs::read(path, 64 * 1024 * 1024)?.0) != *expected
            {
                return Err(stale());
            }
            if let Some(expected) = &self.working {
                let root = git
                    .as_ref()
                    .map(|git| git.root())
                    .or_else(|| self.local_root.as_ref().map(|(root, _)| root.as_path()))
                    .ok_or_else(stale)?;
                let repository = Repository::open_source(&root.join(".workdeck"))?;
                let source_root = repository.root();
                if let Some(stamps) = &self.working_stamps {
                    if !source_stamps_match(source_root, stamps, &self.limits)? {
                        return Err(stale());
                    }
                } else {
                    let current = repository
                        .store()?
                        .with_snapshot(|snapshot| {
                            files_from_snapshot(snapshot, &self.limits, true)
                        })?
                        .files;
                    if &current != expected {
                        return Err(stale());
                    }
                }
            }
            Ok(())
        };
        check().map_err(|error| {
            if error.code == ErrorCode::StaleSource {
                error
            } else {
                stale()
            }
        })
    }
}

#[derive(Debug, Clone)]
struct CapturedFiles {
    files: BTreeMap<PathBuf, Vec<u8>>,
    stamps: BTreeMap<PathBuf, super::fs::Stamp>,
    hashes: BTreeMap<PathBuf, ContentHash>,
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn stale() -> PmError {
    PmError::new(
        ErrorCode::StaleSource,
        "captured planning source changed; refresh without replacing retained intent",
    )
}

/// Revalidate a local working source with directory membership and descriptor
/// metadata before falling back to a full content capture. Unix change time is
/// included in each stamp, so ordinary same-size editor writes remain visible
/// even when modification time is preserved.
fn source_stamps_match(
    root: &Path,
    expected: &BTreeMap<PathBuf, super::fs::Stamp>,
    limits: &SourceCaptureLimits,
) -> Result<bool> {
    Ok(current_source_stamps(root, limits)? == *expected)
}

fn current_source_stamps(
    root: &Path,
    limits: &SourceCaptureLimits,
) -> Result<BTreeMap<PathBuf, super::fs::Stamp>> {
    let repository = Repository::open_source(root)?;
    let paths = repository.store()?.with_snapshot(|snapshot| {
        let mut paths = Vec::new();
        for path in snapshot.list_bounded(Path::new(""), limits.max_entries)? {
            if authoritative(&path)? {
                paths.push(path);
            }
        }
        Ok(paths)
    })?;
    let root_file = super::fs::open(root, true)?;
    let mut open_cache = super::fs::RelativeOpenCache::default();
    let mut stamps = BTreeMap::new();
    for path in paths {
        stamps.insert(
            path.clone(),
            super::fs::stamp_relative_cached(&mut open_cache, &root_file, root, &path)?,
        );
    }
    Ok(stamps)
}

/// Reuse a local working source when directory membership and per-file stamps
/// are unchanged, or rebuild only the files whose stamps changed. A new file,
/// deletion, source-mode change, or repository identity change deliberately
/// returns `None` so the complete capture path retains its stronger admission
/// checks.
pub(crate) fn capture_local_delta(
    worktree: &Path,
    previous: &PlanningSourceView,
    limits: &SourceCaptureLimits,
    deadline: std::time::Instant,
) -> Result<Option<PlanningSourceView>> {
    if previous.snapshot.identity().role != SourceRole::Local {
        return Ok(None);
    }
    let Some(previous_stamps) = previous.guard.working_stamps.as_ref() else {
        return Ok(None);
    };
    let Some(previous_hashes) = previous.guard.working_hashes.as_ref() else {
        return Ok(None);
    };
    if std::time::Instant::now() >= deadline {
        return Err(stale());
    }
    let worktree = worktree
        .canonicalize()
        .map_err(|error| PmError::io(worktree, error))?;
    if let Some((root, identity)) = &previous.guard.local_root
        && (root != &worktree || super::fs::directory(&worktree)? != *identity)
    {
        return Ok(None);
    }
    if let Some(git) = &previous.guard.git
        && (git.verify().is_err()
            || git.head()? != previous.guard.head
            || git.head_ref()? != previous.guard.head_ref)
    {
        return Ok(None);
    }
    let repository = Repository::open_source(&worktree.join(".workdeck"))?;
    let root = repository.root();
    if repository.config()?.sources.is_some() {
        return Ok(None);
    }
    let current_stamps = current_source_stamps(root, limits)?;
    if current_stamps.keys().ne(previous_stamps.keys()) {
        return Ok(None);
    }
    let mut files = previous.snapshot.files.clone();
    let mut hashes = previous_hashes.clone();
    let root_file = super::fs::open(root, true)?;
    for (path, current_stamp) in &current_stamps {
        if previous_stamps.get(path) == Some(current_stamp) {
            continue;
        }
        if std::time::Instant::now() >= deadline {
            return Err(stale());
        }
        let (bytes, read_stamp) =
            super::fs::read_relative(&root_file, root, path, limits.max_file_bytes)?;
        if &read_stamp != current_stamp {
            return Err(stale());
        }
        hashes.insert(path.clone(), ContentHash::of(&bytes));
        files.insert(path.clone(), bytes);
    }
    let memory = Snapshot::from_memory(Path::new("source-snapshot"), &files);
    let config = crate::repository::config_from_snapshot(Path::new("source-snapshot"), &memory)?;
    if config.sources.is_some() || config.repository != previous.snapshot.identity().repository {
        return Ok(None);
    }
    validate_role(&files, &config, SourceRole::Local, None)?;
    let identity = PlanningSourceIdentity {
        repository: config.repository,
        role: SourceRole::Local,
        ref_name: None,
        commit: None,
        tree: None,
        index_content: None,
        content: content_hash_from_hashes(&files, &hashes)?,
    };
    let entries = files
        .keys()
        .map(|path| SourceEntry {
            path: path.clone(),
            mode: "100644".into(),
            oid: None,
            stage: 0,
        })
        .collect();
    let mut guard = previous.guard.clone();
    guard.working = Some(files.clone());
    guard.working_stamps = Some(current_stamps);
    guard.working_hashes = Some(hashes);
    Ok(Some(PlanningSourceView {
        observation: SourceObservation {
            identity: identity.clone(),
            observed_at: chrono::Utc::now(),
            remote_observation: None,
            freshness: SourceFreshness::CurrentAtObservation,
            reason_codes: vec![
                "local_source_only".into(),
                "incremental_metadata_capture".into(),
            ],
        },
        snapshot: SourceSnapshot {
            identity,
            files,
            entries,
        },
        guard,
    }))
}

pub(super) fn authoritative(path: &Path) -> Result<bool> {
    let first = path
        .components()
        .next()
        .and_then(|p| p.as_os_str().to_str())
        .unwrap_or("");
    if matches!(
        first,
        ".local"
            | ".tmp"
            | ".index"
            | "config.local.yml"
            | "config.local.toml"
            | "settings.local.yml"
    ) {
        return Ok(false);
    }
    Ok(crate::snapshots::validation::classify(path)?.is_some())
}
fn files_from_snapshot(
    snapshot: &Snapshot<'_>,
    limits: &SourceCaptureLimits,
    track_reads: bool,
) -> Result<CapturedFiles> {
    let mut paths = Vec::new();
    for path in snapshot.list_bounded(Path::new(""), limits.max_entries)? {
        if authoritative(&path)? {
            paths.push(path);
        }
    }
    if paths.is_empty() {
        return Ok(CapturedFiles {
            files: BTreeMap::new(),
            stamps: BTreeMap::new(),
            hashes: BTreeMap::new(),
        });
    }

    // The source lock and listing remain owned by the caller, while independent
    // descriptor-bound file reads can proceed concurrently. Cap workers by the
    // worst-case per-file reservation so a hostile set of maximum-sized files
    // cannot make the fan-out exceed the complete capture budget.
    let worker_cap = limits
        .max_total_bytes
        .checked_div(limits.max_file_bytes.max(1))
        .unwrap_or(1)
        .clamp(1, 16);
    let workers = thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(worker_cap)
        .min(paths.len());
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();
    let root = snapshot.root().to_owned();
    let root_file = super::fs::open(&root, true)?;
    let next = &next;
    let paths = &paths;
    let root = &root;
    let max_file_bytes = limits.max_file_bytes;
    thread::scope(|scope| {
        for _ in 0..workers {
            let sender = sender.clone();
            let root_file = root_file
                .try_clone()
                .map_err(|error| PmError::io(root, error))?;
            scope.spawn(move || {
                let mut open_cache = super::fs::RelativeOpenCache::default();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    if index >= paths.len() {
                        break;
                    }
                    let path = &paths[index];
                    let result = super::fs::read_relative_cached(
                        &mut open_cache,
                        &root_file,
                        root,
                        path,
                        max_file_bytes,
                    );
                    // The receiver lives until every scoped worker exits, so a
                    // send failure can only indicate an internal programming error.
                    sender
                        .send((index, result))
                        .expect("source capture result receiver remains available");
                }
            });
        }
        drop(sender);
        let mut results = (0..paths.len())
            .map(|_| None)
            .collect::<Vec<Option<Result<(Vec<u8>, super::fs::Stamp)>>>>();
        for (index, result) in receiver {
            results[index] = Some(result);
        }

        let mut files = BTreeMap::new();
        let mut stamps = BTreeMap::new();
        let mut hashes = BTreeMap::new();
        let mut total = 0usize;
        for (path, result) in paths.iter().zip(results) {
            let (bytes, stamp) = result.expect("every source capture path produces one result")?;
            if bytes.len() > limits.max_file_bytes {
                return Err(invalid("planning source file exceeds its capture bound").at(path));
            }
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("planning source capture size overflow"))?;
            if total > limits.max_total_bytes {
                return Err(invalid("planning source exceeds its total capture bound").at(path));
            }
            if track_reads {
                snapshot.record_read(path, Some(bytes.clone()), limits.max_file_bytes);
            }
            stamps.insert(path.clone(), stamp);
            hashes.insert(path.clone(), ContentHash::of(&bytes));
            files.insert(path.clone(), bytes);
        }
        Ok(CapturedFiles {
            files,
            stamps,
            hashes,
        })
    })
}
pub(super) fn content_hash(files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<ContentHash> {
    let hashes = files
        .iter()
        .map(|(path, bytes)| (path.clone(), ContentHash::of(bytes)))
        .collect();
    content_hash_from_hashes(files, &hashes)
}

fn content_hash_from_hashes(
    files: &BTreeMap<PathBuf, Vec<u8>>,
    hashes: &BTreeMap<PathBuf, ContentHash>,
) -> Result<ContentHash> {
    if files.len() != hashes.len() || files.keys().any(|path| !hashes.contains_key(path)) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "source content-hash entries do not match captured files",
        ));
    }
    // Keep the canonical_hash schema and key order stable, but avoid building
    // one serde_json::Value per file. Path/content/length tuple encoding is
    // identical to the previous JSON representation.
    let mut bytes = Vec::with_capacity(files.len().saturating_mul(128).saturating_add(24));
    bytes.extend_from_slice(br#"{"files":["#);
    for (index, (path, content)) in files.iter().enumerate() {
        if index != 0 {
            bytes.push(b',');
        }
        bytes.push(b'[');
        serde_json::to_writer(&mut bytes, path)
            .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?;
        bytes.push(b',');
        serde_json::to_writer(&mut bytes, &hashes[path])
            .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?;
        bytes.push(b',');
        serde_json::to_writer(&mut bytes, &content.len())
            .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?;
        bytes.push(b']');
    }
    bytes.extend_from_slice(br#"],"schema":1}"#);
    Ok(ContentHash::of(&bytes))
}
pub(crate) fn identity_from_snapshot(
    _root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    role: SourceRole,
) -> Result<PlanningSourceIdentity> {
    let files = files_from_snapshot(snapshot, &SourceCaptureLimits::default(), true)?.files;
    Ok(PlanningSourceIdentity {
        repository: config.repository.clone(),
        role,
        ref_name: None,
        commit: None,
        tree: None,
        index_content: None,
        content: content_hash(&files)?,
    })
}
pub(super) fn capture(
    worktree: &Path,
    selector: &SourceSelector,
    limits: &SourceCaptureLimits,
    deadline: std::time::Instant,
) -> Result<PlanningSourceView> {
    limits.validate()?;
    if std::time::Instant::now() >= deadline {
        return Err(PmError::new(
            ErrorCode::Io,
            "Git source operation exceeded its total timeout",
        ));
    }
    // Local planning does not need Git discovery or ambient Git configuration.
    // It also remains valid inside a larger repository's subdirectory.
    if matches!(selector, SourceSelector::WorkingTree) {
        let repository = Repository::open_source(&worktree.join(".workdeck"))?;
        if repository.config()?.sources.is_none() {
            return capture_local(worktree, limits);
        }
    }
    let git = BoundGit::with_deadline(
        worktree,
        limits,
        deadline,
        !matches!(selector, SourceSelector::Staged { .. }),
    )?;
    let head = git.head()?;
    let head_ref = git.head_ref()?;
    let mut guard = CaptureGuard {
        git: Some(git.clone()),
        local_root: None,
        head: head.clone(),
        head_ref: head_ref.clone(),
        reference: None,
        index: None,
        working: None,
        working_stamps: None,
        working_hashes: None,
        configuration: None,
        remote_binding: None,
        limits: limits.clone(),
    };
    let mut remote_observation = None;
    let mut expected_repository = None;
    let mut commit = None;
    let mut tree = None;
    let mut ref_name = None;
    let mut index_content = None;
    let mut binding_unavailable = None;
    let (role, files, entries, mut freshness, mut reasons) = match selector {
        SourceSelector::WorkingTree => {
            let repository = Repository::open_source(&git.root().join(".workdeck"))?;
            let captured = repository
                .store()?
                .with_snapshot(|snapshot| files_from_snapshot(snapshot, limits, false))?;
            let CapturedFiles {
                files,
                stamps,
                hashes,
            } = captured;
            let snapshot = Snapshot::from_memory(Path::new("source-snapshot"), &files);
            let config =
                crate::repository::config_from_snapshot(Path::new("source-snapshot"), &snapshot)?;
            if let Some(shared) = &config.sources {
                match read_binding(&git, &config, shared) {
                    Ok(binding) => guard.remote_binding = Some(binding),
                    Err(error) => binding_unavailable = Some(error.code),
                }
            }
            let role = if config.sources.is_some() {
                ref_name = head_ref;
                commit = head;
                SourceRole::Proposal
            } else {
                SourceRole::Local
            };
            let entries = files
                .keys()
                .map(|path| SourceEntry {
                    path: path.clone(),
                    mode: "100644".into(),
                    oid: None,
                    stage: 0,
                })
                .collect();
            guard.working = Some(files.clone());
            guard.working_stamps = Some(stamps);
            guard.working_hashes = Some(hashes);
            (
                role,
                files,
                entries,
                SourceFreshness::CurrentAtObservation,
                vec!["local_source_only".into()],
            )
        }
        SourceSelector::Staged { index } => {
            let captured = git.capture_index(index)?;
            index_content = Some(ContentHash::of(captured.bytes.as_deref().unwrap_or(b"")));
            commit = head;
            ref_name = head_ref;
            let entries = captured.entries.clone();
            let files = blobs(&git, &entries, limits)?;
            guard.index = Some(captured);
            (
                SourceRole::Staged,
                files,
                entries,
                SourceFreshness::CurrentAtObservation,
                vec!["staged_snapshot_only".into()],
            )
        }
        SourceSelector::Accepted
        | SourceSelector::Coordination
        | SourceSelector::Proposal { .. } => {
            let repository = Repository::open_source(&git.root().join(".workdeck"))?;
            let (config, config_hash) = repository.store()?.with_snapshot(|snapshot| {
                let config = crate::repository::config_from_snapshot(repository.root(), snapshot)?;
                let bytes = snapshot.read(Path::new("config.yml"))?.ok_or_else(stale)?;
                Ok((config, ContentHash::of(&bytes)))
            })?;
            guard.configuration = Some((repository.root().join("config.yml"), config_hash));
            expected_repository = Some(config.repository.clone());
            let shared = config.sources.as_ref().ok_or_else(|| {
                PmError::new(
                    ErrorCode::PolicyBlocked,
                    "explicit shared source configuration is required",
                )
            })?;
            let (reference, role) = match selector {
                SourceSelector::Accepted => (shared.accepted_ref.clone(), SourceRole::Accepted),
                SourceSelector::Coordination => {
                    (shared.coordination_ref.clone(), SourceRole::Coordination)
                }
                SourceSelector::Proposal { reference } => (reference.clone(), SourceRole::Proposal),
                _ => unreachable!(),
            };
            match read_binding(&git, &config, shared) {
                Ok(binding) => guard.remote_binding = Some(binding),
                Err(error) => binding_unavailable = Some(error.code),
            }
            // A cache receipt requires its admitted remote binding. With no
            // available binding, only the exact local configured ref is inspected.
            let cached = if guard.remote_binding.is_some() {
                super::remote_impl::cached(repository.root(), &git, &config, shared, &reference)?
            } else {
                None
            };
            let resolved_ref = cached
                .as_ref()
                .map(|cache| cache.reference.clone())
                .unwrap_or_else(|| reference.clone());
            let oid = if let Some(cache) = cached { remote_observation = Some(cache.observation.clone()); cache.observation.commit } else { git.resolve(reference.as_str())? }.ok_or_else(|| PmError::new(ErrorCode::NotFound, "configured source ref has no cached commit; explicitly fetch or initialize coordination"))?;
            let tree_oid = git.tree(&oid)?;
            let entries = git.tree_entries(&tree_oid)?;
            let files = blobs(&git, &entries, limits)?;
            guard.reference = Some((resolved_ref, oid.clone()));
            ref_name = Some(reference);
            commit = Some(oid);
            tree = Some(tree_oid);
            (
                role,
                files,
                entries,
                SourceFreshness::Cached,
                vec!["remote_not_observed_by_read".into()],
            )
        }
    };
    if let Some(error) = binding_unavailable {
        freshness = SourceFreshness::Unknown;
        reasons.push("publication_binding_unavailable".into());
        reasons.push(format!("binding_error:{error:?}"));
    }
    let snapshot = Snapshot::from_memory(Path::new("source-snapshot"), &files);
    let config = crate::repository::config_from_snapshot(Path::new("source-snapshot"), &snapshot)?;
    if expected_repository
        .as_ref()
        .is_some_and(|id| id != &config.repository)
    {
        return Err(stale());
    }
    validate_role(&files, &config, role, ref_name.as_ref())?;
    let identity = PlanningSourceIdentity {
        repository: config.repository,
        role,
        ref_name,
        commit,
        tree,
        index_content,
        content: content_hash(&files)?,
    };
    let view = PlanningSourceView {
        observation: SourceObservation {
            identity: identity.clone(),
            observed_at: chrono::Utc::now(),
            remote_observation,
            freshness,
            reason_codes: reasons,
        },
        snapshot: SourceSnapshot {
            identity,
            files,
            entries,
        },
        guard,
    };
    view.guard.verify_initial()?;
    Ok(view)
}
fn read_binding(
    git: &BoundGit,
    config: &Config,
    shared: &SharedSources,
) -> Result<(crate::RepositoryId, SharedSources, ContentHash)> {
    let url = git.remote_url(&shared.remote)?;
    Ok((
        config.repository.clone(),
        shared.clone(),
        git.binding(&config.repository, shared, &url)?,
    ))
}
pub(super) fn blobs(
    git: &BoundGit,
    entries: &[SourceEntry],
    limits: &SourceCaptureLimits,
) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut portable = BTreeSet::new();
    let mut selected = Vec::new();
    for entry in entries {
        if !authoritative(&entry.path)? {
            continue;
        }
        if entry.stage != 0 {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "planning source contains unresolved index stages",
            )
            .at(&entry.path));
        }
        if !matches!(entry.mode.as_str(), "100644" | "100755") {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "planning Git entries must be regular files",
            )
            .at(&entry.path));
        }
        if !portable.insert(entry.path.to_string_lossy().to_lowercase()) {
            return Err(invalid("planning source contains case-colliding paths").at(&entry.path));
        }
        if selected.len() >= limits.max_entries {
            return Err(invalid("planning source exceeds entry limit"));
        }
        selected.push(entry);
    }
    let oids = selected
        .iter()
        .map(|entry| {
            entry
                .oid
                .clone()
                .ok_or_else(|| invalid("Git entry lacks an object identity"))
        })
        .collect::<Result<Vec<_>>>()?;
    let blobs = git.blobs(&oids)?;
    for (entry, bytes) in selected.into_iter().zip(blobs) {
        files.insert(entry.path.clone(), bytes);
    }
    Ok(files)
}
pub(super) fn validate_role(
    files: &BTreeMap<PathBuf, Vec<u8>>,
    config: &Config,
    role: SourceRole,
    reference: Option<&GitRefName>,
) -> Result<()> {
    let marker = files.get(Path::new("coordination.yml"));
    if role != SourceRole::Coordination {
        if marker.is_some() {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "coordination records cannot be presented as accepted, proposal or ordinary planning authority",
            ));
        }
        return Ok(());
    }
    let marker = super::parse_coordination_marker(
        Path::new("coordination.yml"),
        marker.ok_or_else(|| invalid("coordination ref lacks its role marker"))?,
        &config.repository,
    )?;
    if reference != Some(&marker.coordination_ref) {
        return Err(invalid("coordination marker differs from selected ref"));
    }
    for path in files.keys() {
        let first = path
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str());
        if !matches!(
            first,
            Some("config.yml" | "coordination.yml" | "claims" | "operations")
        ) {
            return Err(
                invalid("coordination ref cannot contain a second writable issue catalog").at(path),
            );
        }
    }
    Ok(())
}

fn capture_local(worktree: &Path, limits: &SourceCaptureLimits) -> Result<PlanningSourceView> {
    let root = worktree
        .canonicalize()
        .map_err(|e| PmError::io(worktree, e))?;
    let root_identity = super::fs::directory(&root)?;
    let repository = Repository::open_source(&root.join(".workdeck"))?;
    let captured = repository
        .store()?
        .with_snapshot(|snapshot| files_from_snapshot(snapshot, limits, false))?;
    let CapturedFiles {
        files,
        stamps,
        hashes,
    } = captured;
    let memory = Snapshot::from_memory(Path::new("source-snapshot"), &files);
    let config = crate::repository::config_from_snapshot(Path::new("source-snapshot"), &memory)?;
    if config.sources.is_some() {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "shared source configuration requires its bound Git worktree",
        ));
    }
    validate_role(&files, &config, SourceRole::Local, None)?;
    let identity = PlanningSourceIdentity {
        repository: config.repository,
        role: SourceRole::Local,
        ref_name: None,
        commit: None,
        tree: None,
        index_content: None,
        content: content_hash_from_hashes(&files, &hashes)?,
    };
    let entries = files
        .keys()
        .map(|path| SourceEntry {
            path: path.clone(),
            mode: "100644".into(),
            oid: None,
            stage: 0,
        })
        .collect();
    let guard = CaptureGuard {
        git: None,
        local_root: Some((root, root_identity)),
        head: None,
        head_ref: None,
        reference: None,
        index: None,
        working: Some(files.clone()),
        working_stamps: Some(stamps),
        working_hashes: Some(hashes),
        configuration: None,
        remote_binding: None,
        limits: limits.clone(),
    };
    let view = PlanningSourceView {
        observation: SourceObservation {
            identity: identity.clone(),
            observed_at: chrono::Utc::now(),
            remote_observation: None,
            freshness: SourceFreshness::CurrentAtObservation,
            reason_codes: vec!["local_source_only".into()],
        },
        snapshot: SourceSnapshot {
            identity,
            files,
            entries,
        },
        guard,
    };
    view.guard.verify_initial()?;
    Ok(view)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optimized_content_hash_preserves_legacy_canonical_bytes() {
        let files = BTreeMap::from([
            (PathBuf::from("config.yml"), b"repository: repo".to_vec()),
            (
                PathBuf::from("issues/WD-01/item.md"),
                "café\n".as_bytes().to_vec(),
            ),
        ]);
        let legacy = crate::transactions::canonical_hash(&serde_json::json!({
            "schema": 1,
            "files": files
                .iter()
                .map(|(path, bytes)| (path, ContentHash::of(bytes), bytes.len()))
                .collect::<Vec<_>>()
        }))
        .unwrap();
        assert_eq!(content_hash(&files).unwrap(), legacy);
    }
}
