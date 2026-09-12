//! Explicit, recoverable restoration of a complete native PM snapshot.
use crate::{
    ContentHash, NativeSnapshot, RepositoryId, RequestId, Result, SchemaVersion,
    transactions::{ChangedPath, MutationReceipt},
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRestorePlan {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub destination_root: PathBuf,
    pub snapshot: ContentHash,
    pub destination: ContentHash,
    pub fingerprint: ContentHash,
    pub allowed: bool,
    pub changes: Vec<ChangedPath>,
    pub blockers: Vec<crate::PmError>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotRestoreReceipt {
    pub plan: SnapshotRestorePlan,
    /// Complete restored-path manifest, including original operation receipts.
    /// The ordinary receipt excludes engine-owned operation paths.
    pub restored: Vec<ChangedPath>,
    pub receipt: MutationReceipt,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreFault {
    BeforeBarrier,
    AfterBarrier,
    BeforeChange(usize),
    AfterChange(usize),
    BeforeReceipt,
    AfterReceipt,
}

use crate::{
    ErrorCode, OperationId, PmError, SnapshotKind,
    snapshots::{MAX_SNAPSHOT_CONTENT_BYTES, MAX_SNAPSHOT_ENTRIES, MAX_SNAPSHOT_FILES, validation},
    transactions::{RestoreWriter, Snapshot, canonical_hash, validate_receipt},
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
const BARRIER: &str = "restore.yml";
const MAX_STAGE_BYTES: usize = crate::transactions::MAX_TRANSACTION_FILE_BYTES;
const MAX_DESTINATION_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Barrier {
    schema: SchemaVersion,
    operation: OperationId,
    request: RequestId,
    repository: RepositoryId,
    input_hash: ContentHash,
    stage: PathBuf,
    stage_hash: ContentHash,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    snapshot: NativeSnapshot,
    plan: SnapshotRestorePlan,
    expected_plan: Option<ContentHash>,
    before: BTreeMap<PathBuf, ContentHash>,
    receipt: MutationReceipt,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    plan: SnapshotRestorePlan,
    restored: Vec<ChangedPath>,
    source: SourceManifest,
    before: BTreeMap<PathBuf, ContentHash>,
    expected_plan: Option<ContentHash>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceManifest {
    format: String,
    version: SchemaVersion,
    files: Vec<SourceFile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceFile {
    kind: SnapshotKind,
    path: PathBuf,
    content: ContentHash,
    bytes: usize,
}
impl SourceManifest {
    fn from_snapshot(snapshot: &NativeSnapshot) -> Self {
        Self {
            format: snapshot.format.clone(),
            version: snapshot.version,
            files: snapshot
                .files
                .iter()
                .map(|file| SourceFile {
                    kind: file.kind,
                    path: file.path.clone(),
                    content: file.content_hash.clone(),
                    bytes: file.content.len(),
                })
                .collect(),
        }
    }
}

/// Pending restoration is an admission barrier even before config publication.
/// A corrupt barrier also requires explicit restoration recovery.
pub(crate) fn check_root(root: &Path) -> Result<()> {
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(PmError::io(root, error)),
        Ok(metadata) if !metadata.is_dir() || metadata.file_type().is_symlink() => {
            return Err(unsafe_path(
                root,
                "restoration source must be a real directory",
            ));
        }
        Ok(_) => {}
    }
    if Snapshot::new(root)
        .read_bounded(Path::new(BARRIER), crate::documents::MAX_DOCUMENT_BYTES)?
        .is_some()
    {
        return Err(pending(
            "native snapshot restoration is pending; partial authority is unavailable",
        )
        .at(root.join(BARRIER)));
    }
    Ok(())
}

pub fn preview_snapshot_restore(
    destination: &Path,
    input: &NativeSnapshot,
) -> Result<SnapshotRestorePlan> {
    let root = destination_root(destination)?;
    check_root(&root)?;
    let current = capture(&root)?;
    let (plan, _) = plan(&root, input, &current)?;
    // Validate source membership and bytes again without creating a lock area.
    if fingerprint(&capture(&root)?)? != fingerprint(&current)? {
        return Err(stale("restoration destination changed while previewing"));
    }
    if plan.allowed {
        ensure_file_shapes(&root, &destination_paths(input))?;
    }
    Ok(plan)
}
pub fn restore_snapshot(
    destination: &Path,
    input: &NativeSnapshot,
    expected_plan: Option<&ContentHash>,
    request: &RequestId,
) -> Result<SnapshotRestoreReceipt> {
    restore_snapshot_with_faults(destination, input, expected_plan, request, |_| Ok(()))
}
#[doc(hidden)]
pub fn restore_snapshot_with_faults(
    destination: &Path,
    input: &NativeSnapshot,
    expected_plan: Option<&ContentHash>,
    request: &RequestId,
    mut fault: impl FnMut(RestoreFault) -> Result<()>,
) -> Result<SnapshotRestoreReceipt> {
    let root = destination_root(destination)?;
    let intent = intent_hash(&root, input, expected_plan)?;
    // Invalid input must not bootstrap an absent source even as coordination state.
    if !root.exists() {
        input.validate()?;
    }
    let writer = RestoreWriter::open(&root)?;
    if let Some(bytes) = writer.read(Path::new(BARRIER), crate::documents::MAX_DOCUMENT_BYTES)? {
        let barrier: Barrier = decode(&bytes)?;
        if barrier.request != *request || barrier.input_hash != intent {
            return Err(conflict(
                "pending restoration belongs to another request or input",
            ));
        }
        let bundle = load_bundle(&writer, &barrier).map_err(as_pending)?;
        return run(&writer, &barrier, &bytes, &bundle, &mut fault).map_err(as_pending);
    }
    verify_repository_if_present(&writer, &input.repository)?;
    if let Some(receipt) = lookup_request(&writer.snapshot(), request)? {
        return replay(receipt, input, &intent, expected_plan);
    }
    let current = capture(writer.root())?;
    let (plan, before) = plan(writer.root(), input, &current)?;
    if expected_plan.is_some_and(|expected| expected != &plan.fingerprint) {
        return Err(stale(
            "restoration input or destination changed since preview",
        ));
    }
    if !plan.allowed {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "restoration has unresolved blockers",
        )
        .details(json!({"plan":plan})));
    }
    if input
        .files
        .iter()
        .filter(|file| file.kind == SnapshotKind::Operation)
        .any(|file| {
            serde_yaml_ng::from_slice::<MutationReceipt>(&file.content)
                .is_ok_and(|receipt| receipt.request_id == *request)
        })
    {
        return Err(conflict(
            "restoration request is already reserved by an incoming original receipt",
        ));
    }
    let operation = allocate_operation(input, &current)?;
    let outcome = Outcome {
        restored: plan.changes.clone(),
        plan: plan.clone(),
        source: SourceManifest::from_snapshot(input),
        before: before.clone(),
        expected_plan: expected_plan.cloned(),
    };
    let receipt = MutationReceipt {
        schema_version: SchemaVersion::CURRENT,
        repository: Some(input.repository.clone()),
        operation_id: operation.clone(),
        request_id: request.clone(),
        operation: "snapshot.restore".into(),
        input_hash: intent.clone(),
        result: json!(outcome),
        changed: application_changes(&plan.changes),
    };
    validate_restore_receipt(&receipt)?;
    let mut projected = authority(&current)?;
    for file in &input.files {
        projected.insert(file.path.clone(), file.content.clone());
    }
    projected.insert(receipt_path(&receipt.operation_id), encode(&receipt)?);
    validation::validate_files(&projected, &input.repository)?;
    let bundle = Bundle {
        snapshot: input.clone(),
        plan,
        expected_plan: expected_plan.cloned(),
        before,
        receipt,
    };
    let staged = serde_json::to_vec(&bundle).map_err(|error| invalid(error.to_string()))?;
    if staged.len() > MAX_STAGE_BYTES {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "restoration stage exceeds 64 MiB",
        ));
    }
    let stage = stage_path(&operation);
    let barrier = Barrier {
        schema: SchemaVersion::CURRENT,
        operation,
        request: request.clone(),
        repository: input.repository.clone(),
        input_hash: intent,
        stage,
        stage_hash: ContentHash::of(&staged),
    };
    let bytes = encode(&barrier)?;
    // Recheck private ignore policy before retaining recovery material; bootstrap
    // already published it before opening the writer lock.
    writer.create(Path::new(".tmp/.gitignore"), b"*\n")?;
    writer.create(&barrier.stage, &staged)?;
    fault(RestoreFault::BeforeBarrier)?;
    if fingerprint(&capture(writer.root())?)? != bundle.plan.destination {
        return Err(stale("restoration destination changed before publication"));
    }
    ensure_bundle_shapes(writer.root(), &bundle)?;
    let retained_stage = writer
        .read(&barrier.stage, MAX_STAGE_BYTES)?
        .ok_or_else(|| {
            stale("restoration stage disappeared before publication").at(&barrier.stage)
        })?;
    if ContentHash::of(&retained_stage) != barrier.stage_hash {
        return Err(stale("restoration stage changed before publication").at(&barrier.stage));
    }
    // Once the marker is durable, every ordinary reader/writer requires resume.
    if let Err(error) = writer.create(Path::new(BARRIER), &bytes) {
        return Err(
            if writer
                .read(Path::new(BARRIER), crate::documents::MAX_DOCUMENT_BYTES)?
                .is_some()
            {
                as_pending(error)
            } else {
                error
            },
        );
    }
    fault(RestoreFault::AfterBarrier).map_err(as_pending)?;
    run(&writer, &barrier, &bytes, &bundle, &mut fault).map_err(as_pending)
}

/// Resume from durable staged input, including a crash before config.yml exists.
pub fn resume_snapshot_restore(
    destination: &Path,
    request: &RequestId,
) -> Result<SnapshotRestoreReceipt> {
    let root = destination_root(destination)?;
    if !root.exists() {
        return Err(PmError::new(
            ErrorCode::NotFound,
            "no restoration exists at this destination",
        ));
    }
    let writer = RestoreWriter::open(&root)?;
    let Some(bytes) = writer.read(Path::new(BARRIER), crate::documents::MAX_DOCUMENT_BYTES)? else {
        let receipt = lookup_request(&writer.snapshot(), request)?.ok_or_else(|| {
            PmError::new(ErrorCode::NotFound, "restoration request was not found")
        })?;
        let returned = unpack(receipt)?;
        if returned.plan.destination_root != writer.root() {
            return Err(stale(
                "restoration receipt belongs to another destination; inspect its historical operation instead",
            ));
        }
        verify_repository_if_present(&writer, &returned.plan.repository)?;
        return Ok(returned);
    };
    let barrier: Barrier = decode(&bytes)?;
    if barrier.request != *request {
        return Err(conflict("resume request differs from pending restoration"));
    }
    let bundle = load_bundle(&writer, &barrier).map_err(as_pending)?;
    run(&writer, &barrier, &bytes, &bundle, &mut |_| Ok(())).map_err(as_pending)
}

fn run(
    writer: &RestoreWriter,
    barrier: &Barrier,
    marker_bytes: &[u8],
    bundle: &Bundle,
    fault: &mut impl FnMut(RestoreFault) -> Result<()>,
) -> Result<SnapshotRestoreReceipt> {
    verify_destination(writer, bundle, false)?;
    let mut files = bundle.snapshot.checked_files()?;
    if bundle
        .plan
        .changes
        .iter()
        .any(|change| change.path == Path::new(".gitignore"))
    {
        files.insert(".gitignore".into(), ignore_bytes());
    }
    for (index, change) in bundle.plan.changes.iter().enumerate() {
        fault(RestoreFault::BeforeChange(index))?;
        writer.create(
            &change.path,
            files
                .get(&change.path)
                .ok_or_else(|| corrupt("restore plan references missing staged content"))?,
        )?;
        fault(RestoreFault::AfterChange(index))?;
    }
    verify_destination(writer, bundle, true)?;
    validate_live(writer.root(), &bundle.snapshot.repository)?;
    fault(RestoreFault::BeforeReceipt)?;
    writer.create(&receipt_path(&barrier.operation), &encode(&bundle.receipt)?)?;
    fault(RestoreFault::AfterReceipt)?;
    verify_destination(writer, bundle, true)?;
    validate_live(writer.root(), &bundle.snapshot.repository)?;
    writer.clear_barrier(&ContentHash::of(marker_bytes))?;
    unpack(bundle.receipt.clone())
}

fn plan(
    root: &Path,
    input: &NativeSnapshot,
    current: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<(SnapshotRestorePlan, BTreeMap<PathBuf, ContentHash>)> {
    input.validate()?;
    let mut projected = authority(current)?;
    let mut changes = Vec::new();
    let mut blockers = file_shape_errors(root, &destination_paths(input));
    for file in &input.files {
        match current.get(&file.path) {
            Some(bytes) if bytes == &file.content => {}
            Some(_) => blockers.push(
                PmError::new(
                    ErrorCode::Conflict,
                    "restoration requires missing or byte-identical authority",
                )
                .at(&file.path),
            ),
            None => {
                projected.insert(file.path.clone(), file.content.clone());
                changes.push(ChangedPath {
                    path: file.path.clone(),
                    before: None,
                    after: Some(file.content_hash.clone()),
                });
            }
        }
    }
    if let Err(error) = validation::validate_files(&projected, &input.repository) {
        blockers.push(error);
    }
    if root.file_name().is_some_and(|name| name == ".workdeck")
        && root
            .parent()
            .is_some_and(crate::repository::has_legacy_store)
        && !projected.contains_key(Path::new("migration.yml"))
    {
        blockers.push(PmError::new(ErrorCode::LegacyStore,"legacy planning data requires a snapshot with accepted migration authority before native restoration"));
    }
    if let Some(bytes) = current.get(Path::new(".gitignore")) {
        let text = std::str::from_utf8(bytes)
            .map_err(|error| invalid(error.to_string()).at(".gitignore"))?;
        // A later negation can expose ignored state. Require the explicit
        // exclusions after every negation instead of guessing glob semantics.
        let last_negation = text
            .lines()
            .enumerate()
            .filter(|(_, line)| line.starts_with('!'))
            .map(|(index, _)| index)
            .last();
        let missing = crate::repository::IGNORE_ENTRIES
            .iter()
            .filter(|entry| {
                !text.lines().enumerate().any(|(index, line)| {
                    line == **entry && last_negation.is_none_or(|negation| index > negation)
                })
            })
            .copied()
            .collect::<Vec<_>>();
        if !missing.is_empty() {
            blockers.push(PmError::new(ErrorCode::PolicyBlocked,"existing planning ignore rules do not exclude required local state")
                .at(".gitignore").hint("Place the listed exclusions after any negated rules in .workdeck/.gitignore, then preview again; existing user rules are preserved.")
                .details(json!({"required_entries":missing})));
        }
    }
    if !current.contains_key(Path::new(".gitignore")) {
        changes.push(ChangedPath {
            path: ".gitignore".into(),
            before: None,
            after: Some(ContentHash::of(&ignore_bytes())),
        });
    }
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    let before = current
        .iter()
        .map(|(path, bytes)| (path.clone(), ContentHash::of(bytes)))
        .collect::<BTreeMap<_, _>>();
    let mut plan = SnapshotRestorePlan {
        schema: SchemaVersion::CURRENT,
        repository: input.repository.clone(),
        destination_root: root.into(),
        snapshot: input.fingerprint.clone(),
        destination: hash(&before)?,
        fingerprint: ContentHash::of(&[]),
        allowed: blockers.is_empty(),
        changes,
        blockers,
    };
    plan.fingerprint = plan_fingerprint(&plan)?;
    let estimate = MutationReceipt {
        schema_version: SchemaVersion::CURRENT,
        repository: Some(input.repository.clone()),
        operation_id: "OP-00000000000000000000000000"
            .parse()
            .expect("fixed-width estimate"),
        request_id: "r".repeat(96).parse().expect("maximum request length"),
        operation: "snapshot.restore".into(),
        input_hash: ContentHash::of(&[]),
        changed: application_changes(&plan.changes),
        result: json!(Outcome {
            plan: plan.clone(),
            restored: plan.changes.clone(),
            source: SourceManifest::from_snapshot(input),
            before: before.clone(),
            expected_plan: Some(plan.fingerprint.clone()),
        }),
    };
    let estimated_receipt = encode(&estimate)?.len();
    let content_bytes = projected.values().map(Vec::len).sum::<usize>();
    if projected.len() >= MAX_SNAPSHOT_FILES
        || content_bytes.saturating_add(estimated_receipt) > MAX_SNAPSHOT_CONTENT_BYTES
    {
        plan.allowed = false;
        plan.blockers.push(PmError::new(ErrorCode::Unsupported,"restored authority plus its durable restoration receipt exceeds snapshot file or content capacity"));
        plan.fingerprint = plan_fingerprint(&plan)?;
    }
    Ok((plan, before))
}
fn capture(root: &Path) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    if !root.exists() {
        return Ok(BTreeMap::new());
    }
    let snapshot = Snapshot::new(root);
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new(""), MAX_SNAPSHOT_ENTRIES)? {
        if path == Path::new(BARRIER) {
            continue;
        }
        let bytes = snapshot
            .read_bounded(&path, MAX_DESTINATION_BYTES - total)?
            .ok_or_else(|| stale("restoration destination file disappeared").at(&path))?;
        total += bytes.len();
        files.insert(path, bytes);
    }
    Ok(files)
}

