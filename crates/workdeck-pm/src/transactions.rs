//! Serialized, recoverable mutations of an initialized planning root.
//!
//! Cooperating readers and writers hold a bounded OS file lock. File replacements
//! are individually atomic; a durable redo journal makes a multi-file operation
//! recoverable, and readers refuse unfinished operations. Direct editors do not
//! hold this lock: content is checked before preparation is committed and again
//! immediately before every replacement. This is not protection against a hostile
//! process swapping paths or bytes in the remaining check/rename interval.
//!
//! Files and parent directories are synced on Unix. Other platforms return an
//! explicit unsupported durability error rather than claiming equivalent crash
//! guarantees without a qualified directory-sync implementation. A filesystem
//! must honor OS locks, same-filesystem rename, and sync for these guarantees.
use crate::{
    ContentHash, ErrorCode, OperationId, PmError, RepositoryId, RequestId, Result, SchemaVersion,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{ErrorKind, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

const LOCAL_DIRS: &[&str] = &[".tmp", ".index", ".local", "index"];
const LOCK_PATH: &str = ".tmp/writer.lock";
const JOURNALS: &str = ".tmp/journals";
const WRITES: &str = ".tmp/writes";
/// Migration batches can legitimately span several seconds while preserving
/// one coordination lock. Keep the wait bounded, but allow competing
/// subprocesses to observe the completed request instead of failing solely on
/// normal large-fixture contention.
const MIGRATION_LOCK_TIMEOUT: Duration = Duration::from_secs(10);
/// Bound file reads and the complete encoded redo journal before publication.
pub const MAX_TRANSACTION_FILE_BYTES: usize = 64 * 1024 * 1024;
/// Hard ceiling for one authoritative tree traversal, including directories.
pub const MAX_SNAPSHOT_ENTRIES: usize = 1_000_000;

#[derive(Debug, Clone)]
pub struct TransactionStore {
    root: PathBuf,
    lock_timeout: Duration,
    repository: Option<RepositoryId>,
    migration: Option<OperationId>,
    operation_limits: crate::ClaimCatalogLimits,
}

/// Valid only for the duration of the store's locked callback. Fields are private
/// and snapshots cannot be constructed by callers or retained after the callback.
pub struct Snapshot<'a> {
    root: &'a Path,
    memory: Option<&'a BTreeMap<PathBuf, Vec<u8>>>,
    reads: RefCell<BTreeMap<PathBuf, Option<Vec<u8>>>>,
    read_limits: RefCell<BTreeMap<PathBuf, usize>>,
    lists: RefCell<BTreeMap<PathBuf, Listing>>,
    list_limits: RefCell<BTreeMap<PathBuf, usize>>,
}

struct Listing {
    files: Vec<PathBuf>,
    entries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileChange {
    pub path: PathBuf,
    /// `None` is a create-only precondition, never an unconditional overwrite.
    pub expected: Option<ContentHash>,
    /// `None` deletes a file under its expected-content precondition.
    #[serde(with = "content_encoding")]
    pub content: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct PreparedOperation {
    pub changes: Vec<FileChange>,
    pub result: Value,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangedPath {
    pub path: PathBuf,
    pub before: Option<ContentHash>,
    pub after: Option<ContentHash>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MutationReceipt {
    pub schema_version: SchemaVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<RepositoryId>,
    pub operation_id: OperationId,
    pub request_id: RequestId,
    pub operation: String,
    pub input_hash: ContentHash,
    pub result: Value,
    /// Application paths only. The engine-owned receipt is not included here.
    pub changed: Vec<ChangedPath>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
pub struct PendingOperation {
    pub receipt: MutationReceipt,
    pub journal: PathBuf,
    pub applied_paths: Vec<PathBuf>,
    pub remaining_paths: Vec<PathBuf>,
    pub recoverable: bool,
    pub errors: Vec<PmError>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    schema_version: SchemaVersion,
    receipt: MutationReceipt,
    changes: Vec<FileChange>,
    reads: Vec<ReadFingerprint>,
    listings: Vec<ListingFingerprint>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadFingerprint {
    path: PathBuf,
    content: Option<ContentHash>,
    max_bytes: usize,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListingFingerprint {
    prefix: PathBuf,
    files: Vec<PathBuf>,
    #[serde(default = "default_listing_limit")]
    max_entries: usize,
}

fn default_listing_limit() -> usize {
    MAX_SNAPSHOT_ENTRIES
}

/// Fault hooks are explicit test instrumentation, never controlled by an env var.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultPoint {
    BeforeJournal,
    AfterJournal,
    BeforeChange(usize),
    AfterChange(usize),
    BeforeReceipt,
    AfterReceipt,
}

impl TransactionStore {
    /// Open an existing root. This may create ignored coordination directories;
    /// it never creates configuration or other authoritative planning records.
    pub fn open(root: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(root).map_err(|error| {
            if error.kind() == ErrorKind::NotFound {
                PmError::new(ErrorCode::NotInitialized, "planning root does not exist").at(root)
            } else {
                PmError::io(root, error)
            }
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(unsafe_path(root, "planning root must be a real directory"));
        }
        let root = root
            .canonicalize()
            .map_err(|error| PmError::io(root, error))?;
        crate::restore::check_root(&root)?;
        if read_file(&root, Path::new("config.yml"))?.is_none() {
            crate::migration::check_root(&root)?;
            return Err(
                PmError::new(ErrorCode::NotInitialized, "planning root has no config.yml")
                    .at(&root),
            );
        }
        ensure_directory(&root, Path::new(".tmp"))?;
        Ok(Self {
            root,
            lock_timeout: Duration::from_secs(2),
            repository: None,
            migration: None,
            operation_limits: crate::ClaimCatalogLimits::default(),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn for_repository(mut self, repository: RepositoryId) -> Self {
        self.repository = Some(repository);
        self
    }

    /// Only migration orchestration can construct this admission token. Normal
    /// callers always encounter the pending barrier under the writer lock.
    pub(crate) fn for_migration(mut self, migration: OperationId) -> Self {
        self.migration = Some(migration);
        self
    }

    fn check_identity(&self, snapshot: &Snapshot<'_>) -> Result<()> {
        crate::sources::reject_coordination_snapshot(snapshot)?;
        crate::restore::check_root(&self.root)?;
        crate::migration::check_access(&self.root, snapshot, self.migration.as_ref())?;
        if let Some(expected) = &self.repository {
            let config = crate::repository::config_from_snapshot(&self.root, snapshot)?;
            if &config.repository != expected {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "planning repository identity changed after this source was opened",
                )
                .at(self.root.join("config.yml"))
                .hint(
                    "Inspect the replacement source and reopen it explicitly before continuing.",
                ));
            }
        }
        Ok(())
    }

    /// Bounded waits avoid indefinitely blocking an agent behind another writer.
    pub fn with_lock_timeout(mut self, timeout: Duration) -> Self {
        self.lock_timeout = timeout;
        self
    }

    /// Explicit test instrumentation; production operation-history limits may
    /// only be tightened. Durable replay and recovery retain their original intent.
    #[doc(hidden)]
    pub fn with_operation_limits(mut self, max_receipts: usize, max_bytes: usize) -> Result<Self> {
        self.operation_limits.max_operations = max_receipts;
        self.operation_limits.max_operation_bytes = max_bytes;
        self.operation_limits.validate()?;
        Ok(self)
    }

    pub fn with_snapshot<T>(&self, read: impl FnOnce(&Snapshot<'_>) -> Result<T>) -> Result<T> {
        let _lock = self.acquire()?;
        self.require_recovered()?;
        let snapshot = Snapshot::new(&self.root);
        self.check_identity(&snapshot)?;
        let result = read(&snapshot)?;
        snapshot.validate()?;
        Ok(result)
    }

    /// Inspect a retained request before external work. A subsequent mutation
    /// must still perform its normal replay check under the writer lock.
    pub fn replay_receipt(
        &self,
        request: &RequestId,
        operation: &str,
        input: &Value,
    ) -> Result<Option<MutationReceipt>> {
        validate_operation(operation)?;
        let input_hash = canonical_hash(input)?;
        self.with_snapshot(|_| {
            let Some(receipt) = self.find_request(request)? else { return Ok(None); };
            if self.repository.is_some() && receipt.repository != self.repository {
                return Err(PmError::new(ErrorCode::StaleSource,
                    "request receipt does not belong to the opened repository identity"));
            }
            if receipt.operation != operation || receipt.input_hash != input_hash {
                return Err(PmError::new(ErrorCode::IdempotencyConflict,
                    format!("request {request} already identifies a different operation or input"))
                    .hint("Use the original request inputs to retrieve its result, or a new request ID for new work."));
            }
            crate::claims::validate_receipt(&receipt)?;
            crate::completion::validate_receipt(&receipt)?;
            Ok(Some(receipt))
        })
    }

    pub fn transact(
        &self,
        request: &RequestId,
        operation: &str,
        input: &Value,
        prepare: impl FnOnce(&Snapshot<'_>) -> Result<PreparedOperation>,
    ) -> Result<MutationReceipt> {
        self.transact_with_faults(request, operation, input, prepare, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn transact_with_faults(
        &self,
        request: &RequestId,
        operation: &str,
        input: &Value,
        prepare: impl FnOnce(&Snapshot<'_>) -> Result<PreparedOperation>,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.transact_guarded(request, operation, input, |_| Ok(()), prepare, fault)
    }

    /// Optional source-admission checks run under the same writer lock before
    /// replay lookup. Their reads remain part of the transaction's read set.
    /// Ordinary semantic preconditions belong in `prepare`, after replay.
    pub fn transact_with_preflight(
        &self,
        request: &RequestId,
        operation: &str,
        input: &Value,
        guard: impl FnOnce(&Snapshot<'_>) -> Result<()>,
        prepare: impl FnOnce(&Snapshot<'_>) -> Result<PreparedOperation>,
    ) -> Result<MutationReceipt> {
        self.transact_guarded(request, operation, input, guard, prepare, |_| Ok(()))
    }

    fn transact_guarded(
        &self,
        request: &RequestId,
        operation: &str,
        input: &Value,
        guard: impl FnOnce(&Snapshot<'_>) -> Result<()>,
        prepare: impl FnOnce(&Snapshot<'_>) -> Result<PreparedOperation>,
        mut fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        validate_operation(operation)?;
        let _lock = self.acquire()?;
        self.require_recovered()?;
        let snapshot = Snapshot::new(&self.root);
        self.check_identity(&snapshot)?;
        guard(&snapshot)?;
        let input_hash = canonical_hash(input)?;
        if let Some(receipt) = self.find_request(request)? {
            if self.repository.is_some() && receipt.repository != self.repository {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "request receipt does not belong to the opened repository identity",
                ));
            }
            if receipt.operation != operation || receipt.input_hash != input_hash {
                return Err(PmError::new(ErrorCode::IdempotencyConflict,
                    format!("request {request} already identifies a different operation or input"))
                    .hint("Use the original request inputs to retrieve its result, or a new request ID for new work."));
            }
            snapshot.validate()?;
            return Ok(receipt);
        }
        let prepared = prepare(&snapshot)?;
        validate_changes(&prepared.changes)?;
        if self.migration.is_none()
            && prepared
                .changes
                .iter()
                .any(|change| migration_path(&change.path))
        {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "migration manifests and cutover markers are reserved for migration orchestration",
            ));
        }
        let cutover_identity =
            crate::migration::check_access(&self.root, &snapshot, self.migration.as_ref())?;
        if let Some(expected) = self.repository.as_ref().or(cutover_identity.as_ref()) {
            for change in &prepared.changes {
                if portable_key(&change.path) == "config.yml" {
                    let config = crate::repository::parse_config(
                        &self.root.join(&change.path),
                        change
                            .content
                            .as_deref()
                            .expect("configuration deletion was rejected"),
                    )?;
                    if &config.repository != expected {
                        return Err(PmError::new(
                            ErrorCode::InvalidInput,
                            "a mutation cannot replace its repository identity",
                        )
                        .at(&change.path));
                    }
                }
            }
        }
        for change in &prepared.changes {
            require_hash(
                &self.root,
                &change.path,
                &change.expected,
                ErrorCode::StaleSource,
            )?;
        }
        let receipt = MutationReceipt {
            schema_version: SchemaVersion::CURRENT,
            repository: self.repository.clone(),
            operation_id: OperationId::new(),
            request_id: request.clone(),
            operation: operation.into(),
            input_hash,
            result: prepared.result,
            changed: prepared
                .changes
                .iter()
                .map(|change| ChangedPath {
                    path: change.path.clone(),
                    before: change.expected.clone(),
                    after: change.content.as_deref().map(ContentHash::of),
                })
                .collect(),
        };
        // Every canonical receipt consumes the same history budget, including
        // mutations unrelated to claims. Capture both bytes and membership as
        // ordinary journal dependencies before publishing any application path.
        crate::claims::store::validate_operation_capacity(
            &snapshot,
            &receipt,
            &self.operation_limits,
        )?;
        let journal = Journal {
            schema_version: SchemaVersion::CURRENT,
            receipt,
            changes: prepared.changes,
            reads: snapshot
                .reads
                .borrow()
                .iter()
                .map(|(path, bytes)| ReadFingerprint {
                    path: path.clone(),
                    content: bytes.as_deref().map(ContentHash::of),
                    max_bytes: snapshot
                        .read_limits
                        .borrow()
                        .get(path)
                        .copied()
                        .unwrap_or(MAX_TRANSACTION_FILE_BYTES),
                })
                .collect(),
            listings: snapshot
                .lists
                .borrow()
                .iter()
                .map(|(prefix, listing)| ListingFingerprint {
                    prefix: prefix.clone(),
                    files: listing.files.clone(),
                    max_entries: snapshot.list_limits.borrow()[prefix],
                })
                .collect(),
        };
        fault(FaultPoint::BeforeJournal)?;
        // Includes inputs such as configuration, prerequisite records, and issue
        // membership that were consulted but are not themselves being written.
        snapshot.validate()?;
        let path = journal_path(&journal.receipt.operation_id);
        let bytes = encode(&journal, &path)?;
        if bytes.len() > MAX_TRANSACTION_FILE_BYTES {
            return Err(PmError::new(ErrorCode::InvalidInput, "encoded operation journal exceeds 64 MiB; split the request into bounded operations").at(&path));
        }
        // No authority bytes are changed until this publication is durable.
        if let Err(error) = atomic_replace(&self.root, &path, None, Some(&bytes)) {
            // Rename can succeed before the directory sync reports an error.
            // In that case the published journal must remain an explicit barrier.
            return Err(if self.root.join(&path).exists() {
                recovery_error(&path, error)
            } else {
                error
            });
        }
        let apply = (|| {
            fault(FaultPoint::AfterJournal)?;
            self.apply(&journal, false, &mut fault)?;
            self.remove_journal(&path)?;
            Ok(journal.receipt.clone())
        })();
        apply.map_err(|error| recovery_error(&path, error))
    }

    /// Inspect durable intent and current file states without applying a write.
    /// Recoverability is a snapshot observation, not a promise about later edits.
    pub fn pending_operations(&self) -> Result<Vec<PendingOperation>> {
        let _lock = self.acquire()?;
        self.check_identity(&Snapshot::new(&self.root))?;
        let paths = self.journals()?;
        let multiple = paths.len() > 1;
        let mut pending = Vec::new();
        for path in paths {
            let bytes = read_file(&self.root, &path)?.ok_or_else(|| {
                PmError::new(
                    ErrorCode::RecoveryRequired,
                    "journal disappeared during inspection",
                )
                .at(&path)
            })?;
            let journal: Journal =
                decode(&bytes, &path).map_err(|error| recovery_error(&path, error))?;
            self.validate_journal(&path, &journal)
                .map_err(|error| recovery_error(&path, error))?;
            let mut report = PendingOperation {
                receipt: journal.receipt.clone(),
                journal: path,
                applied_paths: Vec::new(),
                remaining_paths: Vec::new(),
                recoverable: true,
                errors: Vec::new(),
            };
            if multiple {
                report.errors.push(PmError::new(
                    ErrorCode::RecoveryRequired,
                    "multiple unfinished journals require explicit inspection",
                ));
            }
            if let Err(error) = self.validate_inputs(&journal) {
                report.errors.push(error);
            }
            for change in &journal.changes {
                let current = read_file(&self.root, &change.path)?
                    .as_deref()
                    .map(ContentHash::of);
                let after = change.content.as_deref().map(ContentHash::of);
                if current == after {
                    report.applied_paths.push(change.path.clone());
                } else if current == change.expected {
                    report.remaining_paths.push(change.path.clone());
                } else {
                    report.errors.push(
                        PmError::new(
                            ErrorCode::StaleSource,
                            "file differs from both recorded before and intended after content",
                        )
                        .at(&change.path),
                    );
                }
            }
            report.recoverable = report.errors.is_empty();
            pending.push(report);
        }
        Ok(pending)
    }

    /// Explicitly finish a durable redo operation. Conflicting direct edits are
    /// never overwritten. This does not perform a destructive rollback.
    pub fn recover(&self) -> Result<Vec<MutationReceipt>> {
        let _lock = self.acquire()?;
        self.check_identity(&Snapshot::new(&self.root))?;
        let paths = self.journals()?;
        // A cooperating transaction cannot start behind an unfinished journal.
        // Multiple journals therefore require inspection, not an invented order.
        if paths.len() > 1 {
            return Err(PmError::new(
                ErrorCode::RecoveryRequired,
                "multiple unfinished journals require explicit inspection",
            )
            .at(self.root.join(JOURNALS)));
        }
        let mut recovered = Vec::new();
        for path in paths {
            let bytes = read_file(&self.root, &path)?.ok_or_else(|| {
                PmError::new(
                    ErrorCode::RecoveryRequired,
                    "journal disappeared during recovery",
                )
                .at(&path)
            })?;
            let journal: Journal =
                decode(&bytes, &path).map_err(|error| recovery_error(&path, error))?;
            self.validate_journal(&path, &journal)
                .map_err(|error| recovery_error(&path, error))?;
            self.apply(&journal, true, &mut |_| Ok(()))
                .map_err(|error| recovery_error(&path, error))?;
            self.remove_journal(&path)
                .map_err(|error| recovery_error(&path, error))?;
            recovered.push(journal.receipt);
        }
        Ok(recovered)
    }

    fn acquire(&self) -> Result<File> {
        let path = checked_path(&self.root, Path::new(LOCK_PATH))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| PmError::io(&path, error))?;
        if !file
            .metadata()
            .map_err(|error| PmError::io(&path, error))?
            .is_file()
        {
            return Err(unsafe_path(&path, "writer lock must be a regular file"));
        }
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(TryLockError::Error(error)) => return Err(PmError::io(&path, error)),
                Err(TryLockError::WouldBlock) if started.elapsed() >= self.lock_timeout => {
                    return Err(PmError::new(
                        ErrorCode::Locked,
                        "planning store is locked by another operation",
                    )
                    .at(&path)
                    .hint(
                        "Retry after the active operation finishes; do not remove the lock file.",
                    ));
                }
                Err(TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }

    fn journals(&self) -> Result<Vec<PathBuf>> {
        list_files(&self.root, Path::new(JOURNALS), false)
    }

    fn require_recovered(&self) -> Result<()> {
        let paths = self.journals()?;
        if let Some(path) = paths.first() {
            return Err(PmError::new(
                ErrorCode::RecoveryRequired,
                "unfinished planning mutation requires recovery",
            )
            .at(path)
            .hint(
                "Run explicit operation recovery before reading or writing this planning source.",
            ));
        }
        Ok(())
    }

    fn find_request(&self, request: &RequestId) -> Result<Option<MutationReceipt>> {
        let mut found = None;
        for path in list_files(&self.root, Path::new("operations"), false)? {
            let bytes = read_file(&self.root, &path)?.ok_or_else(|| {
                PmError::new(ErrorCode::CorruptStore, "operation receipt disappeared").at(&path)
            })?;
            let receipt: MutationReceipt = decode(&bytes, &path)?;
            validate_receipt(&receipt)?;
            if path != receipt_path(&receipt.operation_id) {
                return Err(PmError::new(
                    ErrorCode::CorruptStore,
                    "receipt path does not match operation identity",
                )
                .at(&path));
            }
            if &receipt.request_id == request && found.replace(receipt).is_some() {
                return Err(PmError::new(
                    ErrorCode::CorruptStore,
                    "request ID appears in multiple receipts",
                )
                .at(&path));
            }
        }
        Ok(found)
    }

    fn validate_journal(&self, path: &Path, journal: &Journal) -> Result<()> {
        if self.migration.is_none()
            && journal
                .changes
                .iter()
                .any(|change| migration_path(&change.path))
        {
            return Err(PmError::new(
                ErrorCode::RecoveryRequired,
                "migration journals must be resumed through migration recovery",
            )
            .at(path));
        }
        if path != journal_path(&journal.receipt.operation_id) {
            return Err(PmError::new(
                ErrorCode::CorruptStore,
                "journal path does not match operation identity",
            )
            .at(path));
        }
        validate_receipt(&journal.receipt)?;
        crate::graph::validate_receipt(&journal.receipt)?;
        crate::attestations::records::validate_receipt(&journal.receipt)?;
        crate::retained_reviews::records::validate_receipt(&journal.receipt)?;
        crate::claims::validate_receipt(&journal.receipt)?;
        crate::completion::validate_receipt(&journal.receipt)?;
        validate_changes(&journal.changes)?;
        for read in &journal.reads {
            validate_relative(&read.path, false)?;
            reject_local(&read.path)?;
            if read.max_bytes > MAX_TRANSACTION_FILE_BYTES {
                return Err(PmError::new(
                    ErrorCode::CorruptStore,
                    "journal read bound exceeds the supported limit",
                )
                .at(&read.path));
            }
        }
        for listing in &journal.listings {
            validate_relative(&listing.prefix, true)?;
            reject_local(&listing.prefix)?;
            if listing.max_entries > MAX_SNAPSHOT_ENTRIES
                || listing.files.len() > listing.max_entries
            {
                return Err(PmError::new(
                    ErrorCode::CorruptStore,
                    "journal listing bound is invalid",
                )
                .at(&listing.prefix));
            }
            for path in &listing.files {
                validate_relative(path, false)?;
                reject_local(path)?;
                if !path.starts_with(&listing.prefix) {
                    return Err(PmError::new(
                        ErrorCode::CorruptStore,
                        "journal listing contains a path outside its prefix",
                    )
                    .at(path));
                }
            }
            if listing.files.windows(2).any(|pair| pair[0] >= pair[1]) {
                return Err(PmError::new(
                    ErrorCode::CorruptStore,
                    "journal listing is not sorted and unique",
                )
                .at(&listing.prefix));
            }
        }
        let expected: Vec<_> = journal
            .changes
            .iter()
            .map(|change| ChangedPath {
                path: change.path.clone(),
                before: change.expected.clone(),
                after: change.content.as_deref().map(ContentHash::of),
            })
            .collect();
        if expected != journal.receipt.changed {
            return Err(PmError::new(
                ErrorCode::CorruptStore,
                "journal changes disagree with receipt content identities",
            )
            .at(path));
        }
        if let Some(existing) = self.find_request(&journal.receipt.request_id)?
            && existing != journal.receipt
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "journal request conflicts with a published receipt",
            )
            .at(path));
        }
        Ok(())
    }

    fn apply(
        &self,
        journal: &Journal,
        recovering: bool,
        fault: &mut impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<()> {
        self.validate_inputs(journal)?;
        // Check the entire change set before advancing any remaining file. During
        // recovery, each file may still be before or already be after, but never
        // an unrelated third value. A missing file is a state too.
        for change in &journal.changes {
            let current = read_file(&self.root, &change.path)?
                .as_deref()
                .map(ContentHash::of);
            let after = change.content.as_deref().map(ContentHash::of);
            if current != change.expected && !(recovering && current == after) {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "file differs from the operation's expected content",
                )
                .at(&change.path));
            }
        }
        let receipt_path = receipt_path(&journal.receipt.operation_id);
        let receipt_bytes = encode(&journal.receipt, &receipt_path)?;
        let existing_receipt = read_file(&self.root, &receipt_path)?;
        if existing_receipt
            .as_deref()
            .is_some_and(|bytes| bytes != receipt_bytes)
        {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "published receipt contains different content",
            )
            .at(&receipt_path));
        }
        for (index, change) in journal.changes.iter().enumerate() {
            fault(FaultPoint::BeforeChange(index))?;
            let current = read_file(&self.root, &change.path)?
                .as_deref()
                .map(ContentHash::of);
            let after = change.content.as_deref().map(ContentHash::of);
            if recovering && current == after {
                // Even an already-applied file is synced before publishing a
                // recovered receipt; the previous process may have died after rename.
                sync_existing(&self.root, &change.path)?;
            } else {
                atomic_replace(
                    &self.root,
                    &change.path,
                    change.expected.as_ref(),
                    change.content.as_deref(),
                )?;
            }
            fault(FaultPoint::AfterChange(index))?;
        }
        fault(FaultPoint::BeforeReceipt)?;
        // A noncooperating editor may modify an earlier file while later writes
        // are in progress. Do not publish success for such an observed mismatch.
        for change in &journal.changes {
            require_hash(
                &self.root,
                &change.path,
                &change.content.as_deref().map(ContentHash::of),
                ErrorCode::StaleSource,
            )?;
        }
        self.validate_inputs(journal)?;
        if existing_receipt.is_none() {
            atomic_replace(&self.root, &receipt_path, None, Some(&receipt_bytes))?;
        } else {
            sync_existing(&self.root, &receipt_path)?;
        }
        fault(FaultPoint::AfterReceipt)?;
        Ok(())
    }

    fn validate_inputs(&self, journal: &Journal) -> Result<()> {
        let changes: BTreeMap<_, _> = journal
            .changes
            .iter()
            .map(|change| (portable_key(&change.path), change))
            .collect();
        for read in &journal.reads {
            let current = read_file_bounded(&self.root, &read.path, read.max_bytes)?
                .as_deref()
                .map(ContentHash::of);
            let key = portable_key(&read.path);
            let changed = changes.get(&key);
            let valid = if let Some(change) = changed {
                current == change.expected
                    || current == change.content.as_deref().map(ContentHash::of)
            } else {
                current == read.content
            };
            if !valid {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "an operation input changed after preparation",
                )
                .at(&read.path));
            }
        }
        let mut own_paths: BTreeSet<_> = journal
            .changes
            .iter()
            .map(|change| portable_key(&change.path))
            .collect();
        own_paths.insert(portable_key(&receipt_path(&journal.receipt.operation_id)));
        for listing in &journal.listings {
            // New files and parent directories belonging to this operation may
            // already exist after interruption. Reserve their bounded maximum
            // while keeping external growth subject to the original read bound.
            let mut own_entries = BTreeSet::new();
            for path in journal
                .changes
                .iter()
                .map(|change| change.path.clone())
                .chain(std::iter::once(receipt_path(&journal.receipt.operation_id)))
            {
                for ancestor in path.ancestors().take_while(|ancestor| {
                    *ancestor != listing.prefix && ancestor.starts_with(&listing.prefix)
                }) {
                    own_entries.insert(ancestor.to_owned());
                }
            }
            let current = list_files_bounded(
                &self.root,
                &listing.prefix,
                true,
                listing.max_entries.saturating_add(own_entries.len()),
            )?
            .files;
            let external = |files: &[PathBuf]| {
                files
                    .iter()
                    .filter(|path| !own_paths.contains(&portable_key(path)))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            if external(&current) != external(&listing.files) {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "operation input directory membership changed after preparation",
                )
                .at(&listing.prefix));
            }
        }
        Ok(())
    }

    fn remove_journal(&self, relative: &Path) -> Result<()> {
        let path = checked_path(&self.root, relative)?;
        fs::remove_file(&path).map_err(|error| PmError::io(&path, error))?;
        sync_directory(path.parent().expect("journal has parent"))
    }
}

impl Snapshot<'_> {
    /// Preview the exact serializer overhead with maximum-width request IDs.
    /// Source/list dependencies are included, so preview cannot green-light a
    /// change set whose recoverable journal exceeds the engine's own limit.
    pub(crate) fn check_prepared_capacity(
        &self,
        prepared: &PreparedOperation,
        repository: &RepositoryId,
        operation: &str,
    ) -> Result<()> {
        let receipt = MutationReceipt {
            schema_version: SchemaVersion::CURRENT,
            repository: Some(repository.clone()),
            operation_id: "OP-00000000000000000000000000".parse().expect("fixed ID"),
            request_id: "a".repeat(96).parse().expect("maximum request width"),
            operation: operation.into(),
            input_hash: ContentHash::of(&[]),
            result: prepared.result.clone(),
            changed: prepared
                .changes
                .iter()
                .map(|change| ChangedPath {
                    path: change.path.clone(),
                    before: change.expected.clone(),
                    after: change.content.as_deref().map(ContentHash::of),
                })
                .collect(),
        };
        crate::claims::store::validate_operation_capacity(
            self,
            &receipt,
            &crate::ClaimCatalogLimits::default(),
        )?;
        let journal = Journal {
            schema_version: SchemaVersion::CURRENT,
            receipt,
            changes: prepared.changes.clone(),
            reads: self
                .reads
                .borrow()
                .iter()
                .map(|(path, bytes)| ReadFingerprint {
                    path: path.clone(),
                    content: bytes.as_deref().map(ContentHash::of),
                    max_bytes: self
                        .read_limits
                        .borrow()
                        .get(path)
                        .copied()
                        .unwrap_or(MAX_TRANSACTION_FILE_BYTES),
                })
                .collect(),
            listings: self
                .lists
                .borrow()
                .iter()
                .map(|(prefix, listing)| ListingFingerprint {
                    prefix: prefix.clone(),
                    files: listing.files.clone(),
                    max_entries: self.list_limits.borrow()[prefix],
                })
                .collect(),
        };
        if encode(&journal, Path::new("snapshot import journal capacity"))?.len()
            > MAX_TRANSACTION_FILE_BYTES
        {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "complete import plan exceeds the 64 MiB recoverable journal limit",
            ));
        }
        Ok(())
    }

    /// Fingerprint every authority read so a reviewed composite operation also
    /// binds policy catalogs and absence/listing observations used by validation.
    pub(crate) fn read_fingerprint(&self) -> Result<ContentHash> {
        let reads = self.reads.borrow();
        let lists = self.lists.borrow();
        canonical_hash(&serde_json::json!({
            "root": self.root,
            "reads": reads.iter().map(|(path, bytes)| (path, bytes.as_deref().map(ContentHash::of))).collect::<Vec<_>>(),
            "lists": lists.iter().map(|(path, listing)| (path, &listing.files)).collect::<Vec<_>>()
        }))
    }

    pub fn read(&self, relative: &Path) -> Result<Option<Vec<u8>>> {
        self.read_bounded(relative, MAX_TRANSACTION_FILE_BYTES)
    }

    /// Record a result obtained by a source-specific bounded reader. The
    /// caller has already validated the path and read atomically; retaining it
    /// here keeps the normal snapshot validation/recovery fingerprint intact.
    pub(crate) fn record_read(&self, relative: &Path, bytes: Option<Vec<u8>>, max_bytes: usize) {
        self.reads.borrow_mut().insert(relative.to_owned(), bytes);
        self.read_limits
            .borrow_mut()
            .insert(relative.to_owned(), max_bytes);
    }

    /// Read at most `max_bytes`, additionally bounded by the engine's 64 MiB
    /// limit. The bound applies before allocation and during streaming reads.
    pub fn read_bounded(&self, relative: &Path, max_bytes: usize) -> Result<Option<Vec<u8>>> {
        validate_relative(relative, false)?;
        reject_local(relative)?;
        if max_bytes > MAX_TRANSACTION_FILE_BYTES {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "requested read bound exceeds 64 MiB",
            )
            .at(relative));
        }
        if let Some(bytes) = self.reads.borrow().get(relative) {
            if bytes.as_ref().is_some_and(|bytes| bytes.len() > max_bytes) {
                return Err(oversized_file(relative, max_bytes));
            }
            self.read_limits
                .borrow_mut()
                .entry(relative.to_owned())
                .and_modify(|limit| *limit = (*limit).min(max_bytes))
                .or_insert(max_bytes);
            return Ok(bytes.clone());
        }
        let bytes = if let Some(files) = self.memory {
            let bytes = files.get(relative).cloned();
            if bytes.as_ref().is_some_and(|bytes| bytes.len() > max_bytes) {
                return Err(oversized_file(relative, max_bytes));
            }
            bytes
        } else {
            read_file_bounded(self.root, relative, max_bytes)?
        };
        self.reads
            .borrow_mut()
            .insert(relative.to_owned(), bytes.clone());
        self.read_limits
            .borrow_mut()
            .insert(relative.to_owned(), max_bytes);
        Ok(bytes)
    }

    pub fn list(&self, prefix: &Path) -> Result<Vec<PathBuf>> {
        self.list_bounded(prefix, MAX_SNAPSHOT_ENTRIES)
    }

    /// Sorted file paths relative to the planning root, with a limit enforced
    /// during traversal. Each file and directory below `prefix` consumes one
    /// entry. Local state is excluded. Symlinks and special files are errors.
    /// The strictest successful limit is retained for validation and recovery.
    pub fn list_bounded(&self, prefix: &Path, max_entries: usize) -> Result<Vec<PathBuf>> {
        validate_relative(prefix, true)?;
        reject_local(prefix)?;
        if max_entries > MAX_SNAPSHOT_ENTRIES {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "requested listing bound exceeds 1,000,000 entries",
            )
            .at(prefix));
        }
        if let Some(listing) = self.lists.borrow().get(prefix) {
            if listing.entries > max_entries {
                return Err(oversized_listing(prefix, max_entries));
            }
            self.list_limits
                .borrow_mut()
                .entry(prefix.to_owned())
                .and_modify(|limit| *limit = (*limit).min(max_entries));
            return Ok(listing.files.clone());
        }
        let listing = if let Some(files) = self.memory {
            if files.contains_key(prefix) {
                return Err(unsafe_path(prefix, "a listing prefix must be a directory"));
            }
            let mut entries = BTreeSet::new();
            let mut paths = Vec::new();
            for path in files.keys().filter(|path| path.starts_with(prefix)) {
                validate_relative(path, false)?;
                if reject_local(path).is_err() {
                    continue;
                }
                for ancestor in path.ancestors().take_while(|ancestor| *ancestor != prefix) {
                    entries.insert(ancestor.to_owned());
                    if entries.len() > max_entries {
                        return Err(oversized_listing(prefix, max_entries));
                    }
                }
                paths.push(path.clone());
            }
            Listing {
                files: paths,
                entries: entries.len(),
            }
        } else {
            list_files_bounded(self.root, prefix, true, max_entries)?
        };
        let paths = listing.files.clone();
        self.lists.borrow_mut().insert(prefix.to_owned(), listing);
        self.list_limits
            .borrow_mut()
            .insert(prefix.to_owned(), max_entries);
        Ok(paths)
    }

    fn validate(&self) -> Result<()> {
        if self.memory.is_some() {
            return Ok(());
        }
        for (path, bytes) in self.reads.borrow().iter() {
            let limit = self
                .read_limits
                .borrow()
                .get(path)
                .copied()
                .unwrap_or(MAX_TRANSACTION_FILE_BYTES);
            if read_file_bounded(self.root, path, limit)? != *bytes {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "snapshot input changed outside the cooperating store lock",
                )
                .at(path)
                .hint("Reload the complete source context before retrying."));
            }
        }
        for (prefix, listing) in self.lists.borrow().iter() {
            if list_files_bounded(self.root, prefix, true, self.list_limits.borrow()[prefix])?.files
                != listing.files
            {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "snapshot directory membership changed outside the cooperating store lock",
                )
                .at(prefix)
                .hint("Reload the complete source context before retrying."));
            }
        }
        Ok(())
    }
}

