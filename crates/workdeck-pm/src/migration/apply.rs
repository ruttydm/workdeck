use super::{MigrationDraft, MigrationPreview};
use crate::{
    ContentHash, ErrorCode, OperationId, PmError, RepositoryId, RequestId, Result, SchemaVersion,
    transactions::{
        ChangedPath, FaultPoint, FileChange, MigrationCoordinator, MigrationWriter,
        MutationReceipt, PreparedOperation, TransactionStore, canonical_hash,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const MARKER: &str = "migration.yml";
const MAX_DRAFT_BYTES: usize = 32 * 1024 * 1024;
const BATCH_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marker {
    schema: SchemaVersion,
    migration: OperationId,
    request: RequestId,
    repository: RepositoryId,
    preview: ContentHash,
    plan: PathBuf,
    plan_content: ContentHash,
    completion: Option<Completion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Completion {
    manifest: PathBuf,
    manifest_content: ContentHash,
    receipt: PathBuf,
    receipt_content: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: SchemaVersion,
    migration: OperationId,
    request: RequestId,
    repository: RepositoryId,
    preview: ContentHash,
    plan_content: ContentHash,
    changed: Vec<ChangedPath>,
    batches: Vec<OperationId>,
}

fn metadata_path(migration: &OperationId, name: &str) -> PathBuf {
    PathBuf::from(format!("migrations/{migration}/{name}"))
}
fn staged_path(migration: &OperationId, index: usize) -> PathBuf {
    PathBuf::from(format!(".tmp/migrations/{migration}/{index}.bin"))
}
fn recovery(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::RecoveryRequired, message)
        .hint("Resume the recorded migration; preserve conflicting source or destination edits.")
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_yaml_ng::to_string(value)
        .map(String::into_bytes)
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))
}
fn decode<T: serde::de::DeserializeOwned>(path: &Path, bytes: &[u8]) -> Result<T> {
    serde_yaml_ng::from_slice(bytes)
        .map_err(|error| recovery(format!("invalid migration record: {error}")).at(path))
}

pub(crate) fn check_root(root: &Path) -> Result<Option<RepositoryId>> {
    match std::fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(PmError::io(root, error)),
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(PmError::new(
                ErrorCode::UnsafePath,
                "planning root must be a real directory",
            )
            .at(root));
        }
        Ok(_) => {}
    }
    check_access(root, &crate::transactions::Snapshot::new(root), None)
}

