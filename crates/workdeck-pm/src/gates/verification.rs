use super::*;
use crate::*;
use std::collections::BTreeSet;

fn blocked(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
#[derive(PartialEq, Eq)]
struct Capture {
    gate: GateRecord,
    evidence: Vec<EvidenceRecord>,
    attestations: Vec<(AttestationId, ContentHash)>,
    criteria: Vec<ResolvedCriterion>,
    config: ContentHash,
}
fn capture(repo: &Repository, input: &RedGreenGateRequest) -> Result<Capture> {
    repo.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repo.root(), snapshot)?;
        let gate = load_gate(snapshot, &config, &input.gate)?;
        if gate.source != input.expected_gate {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "gate differs from its inspected source pin",
            ));
        }
        if gate.definition.archived
            || crate::retirement::read_tombstone(
                repo.root(),
                snapshot,
                &config,
                &RetirementTarget::new(RetirementKind::Gate, input.gate.as_str())?,
            )?
            .is_some()
        {
            return Err(blocked("archived or retired gates cannot qualify"));
        }
        let criteria = gate
            .definition
            .requirements
            .iter()
            .map(|requirement| {
                resolve_criterion(
                    repo.root(),
                    snapshot,
                    &config,
                    &requirement.criterion.owner,
                    &requirement.criterion.id,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Capture {
            gate,
            criteria,
            evidence: crate::evidence::store::load_evidence(snapshot, &config)?,
            attestations: crate::attestations::load(snapshot, &config.repository)?
                .into_iter()
                .map(|r| (r.record.id, r.content))
                .collect(),
            config: ContentHash::of(
                &snapshot
                    .read_bounded(
                        std::path::Path::new("config.yml"),
                        crate::documents::MAX_DOCUMENT_BYTES,
                    )?
                    .ok_or_else(|| blocked("configuration is missing"))?,
            ),
        })
    })
}
impl Repository {
    pub fn verify_red_green_gate(
        &self,
        input: &RedGreenGateRequest,
    ) -> Result<RedGreenGateAssessment> {
        self.verify_red_green_gate_with_faults(input, || Ok(()))
    }
    #[doc(hidden)]
    pub fn verify_red_green_gate_with_faults(
        &self,
        input: &RedGreenGateRequest,
        before_revalidation: impl FnOnce() -> Result<()>,
    ) -> Result<RedGreenGateAssessment> {
        if input.evidence.is_empty() || input.evidence.len() > 256 {
            return Err(invalid(
                "gate verification requires one through 256 evidence selections",
            ));
        }
        let captured = capture(self, input)?;
        let required = captured
            .gate
            .definition
            .requirements
            .iter()
            .map(|r| r.id.as_str())
            .collect::<BTreeSet<_>>();
        let selected = input
            .evidence
            .iter()
            .map(|r| r.requirement.as_str())
            .collect::<BTreeSet<_>>();
        if selected.len() != input.evidence.len() || selected != required {
            return Err(blocked(
                "select exactly one evidence record for every gate requirement; duplicate or unknown requirements are forbidden",
            ));
        }
        let mut proofs = Vec::new();
        let mut candidate = None;
        let mut verified_evidence: std::collections::BTreeMap<
            EvidenceId,
            RedGreenEvidenceAssessment,
        > = Default::default();
        for requirement in &captured.gate.definition.requirements {
            let selection = input
                .evidence
                .iter()
                .find(|s| s.requirement == requirement.id)
                .expect("validated complete selection");
            let proof = if let Some(proof) = verified_evidence.get(&selection.evidence) {
                if proof.evidence_content != selection.expected_evidence {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "the same selected evidence has conflicting content pins",
                    ));
                }
                proof.clone()
            } else {
                let proof = self
                    .verify_red_green_evidence(&RedGreenEvidenceRequest {
                        evidence: selection.evidence.clone(),
                        expected_evidence: selection.expected_evidence.clone(),
                        authority: input.authority.clone(),
                    })
                    .map_err(|e| {
                        e.hint(format!(
                            "Gate requirement {} could not qualify.",
                            requirement.id
                        ))
                    })?;
                verified_evidence.insert(selection.evidence.clone(), proof.clone());
                proof
            };
            if proof.criterion.reference != requirement.criterion
                || proof.verification.pair.check != requirement.check
                || proof.verification.pair.green_producer != requirement.producer
            {
                return Err(blocked(format!(
                    "gate requirement {} differs from the authenticated criterion, check or producer",
                    requirement.id
                )));
            }
            if proof.criterion.declaration == CriterionDeclaration::Unchecked {
                return Err(blocked(format!(
                    "gate requirement {} remains explicitly unchecked",
                    requirement.id
                )));
            }
            let source = &proof.verification.pair.candidate;
            if candidate.as_ref().is_some_and(|prior| prior != source) {
                return Err(blocked("gate proofs must qualify the same exact candidate"));
            }
            candidate = Some(source.clone());
            proofs.push(VerifiedGateRequirement {
                requirement: requirement.id.clone(),
                proof,
            });
        }
        let candidate = candidate.expect("validated nonempty gate");
        let root = self
            .root()
            .parent()
            .ok_or_else(|| blocked("missing worktree root"))?;
        let committed = crate::sources::resolve_ci_gate(root, &candidate, &input.gate)?;
        if committed.source != captured.gate.source
            || committed.definition != captured.gate.definition
        {
            return Err(blocked(
                "current gate differs from the gate in the authenticated candidate",
            ));
        }
        before_revalidation()?;
        let current = capture(self, input)?;
        if current != captured {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "gate, criteria, evidence, attestations or configuration changed during verification",
            ));
        }
        let now = chrono::Utc::now();
        for verified in &proofs {
            for producer in [
                &verified.proof.verification.pair.red_producer,
                &verified.proof.verification.pair.green_producer,
            ] {
                if !input
                    .authority
                    .policy
                    .producers
                    .iter()
                    .any(|p| p.id == producer.id && p.not_before <= now && now < p.expires_at)
                {
                    return Err(blocked(
                        "producer authority expired during gate verification",
                    ));
                }
            }
            if let Some(review) = &verified.proof.verification.review
                && review.approval.expires_at <= now
            {
                return Err(blocked("contract review expired during gate verification"));
            }
        }
        if input.authority.review.as_ref().is_some_and(|r| {
            r.policy
                .required_reviewers
                .iter()
                .any(|p| now < p.not_before || now >= p.expires_at)
        }) {
            return Err(blocked(
                "reviewer authority expired during gate verification",
            ));
        }
        let active = crate::evidence::store::active_at(&current.evidence, now);
        for requirement in &current.gate.definition.requirements {
            let selection = input
                .evidence
                .iter()
                .find(|s| s.requirement == requirement.id)
                .expect("validated selection");
            let evidence = active
                .iter()
                .find(|e| e.reference.id == selection.evidence)
                .ok_or_else(|| blocked("selected evidence is no longer active"))?;
            let declaration = &evidence.reference.declaration;
            if declaration.observed_at > now
                || declaration.expires_at.is_some_and(|t| t <= now)
                || requirement.max_age_seconds.is_some_and(|age| {
                    now.signed_duration_since(declaration.observed_at)
                        .num_seconds()
                        > age as i64
                })
            {
                return Err(blocked(format!(
                    "gate requirement {} has future, expired or over-age evidence",
                    requirement.id
                )));
            }
        }
        Ok(RedGreenGateAssessment {
            basis: GateVerificationBasis::AuthenticatedRequirements,
            gate: current.gate,
            candidate,
            requirements: proofs,
            assessed_at: now,
        })
    }

    /// Verify every committed gate requirement against independently supplied
    /// producer authority and signed passed checks. This is the green-only
    /// counterpart to `verify_red_green_gate`; retained/configured pairs remain
    /// enforced by the shared evidence verifier.
    pub fn verify_verified_gate(
        &self,
        input: &VerifiedGateRequest,
    ) -> Result<VerifiedGateAssessment> {
        self.verify_verified_gate_with_faults(input, || Ok(()))
    }

    #[doc(hidden)]
    pub fn verify_verified_gate_with_faults(
        &self,
        input: &VerifiedGateRequest,
        before_revalidation: impl FnOnce() -> Result<()>,
    ) -> Result<VerifiedGateAssessment> {
        if input.evidence.is_empty() || input.evidence.len() > 256 {
            return Err(invalid(
                "gate verification requires one through 256 evidence selections",
            ));
        }
        let captured = capture_verified(self, input)?;
        let required = captured
            .gate
            .definition
            .requirements
            .iter()
            .map(|r| r.id.as_str())
            .collect::<BTreeSet<_>>();
        let selected = input
            .evidence
            .iter()
            .map(|r| r.requirement.as_str())
            .collect::<BTreeSet<_>>();
        if selected.len() != input.evidence.len() || selected != required {
            return Err(blocked(
                "select exactly one evidence record for every gate requirement; duplicate or unknown requirements are forbidden",
            ));
        }
        let mut proofs = Vec::new();
        let mut candidate = None;
        let mut verified_evidence: std::collections::BTreeMap<
            EvidenceId,
            VerifiedEvidenceAssessment,
        > = Default::default();
        for requirement in &captured.gate.definition.requirements {
            let selection = input
                .evidence
                .iter()
                .find(|s| s.requirement == requirement.id)
                .expect("validated complete selection");
            let proof = if let Some(proof) = verified_evidence.get(&selection.evidence) {
                if proof.evidence_content != selection.expected_evidence {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "the same selected evidence has conflicting content pins",
                    ));
                }
                proof.clone()
            } else {
                let proof = self
                    .verify_authenticated_check_evidence(&VerifiedEvidenceRequest {
                        evidence: selection.evidence.clone(),
                        expected_evidence: selection.expected_evidence.clone(),
                        authority: input.authority.clone(),
                    })
                    .map_err(|e| {
                        e.hint(format!(
                            "Gate requirement {} could not qualify.",
                            requirement.id
                        ))
                    })?;
                verified_evidence.insert(selection.evidence.clone(), proof.clone());
                proof
            };
            if proof.criterion.reference != requirement.criterion
                || proof
                    .verification
                    .report
                    .publication
                    .result
                    .result
                    .checks
                    .iter()
                    .all(|c| c.check != requirement.check || c.state != RunState::Passed)
                || proof.verification.producer != requirement.producer
            {
                return Err(blocked(format!(
                    "gate requirement {} differs from the authenticated criterion, check or producer",
                    requirement.id
                )));
            }
            if proof.criterion.declaration == CriterionDeclaration::Unchecked {
                return Err(blocked(format!(
                    "gate requirement {} remains explicitly unchecked",
                    requirement.id
                )));
            }
            let source = &proof.verification.source;
            if candidate.as_ref().is_some_and(|prior| prior != source) {
                return Err(blocked("gate proofs must qualify the same exact candidate"));
            }
            candidate = Some(source.clone());
            proofs.push(GreenGateRequirement {
                requirement: requirement.id.clone(),
                proof,
            });
        }
        let candidate = candidate.expect("validated nonempty gate");
        let root = self
            .root()
            .parent()
            .ok_or_else(|| blocked("missing worktree root"))?;
        let committed = crate::sources::resolve_ci_gate(root, &candidate, &input.gate)?;
        if committed.source != captured.gate.source
            || committed.definition != captured.gate.definition
        {
            return Err(blocked(
                "current gate differs from the gate in the authenticated candidate",
            ));
        }
        before_revalidation()?;
        let current = capture_verified(self, input)?;
        if current != captured {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "gate, criteria, evidence, attestations or configuration changed during verification",
            ));
        }
        let now = chrono::Utc::now();
        let active = crate::evidence::store::active_at(&current.evidence, now);
        for requirement in &current.gate.definition.requirements {
            let selection = input
                .evidence
                .iter()
                .find(|s| s.requirement == requirement.id)
                .expect("validated selection");
            let evidence = active
                .iter()
                .find(|e| e.reference.id == selection.evidence)
                .ok_or_else(|| blocked("selected evidence is no longer active"))?;
            let declaration = &evidence.reference.declaration;
            if declaration.observed_at > now
                || declaration.expires_at.is_some_and(|time| time <= now)
                || requirement.max_age_seconds.is_some_and(|age| {
                    now.signed_duration_since(declaration.observed_at)
                        .num_seconds()
                        > age as i64
                })
            {
                return Err(blocked(format!(
                    "gate requirement {} has future, expired or over-age evidence",
                    requirement.id
                )));
            }
        }
        Ok(VerifiedGateAssessment {
            basis: GateVerificationBasis::AuthenticatedGreenRequirements,
            gate: current.gate,
            candidate,
            requirements: proofs,
            assessed_at: now,
        })
    }
}