impl<'a> Snapshot<'a> {
    /// The canonical source root used by bounded readers. This is crate
    /// internal so source capture can fan out independent descriptor-bound
    /// reads without exposing a host path to application callers.
    pub(crate) fn root(&self) -> &Path {
        self.root
    }

    pub(crate) fn new(root: &'a Path) -> Self {
        Self {
            root,
            memory: None,
            reads: RefCell::new(BTreeMap::new()),
            read_limits: RefCell::new(BTreeMap::new()),
            lists: RefCell::new(BTreeMap::new()),
            list_limits: RefCell::new(BTreeMap::new()),
        }
    }

    /// Immutable validation view only. It cannot write, acquire a store, or read
    /// through to the host filesystem when a key is absent.
    pub(crate) fn from_memory(root: &'a Path, files: &'a BTreeMap<PathBuf, Vec<u8>>) -> Self {
        Self {
            memory: Some(files),
            ..Self::new(root)
        }
    }
}

fn migration_path(path: &Path) -> bool {
    path.components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        .is_some_and(|component| {
            matches!(
                component.to_ascii_lowercase().as_str(),
                "migration.yml" | "migrations"
            )
        })
}

/// Explicit migration bootstrap owns a separate stable coordination lock across
/// batches. Its writer guards share the ordinary transaction lock inode.
pub(crate) struct MigrationCoordinator {
    root: PathBuf,
    _coordination: File,
}

