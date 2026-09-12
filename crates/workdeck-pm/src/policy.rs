//! Shared policy evaluation for planning exits and feature maturity.
//!
//! Policy declarations remain authored in their owning records.  This module
//! evaluates a point-in-time source snapshot and keeps the authority used for
//! a mutation explicit.  A manual acceptance is an attributed declaration; it
//! is never presented as CI, reviewed evidence, or a producer-authenticated
//! check.

use crate::{
    CompletionCondition, ConditionKind, ConditionState, ContentHash, ErrorCode, FeatureId,
    FeatureMaturity, IssueRecord, PlanningCriterion, PlanningKind, PlanningRecord, PmError,
    Repository, RepositoryId, Result, SourcePin, SubjectRef, Timestamp, WorkflowCategory,
    transactions::{MutationReceipt, Snapshot, canonical_hash},
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

const MAX_POLICY_CONDITIONS: usize = 4096;
const MAX_POLICY_PINS: usize = 4096;

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyBasis {
    /// A declaration made by an attributed human/agent actor for this subject.
    ManualAcceptance,
    /// A local check/result without independent producer authentication.
    LocalFeedback,
    /// Evidence admitted under an independent review authority.
    ReviewedEvidence,
    /// A producer-authenticated CI result.
    CiQualification,
    /// A declaration or status without an acceptance authority.
    Declared,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyAcceptance {
    pub actor: String,
    pub reason: String,
}

impl PolicyAcceptance {
    pub fn validate(&self) -> Result<()> {
        text(&self.actor, "policy acceptance actor", 256)?;
        text(&self.reason, "policy acceptance reason", 16 * 1024)
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyAssessment {
    pub schema: crate::SchemaVersion,
    pub repository: RepositoryId,
    pub subject: SubjectRef,
    pub current: String,
    pub requested: String,
    pub basis: PolicyBasis,
    pub allowed: bool,
    pub conditions: Vec<CompletionCondition>,
    pub source_pins: Vec<SourcePin>,
    pub fingerprint: ContentHash,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance: Option<PolicyAcceptance>,
    pub assessed_at: Timestamp,
}

fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn blocked(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::PolicyBlocked, message)
}
fn stale(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::StaleSource, message)
}
fn text(value: &str, name: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{name} must be nonempty text up to {max} bytes without control characters"
        )));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn condition(
    subject: SubjectRef,
    kind: ConditionKind,
    related: Option<SubjectRef>,
    state: ConditionState,
    reason_code: impl Into<String>,
    message: impl Into<String>,
    source_pins: Vec<SourcePin>,
    basis: PolicyBasis,
) -> CompletionCondition {
    CompletionCondition {
        kind,
        subject: subject.clone(),
        related_subject: related,
        state,
        reason_code: reason_code.into(),
        message: message.into(),
        path: vec![subject],
        source_pins,
        basis: serde_json::to_value(basis)
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_else(|| "declared".into()),
    }
}

fn issue_basis(issue: &IssueRecord) -> PolicyBasis {
    if issue.metadata.manual_acceptance.is_some() {
        PolicyBasis::ManualAcceptance
    } else if issue
        .metadata
        .imported_completion
        .as_ref()
        .is_some_and(crate::ImportedCompletion::is_active)
    {
        PolicyBasis::CiQualification
    } else {
        // A completed issue without a retained authenticated receipt is still
        // useful local feedback, but it is not independent CI qualification.
        PolicyBasis::LocalFeedback
    }
}

fn pin(path: impl Into<std::path::PathBuf>, content: ContentHash) -> SourcePin {
    SourcePin {
        path: path.into(),
        content,
    }
}

fn planning_subject(kind: PlanningKind, id: &str) -> Result<SubjectRef> {
    match kind {
        PlanningKind::Project => Ok(SubjectRef::Project(id.into())),
        PlanningKind::Milestone => Ok(SubjectRef::Milestone(id.into())),
        _ => Err(invalid(
            "completion policy is defined for projects and milestones only",
        )),
    }
}

fn planning_criteria(record: &PlanningRecord) -> &[PlanningCriterion] {
    match record.kind {
        PlanningKind::Project => &record.metadata.exit_criteria,
        PlanningKind::Milestone => &record.metadata.outcomes,
        _ => &[],
    }
}

fn completed_status(config: &crate::Config, status: Option<&str>) -> Result<bool> {
    status
        .map(|value| config.workflow.state(value))
        .transpose()
        .map(|state| state.is_some_and(|state| state.category == WorkflowCategory::Completed))
}