pub(crate) fn check_access(
    root: &Path,
    snapshot: &crate::transactions::Snapshot<'_>,
    permit: Option<&OperationId>,
) -> Result<Option<RepositoryId>> {
    let Some(bytes) = snapshot.read(Path::new(MARKER))? else {
        if !snapshot.list(Path::new("migrations"))?.is_empty() {
            return Err(
                recovery("migration records exist without their admission marker")
                    .at(root.join(MARKER)),
            );
        }
        return Ok(None);
    };
    let marker: Marker = decode(&root.join(MARKER), &bytes)?;
    if marker.plan != metadata_path(&marker.migration, "plan.json") {
        return Err(recovery("migration marker has an invalid plan path").at(root.join(MARKER)));
    }
    if permit == Some(&marker.migration) {
        return Ok(Some(marker.repository));
    }
    let Some(completion) = &marker.completion else {
        return Err(recovery(
            "project management migration is pending; partial destination is unavailable",
        )
        .at(root.join(MARKER)));
    };
    let plan = snapshot
        .read(&marker.plan)?
        .ok_or_else(|| recovery("completed migration plan is missing"))?;
    if ContentHash::of(&plan) != marker.plan_content {
        return Err(
            recovery("completed migration plan content changed").at(root.join(&marker.plan))
        );
    }
    if completion.manifest != metadata_path(&marker.migration, "manifest.yml") {
        return Err(recovery("migration manifest path is invalid"));
    }
    let manifest_bytes = snapshot
        .read(&completion.manifest)?
        .ok_or_else(|| recovery("migration manifest is missing"))?;
    if ContentHash::of(&manifest_bytes) != completion.manifest_content {
        return Err(
            recovery("migration manifest content changed").at(root.join(&completion.manifest))
        );
    }
    let manifest: Manifest = decode(&completion.manifest, &manifest_bytes)?;
    if manifest.migration != marker.migration
        || manifest.request != marker.request
        || manifest.repository != marker.repository
        || manifest.preview != marker.preview
        || manifest.plan_content != marker.plan_content
    {
        return Err(recovery(
            "migration marker and manifest identities disagree",
        ));
    }
    let receipt_bytes = snapshot
        .read(&completion.receipt)?
        .ok_or_else(|| recovery("migration completion receipt is missing"))?;
    let receipt: MutationReceipt = decode(&completion.receipt, &receipt_bytes)?;
    if ContentHash::of(&receipt_bytes) != completion.receipt_content
        || completion.receipt.as_os_str()
            != format!("operations/{}.yml", receipt.operation_id).as_str()
        || receipt.operation != "migration.apply"
        || receipt.request_id != marker.request
        || receipt.repository.as_ref() != Some(&marker.repository)
    {
        return Err(recovery(
            "migration receipt identity or content does not match cutover",
        ));
    }
    if receipt.input_hash != canonical_hash(&primary_input(&marker))?
        || receipt.changed
            != vec![ChangedPath {
                path: completion.manifest.clone(),
                before: None,
                after: Some(completion.manifest_content.clone()),
            }]
    {
        return Err(recovery(
            "migration receipt does not publish the recorded manifest",
        ));
    }
    cutover_receipt(
        root,
        snapshot,
        &marker,
        ContentHash::of(&bytes),
        &receipt.operation_id,
    )?;
    let config = crate::repository::config_from_snapshot(root, snapshot)?;
    if config.repository != marker.repository {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "cutover repository identity differs from current configuration",
        )
        .at(root.join("config.yml")));
    }
    Ok(Some(marker.repository))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MigrationReceipt {
    pub schema: SchemaVersion,
    pub migration_id: OperationId,
    pub request_id: RequestId,
    pub repository: RepositoryId,
    pub preview_fingerprint: ContentHash,
    pub manifest_path: PathBuf,
    pub manifest_hash: ContentHash,
    pub cutover_operation: OperationId,
    pub cutover_hash: ContentHash,
    pub changed: Vec<ChangedPath>,
    pub batches: Vec<OperationId>,
    pub receipt: MutationReceipt,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationFault {
    BeforeBarrier,
    AfterBarrier,
    AfterBootstrap,
    Batch { index: usize, point: FaultPoint },
    AfterBatch(usize),
    BeforeManifest,
    Manifest(FaultPoint),
    BeforeCutover,
    Cutover(FaultPoint),
    AfterCutover,
}

pub fn apply(plan: &MigrationPreview, request: &RequestId) -> Result<MigrationReceipt> {
    apply_with_faults(plan, request, |_| Ok(()))
}

#[doc(hidden)]
pub fn apply_with_faults(
    plan: &MigrationPreview,
    request: &RequestId,
    mut fault: impl FnMut(MigrationFault) -> Result<()>,
) -> Result<MigrationReceipt> {
    if super::fingerprint(plan)? != plan.fingerprint {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "migration preview fingerprint does not match its contents",
        ));
    }
    let root = super::scan::destination_root(&plan.destination_root)?;
    let coordinator = MigrationCoordinator::open(&root)?;
    let marker = {
        let writer = coordinator.writer()?;
        if let Some(bytes) = writer.read(Path::new(MARKER))? {
            let marker: Marker = decode(Path::new(MARKER), &bytes)?;
            require_intent(&marker, request, Some(&plan.fingerprint))?;
            marker
        } else {
            check_root(&root)?;
            if writer.request_receipt(request)?.is_some() {
                return Err(PmError::new(
                    ErrorCode::IdempotencyConflict,
                    "migration request ID already identifies another operation",
                ));
            }
            validate_plan(plan)?;
            let current = super::preview(&plan.source_root, &root, &plan.options)?;
            if current.fingerprint != plan.fingerprint {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "migration inputs changed since the reviewed preview",
                ));
            }
            let migration = OperationId::new();
            let mut stored = plan.clone();
            for draft in &mut stored.drafts {
                draft.content.clear();
            }
            let plan_bytes = serde_json::to_vec(&stored)
                .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
            if plan_bytes.len() > MAX_DRAFT_BYTES {
                return Err(PmError::new(
                    ErrorCode::Unsupported,
                    "migration plan metadata exceeds 32 MiB; reduce the migration scope",
                ));
            }
            let marker = Marker {
                schema: SchemaVersion::CURRENT,
                migration: migration.clone(),
                request: request.clone(),
                repository: plan.options.config.repository.clone(),
                preview: plan.fingerprint.clone(),
                plan: metadata_path(&migration, "plan.json"),
                plan_content: ContentHash::of(&plan_bytes),
                completion: None,
            };
            let estimated = Manifest {
                schema: SchemaVersion::CURRENT,
                migration: migration.clone(),
                request: request.clone(),
                repository: marker.repository.clone(),
                preview: marker.preview.clone(),
                plan_content: marker.plan_content.clone(),
                changed: changes(plan),
                batches: vec![migration.clone(); batches(plan).len()],
            };
            if encode(&estimated)?.len() > MAX_DRAFT_BYTES {
                return Err(PmError::new(
                    ErrorCode::Unsupported,
                    "migration manifest exceeds its bounded journal capacity",
                ));
            }
            writer.publish(&staged_plan_path(&migration), None, &plan_bytes)?;
            for (index, draft) in plan.drafts.iter().enumerate() {
                writer.publish(&staged_path(&migration, index), None, &draft.content)?;
            }
            fault(MigrationFault::BeforeBarrier)?;
            verify_source(plan)?;
            verify_destination(plan, &writer, &BTreeSet::new(), &BTreeSet::new())?;
            writer.publish(Path::new(MARKER), None, &encode(&marker)?)?;
            marker
        }
    };
    if let Some(receipt) = completed_replay(&root, &marker)? {
        return Ok(receipt);
    }
    fault(MigrationFault::AfterBarrier).map_err(pending_error)?;
    run(&coordinator, &root, &marker, plan, &mut fault).map_err(pending_error)
}