pub(crate) struct MigrationWriter {
    root: PathBuf,
    _writer: File,
}

/// Conservative encoded size check used before a migration publishes its
/// barrier. Payloads are counted in base64 while all journal paths/guards use
/// the real serializer. The bounded migration result fits in the 64 KiB reserve.
pub(crate) fn check_migration_capacity(
    repository: &RepositoryId,
    changes: &[(PathBuf, usize)],
    dependencies: &[PathBuf],
) -> Result<()> {
    let hash = ContentHash::of(&[]);
    let journal = Journal {
        schema_version: SchemaVersion::CURRENT,
        receipt: MutationReceipt {
            schema_version: SchemaVersion::CURRENT,
            repository: Some(repository.clone()),
            operation_id: "OP-00000000000000000000000000"
                .parse()
                .expect("fixed-width sizing ID"),
            request_id: "a".repeat(96).parse().expect("maximum request length"),
            operation: "migration.cutover".into(),
            input_hash: hash.clone(),
            result: Value::Null,
            changed: changes
                .iter()
                .map(|(path, _)| ChangedPath {
                    path: path.clone(),
                    before: Some(hash.clone()),
                    after: Some(hash.clone()),
                })
                .collect(),
        },
        changes: changes
            .iter()
            .map(|(path, _)| FileChange {
                path: path.clone(),
                expected: Some(hash.clone()),
                content: Some(Vec::new()),
            })
            .collect(),
        reads: dependencies
            .iter()
            .map(|path| ReadFingerprint {
                path: path.clone(),
                content: Some(hash.clone()),
                max_bytes: MAX_TRANSACTION_FILE_BYTES,
            })
            .collect(),
        listings: Vec::new(),
    };
    let payloads = changes
        .iter()
        .try_fold(0usize, |total, (_, length)| {
            total.checked_add(length.div_ceil(3).checked_mul(4)?)
        })
        .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "migration payload size overflow"))?;
    let overhead = encode(&journal, Path::new("migration journal sizing"))?.len();
    if payloads
        .checked_add(overhead)
        .and_then(|size| size.checked_add(64 * 1024))
        .is_none_or(|size| size > MAX_TRANSACTION_FILE_BYTES)
    {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "migration batch and dependency guards exceed the 64 MiB encoded journal limit; reduce the migration scope",
        ));
    }
    Ok(())
}