fn push(
    conditions: &mut Vec<CompletionCondition>,
    source_pins: &mut Vec<SourcePin>,
    value: CompletionCondition,
) -> Result<()> {
    if conditions.len() >= MAX_POLICY_CONDITIONS {
        return Err(invalid("policy assessment exceeds 4096 conditions"));
    }
    for source in &value.source_pins {
        if !source_pins.iter().any(|existing| existing == source) {
            if source_pins.len() >= MAX_POLICY_PINS {
                return Err(invalid("policy assessment exceeds 4096 source pins"));
            }
            source_pins.push(source.clone());
        }
    }
    conditions.push(value);
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn finish(
    repository: &RepositoryId,
    subject: SubjectRef,
    current: impl Into<String>,
    requested: impl Into<String>,
    basis: PolicyBasis,
    allowed: bool,
    conditions: Vec<CompletionCondition>,
    source_pins: Vec<SourcePin>,
    acceptance: Option<PolicyAcceptance>,
) -> Result<PolicyAssessment> {
    let current = current.into();
    let requested = requested.into();
    let fingerprint = canonical_hash(&json!({
        "schema": 1,
        "repository": repository,
        "subject": subject,
        "current": current,
        "requested": requested,
        "basis": basis,
        "allowed": allowed,
        "conditions": conditions,
        "source_pins": source_pins,
        "acceptance": acceptance,
    }))?;
    Ok(PolicyAssessment {
        schema: crate::SchemaVersion::CURRENT,
        repository: repository.clone(),
        subject,
        current,
        requested,
        basis,
        allowed,
        conditions,
        source_pins,
        fingerprint,
        acceptance,
        assessed_at: chrono::Utc::now(),
    })
}

fn evaluate_planning(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &crate::Config,
    kind: PlanningKind,
    id: &str,
    acceptance: Option<&PolicyAcceptance>,
) -> Result<PolicyAssessment> {
    let subject = planning_subject(kind, id)?;
    let record = crate::planning::store::load_planning(root, snapshot, kind, id)?;
    let mut source_pins = vec![pin(record.path.clone(), record.source.content.clone())];
    let root_pin = source_pins[0].clone();
    let mut conditions = Vec::new();
    if record.metadata.archived
        || crate::retirement::read_tombstone(
            root,
            snapshot,
            config,
            &crate::RetirementTarget::new(kind.into(), id)?,
        )?
        .is_some()
    {
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Graph,
                None,
                ConditionState::Unknown,
                "planning_retired",
                "archived or retired planning records cannot be completed",
                vec![root_pin.clone()],
                PolicyBasis::Declared,
            ),
        )?;
    }

    let captured = crate::queries::capture(root, snapshot)?;
    let issues = captured
        .issues()
        .iter()
        .filter(|issue| match kind {
            PlanningKind::Project => issue.metadata.project.as_deref() == Some(id),
            PlanningKind::Milestone => issue.metadata.milestone.as_deref() == Some(id),
            _ => false,
        })
        .collect::<Vec<_>>();
    if issues.is_empty() {
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Child,
                None,
                ConditionState::Unknown,
                "no_issue_members",
                "completion policy requires at least one issue member",
                vec![],
                PolicyBasis::Declared,
            ),
        )?;
    }
    for issue in issues {
        let status = config.workflow.state(&issue.metadata.status)?;
        let (state, code, message) = match status.category {
            WorkflowCategory::Completed => (
                ConditionState::Satisfied,
                "issue_completed",
                format!("issue {} is complete", issue.metadata.id),
            ),
            WorkflowCategory::Canceled => (
                ConditionState::Unsatisfied,
                "issue_canceled",
                format!(
                    "issue {} is canceled and remains unresolved",
                    issue.metadata.id
                ),
            ),
            _ => (
                ConditionState::Unsatisfied,
                "issue_incomplete",
                format!("issue {} is not complete", issue.metadata.id),
            ),
        };
        let basis = if state == ConditionState::Satisfied {
            issue_basis(issue)
        } else {
            PolicyBasis::Declared
        };
        let source = pin(issue.path.clone(), issue.source.content.clone());
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Child,
                Some(SubjectRef::Issue(issue.metadata.id.clone())),
                state,
                code,
                message,
                vec![source],
                basis,
            ),
        )?;
    }

    // Projects also own milestones.  A project cannot be reported complete
    // while a declared milestone is left active or canceled.
    if kind == PlanningKind::Project {
        for milestone in
            crate::planning::store::list_planning(root, snapshot, PlanningKind::Milestone)?
                .into_iter()
                .filter(|milestone| milestone.metadata.project.as_deref() == Some(id))
        {
            let state = completed_status(config, milestone.metadata.status.as_deref())?;
            let state = if state {
                ConditionState::Satisfied
            } else {
                ConditionState::Unsatisfied
            };
            push(
                &mut conditions,
                &mut source_pins,
                condition(
                    subject.clone(),
                    ConditionKind::Child,
                    Some(SubjectRef::Milestone(milestone.metadata.id.clone())),
                    state,
                    if state == ConditionState::Satisfied {
                        "milestone_completed"
                    } else {
                        "milestone_incomplete"
                    },
                    if state == ConditionState::Satisfied {
                        format!("milestone {} is complete", milestone.metadata.id)
                    } else {
                        format!("milestone {} is not complete", milestone.metadata.id)
                    },
                    vec![pin(milestone.path, milestone.source.content)],
                    if state == ConditionState::Satisfied {
                        PolicyBasis::LocalFeedback
                    } else {
                        PolicyBasis::Declared
                    },
                ),
            )?;
        }
    }

    let criteria = planning_criteria(&record).to_vec();
    for criterion in criteria {
        let basis = acceptance.map_or(PolicyBasis::Declared, |_| PolicyBasis::ManualAcceptance);
        let state = if acceptance.is_some() {
            ConditionState::Satisfied
        } else {
            ConditionState::Unknown
        };
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Criterion,
                Some(SubjectRef::Criterion {
                    owner: Box::new(subject.clone()),
                    id: criterion.id.clone(),
                }),
                state,
                if state == ConditionState::Satisfied {
                    "manual_acceptance"
                } else {
                    "criterion_declared"
                },
                if state == ConditionState::Satisfied {
                    format!(
                        "criterion {} was accepted by the attributed actor",
                        criterion.id
                    )
                } else {
                    format!(
                        "criterion {} is declared but has no authenticated policy evidence",
                        criterion.id
                    )
                },
                vec![root_pin.clone()],
                basis,
            ),
        )?;
    }
    let basis = acceptance.map_or(PolicyBasis::Declared, |_| PolicyBasis::ManualAcceptance);
    let allowed = !conditions.is_empty()
        && conditions
            .iter()
            .all(|condition| condition.state == ConditionState::Satisfied)
        && (acceptance.is_none() || !planning_criteria(&record).is_empty());
    if let Some(acceptance) = acceptance {
        acceptance.validate()?;
        crate::organization::validate_actor(snapshot, &config.repository, &acceptance.actor)?;
    }
    finish(
        &config.repository,
        subject,
        record.metadata.status.unwrap_or_default(),
        "completed",
        basis,
        allowed,
        conditions,
        source_pins,
        acceptance.cloned(),
    )
}

