use super::*;
use crate::{transactions::Snapshot, *};
use std::{collections::BTreeMap, path::Path};
fn reason(code: &str, message: impl Into<String>, subject: Option<SubjectRef>) -> QuestionReason {
    QuestionReason {
        code: code.into(),
        message: message.into(),
        subject,
    }
}
pub(crate) fn applicability(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    records: &[QuestionRecord],
    subjects: &[SubjectRef],
) -> Result<Vec<QuestionApplicability>> {
    for subject in subjects {
        validation::validate_subject(subject)?;
    }
    let mut output = Vec::new();
    let mut resolved_subjects = BTreeMap::new();
    let mut resolved_criteria = BTreeMap::new();
    for record in records {
        let m = &record.metadata;
        let matched = m
            .subjects
            .iter()
            .filter(|s| subjects.is_empty() || subjects.contains(&s.subject))
            .map(|s| s.subject.clone())
            .collect::<Vec<_>>();
        if matched.is_empty() {
            continue;
        }
        let mut stale = Vec::new();
        for bound in &m.subjects {
            let resolved = resolved_subjects
                .entry(bound.subject.clone())
                .or_insert_with(|| {
                    validation::subject_record(root, snapshot, config, &bound.subject)
                });
            match resolved.clone() {
                Ok((source, _, inactive)) => {
                    if source != bound.source {
                        stale.push(reason(
                            "subject_source_changed",
                            "the reviewed subject source changed",
                            Some(bound.subject.clone()),
                        ));
                    }
                    if inactive {
                        stale.push(reason(
                            "subject_inactive",
                            "the subject is archived or retired",
                            Some(bound.subject.clone()),
                        ));
                    }
                }
                Err(e) if matches!(e.code, ErrorCode::NotFound) => stale.push(reason(
                    "subject_missing",
                    e.message,
                    Some(bound.subject.clone()),
                )),
                Err(e) => return Err(e),
            }
        }
        for bound in &m.requirements {
            let resolved = resolved_criteria
                .entry((bound.owner.clone(), bound.id.clone()))
                .or_insert_with(|| {
                    crate::gates::resolve_criterion(root, snapshot, config, &bound.owner, &bound.id)
                });
            match resolved.clone() {
                Ok(current) => {
                    if current.reference != *bound || current.retired {
                        stale.push(reason(
                            "criterion_definition_changed",
                            "the pinned criterion is changed or inactive",
                            Some(bound.subject()),
                        ));
                    }
                }
                Err(e) if e.code == ErrorCode::NotFound => stale.push(reason(
                    "criterion_missing",
                    e.message,
                    Some(bound.subject()),
                )),
                Err(e) => return Err(e),
            }
        }
        let mut answer_reasons = stale.clone();
        if m.state != QuestionState::Open {
            answer_reasons.push(reason(
                "question_not_open",
                "only an open question can be answered",
                None,
            ));
        }
        let supersede_reasons = if m.state == QuestionState::Superseded {
            vec![reason(
                "question_superseded",
                "the question was already superseded",
                None,
            )]
        } else {
            Vec::new()
        };
        let freshness = if stale.is_empty() {
            QuestionFreshness::Current
        } else {
            QuestionFreshness::Stale
        };
        output.push(QuestionApplicability {
            question_id: m.id.clone(),
            source: record.source.clone(),
            state: m.state,
            matched_subjects: matched,
            freshness,
            blocks_implementation: m.blocks_work
                && (m.state == QuestionState::Open
                    || (m.state == QuestionState::Answered
                        && freshness == QuestionFreshness::Stale)),
            stale_reasons: stale,
            answer: QuestionActionState {
                allowed: answer_reasons.is_empty(),
                reasons: answer_reasons,
            },
            supersede: QuestionActionState {
                allowed: supersede_reasons.is_empty(),
                reasons: supersede_reasons,
            },
        });
    }
    Ok(output)
}
