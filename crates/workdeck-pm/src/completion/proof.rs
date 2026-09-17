use super::*;
use crate::transactions::{ChangedPath, canonical_hash};

pub(crate) fn match_check(
    prepared: &CiPreparedCheck,
    selection: &CompletionCheckSelection,
    report: &AuthenticatedCheckReport,
) -> Result<()> {
    let current = prepared
        .plan
        .checks
        .iter()
        .find(|c| c.id == selection.check)
        .ok_or_else(|| blocked("proof check is not required by the completion plan"))?;
    let old = &report.report.publication.intent.intent.input.plan;
    if report.source != prepared.binding.source
        || old.checks.iter().find(|c| c.id == selection.check) != Some(current)
        || old.invocations.iter().find(|i| i.id == current.invocation)
            != prepared
                .plan
                .invocations
                .iter()
                .find(|i| i.id == current.invocation)
    {
        return Err(stale(
            "check result inputs, parameters, environment, tools or definition differ from current completion selection",
        ));
    }
    let results = &report.report.publication.result.result;
    let check = results
        .checks
        .iter()
        .find(|c| c.check.id == selection.check)
        .ok_or_else(|| blocked("signed report omits selected completion check"))?;
    if check.state != RunState::Passed
        || results
            .invocations
            .get(check.invocation)
            .is_none_or(|i| i.state != RunState::Passed || !i.inputs_unchanged)
    {
        return Err(blocked(
            "authenticated completion check or invocation did not pass",
        ));
    }
    Ok(())
}
pub(super) fn authenticate(
    record: &ImportedCheckReportRecord,
    authority: &CompletionAuthority,
    now: Timestamp,
) -> Result<AuthenticatedCheckReport> {
    let mut input = record.record.input.clone();
    input.policy = authority.policy.clone();
    input.expected_policy = authority.expected_policy.clone();
    if input.expected_commit != authority.candidate {
        return Err(blocked("completion proof candidate differs from authority"));
    }
    if let Some(current) = &authority.red_green
        && (current.candidate != authority.candidate
            || current.policy != authority.policy
            || current.expected_policy != authority.expected_policy)
    {
        return Err(blocked(
            "completion red/green authority differs from producer authority",
        ));
    }
    if let Some(pair) = input.red_green.as_mut() {
        let authority = authority
            .red_green
            .as_ref()
            .ok_or_else(|| blocked("retained pair requires current red/green authority"))?;
        if pair.baseline != authority.baseline {
            return Err(blocked("completion proof baseline differs from authority"));
        }
        match (&mut pair.review, &authority.review) {
            (Some(review), Some(current)) => {
                review.accepted = current.baseline.clone();
                review.policy = current.policy.clone();
                review.expected_policy = current.expected_policy.clone();
            }
            (None, None) => (),
            _ => {
                return Err(blocked(
                    "completion review authority cannot be omitted or synthesized",
                ));
            }
        }
    }
    crate::attestations::records::authenticate(&input, now)
}
pub(super) fn authenticate_all(
    prepared: &CiPreparedCheck,
    input: &CompleteVerifiedIssue,
    records: &[ImportedCheckReportRecord],
    require_pairs: bool,
    now: Timestamp,
) -> Result<()> {
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
    if selected.is_empty() || selected.len() != input.checks.len() || selected != required {
        return Err(blocked(
            "completion required-check denominator differs from supplied proof",
        ));
    }
    for record in records {
        if crate::attestations::records::parse(
            &record.path,
            record.document.as_bytes(),
            &prepared.plan.repository,
        )? != *record
        {
            return Err(blocked("retained completion attestation is inconsistent"));
        }
        authenticate(record, &input.authority, now)?;
    }
    for selection in &input.checks {
        let record = records
            .iter()
            .find(|r| {
                r.record.id == selection.attestation && r.content == selection.expected_attestation
            })
            .ok_or_else(|| blocked("selected original completion attestation is missing"))?;
        let definition = prepared
            .plan
            .definitions
            .checks
            .iter()
            .find(|c| c.definition.id == selection.check)
            .ok_or_else(|| blocked("completion check definition missing"))?;
        let pair = record.record.input.red_green.as_ref();
        if (require_pairs || definition.definition.red_green.is_some() || pair.is_some())
            && pair.is_none_or(|p| p.check != selection.check)
        {
            return Err(blocked(
                "completion selection lacks its required original check pair",
            ));
        }
        let report = authenticate(record, &input.authority, now)?;
        match_check(prepared, selection, &report)?;
    }
    Ok(())
}
pub(super) fn gate_windows(
    gates: &[CompletionGateAssessment],
    evidence: &[EvidenceRecord],
    now: Timestamp,
) -> Result<()> {
    for gate in gates {
        let (definition, requirements): (&GateDefinition, Vec<(&str, &EvidenceId, &ContentHash)>) =
            match gate {
                CompletionGateAssessment::RedGreen(gate) => (
                    &gate.gate.definition,
                    gate.requirements
                        .iter()
                        .map(|p| {
                            (
                                p.requirement.as_str(),
                                &p.proof.evidence,
                                &p.proof.evidence_content,
                            )
                        })
                        .collect(),
                ),
                CompletionGateAssessment::GreenOnly(gate) => (
                    &gate.gate.definition,
                    gate.requirements
                        .iter()
                        .map(|p| {
                            (
                                p.requirement.as_str(),
                                &p.proof.evidence,
                                &p.proof.evidence_content,
                            )
                        })
                        .collect(),
                ),
            };
        for requirement in definition.requirements.iter() {
            let (_, evidence_id, evidence_content) = requirements
                .iter()
                .find(|(id, _, _)| *id == requirement.id.as_str())
                .ok_or_else(|| blocked("gate requirement proof missing"))?;
            let evidence = evidence
                .iter()
                .find(|e| {
                    e.reference.id.as_str() == (*evidence_id).as_str()
                        && e.content.as_str() == (*evidence_content).as_str()
                })
                .ok_or_else(|| blocked("original gate evidence missing"))?;
            let d = &evidence.reference.declaration;
            if d.observed_at > now
                || d.expires_at.is_some_and(|t| t <= now)
                || requirement.max_age_seconds.is_some_and(|age| {
                    now.signed_duration_since(d.observed_at).num_seconds() > age as i64
                })
            {
                return Err(blocked(
                    "gate evidence expired before completion publication",
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn revalidate(
    snapshot: &Snapshot<'_>,
    config: &Config,
    admission: &Admission,
    now: Timestamp,
) -> Result<()> {
    let records = crate::attestations::load(snapshot, &config.repository)?;
    for record in &admission.attestations {
        if !records.contains(record) {
            return Err(stale("completion attestation changed after preflight"));
        }
    }
    authenticate_all(
        &admission.prepared,
        &admission.input,
        &admission.attestations,
        admission.require_pairs,
        now,
    )?;
    let records = crate::evidence::store::load_evidence(snapshot, config)?;
    let active = crate::evidence::store::active_at(&records, now);
    for evidence in &admission.evidence {
        if !active.contains(&evidence) {
            return Err(stale("completion gate evidence changed or was superseded"));
        }
    }
    for gate in &admission.gates {
        let (record, requirements): (&GateRecord, Vec<(&str, &ResolvedCriterion)>) = match gate {
            CompletionGateAssessment::RedGreen(gate) => (
                &gate.gate,
                gate.requirements
                    .iter()
                    .map(|requirement| {
                        (
                            requirement.requirement.as_str(),
                            &requirement.proof.criterion,
                        )
                    })
                    .collect(),
            ),
            CompletionGateAssessment::GreenOnly(gate) => (
                &gate.gate,
                gate.requirements
                    .iter()
                    .map(|requirement| {
                        (
                            requirement.requirement.as_str(),
                            &requirement.proof.criterion,
                        )
                    })
                    .collect(),
            ),
        };
        let current = crate::gates::load_gate(snapshot, config, &record.definition.id)?;
        if current != *record {
            return Err(stale("completion gate changed after preflight"));
        }
        if crate::retirement::read_tombstone(
            Path::new(""),
            snapshot,
            config,
            &RetirementTarget::new(RetirementKind::Gate, current.definition.id.as_str())?,
        )?
        .is_some()
        {
            return Err(blocked("completion gate was retired"));
        }
        for (_, expected) in requirements {
            let current = crate::gates::resolve_criterion(
                Path::new(""),
                snapshot,
                config,
                &expected.reference.owner,
                &expected.reference.id,
            )?;
            if current != *expected {
                return Err(stale("gate criterion changed after preflight"));
            }
        }
    }
    gate_windows(&admission.gates, &admission.evidence, now)
}
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if !is_operation(&receipt.operation) {
        return Ok(());
    }
    crate::transactions::validate_receipt(receipt)?;
    let mut normalized = receipt.result.clone();
    if receipt.operation == OPERATION {
        let legacy: CompleteRedGreenIssue = serde_json::from_value(normalized["input"].clone())
            .map_err(|e| blocked(e.to_string()))?;
        normalized["input"] = serde_json::json!(CompleteVerifiedIssue::from(&legacy));
    }
    let proof: VerifiedIssueCompletion =
        serde_json::from_value(normalized).map_err(|e| blocked(e.to_string()))?;
    crate::gates::validation::text(&proof.input.actor, "completion actor", 256)?;
    if proof.input.checks.is_empty()
        || proof.input.checks.len() > 256
        || proof.input.gates.len() > 256
        || proof.attestations.len() > 256
    {
        return Err(blocked("completion receipt exceeds supported proof bounds"));
    }
    let config = &proof.prepared.plan.configuration;
    proof.prepared.binding.validate(&proof.prepared.plan)?;
    authenticate_all(
        &proof.prepared,
        &proof.input,
        &proof.attestations,
        receipt.operation == OPERATION,
        proof.admitted_at,
    )?;
    gate_windows(&proof.gates, &proof.evidence, proof.admitted_at)?;
    super::gate_proof::validate(&proof)?;
    for (record, document) in [
        (&proof.before, &proof.before_document),
        (&proof.after, &proof.after_document),
    ] {
        let doc = crate::documents::MarkdownDocument::parse(&record.path, document)?;
        let metadata = crate::issues::parse_issue_metadata(&record.path, &doc)?;
        metadata.validate(config)?;
        if crate::issues::record_from_document(metadata, &doc, record.path.clone()) != *record {
            return Err(blocked(
                "completion issue proof differs from original document bytes",
            ));
        }
    }
    if receipt.repository.as_ref() != Some(&config.repository)
        || receipt.input_hash != canonical_hash(&receipt.result["input"])?
        || proof.input.issue != proof.before.metadata.id
        || proof.input.expected_issue != proof.before.source
        || proof.before.metadata.id != proof.after.metadata.id
        || proof.before.path != proof.after.path
        || proof.before.body != proof.after.body
        || proof
            .prepared
            .plan
            .issue
            .as_ref()
            .is_none_or(|i| i.id != proof.input.issue || i.source != proof.input.expected_issue)
        || !proof.completion.allowed
        || proof.completion.basis != "authenticated_checks"
        || !proof.completion.reasons.is_empty()
        || proof.completion.issue != proof.input.issue
        || proof.completion.source != proof.before.source
        || proof
            .completion
            .conditions
            .iter()
            .any(|c| c.state != ConditionState::Satisfied)
        || config
            .workflow
            .state(&proof.after.metadata.status)?
            .category
            != WorkflowCategory::Completed
        || (config.acceptance.require_description && proof.before.body.trim().is_empty())
        || (config.acceptance.require_all_criteria
            && proof.before.metadata.acceptance.iter().any(|c| !c.checked))
    {
        return Err(blocked(
            "completion receipt subject, policy, state or decision is inconsistent",
        ));
    }
    let mut expected = proof.before.metadata.clone();
    expected.status = proof.after.metadata.status.clone();
    expected.revision = expected.revision.next()?;
    expected.updated_at = proof.after.metadata.updated_at;
    expected.completed_at = Some(expected.updated_at);
    expected.canceled_at = None;
    if expected != proof.after.metadata
        || expected.updated_at < proof.before.metadata.updated_at
        || expected.updated_at > proof.admitted_at
    {
        return Err(blocked(
            "completion receipt changes fields beyond the authorized state transition",
        ));
    }
    let gates = proof.before.metadata.gates.iter().collect::<BTreeSet<_>>();
    let selected = proof
        .input
        .gates
        .iter()
        .map(|g| &g.gate)
        .collect::<BTreeSet<_>>();
    let verified = proof
        .gates
        .iter()
        .map(|g| match g {
            CompletionGateAssessment::RedGreen(g) => &g.gate.definition.id,
            CompletionGateAssessment::GreenOnly(g) => &g.gate.definition.id,
        })
        .collect::<BTreeSet<_>>();
    if gates != selected
        || selected != verified
        || selected.len() != proof.input.gates.len()
        || verified.len() != proof.gates.len()
    {
        return Err(blocked(
            "completion receipt omits or duplicates required gates",
        ));
    }
    config
        .workflow
        .transition(&proof.before.metadata.status, &proof.after.metadata.status)?;
    if receipt.changed
        != vec![ChangedPath {
            path: proof.after.path,
            before: Some(proof.before.source.content),
            after: Some(proof.after.source.content),
        }]
    {
        return Err(blocked(
            "completion receipt does not publish the exact issue transition",
        ));
    }
    Ok(())
}
