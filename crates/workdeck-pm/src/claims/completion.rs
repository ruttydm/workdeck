use super::*;
use crate::documents::MarkdownDocument;
use crate::transactions::{
    ChangedPath, FaultPoint, MutationReceipt, PreparedOperation, Snapshot, canonical_hash,
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, path::Path};

const OPERATION: &str = "issue.complete_claimed";

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteClaimedIssue {
    pub issue: IssueId,
    pub actor: String,
    pub expected_claim: ClaimPrecondition,
    pub expected_issue: SourceToken,
    pub contract: ClaimWorkContract,
    /// Optional reviewed shared destination binding. Portable work contracts
    /// intentionally do not carry local Git or remote configuration identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_binding: Option<ContentHash>,
}

/// A claimed completion that also carries independently authenticated check and
/// gate proof. The ownership request remains separate so local/shared claim
/// semantics cannot be replaced by a portable CI request.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompleteClaimedVerifiedIssue {
    pub claim: CompleteClaimedIssue,
    pub verification: CompleteVerifiedIssue,
}

/// The retained CI qualification attached to a claimed completion receipt. The
/// claim itself remains the ownership authority; these fields preserve the exact
/// authenticated evidence used by the same transaction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaimedCompletionVerification {
    pub input: CompleteVerifiedIssue,
    pub completion: CompletionReport,
    pub prepared: CiPreparedCheck,
    pub attestations: Vec<ImportedCheckReportRecord>,
    pub gates: Vec<CompletionGateAssessment>,
    pub evidence: Vec<EvidenceRecord>,
    pub admitted_at: Timestamp,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaimedCompletionConfirmation {
    /// Original selected Git directories, source refs and remote URL binding.
    /// This contains no remote credentials and does not infer current ownership.
    pub binding: ContentHash,
    pub accepted: SourceObservation,
    pub coordination: SourceObservation,
    pub observed_at: Timestamp,
    pub contract: ClaimWorkContract,
    pub completion: CompletionReport,
}

/// The historical ownership admission and exact issue edit. This is not CI
/// evidence or a statement that the claim was released or published remotely.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaimedCompletionProof {
    pub input: CompleteClaimedIssue,
    pub claim: ClaimRecord,
    pub claim_receipt: MutationReceipt,
    pub before: IssueRecord,
    pub after: IssueRecord,
    pub before_document: String,
    pub after_document: String,
    pub configuration: Config,
    pub config_document: String,
    pub admitted_at: Timestamp,
    pub completion: CompletionReport,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<ClaimedCompletionVerification>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirmation: Option<ClaimedCompletionConfirmation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ClaimedCompletionOutcome {
    pub repository: RepositoryId,
    pub completion: MutationReceipt,
    pub release: Option<MutationReceipt>,
    pub release_error: Option<PmError>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub release_publication: Option<ClaimOperationOutcome>,
    /// The separate requested release is durably recorded. This does not claim
    /// that no later generation has acquired the issue.
    pub release_recorded: bool,
}

