//! Durable, isolated coordination publication. Local intent is never remote proof.
use super::{
    git::BoundGit,
    git_write::PushResult,
    local::{LocalState, request_name},
    publication::*,
    *,
};
use crate::{
    Config, ContentHash, ErrorCode, OperationId, PmError, Repository, RequestId, Result,
    SchemaVersion,
    transactions::{ChangedPath, MutationReceipt, PreparedOperation, canonical_hash},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Intent {
    schema: SchemaVersion,
    repository: crate::RepositoryId,
    request_id: RequestId,
    operation_id: OperationId,
    operation: String,
    input_hash: ContentHash,
    binding: ContentHash,
    attempts: Vec<Attempt>,
    confirmation: Option<PublicationConfirmation>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Attempt {
    publication: PublicationAttempt,
    receipt: MutationReceipt,
    accepted: SourceObservation,
    coordination: SourceObservation,
    /// Only a definite non-fast-forward response admits semantic recomputation.
    retryable: bool,
}
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn stale(message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}

pub(super) fn coordinate_before(
    repository: &Repository,
    request: &RequestId,
    operation: &str,
    input: &serde_json::Value,
    mut prepare: impl FnMut(&CoordinationBasis) -> Result<PreparedOperation>,
    admission: PublicationAdmission<'_>,
    mut fault: impl FnMut(PublicationFaultPoint) -> Result<()>,
) -> Result<PublicationOutcome> {
    let PublicationAdmission {
        deadline,
        expected_binding,
    } = admission;
    if std::time::Instant::now() >= deadline {
        return Err(PmError::new(
            ErrorCode::Io,
            "Git publication exceeded its total timeout",
        ));
    }
    let state = LocalState::open(repository)?;
    let name = request_name(request);
    let input_hash = canonical_hash(input)?;
    let existing = state.read(&name)?;
    if let Some(bytes) = &existing {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| invalid("invalid source request journal"))?;
        if value["operation"] != operation
            || value["input_hash"] != serde_json::json!(input_hash)
            || value["request_id"] != serde_json::json!(request)
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "request already identifies different publication inputs",
            ));
        }
    }
    if !operation.starts_with("claim.") {
        return Err(invalid(
            "coordination preparation only supports explicit claim operations",
        ));
    }
    let (config, config_content) = repository.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repository.root(), snapshot)?;
        let bytes = snapshot
            .read(Path::new("config.yml"))?
            .ok_or_else(|| stale("configuration disappeared"))?;
        Ok((config, ContentHash::of(&bytes)))
    })?;
    let shared = config.sources.as_ref().ok_or_else(|| {
        PmError::new(
            ErrorCode::PolicyBlocked,
            "shared publication requires configured accepted and coordination refs",
        )
    })?;
    if repository
        .root()
        .file_name()
        .is_none_or(|name| name != ".workdeck")
    {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "shared publication requires canonical .workdeck worktree binding",
        ));
    }
    let worktree = repository
        .root()
        .parent()
        .ok_or_else(|| invalid("planning root has no worktree"))?;
    let mut git =
        BoundGit::with_deadline(worktree, &SourceCaptureLimits::default(), deadline, true)?;
    let url = git.remote_url(&shared.remote)?;
    let binding = git.binding(repository.identity(), shared, &url)?;
    if expected_binding.is_some_and(|expected| expected != &binding) {
        return Err(stale(
            "reviewed Git directory or remote binding changed before claim publication",
        ));
    }
    let replayed = existing.is_some();
    let mut intent: Intent = if let Some(bytes) = existing {
        serde_json::from_slice(&bytes)
            .map_err(|_| invalid("invalid coordination request journal"))?
    } else {
        Intent {
            schema: SchemaVersion::CURRENT,
            repository: config.repository.clone(),
            request_id: request.clone(),
            operation_id: OperationId::new(),
            operation: operation.into(),
            input_hash: input_hash.clone(),
            binding: binding.clone(),
            attempts: vec![],
            confirmation: None,
        }
    };
    if intent.repository != config.repository || intent.binding != binding {
        return Err(stale(
            "publication intent belongs to a different repository, Git directory or remote binding",
        ));
    }
    if expected_binding.is_some_and(|expected| expected != &intent.binding) {
        return Err(stale(
            "publication replay differs from its required original source binding",
        ));
    }
    validate_intent(&intent)?;
    validate_candidates(&git, &config, shared, &intent)?;
    state.save(&name, &intent)?;
    let mut captured = match observe_pair(&state, &mut git, &url, &config, shared) {
        Ok(captured) => captured,
        Err(error) if !intent.attempts.is_empty() => {
            return outcome(
                &intent,
                None,
                None,
                PublicationState::Uncertain,
                replayed,
                vec![
                    "remote_observation_unavailable".into(),
                    format!("observation_error:{:?}", error.code),
                ],
            );
        }
        Err(error) => return Err(error),
    };
    // Exact request proof on the current ref wins before caller preparation.
    // Its original token may already be released or superseded by later history.
    if let Some(receipt) = find_receipt(&captured.coordination, request, operation, &input_hash)? {
        confirm_existing(&mut intent, &git, &captured, &receipt)?;
        state.save(&name, &intent)?;
        return outcome(
            &intent,
            Some(&captured),
            Some(receipt),
            PublicationState::Confirmed,
            true,
            vec![],
        );
    }
    if intent.confirmation.is_some()
        || intent.attempts.last().is_some_and(|a| {
            a.publication.state == PublicationState::Uncertain
                || a.publication.state == PublicationState::Confirmed
        })
    {
        // An unchanged or advanced remote tip cannot prove a timed-out push
        // failed. Never create a second acquisition while it might be in flight.
        return outcome(
            &intent,
            Some(&captured),
            None,
            PublicationState::Uncertain,
            replayed,
            vec!["publication_not_yet_reconciled".into()],
        );
    }
    let policy = captured.accepted.config()?.claims.unwrap_or_default();
    policy.validate()?;
    loop {
        let prepared_pending = intent
            .attempts
            .last()
            .is_some_and(|a| a.publication.state == PublicationState::Prepared);
        if !prepared_pending {
            if intent.attempts.last().is_some_and(|a| !a.retryable)
                || intent.attempts.len() >= usize::from(policy.max_publish_attempts)
            {
                return outcome(
                    &intent,
                    Some(&captured),
                    None,
                    PublicationState::Rejected,
                    replayed,
                    vec!["publication_rejected".into()],
                );
            }
            let number = u8::try_from(intent.attempts.len() + 1)
                .map_err(|_| invalid("publication attempt bound exceeded"))?;
            let basis = CoordinationBasis {
                accepted: captured.accepted.clone(),
                coordination: captured.coordination.clone(),
                accepted_observation: captured.accepted_observation.clone(),
                coordination_observation: captured.coordination_observation.clone(),
                request_id: request.clone(),
                operation_id: intent.operation_id.clone(),
                attempt: number,
                as_of: chrono::Utc::now(),
                observed: captured.coordination_remote.clone(),
            };
            let prepared = prepare(&basis)?;
            let (files, receipt) = prospective(&basis, &intent, prepared)?;
            let changes = files
                .iter()
                .map(|(path, bytes)| (Path::new(".workdeck").join(path), Some(bytes.clone())))
                .collect();
            let tree = git.write_tree(None, &changes)?;
            let candidate = git.commit_tree(
                &tree,
                basis.observed.commit.as_ref(),
                &format!("Workdeck {} {}\n", operation, intent.operation_id),
                basis.as_of,
            )?;
            let attempt = Attempt {
                publication: PublicationAttempt {
                    number: basis.attempt,
                    observed: basis.observed.commit.clone(),
                    candidate,
                    state: PublicationState::Prepared,
                },
                receipt,
                accepted: basis.accepted_observation,
                coordination: basis.coordination_observation,
                retryable: false,
            };
            intent.attempts.push(attempt);
            state.save(&name, &intent)?;
            let retained: GitRefName = format!(
                "refs/workdeck/cache/publications/{}/{}",
                intent.operation_id, number
            )
            .parse()?;
            git.update_cache(
                &retained,
                Some(
                    &intent
                        .attempts
                        .last()
                        .expect("attempt")
                        .publication
                        .candidate,
                ),
            )?;
            fault(PublicationFaultPoint::AfterPrepared)?;
        }
        let current = intent.attempts.last().expect("prepared attempt");
        // A prepared-but-unspawned candidate can be discarded safely if its
        // observed basis moved. Once spawn may have occurred it cannot.
        let observations = git.observe(
            &shared.remote,
            &url,
            &[shared.accepted_ref.clone(), shared.coordination_ref.clone()],
        )?;
        if observations[0].commit != current.accepted.identity.commit
            || observations[1].commit != current.publication.observed
        {
            intent
                .attempts
                .last_mut()
                .expect("attempt")
                .publication
                .state = PublicationState::Rejected;
            intent.attempts.last_mut().expect("attempt").retryable = true;
            state.save(&name, &intent)?;
            captured = observe_pair(&state, &mut git, &url, &config, shared)?;
            if let Some(receipt) =
                find_receipt(&captured.coordination, request, operation, &input_hash)?
            {
                confirm_existing(&mut intent, &git, &captured, &receipt)?;
                state.save(&name, &intent)?;
                return outcome(
                    &intent,
                    Some(&captured),
                    Some(receipt),
                    PublicationState::Confirmed,
                    true,
                    vec![],
                );
            }
            continue;
        }
        validate_binding(repository, &git, shared, &url, &config_content)?;
        let candidate = current.publication.candidate.clone();
        // This durable transition precedes spawn. Crashes here deliberately
        // require reconciliation rather than guessing whether a child started.
        intent
            .attempts
            .last_mut()
            .expect("attempt")
            .publication
            .state = PublicationState::Uncertain;
        state.save(&name, &intent)?;
        fault(PublicationFaultPoint::BeforePush)?;
        git.check_deadline()?;
        let pushed = git
            .push_candidate(&url, &shared.coordination_ref, &candidate)
            .unwrap_or(PushResult::Uncertain);
        fault(PublicationFaultPoint::AfterPush)?;
        if matches!(
            pushed,
            PushResult::Rejected | PushResult::RetryableRejection
        ) {
            let attempt = intent.attempts.last_mut().expect("attempt");
            attempt.publication.state = PublicationState::Rejected;
            attempt.retryable = pushed == PushResult::RetryableRejection;
            state.save(&name, &intent)?;
        }
        captured = match observe_pair(&state, &mut git, &url, &config, shared) {
            Ok(c) => c,
            Err(error) => {
                return outcome(
                    &intent,
                    None,
                    None,
                    PublicationState::Uncertain,
                    replayed,
                    vec![
                        "remote_observation_unavailable".into(),
                        format!("observation_error:{:?}", error.code),
                    ],
                );
            }
        };
        if let Some(receipt) =
            find_receipt(&captured.coordination, request, operation, &input_hash)?
        {
            confirm_existing(&mut intent, &git, &captured, &receipt)?;
            state.save(&name, &intent)?;
            fault(PublicationFaultPoint::AfterConfirmation)?;
            return outcome(
                &intent,
                Some(&captured),
                Some(receipt),
                PublicationState::Confirmed,
                replayed,
                vec![],
            );
        }
        if pushed != PushResult::RetryableRejection {
            return outcome(
                &intent,
                Some(&captured),
                None,
                if pushed == PushResult::Rejected {
                    PublicationState::Rejected
                } else {
                    PublicationState::Uncertain
                },
                replayed,
                vec![
                    if pushed == PushResult::Rejected {
                        "publication_rejected"
                    } else {
                        "publication_not_yet_reconciled"
                    }
                    .into(),
                ],
            );
        }
        // A new callback sees the complete newly observed state and must pass
        // all current eligibility/token/contract checks. No blind replay/rebase.
    }
}