fn capture_verified(repo: &Repository, input: &VerifiedGateRequest) -> Result<Capture> {
    repo.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repo.root(), snapshot)?;
        let gate = load_gate(snapshot, &config, &input.gate)?;
        if gate.source != input.expected_gate {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "gate differs from its inspected source pin",
            ));
        }
        if gate.definition.archived
            || crate::retirement::read_tombstone(
                repo.root(),
                snapshot,
                &config,
                &RetirementTarget::new(RetirementKind::Gate, input.gate.as_str())?,
            )?
            .is_some()
        {
            return Err(blocked("archived or retired gates cannot qualify"));
        }
        let criteria = gate
            .definition
            .requirements
            .iter()
            .map(|requirement| {
                resolve_criterion(
                    repo.root(),
                    snapshot,
                    &config,
                    &requirement.criterion.owner,
                    &requirement.criterion.id,
                )
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Capture {
            gate,
            criteria,
            evidence: crate::evidence::store::load_evidence(snapshot, &config)?,
            attestations: crate::attestations::load(snapshot, &config.repository)?
                .into_iter()
                .map(|r| (r.record.id, r.content))
                .collect(),
            config: ContentHash::of(
                &snapshot
                    .read_bounded(
                        std::path::Path::new("config.yml"),
                        crate::documents::MAX_DOCUMENT_BYTES,
                    )?
                    .ok_or_else(|| blocked("configuration is missing"))?,
            ),
        })
    })
}
