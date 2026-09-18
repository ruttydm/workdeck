use super::*;

pub(super) fn validate(proof: &VerifiedIssueCompletion) -> Result<()> {
    for gate in &proof.gates {
        match gate {
            CompletionGateAssessment::RedGreen(gate) => validate_red_green(proof, gate)?,
            CompletionGateAssessment::GreenOnly(gate) => validate_green_only(proof, gate)?,
        }
    }
    Ok(())
}

fn validate_red_green(
    proof: &VerifiedIssueCompletion,
    gate: &RedGreenGateAssessment,
) -> Result<()> {
    let repository = &proof.prepared.plan.repository;
    if crate::gates::parse(&gate.gate.path, gate.gate.document.as_bytes(), repository)? != gate.gate
        || gate.gate.definition.archived
        || gate.gate.retirement.is_some()
        || gate.basis != GateVerificationBasis::AuthenticatedRequirements
        || gate.candidate != proof.prepared.binding.source
    {
        return Err(blocked(
            "retained completion gate definition or candidate differs",
        ));
    }
    validate_selection(
        &gate.gate,
        &proof.input.gates,
        gate.requirements
            .iter()
            .map(|r| r.requirement.as_str())
            .collect(),
        false,
    )?;
    let selected = proof
        .input
        .gates
        .iter()
        .find(|g| g.gate == gate.gate.definition.id)
        .expect("validated gate selection");
    for requirement in &gate.gate.definition.requirements {
        let selected = selected
            .evidence
            .iter()
            .find(|e| e.requirement == requirement.id)
            .expect("complete selection");
        let verified = &gate
            .requirements
            .iter()
            .find(|r| r.requirement == requirement.id)
            .expect("complete verification")
            .proof;
        let evidence = proof
            .evidence
            .iter()
            .find(|e| {
                e.reference.id == selected.evidence && e.content == selected.expected_evidence
            })
            .ok_or_else(|| blocked("retained gate evidence is missing"))?;
        if crate::evidence::store::parse(&evidence.path, evidence.document.as_bytes(), repository)?
            != *evidence
        {
            return Err(blocked(
                "retained gate evidence differs from original bytes",
            ));
        }
        let record = proof
            .attestations
            .iter()
            .find(|r| {
                r.record.id == verified.attestation && r.content == verified.attestation_content
            })
            .ok_or_else(|| blocked("retained gate attestation is missing"))?;
        let report = super::proof::authenticate(record, &proof.input.authority, proof.admitted_at)?;
        let declaration = &evidence.reference.declaration;
        let check = report
            .report
            .publication
            .result
            .result
            .checks
            .iter()
            .find(|c| c.check == requirement.check)
            .ok_or_else(|| blocked("retained gate check is missing from signed report"))?;
        if record
            .record
            .input
            .red_green
            .as_ref()
            .is_none_or(|p| p.check != requirement.check.id)
            || verified.basis != EvidenceVerificationBasis::AuthenticatedCheckLink
            || declaration.criterion != requirement.criterion
            || declaration.check != requirement.check
            || declaration.producer != requirement.producer
            || report.producer != requirement.producer
            || declaration.subject
                != (ExactSubject {
                    repository: repository.clone(),
                    kind: ExactSubjectKind::Source,
                    content: proof.prepared.binding.source.content.clone(),
                })
            || declaration.result.content != report.report.fingerprint
            || declaration.result.id != report.report.publication.intent.intent.id.as_str()
            || declaration.observed_at != report.report.observed_at
            || check.state != RunState::Passed
            || !declaration.links.contains(&EvidenceLink::Attestation {
                id: verified.attestation.clone(),
                content: verified.attestation_content.clone(),
            })
            || verified.evidence != selected.evidence
            || verified.evidence_content != selected.expected_evidence
            || verified.criterion.reference != requirement.criterion
            || verified.committed_criterion.reference != requirement.criterion
            || verified.criterion.retired
            || verified.committed_criterion.retired
            || verified.criterion.declaration == CriterionDeclaration::Unchecked
            || verified.verification.pair.candidate != proof.prepared.binding.source
            || verified.verification.pair.check != requirement.check
            || verified.verification.pair.green_producer != report.producer
        {
            return Err(blocked(
                "retained gate criterion, evidence, producer or signed result differs",
            ));
        }
        validate_red_green_criterion_hashes(repository, verified)?;
    }
    Ok(())
}