fn maturity_rank(value: FeatureMaturity) -> u8 {
    match value {
        FeatureMaturity::Draft => 0,
        FeatureMaturity::Specified => 1,
        FeatureMaturity::Implemented => 2,
    }
}

fn maturity_name(value: FeatureMaturity) -> &'static str {
    match value {
        FeatureMaturity::Draft => "draft",
        FeatureMaturity::Specified => "specified",
        FeatureMaturity::Implemented => "implemented",
    }
}

fn evaluate_feature(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &crate::Config,
    id: &FeatureId,
    requested: FeatureMaturity,
    acceptance: Option<&PolicyAcceptance>,
) -> Result<PolicyAssessment> {
    let record = crate::features::load_feature(snapshot, config, id)?;
    let subject = SubjectRef::Feature(id.clone());
    let mut source_pins = vec![pin(record.path.clone(), record.source.content.clone())];
    let root_pin = source_pins[0].clone();
    let mut conditions = Vec::new();
    let current = record.metadata.maturity;
    let current_rank = maturity_rank(current);
    let requested_rank = maturity_rank(requested);
    if requested_rank < current_rank {
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Graph,
                None,
                ConditionState::Unsatisfied,
                "maturity_downgrade",
                "feature maturity cannot move backwards",
                vec![root_pin.clone()],
                PolicyBasis::Declared,
            ),
        )?;
    }
    if requested_rank > current_rank + 1 {
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Graph,
                None,
                ConditionState::Unsatisfied,
                "maturity_jump",
                "feature maturity transitions must advance one stage at a time",
                vec![root_pin.clone()],
                PolicyBasis::Declared,
            ),
        )?;
    }
    if record.metadata.archived
        || crate::retirement::read_tombstone(
            root,
            snapshot,
            config,
            &crate::RetirementTarget::new(crate::RetirementKind::Feature, id.as_str())?,
        )?
        .is_some()
    {
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Graph,
                None,
                ConditionState::Unknown,
                "feature_retired",
                "archived or retired features cannot advance maturity",
                vec![root_pin.clone()],
                PolicyBasis::Declared,
            ),
        )?;
    }
    if requested_rank >= 1 {
        let state = if record.metadata.decision == crate::FeatureDecision::Accepted {
            ConditionState::Satisfied
        } else {
            ConditionState::Unsatisfied
        };
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Criterion,
                None,
                state,
                if state == ConditionState::Satisfied {
                    "feature_decision_accepted"
                } else {
                    "feature_decision_not_accepted"
                },
                if state == ConditionState::Satisfied {
                    "feature decision is accepted"
                } else {
                    "feature decision must be accepted before maturity can advance"
                },
                vec![root_pin.clone()],
                PolicyBasis::Declared,
            ),
        )?;
        let state = if record.metadata.criteria.is_empty() {
            ConditionState::Unsatisfied
        } else if acceptance.is_some() {
            ConditionState::Satisfied
        } else {
            ConditionState::Unknown
        };
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Criterion,
                None,
                state,
                if record.metadata.criteria.is_empty() {
                    "feature_criteria_missing"
                } else if acceptance.is_some() {
                    "manual_acceptance"
                } else {
                    "feature_criteria_declared"
                },
                if record.metadata.criteria.is_empty() {
                    "feature maturity requires at least one criterion"
                } else if acceptance.is_some() {
                    "feature criteria were accepted by the attributed actor"
                } else {
                    "feature criteria are declared without policy evidence"
                },
                vec![root_pin.clone()],
                acceptance.map_or(PolicyBasis::Declared, |_| PolicyBasis::ManualAcceptance),
            ),
        )?;
    }

    if requested == FeatureMaturity::Implemented {
        let captured = crate::queries::capture(root, snapshot)?;
        let issues = captured
            .issues()
            .iter()
            .filter(|issue| issue.metadata.features.contains(id))
            .collect::<Vec<_>>();
        if issues.is_empty() {
            push(
                &mut conditions,
                &mut source_pins,
                condition(
                    subject.clone(),
                    ConditionKind::Child,
                    None,
                    ConditionState::Unknown,
                    "feature_has_no_issues",
                    "implemented maturity requires at least one associated issue",
                    vec![],
                    PolicyBasis::Declared,
                ),
            )?;
        }
        for issue in issues {
            let status = config.workflow.state(&issue.metadata.status)?;
            let (state, code, message) = if status.category == WorkflowCategory::Completed {
                (
                    ConditionState::Satisfied,
                    "issue_completed",
                    format!("associated issue {} is complete", issue.metadata.id),
                )
            } else if status.category == WorkflowCategory::Canceled {
                (
                    ConditionState::Unsatisfied,
                    "issue_canceled",
                    format!("associated issue {} is canceled", issue.metadata.id),
                )
            } else {
                (
                    ConditionState::Unsatisfied,
                    "issue_incomplete",
                    format!("associated issue {} is not complete", issue.metadata.id),
                )
            };
            let basis = if state == ConditionState::Satisfied {
                issue_basis(issue)
            } else {
                PolicyBasis::Declared
            };
            push(
                &mut conditions,
                &mut source_pins,
                condition(
                    subject.clone(),
                    ConditionKind::Child,
                    Some(SubjectRef::Issue(issue.metadata.id.clone())),
                    state,
                    code,
                    message,
                    vec![pin(issue.path.clone(), issue.source.content.clone())],
                    basis,
                ),
            )?;
        }
        for prerequisite in &record.metadata.prerequisites {
            let prerequisite_record =
                crate::features::load_feature(snapshot, config, prerequisite)?;
            let state = if maturity_rank(prerequisite_record.metadata.maturity) >= requested_rank {
                ConditionState::Satisfied
            } else {
                ConditionState::Unsatisfied
            };
            push(
                &mut conditions,
                &mut source_pins,
                condition(
                    subject.clone(),
                    ConditionKind::Prerequisite,
                    Some(SubjectRef::Feature(prerequisite.clone())),
                    state,
                    if state == ConditionState::Satisfied {
                        "feature_prerequisite_ready"
                    } else {
                        "feature_prerequisite_incomplete"
                    },
                    if state == ConditionState::Satisfied {
                        format!("feature prerequisite {prerequisite} has sufficient maturity")
                    } else {
                        format!("feature prerequisite {prerequisite} needs maturity first")
                    },
                    vec![pin(
                        prerequisite_record.path,
                        prerequisite_record.source.content,
                    )],
                    PolicyBasis::Declared,
                ),
            )?;
        }
    }
    if !record.metadata.gates.is_empty() && requested_rank >= 1 {
        push(
            &mut conditions,
            &mut source_pins,
            condition(
                subject.clone(),
                ConditionKind::Gate,
                None,
                ConditionState::Unknown,
                "feature_gates_require_authenticated_evidence",
                "feature gates require authenticated evidence before maturity can advance",
                vec![],
                PolicyBasis::Declared,
            ),
        )?;
    }
    if let Some(acceptance) = acceptance {
        acceptance.validate()?;
        crate::organization::validate_actor(snapshot, &config.repository, &acceptance.actor)?;
    }
    let basis = acceptance.map_or(PolicyBasis::Declared, |_| PolicyBasis::ManualAcceptance);
    let allowed = !conditions.is_empty()
        && conditions
            .iter()
            .all(|condition| condition.state == ConditionState::Satisfied)
        && requested_rank >= current_rank;
    finish(
        &config.repository,
        subject,
        maturity_name(current),
        maturity_name(requested),
        basis,
        allowed,
        conditions,
        source_pins,
        acceptance.cloned(),
    )
}