pub fn resume(destination: &Path, request: &RequestId) -> Result<MigrationReceipt> {
    let root = super::scan::destination_root(destination)?;
    // Inspection does not create a new migration for an absent marker.
    let bytes = super::scan::destination_file(&root, Path::new(MARKER))?.ok_or_else(|| {
        PmError::new(
            ErrorCode::NotFound,
            "no migration is recorded at this destination",
        )
    })?;
    let _: Marker = decode(Path::new(MARKER), &bytes)?;
    let coordinator = MigrationCoordinator::open(&root)?;
    let marker = {
        let writer = coordinator.writer()?;
        let bytes = writer.read(Path::new(MARKER))?.ok_or_else(|| {
            recovery("migration marker disappeared while waiting for coordination")
        })?;
        decode::<Marker>(Path::new(MARKER), &bytes)?
    };
    require_intent(&marker, request, None)?;
    if let Some(receipt) = completed_replay(&root, &marker)? {
        return Ok(receipt);
    }
    let plan = {
        let writer = coordinator.writer()?;
        load_plan(&writer, &marker)?
    };
    run(&coordinator, &root, &marker, &plan, &mut |_| Ok(())).map_err(pending_error)
}

fn pending_error(error: PmError) -> PmError {
    if error.code == ErrorCode::RecoveryRequired {
        error
    } else {
        recovery(format!("migration remains pending: {}", error.message))
            .at(error.path.unwrap_or_else(|| MARKER.into()))
    }
}
fn staged_plan_path(migration: &OperationId) -> PathBuf {
    PathBuf::from(format!(".tmp/migrations/{migration}/plan.json"))
}
fn require_intent(
    marker: &Marker,
    request: &RequestId,
    preview: Option<&ContentHash>,
) -> Result<()> {
    if &marker.request != request || preview.is_some_and(|preview| preview != &marker.preview) {
        return Err(PmError::new(
            ErrorCode::IdempotencyConflict,
            "destination migration belongs to a different request or preview",
        ));
    }
    Ok(())
}
fn validate_plan(plan: &MigrationPreview) -> Result<()> {
    if !plan.complete || !plan.blockers.is_empty() {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "migration requires a complete preview without blocking diagnostics",
        ));
    }
    plan.options.config.validate()?;
    for draft in &plan.drafts {
        if draft.content.len() > MAX_DRAFT_BYTES
            || ContentHash::of(&draft.content) != draft.content_hash
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "migration draft is oversized or disagrees with its content hash",
            )
            .at(&draft.destination_path));
        }
    }
    validate_publication_sizes(plan)
}