impl MigrationCoordinator {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        match fs::create_dir(root) {
            Ok(()) => sync_directory(
                root.parent()
                    .ok_or_else(|| unsafe_path(root, "migration destination requires a parent"))?,
            )?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(PmError::io(root, error)),
        }
        let root = checked_path(root, Path::new(""))?
            .canonicalize()
            .map_err(|error| PmError::io(root, error))?;
        ensure_directory(&root, Path::new(".tmp"))?;
        let path = checked_path(&root, Path::new(".tmp/migration.lock"))?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| PmError::io(&path, error))?;
        let start = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if start.elapsed() < MIGRATION_LOCK_TIMEOUT => {
                    std::thread::sleep(Duration::from_millis(5))
                }
                Err(TryLockError::WouldBlock) => {
                    return Err(PmError::new(
                        ErrorCode::Locked,
                        "another migration is active; retry without removing its lock",
                    )
                    .at(&path));
                }
                Err(TryLockError::Error(error)) => return Err(PmError::io(&path, error)),
            }
        }
        Ok(Self {
            root,
            _coordination: file,
        })
    }

    pub(crate) fn writer(&self) -> Result<MigrationWriter> {
        let store = TransactionStore {
            root: self.root.clone(),
            lock_timeout: Duration::from_secs(2),
            repository: None,
            migration: None,
            operation_limits: crate::ClaimCatalogLimits::default(),
        };
        let writer = store.acquire()?;
        crate::restore::check_root(&self.root)?;
        Ok(MigrationWriter {
            root: self.root.clone(),
            _writer: writer,
        })
    }
}

