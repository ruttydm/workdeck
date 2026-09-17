use super::{
    files::{self, DirectoryPin, MAX_HOOK, MAX_PROOF},
    *,
};
use crate::{
    ContentHash, Repository, RepositoryId,
    sources::{
        fs as source_fs,
        git::{BoundGit, HookTarget},
    },
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

const BODY_V1: &str = "#!/bin/sh\n# Workdeck managed pre-commit hook v1; use workdeck hooks to update or remove.\nexec workdeck doctor --staged\n";
const SCOPE: &str = "local_git_hook";
const SNIPPET: &str = "# Integrate explicitly into your existing hook; preserve its existing checks.\nworkdeck doctor --staged || exit $?\n";
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn stale(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
fn corrupt(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::CorruptStore, message)
}
fn hash(bytes: Option<&[u8]>) -> Option<ContentHash> {
    bytes.map(ContentHash::of)
}
fn encode(value: &impl Serialize) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))
}
fn owned(bytes: Option<&[u8]>) -> bool {
    bytes == Some(BODY_V1.as_bytes())
}

struct Context {
    git: BoundGit,
    repository: RepositoryId,
    configuration: ContentHash,
    planning_root: PathBuf,
    planning_identity: source_fs::Identity,
    coordinator: Option<files::Lock>,
}
impl Context {
    fn open(worktree: &Path) -> Result<Self> {
        let git = BoundGit::open(worktree)?;
        let repository = Repository::discover(git.root())?;
        if repository.root() != git.root().join(".workdeck") {
            return Err(invalid(
                "hooks require the selected worktree's native planning source",
            ));
        }
        let planning_root = repository.root().to_owned();
        let planning_identity = source_fs::directory(&planning_root)?;
        let bytes = source_fs::read(
            &planning_root.join("config.yml"),
            crate::documents::MAX_DOCUMENT_BYTES,
        )?
        .0;
        let config: crate::Config = crate::documents::YamlDocument::parse(
            Path::new("config.yml"),
            std::str::from_utf8(&bytes)
                .map_err(|_| invalid("planning configuration must be UTF-8"))?,
        )?
        .deserialize()?;
        config.validate()?;
        if &config.repository != repository.identity() {
            return Err(stale(
                "planning repository changed while inspecting hook configuration",
            ));
        }
        Ok(Self {
            git,
            repository: config.repository,
            configuration: ContentHash::of(&bytes),
            planning_root,
            planning_identity,
            coordinator: None,
        })
    }
    fn local(&self) -> PathBuf {
        self.planning_root.join(".local/hooks")
    }
    fn journal(&self) -> PathBuf {
        self.local().join("journal.json")
    }
    fn receipt(&self, request: &RequestId) -> PathBuf {
        self.local()
            .join("requests")
            .join(format!("{request}.json"))
    }
    fn verify(&self) -> Result<()> {
        if let Some(coordinator) = &self.coordinator {
            coordinator.verify()?;
        }
        self.git.verify()?;
        if source_fs::directory(&self.planning_root)? != self.planning_identity
            || ContentHash::of(
                &source_fs::read(
                    &self.planning_root.join("config.yml"),
                    crate::documents::MAX_DOCUMENT_BYTES,
                )?
                .0,
            ) != self.configuration
        {
            return Err(stale("planning source changed during hook publication"));
        }
        crate::restore::check_root(&self.planning_root)?;
        crate::migration::check_root(&self.planning_root)?;
        Ok(())
    }
    fn verify_target(&self, plan: &HookPlan) -> Result<()> {
        self.verify()?;
        let target = self.git.hook_target()?;
        if target.directory.join("pre-commit") != plan.target
            || target.configuration != plan.hook_configuration
            || target.worktree != plan.worktree
            || self.configuration != plan.planning_configuration
        {
            return Err(stale(
                "configured hook destination or planning source differs from the reviewed plan",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    receipt: HookReceipt,
    before: Option<Vec<u8>>,
    after: Option<Vec<u8>>,
    parents: Vec<DirectoryPin>,
}
fn fingerprint(plan: &HookPlan) -> Result<ContentHash> {
    let mut value = serde_json::to_value(plan).map_err(|error| invalid(error.to_string()))?;
    value
        .as_object_mut()
        .expect("hook plan is an object")
        .remove("fingerprint");
    Ok(ContentHash::of(&encode(&(
        "workdeck.local-hook-plan.v1",
        value,
    ))?))
}
fn prepare(
    context: &Context,
    mode: HookMode,
) -> Result<(HookPlan, Option<Vec<u8>>, Vec<DirectoryPin>)> {
    let HookTarget {
        directory,
        configuration,
        worktree,
    } = context.git.hook_target()?;
    let target = directory.join("pre-commit");
    let parents = files::parents(&target)?;
    let before = files::read(&target, MAX_HOOK)?;
    let before_mode = files::mode(&target)?;
    let mut blockers = Vec::new();
    if before.is_some() && !owned(before.as_deref()) {
        blockers.push("Existing pre-commit hook is not an exact Workdeck-owned hook; integrate the displayed snippet explicitly without replacing its checks.".into());
    } else if before.is_none() && matches!(mode, HookMode::Update | HookMode::Remove) {
        blockers.push("No Workdeck hook is installed; use a reviewed install operation.".into());
    }
    let generated_hook = (mode != HookMode::Remove).then(|| BODY_V1.into());
    let after = generated_hook.as_deref().map(str::as_bytes);
    let after_mode = after.map(|_| before_mode.unwrap_or(0o755) | 0o111);
    let mut plan = HookPlan {
        schema_version: 1,
        repository: context.repository.clone(),
        worktree,
        target,
        parent_identity: files::parent_hash(&parents)?,
        hook_configuration: configuration,
        planning_configuration: context.configuration.clone(),
        mode,
        before: hash(before.as_deref()),
        before_mode,
        after: hash(after),
        after_mode,
        changed: before.as_deref() != after || before_mode != after_mode,
        generated_hook,
        integration_snippet: SNIPPET.into(),
        allowed: blockers.is_empty(),
        blockers,
        fingerprint: ContentHash::of(b"pending"),
    };
    plan.fingerprint = fingerprint(&plan)?;
    files::verify_parents(&parents)?;
    context.verify_target(&plan)?;
    if files::read(&plan.target, MAX_HOOK)? != before || files::mode(&plan.target)? != before_mode {
        return Err(stale("hook changed while preparing its review plan"));
    }
    Ok((plan, before, parents))
}
fn validate(proof: &Proof, context: &Context) -> Result<()> {
    let receipt = &proof.receipt;
    let plan = &receipt.plan;
    let after = (plan.mode != HookMode::Remove).then_some(BODY_V1.as_bytes());
    let mode = after.map(|_| plan.before_mode.unwrap_or(0o755) | 0o111);
    if receipt.schema_version != 1
        || plan.schema_version != 1
        || receipt.scope != SCOPE
        || receipt.repository != context.repository
        || plan.repository != receipt.repository
        || plan.worktree != context.git.root()
        || !plan.target.is_absolute()
        || plan
            .target
            .file_name()
            .is_none_or(|name| name != "pre-commit")
        || !plan.allowed
        || !plan.blockers.is_empty()
        || fingerprint(plan)? != plan.fingerprint
        || files::parent_hash(&proof.parents)? != plan.parent_identity
        || plan.before != hash(proof.before.as_deref())
        || plan.after != hash(proof.after.as_deref())
        || proof.after.as_deref() != after
        || plan.generated_hook.as_deref().map(str::as_bytes) != after
        || plan.integration_snippet != SNIPPET
        || plan.before_mode.is_some() != proof.before.is_some()
        || plan.after_mode != mode
        || plan.before_mode.is_some_and(|mode| mode > 0o777)
        || plan.changed != (proof.before != proof.after || plan.before_mode != plan.after_mode)
        || proof
            .before
            .as_ref()
            .is_some_and(|bytes| bytes.len() > MAX_HOOK || !owned(Some(bytes)))
        || proof.before.is_none() && plan.mode != HookMode::Install
    {
        return Err(corrupt(
            "local hook receipt has inconsistent input, source, mode, or generated-body proof",
        ));
    }
    Ok(())
}
fn decode(path: &Path, context: &Context) -> Result<Option<Proof>> {
    files::read(path, MAX_PROOF)?
        .map(|bytes| {
            let proof: Proof = serde_json::from_slice(&bytes)
                .map_err(|error| corrupt(format!("invalid local hook record: {error}")).at(path))?;
            validate(&proof, context)?;
            Ok(proof)
        })
        .transpose()
}
fn pending(context: &Context) -> Result<Option<Proof>> {
    decode(&context.journal(), context)
}
fn recovery(proof: &Proof) -> PmError {
    PmError::new(ErrorCode::RecoveryRequired, "an interrupted hook publication requires its original request to recover")
        .hint(format!("Run workdeck hooks recover --request-id {}.", proof.receipt.request_id))
        .details(serde_json::json!({"request_id":proof.receipt.request_id,"target":proof.receipt.plan.target,"scope":SCOPE}))
}
fn require_journal(context: &Context, proof: &Proof) -> Result<()> {
    context.verify()?;
    if files::read(&context.journal(), MAX_PROOF)?.as_deref() != Some(encode(proof)?.as_slice()) {
        return Err(
            stale("hook journal changed or disappeared; publication stopped").at(context.journal()),
        );
    }
    Ok(())
}
fn remove_journal(context: &Context, proof: &Proof) -> Result<()> {
    require_journal(context, proof)?;
    fs::remove_file(context.journal()).map_err(|error| PmError::io(context.journal(), error))?;
    files::sync(&context.local())
}
fn replay(
    context: &Context,
    input: &HookApply,
    request: &RequestId,
) -> Result<Option<HookReceipt>> {
    let Some(proof) = decode(&context.receipt(request), context)? else {
        return Ok(None);
    };
    if proof.receipt.request_id != *request
        || proof.receipt.plan.mode != input.mode
        || proof.receipt.plan.fingerprint != input.expected_plan
    {
        return Err(PmError::new(
            ErrorCode::IdempotencyConflict,
            "hook request was already used for a different reviewed intent",
        ));
    }
    if let Some(journal) = pending(context)?
        && journal.receipt.request_id == *request
    {
        if encode(&journal)? != encode(&proof)? {
            return Err(corrupt(
                "completed hook receipt and pending journal disagree",
            ));
        }
        remove_journal(context, &proof)?;
    }
    Ok(Some(proof.receipt))
}
pub(super) fn preview(worktree: &Path, mode: HookMode) -> Result<HookPlan> {
    let context = Context::open(worktree)?;
    if let Some(proof) = pending(&context)? {
        return Err(recovery(&proof));
    }
    let plan = prepare(&context, mode)?.0;
    if let Some(proof) = pending(&context)? {
        return Err(recovery(&proof));
    }
    Ok(plan)
}
pub(super) fn status(worktree: &Path) -> Result<HookStatus> {
    let context = Context::open(worktree)?;
    let (plan, before, _) = prepare(&context, HookMode::Install)?;
    let pending = pending(&context)?;
    context.verify_target(&plan)?;
    Ok(HookStatus {
        repository: context.repository,
        worktree: plan.worktree,
        target: plan.target,
        owned: owned(before.as_deref()),
        content: plan.before,
        executable: plan.before_mode.is_some_and(|mode| mode & 0o111 != 0),
        pending_request: pending.map(|proof| proof.receipt.request_id),
    })
}
fn publish(context: &Context, proof: &Proof) -> Result<()> {
    let plan = &proof.receipt.plan;
    context.verify_target(plan)?;
    files::verify_parents(&proof.parents)?;
    let current = files::read(&plan.target, MAX_HOOK)?;
    let current_mode = files::mode(&plan.target)?;
    if current == proof.after && current_mode == plan.after_mode {
        return Ok(());
    }
    if current != proof.before || current_mode != plan.before_mode {
        return Err(stale(
            "hook bytes or permissions changed; pending publication is preserved",
        ));
    }
    let parent = plan
        .target
        .parent()
        .ok_or_else(|| files::unsafe_path(&plan.target))?;
    files::ensure_directory(parent)?;
    require_journal(context, proof)?;
    context.verify_target(plan)?;
    files::verify_parents(&proof.parents)?;
    if files::read(&plan.target, MAX_HOOK)? != proof.before
        || files::mode(&plan.target)? != plan.before_mode
    {
        return Err(stale("hook changed immediately before publication"));
    }
    if let Some(after) = &proof.after {
        files::write(
            &plan.target,
            after,
            plan.after_mode.expect("validated generated hook mode"),
            proof.before.is_some(),
        )?;
    } else {
        fs::remove_file(&plan.target).map_err(|error| PmError::io(&plan.target, error))?;
        files::sync(parent)?;
    }
    Ok(())
}
fn finish(
    context: &Context,
    proof: &Proof,
    fault: &mut impl FnMut(HookFaultPoint) -> Result<()>,
) -> Result<HookReceipt> {
    let result = (|| {
        fault(HookFaultPoint::BeforePublish)?;
        require_journal(context, proof)?;
        publish(context, proof)?;
        fault(HookFaultPoint::AfterPublish)?;
        require_journal(context, proof)?;
        context.verify()?;
        let path = context.receipt(&proof.receipt.request_id);
        let bytes = encode(proof)?;
        files::write(&path, &bytes, 0o600, false)?;
        fault(HookFaultPoint::AfterReceipt)?;
        remove_journal(context, proof)?;
        Ok(proof.receipt.clone())
    })();
    result.map_err(|mut error: PmError| {
        let published = files::read(&proof.receipt.plan.target, MAX_HOOK).ok().map(|bytes| bytes == proof.after && files::mode(&proof.receipt.plan.target).ok() == Some(proof.receipt.plan.after_mode));
        error.details = Some(Box::new(serde_json::json!({"hook_published":published,"request_id":proof.receipt.request_id,"local_hook_receipt":proof.receipt,"scope":SCOPE})));
        error
    })
}
pub(super) fn apply(
    worktree: &Path,
    input: &HookApply,
    request: &RequestId,
    mut fault: impl FnMut(HookFaultPoint) -> Result<()>,
) -> Result<HookReceipt> {
    let mut context = Context::open(worktree)?;
    // Reject blocked or stale fresh plans before publishing private writer state.
    if decode(&context.receipt(request), &context)?.is_none() && pending(&context)?.is_none() {
        let plan = prepare(&context, input.mode)?.0;
        if plan.fingerprint != input.expected_plan {
            return Err(stale(
                "hook plan changed; inspect a new preview before applying",
            ));
        }
        if !plan.allowed {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "hook installation would replace unmanaged or absent hook ownership",
            )
            .details(serde_json::json!({"plan":plan})));
        }
    }
    files::ensure_local(&context.local())?;
    context.coordinator = Some(files::lock(&context.local())?);
    context.verify()?;
    if let Some(receipt) = replay(&context, input, request)? {
        return Ok(receipt);
    }
    let proof = if let Some(proof) = pending(&context)? {
        if proof.receipt.request_id != *request {
            return Err(recovery(&proof));
        }
        if proof.receipt.plan.mode != input.mode
            || proof.receipt.plan.fingerprint != input.expected_plan
        {
            return Err(PmError::new(
                ErrorCode::IdempotencyConflict,
                "pending hook request has different input",
            ));
        }
        proof
    } else {
        let (plan, before, parents) = prepare(&context, input.mode)?;
        if plan.fingerprint != input.expected_plan {
            return Err(stale(
                "hook plan changed before publication acquired its writer lock",
            ));
        }
        if !plan.allowed {
            return Err(
                PmError::new(ErrorCode::PolicyBlocked, "hook plan is blocked")
                    .details(serde_json::json!({"plan":plan})),
            );
        }
        let after = plan
            .generated_hook
            .as_deref()
            .map(|body| body.as_bytes().to_vec());
        let proof = Proof {
            receipt: HookReceipt {
                schema_version: 1,
                repository: context.repository.clone(),
                request_id: request.clone(),
                plan,
                scope: SCOPE.into(),
            },
            before,
            after,
            parents,
        };
        validate(&proof, &context)?;
        let bytes = encode(&proof)?;
        if bytes.len() > MAX_PROOF {
            return Err(invalid("hook recovery proof exceeds 1 MiB"));
        }
        fault(HookFaultPoint::BeforeJournal)?;
        context.verify_target(&proof.receipt.plan)?;
        files::verify_parents(&proof.parents)?;
        if files::read(&proof.receipt.plan.target, MAX_HOOK)? != proof.before
            || files::mode(&proof.receipt.plan.target)? != proof.receipt.plan.before_mode
        {
            return Err(stale("hook changed before recovery journal publication"));
        }
        files::write(&context.journal(), &bytes, 0o600, false)?;
        fault(HookFaultPoint::AfterJournal)?;
        proof
    };
    finish(&context, &proof, &mut fault)
}
pub(super) fn recover(worktree: &Path, request: &RequestId) -> Result<HookReceipt> {
    let context = Context::open(worktree)?;
    let proof = if let Some(proof) = decode(&context.receipt(request), &context)? {
        proof
    } else {
        pending(&context)?.ok_or_else(|| {
            PmError::new(
                ErrorCode::NotFound,
                "no local hook request exists to recover",
            )
        })?
    };
    if proof.receipt.request_id != *request {
        return Err(recovery(&proof));
    }
    apply(
        worktree,
        &HookApply {
            mode: proof.receipt.plan.mode,
            expected_plan: proof.receipt.plan.fingerprint,
        },
        request,
        |_| Ok(()),
    )
}