pub(super) fn validate_publication_sizes(plan: &MigrationPreview) -> Result<()> {
    let mut stored = plan.clone();
    for draft in &mut stored.drafts {
        draft.content.clear();
    }
    let metadata = serde_json::to_vec(&stored)
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
    if metadata.len() > MAX_DRAFT_BYTES {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "migration plan metadata exceeds 32 MiB; reduce the migration scope",
        ));
    }
    // Maximum request length, fixed-width IDs and hashes: the resulting YAML
    // manifest is itself the only payload in its transaction. The 32 MiB cap
    // leaves at least 20 MiB below the journal limit after base64 and guards.
    let id: OperationId = "OP-00000000000000000000000000"
        .parse()
        .expect("valid fixed-width sizing identity");
    let manifest = Manifest {
        schema: SchemaVersion::CURRENT,
        migration: id.clone(),
        request: "a".repeat(96).parse().expect("maximum request length"),
        repository: plan.options.config.repository.clone(),
        preview: plan.fingerprint.clone(),
        plan_content: ContentHash::of(&metadata),
        changed: changes(plan),
        batches: vec![id; batches(plan).len()],
    };
    let manifest_bytes = encode(&manifest)?;
    if manifest_bytes.len() > MAX_DRAFT_BYTES {
        return Err(PmError::new(
            ErrorCode::Unsupported,
            "migration manifest exceeds 32 MiB; reduce the migration scope",
        ));
    }
    let mut dependencies = plan
        .destination_inventory
        .iter()
        .map(|entry| entry.path.clone())
        .collect::<Vec<_>>();
    dependencies.extend([PathBuf::from("config.yml"), PathBuf::from(MARKER)]);
    dependencies.sort();
    dependencies.dedup();
    for batch in batches(plan) {
        let changes = batch
            .iter()
            .map(|index| {
                let draft = &plan.drafts[*index];
                (draft.destination_path.clone(), draft.content.len())
            })
            .collect::<Vec<_>>();
        crate::transactions::check_migration_capacity(
            &plan.options.config.repository,
            &changes,
            &dependencies,
        )?;
    }
    crate::transactions::check_migration_capacity(
        &plan.options.config.repository,
        &[(
            metadata_path(&manifest.migration, "manifest.yml"),
            manifest_bytes.len(),
        )],
        &dependencies,
    )?;
    crate::transactions::check_migration_capacity(
        &plan.options.config.repository,
        &[(MARKER.into(), 4096)],
        &dependencies,
    )?;
    Ok(())
}
fn load_plan(writer: &MigrationWriter, marker: &Marker) -> Result<MigrationPreview> {
    let bytes = match writer.read(&marker.plan)? {
        Some(bytes) => Some(bytes),
        None => writer.read(&staged_plan_path(&marker.migration))?,
    }
    .ok_or_else(|| recovery("persisted migration plan is missing"))?;
    if ContentHash::of(&bytes) != marker.plan_content {
        return Err(recovery("persisted migration plan has changed"));
    }
    let mut plan: MigrationPreview = serde_json::from_slice(&bytes)
        .map_err(|error| recovery(format!("invalid persisted preview: {error}")))?;
    for (index, draft) in plan.drafts.iter_mut().enumerate() {
        draft.content = writer
            .read(&staged_path(&marker.migration, index))?
            .ok_or_else(|| recovery("staged migration payload is missing"))?;
    }
    validate_plan(&plan)?;
    if super::fingerprint(&plan)? != marker.preview
        || plan.options.config.repository != marker.repository
    {
        return Err(recovery(
            "persisted migration context does not match its marker",
        ));
    }
    Ok(plan)
}
fn before(plan: &MigrationPreview, path: &Path) -> Option<ContentHash> {
    plan.destination_inventory
        .iter()
        .find(|entry| entry.path == path)
        .and_then(|entry| entry.content_hash.clone())
}
fn changes(plan: &MigrationPreview) -> Vec<ChangedPath> {
    plan.drafts
        .iter()
        .map(|draft| ChangedPath {
            path: draft.destination_path.clone(),
            before: before(plan, &draft.destination_path),
            after: Some(draft.content_hash.clone()),
        })
        .collect()
}
fn batches(plan: &MigrationPreview) -> Vec<Vec<usize>> {
    let mut result = Vec::new();
    let mut batch = Vec::new();
    let mut bytes = 0usize;
    for (index, draft) in plan.drafts.iter().enumerate() {
        if draft.destination_path == Path::new("config.yml") {
            continue;
        }
        if !batch.is_empty() && (bytes + draft.content.len() > BATCH_BYTES || batch.len() >= 128) {
            result.push(batch);
            batch = Vec::new();
            bytes = 0;
        }
        batch.push(index);
        bytes += draft.content.len();
    }
    if !batch.is_empty() {
        result.push(batch);
    }
    result
}
fn subrequest(marker: &Marker, suffix: &str) -> RequestId {
    format!("migration_{}_{}", marker.migration, suffix)
        .parse()
        .expect("bounded generated request ID")
}
fn batch_input(marker: &Marker, index: usize) -> serde_json::Value {
    json!({"batch":index,"migration":marker.migration,"preview":marker.preview})
}
fn primary_input(marker: &Marker) -> serde_json::Value {
    json!({"preview_fingerprint":marker.preview})
}
fn file_change(plan: &MigrationPreview, draft: &MigrationDraft) -> FileChange {
    FileChange {
        path: draft.destination_path.clone(),
        expected: before(plan, &draft.destination_path),
        content: Some(draft.content.clone()),
    }
}

