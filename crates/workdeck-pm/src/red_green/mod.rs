//! Authenticated assertion transitions under an independently pinned red baseline.
//! A verified pair is evidence for this check, not an issue-completion credential.
mod facts;
mod types;
use crate::*;
use std::path::Path;
pub use types::*;
fn blocked(message: &str) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}

pub fn verify_red_green(root: &Path, request: &RedGreenRequest) -> Result<RedGreenAssessment> {
    if request.red_artifact.len() > MAX_REPORT_BYTES
        || request.green_artifact.len() > MAX_REPORT_BYTES
    {
        return Err(blocked("red/green artifacts exceed 4 MiB each"));
    }
    crate::commands::validation::id(&request.check)?;
    if request.baseline.commit == request.candidate {
        return Err(blocked(
            "red/green requires distinct baseline and candidate commits",
        ));
    }
    let now = chrono::Utc::now();
    let red = authenticate_check_report(
        &request.red,
        &request.producer_policy,
        &request.expected_producer_policy,
        &request.baseline.commit,
        now,
    )?;
    let green = authenticate_check_report(
        &request.green,
        &request.producer_policy,
        &request.expected_producer_policy,
        &request.candidate,
        now,
    )?;
    let validation = ci_validate_pinned(
        root,
        &CiValidateRequest {
            base: CiRevision::Commit {
                oid: request.baseline.commit.clone(),
            },
            head: CiRevision::Commit {
                oid: request.candidate.clone(),
            },
        },
        &request.baseline,
    )?;
    if !validation.valid
        || validation.contracts.review_required
        || validation.base != red.source
        || validation.head != green.source
    {
        return Err(blocked(
            "red/green requires valid committed sources and unchanged accepted evaluation contracts",
        ));
    }
    let base = validation
        .contracts
        .base
        .as_ref()
        .ok_or_else(|| blocked("accepted red/green baseline contract is unavailable"))?;
    let head = validation
        .contracts
        .head
        .as_ref()
        .ok_or_else(|| blocked("red/green candidate contract is unavailable"))?;
    if base.fingerprint != head.fingerprint {
        return Err(blocked("red/green exact evaluation contract changed"));
    }
    let definition = base
        .checks
        .iter()
        .find(|c| c.definition.id == request.check)
        .ok_or_else(|| blocked("red/green check must be required by the accepted baseline"))?;
    let requirement = definition
        .definition
        .red_green
        .as_ref()
        .ok_or_else(|| blocked("accepted check has no red/green requirement"))?;
    requirement.validate(&definition.definition.expectation)?;
    let git =
        crate::sources::git::BoundGit::with_shared_limits(root, &SourceCaptureLimits::default())?;
    git.output(
        &[
            "merge-base".into(),
            "--is-ancestor".into(),
            request.baseline.commit.to_string().into(),
            request.candidate.to_string().into(),
        ],
        None,
        128,
    )
    .map_err(|_| blocked("red/green candidate must descend from its accepted red baseline"))?;
    git.verify()?;
    facts::source(root, &red)?;
    facts::source(root, &green)?;
    let changed_inputs = facts::pair(request, &red, &green, definition)?;
    let mut result = RedGreenAssessment {
        basis: RedGreenBasis::AuthenticatedCheckPair,
        repository: base.repository.clone(),
        baseline: request.baseline.clone(),
        candidate: validation.head,
        check: CheckRef {
            id: request.check.clone(),
            definition: definition.content.clone(),
        },
        policy: request.expected_producer_policy.clone(),
        red_producer: red.producer,
        green_producer: green.producer,
        red_run: red.report.publication.intent.intent.id.clone(),
        green_run: green.report.publication.intent.intent.id.clone(),
        red_payload: red.payload,
        green_payload: green.payload,
        red_artifact: ContentHash::of(request.red_artifact.as_bytes()),
        green_artifact: ContentHash::of(request.green_artifact.as_bytes()),
        cases: requirement.cases.clone(),
        changed_inputs,
        assessed_at: now,
        fingerprint: ContentHash::of(b""),
    };
    let mut value = serde_json::to_value(&result).map_err(|e| blocked(&e.to_string()))?;
    value.as_object_mut().unwrap().remove("fingerprint");
    value.as_object_mut().unwrap().remove("assessed_at");
    result.fingerprint = crate::transactions::canonical_hash(
        &serde_json::json!({"domain":"workdeck.red-green.v1","assessment":value}),
    )?;
    Ok(result)
}

