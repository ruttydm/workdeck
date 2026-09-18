//! Shared revision/subject-bound assessment; retained policy snapshots never renew trust.
mod types;
mod working;
use crate::transactions::Snapshot;
use crate::*;
use serde_json::json;
use std::path::Path;
pub use types::*;

impl Repository {
    pub fn contract_review_coverage(
        &self,
        request: &ReviewCoverageRequest,
    ) -> Result<ReviewCoverage> {
        self.store()?
            .with_snapshot(|snapshot| capture(self.root(), snapshot, self.identity(), request))
    }
}
pub(crate) fn capture(
    root: &Path,
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
    request: &ReviewCoverageRequest,
) -> Result<ReviewCoverage> {
    if request.expected_subject.is_some() && request.subject.is_none() {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "selected subject content requires a subject identity",
        ));
    }
    let records = crate::retained_reviews::load(snapshot, repository)?;
    capture_records(root, snapshot, repository, request, records)
}
pub(crate) fn capture_records(
    root: &Path,
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
    request: &ReviewCoverageRequest,
    records: Vec<ImportedContractReviewRecord>,
) -> Result<ReviewCoverage> {
    let validation_request = CiValidateRequest {
        base: request
            .authority
            .as_ref()
            .map(|a| CiRevision::Commit {
                oid: a.baseline.commit.clone(),
            })
            .unwrap_or_else(|| request.revision.clone()),
        head: request.revision.clone(),
    };
    let worktree = root
        .parent()
        .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "missing worktree root"))?;
    let validation = match &request.authority {
        Some(authority) => ci_validate_pinned(worktree, &validation_request, &authority.baseline),
        None => ci_validate(worktree, &validation_request),
    };
    let now = chrono::Utc::now();
    let mut diagnostics = Vec::new();
    let mut subject = None;
    let mut source = None;
    let mut common = None;
    let mut contract = None;
    let mut working_tree = None;
    match validation {
        Err(error) => {
            common = Some((
                ReviewCoverageState::Unknown,
                "candidate_validation_unavailable",
            ));
            diagnostics.push(error);
        }
        Ok(validation) => {
            if validation.head.repository != *repository {
                return Err(PmError::new(
                    ErrorCode::PolicyBlocked,
                    "coverage candidate belongs to another repository",
                ));
            }
            source = Some(validation.head);
            if let Some(head) = &validation.contracts.head {
                if request.working_tree {
                    match working::capture(root, snapshot, head) {
                        Ok(current) => {
                            if !current.matches_revision {
                                common = Some((
                                    ReviewCoverageState::Stale,
                                    "working_contract_or_evaluators_differ_from_commit",
                                ));
                            }
                            working_tree = Some(current);
                        }
                        Err(error) => {
                            common = Some((
                                ReviewCoverageState::Unknown,
                                "working_contract_capture_unavailable",
                            ));
                            diagnostics.push(error);
                        }
                    }
                }
                contract = Some(head.fingerprint.clone());
                subject = request
                    .subject
                    .as_ref()
                    .and_then(|id| head.subjects.iter().find(|s| &s.subject == id).cloned());
            }
            if !validation.base_report.valid
                || !validation.head_report.valid
                || !validation.head_report.policy_compliant
                || !validation.contracts.errors.is_empty()
            {
                common = Some((ReviewCoverageState::Rejected, "candidate_planning_invalid"));
                diagnostics.extend(validation.base_report.errors);
                diagnostics.extend(validation.head_report.errors);
                diagnostics.extend(validation.head_report.policy_violations.into_iter().map(|violation|
                    PmError::new(ErrorCode::PolicyBlocked, violation.message).at(&violation.path)
                        .details(json!({"field":violation.field,"historical":violation.historical,"source":"head"}))));
                diagnostics.extend(
                    validation
                        .contracts
                        .errors
                        .into_iter()
                        .map(|error| error.error),
                );
            } else if request.subject.is_some() && subject.is_none() {
                common = Some((
                    ReviewCoverageState::Unknown,
                    "subject_has_no_captured_review_contract",
                ));
            } else if request
                .expected_subject
                .as_ref()
                .is_some_and(|expected| subject.as_ref().map(|s| &s.content) != Some(expected))
            {
                common = Some((
                    ReviewCoverageState::Stale,
                    "selected_subject_differs_from_commit",
                ));
            }
        }
    }
    let mut rows = Vec::new();
    for record in records {
        let approval = &record.record.admission.approval;
        let mut current_reviewers = Vec::new();
        let (state, reason, diagnostic) = if let Some((state, reason)) = common {
            (state, reason, None)
        } else if source.as_ref() != Some(&approval.head)
            || contract.as_ref() != Some(&approval.head_contract)
        {
            (
                ReviewCoverageState::Stale,
                "review_targets_different_candidate",
                None,
            )
        } else if now >= approval.expires_at {
            (ReviewCoverageState::Stale, "signed_approval_expired", None)
        } else if let Some(authority) = &request.authority {
            let envelope =
                SignedContractReview::from_json(record.record.input.envelope.as_bytes())?;
            match crate::contract_reviews::authenticate(
                &envelope,
                &authority.policy,
                &authority.expected_policy,
                &authority.baseline,
                now,
            ) {
                Ok(admission) => {
                    current_reviewers = admission.reviewers;
                    (
                        ReviewCoverageState::Authenticated,
                        "current_review_policy_authenticated",
                        None,
                    )
                }
                Err(error) => (
                    ReviewCoverageState::Rejected,
                    "current_review_policy_rejected",
                    Some(error),
                ),
            }
        } else {
            (
                ReviewCoverageState::HistoricalMatch,
                "independent_current_policy_required",
                None,
            )
        };
        rows.push(ReviewCoverageRow {
            review: record.into(),
            state,
            reason_codes: vec![reason.into()],
            diagnostic,
            current_reviewers,
        });
    }
    // Timestamp is an observation, not a changing source pin. Expiry changes the state hash.
    let fingerprint = crate::transactions::canonical_hash(
        &json!({"repository":repository,"request":request,
        "source":source,"subject":subject,"working_tree":working_tree,"diagnostics":diagnostics,"rows":rows}),
    )?;
    Ok(ReviewCoverage {
        repository: repository.clone(),
        request: request.clone(),
        source,
        subject,
        working_tree,
        diagnostics,
        authenticated: rows
            .iter()
            .any(|r| r.state == ReviewCoverageState::Authenticated),
        rows,
        assessed_at: now,
        fingerprint,
    })
}