struct Observed {
    accepted: SourceSnapshot,
    coordination: SourceSnapshot,
    accepted_observation: SourceObservation,
    coordination_observation: SourceObservation,
    coordination_remote: RemoteRefObservation,
}
fn observe_pair(
    state: &LocalState,
    git: &mut BoundGit,
    url: &str,
    config: &Config,
    shared: &SharedSources,
) -> Result<Observed> {
    git.check_deadline()?;
    let observations = git.observe(
        &shared.remote,
        url,
        &[shared.accepted_ref.clone(), shared.coordination_ref.clone()],
    )?;
    for observed in &observations {
        if let Some(commit) = &observed.commit {
            git.fetch_object(url, commit)?;
        }
    }
    let accepted = remote_impl::capture_commit(
        git,
        config,
        shared,
        &observations[0],
        &SourceCaptureLimits::default(),
    )?
    .ok_or_else(|| {
        PmError::new(
            ErrorCode::NotFound,
            "accepted planning ref must exist before coordination",
        )
    })?;
    let coordination = match remote_impl::capture_commit(
        git,
        config,
        shared,
        &observations[1],
        &SourceCaptureLimits::default(),
    )? {
        Some(snapshot) => snapshot,
        None => empty_coordination(&accepted, shared)?,
    };
    let accepted_config = accepted.config()?;
    if accepted_config.sources.as_ref() != Some(shared) {
        return Err(stale(
            "accepted policy names different shared planning authorities",
        ));
    }
    coordination
        .with_snapshot(|snapshot| crate::claims::load_claims(snapshot, &accepted_config))?;
    git.verify()?;
    if git.remote_url(&shared.remote)? != url {
        return Err(stale(
            "configured remote changed during coordination observation",
        ));
    }
    for observed in &observations {
        let (name, reference) = remote_impl::cache_name(config, shared, &observed.reference)?;
        git.update_cache(&reference, observed.commit.as_ref())?;
        state.save(
            &name,
            &remote_impl::CacheRecord {
                repository: config.repository.clone(),
                binding: git.binding(&config.repository, shared, url)?,
                reference,
                observation: observed.clone(),
            },
        )?;
    }
    let observation =
        |snapshot: &SourceSnapshot, remote: &RemoteRefObservation| SourceObservation {
            identity: snapshot.identity().clone(),
            observed_at: remote.observed_at,
            remote_observation: Some(remote.clone()),
            freshness: SourceFreshness::CurrentAtObservation,
            reason_codes: vec!["remote_observed".into()],
        };
    Ok(Observed {
        accepted_observation: observation(&accepted, &observations[0]),
        coordination_observation: observation(&coordination, &observations[1]),
        accepted,
        coordination,
        coordination_remote: observations[1].clone(),
    })
}
fn empty_coordination(accepted: &SourceSnapshot, shared: &SharedSources) -> Result<SourceSnapshot> {
    let config = accepted.config()?;
    let marker = CoordinationMarker {
        schema: SchemaVersion::CURRENT,
        repository: config.repository.clone(),
        coordination_ref: shared.coordination_ref.clone(),
    };
    let files = BTreeMap::from([
        (
            PathBuf::from("config.yml"),
            accepted
                .files()
                .get(Path::new("config.yml"))
                .ok_or_else(|| invalid("accepted source lacks configuration"))?
                .clone(),
        ),
        (
            PathBuf::from("coordination.yml"),
            serde_yaml_ng::to_string(&marker)
                .map_err(|_| invalid("cannot encode coordination marker"))?
                .into_bytes(),
        ),
    ]);
    Ok(SourceSnapshot {
        identity: PlanningSourceIdentity {
            repository: config.repository,
            role: SourceRole::Coordination,
            ref_name: Some(shared.coordination_ref.clone()),
            commit: None,
            tree: None,
            index_content: None,
            content: capture_core::content_hash(&files)?,
        },
        entries: vec![],
        files,
    })
}
fn prospective(
    basis: &CoordinationBasis,
    intent: &Intent,
    prepared: PreparedOperation,
) -> Result<(BTreeMap<PathBuf, Vec<u8>>, MutationReceipt)> {
    let mut files = basis.coordination.files().clone();
    let mut changed = Vec::new();
    for change in prepared.changes {
        let parts = change.path.components().collect::<Vec<_>>();
        if parts.len() != 2
            || parts[0].as_os_str() != "claims"
            || parts
                .iter()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || change.path.extension().is_none_or(|e| e != "yml")
            || changed.iter().any(|c: &ChangedPath| c.path == change.path)
        {
            return Err(invalid(
                "coordination callbacks may mutate only unique canonical claims/<issue>.yml records",
            ));
        }
        let before = files.get(&change.path).map(|bytes| ContentHash::of(bytes));
        if before != change.expected {
            return Err(stale(
                "coordination change precondition differs from observed source",
            ));
        }
        let after = change.content.as_ref().map(|bytes| ContentHash::of(bytes));
        match change.content {
            Some(bytes) => {
                files.insert(change.path.clone(), bytes);
            }
            None => {
                files.remove(&change.path);
            }
        }
        changed.push(ChangedPath {
            path: change.path,
            before,
            after,
        });
    }
    let receipt = MutationReceipt {
        schema_version: SchemaVersion::CURRENT,
        repository: Some(intent.repository.clone()),
        operation_id: intent.operation_id.clone(),
        request_id: intent.request_id.clone(),
        operation: intent.operation.clone(),
        input_hash: intent.input_hash.clone(),
        result: prepared.result,
        changed,
    };
    crate::transactions::validate_receipt(&receipt)?;
    crate::claims::validate_receipt(&receipt)?;
    crate::completion::validate_receipt(&receipt)?;
    let path = PathBuf::from(format!("operations/{}.yml", receipt.operation_id));
    if files.contains_key(&path) {
        return Err(invalid(
            "operation identity already exists in coordination history",
        ));
    }
    files.insert(
        path,
        serde_yaml_ng::to_string(&receipt)
            .map_err(|_| invalid("cannot encode coordination receipt"))?
            .into_bytes(),
    );
    let limits = SourceCaptureLimits::default();
    if files.len() > limits.max_entries
        || files.values().any(|v| v.len() > limits.max_file_bytes)
        || files
            .values()
            .try_fold(0usize, |n, v| n.checked_add(v.len()))
            .is_none_or(|n| n > limits.max_total_bytes)
    {
        return Err(invalid(
            "prospective coordination source exceeds complete capture bounds",
        ));
    }
    let config = basis.accepted.config()?;
    let memory = crate::transactions::Snapshot::from_memory(Path::new("source-snapshot"), &files);
    crate::claims::load_claims(&memory, &config)?;
    Ok((files, receipt))
}
fn find_receipt(
    snapshot: &SourceSnapshot,
    request: &RequestId,
    operation: &str,
    input: &ContentHash,
) -> Result<Option<MutationReceipt>> {
    let mut found = None;
    for (path, bytes) in snapshot
        .files()
        .iter()
        .filter(|(p, _)| p.starts_with("operations"))
    {
        let receipt: MutationReceipt = serde_yaml_ng::from_slice(bytes)
            .map_err(|_| invalid("invalid coordination operation receipt"))?;
        crate::transactions::validate_receipt(&receipt)?;
        crate::claims::validate_receipt(&receipt)?;
        crate::completion::validate_receipt(&receipt)?;
        if !receipt.operation.starts_with("claim.")
            || receipt.repository.as_ref() != Some(&snapshot.identity().repository)
            || path != Path::new(&format!("operations/{}.yml", receipt.operation_id))
        {
            return Err(invalid(
                "coordination receipt has wrong operation, source or path",
            ));
        }
        if receipt.request_id == *request {
            if receipt.operation != operation || receipt.input_hash != *input {
                return Err(PmError::new(
                    ErrorCode::IdempotencyConflict,
                    "remote coordination request identifies different inputs",
                ));
            }
            if found.replace(receipt).is_some() {
                return Err(invalid("coordination history repeats request identity"));
            }
        }
    }
    Ok(found)
}
fn confirm_existing(
    intent: &mut Intent,
    git: &BoundGit,
    current: &Observed,
    receipt: &MutationReceipt,
) -> Result<()> {
    let tip = current
        .coordination
        .identity()
        .commit
        .as_ref()
        .ok_or_else(|| invalid("receipt has no published commit"))?;
    if let Some(attempt) = intent
        .attempts
        .iter_mut()
        .find(|a| a.receipt.operation_id == receipt.operation_id && a.receipt == *receipt)
    {
        if !git.ancestor(&attempt.publication.candidate, tip)? {
            return Err(stale(
                "receipt content exists without the expected candidate ancestry",
            ));
        }
        attempt.publication.state = PublicationState::Confirmed;
        if intent.confirmation.is_none() {
            intent.confirmation = Some(PublicationConfirmation {
                remote: current.coordination_remote.remote.clone(),
                reference: current.coordination_remote.reference.clone(),
                commit: attempt.publication.candidate.clone(),
                observed_parent: attempt.publication.observed.clone(),
                confirmed_at: current.coordination_remote.observed_at,
            });
        }
    } else if !intent.attempts.is_empty() {
        return Err(stale(
            "published request differs from this retained intent candidate",
        ));
    } else {
        intent.operation_id = receipt.operation_id.clone();
        intent.confirmation = Some(PublicationConfirmation {
            remote: current.coordination_remote.remote.clone(),
            reference: current.coordination_remote.reference.clone(),
            commit: tip.clone(),
            observed_parent: None,
            confirmed_at: current.coordination_remote.observed_at,
        });
    }
    Ok(())
}
fn outcome(
    intent: &Intent,
    current: Option<&Observed>,
    receipt: Option<MutationReceipt>,
    state: PublicationState,
    replayed: bool,
    reason_codes: Vec<String>,
) -> Result<PublicationOutcome> {
    Ok(PublicationOutcome {
        repository: intent.repository.clone(),
        request_id: intent.request_id.clone(),
        operation_id: intent.operation_id.clone(),
        state,
        receipt: receipt.or_else(|| intent.attempts.last().map(|a| a.receipt.clone())),
        confirmation: intent.confirmation.clone(),
        accepted: current.map(|c| c.accepted_observation.clone()),
        coordination: current.map(|c| c.coordination_observation.clone()),
        attempts: intent
            .attempts
            .iter()
            .map(|a| a.publication.clone())
            .collect(),
        reason_codes,
        replayed,
    })
}
fn validate_intent(intent: &Intent) -> Result<()> {
    if intent.attempts.len() > 8 {
        return Err(invalid("publication journal exceeds bounded attempts"));
    }
    for (i, attempt) in intent.attempts.iter().enumerate() {
        crate::transactions::validate_receipt(&attempt.receipt)?;
        crate::claims::validate_receipt(&attempt.receipt)?;
        crate::completion::validate_receipt(&attempt.receipt)?;
        if usize::from(attempt.publication.number) != i + 1
            || attempt.receipt.repository.as_ref() != Some(&intent.repository)
            || attempt.receipt.request_id != intent.request_id
            || attempt.receipt.operation_id != intent.operation_id
            || attempt.receipt.operation != intent.operation
            || attempt.receipt.input_hash != intent.input_hash
            || attempt.coordination.identity.commit != attempt.publication.observed
        {
            return Err(invalid(
                "publication journal receipt or source identity is inconsistent",
            ));
        }
    }
    Ok(())
}
fn validate_candidates(
    git: &BoundGit,
    config: &Config,
    shared: &SharedSources,
    intent: &Intent,
) -> Result<()> {
    for attempt in &intent.attempts {
        let accepted_remote = attempt
            .accepted
            .remote_observation
            .as_ref()
            .ok_or_else(|| invalid("publication attempt lacks accepted source observation"))?;
        let accepted = remote_impl::capture_commit(
            git,
            config,
            shared,
            accepted_remote,
            &SourceCaptureLimits::default(),
        )?
        .ok_or_else(|| invalid("publication accepted basis has no commit"))?;
        if accepted.identity() != &attempt.accepted.identity {
            return Err(invalid(
                "publication accepted basis differs from retained Git content",
            ));
        }
        let previous = if let Some(previous) = &attempt.publication.observed {
            let observed = RemoteRefObservation {
                remote: shared.remote.clone(),
                reference: shared.coordination_ref.clone(),
                commit: Some(previous.clone()),
                observed_at: attempt.coordination.observed_at,
            };
            remote_impl::capture_commit(
                git,
                config,
                shared,
                &observed,
                &SourceCaptureLimits::default(),
            )?
            .ok_or_else(|| invalid("publication parent disappeared"))?
        } else {
            empty_coordination(&accepted, shared)?
        };
        if previous.identity() != &attempt.coordination.identity {
            return Err(invalid(
                "publication parent differs from retained coordination content",
            ));
        }
        let candidate = remote_impl::capture_commit(
            git,
            config,
            shared,
            &RemoteRefObservation {
                remote: shared.remote.clone(),
                reference: shared.coordination_ref.clone(),
                commit: Some(attempt.publication.candidate.clone()),
                observed_at: attempt.coordination.observed_at,
            },
            &SourceCaptureLimits::default(),
        )?
        .ok_or_else(|| invalid("publication candidate disappeared"))?;
        if git.parents(&attempt.publication.candidate)?
            != attempt
                .publication
                .observed
                .iter()
                .cloned()
                .collect::<Vec<_>>()
        {
            return Err(invalid(
                "publication candidate does not descend from its exact observed parent",
            ));
        }
        let mut expected = previous.files().clone();
        for change in &attempt.receipt.changed {
            if expected
                .get(&change.path)
                .map(|bytes| ContentHash::of(bytes))
                != change.before
                || candidate
                    .files()
                    .get(&change.path)
                    .map(|bytes| ContentHash::of(bytes))
                    != change.after
            {
                return Err(invalid(
                    "publication candidate disagrees with its receipt content preconditions",
                ));
            }
            if let Some(bytes) = candidate.files().get(&change.path) {
                expected.insert(change.path.clone(), bytes.clone());
            } else {
                expected.remove(&change.path);
            }
        }
        expected.insert(
            PathBuf::from(format!("operations/{}.yml", attempt.receipt.operation_id)),
            serde_yaml_ng::to_string(&attempt.receipt)
                .map_err(|_| invalid("cannot encode retained receipt"))?
                .into_bytes(),
        );
        if &expected != candidate.files() {
            return Err(invalid(
                "publication candidate changes content outside its exact retained receipt",
            ));
        }
        candidate
            .with_snapshot(|snapshot| crate::claims::load_claims(snapshot, &accepted.config()?))?;
    }
    Ok(())
}
fn validate_binding(
    repository: &Repository,
    git: &BoundGit,
    shared: &SharedSources,
    url: &str,
    expected: &ContentHash,
) -> Result<()> {
    git.verify()?;
    if git.remote_url(&shared.remote)? != url {
        return Err(stale("remote binding changed before publication"));
    }
    repository.store()?.with_snapshot(|snapshot| {
        let bytes = snapshot
            .read(Path::new("config.yml"))?
            .ok_or_else(|| stale("configuration disappeared"))?;
        if ContentHash::of(&bytes) != *expected {
            return Err(stale("source configuration changed before publication"));
        }
        Ok(())
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::{AcquireClaim, ClaimRequest, ClaimToken, CreateIssue, IssueId, IssueRecord};
    use std::{
        fs,
        process::Command,
        sync::{Arc, Barrier},
    };
    fn coordinate(
        repository: &Repository,
        request: &RequestId,
        operation: &str,
        input: &serde_json::Value,
        prepare: impl FnMut(&CoordinationBasis) -> Result<PreparedOperation>,
        fault: impl FnMut(PublicationFaultPoint) -> Result<()>,
    ) -> Result<PublicationOutcome> {
        coordinate_before(
            repository,
            request,
            operation,
            input,
            prepare,
            PublicationAdmission {
                deadline: std::time::Instant::now()
                    + std::time::Duration::from_secs(
                        SourceCaptureLimits::default().timeout_seconds,
                    ),
                expected_binding: None,
            },
            fault,
        )
    }
    fn git(root: &Path, args: &[&str]) -> Vec<u8> {
        let mut command = Command::new("git");
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(|k| k.starts_with("GIT_")) {
                command.env_remove(key);
            }
        }
        let output = command
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }
    fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Repository, IssueId) {
        let temp = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-b", "main"]);
        git(remote.path(), &["init", "--bare"]);
        git(temp.path(), &["config", "user.name", "Fixture"]);
        git(
            temp.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(
            temp.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        let repo = Repository::init(temp.path(), "WD").unwrap();
        let issue: IssueRecord = serde_json::from_value(
            repo.create_issue(&CreateIssue::new("Work", "accepted"), &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        let mut config = repo.config().unwrap();
        config.sources = Some(SharedSources {
            remote: "origin".into(),
            accepted_ref: "refs/heads/main".parse().unwrap(),
            coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
            proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
        });
        fs::write(
            repo.root().join("config.yml"),
            serde_yaml_ng::to_string(&config).unwrap(),
        )
        .unwrap();
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-m", "accepted"]);
        git(temp.path(), &["push", "origin", "main"]);
        (temp, remote, repo, issue.metadata.id)
    }
    fn request(repo: &Repository, issue: &IssueId, actor: &str) -> ClaimRequest {
        ClaimRequest::Acquire {
            input: Box::new(AcquireClaim {
                actor: actor.into(),
                contract: repo.claim_contract(issue).unwrap(),
                ttl_seconds: None,
                recovery: None,
            }),
        }
    }
    fn prepare_claim(
        basis: &CoordinationBasis,
        request: &ClaimRequest,
    ) -> Result<PreparedOperation> {
        let config = basis.accepted.config()?;
        let contract = basis.accepted.claim_contract(request.issue())?;
        basis.accepted.with_snapshot(|snapshot| {
            crate::claims::store::eligible(
                Path::new("source-snapshot"),
                snapshot,
                &config,
                request.issue(),
                match request {
                    ClaimRequest::Acquire { input } => &input.actor,
                    _ => unreachable!(),
                },
            )
        })?;
        let token: ClaimToken = format!(
            "CLM-{}",
            basis.operation_id.as_str().strip_prefix("OP-").unwrap()
        )
        .parse()?;
        basis.coordination.with_snapshot(|snapshot| {
            crate::claims::store::prepare(
                snapshot,
                &config,
                request,
                &contract,
                &basis.request_id,
                basis.as_of,
                token,
            )
        })
    }
    fn crash() -> PmError {
        PmError::new(ErrorCode::Io, "simulated interruption")
    }
    #[test]
    fn lost_push_response_reconciles_exact_candidate_without_preparing_again() {
        let (_temp, _remote, repo, issue) = fixture();
        let request = request(&repo, &issue, "alice");
        let id = RequestId::new();
        let input = serde_json::json!({"request":request});
        assert!(
            coordinate(
                &repo,
                &id,
                request.operation(),
                &input,
                |basis| prepare_claim(basis, &request),
                |point| if point == PublicationFaultPoint::AfterPush {
                    Err(crash())
                } else {
                    Ok(())
                }
            )
            .is_err()
        );
        let resumed = coordinate(
            &repo,
            &id,
            request.operation(),
            &input,
            |_| panic!("replay must not prepare"),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(resumed.state, PublicationState::Confirmed);
        assert!(resumed.replayed);
        assert_eq!(resumed.attempts.len(), 1);
        assert_eq!(resumed.receipt.unwrap().operation_id, resumed.operation_id);
    }
    #[test]
    fn uncertain_before_spawn_never_assumes_unchanged_remote_means_failure() {
        let (_temp, remote, repo, issue) = fixture();
        let request = request(&repo, &issue, "alice");
        let id = RequestId::new();
        let input = serde_json::json!({"request":request});
        assert!(
            coordinate(
                &repo,
                &id,
                request.operation(),
                &input,
                |basis| prepare_claim(basis, &request),
                |point| if point == PublicationFaultPoint::BeforePush {
                    Err(crash())
                } else {
                    Ok(())
                }
            )
            .is_err()
        );
        let resumed = coordinate(
            &repo,
            &id,
            request.operation(),
            &input,
            |_| panic!("uncertainty must not reprepare"),
            |_| panic!("uncertainty must not spawn"),
        )
        .unwrap();
        assert_eq!(resumed.state, PublicationState::Uncertain);
        assert_eq!(resumed.attempts.len(), 1);
        assert!(
            git(
                remote.path(),
                &[
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/heads/workdeck-coordination"
                ]
            )
            .is_empty()
        );
        // Model a previously in-flight push finally reaching the remote after
        // that unchanged-tip observation. The SAME candidate then reconciles.
        let candidate = &resumed.attempts[0].candidate;
        git(
            repo.root().parent().unwrap(),
            &[
                "push",
                "origin",
                &format!("{candidate}:refs/heads/workdeck-coordination"),
            ],
        );
        let confirmed = coordinate(
            &repo,
            &id,
            request.operation(),
            &input,
            |_| panic!("late arrival must not reprepare"),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(confirmed.state, PublicationState::Confirmed);
        assert_eq!(confirmed.operation_id, resumed.operation_id);
    }
    #[test]
    fn prepared_unspawned_operation_resumes_the_original_candidate() {
        let (_temp, _remote, repo, issue) = fixture();
        let request = request(&repo, &issue, "alice");
        let id = RequestId::new();
        let input = serde_json::json!({"request":request});
        assert!(
            coordinate(
                &repo,
                &id,
                request.operation(),
                &input,
                |basis| prepare_claim(basis, &request),
                |point| if point == PublicationFaultPoint::AfterPrepared {
                    Err(crash())
                } else {
                    Ok(())
                }
            )
            .is_err()
        );
        let resumed = coordinate(
            &repo,
            &id,
            request.operation(),
            &input,
            |_| panic!("prepared candidate should retain original preparation"),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(resumed.state, PublicationState::Confirmed);
        assert_eq!(resumed.attempts.len(), 1);
    }
    #[test]
    fn tampered_prepared_candidate_cannot_publish_application_content_to_coordination() {
        let (temp, remote, repo, issue) = fixture();
        let request = request(&repo, &issue, "alice");
        let id = RequestId::new();
        let input = serde_json::json!({"request":request});
        assert!(
            coordinate(
                &repo,
                &id,
                request.operation(),
                &input,
                |basis| prepare_claim(basis, &request),
                |point| if point == PublicationFaultPoint::AfterPrepared {
                    Err(crash())
                } else {
                    Ok(())
                }
            )
            .is_err()
        );
        {
            let state = LocalState::open(&repo).unwrap();
            let name = request_name(&id);
            let mut intent: Intent =
                serde_json::from_slice(&state.read(&name).unwrap().unwrap()).unwrap();
            intent.attempts[0].publication.candidate =
                String::from_utf8(git(temp.path(), &["rev-parse", "HEAD"]))
                    .unwrap()
                    .trim()
                    .parse()
                    .unwrap();
            state.save(&name, &intent).unwrap();
        }
        let result = coordinate(
            &repo,
            &id,
            request.operation(),
            &input,
            |_| panic!("retained candidate must be validated"),
            |_| Ok(()),
        );
        assert!(
            result.is_err(),
            "tampered prepared candidate was admitted: {result:?}"
        );
        assert!(
            git(
                remote.path(),
                &[
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/heads/workdeck-coordination"
                ]
            )
            .is_empty()
        );
    }
    #[test]
    fn racing_clones_have_one_confirmed_winner_and_rejected_candidate_is_reevaluated() {
        let (temp, remote, repo, issue) = fixture();
        let clone = tempfile::tempdir().unwrap();
        git(
            clone.path(),
            &[
                "clone",
                "--branch",
                "main",
                remote.path().to_str().unwrap(),
                ".",
            ],
        );
        let second = Repository::open_source(&clone.path().join(".workdeck")).unwrap();
        let first = request(&repo, &issue, "alice");
        let other = request(&second, &issue, "bob");
        let barrier = Arc::new(Barrier::new(2));
        let threads = [(repo, first), (second, other)]
            .into_iter()
            .map(|(repo, request)| {
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let mut waited = false;
                    coordinate(
                        &repo,
                        &RequestId::new(),
                        request.operation(),
                        &serde_json::json!({"request":request}),
                        |basis| prepare_claim(basis, &request),
                        |point| {
                            if point == PublicationFaultPoint::AfterPrepared && !waited {
                                waited = true;
                                barrier.wait();
                            }
                            Ok(())
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        let results = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            results
                .iter()
                .filter(|r| r
                    .as_ref()
                    .is_ok_and(|o| o.state == PublicationState::Confirmed))
                .count(),
            1,
            "{results:?}"
        );
        let refs = git(
            remote.path(),
            &["rev-list", "--count", "refs/heads/workdeck-coordination"],
        );
        assert_eq!(refs, b"1\n");
        assert_eq!(
            git(temp.path(), &["rev-parse", "HEAD"]),
            git(temp.path(), &["rev-parse", "refs/heads/main"])
        );
    }
}

/// A bounded explicit remote observation, kept outside the local writer lock.
/// This is cooperative observed-at proof, never an atomic cross-ref lease.
pub(crate) struct ConfirmedSources {
    pub binding: ContentHash,
    pub accepted: SourceSnapshot,
    pub coordination: SourceSnapshot,
    pub accepted_observation: SourceObservation,
    pub coordination_observation: SourceObservation,
    pub observed_at: crate::Timestamp,
    git: BoundGit,
    local_guard: super::git::LocalGitGuard,
    root: PathBuf,
    config: ContentHash,
    shared: SharedSources,
    url: ContentHash,
}
impl ConfirmedSources {
    pub(crate) fn revalidate_files(&self) -> Result<()> {
        self.local_guard.verify()
    }
    /// Complete bounded local Git/config verification outside a PM writer lock.
    pub(crate) fn verify_local(&self) -> Result<()> {
        self.local_guard.verify()?;
        self.git.verify()?;
        if ContentHash::of(self.git.remote_url(&self.shared.remote)?.as_bytes()) != self.url {
            return Err(stale(
                "remote configuration changed after shared confirmation",
            ));
        }
        self.local_guard.verify()
    }
    /// No network and no nested store acquisition; the caller supplies its
    /// already locked local Snapshot immediately before its journal is admitted.
    pub(crate) fn revalidate_local(
        &self,
        snapshot: &crate::transactions::Snapshot<'_>,
    ) -> Result<()> {
        let config = crate::repository::config_from_snapshot(&self.root, snapshot)?;
        let bytes = snapshot
            .read(Path::new("config.yml"))?
            .ok_or_else(|| stale("local source configuration disappeared"))?;
        if config.repository != self.accepted.identity().repository
            || ContentHash::of(&bytes) != self.config
            || config.sources.as_ref() != Some(&self.shared)
        {
            return Err(stale(
                "local source binding changed after shared confirmation",
            ));
        }
        self.revalidate_files()?;
        Ok(())
    }
}
pub(crate) fn confirm_sources_bound(
    repository: &Repository,
    expected: Option<&ContentHash>,
) -> Result<ConfirmedSources> {
    let state = LocalState::open(repository)?;
    let (config, content) = repository.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repository.root(), snapshot)?;
        let bytes = snapshot
            .read(Path::new("config.yml"))?
            .ok_or_else(|| stale("local configuration disappeared"))?;
        Ok((config, ContentHash::of(&bytes)))
    })?;
    let shared = config
        .sources
        .clone()
        .ok_or_else(|| invalid("shared confirmation requires explicit source configuration"))?;
    if repository
        .root()
        .file_name()
        .is_none_or(|name| name != ".workdeck")
    {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "shared confirmation requires canonical .workdeck binding",
        ));
    }
    let mut git = BoundGit::open_shared(
        repository
            .root()
            .parent()
            .ok_or_else(|| invalid("planning source has no worktree"))?,
    )?;
    let url = git.remote_url(&shared.remote)?;
    let binding = git.binding(repository.identity(), &shared, &url)?;
    if expected.is_some_and(|expected| expected != &binding) {
        return Err(stale(
            "reviewed Git directory or remote binding changed before shared confirmation",
        ));
    }
    let observed = observe_pair(&state, &mut git, &url, &config, &shared)?;
    let local_guard = git.local_guard()?;
    validate_binding(repository, &git, &shared, &url, &content)?;
    local_guard.verify()?;
    Ok(ConfirmedSources {
        binding,
        observed_at: observed.coordination_remote.observed_at,
        accepted: observed.accepted,
        coordination: observed.coordination,
        accepted_observation: observed.accepted_observation,
        coordination_observation: observed.coordination_observation,
        git,
        local_guard,
        root: repository.root().to_owned(),
        config: content,
        shared,
        url: ContentHash::of(url.as_bytes()),
    })
}
