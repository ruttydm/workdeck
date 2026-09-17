use crate::*;
fn blocked(message: &str) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
fn stale(message: &str) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
impl Repository {
    /// Qualify an exact check for current HEAD; this read-only result is not a mutation credential.
    pub fn verify_imported_check(
        &self,
        input: &VerifyImportedCheck,
    ) -> Result<AuthenticatedCheckReport> {
        crate::gates::validation::text(&input.check, "check selection", 256)?;
        let root = self
            .root()
            .parent()
            .ok_or_else(|| blocked("missing worktree"))?;
        let git = crate::sources::git::BoundGit::open_shared(root)?;
        if git.head()?.as_ref() != Some(&input.candidate) {
            return Err(stale("qualified check candidate is not current HEAD"));
        }
        let record = self.imported_check_report(&input.attestation)?;
        if record.content != input.expected_attestation {
            return Err(stale(
                "qualified check attestation differs from its selected pin",
            ));
        }
        let prepared = prepare_ci_check(
            root,
            &CiRevision::Commit {
                oid: input.candidate.clone(),
            },
            &CheckPlanRequest {
                checks: vec![input.check.clone()],
                ..Default::default()
            },
        )?;
        let definition = prepared
            .plan
            .definitions
            .checks
            .iter()
            .find(|c| c.definition.id == input.check)
            .ok_or_else(|| blocked("selected check definition is missing"))?;
        if definition.definition.red_green.is_some()
            || record.record.input.red_green.is_some()
            || input.red_green.is_some()
        {
            let authority = input
                .red_green
                .as_ref()
                .ok_or_else(|| blocked("selected check requires explicit red/green authority"))?;
            if authority.candidate != input.candidate
                || authority.policy != input.policy
                || authority.expected_policy != input.expected_policy
            {
                return Err(blocked(
                    "red/green authority differs from selected check authority",
                ));
            }
            if record
                .record
                .input
                .red_green
                .as_ref()
                .is_none_or(|p| p.check != input.check)
            {
                return Err(blocked(
                    "selected check requires its original red/green proof",
                ));
            }
            let pair = self.reauthenticate_imported_red_green(&input.attestation, authority)?;
            if pair.pair.check.id != input.check || pair.pair.candidate != prepared.binding.source {
                return Err(blocked("red/green proof qualifies another check or source"));
            }
        }
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let guards =
                crate::checks::prepare_plan(self.root(), snapshot, &config, &prepared.plan)?;
            if !super::load(snapshot, self.identity())?.contains(&record) {
                return Err(stale(
                    "qualified check attestation changed during verification",
                ));
            }
            let mut original = record.record.input.clone();
            original.policy = input.policy.clone();
            original.expected_policy = input.expected_policy.clone();
            if original.expected_commit != input.candidate {
                return Err(blocked("qualified check report names another commit"));
            }
            if let (Some(pair), Some(authority)) = (&mut original.red_green, &input.red_green)
                && let (Some(review), Some(current)) = (&mut pair.review, &authority.review)
            {
                review.accepted = current.baseline.clone();
                review.policy = current.policy.clone();
                review.expected_policy = current.expected_policy.clone();
            }
            let report = super::records::authenticate(&original, chrono::Utc::now())?;
            crate::completion::match_check(
                &prepared,
                &CompletionCheckSelection {
                    check: input.check.clone(),
                    attestation: input.attestation.clone(),
                    expected_attestation: input.expected_attestation.clone(),
                },
                &report,
            )?;
            let results = &report.report.publication.result.result;
            let check = results
                .checks
                .iter()
                .find(|c| c.check.id == input.check)
                .ok_or_else(|| blocked("signed report omits the selected check result"))?;
            if check.state != RunState::Passed
                || results
                    .invocations
                    .get(check.invocation)
                    .is_none_or(|i| i.state != RunState::Passed || !i.inputs_unchanged)
            {
                return Err(blocked(
                    "authenticated check or its invocation did not pass",
                ));
            }
            for guard in guards {
                guard.verify()?;
            }
            if git.head()?.as_ref() != Some(&input.candidate) {
                return Err(stale("HEAD moved during check qualification"));
            }
            git.verify()?;
            Ok(report)
        })
    }
}