impl Repository {
    pub fn complete_claimed_issue(
        &self,
        input: &CompleteClaimedIssue,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.complete_claimed_issue_with_faults(input, request, chrono::Utc::now, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn complete_claimed_issue_with_faults(
        &self,
        input: &CompleteClaimedIssue,
        request: &RequestId,
        clock: impl FnMut() -> Timestamp,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.complete_claimed_issue_inner(input, None, request, clock, fault)
    }

    /// Complete an issue only while the actor owns the current claim and the
    /// supplied authenticated checks/gates qualify the same candidate.
    pub fn complete_claimed_verified_issue(
        &self,
        input: &CompleteClaimedVerifiedIssue,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.complete_claimed_verified_issue_with_faults(input, request, chrono::Utc::now, |_| {
            Ok(())
        })
    }

    #[doc(hidden)]
    pub fn complete_claimed_verified_issue_with_faults(
        &self,
        input: &CompleteClaimedVerifiedIssue,
        request: &RequestId,
        clock: impl FnMut() -> Timestamp,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.complete_claimed_issue_inner(
            &input.claim,
            Some(&input.verification),
            request,
            clock,
            fault,
        )
    }

    fn complete_claimed_issue_inner(
        &self,
        input: &CompleteClaimedIssue,
        verification: Option<&CompleteVerifiedIssue>,
        request: &RequestId,
        clock: impl FnMut() -> Timestamp,
        mut fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let clock = RefCell::new(clock);
        let admission = RefCell::new(None::<(ClaimRecord, ClaimWorkContract, ClaimPolicy)>);
        let verified_guards = RefCell::new(None::<Vec<crate::execution::inputs::CapturedInputs>>);
        let store = self.store()?;
        let intent = match verification {
            Some(verification) => serde_json::json!({
                "input": input,
                "verification": verification,
            }),
            None => serde_json::json!({"input": input}),
        };
        if let Some(receipt) = store.replay_receipt(request, OPERATION, &intent)? {
            validate_receipt(&receipt)?;
            return Ok(receipt);
        }
        if let Some(verification) = verification {
            validate_claimed_verification_input(input, verification)?;
        }
        let verified_admission = verification
            .map(|verification| crate::completion::prepare_claimed(self, verification))
            .transpose()?;
        let confirmed = if self.config()?.sources.is_some() {
            let confirmed =
                crate::sources::confirm_sources_bound(self, input.expected_binding.as_ref())?;
            confirmed.verify_local()?;
            Some(confirmed)
        } else {
            if input.expected_binding.is_some() {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "reviewed shared completion cannot switch to local authority",
                ));
            }
            None
        };
        let receipt = store.transact_with_faults(
            request,
            OPERATION,
            &intent,
            |snapshot| {
                let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                let admitted_at = (clock.borrow_mut())();
                let (claim, current, claim_receipt, confirmation) =
                    match (&confirmed, &config.sources) {
                        (Some(confirmed), Some(_)) => {
                            confirmed.revalidate_local(snapshot)?;
                            if input
                                .expected_binding
                                .as_ref()
                                .is_some_and(|expected| expected != &confirmed.binding)
                            {
                                return Err(PmError::new(
                                    ErrorCode::StaleSource,
                                    "reviewed completion source binding changed",
                                ));
                            }
                            shared_admission(
                                self.root(),
                                snapshot,
                                &config,
                                confirmed,
                                input,
                                admitted_at,
                            )?
                        }
                        (None, None) => {
                            let claim = store::load_claims(snapshot, &config)?
                                .into_iter()
                                .find(|claim| claim.metadata.issue == input.issue)
                                .ok_or_else(|| {
                                    PmError::new(ErrorCode::ClaimLost, "claim no longer exists")
                                })?;
                            let identity = crate::sources::identity_from_snapshot(
                                self.root(),
                                snapshot,
                                &config,
                                SourceRole::Local,
                            )?;
                            let current = store::contract_from_snapshot(
                                self.root(),
                                snapshot,
                                &config,
                                &input.issue,
                                identity,
                            )?;
                            let receipt = origin(snapshot, &claim)?;
                            (claim, current, receipt, None)
                        }
                        _ => {
                            return Err(PmError::new(
                                ErrorCode::StaleSource,
                                "planning authority changed before claimed completion",
                            ));
                        }
                    };
                let policy = config.claims.clone().unwrap_or_default();
                check_admission(input, &claim, &current, &policy, admitted_at)?;
                store::eligible(self.root(), snapshot, &config, &input.issue, &input.actor)?;
                let before = crate::issues::resolve_issue(
                    self.root(),
                    snapshot,
                    &config,
                    input.issue.as_str(),
                )?;
                let before_document =
                    read_text(snapshot, &before.path, crate::documents::MAX_DOCUMENT_BYTES)?;
                let config_document = read_text(
                    snapshot,
                    Path::new("config.yml"),
                    crate::documents::MAX_DOCUMENT_BYTES,
                )?;
                let completion = if let Some(verified_admission) = verified_admission.as_ref() {
                    let (report, guards) = crate::completion::revalidate_claimed(
                        self.root(),
                        snapshot,
                        &config,
                        verified_admission,
                        &before,
                        admitted_at,
                    )?;
                    verified_guards.replace(Some(guards));
                    report
                } else {
                    crate::issues::completion(self.root(), snapshot, &config, &before)?
                };
                if !completion.allowed {
                    return Err(PmError::new(
                        ErrorCode::PolicyBlocked,
                        completion.reasons.join("; "),
                    ));
                }
                let mut prepared = if let Some(verified_admission) = verified_admission.as_ref() {
                    crate::issues::prepare_issue_mutation_admitted(
                        self.root(),
                        snapshot,
                        &config,
                        before.clone(),
                        Some(&input.expected_issue),
                        &IssueMutation::Complete { manual: None },
                        Some(verified_admission),
                    )?
                } else {
                    crate::issues::prepare_issue_mutation(
                        self.root(),
                        snapshot,
                        &config,
                        before.clone(),
                        Some(&input.expected_issue),
                        &IssueMutation::Complete { manual: None },
                    )?
                };
                let after: IssueRecord = serde_json::from_value(prepared.result)
                    .map_err(|error| invalid(error.to_string()))?;
                let after_document = prepared
                    .changes
                    .iter()
                    .find(|change| change.path == after.path)
                    .and_then(|change| change.content.as_ref())
                    .ok_or_else(|| invalid("claimed completion must publish its issue document"))?;
                let after_document = String::from_utf8(after_document.clone())
                    .map_err(|error| invalid(error.to_string()))?;
                let verification_admitted_at = after.metadata.updated_at;
                let verification_proof =
                    verified_admission
                        .as_ref()
                        .map(|admission| ClaimedCompletionVerification {
                            input: admission.input.clone(),
                            completion: completion.clone(),
                            prepared: admission.prepared.clone(),
                            attestations: admission.attestations.clone(),
                            gates: admission.gates.clone(),
                            evidence: admission.evidence.clone(),
                            admitted_at: verification_admitted_at,
                        });
                let proof = ClaimedCompletionProof {
                    input: input.clone(),
                    claim: claim.clone(),
                    claim_receipt,
                    before,
                    after,
                    before_document,
                    after_document,
                    configuration: config,
                    config_document,
                    admitted_at,
                    completion,
                    verification: verification_proof,
                    confirmation,
                };
                prepared.result =
                    serde_json::to_value(&proof).map_err(|error| invalid(error.to_string()))?;
                let candidate = candidate_receipt(self.identity(), request, &prepared, &intent)?;
                validate_receipt(&candidate)?;
                store::validate_operation_capacity(
                    snapshot,
                    &candidate,
                    &ClaimCatalogLimits::default(),
                )?;
                admission.replace(Some((claim, current, policy)));
                Ok(prepared)
            },
            |point| {
                fault(point)?;
                if point == FaultPoint::BeforeJournal {
                    if let Some(confirmed) = &confirmed {
                        confirmed.revalidate_files()?;
                    }
                    if let Some((claim, current, policy)) = admission.borrow().as_ref() {
                        check_admission(input, claim, current, policy, (clock.borrow_mut())())?;
                    }
                    if let Some(verified_admission) = verified_admission.as_ref() {
                        crate::completion::revalidate_claimed_at_journal(
                            verified_admission,
                            &verified_admission.input,
                            (clock.borrow_mut())(),
                        )?;
                        for guard in verified_guards.borrow().as_deref().unwrap_or_default() {
                            guard.verify()?;
                        }
                    }
                }
                Ok(())
            },
        )?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }

