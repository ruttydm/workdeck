use super::{SourceCaptureLimits, capture_core, git::BoundGit};
use crate::{
    CiRevision, CiSourceIdentity, CiValidateRequest, CiValidationReport, ErrorCode, PmError, Result,
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn resolve(git: &BoundGit, revision: &CiRevision) -> Result<super::GitOid> {
    let name = match revision {
        CiRevision::Commit { oid } => {
            return git.resolve_commit(oid)?.ok_or_else(|| {
                PmError::new(ErrorCode::NotFound, "CI revision has no local commit")
            });
        }
        CiRevision::Reference { reference } => reference.as_str(),
        CiRevision::Head {} => "HEAD",
    };
    git.resolve(name)?
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "CI revision has no local commit"))
}

pub(crate) fn capture_commits(
    root: &Path,
    request: &CiValidateRequest,
    mut fault: impl FnMut(crate::CiValidationFaultPoint) -> Result<()>,
) -> Result<CiValidationReport> {
    let limits = SourceCaptureLimits::default();
    let deadline = Instant::now() + Duration::from_secs(limits.timeout_seconds);
    let git = BoundGit::with_deadline(root, &limits, deadline, true)?;
    let base = resolve(&git, &request.base)?;
    let head = resolve(&git, &request.head)?;
    let capture = |commit: &super::GitOid| -> Result<_> {
        let tree = git.tree(commit)?;
        let entries = git.tree_entries(&tree)?;
        let files = capture_core::blobs(&git, &entries, &limits)?;
        let snapshot =
            crate::transactions::Snapshot::from_memory(Path::new("source-snapshot"), &files);
        let config =
            crate::repository::config_from_snapshot(Path::new("source-snapshot"), &snapshot)?;
        capture_core::validate_role(&files, &config, super::SourceRole::Proposal, None)?;
        let identity = CiSourceIdentity {
            repository: config.repository.clone(),
            commit: commit.clone(),
            tree,
            content: capture_core::content_hash(&files)?,
        };
        let report =
            crate::staged_doctor::inspect_source_files(&files, &identity.repository, &limits)?;
        let mut evaluator_limits = limits.clone();
        evaluator_limits.max_total_bytes = limits
            .max_total_bytes
            .saturating_sub(files.values().map(Vec::len).sum());
        evaluator_limits.max_entries = limits.max_entries.saturating_sub(files.len());
        let contract = crate::ci_contracts::capture(&snapshot, &config, |checks| {
            super::evaluators::capture(&git, &identity.tree, checks, &evaluator_limits)
        });
        Ok((identity, report, contract))
    };
    let (base_identity, base_report, base_contract) = capture(&base)?;
    let (head_identity, head_report, head_contract) = capture(&head)?;
    if base_identity.repository != head_identity.repository {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "CI base and head belong to different planning repositories",
        ));
    }
    let contracts = crate::ci_contracts::compare(base_contract, head_contract)?;
    fault(crate::CiValidationFaultPoint::BeforeRevalidation)?;
    git.verify()?;
    if resolve(&git, &request.base)? != base || resolve(&git, &request.head)? != head {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "CI revision moved during validation",
        ));
    }
    if Instant::now() >= deadline {
        return Err(PmError::new(
            ErrorCode::Io,
            "CI validation exceeded its total timeout",
        ));
    }
    Ok(CiValidationReport {
        basis: crate::CiValidationBasis::PlanningSourceValidation,
        baseline_pin: None,
        base: base_identity,
        head: head_identity,
        valid: base_report.valid
            && head_report.valid
            && head_report.policy_compliant
            && contracts.errors.is_empty()
            && !contracts.review_required,
        contracts,
        base_report,
        head_report,
    })
}

/// Resolve a criterion from the exact immutable source already verified by a check pair.
pub(crate) fn resolve_ci_criterion(
    root: &Path,
    source: &CiSourceIdentity,
    criterion: &crate::CriterionRef,
) -> Result<crate::ResolvedCriterion> {
    with_ci_source(root, source, |snapshot, config| {
        crate::gates::resolve_criterion(
            Path::new("source-snapshot"),
            snapshot,
            config,
            &criterion.owner,
            &criterion.id,
        )
    })
}
pub(crate) fn resolve_ci_gate(
    root: &Path,
    source: &CiSourceIdentity,
    gate: &crate::GateId,
) -> Result<crate::GateRecord> {
    with_ci_source(root, source, |snapshot, config| {
        let record = crate::gates::load_gate(snapshot, config, gate)?;
        if record.definition.archived
            || crate::retirement::read_tombstone(
                Path::new("source-snapshot"),
                snapshot,
                config,
                &crate::RetirementTarget::new(crate::RetirementKind::Gate, gate.as_str())?,
            )?
            .is_some()
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "gate was archived or retired in the verified candidate",
            ));
        }
        Ok(record)
    })
}
fn with_ci_source<T>(
    root: &Path,
    source: &CiSourceIdentity,
    read: impl FnOnce(&crate::transactions::Snapshot<'_>, &crate::Config) -> Result<T>,
) -> Result<T> {
    let limits = SourceCaptureLimits::default();
    let git = BoundGit::with_shared_limits(root, &limits)?;
    if git.resolve_commit(&source.commit)?.as_ref() != Some(&source.commit)
        || git.tree(&source.commit)? != source.tree
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "verified candidate commit/tree is unavailable or changed",
        ));
    }
    let entries = git.tree_entries(&source.tree)?;
    let files = capture_core::blobs(&git, &entries, &limits)?;
    let snapshot = crate::transactions::Snapshot::from_memory(Path::new("source-snapshot"), &files);
    let config = crate::repository::config_from_snapshot(Path::new("source-snapshot"), &snapshot)?;
    if config.repository != source.repository
        || capture_core::content_hash(&files)? != source.content
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "planning source differs from verified candidate",
        ));
    }
    let resolved = read(&snapshot, &config)?;
    git.verify()?;
    Ok(resolved)
}