impl Repository {
    /// Read-only policy assessment for a project or milestone exit.
    pub fn assess_planning_policy(&self, kind: PlanningKind, id: &str) -> Result<PolicyAssessment> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            evaluate_planning(self.root(), snapshot, &config, kind, id, None)
        })
    }

    /// Complete a project or milestone after the current source snapshot has
    /// satisfied its explicit child/criterion policy under an attributed manual
    /// acceptance.  The ordinary transaction engine supplies stale-source and
    /// idempotent replay guarantees.
    pub fn complete_planning(
        &self,
        kind: PlanningKind,
        id: &str,
        expected: &crate::SourceToken,
        acceptance: &PolicyAcceptance,
        request: &crate::RequestId,
    ) -> Result<MutationReceipt> {
        if !matches!(kind, PlanningKind::Project | PlanningKind::Milestone) {
            return Err(invalid(
                "only projects and milestones have completion policy",
            ));
        }
        self.mutate_planning(
            kind,
            id,
            Some(expected),
            &crate::PlanningMutation::Complete {
                acceptance: acceptance.clone(),
            },
            request,
        )
    }

    /// Read-only maturity assessment for a feature transition.
    pub fn assess_feature_maturity(
        &self,
        id: &str,
        requested: FeatureMaturity,
    ) -> Result<PolicyAssessment> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let id = id.parse::<FeatureId>()?;
            evaluate_feature(self.root(), snapshot, &config, &id, requested, None)
        })
    }

    pub fn promote_feature(
        &self,
        id: &str,
        expected: &crate::SourceToken,
        requested: FeatureMaturity,
        acceptance: &PolicyAcceptance,
        request: &crate::RequestId,
    ) -> Result<MutationReceipt> {
        let id = id.parse::<FeatureId>()?;
        self.mutate_feature(
            id.as_str(),
            Some(expected),
            &crate::FeatureMutation::Promote {
                maturity: requested,
                acceptance: acceptance.clone(),
            },
            request,
        )
    }
}