fn validate_green_only(
    proof: &VerifiedIssueCompletion,
    gate: &VerifiedGateAssessment,
) -> Result<()> {
    let repository = &proof.prepared.plan.repository;
    if crate::gates::parse(&gate.gate.path, gate.gate.document.as_bytes(), repository)? != gate.gate
        || gate.gate.definition.archived
        || gate.gate.retirement.is_some()
        || gate.basis != GateVerificationBasis::AuthenticatedGreenRequirements
        || gate.candidate != proof.prepared.binding.source
    {
        return Err(blocked(
            "retained green-only gate definition or candidate differs",
        ));
    }
    validate_selection(
        &gate.gate,
        &proof.input.gates,
        gate.requirements
            .iter()
            .map(|r| r.requirement.as_str())
            .collect(),
        true,
    )?;
    let selected = proof
        .input
        .gates
        .iter()
        .find(|g| g.gate == gate.gate.definition.id)
        .expect("validated gate selection");
    for requirement in &gate.gate.definition.requirements {
        let selected = selected
            .evidence
            .iter()
            .find(|e| e.requirement == requirement.id)
            .expect("complete selection");
        let verified = &gate
            .requirements
            .iter()
            .find(|r| r.requirement == requirement.id)
            .expect("complete verification")
            .proof;
        let evidence = proof
            .evidence
            .iter()
            .find(|e| {
                e.reference.id == selected.evidence && e.content == selected.expected_evidence
            })
            .ok_or_else(|| blocked("retained gate evidence is missing"))?;
        if crate::evidence::store::parse(&evidence.path, evidence.document.as_bytes(), repository)?
            != *evidence
        {
            return Err(blocked(
                "retained green-only gate evidence differs from original bytes",
            ));
        }
        let record = proof
            .attestations
            .iter()
            .find(|r| {
                r.record.id == verified.attestation && r.content == verified.attestation_content
            })
            .ok_or_else(|| blocked("retained gate attestation is missing"))?;
        let report = super::proof::authenticate(record, &proof.input.authority, proof.admitted_at)?;
        let declaration = &evidence.reference.declaration;
        let check = report
            .report
            .publication
            .result
            .result
            .checks
            .iter()
            .find(|c| c.check == requirement.check)
            .ok_or_else(|| blocked("retained gate check is missing from signed report"))?;
        let invocation = report
            .report
            .publication
            .result
            .result
            .invocations
            .get(check.invocation)
            .ok_or_else(|| blocked("retained gate invocation is missing from signed report"))?;
        if verified.basis != EvidenceVerificationBasis::AuthenticatedCheck
            || declaration.criterion != requirement.criterion
            || declaration.check != requirement.check
            || declaration.producer != requirement.producer
            || report.producer != requirement.producer
            || declaration.subject
                != (ExactSubject {
                    repository: repository.clone(),
                    kind: ExactSubjectKind::Source,
                    content: proof.prepared.binding.source.content.clone(),
                })
            || declaration.result.content != report.report.fingerprint
            || declaration.result.id != report.report.publication.intent.intent.id.as_str()
            || declaration.observed_at != report.report.observed_at
            || check.state != RunState::Passed
            || invocation.state != RunState::Passed
            || !invocation.inputs_unchanged
            || !declaration.links.contains(&EvidenceLink::Attestation {
                id: verified.attestation.clone(),
                content: verified.attestation_content.clone(),
            })
            || verified.evidence != selected.evidence
            || verified.evidence_content != selected.expected_evidence
            || verified.criterion.reference != requirement.criterion
            || verified.committed_criterion.reference != requirement.criterion
            || verified.criterion.retired
            || verified.committed_criterion.retired
            || verified.criterion.declaration == CriterionDeclaration::Unchecked
            || verified.verification.source != proof.prepared.binding.source
            || verified.verification.producer != report.producer
        {
            return Err(blocked(
                "retained green-only gate criterion, evidence, producer or signed result differs",
            ));
        }
        validate_criterion_hashes(repository, verified)?;
    }
    Ok(())
}

fn validate_selection(
    gate: &GateRecord,
    selected: &[CompletionGateSelection],
    verified: BTreeSet<&str>,
    green_only: bool,
) -> Result<()> {
    let selected = selected
        .iter()
        .find(|selection| selection.gate == gate.definition.id)
        .ok_or_else(|| blocked("retained gate is not selected"))?;
    let required = gate
        .definition
        .requirements
        .iter()
        .map(|r| r.id.as_str())
        .collect::<BTreeSet<_>>();
    let evidence = selected
        .evidence
        .iter()
        .map(|e| e.requirement.as_str())
        .collect::<BTreeSet<_>>();
    if selected.expected_gate != gate.source
        || required != evidence
        || evidence != verified
        || selected.evidence.len() != evidence.len()
        || (green_only && verified.is_empty())
    {
        return Err(blocked(
            "retained completion gate omits, duplicates or substitutes requirements",
        ));
    }
    Ok(())
}

fn validate_criterion_hashes(
    repository: &RepositoryId,
    proof: &VerifiedEvidenceAssessment,
) -> Result<()> {
    for criterion in [&proof.criterion, &proof.committed_criterion] {
        if criterion_definition_hash(
            repository,
            &criterion.reference.owner,
            &criterion.reference.id,
            &criterion.description,
        )? != criterion.reference.definition
        {
            return Err(blocked(
                "retained gate criterion hash differs from its definition",
            ));
        }
    }
    Ok(())
}

fn validate_red_green_criterion_hashes(
    repository: &RepositoryId,
    proof: &RedGreenEvidenceAssessment,
) -> Result<()> {
    for criterion in [&proof.criterion, &proof.committed_criterion] {
        if criterion_definition_hash(
            repository,
            &criterion.reference.owner,
            &criterion.reference.id,
            &criterion.description,
        )? != criterion.reference.definition
        {
            return Err(blocked(
                "retained gate criterion hash differs from its definition",
            ));
        }
    }
    Ok(())
}