    pub fn complete_claimed_issue_and_release(
        &self,
        input: &CompleteClaimedIssue,
        completion_request: &RequestId,
        release_request: &RequestId,
        reason: &str,
    ) -> Result<ClaimedCompletionOutcome> {
        self.complete_claimed_issue_and_release_with_faults(
            input,
            completion_request,
            release_request,
            reason,
            |_| Ok(()),
        )
    }

    #[doc(hidden)]
    pub fn complete_claimed_issue_and_release_with_publication_faults(
        &self,
        input: &CompleteClaimedIssue,
        completion_request: &RequestId,
        release_request: &RequestId,
        reason: &str,
        fault: impl FnMut(PublicationFaultPoint) -> Result<()>,
    ) -> Result<ClaimedCompletionOutcome> {
        self.complete_and_release(
            input,
            completion_request,
            release_request,
            reason,
            |_| Ok(()),
            fault,
        )
    }

    #[doc(hidden)]
    pub fn complete_claimed_issue_and_release_with_faults(
        &self,
        input: &CompleteClaimedIssue,
        completion_request: &RequestId,
        release_request: &RequestId,
        reason: &str,
        before_release: impl FnOnce(&MutationReceipt) -> Result<()>,
    ) -> Result<ClaimedCompletionOutcome> {
        self.complete_and_release(
            input,
            completion_request,
            release_request,
            reason,
            before_release,
            |_| Ok(()),
        )
    }