fn verify_source(plan: &MigrationPreview) -> Result<()> {
    if super::scan::source_root(&plan.source_root)? != plan.source_root {
        return Err(recovery("legacy root identity changed"));
    }
    let current = super::scan::capture(&plan.source_root)?;
    if !current.errors.is_empty()
        || current.directories != plan.directories
        || current.files.len() != plan.inventory.len()
        || current
            .files
            .iter()
            .zip(&plan.inventory)
            .any(|(actual, expected)| {
                actual.path != expected.path
                    || actual.hash != expected.content_hash
                    || actual.size != expected.size
                    || actual.kind != expected.kind
            })
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "legacy source bytes or membership differ from the reviewed migration",
        )
        .at(&plan.source_root));
    }
    Ok(())
}
fn verify_destination(
    plan: &MigrationPreview,
    writer: &MigrationWriter,
    committed: &BTreeSet<PathBuf>,
    pending: &BTreeSet<PathBuf>,
) -> Result<()> {
    verify_destination_with(plan, committed, pending, |path| writer.read(path))
}
fn verify_destination_with(
    plan: &MigrationPreview,
    committed: &BTreeSet<PathBuf>,
    pending: &BTreeSet<PathBuf>,
    read: impl Fn(&Path) -> Result<Option<Vec<u8>>>,
) -> Result<()> {
    for original in &plan.destination_inventory {
        let current = read(&original.path)?.as_deref().map(ContentHash::of);
        let after = plan
            .drafts
            .iter()
            .find(|draft| draft.destination_path == original.path)
            .map(|draft| draft.content_hash.clone());
        let valid = if committed.contains(&original.path) {
            current == after
        } else if pending.contains(&original.path) {
            current == original.content_hash || current == after
        } else {
            current == original.content_hash
        };
        if !valid {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "migration destination differs from its recorded before/after state",
            )
            .at(plan.destination_root.join(&original.path)));
        }
    }
    Ok(())
}
fn known_batches(
    plan: &MigrationPreview,
    marker: &Marker,
    writer: &MigrationWriter,
) -> Result<BTreeSet<PathBuf>> {
    let mut committed = BTreeSet::new();
    if plan
        .drafts
        .iter()
        .any(|draft| draft.destination_path == Path::new("config.yml"))
    {
        committed.insert(PathBuf::from("config.yml"));
    }
    for (index, batch) in batches(plan).iter().enumerate() {
        if let Some(receipt) =
            writer.request_receipt(&subrequest(marker, &format!("batch_{index}")))?
        {
            let expected = batch
                .iter()
                .map(|index| {
                    let draft = &plan.drafts[*index];
                    ChangedPath {
                        path: draft.destination_path.clone(),
                        before: before(plan, &draft.destination_path),
                        after: Some(draft.content_hash.clone()),
                    }
                })
                .collect::<Vec<_>>();
            if receipt.operation != "migration.batch"
                || receipt.repository.as_ref() != Some(&marker.repository)
                || receipt.input_hash != canonical_hash(&batch_input(marker, index))?
                || receipt.changed != expected
            {
                return Err(recovery(
                    "published migration batch receipt conflicts with the reviewed plan",
                ));
            }
            committed.extend(receipt.changed.into_iter().map(|change| change.path));
        }
    }
    Ok(committed)
}