impl MigrationWriter {
    pub(crate) fn read(&self, path: &Path) -> Result<Option<Vec<u8>>> {
        read_file(&self.root, path)
    }
    pub(crate) fn request_receipt(&self, request: &RequestId) -> Result<Option<MutationReceipt>> {
        TransactionStore {
            root: self.root.clone(),
            lock_timeout: Duration::from_secs(2),
            repository: None,
            migration: None,
            operation_limits: crate::ClaimCatalogLimits::default(),
        }
        .find_request(request)
    }
    pub(crate) fn publish(
        &self,
        path: &Path,
        expected: Option<&ContentHash>,
        content: &[u8],
    ) -> Result<()> {
        TransactionStore {
            root: self.root.clone(),
            lock_timeout: Duration::from_secs(2),
            repository: None,
            migration: None,
            operation_limits: crate::ClaimCatalogLimits::default(),
        }
        .require_recovered()?;
        if !(migration_path(path)
            || path.starts_with(".tmp/migrations")
            || path == Path::new("config.yml"))
        {
            return Err(unsafe_path(
                path,
                "bootstrap may publish only migration state or initial configuration",
            ));
        }
        if path == Path::new("config.yml") && read_file(&self.root, path)?.is_some() {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "migration bootstrap cannot replace existing configuration",
            )
            .at(path));
        }
        if content.len() > MAX_TRANSACTION_FILE_BYTES {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "migration bootstrap file exceeds 64 MiB",
            )
            .at(path));
        }
        atomic_replace(&self.root, path, expected, Some(content))
    }
}