    fn complete_and_release(
        &self,
        input: &CompleteClaimedIssue,
        completion_request: &RequestId,
        release_request: &RequestId,
        reason: &str,
        before_release: impl FnOnce(&MutationReceipt) -> Result<()>,
        publication_fault: impl FnMut(PublicationFaultPoint) -> Result<()>,
    ) -> Result<ClaimedCompletionOutcome> {
        transition::text(reason, "claim release reason")?;
        if completion_request == release_request {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "completion and release require distinct request IDs",
            ));
        }
        let completion = self.complete_claimed_issue(input, completion_request)?;
        let proof: ClaimedCompletionProof = serde_json::from_value(completion.result.clone())
            .map_err(|error| invalid(error.to_string()))?;
        let result = before_release(&completion).and_then(|()| {
            let request = ClaimRequest::Mutate {
                issue: input.issue.clone(),
                expected: input.expected_claim.clone(),
                mutation: ClaimMutation::Release {
                    actor: input.actor.clone(),
                    reason: reason.into(),
                },
            };
            if proof.confirmation.is_some() {
                if self.config()?.sources != proof.configuration.sources {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "shared release authority changed after completion",
                    ));
                }
                self.mutate_claim_bound(
                    &request,
                    release_request,
                    proof
                        .confirmation
                        .as_ref()
                        .map(|confirmation| &confirmation.binding),
                    publication_fault,
                )
                .map(|outcome| (outcome.receipt.clone(), Some(outcome)))
            } else {
                self.mutate_local_claim(&request, release_request)
                    .map(|receipt| (Some(receipt), None))
            }
        });
        let (release, release_publication, release_error) = match result {
            Ok((receipt, publication)) => (receipt, publication, None),
            Err(error) => (None, None, Some(error)),
        };
        let release_recorded = release.is_some()
            && release_publication.as_ref().is_none_or(|outcome| {
                outcome
                    .publication
                    .as_ref()
                    .is_some_and(|publication| publication.state == PublicationState::Confirmed)
            });
        Ok(ClaimedCompletionOutcome {
            repository: self.identity().clone(),
            completion,
            release_recorded,
            release,
            release_error,
            release_publication,
        })
    }
}

type Admission = (
    ClaimRecord,
    ClaimWorkContract,
    MutationReceipt,
    Option<ClaimedCompletionConfirmation>,
);

