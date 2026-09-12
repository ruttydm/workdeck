use super::*;
use crate::transactions::{Snapshot, canonical_hash};
use serde_json::json;
use std::path::Path;
const MAX_CONDITIONS: usize = 128;
const MAX_ACTIONS: usize = 32;

pub(super) fn actions(
    captured: &capture::Capture,
    issue: &IssueRecord,
    anchor: ContextAnchor,
    questions: &[QuestionApplicability],
) -> Result<NextActions> {
    let readiness = captured.graph.readiness(&issue.metadata.id)?;
    let mut actions = Vec::new();
    let preconditions = ActionPreconditions {
        repository: anchor.repository.clone(),
        issue: anchor.issue.clone(),
        source: issue.source.clone(),
        target_source: issue.source.clone(),
        graph: captured.graph.fingerprint().clone(),
        requirements: anchor.requirements.clone(),
        context: anchor.fingerprint.clone(),
    };
    let action = |kind, available, code: &str, explanation: &str| SuggestedAction {
        kind,
        available,
        reason_code: code.into(),
        explanation: explanation.into(),
        target: ContextTarget::Issue {
            id: issue.metadata.id.clone(),
        },
        preconditions: preconditions.clone(),
    };
    let category = captured
        .config
        .workflow
        .state(&issue.metadata.status)?
        .category;
    let inactive = issue.retirement.is_some()
        || issue.metadata.archived
        || matches!(
            category,
            WorkflowCategory::Completed | WorkflowCategory::Canceled
        );
    if !inactive {
        for question in questions.iter().filter(|q| q.blocks_implementation) {
            let mut suggested = action(
                NextActionKind::ResolveQuestion,
                true,
                if question.state == QuestionState::Answered {
                    "stale_decision"
                } else {
                    "open_question"
                },
                "Resolve or supersede the question using its current source; answering never changes issue requirements.",
            );
            suggested.target = ContextTarget::Question {
                id: question.question_id.to_string(),
            };
            suggested.preconditions.target_source = question.source.clone();
            actions.push(suggested);
        }
        if !readiness.ready {
            actions.push(action(NextActionKind::ResolvePrerequisite,true,"unresolved_prerequisite","Inspect hard prerequisite diagnostics; resolution or an authorized waiver must be explicit."));
        }
        let can_work = readiness.ready && !questions.iter().any(|q| q.blocks_implementation);
        match category {
            WorkflowCategory::Triage|WorkflowCategory::Backlog=>actions.push(action(NextActionKind::ClarifyRequirements,true,"requirements_refinement","Clarify scope and authored requirements before selecting implementation work.")),
            WorkflowCategory::Unstarted if can_work=>actions.push(action(NextActionKind::Implement,true,"ready_for_implementation","This issue is active, unstarted, and has no unresolved hard prerequisites or blocking questions.")),
            WorkflowCategory::Started if can_work=>actions.push(action(NextActionKind::ContinueImplementation,true,"implementation_in_progress","Continue the declared implementation; this advisory action grants no ownership or permission.")),
            WorkflowCategory::Review=>actions.push(action(NextActionKind::RequestReview,true,"review_declared","Inspect changes and obtain an explicit review; workflow state is not review evidence.")),
            WorkflowCategory::Verification=>actions.push(action(NextActionKind::InspectCompletion,true,"verification_requirements","Inspect current completion requirements; no execution result is inferred.")),
            _=>(),
        }
        actions.push(if cfg!(unix) {
            action(NextActionKind::RunChecks,true,"inspect_check_plan","Inspect repository check definitions and a source-bound plan. Execution requires a separate explicit foreground run; local feedback does not verify external evidence or complete an issue.")
        } else {
            action(NextActionKind::RunChecks,false,"check_execution_unavailable","Foreground process-group execution is not qualified on this platform.")
        });
        actions.push(action(NextActionKind::RecordHandoff,true,"continuity_available","Record attempted work, uncertainties and pending operations against this context anchor."));
    }
    let omitted_actions = actions.len().saturating_sub(MAX_ACTIONS);
    actions.truncate(MAX_ACTIONS);
    let (conditions, omitted_conditions) = bounded_conditions(readiness.conditions, 16 * 1024)?;
    Ok(NextActions {
        anchor,
        graph: captured.graph.fingerprint().clone(),
        actions,
        conditions,
        omitted_conditions,
        omitted_actions,
    })
}