/// Restoration holds the normal writer inode for the complete bounded apply.
/// Only the restore protocol receives this create-only authority publisher;
/// ordinary transaction and receipt validation retain reserved-path checks.
pub(crate) struct RestoreWriter {
    root: PathBuf,
    _writer: File,
}
impl RestoreWriter {
    pub(crate) fn open(root: &Path) -> Result<Self> {
        match fs::create_dir(root) {
            Ok(()) => sync_directory(
                root.parent()
                    .ok_or_else(|| unsafe_path(root, "restoration requires a parent directory"))?,
            )?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {}
            Err(error) => return Err(PmError::io(root, error)),
        }
        let root = checked_path(root, Path::new(""))?
            .canonicalize()
            .map_err(|error| PmError::io(root, error))?;
        ensure_directory(&root, Path::new(".tmp"))?;
        Self::bootstrap_local_ignore(&root)?;
        let store = TransactionStore {
            root: root.clone(),
            lock_timeout: Duration::from_secs(2),
            repository: None,
            migration: None,
            operation_limits: crate::ClaimCatalogLimits::default(),
        };
        let writer = store.acquire()?;
        store.require_recovered()?;
        Ok(Self {
            root,
            _writer: writer,
        })
    }

    // A restore can reject its plan before publishing the root ignore policy.
    // Publish the private policy before writer.lock, without changing user rules
    // or requiring successful PM initialization. Concurrent equal bootstraps
    // converge on the same durable bytes before acquiring the shared writer.
    fn bootstrap_local_ignore(root: &Path) -> Result<()> {
        let path = Path::new(".tmp/.gitignore");
        let expected = b"*\n";
        match read_file_bounded(root, path, crate::documents::MAX_DOCUMENT_BYTES)? {
            Some(bytes) if bytes == expected => sync_existing(root, path),
            Some(_) => Err(PmError::new(
                ErrorCode::StaleSource,
                "restoration preserves conflicting existing local ignore rules",
            )
            .at(path)),
            None => match atomic_replace(root, path, None, Some(expected)) {
                Ok(()) => Ok(()),
                Err(error) => {
                    if read_file_bounded(root, path, crate::documents::MAX_DOCUMENT_BYTES)?
                        .as_deref()
                        == Some(expected)
                    {
                        sync_existing(root, path)
                    } else {
                        Err(error)
                    }
                }
            },
        }
    }
    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
    pub(crate) fn snapshot(&self) -> Snapshot<'_> {
        Snapshot::new(&self.root)
    }
    pub(crate) fn read(&self, path: &Path, limit: usize) -> Result<Option<Vec<u8>>> {
        read_file_bounded(&self.root, path, limit)
    }
    pub(crate) fn create(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        validate_relative(path, false)?;
        let local_bundle = path.components().count() == 4
            && path.starts_with(".tmp/restorations")
            && path.file_name().is_some_and(|name| name == "bundle.json")
            && path
                .parent()
                .and_then(Path::file_name)
                .and_then(|name| name.to_str())
                .is_some_and(|id| id.parse::<OperationId>().is_ok());
        if !local_bundle
            && path != Path::new("restore.yml")
            && path != Path::new(".gitignore")
            && path != Path::new(".tmp/.gitignore")
            && crate::snapshots::validation::classify(path)?.is_none()
        {
            return Err(unsafe_path(
                path,
                "restoration cannot publish application preferences or extension source",
            ));
        }
        if bytes.len() > MAX_TRANSACTION_FILE_BYTES {
            return Err(oversized_file(path, MAX_TRANSACTION_FILE_BYTES));
        }
        match read_file(&self.root, path)? {
            Some(current) if current == bytes => sync_existing(&self.root, path),
            Some(_) => Err(PmError::new(
                ErrorCode::StaleSource,
                "restoration never overwrites different existing bytes",
            )
            .at(path)),
            None => atomic_replace(&self.root, path, None, Some(bytes)),
        }
    }
    pub(crate) fn clear_barrier(&self, expected: &ContentHash) -> Result<()> {
        atomic_replace(&self.root, Path::new("restore.yml"), Some(expected), None)
    }
}

fn validate_operation(operation: &str) -> Result<()> {
    if operation.is_empty()
        || operation.len() > 128
        || !operation
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "operation name must contain 1–128 ASCII letters, digits, dots, dashes, or underscores",
        ));
    }
    Ok(())
}

fn validate_relative(path: &Path, allow_empty: bool) -> Result<()> {
    if allow_empty && path.as_os_str().is_empty() {
        return Ok(());
    }
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(unsafe_path(
            path,
            "path must be a nonempty relative path without traversal",
        ));
    }
    let Some(text) = path.to_str() else {
        return Err(unsafe_path(path, "planning paths must be UTF-8"));
    };
    if text.contains(['\\', ':', '\0'])
        || text
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(unsafe_path(
            path,
            "path contains a nonportable or ambiguous component",
        ));
    }
    Ok(())
}

fn reject_local(path: &Path) -> Result<()> {
    let first = path
        .components()
        .next()
        .and_then(|part| part.as_os_str().to_str())
        .map(str::to_ascii_lowercase);
    if first.as_deref().is_some_and(|first| {
        LOCAL_DIRS.contains(&first)
            || matches!(
                first,
                "settings.local.yml" | "config.local.yml" | "config.local.toml"
            )
    }) {
        return Err(unsafe_path(
            path,
            "machine-local state is not an authoritative planning path",
        ));
    }
    Ok(())
}

fn validate_authority_path(path: &Path) -> Result<()> {
    validate_relative(path, false)?;
    reject_local(path)?;
    let first = path
        .components()
        .next()
        .and_then(|part| part.as_os_str().to_str())
        .map(str::to_ascii_lowercase);
    if matches!(
        first.as_deref(),
        Some("operations" | ".git" | "restore.yml")
    ) {
        return Err(unsafe_path(
            path,
            "path is reserved for the transaction engine or Git",
        ));
    }
    Ok(())
}

fn validate_changes(changes: &[FileChange]) -> Result<()> {
    let mut paths = BTreeSet::new();
    let mut portable_paths = BTreeSet::new();
    for change in changes {
        validate_authority_path(&change.path)?;
        if portable_key(&change.path) == "coordination.yml" {
            return Err(PmError::new(ErrorCode::UnsafePath,
                "coordination markers belong to isolated Git coordination publication, not ordinary planning mutations").at(&change.path));
        }
        if change.content.is_none() && portable_key(&change.path) == "config.yml" {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "planning configuration cannot be deleted by a mutation",
            )
            .at(&change.path));
        }
        if change
            .content
            .as_ref()
            .is_some_and(|bytes| bytes.len() > MAX_TRANSACTION_FILE_BYTES)
        {
            return Err(
                PmError::new(ErrorCode::InvalidInput, "changed file exceeds 64 MiB")
                    .at(&change.path),
            );
        }
        if !paths.insert(&change.path) || !portable_paths.insert(portable_key(&change.path)) {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "change set contains duplicate paths",
            )
            .at(&change.path));
        }
        if change.expected.is_none() && change.content.is_none() {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "delete requires expected existing content",
            )
            .at(&change.path));
        }
    }
    for path in &portable_paths {
        if Path::new(path).ancestors().skip(1).any(|parent| {
            parent
                .to_str()
                .is_some_and(|parent| portable_paths.contains(parent))
        }) {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "a changed file is also another changed file's parent",
            )
            .at(path));
        }
    }
    Ok(())
}