// Directory membership is intentionally not added to durable authority hashes:
// old receipts remain valid and unrelated empty directories remain harmless.
// Missing files may be created only when every existing parent is a directory
// and every existing leaf is regular. The shared reader rejects symlinks and
// special files before opening, and all source traversal remains bounded.
fn destination_paths(input: &NativeSnapshot) -> Vec<PathBuf> {
    input
        .files
        .iter()
        .map(|file| file.path.clone())
        .chain([PathBuf::from(".gitignore"), PathBuf::from(BARRIER)])
        .collect()
}

fn file_shape_errors(root: &Path, paths: &[PathBuf]) -> Vec<PmError> {
    if !root.exists() {
        return Vec::new();
    }
    let snapshot = Snapshot::new(root);
    paths
        .iter()
        .filter_map(|path| snapshot.read_bounded(path, MAX_DESTINATION_BYTES).err())
        .collect()
}

fn ensure_file_shapes(root: &Path, paths: &[PathBuf]) -> Result<()> {
    if let Some(error) = file_shape_errors(root, paths).into_iter().next() {
        return Err(error);
    }
    Ok(())
}

fn ensure_bundle_shapes(root: &Path, bundle: &Bundle) -> Result<()> {
    let mut paths = destination_paths(&bundle.snapshot);
    paths.push(receipt_path(&bundle.receipt.operation_id));
    ensure_file_shapes(root, &paths)
}
fn authority(files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    files
        .iter()
        .filter_map(|(path, bytes)| match validation::classify(path) {
            Ok(Some(_)) => Some(Ok((path.clone(), bytes.clone()))),
            Ok(None) => None,
            Err(error) => Some(Err(error)),
        })
        .collect()
}
fn validate_live(root: &Path, repository: &RepositoryId) -> Result<()> {
    validation::validate_files(&authority(&capture(root)?)?, repository)
}
fn fingerprint(files: &BTreeMap<PathBuf, Vec<u8>>) -> Result<ContentHash> {
    hash(
        &files
            .iter()
            .map(|(path, bytes)| (path.clone(), ContentHash::of(bytes)))
            .collect::<BTreeMap<_, _>>(),
    )
}
fn plan_fingerprint(plan: &SnapshotRestorePlan) -> Result<ContentHash> {
    hash(&(
        &plan.schema,
        &plan.repository,
        &plan.destination_root,
        &plan.snapshot,
        &plan.destination,
        plan.allowed,
        &plan.changes,
        &plan.blockers,
    ))
}
fn intent_hash(
    root: &Path,
    input: &NativeSnapshot,
    expected_plan: Option<&ContentHash>,
) -> Result<ContentHash> {
    if input.files.len() > MAX_SNAPSHOT_FILES || input.format.len() > 128 {
        return Err(invalid("restoration exceeds 4096 files"));
    }
    let mut total = 0usize;
    let mut files = Vec::with_capacity(input.files.len());
    for file in &input.files {
        total = total
            .checked_add(file.content.len())
            .ok_or_else(|| invalid("restoration size overflow"))?;
        if total > MAX_SNAPSHOT_CONTENT_BYTES || file.path.as_os_str().len() > 1024 {
            return Err(invalid("restoration exceeds content or path bounds"));
        }
        files.push((
            &file.path,
            file.kind,
            &file.content_hash,
            ContentHash::of(&file.content),
            file.content.len(),
        ));
    }
    hash(&(
        root,
        &input.format,
        input.version,
        &input.repository,
        &input.fingerprint,
        files,
        expected_plan,
    ))
}
fn verify_destination(writer: &RestoreWriter, bundle: &Bundle, complete: bool) -> Result<()> {
    ensure_bundle_shapes(writer.root(), bundle)?;
    let current = capture(writer.root())?;
    let mut allowed = bundle.before.clone();
    for change in &bundle.plan.changes {
        if change.before.is_some() || change.after.is_none() {
            return Err(corrupt("restoration may only create authority"));
        }
        if let Some(bytes) = current.get(&change.path) {
            if Some(ContentHash::of(bytes)) != change.after {
                return Err(
                    stale("restoration destination contains conflicting bytes").at(&change.path)
                );
            }
        } else if complete {
            return Err(stale("restored file disappeared before completion").at(&change.path));
        }
        allowed.insert(change.path.clone(), change.after.clone().expect("checked"));
    }
    allowed.insert(
        receipt_path(&bundle.receipt.operation_id),
        ContentHash::of(&encode(&bundle.receipt)?),
    );
    for (path, hash) in &bundle.before {
        if current
            .get(path)
            .map(|bytes| ContentHash::of(bytes))
            .as_ref()
            != Some(hash)
        {
            return Err(stale("preserved destination file changed during restoration").at(path));
        }
    }
    for (path, bytes) in &current {
        if allowed.get(path) != Some(&ContentHash::of(bytes)) {
            return Err(stale(
                "unreviewed destination membership or content appeared during restoration",
            )
            .at(path));
        }
    }
    Ok(())
}
fn load_bundle(writer: &RestoreWriter, barrier: &Barrier) -> Result<Bundle> {
    if barrier.stage != stage_path(&barrier.operation) {
        return Err(pending("restoration barrier has an invalid stage path"));
    }
    let bytes = writer
        .read(&barrier.stage, MAX_STAGE_BYTES)?
        .ok_or_else(|| pending("durable restoration stage is missing"))?;
    if ContentHash::of(&bytes) != barrier.stage_hash {
        return Err(pending("durable restoration stage hash changed"));
    }
    let bundle: Bundle = serde_json::from_slice(&bytes)
        .map_err(|error| pending(format!("invalid restoration stage: {error}")))?;
    bundle.snapshot.validate()?;
    let intent = intent_hash(
        writer.root(),
        &bundle.snapshot,
        bundle.expected_plan.as_ref(),
    )?;
    if bundle.plan.destination_root != writer.root()
        || !bundle.plan.allowed
        || !bundle.plan.blockers.is_empty()
        || bundle.plan.repository != barrier.repository
        || bundle.snapshot.repository != barrier.repository
        || barrier.input_hash != intent
        || bundle.receipt.input_hash != intent
        || bundle.receipt.request_id != barrier.request
        || bundle.receipt.operation_id != barrier.operation
        || bundle.plan.snapshot != bundle.snapshot.fingerprint
        || bundle.plan.fingerprint != plan_fingerprint(&bundle.plan)?
        || bundle.plan.destination != hash(&bundle.before)?
        || bundle
            .expected_plan
            .as_ref()
            .is_some_and(|expected| expected != &bundle.plan.fingerprint)
    {
        return Err(pending(
            "restoration stage, marker, plan and receipt identities disagree",
        ));
    }
    let returned = replay(
        bundle.receipt.clone(),
        &bundle.snapshot,
        &intent,
        bundle.expected_plan.as_ref(),
    )?;
    if returned.plan != bundle.plan {
        return Err(pending(
            "restoration stage result differs from reviewed plan",
        ));
    }
    let mut expected = BTreeMap::new();
    for file in &bundle.snapshot.files {
        if let Some(before) = bundle.before.get(&file.path) {
            if before != &file.content_hash {
                return Err(pending(
                    "staged restoration would overwrite existing authority",
                ));
            }
        } else {
            expected.insert(file.path.clone(), file.content_hash.clone());
        }
    }
    if !bundle.before.contains_key(Path::new(".gitignore")) {
        expected.insert(".gitignore".into(), ContentHash::of(&ignore_bytes()));
    }
    if expected
        != bundle
            .plan
            .changes
            .iter()
            .map(|change| {
                (
                    change.path.clone(),
                    change.after.clone().expect("replay checked"),
                )
            })
            .collect()
    {
        return Err(pending(
            "restoration stage changed-path manifest is incomplete",
        ));
    }
    Ok(bundle)
}
fn lookup_request(snapshot: &Snapshot<'_>, request: &RequestId) -> Result<Option<MutationReceipt>> {
    let mut result = None;
    let mut requests = BTreeSet::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("operations"), MAX_SNAPSHOT_ENTRIES)? {
        let bytes = snapshot
            .read_bounded(&path, MAX_DESTINATION_BYTES - total)?
            .ok_or_else(|| corrupt("operation disappeared"))?;
        total += bytes.len();
        let receipt: MutationReceipt = serde_yaml_ng::from_slice(&bytes)
            .map_err(|error| corrupt(error.to_string()).at(&path))?;
        validate_receipt(&receipt)?;
        if path != receipt_path(&receipt.operation_id)
            || !requests.insert(receipt.request_id.clone())
        {
            return Err(corrupt("operation identity or unique request is invalid").at(path));
        }
        if &receipt.request_id == request {
            result = Some(receipt);
        }
    }
    Ok(result)
}
/// Validate a historical restoration result without rereading mutable target
/// files. Staging additionally checks every advertised path's actual bytes.
pub fn validate_restore_receipt(receipt: &MutationReceipt) -> Result<SnapshotRestoreReceipt> {
    unpack(receipt.clone())
}
fn unpack(receipt: MutationReceipt) -> Result<SnapshotRestoreReceipt> {
    validate_receipt(&receipt)?;
    if receipt.operation != "snapshot.restore" {
        return Err(conflict("request belongs to a different operation"));
    }
    let result: Outcome = serde_json::from_value(receipt.result.clone())
        .map_err(|error| corrupt(error.to_string()))?;
    if receipt.repository.as_ref() != Some(&result.plan.repository)
        || !result.plan.allowed
        || !result.plan.blockers.is_empty()
        || result.plan.fingerprint != plan_fingerprint(&result.plan)?
        || result.restored != result.plan.changes
        || receipt.changed != application_changes(&result.restored)
    {
        return Err(corrupt(
            "restoration receipt result and durable changes disagree",
        ));
    }
    validate_outcome_source(&result, &receipt.input_hash)?;
    let mut seen = BTreeSet::new();
    for change in &result.restored {
        if change.before.is_some()
            || change.after.is_none()
            || !seen.insert(change.path.to_string_lossy().to_lowercase())
        {
            return Err(corrupt("invalid restored-path manifest"));
        }
        if change.path != Path::new(".gitignore") && validation::classify(&change.path)?.is_none() {
            return Err(corrupt("restore manifest contains application preferences"));
        }
    }
    Ok(SnapshotRestoreReceipt {
        plan: result.plan,
        restored: result.restored,
        receipt,
    })
}
fn validate_outcome_source(result: &Outcome, input_hash: &ContentHash) -> Result<()> {
    if result.source.format != "workdeck.native-snapshot"
        || result.source.files.len() > MAX_SNAPSHOT_FILES
        || result.before.len() > MAX_SNAPSHOT_ENTRIES
        || hash(&result.before)? != result.plan.destination
        || result
            .expected_plan
            .as_ref()
            .is_some_and(|expected| expected != &result.plan.fingerprint)
    {
        return Err(corrupt(
            "restoration source manifest or destination fingerprint is invalid",
        ));
    }
    let mut paths = BTreeSet::new();
    let mut total = 0usize;
    let mut source_hashes = Vec::new();
    let mut intent_files = Vec::new();
    let mut expected = Vec::new();
    for file in &result.source.files {
        total = total
            .checked_add(file.bytes)
            .ok_or_else(|| corrupt("restoration manifest size overflow"))?;
        if total > MAX_SNAPSHOT_CONTENT_BYTES
            || file.bytes > validation::file_limit(file.kind)
            || validation::classify(&file.path)? != Some(file.kind)
            || !paths.insert(file.path.to_string_lossy().to_lowercase())
        {
            return Err(corrupt(
                "restoration source manifest contains invalid paths, kinds or sizes",
            ));
        }
        if file.kind == SnapshotKind::Operation {
            let id = file
                .path
                .file_stem()
                .and_then(|id| id.to_str())
                .ok_or_else(|| corrupt("restored operation path must be UTF-8"))?;
            if id.parse::<OperationId>().is_err() {
                return Err(corrupt("restored operation path has an invalid identity"));
            }
        }
        if let Some(before) = result.before.get(&file.path) {
            if before != &file.content {
                return Err(corrupt(
                    "restoration result would overwrite previous authority",
                ));
            }
        } else {
            expected.push(ChangedPath {
                path: file.path.clone(),
                before: None,
                after: Some(file.content.clone()),
            });
        }
        source_hashes.push((file.kind, &file.path, &file.content, file.bytes));
        intent_files.push((
            &file.path,
            file.kind,
            &file.content,
            &file.content,
            file.bytes,
        ));
    }
    if paths.iter().any(|path| {
        Path::new(path)
            .ancestors()
            .skip(1)
            .any(|parent| parent.to_str().is_some_and(|parent| paths.contains(parent)))
    }) {
        return Err(corrupt(
            "restoration source manifest file is also another file's parent",
        ));
    }
    if !paths.contains("config.yml")
        || result
            .source
            .files
            .windows(2)
            .any(|pair| pair[0].path >= pair[1].path)
        || hash(&(
            &result.source.format,
            result.source.version,
            &result.plan.repository,
            source_hashes,
        ))? != result.plan.snapshot
    {
        return Err(corrupt(
            "restoration source fingerprint differs from its ordered manifest",
        ));
    }
    for path in result.before.keys() {
        validation::classify(path)?;
    }
    if !result.before.contains_key(Path::new(".gitignore")) {
        expected.push(ChangedPath {
            path: ".gitignore".into(),
            before: None,
            after: Some(ContentHash::of(&ignore_bytes())),
        });
    }
    expected.sort_by(|a, b| a.path.cmp(&b.path));
    if expected != result.restored
        || hash(&(
            &result.plan.destination_root,
            &result.source.format,
            result.source.version,
            &result.plan.repository,
            &result.plan.snapshot,
            intent_files,
            result.expected_plan.as_ref(),
        ))? != *input_hash
    {
        return Err(corrupt(
            "restoration input hash or complete changed-path manifest differs from its result",
        ));
    }
    Ok(())
}
fn verify_repository_if_present(writer: &RestoreWriter, repository: &RepositoryId) -> Result<()> {
    if let Some(bytes) = writer.read(
        Path::new("config.yml"),
        crate::documents::MAX_DOCUMENT_BYTES,
    )? && crate::repository::parse_config(&writer.root().join("config.yml"), &bytes)?.repository
        != *repository
    {
        return Err(stale(
            "restoration destination repository identity differs from requested source",
        ));
    }
    Ok(())
}
fn replay(
    receipt: MutationReceipt,
    input: &NativeSnapshot,
    intent: &ContentHash,
    expected_plan: Option<&ContentHash>,
) -> Result<SnapshotRestoreReceipt> {
    if &receipt.input_hash != intent || receipt.operation != "snapshot.restore" {
        return Err(conflict(
            "restoration request identifies different input or operation",
        ));
    }
    let result = unpack(receipt)?;
    if result.plan.repository != input.repository
        || result.plan.snapshot != input.fingerprint
        || expected_plan.is_some_and(|expected| expected != &result.plan.fingerprint)
    {
        return Err(corrupt(
            "restoration receipt differs from requested snapshot or plan",
        ));
    }
    let files = input.checked_files()?;
    for change in &result.restored {
        let after = if change.path == Path::new(".gitignore") {
            Some(ContentHash::of(&ignore_bytes()))
        } else {
            files.get(&change.path).map(|bytes| ContentHash::of(bytes))
        };
        if after != change.after {
            return Err(corrupt(
                "restoration result refers to bytes outside requested snapshot",
            ));
        }
    }
    Ok(result)
}
fn allocate_operation(
    input: &NativeSnapshot,
    current: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<OperationId> {
    for _ in 0..16 {
        let id = OperationId::new();
        let path = receipt_path(&id);
        if !current.contains_key(&path) && input.files.iter().all(|file| file.path != path) {
            return Ok(id);
        }
    }
    Err(PmError::new(
        ErrorCode::Conflict,
        "could not allocate an unused restoration operation identity",
    ))
}
fn destination_root(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => path
            .canonicalize()
            .map_err(|error| PmError::io(path, error)),
        Ok(_) => Err(unsafe_path(
            path,
            "restoration destination must be a real directory",
        )),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let name = path.file_name().ok_or_else(|| {
                unsafe_path(path, "restoration destination needs a directory name")
            })?;
            let parent = destination_root(parent)?;
            if !parent.is_dir() {
                return Err(unsafe_path(
                    path,
                    "restoration destination parent must already exist",
                ));
            }
            Ok(parent.join(name))
        }
        Err(error) => Err(PmError::io(path, error)),
    }
}
fn stage_path(operation: &OperationId) -> PathBuf {
    format!(".tmp/restorations/{operation}/bundle.json").into()
}
fn receipt_path(operation: &OperationId) -> PathBuf {
    format!("operations/{operation}.yml").into()
}
fn ignore_bytes() -> Vec<u8> {
    format!("{}\n", crate::repository::IGNORE_ENTRIES.join("\n")).into_bytes()
}
fn application_changes(changes: &[ChangedPath]) -> Vec<ChangedPath> {
    changes
        .iter()
        .filter(|change| !change.path.starts_with("operations"))
        .cloned()
        .collect()
}
fn hash(value: &impl Serialize) -> Result<ContentHash> {
    canonical_hash(&serde_json::to_value(value).map_err(|error| invalid(error.to_string()))?)
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_yaml_ng::to_string(value)
        .map(String::into_bytes)
        .map_err(|error| invalid(error.to_string()))
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_yaml_ng::from_slice(bytes)
        .map_err(|error| pending(format!("invalid restoration marker: {error}")))
}
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn corrupt(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::CorruptStore, message)
}
fn conflict(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::IdempotencyConflict, message)
}
fn stale(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
fn unsafe_path(path: &Path, message: &str) -> PmError {
    PmError::new(ErrorCode::UnsafePath, message).at(path)
}
fn pending(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::RecoveryRequired,message).hint("Resume this native import with --restore --resume and its original request ID; preserve conflicting destination edits.")
}
fn as_pending(error: PmError) -> PmError {
    if error.code == ErrorCode::RecoveryRequired {
        return error;
    }
    pending(format!("published restoration requires recovery: {error}"))
        .details(json!({"cause":error}))
}