fn validate_pending_receipt(
    plan: &MigrationPreview,
    marker: &Marker,
    writer: &MigrationWriter,
    receipt: &MutationReceipt,
) -> Result<()> {
    let checked = |operation: &str,
                   input: serde_json::Value,
                   changed: Vec<ChangedPath>,
                   result: serde_json::Value|
     -> Result<()> {
        if receipt.repository.as_ref() != Some(&marker.repository)
            || receipt.operation != operation
            || receipt.input_hash != canonical_hash(&input)?
            || receipt.changed != changed
            || receipt.result != result
        {
            return Err(recovery(
                "unfinished migration journal does not match the exact recorded operation intent",
            ));
        }
        Ok(())
    };
    for (index, batch) in batches(plan).iter().enumerate() {
        if receipt.request_id == subrequest(marker, &format!("batch_{index}")) {
            let changed = batch
                .iter()
                .map(|index| {
                    let draft = &plan.drafts[*index];
                    ChangedPath {
                        path: draft.destination_path.clone(),
                        before: before(plan, &draft.destination_path),
                        after: Some(draft.content_hash.clone()),
                    }
                })
                .collect();
            return checked(
                "migration.batch",
                batch_input(marker, index),
                changed,
                batch_input(marker, index),
            );
        }
    }
    if receipt.request_id == marker.request {
        let mut ids = Vec::new();
        for index in 0..batches(plan).len() {
            let batch = writer
                .request_receipt(&subrequest(marker, &format!("batch_{index}")))?
                .ok_or_else(|| recovery("migration manifest has an unpublished batch"))?;
            ids.push(batch.operation_id);
        }
        let manifest = Manifest {
            schema: SchemaVersion::CURRENT,
            migration: marker.migration.clone(),
            request: marker.request.clone(),
            repository: marker.repository.clone(),
            preview: marker.preview.clone(),
            plan_content: marker.plan_content.clone(),
            changed: changes(plan),
            batches: ids,
        };
        let path = metadata_path(&marker.migration, "manifest.yml");
        let hash = ContentHash::of(&encode(&manifest)?);
        return checked(
            "migration.apply",
            primary_input(marker),
            vec![ChangedPath {
                path: path.clone(),
                before: None,
                after: Some(hash.clone()),
            }],
            json!({"migration":marker.migration,"preview":marker.preview,"manifest":path,"manifest_content":hash}),
        );
    }
    if receipt.request_id == subrequest(marker, "cutover") {
        let primary = writer
            .request_receipt(&marker.request)?
            .ok_or_else(|| recovery("cutover journal has no durable migration manifest receipt"))?;
        validate_pending_receipt(plan, marker, writer, &primary)?;
        let manifest = metadata_path(&marker.migration, "manifest.yml");
        let hash = primary.changed[0]
            .after
            .clone()
            .expect("validated manifest publication");
        let pending = Marker {
            completion: None,
            ..marker.clone()
        };
        let complete = Marker {
            completion: Some(Completion {
                manifest: manifest.clone(),
                manifest_content: hash.clone(),
                receipt: PathBuf::from(format!("operations/{}.yml", primary.operation_id)),
                receipt_content: ContentHash::of(&encode(&primary)?),
            }),
            ..pending.clone()
        };
        return checked(
            "migration.cutover",
            json!({"manifest_content":hash,"migration":marker.migration,"receipt":primary.operation_id}),
            vec![ChangedPath {
                path: MARKER.into(),
                before: Some(ContentHash::of(&encode(&pending)?)),
                after: Some(ContentHash::of(&encode(&complete)?)),
            }],
            json!({"migration":marker.migration,"manifest":manifest}),
        );
    }
    Err(recovery(
        "unfinished operation belongs to a different migration intent",
    ))
}