fn shared_admission(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    confirmed: &crate::sources::ConfirmedSources,
    input: &CompleteClaimedIssue,
    admitted_at: Timestamp,
) -> Result<Admission> {
    let accepted_root = Path::new("source-snapshot");
    let local_config = read_text(
        snapshot,
        Path::new("config.yml"),
        crate::documents::MAX_DOCUMENT_BYTES,
    )?;
    let (current, completion) = confirmed.accepted.with_snapshot(|accepted| {
        let accepted_config = crate::repository::config_from_snapshot(accepted_root, accepted)?;
        let accepted_document = read_text(
            accepted,
            Path::new("config.yml"),
            crate::documents::MAX_DOCUMENT_BYTES,
        )?;
        if &accepted_config != config || accepted_document != local_config {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "local completion policy differs from confirmed accepted policy",
            ));
        }
        let current = store::contract_from_snapshot(
            accepted_root,
            accepted,
            config,
            &input.issue,
            confirmed.accepted.identity().clone(),
        )?;
        // Validate the accepted graph, questions, identities and policy independently
        // of local proposals. The local write receives the same strict checks below.
        store::eligible(accepted_root, accepted, config, &input.issue, &input.actor)?;
        let issue =
            crate::issues::resolve_issue(accepted_root, accepted, config, input.issue.as_str())?;
        let completion = crate::issues::completion(accepted_root, accepted, config, &issue)?;
        if !completion.allowed {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "confirmed accepted requirements do not permit completion",
            )
            .details(
                serde_json::to_value(&completion).map_err(|error| invalid(error.to_string()))?,
            ));
        }
        Ok((current, completion))
    })?;
    let (claim, receipt) = confirmed.coordination.with_snapshot(|coordination| {
        let claim = store::load_claims(coordination, config)?
            .into_iter()
            .find(|claim| claim.metadata.issue == input.issue)
            .ok_or_else(|| {
                PmError::new(
                    ErrorCode::ClaimLost,
                    "confirmed coordination has no current claim",
                )
            })?;
        let receipt = origin(coordination, &claim)?;
        Ok((claim, receipt))
    })?;
    check_admission(
        input,
        &claim,
        &current,
        &config.claims.clone().unwrap_or_default(),
        admitted_at,
    )?;
    let local = store::contract_from_snapshot(
        root,
        snapshot,
        config,
        &input.issue,
        confirmed.accepted.identity().clone(),
    )?;
    if !transition::same_requirements(&local, &current) {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "local requirements differ from confirmed accepted requirements",
        ));
    }
    let confirmation = ClaimedCompletionConfirmation {
        binding: confirmed.binding.clone(),
        accepted: confirmed.accepted_observation.clone(),
        coordination: confirmed.coordination_observation.clone(),
        observed_at: confirmed.observed_at,
        contract: current.clone(),
        completion,
    };
    Ok((claim, current, receipt, Some(confirmation)))
}

fn check_admission(
    input: &CompleteClaimedIssue,
    claim: &ClaimRecord,
    current: &ClaimWorkContract,
    policy: &ClaimPolicy,
    now: Timestamp,
) -> Result<()> {
    input.contract.validate()?;
    current.validate()?;
    transition::check_expected(claim, &input.expected_claim)?;
    transition::text(&input.actor, "completion actor")?;
    if input.actor != claim.metadata.actor || claim.metadata.state != ClaimState::Active {
        return Err(PmError::new(
            ErrorCode::ClaimLost,
            "completion requires the current active claim actor",
        ));
    }
    if input.issue != current.issue
        || input.expected_issue != current.issue_source
        || !transition::same_requirements(&input.contract, current)
        || !transition::same_requirements(&claim.metadata.contract, current)
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "accepted requirements changed; explicitly revalidate ownership before completion",
        ));
    }
    if now < claim.metadata.updated_at
        || !transition::assess(
            claim,
            Some(current),
            policy,
            now,
            ClaimGuarantee::LocalSourceOnly,
        )
        .may_continue
    {
        return Err(PmError::new(
            ErrorCode::ClaimLost,
            "claimed completion requires an unexpired contract outside the clock-skew window",
        ));
    }
    Ok(())
}

fn read_text(snapshot: &Snapshot<'_>, path: &Path, limit: usize) -> Result<String> {
    String::from_utf8(
        snapshot
            .read_bounded(path, limit)?
            .ok_or_else(|| invalid("completion input disappeared").at(path))?,
    )
    .map_err(|error| invalid(error.to_string()).at(path))
}