fn portable_key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    validate_operation(&receipt.operation)?;
    let mut paths = BTreeSet::new();
    for change in &receipt.changed {
        validate_authority_path(&change.path)?;
        if !paths.insert(&change.path) || (change.before.is_none() && change.after.is_none()) {
            return Err(PmError::new(
                ErrorCode::CorruptStore,
                "receipt contains invalid or duplicate changes",
            )
            .at(&change.path));
        }
    }
    Ok(())
}

pub(crate) fn canonical_hash(value: &Value) -> Result<ContentHash> {
    fn sorted(value: &Value) -> Value {
        match value {
            Value::Object(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_by_key(|(left, _)| *left);
                Value::Object(
                    entries
                        .into_iter()
                        .map(|(key, value)| (key.clone(), sorted(value)))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.iter().map(sorted).collect()),
            value => value.clone(),
        }
    }
    serde_json::to_vec(&sorted(value))
        .map(|bytes| ContentHash::of(&bytes))
        .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))
}

fn receipt_path(operation: &OperationId) -> PathBuf {
    Path::new("operations").join(format!("{operation}.yml"))
}
fn journal_path(operation: &OperationId) -> PathBuf {
    Path::new(JOURNALS).join(format!("{operation}.yml"))
}

fn encode(value: &impl Serialize, path: &Path) -> Result<Vec<u8>> {
    serde_yaml_ng::to_string(value)
        .map(String::into_bytes)
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()).at(path))
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8], path: &Path) -> Result<T> {
    serde_yaml_ng::from_slice(bytes).map_err(|error| {
        PmError::new(
            ErrorCode::CorruptStore,
            format!("invalid transaction record: {error}"),
        )
        .at(path)
    })
}

fn unsafe_path(path: &Path, message: &str) -> PmError {
    PmError::new(ErrorCode::UnsafePath, message).at(path)
}

fn recovery_error(path: &Path, error: PmError) -> PmError {
    PmError::new(
        ErrorCode::RecoveryRequired,
        format!("published operation needs recovery: {error}"),
    )
    .at(path)
    .hint("Inspect the durable journal and run recovery; preserve conflicting external edits.")
}

#[cfg(test)]
thread_local! { static METADATA_OBSERVATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
fn observed_metadata(path: &Path) -> std::io::Result<fs::Metadata> {
    #[cfg(test)]
    METADATA_OBSERVATIONS.with(|count| count.set(count.get() + 1));
    fs::symlink_metadata(path)
}

/// Validate every existing component beneath the canonical root. This intentionally
/// does not claim to defeat malicious path swaps between inspection and opening.
fn checked_path(root: &Path, relative: &Path) -> Result<PathBuf> {
    checked_path_metadata(root, relative).map(|(path, _)| path)
}

fn checked_path_metadata(root: &Path, relative: &Path) -> Result<(PathBuf, Option<fs::Metadata>)> {
    validate_relative(relative, true)?;
    let mut path = root.to_owned();
    let root_metadata = observed_metadata(root).map_err(|error| PmError::io(root, error))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err(unsafe_path(
            root,
            "planning root is no longer a real directory",
        ));
    }
    let mut leaf = Some(root_metadata);
    let parts: Vec<_> = relative.components().collect();
    for (index, part) in parts.iter().enumerate() {
        path.push(part.as_os_str());
        match observed_metadata(&path) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(unsafe_path(&path, "symlink inside planning source"));
                }
                if index + 1 < parts.len() && !metadata.is_dir() {
                    return Err(unsafe_path(
                        &path,
                        "planning path parent is not a directory",
                    ));
                }
                if !metadata.is_dir() && !metadata.is_file() {
                    return Err(unsafe_path(&path, "special file inside planning source"));
                }
                leaf = Some(metadata);
            }
            Err(error) if error.kind() == ErrorKind::NotFound => leaf = None,
            Err(error) => return Err(PmError::io(&path, error)),
        }
    }
    Ok((path, leaf))
}

fn read_file(root: &Path, relative: &Path) -> Result<Option<Vec<u8>>> {
    read_file_bounded(root, relative, MAX_TRANSACTION_FILE_BYTES)
}

fn read_file_bounded(root: &Path, relative: &Path, max_bytes: usize) -> Result<Option<Vec<u8>>> {
    let (path, metadata) = checked_path_metadata(root, relative)?;
    match metadata {
        None => Ok(None),
        Some(metadata) if !metadata.is_file() => Err(unsafe_path(&path, "expected a regular file")),
        Some(metadata) => {
            if metadata.len() > max_bytes as u64 {
                return Err(oversized_file(&path, max_bytes));
            }
            let file = File::open(&path).map_err(|error| PmError::io(&path, error))?;
            let metadata = file.metadata().map_err(|error| PmError::io(&path, error))?;
            if !metadata.is_file() {
                return Err(unsafe_path(&path, "expected a regular file"));
            }
            if metadata.len() > max_bytes as u64 {
                return Err(oversized_file(&path, max_bytes));
            }
            let mut bytes = Vec::new();
            file.take(max_bytes as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|error| PmError::io(&path, error))?;
            if bytes.len() > max_bytes {
                return Err(oversized_file(&path, max_bytes));
            }
            Ok(Some(bytes))
        }
    }
}

fn oversized_file(path: &Path, limit: usize) -> PmError {
    PmError::new(
        ErrorCode::InvalidSchema,
        format!("planning file exceeds the {limit}-byte read limit"),
    )
    .at(path)
}

/// Compact redo payloads keep binary attachments within the journal limit. The
/// array form reads early local prototype journals; all new journals use base64.
mod content_encoding {
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(
        value: &Option<Vec<u8>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(bytes) => serializer.serialize_some(&format!("base64:{}", STANDARD.encode(bytes))),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<u8>>, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Encoded {
            Text(String),
            Legacy(Vec<u8>),
        }
        Option::<Encoded>::deserialize(deserializer)?
            .map(|encoded| match encoded {
                Encoded::Text(value) => {
                    let value = value
                        .strip_prefix("base64:")
                        .ok_or_else(|| D::Error::custom("unknown transaction content encoding"))?;
                    STANDARD.decode(value).map_err(D::Error::custom)
                }
                Encoded::Legacy(bytes) => Ok(bytes),
            })
            .transpose()
    }
}

fn list_files(root: &Path, prefix: &Path, skip_local: bool) -> Result<Vec<PathBuf>> {
    Ok(list_files_bounded(root, prefix, skip_local, MAX_SNAPSHOT_ENTRIES)?.files)
}

fn oversized_listing(prefix: &Path, max_entries: usize) -> PmError {
    PmError::new(
        ErrorCode::Unsupported,
        format!("planning directory exceeds the {max_entries}-entry traversal limit"),
    )
    .at(prefix)
}

fn list_files_bounded(
    root: &Path,
    prefix: &Path,
    skip_local: bool,
    max_entries: usize,
) -> Result<Listing> {
    let start = checked_path(root, prefix)?;
    match observed_metadata(&start) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return Ok(Listing {
                files: Vec::new(),
                entries: 0,
            });
        }
        Err(error) => return Err(PmError::io(&start, error)),
        Ok(metadata) if !metadata.is_dir() => {
            return Err(unsafe_path(&start, "a listing prefix must be a directory"));
        }
        Ok(_) => {}
    }
    let mut directories = vec![prefix.to_owned()];
    let mut files = Vec::new();
    let mut entries = 0usize;
    while let Some(directory) = directories.pop() {
        let path = checked_path(root, &directory)?;
        for entry in fs::read_dir(&path).map_err(|error| PmError::io(&path, error))? {
            let entry = entry.map_err(|error| PmError::io(&path, error))?;
            let relative = directory.join(entry.file_name());
            if skip_local && reject_local(&relative).is_err() {
                continue;
            }
            entries += 1;
            if entries > max_entries {
                return Err(oversized_listing(prefix, max_entries));
            }
            // The containing directory is checked before and after enumeration.
            // Entry names cannot traverse it; inspect each leaf without repeating
            // the same ancestor walk. Actual content reads independently validate
            // their complete paths, and Snapshot::validate repeats membership.
            validate_relative(&relative, false)?;
            let path = root.join(&relative);
            let metadata = observed_metadata(&path).map_err(|error| PmError::io(&path, error))?;
            if metadata.file_type().is_symlink() {
                return Err(unsafe_path(&path, "symlink inside planning source"));
            }
            if metadata.is_dir() {
                directories.push(relative);
            } else if metadata.is_file() {
                files.push(relative);
            } else {
                return Err(unsafe_path(&path, "special file inside planning source"));
            }
        }
        let (path, metadata) = checked_path_metadata(root, &directory)?;
        if metadata.is_none_or(|metadata| !metadata.is_dir()) {
            return Err(unsafe_path(
                &path,
                "listing directory changed during traversal",
            ));
        }
    }
    files.sort();
    Ok(Listing { files, entries })
}

