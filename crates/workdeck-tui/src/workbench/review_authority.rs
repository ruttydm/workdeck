//! Explicit, task-local inspection authority. Never inferred from retained proof.
use super::{
    context_workspace::{ContextWorkspace, Row},
    input::{FormKind, TextField, WorkbenchForm},
};
use workdeck_pm::*;

#[derive(Debug, Default)]
pub(super) struct ReviewAssessment {
    pub form: Option<WorkbenchForm>,
    pub visible: bool,
    pub authority: Option<ReviewCoverageAuthority>,
    pub report: Option<ReviewCoverage>,
    pub error: Option<PmError>,
}
impl ReviewAssessment {
    pub fn edited(&mut self) {
        self.report = None;
        self.authority = None;
        self.error = None;
    }
    pub fn rows(&self) -> Vec<Row> {
        if self.form.is_none() {
            return Vec::new();
        }
        let (status, body) = match &self.report {
            Some(report) => {
                let mut details = vec![format!(
                    "Inspected review authentication\nAssessed HEAD: {}\nObserved: {}\nIndependent policy: {}\nWorking planning/evaluator comparison: {}\n",
                    report.source.as_ref().map(|s| s.commit.to_string()).unwrap_or_else(|| "unavailable".into()),
                    report.assessed_at.to_rfc3339(),
                    report.request.authority.as_ref().map(|a| a.expected_policy.to_string()).unwrap_or_else(|| "none".into()),
                    report.working_tree.as_ref().map(|w| if w.matches_revision {"matches"} else {"stale"}).unwrap_or("unavailable"))];
                details.extend(report.diagnostics.iter().map(|e| e.message.clone()));
                for row in &report.rows {
                    details.push(format!("{} · {:?}\nCurrent reviewers: {}\n{}", row.review.id, row.state,
                        if row.current_reviewers.is_empty() { "none".into() } else { row.current_reviewers.join(", ") }, row.reason_codes.join(" · ")));
                    if let Some(error) = &row.diagnostic { details.push(error.message.clone()); }
                }
                if report.rows.is_empty() { details.push("No retained review covers this selection.".into()); }
                details.push("Inspection only; no check success or completion is granted. r revalidates with the submitted pins; v edits them. Historical proof citations remain below.".into());
                (if report.authenticated { "Authenticated" } else { "Not authenticated" }, details.join("\n"))
            }
            None => ("Not authenticated", self.error.as_ref().map(|e| e.message.clone()).unwrap_or_else(|| "No current assessment. v supplies independent review authority; no policy is inferred from candidate files or retained proof.".into())),
        };
        vec![Row {
            id: "review-auth:summary".into(),
            title: format!("Review authentication · {status}"),
            body,
            target: None,
        }]
    }
}
impl ContextWorkspace {
    pub(super) fn begin_review_authentication(&mut self) -> Result<()> {
        if self.current.is_none() || self.state().and_then(|s| s.packet.as_ref()).is_none() {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "Inspect a task before assessing its reviews",
            ));
        }
        let state = self.state_mut();
        let review = &mut state.review;
        review.form.get_or_insert_with(|| {
            let mut form = WorkbenchForm::new(FormKind::Planning, "Authenticate task review", vec![
                TextField::new("Independent reviewer policy JSON", String::new(), true),
                TextField::new("Expected policy hash", String::new(), false),
                TextField::new("Accepted baseline commit", String::new(), false),
                TextField::new("Accepted baseline contract hash", String::new(), false),
            ]);
            form.help = vec!["Paste policy and pins obtained through an independent accepted channel. No values are copied from candidate files or historical proof.".into(),
                "Ctrl-S inspects HEAD and live planning/evaluators. This is read-only and does not grant completion.".into()];
            form
        });
        review.visible = true;
        state.error = None;
        Ok(())
    }
    pub(super) fn submit_review_authentication(&mut self) -> Result<()> {
        let result: Result<()> = (|| {
            let review = &mut self.state_mut().review;
            review.edited();
            let form = review
                .form
                .as_ref()
                .ok_or_else(|| PmError::new(ErrorCode::InvalidInput, "No review authority form"))?;
            let authority = ReviewCoverageAuthority {
                policy: ContractReviewPolicy::from_json(form.fields[0].value.as_bytes())?,
                expected_policy: form.fields[1].value.trim().parse()?,
                baseline: CiBaselinePin {
                    commit: form.fields[2].value.trim().parse()?,
                    contract: form.fields[3].value.trim().parse()?,
                },
            };
            review.authority = Some(authority);
            self.refresh_review_authentication()?;
            let state = self.state_mut();
            state.review.visible = false;
            state.selected[0] = Some("review-auth:summary".into());
            state.scroll[0] = 0;
            state.error = None;
            Ok(())
        })();
        if let Err(error) = &result {
            self.state_mut().review.error = Some(error.clone());
        }
        result
    }
    pub(super) fn refresh_review_authentication(&mut self) -> Result<()> {
        let Some(authority) = self.state().and_then(|s| s.review.authority.clone()) else {
            return Ok(());
        };
        self.state_mut().review.report = None;
        let result = (|| {
            let repository = self.repository()?;
            let packet = self
                .state()
                .and_then(|s| s.packet.as_ref())
                .ok_or_else(|| {
                    PmError::new(ErrorCode::StaleSource, "Task context is unavailable")
                })?;
            if packet.anchor.repository != *repository.identity()
                || self.current.as_ref() != Some(&packet.anchor.issue)
            {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "Review selection belongs to another task or repository",
                ));
            }
            repository.contract_review_coverage(&ReviewCoverageRequest {
                revision: CiRevision::Head {},
                working_tree: true,
                subject: Some(CiSubjectIdentity::Issue {
                    id: packet.anchor.issue.clone(),
                }),
                expected_subject: Some(packet.anchor.issue_source.content.clone()),
                authority: Some(authority),
            })
        })();
        match result {
            Ok(report) => {
                let review = &mut self.state_mut().review;
                review.report = Some(report);
                review.error = None;
                Ok(())
            }
            Err(error) => {
                self.state_mut().review.error = Some(error.clone());
                Err(error)
            }
        }
    }
}
