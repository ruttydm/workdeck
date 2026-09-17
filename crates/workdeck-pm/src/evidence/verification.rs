use super::*;
use crate::*;
fn blocked(message: &str) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
#[derive(PartialEq, Eq)]
struct Capture {
    evidence: EvidenceRecord,
    attestation: ImportedCheckReportRecord,
    criterion: ResolvedCriterion,
}
fn capture(repo: &Repository, request: &RedGreenEvidenceRequest) -> Result<Capture> {
    repo.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repo.root(), snapshot)?;
        let now = chrono::Utc::now();
        let records = store::load_evidence(snapshot, &config)?;
        let evidence = store::active_at(&records, now)
            .into_iter()
            .find(|record| record.reference.id == request.evidence)
            .ok_or_else(|| blocked("evidence is missing, not yet recorded or superseded"))?
            .clone();
        if evidence.content != request.expected_evidence {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "evidence differs from its independently selected content pin",
            ));
        }
        let declaration = &evidence.reference.declaration;
        if declaration.expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(blocked("evidence declaration has expired"));
        }
        let (id, content) = declaration
            .links
            .iter()
            .find_map(|link| match link {
                EvidenceLink::Attestation { id, content } => Some((id, content)),
                _ => None,
            })
            .ok_or_else(|| blocked("evidence has no pinned attestation link"))?;
        let attestation = crate::attestations::load(snapshot, &config.repository)?
            .into_iter()
            .find(|record| &record.record.id == id)
            .ok_or_else(|| blocked("linked attestation is missing"))?;
        if &attestation.content != content {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "linked attestation content differs from declaration",
            ));
        }
        let criterion = crate::gates::resolve_criterion(
            repo.root(),
            snapshot,
            &config,
            &declaration.criterion.owner,
            &declaration.criterion.id,
        )?;
        if criterion.retired || criterion.reference != declaration.criterion {
            return Err(blocked(
                "current criterion is retired or differs from the evidence definition",
            ));
        }
        Ok(Capture {
            evidence,
            attestation,
            criterion,
        })
    })
}
impl Repository {
    pub fn verify_red_green_evidence(
        &self,
        request: &RedGreenEvidenceRequest,
    ) -> Result<RedGreenEvidenceAssessment> {
        self.verify_red_green_evidence_with_faults(request, || Ok(()))
    }
    #[doc(hidden)]
    pub fn verify_red_green_evidence_with_faults(
        &self,
        request: &RedGreenEvidenceRequest,
        before_revalidation: impl FnOnce() -> Result<()>,
    ) -> Result<RedGreenEvidenceAssessment> {
        let captured = capture(self, request)?;
        let evidence = &captured.evidence;
        let attestation = &captured.attestation;
        let declaration = &evidence.reference.declaration;
        let verified =
            self.reauthenticate_imported_red_green(&attestation.record.id, &request.authority)?;
        let report = self.reauthenticate_imported_report(
            &attestation.record.id,
            &request.authority.policy,
            &request.authority.expected_policy,
            &request.authority.candidate,
        )?;
        if declaration.subject
            != (ExactSubject {
                repository: verified.pair.repository.clone(),
                kind: ExactSubjectKind::Source,
                content: verified.pair.candidate.content.clone(),
            })
            || declaration.check != verified.pair.check
            || declaration.producer != verified.pair.green_producer
            || declaration.result.id != verified.pair.green_run.as_str()
            || declaration.result.content != report.report.fingerprint
            || declaration.observed_at != report.report.observed_at
        {
            return Err(blocked(
                "evidence source, check, producer, result or observation differs from authenticated execution",
            ));
        }
        let root = self
            .root()
            .parent()
            .ok_or_else(|| blocked("missing worktree root"))?;
        let committed = crate::sources::resolve_ci_criterion(
            root,
            &verified.pair.candidate,
            &declaration.criterion,
        )?;
        if committed.retired || committed.reference != declaration.criterion {
            return Err(blocked(
                "criterion was absent, retired or different in the verified candidate",
            ));
        }
        before_revalidation()?;
        if capture(self, request)? != captured {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "evidence, attestation or criterion changed during verification",
            ));
        }
        Ok(RedGreenEvidenceAssessment {
            basis: EvidenceVerificationBasis::AuthenticatedCheckLink,
            evidence: evidence.reference.id.clone(),
            evidence_content: evidence.content.clone(),
            attestation: attestation.record.id.clone(),
            attestation_content: attestation.content.clone(),
            criterion: captured.criterion,
            committed_criterion: committed,
            verification: verified,
        })
    }

    /// Qualify a declaration against a signed passed check under independently
    /// supplied producer authority. Configured or retained red/green proof is
    /// still enforced by the shared imported-check verifier.
    pub fn verify_authenticated_check_evidence(
        &self,
        request: &VerifiedEvidenceRequest,
    ) -> Result<VerifiedEvidenceAssessment> {
        let captured = capture_verified(self, request)?;
        let evidence = &captured.evidence;
        let attestation = &captured.attestation;
        let declaration = &evidence.reference.declaration;
        if declaration.subject.kind != ExactSubjectKind::Source {
            return Err(blocked(
                "authenticated check evidence must cite an exact source subject",
            ));
        }
        let report = self.verify_imported_check(&VerifyImportedCheck {
            attestation: attestation.record.id.clone(),
            expected_attestation: attestation.content.clone(),
            check: declaration.check.id.clone(),
            candidate: request.authority.candidate.clone(),
            policy: request.authority.policy.clone(),
            expected_policy: request.authority.expected_policy.clone(),
            red_green: attestation
                .record
                .input
                .red_green
                .as_ref()
                .map(|_| {
                    request
                        .authority
                        .red_green
                        .clone()
                        .ok_or_else(|| blocked("retained pair requires current authority"))
                })
                .transpose()?,
        })?;
        let check = report
            .report
            .publication
            .result
            .result
            .checks
            .iter()
            .find(|check| check.check.id == declaration.check.id)
            .ok_or_else(|| blocked("signed report omits the declared evidence check"))?;
        if declaration.subject.repository != report.source.repository
            || declaration.subject.content != report.source.content
            || declaration.producer != report.producer
            || declaration.check != check.check
            || declaration.result.id != report.report.publication.intent.intent.id.as_str()
            || declaration.result.content != report.report.fingerprint
            || declaration.observed_at != report.report.observed_at
            || check.state != RunState::Passed
            || report
                .report
                .publication
                .result
                .result
                .invocations
                .get(check.invocation)
                .is_none_or(|invocation| {
                    invocation.state != RunState::Passed || !invocation.inputs_unchanged
                })
        {
            return Err(blocked(
                "evidence source, check, producer, result or observation differs from authenticated execution",
            ));
        }
        let root = self
            .root()
            .parent()
            .ok_or_else(|| blocked("missing worktree root"))?;
        let committed =
            crate::sources::resolve_ci_criterion(root, &report.source, &declaration.criterion)?;
        if committed.retired || committed.reference != declaration.criterion {
            return Err(blocked(
                "criterion was absent, retired or different in the authenticated candidate",
            ));
        }
        if capture_verified(self, request)? != captured {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "evidence, attestation or criterion changed during verification",
            ));
        }
        Ok(VerifiedEvidenceAssessment {
            basis: EvidenceVerificationBasis::AuthenticatedCheck,
            evidence: evidence.reference.id.clone(),
            evidence_content: evidence.content.clone(),
            attestation: attestation.record.id.clone(),
            attestation_content: attestation.content.clone(),
            criterion: captured.criterion,
            committed_criterion: committed,
            verification: report,
        })
    }
}