fn run(
    coordinator: &MigrationCoordinator,
    root: &Path,
    marker: &Marker,
    plan: &MigrationPreview,
    fault: &mut impl FnMut(MigrationFault) -> Result<()>,
) -> Result<MigrationReceipt> {
    verify_source(plan)?;
    {
        let writer = coordinator.writer()?;
        let stored = load_plan(&writer, marker)?;
        if stored != *plan {
            return Err(recovery(
                "supplied preview differs from the durable migration plan",
            ));
        }
        if writer.read(&marker.plan)?.is_none() {
            let bytes = writer
                .read(&staged_plan_path(&marker.migration))?
                .ok_or_else(|| recovery("staged plan disappeared"))?;
            writer.publish(&marker.plan, None, &bytes)?;
        }
        if writer.read(Path::new("config.yml"))?.is_none() {
            let config = plan
                .drafts
                .iter()
                .find(|draft| draft.destination_path == Path::new("config.yml"))
                .ok_or_else(|| recovery("original destination configuration disappeared"))?;
            writer.publish(Path::new("config.yml"), None, &config.content)?;
        }
    }
    fault(MigrationFault::AfterBootstrap)?;
    let store = TransactionStore::open(root)?
        .for_repository(marker.repository.clone())
        .for_migration(marker.migration.clone());
    let pending = store.pending_operations()?;
    {
        let writer = coordinator.writer()?;
        let committed = known_batches(plan, marker, &writer)?;
        for operation in &pending {
            if !operation.recoverable {
                return Err(recovery(
                    "an unfinished operation has conflicting dependencies or destination bytes",
                ));
            }
            validate_pending_receipt(plan, marker, &writer, &operation.receipt)?;
        }
        let pending_paths = pending
            .iter()
            .flat_map(|operation| {
                operation
                    .receipt
                    .changed
                    .iter()
                    .map(|change| change.path.clone())
            })
            .collect();
        verify_destination(plan, &writer, &committed, &pending_paths)?;
    }
    if !pending.is_empty() {
        verify_source(plan)?;
        store.recover()?;
    }
    let mut batch_ids = Vec::new();
    for (index, batch) in batches(plan).iter().enumerate() {
        verify_source(plan)?;
        let committed = {
            let writer = coordinator.writer()?;
            let committed = known_batches(plan, marker, &writer)?;
            verify_destination(plan, &writer, &committed, &BTreeSet::new())?;
            committed
        };
        let receipt = store.transact_with_faults(
            &subrequest(marker, &format!("batch_{index}")),
            "migration.batch",
            &batch_input(marker, index),
            |snapshot| {
                verify_destination_with(plan, &committed, &BTreeSet::new(), |path| {
                    snapshot.read(path)
                })?;
                Ok(PreparedOperation {
                    changes: batch
                        .iter()
                        .map(|index| file_change(plan, &plan.drafts[*index]))
                        .collect(),
                    result: batch_input(marker, index),
                })
            },
            |point| {
                fault(MigrationFault::Batch { index, point })?;
                if matches!(point, FaultPoint::BeforeJournal | FaultPoint::BeforeReceipt) {
                    verify_source(plan)?;
                }
                Ok(())
            },
        )?;
        batch_ids.push(receipt.operation_id);
        fault(MigrationFault::AfterBatch(index))?;
    }
    fault(MigrationFault::BeforeManifest)?;
    verify_source(plan)?;
    {
        let writer = coordinator.writer()?;
        let committed = plan
            .drafts
            .iter()
            .map(|draft| draft.destination_path.clone())
            .collect();
        verify_destination(plan, &writer, &committed, &BTreeSet::new())?;
    }
    let all_committed = plan
        .drafts
        .iter()
        .map(|draft| draft.destination_path.clone())
        .collect();
    let manifest = Manifest {
        schema: SchemaVersion::CURRENT,
        migration: marker.migration.clone(),
        request: marker.request.clone(),
        repository: marker.repository.clone(),
        preview: marker.preview.clone(),
        plan_content: marker.plan_content.clone(),
        changed: changes(plan),
        batches: batch_ids,
    };
    let manifest_path = metadata_path(&marker.migration, "manifest.yml");
    let manifest_bytes = encode(&manifest)?;
    let manifest_hash = ContentHash::of(&manifest_bytes);
    let receipt=store.transact_with_faults(&marker.request,"migration.apply",&primary_input(marker),|snapshot|{verify_destination_with(plan,&all_committed,&BTreeSet::new(),|path|snapshot.read(path))?;Ok(PreparedOperation{changes:vec![FileChange{path:manifest_path.clone(),expected:None,content:Some(manifest_bytes.clone())}],result:json!({"migration":marker.migration,"preview":marker.preview,"manifest":manifest_path,"manifest_content":manifest_hash})})},|point|{
        fault(MigrationFault::Manifest(point))?;
        if matches!(point,FaultPoint::BeforeJournal|FaultPoint::BeforeReceipt) {verify_source(plan)?;}
        Ok(())
    })?;
    fault(MigrationFault::BeforeCutover)?;
    verify_source(plan)?;
    {
        let writer = coordinator.writer()?;
        let committed = plan
            .drafts
            .iter()
            .map(|draft| draft.destination_path.clone())
            .collect();
        verify_destination(plan, &writer, &committed, &BTreeSet::new())?;
    }
    let complete = Marker {
        completion: Some(Completion {
            manifest: manifest_path.clone(),
            manifest_content: manifest_hash.clone(),
            receipt: PathBuf::from(format!("operations/{}.yml", receipt.operation_id)),
            receipt_content: ContentHash::of(&encode(&receipt)?),
        }),
        ..marker.clone()
    };
    let complete_bytes = encode(&complete)?;
    store.transact_with_faults(&subrequest(marker,"cutover"),"migration.cutover",&json!({"manifest_content":manifest_hash,"migration":marker.migration,"receipt":receipt.operation_id}),|snapshot|{
        verify_destination_with(plan,&all_committed,&BTreeSet::new(),|path|snapshot.read(path))?;
        let current=snapshot.read(Path::new(MARKER))?.ok_or_else(||recovery("migration marker disappeared"))?;
        let current_marker:Marker=decode(Path::new(MARKER),&current)?;
        if current_marker!=*marker {return Err(recovery("pending migration marker changed before cutover"));}
        Ok(PreparedOperation{changes:vec![FileChange{path:MARKER.into(),expected:Some(ContentHash::of(&current)),content:Some(complete_bytes.clone())}],result:json!({"migration":marker.migration,"manifest":manifest_path})})
    },|point|{
        fault(MigrationFault::Cutover(point))?;
        if matches!(point,FaultPoint::BeforeJournal|FaultPoint::BeforeReceipt) {verify_source(plan)?;}
        Ok(())
    })?;
    fault(MigrationFault::AfterCutover)?;
    completed_replay(root, &complete)?.ok_or_else(|| recovery("cutover was not durably completed"))
}