pub(super) fn selection(
    root: &Path,
    snapshot: &Snapshot<'_>,
    captured: &capture::Capture,
    request: &NextIssueRequest,
) -> Result<NextIssueSelection> {
    if !(1..=100).contains(&request.limit) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "next issue limit must be between 1 and 100",
        ));
    }
    if !request.query.sort.is_empty() {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "next issue uses fixed priority/creation/ID ranking; query.sort must be empty",
        ));
    }
    if request.query.archive != ArchiveFilter::Active {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "next issue selection requires active archive scope",
        ));
    }
    let mut query = request.query.clone();
    query.sort = vec![
        IssueSort {
            field: IssueSortField::Priority,
            direction: SortDirection::Descending,
        },
        IssueSort {
            field: IssueSortField::CreatedAt,
            direction: SortDirection::Ascending,
        },
        IssueSort {
            field: IssueSortField::Id,
            direction: SortDirection::Ascending,
        },
    ];
    let indices = captured.query.select_indices(&query)?;
    let mut candidates = Vec::new();
    let mut question_inputs = Vec::new();
    for index in indices {
        let issue = captured.resolve(captured.query.issues()[index].metadata.id.as_str())?;
        let mut reasons = Vec::new();
        if issue.metadata.archived {
            reasons.push("issue_archived".into());
        }
        if issue.retirement.is_some() {
            reasons.push("issue_retired".into());
        }
        if captured
            .config
            .workflow
            .state(&issue.metadata.status)?
            .category
            != WorkflowCategory::Unstarted
        {
            reasons.push("workflow_not_unstarted".into());
        }
        let readiness = captured.graph.readiness(&issue.metadata.id)?;
        if !readiness.ready {
            reasons.push("prerequisite_or_terminal_state".into());
        }
        let questions = captured.applicability(root, snapshot, issue)?;
        if questions
            .iter()
            .any(|q| q.blocks_implementation && q.state == QuestionState::Open)
        {
            reasons.push("open_question".into());
        }
        if questions
            .iter()
            .any(|q| q.blocks_implementation && q.state == QuestionState::Answered)
        {
            reasons.push("stale_decision".into());
        }
        question_inputs.push((issue.metadata.id.clone(), questions));
        let (conditions, omitted_conditions) = bounded_conditions(readiness.conditions, 4 * 1024)?;
        let mut title = issue.metadata.title.clone();
        let title_truncated = title.len() > 256;
        if title_truncated {
            let mut end = 256;
            while !title.is_char_boundary(end) {
                end -= 1;
            }
            title.truncate(end);
        }
        candidates.push(NextIssueCandidate {
            issue: issue.metadata.id.clone(),
            title,
            title_truncated,
            source: issue.source.clone(),
            eligible: reasons.is_empty(),
            reason_codes: reasons,
            conditions,
            omitted_conditions,
        });
    }
    let fingerprint = canonical_hash(
        &json!({"schema":1,"repository":captured.config.repository,"graph":captured.graph.fingerprint(),"query":query,"questions":question_inputs,"candidates":candidates}),
    )?;
    let offset = if let Some(cursor) = &request.cursor {
        if cursor.fingerprint != fingerprint {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "next issue cursor belongs to a different query/source capture",
            ));
        }
        if cursor.offset > candidates.len() {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "next issue cursor offset is out of bounds",
            ));
        }
        cursor.offset
    } else {
        0
    };
    let total = candidates.len();
    let eligible = candidates.iter().filter(|c| c.eligible).count();
    let selected = candidates.iter().find(|c| c.eligible).cloned();
    let end = offset.saturating_add(request.limit).min(total);
    let next_cursor = (end < total).then(|| NextIssueCursor {
        fingerprint: fingerprint.clone(),
        offset: end,
    });
    Ok(NextIssueSelection {
        repository: captured.config.repository.clone(),
        fingerprint,
        selected,
        candidates: candidates
            .into_iter()
            .skip(offset)
            .take(request.limit)
            .collect(),
        total,
        eligible,
        excluded: total - eligible,
        next_cursor,
    })
}

fn bounded_conditions(
    input: Vec<CompletionCondition>,
    budget: usize,
) -> Result<(Vec<CompletionCondition>, usize)> {
    let total = input.len();
    let mut remaining = budget;
    let mut output = Vec::new();
    for condition in input {
        if output.len() == MAX_CONDITIONS {
            break;
        }
        let size = serde_json::to_vec(&condition).map_err(serialization)?.len() + 1;
        if size <= remaining {
            remaining -= size;
            output.push(condition);
        }
    }
    let omitted = total - output.len();
    Ok((output, omitted))
}