fn origin(snapshot: &Snapshot<'_>, claim: &ClaimRecord) -> Result<MutationReceipt> {
    for path in snapshot.list_bounded(
        Path::new("operations"),
        ClaimCatalogLimits::default().max_operations,
    )? {
        let bytes = snapshot
            .read_bounded(&path, ClaimCatalogLimits::default().max_operation_bytes)?
            .ok_or_else(|| invalid("claim origin receipt disappeared"))?;
        let receipt: MutationReceipt =
            serde_yaml_ng::from_slice(&bytes).map_err(|error| invalid(error.to_string()))?;
        if receipt.request_id == claim.metadata.last_request {
            validation::validate_receipt(&receipt)?;
            let change: ClaimChange = serde_json::from_value(receipt.result.clone())
                .map_err(|error| invalid(error.to_string()))?;
            if change.after != *claim {
                return Err(invalid("claim origin does not prove the current claim"));
            }
            return Ok(receipt);
        }
    }
    Err(invalid("claim has no retained origin receipt"))
}

fn candidate_receipt(
    repository: &RepositoryId,
    request: &RequestId,
    prepared: &PreparedOperation,
    intent: &serde_json::Value,
) -> Result<MutationReceipt> {
    Ok(MutationReceipt {
        schema_version: SchemaVersion::CURRENT,
        repository: Some(repository.clone()),
        operation_id: "OP-00000000000000000000000000".parse()?,
        request_id: request.clone(),
        operation: OPERATION.into(),
        input_hash: canonical_hash(intent)?,
        result: prepared.result.clone(),
        changed: prepared
            .changes
            .iter()
            .map(|change| ChangedPath {
                path: change.path.clone(),
                before: change.expected.clone(),
                after: change.content.as_deref().map(ContentHash::of),
            })
            .collect(),
    })
}

fn parse_issue(record: &IssueRecord, text: &str, config: &Config) -> Result<MarkdownDocument> {
    if text.len() > crate::documents::MAX_DOCUMENT_BYTES {
        return Err(invalid(
            "claimed completion issue document exceeds its bound",
        ));
    }
    let document = MarkdownDocument::parse(&record.path, text)?;
    let metadata = crate::issues::parse_issue_metadata(&record.path, &document)?;
    metadata.validate(config)?;
    if record.path != Path::new(&format!("issues/{}/item.md", metadata.id))
        || crate::issues::record_from_document(metadata, &document, record.path.clone()) != *record
    {
        return Err(invalid(
            "claimed completion issue descriptor disagrees with its exact document",
        ));
    }
    Ok(document)
}

pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != OPERATION {
        return Ok(());
    }
    if serde_yaml_ng::to_string(receipt)
        .map_err(|error| invalid(error.to_string()))?
        .len()
        > 16 * 1024 * 1024
    {
        return Err(invalid("claimed completion receipt exceeds 16 MiB"));
    }
    let proof: ClaimedCompletionProof = serde_json::from_value(receipt.result.clone())
        .map_err(|error| invalid(error.to_string()))?;
    let config =
        crate::repository::parse_config(Path::new("config.yml"), proof.config_document.as_bytes())?;
    if config != proof.configuration
        || receipt.repository.as_ref() != Some(&config.repository)
        || receipt.input_hash != canonical_hash(&completion_intent(&proof))?
    {
        return Err(invalid(
            "claimed completion configuration, repository or original input differs",
        ));
    }
    validate_confirmation(&proof, &config)?;
    if let Some(verification) = &proof.verification {
        validate_claimed_verification(&proof, verification, &config)?;
    }
    crate::transactions::validate_receipt(&proof.claim_receipt)?;
    if !proof.claim_receipt.operation.starts_with("claim.")
        || proof.claim_receipt.operation != proof.claim.metadata.last_operation
        || proof.claim_receipt.request_id != proof.claim.metadata.last_request
    {
        return Err(invalid(
            "claimed completion origin must be its exact ownership operation",
        ));
    }
    validation::validate_receipt(&proof.claim_receipt)?;
    let origin: ClaimChange = serde_json::from_value(proof.claim_receipt.result.clone())
        .map_err(|error| invalid(error.to_string()))?;
    if origin.after != proof.claim || proof.claim.metadata.repository != config.repository {
        return Err(invalid(
            "claimed completion is not bound to its owning receipt",
        ));
    }
    check_admission(
        &proof.input,
        &proof.claim,
        &proof.input.contract,
        &config.claims.clone().unwrap_or_default(),
        proof.admitted_at,
    )?;
    let mut document = parse_issue(&proof.before, &proof.before_document, &config)?;
    parse_issue(&proof.after, &proof.after_document, &config)?;
    let before = &proof.before.metadata;
    let after = &proof.after.metadata;
    let completed = config
        .workflow
        .states
        .iter()
        .find(|state| state.category == WorkflowCategory::Completed)
        .ok_or_else(|| invalid("claimed completion workflow has no completed state"))?;
    let category = config.workflow.state(&before.status)?.category;
    if before.archived
        || matches!(
            category,
            WorkflowCategory::Completed | WorkflowCategory::Canceled
        )
        || before
            .assignee
            .as_deref()
            .is_some_and(|actor| actor != proof.input.actor)
        || proof.before.source != proof.input.expected_issue
        || before.id != proof.input.issue
        || proof.after.path != proof.before.path
        || proof.after.body != proof.before.body
    {
        return Err(invalid(
            "claimed completion subject, ownership or original workflow is inconsistent",
        ));
    }
    config.workflow.transition(&before.status, &completed.id)?;
    if proof.verification.is_none() {
        config.acceptance.ensure_supported_for_completion()?;
    }
    let basis = if proof.verification.is_some() {
        "authenticated_checks"
    } else if before.manual_acceptance.is_some() {
        "manual"
    } else if before
        .imported_completion
        .as_ref()
        .is_some_and(ImportedCompletion::is_active)
    {
        "imported"
    } else {
        "declared"
    };
    if !report_allows(&proof.completion, &proof.before, basis)
        || proof.confirmation.as_ref().is_some_and(|confirmation| {
            !report_allows(&confirmation.completion, &proof.before, basis)
        })
        || (config.acceptance.require_description && proof.before.body.trim().is_empty())
        || (config.acceptance.require_all_criteria
            && before.acceptance.iter().any(|criterion| !criterion.checked))
    {
        return Err(invalid(
            "claimed completion contradicts its required completion policy",
        ));
    }
    let mut expected = before.clone();
    expected.status = completed.id.clone();
    expected.revision = before.revision.next()?;
    expected.updated_at = after.updated_at;
    expected.completed_at = Some(after.updated_at);
    expected.canceled_at = None;
    if after != &expected
        || after.updated_at < before.updated_at
        || after.updated_at < proof.admitted_at
        || after.manual_acceptance.is_some()
    {
        return Err(invalid(
            "claimed completion changes fields outside its strict completion intent",
        ));
    }
    crate::issues::replace_metadata(&mut document, &expected)?;
    if document.render() != proof.after_document
        || receipt.changed
            != vec![ChangedPath {
                path: proof.before.path.clone(),
                before: Some(proof.before.source.content.clone()),
                after: Some(proof.after.source.content.clone()),
            }]
    {
        return Err(invalid(
            "claimed completion publication does not match its exact historical issue edit",
        ));
    }
    Ok(())
}

fn completion_intent(proof: &ClaimedCompletionProof) -> serde_json::Value {
    let mut intent = serde_json::json!({"input": proof.input});
    if let Some(verification) = &proof.verification {
        intent["verification"] = serde_json::json!(verification.input);
    }
    intent
}

fn validate_claimed_verification_input(
    claim: &CompleteClaimedIssue,
    verification: &CompleteVerifiedIssue,
) -> Result<()> {
    if verification.issue != claim.issue
        || verification.expected_issue != claim.expected_issue
        || verification.actor != claim.actor
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "claimed verification must qualify the claimed issue, actor and source token",
        ));
    }
    Ok(())
}

