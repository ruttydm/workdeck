//! Source-bound completion. Only live preflight can construct an admission.
mod gate_proof;
mod proof;
mod types;
use crate::transactions::{FaultPoint, MutationReceipt, Snapshot};
use crate::*;
use std::{cell::RefCell, collections::BTreeSet, path::Path};
pub use types::*;
pub(crate) const OPERATION: &str = "issue.complete_red_green";
pub(crate) const VERIFIED_OPERATION: &str = "issue.complete_verified";
pub(crate) fn is_operation(operation: &str) -> bool {
    matches!(operation, OPERATION | VERIFIED_OPERATION)
}
fn blocked(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
fn stale(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
pub(crate) use proof::{match_check, validate_receipt};

/// Intentionally not deserializable, and its fields are private to this module.
pub(crate) struct Admission {
    pub(crate) git: crate::sources::git::BoundGit,
    pub(crate) input: CompleteVerifiedIssue,
    pub(crate) require_pairs: bool,
    pub(crate) prepared: CiPreparedCheck,
    pub(crate) attestations: Vec<ImportedCheckReportRecord>,
    pub(crate) gates: Vec<CompletionGateAssessment>,
    pub(crate) evidence: Vec<EvidenceRecord>,
}
impl Admission {
    pub(crate) fn conditions(
        &self,
        config: &Config,
        issue: &IssueRecord,
    ) -> Result<Vec<CompletionCondition>> {
        if self.prepared.plan.configuration != *config || self.input.issue != issue.metadata.id {
            return Err(stale(
                "completion policy or subject differs from qualified preflight",
            ));
        }
        let required = issue.metadata.gates.iter().collect::<BTreeSet<_>>();
        let supplied = self
            .gates
            .iter()
            .map(|g| match g {
                CompletionGateAssessment::RedGreen(g) => &g.gate.definition.id,
                CompletionGateAssessment::GreenOnly(g) => &g.gate.definition.id,
            })
            .collect::<BTreeSet<_>>();
        if required != supplied {
            return Err(blocked(
                "completion gate selection is incomplete or contains unrelated gates",
            ));
        }
        let mut conditions = self
            .input
            .checks
            .iter()
            .map(|selection| {
                let definition = self
                    .prepared
                    .plan
                    .definitions
                    .checks
                    .iter()
                    .find(|record| record.definition.id == selection.check)
                    .ok_or_else(|| blocked("completion check definition is missing"))?;
                Ok(CompletionCondition {
                    kind: ConditionKind::Evidence,
                    subject: SubjectRef::Issue(issue.metadata.id.clone()),
                    related_subject: None,
                    state: ConditionState::Satisfied,
                    reason_code: "authenticated_check".into(),
                    message: format!(
                        "Check {} passed for the selected candidate and inputs",
                        selection.check
                    ),
                    path: vec![SubjectRef::Issue(issue.metadata.id.clone())],
                    source_pins: vec![SourcePin {
                        path: definition.path.clone(),
                        content: definition.content.clone(),
                    }],
                    basis: "authenticated_check".into(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        conditions.extend(self.gates.iter().map(|g| {
            let gate = match g {
                CompletionGateAssessment::RedGreen(g) => &g.gate,
                CompletionGateAssessment::GreenOnly(g) => &g.gate,
            };
            CompletionCondition {
                kind: ConditionKind::Gate,
                subject: SubjectRef::Issue(issue.metadata.id.clone()),
                related_subject: Some(SubjectRef::Gate(gate.definition.id.clone())),
                state: ConditionState::Satisfied,
                reason_code: "authenticated_gate".into(),
                message: format!(
                    "Gate {} qualified for the selected candidate",
                    gate.definition.id
                ),
                path: vec![SubjectRef::Gate(gate.definition.id.clone())],
                source_pins: vec![SourcePin {
                    path: gate.path.clone(),
                    content: gate.source.content.clone(),
                }],
                basis: "authenticated_requirements".into(),
            }
        }));
        Ok(conditions)
    }
}
fn prepare(
    repo: &Repository,
    input: &CompleteVerifiedIssue,
    require_pairs: bool,
) -> Result<Admission> {
    if input.checks.is_empty() || input.checks.len() > 256 || input.gates.len() > 256 {
        return Err(blocked(
            "completion requires one through 256 check proofs and at most 256 gates",
        ));
    }
    crate::gates::validation::text(&input.actor, "completion actor", 256)?;
    let root = repo
        .root()
        .parent()
        .ok_or_else(|| blocked("missing worktree"))?;
    let git = crate::sources::git::BoundGit::open_shared(root)?;
    if git.head()?.as_ref() != Some(&input.authority.candidate) {
        return Err(stale("completion candidate is not current HEAD"));
    }
    let prepared = prepare_ci_check(
        root,
        &CiRevision::Commit {
            oid: input.authority.candidate.clone(),
        },
        &CheckPlanRequest {
            issue: Some(input.issue.to_string()),
            ..Default::default()
        },
    )?;
    if prepared
        .plan
        .issue
        .as_ref()
        .is_none_or(|i| i.source != input.expected_issue)
    {
        return Err(stale("issue differs from its inspected completion source"));
    }
    let selected = input
        .checks
        .iter()
        .map(|c| c.check.as_str())
        .collect::<BTreeSet<_>>();
    let required = prepared
        .plan
        .checks
        .iter()
        .map(|c| c.id.as_str())
        .collect::<BTreeSet<_>>();
    if selected.len() != input.checks.len() || selected != required {
        return Err(blocked(
            "select exactly every configured required check, including required profiles",
        ));
    }
    let mut attestations = Vec::new();
    for selection in &input.checks {
        let record = repo.imported_check_report(&selection.attestation)?;
        if record.content != selection.expected_attestation {
            return Err(stale(
                "completion attestation differs from its selected pin",
            ));
        }
        let report = if require_pairs {
            let authority = input
                .authority
                .red_green
                .as_ref()
                .ok_or_else(|| blocked("red/green completion requires pair authority"))?;
            let pair = repo.reauthenticate_imported_red_green(&selection.attestation, authority)?;
            if pair.pair.check.id != selection.check
                || pair.pair.candidate != prepared.binding.source
            {
                return Err(blocked(
                    "completion proof qualifies another check or candidate",
                ));
            }
            repo.reauthenticate_imported_report(
                &selection.attestation,
                &input.authority.policy,
                &input.authority.expected_policy,
                &input.authority.candidate,
            )?
        } else {
            repo.verify_imported_check(&VerifyImportedCheck {
                attestation: selection.attestation.clone(),
                expected_attestation: selection.expected_attestation.clone(),
                check: selection.check.clone(),
                candidate: input.authority.candidate.clone(),
                policy: input.authority.policy.clone(),
                expected_policy: input.authority.expected_policy.clone(),
                red_green: if record.record.input.red_green.is_some() {
                    input.authority.red_green.clone()
                } else {
                    None
                },
            })?
        };
        proof::match_check(&prepared, selection, &report)?;
        attestations.push(record);
    }
    let mut gates = Vec::new();
    let mut ids = BTreeSet::new();
    for selection in &input.gates {
        if !ids.insert(&selection.gate) {
            return Err(blocked("duplicate completion gate selection"));
        }
        let gate = if let Some(authority) = input.authority.red_green.clone() {
            CompletionGateAssessment::RedGreen(repo.verify_red_green_gate(
                &RedGreenGateRequest {
                    gate: selection.gate.clone(),
                    expected_gate: selection.expected_gate.clone(),
                    evidence: selection.evidence.clone(),
                    authority,
                },
            )?)
        } else {
            CompletionGateAssessment::GreenOnly(repo.verify_verified_gate(
                &VerifiedGateRequest {
                    gate: selection.gate.clone(),
                    expected_gate: selection.expected_gate.clone(),
                    evidence: selection.evidence.clone(),
                    authority: input.authority.clone(),
                },
            )?)
        };
        gates.push(gate);
    }
    let mut evidence = Vec::new();
    for gate in &gates {
        let refs = match gate {
            CompletionGateAssessment::RedGreen(gate) => gate
                .requirements
                .iter()
                .map(|requirement| {
                    (
                        requirement.proof.evidence.clone(),
                        requirement.proof.evidence_content.clone(),
                        requirement.proof.attestation.clone(),
                        requirement.proof.attestation_content.clone(),
                    )
                })
                .collect::<Vec<_>>(),
            CompletionGateAssessment::GreenOnly(gate) => gate
                .requirements
                .iter()
                .map(|requirement| {
                    (
                        requirement.proof.evidence.clone(),
                        requirement.proof.evidence_content.clone(),
                        requirement.proof.attestation.clone(),
                        requirement.proof.attestation_content.clone(),
                    )
                })
                .collect::<Vec<_>>(),
        };
        for (evidence_id, evidence_content, attestation_id, attestation_content) in refs {
            let record = repo.evidence(&evidence_id)?;
            if record.content != evidence_content {
                return Err(stale("gate evidence changed during completion preflight"));
            }
            if !attestations.iter().any(|r| r.record.id == attestation_id) {
                let record = repo.imported_check_report(&attestation_id)?;
                if record.content != attestation_content {
                    return Err(stale(
                        "gate attestation changed during completion preflight",
                    ));
                }
                attestations.push(record);
            }
            if !evidence
                .iter()
                .any(|r: &EvidenceRecord| r.reference.id == record.reference.id)
            {
                evidence.push(record);
            }
        }
    }
    git.verify()?;
    if git.head()?.as_ref() != Some(&input.authority.candidate) {
        return Err(stale("HEAD moved during completion preflight"));
    }
    Ok(Admission {
        git,
        input: input.clone(),
        require_pairs,
        prepared,
        attestations,
        gates,
        evidence,
    })
}

/// Prepare authenticated completion evidence for a claim transaction without
/// opening a second planning transaction. The returned admission retains the
/// original Git binding for the final journal boundary.
pub(crate) fn prepare_claimed(
    repo: &Repository,
    input: &CompleteVerifiedIssue,
) -> Result<Admission> {
    prepare(repo, input, input.authority.red_green.is_some())
}

/// Revalidate authenticated completion evidence while a caller already owns the
/// planning snapshot lock. This is the claim-aware equivalent of the ordinary
/// verified completion preflight.
pub(crate) fn revalidate_claimed(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    admission: &Admission,
    issue: &IssueRecord,
    now: Timestamp,
) -> Result<(
    CompletionReport,
    Vec<crate::execution::inputs::CapturedInputs>,
)> {
    if admission.input.issue != issue.metadata.id || admission.input.expected_issue != issue.source
    {
        return Err(stale(
            "claimed verification subject changed after preflight",
        ));
    }
    let guards = crate::checks::prepare_plan(root, snapshot, config, &admission.prepared.plan)?;
    proof::revalidate(snapshot, config, admission, now)?;
    crate::graph::preflight(snapshot)?;
    crate::retirement::ensure_writable(
        root,
        snapshot,
        config,
        &RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.as_str())?,
    )?;
    crate::organization::validate_actor(snapshot, &config.repository, &admission.input.actor)?;
    crate::organization::validate_issue_change(
        snapshot,
        config,
        Some(&issue.metadata),
        &issue.metadata,
        true,
    )?;
    let report =
        crate::issues::completion_admitted(root, snapshot, config, issue, Some(admission))?;
    for guard in &guards {
        guard.verify()?;
    }
    revalidate_claimed_at_journal(admission, &admission.input, now)?;
    if !report.allowed {
        return Err(blocked(report.reasons.join("; ")));
    }
    Ok((report, guards))
}

/// Recheck the retained external and evidence identities immediately before a
/// claim transaction journals its issue change.
pub(crate) fn revalidate_claimed_at_journal(
    admission: &Admission,
    input: &CompleteVerifiedIssue,
    now: Timestamp,
) -> Result<()> {
    admission.git.verify()?;
    if admission.git.head()?.as_ref() != Some(&input.authority.candidate) {
        return Err(stale("HEAD moved before claimed completion publication"));
    }
    proof::authenticate_all(
        &admission.prepared,
        input,
        &admission.attestations,
        admission.require_pairs,
        now,
    )?;
    proof::gate_windows(&admission.gates, &admission.evidence, now)
}
impl Repository {
    pub fn red_green_completion_report(
        &self,
        input: &CompleteRedGreenIssue,
    ) -> Result<CompletionReport> {
        self.completion_report_verified(&CompleteVerifiedIssue::from(input), true)
    }
    pub fn verified_completion_report(
        &self,
        input: &CompleteVerifiedIssue,
    ) -> Result<CompletionReport> {
        self.completion_report_verified(input, false)
    }
    fn completion_report_verified(
        &self,
        input: &CompleteVerifiedIssue,
        require_pairs: bool,
    ) -> Result<CompletionReport> {
        let admission = prepare(self, input, require_pairs)?;
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let guards = crate::checks::prepare_plan(
                self.root(),
                snapshot,
                &config,
                &admission.prepared.plan,
            )?;
            proof::revalidate(snapshot, &config, &admission, chrono::Utc::now())?;
            crate::graph::preflight(snapshot)?;
            let issue =
                crate::issues::resolve_issue(self.root(), snapshot, &config, input.issue.as_str())?;
            crate::retirement::ensure_writable(
                self.root(),
                snapshot,
                &config,
                &RetirementTarget::new(RetirementKind::Issue, input.issue.as_str())?,
            )?;
            crate::organization::validate_issue_change(
                snapshot,
                &config,
                Some(&issue.metadata),
                &issue.metadata,
                true,
            )?;
            let report = crate::issues::completion_admitted(
                self.root(),
                snapshot,
                &config,
                &issue,
                Some(&admission),
            )?;
            for guard in guards {
                guard.verify()?;
            }
            let git = &admission.git;
            if git.head()?.as_ref() != Some(&input.authority.candidate) {
                return Err(stale("HEAD moved before completion assessment"));
            }
            git.verify()?;
            Ok(report)
        })
    }
    pub fn complete_red_green_issue(
        &self,
        input: &CompleteRedGreenIssue,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.complete_red_green_issue_with_faults(input, request, || Ok(()), |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn complete_red_green_issue_with_faults(
        &self,
        input: &CompleteRedGreenIssue,
        request: &RequestId,
        before_transaction: impl FnOnce() -> Result<()>,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.complete_admitted_issue(
            &CompleteVerifiedIssue::from(input),
            request,
            OPERATION,
            serde_json::json!(input),
            before_transaction,
            fault,
        )
    }
    pub fn complete_verified_issue(
        &self,
        input: &CompleteVerifiedIssue,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.complete_verified_issue_with_faults(input, request, || Ok(()), |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn complete_verified_issue_with_faults(
        &self,
        input: &CompleteVerifiedIssue,
        request: &RequestId,
        before_transaction: impl FnOnce() -> Result<()>,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.complete_admitted_issue(
            input,
            request,
            VERIFIED_OPERATION,
            serde_json::json!(input),
            before_transaction,
            fault,
        )
    }
    fn complete_admitted_issue(
        &self,
        input: &CompleteVerifiedIssue,
        request: &RequestId,
        operation: &str,
        intent: serde_json::Value,
        before_transaction: impl FnOnce() -> Result<()>,
        mut fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let store = self.store()?;
        if let Some(receipt) = store.replay_receipt(request, operation, &intent)? {
            validate_receipt(&receipt)?;
            return Ok(receipt);
        }
        let admission = match prepare(self, input, operation == OPERATION) {
            Ok(admission) => admission,
            Err(error) => {
                if let Some(receipt) = store.replay_receipt(request, operation, &intent)? {
                    validate_receipt(&receipt)?;
                    return Ok(receipt);
                }
                return Err(error);
            }
        };
        before_transaction()?;
        let guards = RefCell::new(Vec::new());
        let git = &admission.git;
        let receipt = store.transact_with_faults(
            request,
            operation,
            &intent,
            |snapshot| {
                let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                let captured = crate::checks::prepare_plan(
                    self.root(),
                    snapshot,
                    &config,
                    &admission.prepared.plan,
                )?;
                proof::revalidate(snapshot, &config, &admission, chrono::Utc::now())?;
                crate::graph::preflight(snapshot)?;
                let before = crate::issues::resolve_issue(
                    self.root(),
                    snapshot,
                    &config,
                    input.issue.as_str(),
                )?;
                if before.source != input.expected_issue {
                    return Err(stale("issue changed after completion preflight"));
                }
                crate::organization::validate_actor(snapshot, &config.repository, &input.actor)?;
                let completion = crate::issues::completion_admitted(
                    self.root(),
                    snapshot,
                    &config,
                    &before,
                    Some(&admission),
                )?;
                if !completion.allowed {
                    return Err(blocked(completion.reasons.join("; ")));
                }
                let before_document = String::from_utf8(
                    snapshot
                        .read(&before.path)?
                        .ok_or_else(|| stale("issue disappeared"))?,
                )
                .map_err(|_| blocked("issue is not UTF-8"))?;
                let mut result = crate::issues::prepare_issue_mutation_admitted(
                    self.root(),
                    snapshot,
                    &config,
                    before.clone(),
                    Some(&input.expected_issue),
                    &IssueMutation::Complete { manual: None },
                    Some(&admission),
                )?;
                let after: IssueRecord = serde_json::from_value(result.result.clone())
                    .map_err(|e| blocked(e.to_string()))?;
                let after_document = String::from_utf8(
                    result
                        .changes
                        .iter()
                        .find(|c| c.path == after.path)
                        .and_then(|c| c.content.clone())
                        .ok_or_else(|| blocked("completion produced no issue change"))?,
                )
                .map_err(|_| blocked("issue is not UTF-8"))?;
                let proof = VerifiedIssueCompletion {
                    input: input.clone(),
                    before,
                    after,
                    before_document,
                    after_document,
                    completion,
                    prepared: admission.prepared.clone(),
                    attestations: admission.attestations.clone(),
                    gates: admission.gates.clone(),
                    evidence: admission.evidence.clone(),
                    admitted_at: chrono::Utc::now(),
                };
                result.result = serde_json::to_value(proof).map_err(|e| blocked(e.to_string()))?;
                // Preserve the original request bytes/shape in both operation protocols.
                result.result["input"] = intent.clone();
                validate_receipt(&MutationReceipt {
                    schema_version: SchemaVersion::CURRENT,
                    repository: Some(config.repository.clone()),
                    operation_id: OperationId::new(),
                    request_id: request.clone(),
                    operation: operation.into(),
                    input_hash: crate::transactions::canonical_hash(&intent)?,
                    result: result.result.clone(),
                    changed: result
                        .changes
                        .iter()
                        .map(|c| crate::transactions::ChangedPath {
                            path: c.path.clone(),
                            before: c.expected.clone(),
                            after: c.content.as_deref().map(ContentHash::of),
                        })
                        .collect(),
                })?;
                *guards.borrow_mut() = captured;
                Ok(result)
            },
            |point| {
                fault(point)?;
                if point == FaultPoint::BeforeJournal {
                    git.verify()?;
                    if git.head()?.as_ref() != Some(&input.authority.candidate) {
                        return Err(stale("HEAD moved before completion publication"));
                    }
                    for guard in guards.borrow().iter() {
                        guard.verify()?;
                    }
                    proof::authenticate_all(
                        &admission.prepared,
                        input,
                        &admission.attestations,
                        admission.require_pairs,
                        chrono::Utc::now(),
                    )?;
                    proof::gate_windows(&admission.gates, &admission.evidence, chrono::Utc::now())?;
                }
                Ok(())
            },
        )?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}