fn completed_replay(root: &Path, marker: &Marker) -> Result<Option<MigrationReceipt>> {
    let Some(completion) = &marker.completion else {
        return Ok(None);
    };
    let store = TransactionStore::open(root)?
        .for_repository(marker.repository.clone())
        .for_migration(marker.migration.clone());
    if store
        .pending_operations()?
        .iter()
        .any(|operation| operation.receipt.operation.starts_with("migration."))
    {
        return Ok(None);
    }
    store.with_snapshot(|snapshot| {
        check_access(root, snapshot, None)?;
        let marker_bytes = snapshot
            .read(Path::new(MARKER))?
            .ok_or_else(|| recovery("migration marker disappeared"))?;
        let live: Marker = decode(Path::new(MARKER), &marker_bytes)?;
        if &live != marker {
            return Err(recovery("migration marker changed during replay"));
        }
        let manifest_bytes = snapshot
            .read(&completion.manifest)?
            .ok_or_else(|| recovery("migration manifest disappeared"))?;
        let manifest: Manifest = decode(&completion.manifest, &manifest_bytes)?;
        let receipt_bytes = snapshot
            .read(&completion.receipt)?
            .ok_or_else(|| recovery("migration receipt disappeared"))?;
        let receipt: MutationReceipt = decode(&completion.receipt, &receipt_bytes)?;
        let cutover_hash = ContentHash::of(&marker_bytes);
        let cutover = cutover_receipt(
            root,
            snapshot,
            marker,
            cutover_hash.clone(),
            &receipt.operation_id,
        )?;
        Ok(Some(MigrationReceipt {
            schema: SchemaVersion::CURRENT,
            migration_id: marker.migration.clone(),
            request_id: marker.request.clone(),
            repository: marker.repository.clone(),
            preview_fingerprint: marker.preview.clone(),
            manifest_path: completion.manifest.clone(),
            manifest_hash: completion.manifest_content.clone(),
            cutover_operation: cutover.operation_id,
            cutover_hash,
            changed: manifest.changed,
            batches: manifest.batches,
            receipt,
        }))
    })
}

fn cutover_receipt(
    root: &Path,
    snapshot: &crate::transactions::Snapshot<'_>,
    marker: &Marker,
    marker_hash: ContentHash,
    primary: &OperationId,
) -> Result<MutationReceipt> {
    let completion = marker
        .completion
        .as_ref()
        .ok_or_else(|| recovery("pending migration has no cutover receipt"))?;
    let request = subrequest(marker, "cutover");
    let mut found = None;
    for path in snapshot.list(Path::new("operations"))? {
        let bytes = snapshot
            .read(&path)?
            .ok_or_else(|| recovery("operation receipt disappeared"))?;
        let receipt: MutationReceipt = decode(&root.join(&path), &bytes)?;
        if receipt.request_id != request {
            continue;
        }
        let expected = canonical_hash(
            &json!({"manifest_content":completion.manifest_content,"migration":marker.migration,"receipt":primary}),
        )?;
        if found.is_some()
            || receipt.operation != "migration.cutover"
            || receipt.repository.as_ref() != Some(&marker.repository)
            || receipt.input_hash != expected
            || receipt.changed.len() != 1
            || receipt.changed[0].path != Path::new(MARKER)
            || receipt.changed[0].after.as_ref() != Some(&marker_hash)
            || path.as_os_str() != format!("operations/{}.yml", receipt.operation_id).as_str()
        {
            return Err(recovery(
                "cutover receipt does not identify the completed marker",
            ));
        }
        found = Some(receipt);
    }
    found.ok_or_else(|| recovery("completed migration is missing its durable cutover receipt"))
}
