//! Reviewed planning-only overlays, published to the configured proposal namespace.
use super::{
    git::BoundGit,
    git_write::PushResult,
    local::{LocalState, request_name},
    proposals::*,
    *,
};
use crate::{
    Config, ContentHash, ErrorCode, OperationId, PmError, Repository, RequestId, Result,
    SchemaVersion,
    transactions::{ChangedPath, MutationReceipt, canonical_hash},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
fn invalid(message: &str) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn stale(message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
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
    plan: ProposalPlan,
    candidate: GitOid,
    state: PublicationState,
    confirmation: Option<PublicationConfirmation>,
}
fn fingerprint(plan: &ProposalPlan) -> Result<ContentHash> {
    let mut value =
        serde_json::to_value(plan).map_err(|_| invalid("cannot encode proposal plan"))?;
    value
        .as_object_mut()
        .ok_or_else(|| invalid("invalid proposal plan"))?
        .remove("fingerprint");
    canonical_hash(&value)
}
pub(super) fn validate(plan: &ProposalPlan) -> Result<()> {
    if serde_json::to_vec(plan)
        .map_err(|_| invalid("cannot encode proposal plan"))?
        .len()
        > MAX_PROPOSAL_PLAN_BYTES
    {
        return Err(invalid(
            "proposal plan exceeds its 8 MiB complete JSON bound",
        ));
    }
    if plan.title.trim().is_empty()
        || plan.title.len() > 4096
        || plan.title.chars().any(char::is_control)
        || plan.source.repository != plan.repository
        || plan.accepted.repository != plan.repository
        || plan.source.role != SourceRole::Proposal
        || plan.accepted.role != SourceRole::Accepted
        || plan.accepted.commit.as_ref() != Some(&plan.application_base)
        || plan.accepted.tree.is_none()
        || plan.accepted.ref_name.as_ref() == Some(&plan.reference)
        || !plan.reference.as_str().starts_with("refs/heads/")
    {
        return Err(invalid(
            "proposal plan has invalid title, source roles or exact accepted application identity",
        ));
    }
    if let Some(previous) = &plan.expected_proposal
        && (previous.repository != plan.repository
            || previous.role != SourceRole::Proposal
            || previous.ref_name.as_ref() != Some(&plan.reference)
            || previous.commit.is_none()
            || previous.tree.is_none())
    {
        return Err(invalid(
            "proposal plan has an invalid reviewed prior proposal",
        ));
    }
    for changes in [&plan.changed, &plan.proposal_changed] {
        let mut previous = None;
        for change in changes {
            if previous
                .as_ref()
                .is_some_and(|p: &PathBuf| p >= &change.path)
                || change.before == change.after
                || !capture_core::authoritative(&change.path)?
                || change.path.starts_with("claims")
                || change.path == Path::new("coordination.yml")
            {
                return Err(invalid(
                    "proposal plan contains unordered, repeated, nonauthority or claim changes",
                ));
            }
            previous = Some(change.path.clone());
        }
    }
    if plan.changed.is_empty() && plan.proposal_changed.is_empty() {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "proposal contains no planning changes",
        ));
    }
    if plan.fingerprint != fingerprint(plan)? {
        return Err(invalid(
            "proposal plan fingerprint does not match its complete reviewed input",
        ));
    }
    Ok(())
}
fn config(repo: &Repository) -> Result<(Config, ContentHash)> {
    repo.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repo.root(), snapshot)?;
        let bytes = snapshot
            .read(Path::new("config.yml"))?
            .ok_or_else(|| stale("configuration disappeared"))?;
        Ok((config, ContentHash::of(&bytes)))
    })
}
fn bound(
    repo: &Repository,
    config: &Config,
) -> Result<(BoundGit, SharedSources, String, ContentHash)> {
    if repo
        .root()
        .file_name()
        .is_none_or(|name| name != ".workdeck")
    {
        return Err(PmError::new(
            ErrorCode::UnsafePath,
            "proposals require canonical .workdeck worktree binding",
        ));
    }
    let shared = config.sources.clone().ok_or_else(|| {
        PmError::new(
            ErrorCode::PolicyBlocked,
            "configure shared sources before proposing planning changes",
        )
    })?;
    let git = BoundGit::open_shared(
        repo.root()
            .parent()
            .ok_or_else(|| invalid("planning root has no worktree"))?,
    )?;
    let url = git.remote_url(&shared.remote)?;
    let binding = git.binding(repo.identity(), &shared, &url)?;
    Ok((git, shared, url, binding))
}
fn reference(shared: &SharedSources, target: &GitRefName) -> Result<()> {
    if !target
        .as_str()
        .starts_with(&format!("{}/", shared.proposal_namespace))
    {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "planning proposals publish only beneath the configured proposal namespace",
        ));
    }
    Ok(())
}
struct CapturedPreview {
    plan: ProposalPlan,
    source: PlanningSourceView,
    accepted: PlanningSourceView,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

pub(super) fn preview(repo: &Repository, request: &ProposalRequest) -> Result<ProposalPlan> {
    Ok(capture_preview(repo, request)?.plan)
}

fn capture_preview(repo: &Repository, request: &ProposalRequest) -> Result<CapturedPreview> {
    let (config, config_content) = config(repo)?;
    let (git, shared, _, binding) = bound(repo, &config)?;
    reference(&shared, &request.reference)?;
    let source = capture(
        git.root(),
        &SourceSelector::WorkingTree,
        &SourceCaptureLimits::default(),
    )?;
    let accepted = capture(
        git.root(),
        &SourceSelector::Accepted,
        &SourceCaptureLimits::default(),
    )?;
    if accepted.snapshot.config()?.sources.as_ref() != Some(&shared) {
        return Err(stale(
            "accepted policy names different shared planning authorities",
        ));
    }
    let previous = match capture(
        git.root(),
        &SourceSelector::Proposal {
            reference: request.reference.clone(),
        },
        &SourceCaptureLimits::default(),
    ) {
        Ok(view) => Some(view),
        Err(error) if error.code == ErrorCode::NotFound => None,
        Err(error) => return Err(error),
    };
    if let Some(previous) = &previous
        && !git.ancestor(
            accepted
                .observation
                .identity
                .commit
                .as_ref()
                .ok_or_else(|| invalid("accepted source has no commit"))?,
            previous
                .observation
                .identity
                .commit
                .as_ref()
                .ok_or_else(|| invalid("prior proposal has no commit"))?,
        )?
    {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "existing proposal does not descend from this accepted base; review a new proposal reference",
        ));
    }
    let files = proposed_files(&source.snapshot, &accepted.snapshot)?;
    let application_base = accepted
        .observation
        .identity
        .commit
        .clone()
        .ok_or_else(|| invalid("accepted source has no commit"))?;
    let mut plan = ProposalPlan {
        schema: SchemaVersion::CURRENT,
        repository: config.repository,
        config: config_content,
        binding,
        source: source.observation.identity.clone(),
        accepted: accepted.observation.identity.clone(),
        expected_proposal: previous.as_ref().map(|v| v.observation.identity.clone()),
        reference: request.reference.clone(),
        title: request.title.clone(),
        changed: diff(accepted.snapshot.files(), &files),
        proposal_changed: diff(
            previous
                .as_ref()
                .map(|v| v.snapshot.files())
                .unwrap_or_else(|| accepted.snapshot.files()),
            &files,
        ),
        application_base,
        fingerprint: ContentHash::of(b"pending"),
    };
    plan.fingerprint = fingerprint(&plan)?;
    validate(&plan)?;
    source.revalidate()?;
    accepted.revalidate()?;
    if let Some(previous) = previous {
        previous.revalidate()?;
    }
    Ok(CapturedPreview {
        plan,
        source,
        accepted,
        files,
    })
}
fn claims_path(path: &Path, bytes: Option<&Vec<u8>>) -> Result<bool> {
    if path.starts_with("claims") || path == Path::new("coordination.yml") {
        return Ok(true);
    }
    if path.starts_with("operations")
        && let Some(bytes) = bytes
    {
        let receipt: MutationReceipt = serde_yaml_ng::from_slice(bytes)
            .map_err(|_| invalid("proposal contains an invalid operation receipt"))?;
        return Ok(receipt.operation.starts_with("claim."));
    }
    Ok(false)
}
fn proposed_files(
    source: &SourceSnapshot,
    accepted: &SourceSnapshot,
) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = accepted.files().clone();
    let keys = source
        .files()
        .keys()
        .chain(accepted.files().keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in keys {
        if claims_path(&path, source.files().get(&path))?
            || claims_path(&path, accepted.files().get(&path))?
        {
            continue;
        }
        match source.files().get(&path) {
            Some(bytes) => {
                files.insert(path, bytes.clone());
            }
            None => {
                files.remove(&path);
            }
        }
    }
    let limits = SourceCaptureLimits::default();
    crate::snapshots::validation::validate_source_files(
        &files,
        &accepted.identity().repository,
        limits.max_entries,
        limits.max_total_bytes,
    )?;
    Ok(files)
}
fn diff(
    before: &BTreeMap<PathBuf, Vec<u8>>,
    after: &BTreeMap<PathBuf, Vec<u8>>,
) -> Vec<ChangedPath> {
    before
        .keys()
        .chain(after.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|path| {
            let before = before.get(&path).map(|b| ContentHash::of(b));
            let after = after.get(&path).map(|b| ContentHash::of(b));
            (before != after).then_some(ChangedPath {
                path,
                before,
                after,
            })
        })
        .collect()
}
fn plan_name(hash: &ContentHash) -> String {
    format!("plans/{hash}.json")
}
pub(super) fn save(repo: &Repository, plan: &ProposalPlan) -> Result<PathBuf> {
    validate(plan)?;
    if plan.repository != *repo.identity() {
        return Err(stale("proposal plan belongs to another repository"));
    }
    repo.config()?;
    let state = LocalState::open(repo)?;
    let name = plan_name(&plan.fingerprint);
    let bytes = serde_json::to_vec(plan).map_err(|_| invalid("cannot encode proposal plan"))?;
    match state.read(&name)? {
        Some(existing) if existing == bytes => {}
        Some(_) => {
            return Err(PmError::new(
                ErrorCode::Conflict,
                "saved proposal plan was changed; preserve it before retrying",
            ));
        }
        None => state.publish(&name, &bytes, None)?,
    }
    state.path(&name)
}
pub(super) fn load(repo: &Repository, hash: &ContentHash) -> Result<ProposalPlan> {
    repo.config()?;
    let bytes = local::read_at(repo.root(), &plan_name(hash))?
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "saved proposal plan does not exist"))?;
    let plan: ProposalPlan =
        serde_json::from_slice(&bytes).map_err(|_| invalid("invalid saved proposal plan"))?;
    validate(&plan)?;
    if plan.repository != *repo.identity() || &plan.fingerprint != hash {
        return Err(stale(
            "saved proposal plan belongs to a different source or hash",
        ));
    }
    Ok(plan)
}
fn read_intent(repo: &Repository, request: &RequestId) -> Result<Intent> {
    let bytes = local::read_at(repo.root(), &request_name(request))?.ok_or_else(|| {
        PmError::new(
            ErrorCode::NotFound,
            "proposal request has no persisted intent",
        )
    })?;
    let intent: Intent = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("request is not a valid proposal intent"))?;
    validate_intent(&intent)?;
    if intent.request_id != *request || intent.repository != *repo.identity() {
        return Err(stale("proposal intent source/request identity differs"));
    }
    Ok(intent)
}
fn validate_intent(intent: &Intent) -> Result<()> {
    validate(&intent.plan)?;
    if intent.operation != "sources.propose"
        || intent.repository != intent.plan.repository
        || intent.input_hash
            != canonical_hash(
                &serde_json::to_value(&intent.plan)
                    .map_err(|_| invalid("invalid proposal input"))?,
            )?
    {
        return Err(invalid(
            "proposal intent does not match its complete original plan",
        ));
    }
    if let Some(confirmation) = &intent.confirmation
        && (confirmation.commit != intent.candidate
            || confirmation.reference != intent.plan.reference)
    {
        return Err(invalid(
            "proposal confirmation differs from retained candidate",
        ));
    }
    Ok(())
}
pub(super) fn status(repo: &Repository, request: &RequestId) -> Result<ProposalOutcome> {
    let intent = read_intent(repo, request)?;
    let (config, _) = config(repo)?;
    let (_, _, _, binding) = bound(repo, &config)?;
    if binding != intent.binding {
        return Err(stale(
            "proposal status belongs to another Git directory or remote binding",
        ));
    }
    Ok(outcome(
        &intent,
        None,
        true,
        vec!["local_intent_status_only".into()],
    ))
}
pub(super) fn resume(repo: &Repository, request: &RequestId) -> Result<ProposalOutcome> {
    let intent = read_intent(repo, request)?;
    publish(repo, &intent.plan, request, |_| Ok(()))
}
pub(super) fn publish(
    repo: &Repository,
    plan: &ProposalPlan,
    request: &RequestId,
    mut fault: impl FnMut(PublicationFaultPoint) -> Result<()>,
) -> Result<ProposalOutcome> {
    validate(plan)?;
    let state = LocalState::open(repo)?;
    let name = request_name(request);
    let input_hash = canonical_hash(
        &serde_json::to_value(plan).map_err(|_| invalid("cannot encode proposal input"))?,
    )?;
    let existing = state.read(&name)?;
    if let Some(bytes) = &existing {
        let value: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|_| invalid("invalid source request journal"))?;
        if value["operation"] != "sources.propose"
            || value["input_hash"] != serde_json::json!(input_hash)
            || value["request_id"] != serde_json::json!(request)
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "request already identifies different proposal inputs",
            ));
        }
    }
    let (config, config_content) = config(repo)?;
    let (mut git, shared, url, binding) = bound(repo, &config)?;
    if plan.binding != binding {
        return Err(stale(
            "reviewed Git directory or remote binding changed before proposal publication",
        ));
    }
    reference(&shared, &plan.reference)?;
    let replayed = existing.is_some();
    let mut intent = if let Some(bytes) = existing {
        let intent: Intent = serde_json::from_slice(&bytes)
            .map_err(|_| invalid("invalid proposal request journal"))?;
        validate_intent(&intent)?;
        if intent.binding != binding || intent.repository != *repo.identity() {
            return Err(stale(
                "proposal request belongs to another Git directory or remote binding",
            ));
        }
        intent
    } else {
        if plan.repository != *repo.identity() || plan.config != config_content {
            return Err(stale("proposal configuration changed before publication"));
        }
        let reviewed = capture_preview(
            repo,
            &ProposalRequest {
                reference: plan.reference.clone(),
                title: plan.title.clone(),
            },
        )?;
        if reviewed.plan != *plan {
            return Err(stale(
                "proposal source changed after review; preview a new plan and request",
            ));
        }
        // Build from the exact immutable bytes used for fresh plan validation.
        // Re-capturing here duplicates Git/filesystem work and introduces another
        // observation; the original views retain their full revalidation guards.
        let source = reviewed.source;
        let accepted = reviewed.accepted;
        if source.observation.identity != plan.source
            || accepted.observation.identity != plan.accepted
        {
            return Err(stale("proposal source changed before capture"));
        }
        let files = reviewed.files;
        if diff(accepted.snapshot.files(), &files) != plan.changed {
            return Err(stale("proposal content differs from reviewed changes"));
        }
        let changes = plan
            .changed
            .iter()
            .map(|change| {
                (
                    Path::new(".workdeck").join(&change.path),
                    files.get(&change.path).cloned(),
                )
            })
            .collect();
        let tree = git.write_tree(plan.accepted.tree.as_ref(), &changes)?;
        let parent = plan
            .expected_proposal
            .as_ref()
            .and_then(|p| p.commit.as_ref())
            .unwrap_or(&plan.application_base);
        let operation_id = OperationId::new();
        let candidate = git.commit_tree(
            &tree,
            Some(parent),
            &format!("{}\n\nWorkdeck proposal {}\n", plan.title, operation_id),
            chrono::Utc::now(),
        )?;
        source.revalidate()?;
        accepted.revalidate()?;
        Intent {
            schema: SchemaVersion::CURRENT,
            repository: repo.identity().clone(),
            request_id: request.clone(),
            operation_id,
            operation: "sources.propose".into(),
            input_hash,
            binding: binding.clone(),
            plan: plan.clone(),
            candidate,
            state: PublicationState::Prepared,
            confirmation: None,
        }
    };
    validate_candidate(&git, &config, &shared, &intent)?;
    state.save(&name, &intent)?;
    let retained: GitRefName =
        format!("refs/workdeck/cache/proposals/{}", intent.operation_id).parse()?;
    git.update_cache(&retained, Some(&intent.candidate))?;
    fault(PublicationFaultPoint::AfterPrepared)?;
    let observed = match observe(
        &state,
        &mut git,
        &url,
        &config,
        &shared,
        &intent.plan.reference,
    ) {
        Ok(observed) => observed,
        Err(error) => {
            return Ok(outcome(
                &intent,
                None,
                replayed,
                vec![
                    "remote_observation_unavailable".into(),
                    format!("observation_error:{:?}", error.code),
                ],
            ));
        }
    };
    if let Some(current) = &observed.proposal
        && git.ancestor(
            &intent.candidate,
            current.identity().commit.as_ref().expect("captured commit"),
        )?
    {
        confirm(&mut intent, &observed, &shared);
        state.save(&name, &intent)?;
        fault(PublicationFaultPoint::AfterConfirmation)?;
        return Ok(outcome(
            &intent,
            observed
                .proposal
                .as_ref()
                .map(|p| observation(p, &observed.remote)),
            true,
            vec![],
        ));
    }
    if intent.state != PublicationState::Prepared {
        return Ok(outcome(
            &intent,
            observed
                .proposal
                .as_ref()
                .map(|p| observation(p, &observed.remote)),
            replayed,
            vec!["publication_not_yet_reconciled".into()],
        ));
    }
    let expected = plan
        .expected_proposal
        .as_ref()
        .and_then(|p| p.commit.as_ref());
    if observed.accepted.identity() != &plan.accepted || observed.remote.commit.as_ref() != expected
    {
        intent.state = PublicationState::Rejected;
        state.save(&name, &intent)?;
        return Err(stale(
            "accepted or proposal ref changed after review; existing branch was preserved",
        ));
    }
    git.verify()?;
    if git.remote_url(&shared.remote)? != url || self_config_changed(repo, &config_content)? {
        return Err(stale("proposal binding changed before publication"));
    }
    intent.state = PublicationState::Uncertain;
    state.save(&name, &intent)?;
    git.check_deadline()?;
    fault(PublicationFaultPoint::BeforePush)?;
    let pushed = git
        .push_candidate(&url, &plan.reference, &intent.candidate)
        .unwrap_or(PushResult::Uncertain);
    fault(PublicationFaultPoint::AfterPush)?;
    if matches!(
        pushed,
        PushResult::Rejected | PushResult::RetryableRejection
    ) {
        intent.state = PublicationState::Rejected;
        state.save(&name, &intent)?;
    }
    let observed = match observe(
        &state,
        &mut git,
        &url,
        &config,
        &shared,
        &intent.plan.reference,
    ) {
        Ok(observed) => observed,
        Err(error) => {
            return Ok(outcome(
                &intent,
                None,
                replayed,
                vec![
                    "remote_observation_unavailable".into(),
                    format!("observation_error:{:?}", error.code),
                ],
            ));
        }
    };
    if let Some(current) = &observed.proposal
        && git.ancestor(
            &intent.candidate,
            current.identity().commit.as_ref().expect("captured commit"),
        )?
    {
        confirm(&mut intent, &observed, &shared);
        state.save(&name, &intent)?;
        fault(PublicationFaultPoint::AfterConfirmation)?;
        return Ok(outcome(
            &intent,
            Some(observation(current, &observed.remote)),
            replayed,
            vec![],
        ));
    }
    Ok(outcome(
        &intent,
        observed
            .proposal
            .as_ref()
            .map(|p| observation(p, &observed.remote)),
        replayed,
        vec![
            if intent.state == PublicationState::Rejected {
                "publication_rejected"
            } else {
                "publication_not_yet_reconciled"
            }
            .into(),
        ],
    ))
}
fn validate_candidate(
    git: &BoundGit,
    config: &Config,
    shared: &SharedSources,
    intent: &Intent,
) -> Result<()> {
    let make_observed = |reference: GitRefName, commit: GitOid| RemoteRefObservation {
        remote: shared.remote.clone(),
        reference,
        commit: Some(commit),
        observed_at: chrono::Utc::now(),
    };
    let accepted = remote_impl::capture_commit(
        git,
        config,
        shared,
        &make_observed(
            shared.accepted_ref.clone(),
            intent.plan.application_base.clone(),
        ),
        &SourceCaptureLimits::default(),
    )?
    .ok_or_else(|| invalid("proposal accepted basis disappeared"))?;
    if accepted.identity() != &intent.plan.accepted {
        return Err(invalid(
            "proposal accepted identity differs from retained Git content",
        ));
    }
    let candidate = remote_impl::capture_commit(
        git,
        config,
        shared,
        &make_observed(intent.plan.reference.clone(), intent.candidate.clone()),
        &SourceCaptureLimits::default(),
    )?
    .ok_or_else(|| invalid("proposal candidate disappeared"))?;
    let parent = intent
        .plan
        .expected_proposal
        .as_ref()
        .and_then(|p| p.commit.clone())
        .unwrap_or_else(|| intent.plan.application_base.clone());
    if git.parents(&intent.candidate)? != vec![parent] {
        return Err(invalid(
            "proposal candidate parent differs from its reviewed prior ref",
        ));
    }
    if diff(accepted.files(), candidate.files()) != intent.plan.changed {
        return Err(invalid(
            "proposal candidate differs from its reviewed accepted-source diff",
        ));
    }
    if let Some(previous) = &intent.plan.expected_proposal {
        let source = remote_impl::capture_commit(
            git,
            config,
            shared,
            &make_observed(
                intent.plan.reference.clone(),
                previous
                    .commit
                    .clone()
                    .ok_or_else(|| invalid("prior proposal has no commit"))?,
            ),
            &SourceCaptureLimits::default(),
        )?
        .ok_or_else(|| invalid("prior proposal disappeared"))?;
        if source.identity() != previous
            || diff(source.files(), candidate.files()) != intent.plan.proposal_changed
        {
            return Err(invalid(
                "proposal candidate differs from reviewed previous-proposal diff",
            ));
        }
    } else if intent.plan.changed != intent.plan.proposal_changed {
        return Err(invalid("new proposal has conflicting review baselines"));
    }
    let actual = git.changed_paths(&intent.plan.application_base, &intent.candidate)?;
    let expected = intent
        .plan
        .changed
        .iter()
        .map(|c| Path::new(".workdeck").join(&c.path))
        .collect::<BTreeSet<_>>();
    if actual != expected {
        return Err(invalid(
            "proposal candidate changes application or unreviewed Git paths",
        ));
    }
    let limits = SourceCaptureLimits::default();
    crate::snapshots::validation::validate_source_files(
        candidate.files(),
        &intent.repository,
        limits.max_entries,
        limits.max_total_bytes,
    )?;
    Ok(())
}
fn self_config_changed(repo: &Repository, expected: &ContentHash) -> Result<bool> {
    Ok(config(repo)?.1 != *expected)
}
struct Observed {
    accepted: SourceSnapshot,
    proposal: Option<SourceSnapshot>,
    remote: RemoteRefObservation,
}
fn observe(
    state: &LocalState,
    git: &mut BoundGit,
    url: &str,
    config: &Config,
    shared: &SharedSources,
    reference: &GitRefName,
) -> Result<Observed> {
    git.check_deadline()?;
    let records = git.observe(
        &shared.remote,
        url,
        &[shared.accepted_ref.clone(), reference.clone()],
    )?;
    for record in &records {
        if let Some(commit) = &record.commit {
            git.fetch_object(url, commit)?;
        }
    }
    let accepted = remote_impl::capture_commit(
        git,
        config,
        shared,
        &records[0],
        &SourceCaptureLimits::default(),
    )?
    .ok_or_else(|| PmError::new(ErrorCode::NotFound, "accepted ref is absent"))?;
    if accepted.config()?.sources.as_ref() != Some(shared) {
        return Err(stale("accepted source names different shared authorities"));
    }
    let proposal = remote_impl::capture_commit(
        git,
        config,
        shared,
        &records[1],
        &SourceCaptureLimits::default(),
    )?;
    git.verify()?;
    if git.remote_url(&shared.remote)? != url {
        return Err(stale("proposal remote binding changed during observation"));
    }
    for record in &records {
        let (name, cache) = remote_impl::cache_name(config, shared, &record.reference)?;
        git.update_cache(&cache, record.commit.as_ref())?;
        state.save(
            &name,
            &remote_impl::CacheRecord {
                repository: config.repository.clone(),
                binding: git.binding(&config.repository, shared, url)?,
                reference: cache,
                observation: record.clone(),
            },
        )?;
    }
    Ok(Observed {
        accepted,
        proposal,
        remote: records[1].clone(),
    })
}
fn observation(snapshot: &SourceSnapshot, remote: &RemoteRefObservation) -> SourceObservation {
    SourceObservation {
        identity: snapshot.identity().clone(),
        observed_at: remote.observed_at,
        remote_observation: Some(remote.clone()),
        freshness: SourceFreshness::CurrentAtObservation,
        reason_codes: vec!["remote_observed".into()],
    }
}
fn confirm(intent: &mut Intent, observed: &Observed, shared: &SharedSources) {
    intent.state = PublicationState::Confirmed;
    if intent.confirmation.is_none() {
        intent.confirmation = Some(PublicationConfirmation {
            remote: shared.remote.clone(),
            reference: intent.plan.reference.clone(),
            commit: intent.candidate.clone(),
            observed_parent: intent
                .plan
                .expected_proposal
                .as_ref()
                .and_then(|p| p.commit.clone()),
            confirmed_at: observed.remote.observed_at,
        });
    }
}
fn outcome(
    intent: &Intent,
    current: Option<SourceObservation>,
    replayed: bool,
    reason_codes: Vec<String>,
) -> ProposalOutcome {
    ProposalOutcome {
        repository: intent.repository.clone(),
        request_id: intent.request_id.clone(),
        operation_id: intent.operation_id.clone(),
        state: if intent.state == PublicationState::Confirmed
            && reason_codes.iter().any(|r| {
                r == "remote_observation_unavailable" || r == "publication_not_yet_reconciled"
            }) {
            PublicationState::Uncertain
        } else {
            intent.state
        },
        plan: intent.plan.clone(),
        candidate: intent.candidate.clone(),
        confirmation: intent.confirmation.clone(),
        current,
        reason_codes,
        replayed,
    }
}