#[derive(PartialEq, Eq)]
struct VerifiedCapture {
    evidence: EvidenceRecord,
    attestation: ImportedCheckReportRecord,
    criterion: ResolvedCriterion,
}

fn capture_verified(
    repo: &Repository,
    request: &VerifiedEvidenceRequest,
) -> Result<VerifiedCapture> {
    repo.store()?.with_snapshot(|snapshot| {
        let config = crate::repository::config_from_snapshot(repo.root(), snapshot)?;
        let now = chrono::Utc::now();
        let records = store::load_evidence(snapshot, &config)?;
        let evidence = store::active_at(&records, now)
            .into_iter()
            .find(|record| record.reference.id == request.evidence)
            .ok_or_else(|| blocked("evidence is missing, not yet recorded or superseded"))?
            .clone();
        if evidence.content != request.expected_evidence {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "evidence differs from its independently selected content pin",
            ));
        }
        let declaration = &evidence.reference.declaration;
        if declaration.expires_at.is_some_and(|expiry| expiry <= now) {
            return Err(blocked("evidence declaration has expired"));
        }
        let (id, content) = declaration
            .links
            .iter()
            .find_map(|link| match link {
                EvidenceLink::Attestation { id, content } => Some((id, content)),
                _ => None,
            })
            .ok_or_else(|| blocked("evidence has no pinned attestation link"))?;
        let attestation = crate::attestations::load(snapshot, &config.repository)?
            .into_iter()
            .find(|record| &record.record.id == id)
            .ok_or_else(|| blocked("linked attestation is missing"))?;
        if &attestation.content != content {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "linked attestation content differs from declaration",
            ));
        }
        let criterion = crate::gates::resolve_criterion(
            repo.root(),
            snapshot,
            &config,
            &declaration.criterion.owner,
            &declaration.criterion.id,
        )?;
        if criterion.retired || criterion.reference != declaration.criterion {
            return Err(blocked(
                "current criterion is retired or differs from the evidence definition",
            ));
        }
        Ok(VerifiedCapture {
            evidence,
            attestation,
            criterion,
        })
    })
}
