use super::{
    git::BoundGit,
    local::{LocalState, request_name},
    *,
};
use crate::{
    Config, ContentHash, ErrorCode, OperationId, PmError, Repository, RequestId, Result,
    transactions::canonical_hash,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FetchIntent {
    schema: crate::SchemaVersion,
    repository: crate::RepositoryId,
    request_id: RequestId,
    operation_id: OperationId,
    operation: String,
    input_hash: ContentHash,
    binding: ContentHash,
    config: ContentHash,
    observations: Option<Vec<RemoteRefObservation>>,
    complete: Option<SourceFetchOutcome>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CacheRecord {
    pub repository: crate::RepositoryId,
    pub binding: ContentHash,
    pub reference: GitRefName,
    pub observation: RemoteRefObservation,
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
pub(super) fn cache_name(
    config: &Config,
    shared: &SharedSources,
    reference: &GitRefName,
) -> Result<(String, GitRefName)> {
    let key = canonical_hash(
        &serde_json::json!({"repository":config.repository,"shared":shared,"ref":reference}),
    )?;
    Ok((
        format!("cache/{key}.json"),
        format!("refs/workdeck/cache/{key}").parse()?,
    ))
}
pub(super) fn cached(
    root: &Path,
    git: &BoundGit,
    config: &Config,
    shared: &SharedSources,
    reference: &GitRefName,
) -> Result<Option<CacheRecord>> {
    let (name, cache_ref) = cache_name(config, shared, reference)?;
    let Some(bytes) = local::read_at(root, &name)? else {
        return Ok(None);
    };
    let record: CacheRecord = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("invalid private source-cache observation"))?;
    let url = git.remote_url(&shared.remote)?;
    if record.repository != config.repository
        || record.reference != cache_ref
        || record.observation.reference != *reference
        || record.observation.remote != shared.remote
        || record.binding != git.binding(&config.repository, shared, &url)?
        || git.resolve(cache_ref.as_str())? != record.observation.commit
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "source cache no longer matches its configured remote, Git binding or observed ref",
        ));
    }
    Ok(Some(record))
}
pub(super) fn capture_commit(
    git: &BoundGit,
    config: &Config,
    shared: &SharedSources,
    observation: &RemoteRefObservation,
    limits: &SourceCaptureLimits,
) -> Result<Option<SourceSnapshot>> {
    let Some(commit) = &observation.commit else {
        return Ok(None);
    };
    let tree = git.tree(commit)?;
    let entries = git.tree_entries(&tree)?;
    let files = super::capture_core::blobs(git, &entries, limits)?;
    let memory = crate::transactions::Snapshot::from_memory(Path::new("source-snapshot"), &files);
    let parsed = crate::repository::config_from_snapshot(Path::new("source-snapshot"), &memory)?;
    if parsed.repository != config.repository {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "observed ref belongs to another planning repository",
        ));
    }
    let role = if observation.reference == shared.accepted_ref {
        SourceRole::Accepted
    } else if observation.reference == shared.coordination_ref {
        SourceRole::Coordination
    } else {
        SourceRole::Proposal
    };
    super::capture_core::validate_role(&files, &parsed, role, Some(&observation.reference))?;
    if role == SourceRole::Coordination {
        git.coordination_root_only(commit)?;
    }
    Ok(Some(SourceSnapshot {
        identity: PlanningSourceIdentity {
            repository: parsed.repository,
            role,
            ref_name: Some(observation.reference.clone()),
            commit: Some(commit.clone()),
            tree: Some(tree),
            index_content: None,
            content: super::capture_core::content_hash(&files)?,
        },
        files,
        entries,
    }))
}
pub(super) fn fetch(
    repository: &Repository,
    request: &SourceFetchRequest,
    id: &RequestId,
    sync: bool,
) -> Result<SourceFetchOutcome> {
    let state = LocalState::open(repository)?;
    let name = request_name(id);
    let input_hash =
        canonical_hash(&serde_json::to_value(request).map_err(|e| invalid(&e.to_string()))?)?;
    let operation = if sync {
        "sources.sync"
    } else {
        "sources.fetch"
    };
    let existing = state.read(&name)?;
    if let Some(bytes) = &existing {
        let raw: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|_| invalid("invalid local source request journal"))?;
        if raw["operation"] != operation
            || raw["input_hash"] != serde_json::json!(input_hash)
            || raw["request_id"] != serde_json::json!(id)
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "request already identifies different source-operation inputs",
            ));
        }
    }
    let (config, config_content) = repository.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repository.root(), snapshot)?;
        let bytes = snapshot
            .read(Path::new("config.yml"))?
            .ok_or_else(|| invalid("configuration disappeared"))?;
        Ok((config, ContentHash::of(&bytes)))
    })?;
    let shared = config.sources.as_ref().ok_or_else(|| {
        PmError::new(
            ErrorCode::PolicyBlocked,
            "configure shared sources before explicit fetch/sync",
        )
    })?;
    let worktree = repository
        .root()
        .parent()
        .ok_or_else(|| invalid("planning source has no worktree"))?;
    if repository
        .root()
        .file_name()
        .is_none_or(|p| p != ".workdeck")
    {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "Git sources require canonical .workdeck binding",
        ));
    }
    let git = BoundGit::open_shared(worktree)?;
    let url = git.remote_url(&shared.remote)?;
    let binding = git.binding(repository.identity(), shared, &url)?;
    if request.expected_binding != binding {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "reviewed Git directory or remote binding changed before fetch/sync",
        ));
    }
    let mut intent: FetchIntent = if let Some(bytes) = existing {
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid source request journal"))?
    } else {
        if request.expected_config != config_content {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "source configuration changed before fetch/sync",
            ));
        }
        FetchIntent {
            schema: crate::SchemaVersion::CURRENT,
            repository: repository.identity().clone(),
            request_id: id.clone(),
            operation_id: OperationId::new(),
            operation: operation.into(),
            input_hash,
            binding: binding.clone(),
            config: config_content.clone(),
            observations: None,
            complete: None,
        }
    };
    if intent.repository != *repository.identity() || intent.binding != binding {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "source request belongs to another Git directory or remote binding",
        ));
    }
    if let Some(mut completed) = intent.complete {
        completed.replayed = true;
        return Ok(completed);
    }
    state.save(&name, &intent)?;
    if intent.observations.is_none() {
        git.check_deadline()?;
        intent.observations = Some(git.observe(
            &shared.remote,
            &url,
            &[shared.accepted_ref.clone(), shared.coordination_ref.clone()],
        )?);
        state.save(&name, &intent)?;
    }
    let observations = intent
        .observations
        .clone()
        .expect("persisted advertisement");
    let mut materialized = Vec::new();
    for observation in &observations {
        git.check_deadline()?;
        if let Some(commit) = &observation.commit {
            git.fetch_object(&url, commit)?;
        }
        let snapshot = capture_commit(
            &git,
            &config,
            shared,
            observation,
            &SourceCaptureLimits::default(),
        )?;
        git.verify()?;
        if git.remote_url(&shared.remote)? != url {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "configured remote changed during fetch",
            ));
        }
        let (cache_name, reference) = cache_name(&config, shared, &observation.reference)?;
        git.update_cache(&reference, observation.commit.as_ref())?;
        state.save(
            &cache_name,
            &CacheRecord {
                repository: config.repository.clone(),
                binding: binding.clone(),
                reference,
                observation: observation.clone(),
            },
        )?;
        if sync && let Some(snapshot) = snapshot {
            materialized.push(materialize(&state, &snapshot)?);
        }
    }
    repository.store()?.with_snapshot(|snapshot| {
        let current = snapshot
            .read(Path::new("config.yml"))?
            .ok_or_else(|| invalid("configuration disappeared"))?;
        if ContentHash::of(&current) != config_content {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "source configuration changed during fetch/sync; inspect retained local operation",
            ));
        }
        Ok(())
    })?;
    let outcome = SourceFetchOutcome {
        repository: repository.identity().clone(),
        request_id: id.clone(),
        operation_id: intent.operation_id.clone(),
        config: intent.config.clone(),
        observations,
        materialized,
        replayed: false,
    };
    intent.complete = Some(outcome.clone());
    state.save(&name, &intent)?;
    Ok(outcome)
}
fn materialize(state: &LocalState, snapshot: &SourceSnapshot) -> Result<PathBuf> {
    if snapshot.identity().role != SourceRole::Coordination {
        let report = snapshot.with_snapshot(|memory| {
            crate::repository::inspect_snapshot(Path::new("source-snapshot"), memory)
        })?;
        if !report.valid {
            return Err(PmError::new(
                ErrorCode::InvalidSchema,
                "isolated source view contains invalid planning records",
            )
            .details(serde_json::json!({"errors":report.errors})));
        }
    }
    let directory = format!(
        "views/{}",
        canonical_hash(
            &serde_json::to_value(snapshot.identity()).map_err(|e| invalid(&e.to_string()))?
        )?
    );
    let mut files = BTreeMap::new();
    for (path, bytes) in snapshot.files() {
        let content = ContentHash::of(bytes);
        let name = format!("{directory}/blobs/{content}");
        match state.read(&name)? {
            Some(existing) if existing == *bytes => {}
            Some(_) => {
                return Err(PmError::new(
                    ErrorCode::Conflict,
                    "isolated view blob was modified; preserve it before retrying",
                ));
            }
            None => state.publish(&name, bytes, None)?,
        }
        files.insert(
            path,
            serde_json::json!({"content":content,"bytes":bytes.len()}),
        );
    }
    let name = format!("{directory}/manifest.json");
    let bytes = serde_json::to_vec(
        &serde_json::json!({"schema":1,"source":snapshot.identity(),"files":files}),
    )
    .map_err(|e| invalid(&e.to_string()))?;
    match state.read(&name)? {
        Some(existing) if existing == bytes => {}
        Some(_) => {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "isolated view manifest was modified",
            ));
        }
        None => state.publish(&name, &bytes, None)?,
    }
    state.path(&name)
}