pub(crate) fn validate_planning_transition(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &crate::Config,
    kind: PlanningKind,
    id: &str,
    acceptance: &PolicyAcceptance,
) -> Result<()> {
    let assessment = evaluate_planning(root, snapshot, config, kind, id, Some(acceptance))?;
    if !assessment.allowed {
        return Err(blocked(
            assessment
                .conditions
                .iter()
                .filter(|condition| condition.state != ConditionState::Satisfied)
                .map(|condition| condition.message.clone())
                .collect::<Vec<_>>()
                .join("; "),
        )
        .details(json!({"assessment":assessment})));
    }
    Ok(())
}

pub(crate) fn validate_feature_transition(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &crate::Config,
    before: &crate::FeatureRecord,
    after: &crate::FeatureMetadata,
    acceptance: &PolicyAcceptance,
) -> Result<()> {
    let assessment = evaluate_feature(
        root,
        snapshot,
        config,
        &before.metadata.id,
        after.maturity,
        Some(acceptance),
    )?;
    if assessment.current != maturity_name(before.metadata.maturity)
        || assessment.requested != maturity_name(after.maturity)
    {
        return Err(stale("feature maturity changed while policy was evaluated"));
    }
    if !assessment.allowed {
        return Err(blocked(
            assessment
                .conditions
                .iter()
                .filter(|condition| condition.state != ConditionState::Satisfied)
                .map(|condition| condition.message.clone())
                .collect::<Vec<_>>()
                .join("; "),
        )
        .details(json!({"assessment":assessment})));
    }
    Ok(())
}