fn require_hash(
    root: &Path,
    path: &Path,
    expected: &Option<ContentHash>,
    code: ErrorCode,
) -> Result<()> {
    let actual = read_file(root, path)?.as_deref().map(ContentHash::of);
    if &actual != expected {
        return Err(PmError::new(
            code,
            "content precondition failed; reload and reevaluate the change",
        )
        .at(path));
    }
    Ok(())
}

fn ensure_directory(root: &Path, relative: &Path) -> Result<()> {
    validate_relative(relative, true)?;
    let mut prefix = PathBuf::new();
    for part in relative.components() {
        prefix.push(part.as_os_str());
        let path = checked_path(root, &prefix)?;
        match fs::create_dir(&path) {
            Ok(()) => sync_directory(path.parent().expect("child directory has parent"))?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&path)
                    .map_err(|error| PmError::io(&path, error))?
                    .is_dir()
                {
                    return Err(unsafe_path(&path, "expected a directory"));
                }
            }
            Err(error) => return Err(PmError::io(&path, error)),
        }
    }
    Ok(())
}

fn atomic_replace(
    root: &Path,
    relative: &Path,
    expected: Option<&ContentHash>,
    content: Option<&[u8]>,
) -> Result<()> {
    require_hash(root, relative, &expected.cloned(), ErrorCode::StaleSource)?;
    let destination = checked_path(root, relative)?;
    if let Some(content) = content {
        ensure_directory(root, relative.parent().unwrap_or(Path::new("")))?;
        ensure_directory(root, Path::new(WRITES))?;
        let temporary_relative = Path::new(WRITES).join(format!("{}.tmp", OperationId::new()));
        let temporary = checked_path(root, &temporary_relative)?;
        let mut file =
            File::create_new(&temporary).map_err(|error| PmError::io(&temporary, error))?;
        let result = (|| {
            file.write_all(content)
                .map_err(|error| PmError::io(&temporary, error))?;
            file.sync_all()
                .map_err(|error| PmError::io(&temporary, error))?;
            drop(file);
            require_hash(root, relative, &expected.cloned(), ErrorCode::StaleSource)?;
            checked_path(root, relative)?;
            fs::rename(&temporary, &destination)
                .map_err(|error| PmError::io(&destination, error))?;
            sync_directory(destination.parent().expect("file has parent"))?;
            sync_directory(temporary.parent().expect("temporary file has parent"))?;
            Ok(())
        })();
        // Never delete a destination on failure. Orphaned local staging bytes are
        // disposable; an authoritative redo journal is removed only after commit.
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    } else {
        require_hash(root, relative, &expected.cloned(), ErrorCode::StaleSource)?;
        fs::remove_file(&destination).map_err(|error| PmError::io(&destination, error))?;
        sync_directory(destination.parent().expect("file has parent"))
    }
}

fn sync_existing(root: &Path, relative: &Path) -> Result<()> {
    let path = checked_path(root, relative)?;
    if read_file(root, relative)?.is_some() {
        File::open(&path)
            .and_then(|file| file.sync_all())
            .map_err(|error| PmError::io(&path, error))?;
    }
    sync_directory(path.parent().expect("file has parent"))
}

#[cfg(unix)]
fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| PmError::io(path, error))
}

#[cfg(not(unix))]
fn sync_directory(path: &Path) -> Result<()> {
    Err(PmError::new(ErrorCode::Unsupported,
        "durable PM mutations need a qualified parent-directory sync implementation on this platform").at(path))
}

#[cfg(test)]
mod bounded_listing_tests {
    use super::*;

    #[test]
    fn bounded_read_reuses_the_checked_leaf_metadata() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("record.md"), b"record").unwrap();
        METADATA_OBSERVATIONS.with(|count| count.set(0));
        let bytes = read_file_bounded(temp.path(), Path::new("record.md"), 6).unwrap();
        let observations = METADATA_OBSERVATIONS.with(|count| count.replace(0));
        assert_eq!(bytes, Some(b"record".to_vec()));
        assert_eq!(
            observations, 2,
            "the root and leaf need one metadata observation each"
        );
        assert!(read_file_bounded(temp.path(), Path::new("record.md"), 5).is_err());
    }

    #[test]
    fn flat_listing_does_not_recheck_every_ancestor_for_each_child() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("records");
        fs::create_dir(&directory).unwrap();
        for index in 0..128 {
            fs::write(directory.join(format!("record-{index:03}.md")), b"record").unwrap();
        }
        METADATA_OBSERVATIONS.with(|count| count.set(0));
        let listing = list_files_bounded(temp.path(), Path::new("records"), true, 128).unwrap();
        let observations = METADATA_OBSERVATIONS.with(|count| count.replace(0));
        assert_eq!(listing.files.len(), 128);
        assert!(
            observations <= 140,
            "128 flat records required {observations} metadata observations"
        );
    }

    #[test]
    fn filesystem_and_memory_listings_agree_on_nested_membership_and_exact_bounds() {
        let temp = tempfile::tempdir().unwrap();
        let files = BTreeMap::from([
            (PathBuf::from("records/a/b.md"), b"first".to_vec()),
            (PathBuf::from("records/a/deeper/c.md"), b"second".to_vec()),
            (PathBuf::from("records/last.md"), b"third".to_vec()),
            (PathBuf::from(".local/cache"), b"ignored".to_vec()),
        ]);
        for (path, bytes) in &files {
            fs::create_dir_all(temp.path().join(path).parent().unwrap()).unwrap();
            fs::write(temp.path().join(path), bytes).unwrap();
        }
        let memory = Snapshot::from_memory(temp.path(), &files);
        let native = Snapshot::new(temp.path());
        for (prefix, limit) in [
            ("records", 5),
            ("records/a", 3),
            ("records/a/deeper", 1),
            ("missing", 0),
        ] {
            let prefix = Path::new(prefix);
            assert_eq!(
                native.list_bounded(prefix, limit).unwrap(),
                memory.list_bounded(prefix, limit).unwrap()
            );
            if limit > 0 {
                assert_eq!(
                    native.list_bounded(prefix, limit - 1).unwrap_err().code,
                    ErrorCode::Unsupported
                );
                assert_eq!(
                    memory.list_bounded(prefix, limit - 1).unwrap_err().code,
                    ErrorCode::Unsupported
                );
            }
        }
        assert_eq!(
            native.list(Path::new("")).unwrap(),
            memory.list(Path::new("")).unwrap()
        );
        native.validate().unwrap();
    }

    #[test]
    fn memory_listing_counts_virtual_parents_and_respects_cached_limits() {
        let root = Path::new("not-a-host-source");
        let files = BTreeMap::from([
            (PathBuf::from("records/a/b.md"), b"first".to_vec()),
            (PathBuf::from("records/a/c.md"), b"second".to_vec()),
            (PathBuf::from(".local/not-authority"), b"local".to_vec()),
        ]);
        let snapshot = Snapshot::from_memory(root, &files);
        assert_eq!(
            snapshot
                .list_bounded(Path::new("records"), 3)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            snapshot
                .list_bounded(Path::new("records"), 2)
                .unwrap_err()
                .code,
            ErrorCode::Unsupported
        );
        assert_eq!(snapshot.list_bounded(Path::new(""), 4).unwrap().len(), 2);
        assert_eq!(
            snapshot.list_bounded(Path::new("absent"), 0).unwrap(),
            Vec::<PathBuf>::new()
        );
        assert_eq!(
            snapshot
                .list_bounded(Path::new(""), MAX_SNAPSHOT_ENTRIES + 1)
                .unwrap_err()
                .code,
            ErrorCode::InvalidInput
        );
    }
}