/// Admit a proposed red evaluator baseline from prior independent acceptance, then
/// verify unchanged evaluators and real assertion transitions through the green candidate.
/// This does not update a branch, run checks, retain proof, or complete a work item.
pub fn verify_reviewed_red_green(
    root: &Path,
    pair: &RedGreenRequest,
    authority: &RedGreenBaselineReview,
) -> Result<ReviewedRedGreenAssessment> {
    let reviewed = ci_validate_reviewed(
        root,
        &CiValidateRequest {
            base: CiRevision::Commit {
                oid: authority.accepted.commit.clone(),
            },
            head: CiRevision::Commit {
                oid: pair.baseline.commit.clone(),
            },
        },
        &authority.accepted,
        &authority.policy,
        &authority.expected_policy,
        &authority.envelope,
        chrono::Utc::now(),
    )?;
    if !reviewed.valid || reviewed.admission.approval.head_contract != pair.baseline.contract {
        return Err(blocked(
            "review does not admit the exact red baseline contract",
        ));
    }
    let git =
        crate::sources::git::BoundGit::with_shared_limits(root, &SourceCaptureLimits::default())?;
    git.output(
        &[
            "merge-base".into(),
            "--is-ancestor".into(),
            authority.accepted.commit.to_string().into(),
            pair.baseline.commit.to_string().into(),
        ],
        None,
        128,
    )
    .map_err(|_| blocked("reviewed red baseline must descend from prior acceptance"))?;
    git.verify()?;
    let assessment = verify_red_green(root, pair)?;
    Ok(ReviewedRedGreenAssessment {
        review: reviewed.admission,
        pair: assessment,
    })
}

/// Offline consistency only, for retained history. Git ancestry, committed inputs
/// and current independent authority still require full live verification.
pub(crate) fn validate_retained(
    request: &RedGreenRequest,
    review: Option<&RedGreenBaselineReview>,
    now: Timestamp,
) -> Result<()> {
    if request.red_artifact.len() > MAX_REPORT_BYTES
        || request.green_artifact.len() > MAX_REPORT_BYTES
        || request.baseline.commit == request.candidate
    {
        return Err(blocked(
            "retained red/green bounds or commit identities are invalid",
        ));
    }
    crate::commands::validation::id(&request.check)?;
    let red = authenticate_check_report(
        &request.red,
        &request.producer_policy,
        &request.expected_producer_policy,
        &request.baseline.commit,
        now,
    )?;
    let green = authenticate_check_report(
        &request.green,
        &request.producer_policy,
        &request.expected_producer_policy,
        &request.candidate,
        now,
    )?;
    let checks = &red
        .report
        .publication
        .intent
        .intent
        .input
        .plan
        .definitions
        .checks;
    let definition = checks
        .iter()
        .find(|c| c.definition.id == request.check)
        .ok_or_else(|| blocked("retained red/green check definition is missing"))?;
    if !green
        .report
        .publication
        .intent
        .intent
        .input
        .plan
        .definitions
        .checks
        .contains(definition)
    {
        return Err(blocked("retained red/green check definitions differ"));
    }
    facts::pair(request, &red, &green, definition)?;
    if let Some(review) = review {
        let admission = crate::contract_reviews::authenticate(
            &review.envelope,
            &review.policy,
            &review.expected_policy,
            &review.accepted,
            now,
        )?;
        if admission.approval.head != red.source
            || admission.approval.head_contract != request.baseline.contract
        {
            return Err(blocked(
                "retained review does not admit the exact red source/contract",
            ));
        }
    }
    Ok(())
}
