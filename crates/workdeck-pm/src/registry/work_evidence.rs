//! Supplemental observations are pinned to the selected planning source. They do
//! not confer mutation authority, claim confirmation, or completion admission.
use super::*;
use crate::{
    IssueId, IssueReadiness, PlanningSourceView, QuestionApplicability, SourceObservation,
    WorkflowCategory, projection::ProjectionReadView,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::Path, time::Instant};

#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MyWorkEvidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readiness: Option<IssueReadiness>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<crate::ClaimStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_requirements_match: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub questions: Vec<QuestionApplicability>,
}
impl MyWorkEvidence {
    pub fn is_empty(&self) -> bool {
        self.readiness.is_none()
            && self.questions.is_empty()
            && self.claim.is_none()
            && self.selected_requirements_match.is_none()
    }
}

pub(super) struct CapturedEvidence {
    source: PlanningSourceView,
    supplemental: Vec<PlanningSourceView>,
    pub rows: BTreeMap<IssueId, MyWorkEvidence>,
}
fn time_left(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(PmError::new(
            ErrorCode::Io,
            "My work evidence observation budget expired",
        ));
    }
    Ok(())
}
impl CapturedEvidence {
    pub fn blocked(
        entry: &RegisteredCheckout,
        view: &ProjectionReadView,
        actor: &str,
        deadline: Instant,
    ) -> Result<Self> {
        time_left(deadline)?;
        let source = crate::sources::capture_with_deadline(
            &entry.checkout,
            &entry.source,
            &view.limits().source,
            deadline,
        )?;
        if source.observation.identity != view.id().source
            || source.publication_binding() != view.publication_binding()
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Planning changed between indexed rows and blocker assessment; refresh the report",
            ));
        }
        let rows = source.snapshot.with_snapshot(|snapshot| {
            let root = Path::new("source-snapshot");
            let config = source.snapshot.config()?;
            let graph = crate::graph::capture(root, snapshot, &config)?;
            let questions = crate::questions::load_questions(root, snapshot, &config)?;
            let mut rows = BTreeMap::new();
            let mut bytes = 0usize;
            for issue in graph.issues() {
                time_left(deadline)?;
                if issue.metadata.archived
                    || issue.retirement.is_some()
                    || issue.metadata.assignee.as_deref() != Some(actor)
                    || matches!(
                        config.workflow.state(&issue.metadata.status)?.category,
                        WorkflowCategory::Completed | WorkflowCategory::Canceled
                    )
                {
                    continue;
                }
                let readiness = graph.readiness(&issue.metadata.id)?;
                let applicable = crate::questions::applicability(
                    root,
                    snapshot,
                    &config,
                    &questions,
                    &crate::context::issue_subjects(issue),
                )?;
                if readiness.ready
                    && !applicable
                        .iter()
                        .any(|question| question.blocks_implementation)
                {
                    continue;
                }
                let evidence = MyWorkEvidence {
                    readiness: Some(readiness),
                    claim: None,
                    selected_requirements_match: None,
                    questions: applicable,
                };
                let encoded =
                    serde_json::to_vec(&evidence).map_err(|error| invalid(error.to_string()))?;
                bytes = bytes
                    .checked_add(encoded.len())
                    .ok_or_else(|| invalid("My work evidence size overflow"))?;
                if bytes > view.limits().max_query_bytes {
                    return Err(PmError::new(
                        ErrorCode::Unsupported,
                        "My work explanations exceed the configured query memory bound",
                    ));
                }
                rows.insert(issue.metadata.id.clone(), evidence);
            }
            Ok(rows)
        })?;
        time_left(deadline)?;
        Ok(Self {
            source,
            supplemental: Vec::new(),
            rows,
        })
    }
    pub fn claimed(
        entry: &RegisteredCheckout,
        view: &ProjectionReadView,
        actor: &str,
        as_of: crate::Timestamp,
        deadline: Instant,
    ) -> Result<Self> {
        time_left(deadline)?;
        let capture = |selector: &SourceSelector| {
            crate::sources::capture_with_deadline(
                &entry.checkout,
                selector,
                &view.limits().source,
                deadline,
            )
        };
        let source = capture(&entry.source)?;
        if source.observation.identity != view.id().source
            || source.publication_binding() != view.publication_binding()
        {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "Planning changed before claim assessment; refresh the report",
            ));
        }
        let shared = source.snapshot.config()?.sources.is_some();
        let accepted = if shared && entry.source != SourceSelector::Accepted {
            Some(capture(&SourceSelector::Accepted)?)
        } else {
            None
        };
        let coordination = if shared {
            Some(capture(&SourceSelector::Coordination)?)
        } else {
            None
        };
        let statuses = coordination.as_ref().unwrap_or(&source).claim_statuses_at(
            accepted.as_ref().unwrap_or(&source),
            as_of,
            deadline,
        )?;
        let mut rows = BTreeMap::new();
        let mut bytes = 0usize;
        for claim in statuses {
            time_left(deadline)?;
            if claim.claim.metadata.actor != actor
                || claim.claim.metadata.state != crate::ClaimState::Active
            {
                continue;
            }
            let id = claim.claim.metadata.issue.clone();
            let selected_requirements_match = source.snapshot.with_snapshot(|snapshot| {
                let root = Path::new("source-snapshot");
                let config = source.snapshot.config()?;
                let selected =
                    match crate::issues::resolve_issue(root, snapshot, &config, id.as_str()) {
                        Ok(selected) => selected,
                        Err(error) if error.code == ErrorCode::NotFound => return Ok(None),
                        Err(error) => return Err(error),
                    };
                Ok(Some(
                    selected.source == claim.claim.metadata.contract.issue_source
                        && crate::context::requirement_fingerprint(root, snapshot, &config, &id)?
                            == claim.claim.metadata.contract.requirements,
                ))
            })?;
            let evidence = MyWorkEvidence {
                claim: Some(claim),
                selected_requirements_match,
                ..Default::default()
            };
            bytes = bytes
                .checked_add(
                    serde_json::to_vec(&evidence)
                        .map_err(|error| invalid(error.to_string()))?
                        .len(),
                )
                .ok_or_else(|| invalid("My work evidence size overflow"))?;
            if bytes > view.limits().max_query_bytes {
                return Err(PmError::new(
                    ErrorCode::Unsupported,
                    "My work claim evidence exceeds the configured query memory bound",
                ));
            }
            rows.insert(id, evidence);
        }
        Ok(Self {
            source,
            supplemental: accepted.into_iter().chain(coordination).collect(),
            rows,
        })
    }

    pub fn observations(&self) -> Vec<SourceObservation> {
        std::iter::once(&self.source)
            .chain(self.supplemental.iter())
            .map(|source| source.observation.clone())
            .collect()
    }
    pub fn revalidate(&self, deadline: Instant) -> Result<()> {
        time_left(deadline)?;
        self.source.revalidate_before(deadline)?;
        for source in &self.supplemental {
            source.revalidate_before(deadline)?;
        }
        Ok(())
    }
}
