use super::*;
use crate::transactions::Snapshot;
use std::path::Path;
const VISIBLE: usize = 5;
pub(super) struct Reviews {
    pub report: Option<ReviewCoverage>,
}
impl Reviews {
    pub fn capture(
        root: &Path,
        snapshot: &Snapshot<'_>,
        config: &Config,
        issue: &IssueRecord,
    ) -> Result<Self> {
        let records = crate::retained_reviews::load(snapshot, &config.repository)?;
        if records.is_empty() {
            return Ok(Self { report: None });
        }
        let request = ReviewCoverageRequest {
            revision: CiRevision::Head {},
            working_tree: true,
            subject: Some(CiSubjectIdentity::Issue {
                id: issue.metadata.id.clone(),
            }),
            expected_subject: Some(issue.source.content.clone()),
            authority: None,
        };
        let report = crate::review_coverage::capture_records(
            root,
            snapshot,
            &config.repository,
            &request,
            records,
        )?;
        Ok(Self {
            report: Some(report),
        })
    }
    pub fn verify(
        &self,
        root: &Path,
        snapshot: &Snapshot<'_>,
        config: &Config,
        issue: &IssueRecord,
    ) -> Result<()> {
        let current = Self::capture(root, snapshot, config, issue)?;
        if current.report.as_ref().map(|r| &r.fingerprint)
            != self.report.as_ref().map(|r| &r.fingerprint)
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Contract reviews or their selected revision changed while capturing context",
            ));
        }
        Ok(())
    }
    pub fn section(&self) -> Option<ContextSection> {
        let report = self.report.as_ref()?;
        let mut rows = report.rows.iter().collect::<Vec<_>>();
        rows.sort_by(|a, b| {
            b.review
                .imported_at
                .cmp(&a.review.imported_at)
                .then_with(|| a.review.id.cmp(&b.review.id))
        });
        let total = rows.len() + report.diagnostics.len();
        let mut entries = rows
            .into_iter()
            .take(VISIBLE)
            .map(|summary| ContextEntry {
                citations: vec![ContextCitation {
                    target: ContextTarget::PlanningSource {
                        path: summary.review.path.clone(),
                    },
                    source: Some(summary.review.content.clone()),
                }],
                content: ContextContent::ContractReview {
                    summary: Box::new(summary.clone()),
                    selected_revision: report.source.as_ref().map(|s| s.commit.clone()),
                },
            })
            .collect::<Vec<_>>();
        entries.extend(
            report
                .diagnostics
                .iter()
                .take(2)
                .map(|error| super::notice("review_assessment_unavailable", &error.message)),
        );
        Some(ContextSection {
            kind: ContextSectionKind::Reviews,
            total,
            omitted: 0,
            coverage_complete: entries.len() == total,
            omission_reasons: if entries.len() < total {
                vec!["review_display_limit".into()]
            } else {
                Vec::new()
            },
            entries,
        })
    }
}