fn validate_claimed_verification(
    proof: &ClaimedCompletionProof,
    verification: &ClaimedCompletionVerification,
    config: &Config,
) -> Result<()> {
    validate_claimed_verification_input(&proof.input, &verification.input)
        .map_err(|error| invalid(error.message))?;
    if verification.admitted_at < proof.admitted_at
        || verification.admitted_at < proof.after.metadata.updated_at
    {
        return Err(invalid(
            "claimed verification admission time differs from ownership admission",
        ));
    }
    let retained = VerifiedIssueCompletion {
        input: verification.input.clone(),
        before: proof.before.clone(),
        after: proof.after.clone(),
        before_document: proof.before_document.clone(),
        after_document: proof.after_document.clone(),
        completion: verification.completion.clone(),
        prepared: verification.prepared.clone(),
        attestations: verification.attestations.clone(),
        gates: verification.gates.clone(),
        evidence: verification.evidence.clone(),
        admitted_at: verification.admitted_at,
    };
    let result = serde_json::to_value(&retained).map_err(|error| invalid(error.to_string()))?;
    let receipt = MutationReceipt {
        schema_version: SchemaVersion::CURRENT,
        repository: Some(config.repository.clone()),
        operation_id: OperationId::new(),
        request_id: RequestId::new(),
        operation: crate::completion::VERIFIED_OPERATION.into(),
        input_hash: canonical_hash(&result["input"])?,
        changed: vec![ChangedPath {
            path: proof.after.path.clone(),
            before: Some(proof.before.source.content.clone()),
            after: Some(proof.after.source.content.clone()),
        }],
        result,
    };
    crate::completion::validate_receipt(&receipt).map_err(|error| {
        invalid(format!(
            "claimed verification proof is invalid: {}",
            error.message
        ))
    })
}

fn report_allows(report: &CompletionReport, before: &IssueRecord, basis: &str) -> bool {
    report.allowed
        && report.basis == basis
        && report.reasons.is_empty()
        && report.issue == before.metadata.id
        && report.source == before.source
        && report
            .conditions
            .iter()
            .all(|condition| condition.state == ConditionState::Satisfied)
}

fn validate_confirmation(proof: &ClaimedCompletionProof, config: &Config) -> Result<()> {
    match (&config.sources, &proof.confirmation) {
        (None, None)
            if proof.input.contract.accepted_source.role == SourceRole::Local
                && proof.input.expected_binding.is_none() =>
        {
            Ok(())
        }
        (Some(shared), Some(confirmation)) => {
            if proof
                .input
                .expected_binding
                .as_ref()
                .is_some_and(|expected| expected != &confirmation.binding)
            {
                return Err(invalid(
                    "claimed completion differs from its reviewed source binding",
                ));
            }
            confirmation.contract.validate()?;
            for (observation, role, reference) in [
                (
                    &confirmation.accepted,
                    SourceRole::Accepted,
                    &shared.accepted_ref,
                ),
                (
                    &confirmation.coordination,
                    SourceRole::Coordination,
                    &shared.coordination_ref,
                ),
            ] {
                let remote = observation.remote_observation.as_ref().ok_or_else(|| {
                    invalid("claimed completion requires explicit remote observations")
                })?;
                if observation.identity.repository != config.repository
                    || observation.identity.role != role
                    || observation.identity.ref_name.as_ref() != Some(reference)
                    || observation.identity.commit.is_none()
                    || observation.identity.tree.is_none()
                    || observation.identity.index_content.is_some()
                    || observation.freshness != SourceFreshness::CurrentAtObservation
                    || observation.reason_codes != ["remote_observed"]
                    || remote.reference != *reference
                    || remote.remote != shared.remote
                    || remote.commit != observation.identity.commit
                    || remote.observed_at != observation.observed_at
                    || observation.observed_at != confirmation.observed_at
                    || confirmation.observed_at > proof.admitted_at
                    || (proof.admitted_at - confirmation.observed_at).num_seconds()
                        > SourceCaptureLimits::default().timeout_seconds as i64
                {
                    return Err(invalid(
                        "claimed completion observation disagrees with confirmed source authority",
                    ));
                }
            }
            if confirmation.contract.accepted_source != confirmation.accepted.identity
                || !transition::same_requirements(&confirmation.contract, &proof.input.contract)
                || !transition::same_requirements(
                    &confirmation.contract,
                    &proof.claim.metadata.contract,
                )
            {
                return Err(invalid(
                    "claimed completion accepted contract differs from its source observation",
                ));
            }
            Ok(())
        }
        _ => Err(invalid(
            "claimed completion authority requires matching confirmation proof",
        )),
    }
}
