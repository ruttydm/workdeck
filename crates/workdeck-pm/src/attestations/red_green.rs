use super::*;
use std::path::Path;

impl RetainedRedGreenProof {
    pub(crate) fn request(&self, input: &ImportCheckReportRequest) -> Result<RedGreenRequest> {
        Ok(RedGreenRequest {
            baseline: self.baseline.clone(),
            candidate: input.expected_commit.clone(),
            check: self.check.clone(),
            producer_policy: input.policy.clone(),
            expected_producer_policy: input.expected_policy.clone(),
            red: self.red.clone(),
            green: SignedCheckReport::from_json(input.envelope.as_bytes())?,
            red_artifact: self.red_artifact.clone(),
            green_artifact: self.green_artifact.clone(),
        })
    }
    pub(crate) fn verify_import(
        &self,
        root: &Path,
        input: &ImportCheckReportRequest,
    ) -> Result<()> {
        let request = self.request(input)?;
        if let Some(review) = &self.review {
            verify_reviewed_red_green(root, &request, review)?;
        } else {
            verify_red_green(root, &request)?;
        }
        Ok(())
    }
}
impl Repository {
    pub fn reauthenticate_imported_red_green(
        &self,
        id: &AttestationId,
        authority: &RetainedRedGreenAuthority,
    ) -> Result<RetainedRedGreenAssessment> {
        let record = self.imported_check_report(id)?;
        let input = &record.record.input;
        let proof = input
            .red_green
            .as_ref()
            .ok_or_else(|| invalid("import has no retained red/green proof"))?;
        if proof.baseline != authority.baseline || input.expected_commit != authority.candidate {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "retained proof differs from independently accepted baseline/candidate",
            ));
        }
        let mut request = proof.request(input)?;
        request.producer_policy = authority.policy.clone();
        request.expected_producer_policy = authority.expected_policy.clone();
        let root = self
            .root()
            .parent()
            .ok_or_else(|| invalid("missing worktree root"))?;
        match (&proof.review, &authority.review) {
            (Some(stored), Some(current)) => {
                let review = RedGreenBaselineReview {
                    accepted: current.baseline.clone(),
                    policy: current.policy.clone(),
                    expected_policy: current.expected_policy.clone(),
                    envelope: stored.envelope.clone(),
                };
                let verified = verify_reviewed_red_green(root, &request, &review)?;
                Ok(RetainedRedGreenAssessment {
                    pair: verified.pair,
                    review: Some(verified.review),
                })
            }
            (None, None) => Ok(RetainedRedGreenAssessment {
                pair: verify_red_green(root, &request)?,
                review: None,
            }),
            _ => Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "retained reviewed proof requires explicit current reviewer authority; review cannot be discarded or synthesized",
            )),
        }
    }
}
