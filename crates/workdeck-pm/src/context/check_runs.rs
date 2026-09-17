use super::*;
use crate::transactions::Snapshot;
use std::path::Path;

const VISIBLE_RUNS: usize = 5;
const VISIBLE_FAILURES: usize = 8;
const VISIBLE_ARTIFACTS: usize = 16;

pub(super) struct Runs {
    pub summaries: Vec<ContextCheckRun>,
    pub total: usize,
    pub fingerprint: ContentHash,
}
impl Runs {
    pub fn capture(
        root: &Path,
        snapshot: &Snapshot<'_>,
        config: &Config,
        issue: &IssueId,
    ) -> Result<Self> {
        let (outcomes, total, fingerprint) =
            crate::execution::results_for_context(root, snapshot, config, issue, VISIBLE_RUNS)?;
        let summaries = outcomes.into_iter().map(summary).collect();
        Ok(Self {
            summaries,
            total,
            fingerprint,
        })
    }
    pub fn verify(
        &self,
        root: &Path,
        snapshot: &Snapshot<'_>,
        config: &Config,
        issue: &IssueId,
    ) -> Result<()> {
        let (_, _, current) =
            crate::execution::results_for_context(root, snapshot, config, issue, VISIBLE_RUNS)?;
        if current != self.fingerprint {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Local check results or their current source/artifact assessment changed while capturing context",
            ));
        }
        Ok(())
    }
    pub fn section(&self) -> ContextSection {
        let entries = self
            .summaries
            .iter()
            .map(|summary| {
                let citations = std::iter::once(&summary.intent)
                    .chain(summary.result.iter())
                    .map(|pin| ContextCitation {
                        target: ContextTarget::PlanningSource {
                            path: pin.path.clone(),
                        },
                        source: Some(pin.content.clone()),
                    })
                    .collect();
                ContextEntry {
                    content: ContextContent::CheckRun {
                        summary: summary.clone(),
                    },
                    citations,
                }
            })
            .collect();
        ContextSection {
            kind: ContextSectionKind::Checks,
            total: self.total,
            omitted: 0,
            entries,
            coverage_complete: true,
            omission_reasons: if self.total > self.summaries.len() {
                vec!["older_runs_not_assessed".into()]
            } else {
                Vec::new()
            },
        }
    }
}

fn summary(outcome: RunOutcome) -> ContextCheckRun {
    let mut failures = Vec::new();
    let mut omitted_failures = 0;
    if let Some(result) = &outcome.results {
        for check in &result.result.checks {
            omitted_failures += check.report.omitted_failures;
            for diagnostic in &check.report.failures {
                if failures.len() == VISIBLE_FAILURES {
                    omitted_failures += 1;
                } else {
                    failures.push(ContextCheckFailure {
                        check: check.check.id.clone(),
                        diagnostic: diagnostic.clone(),
                    });
                }
            }
        }
    }
    let mut artifacts = outcome.assessment.artifacts;
    let omitted_artifacts = artifacts.len().saturating_sub(VISIBLE_ARTIFACTS);
    artifacts.truncate(VISIBLE_ARTIFACTS);
    ContextCheckRun {
        id: outcome.run.intent.id,
        request_id: outcome.run.intent.request_id,
        recorded_at: outcome.run.intent.recorded_at,
        state: outcome.assessment.state,
        historical_state: outcome.assessment.historical_state,
        basis: VerificationBasis::LocalFeedback,
        plan: outcome.run.intent.input.expected_plan,
        intent: SourcePin {
            path: outcome.run.path,
            content: outcome.run.content,
        },
        result: outcome.results.map(|record| SourcePin {
            path: record.path,
            content: record.content,
        }),
        reason_codes: outcome.assessment.reason_codes,
        artifacts,
        omitted_artifacts,
        failures,
        omitted_failures,
    }
}
