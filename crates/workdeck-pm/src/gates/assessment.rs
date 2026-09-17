use super::*;
use crate::{
    transactions::{Snapshot, canonical_hash},
    *,
};
use serde_json::json;
use std::{collections::BTreeMap, path::Path};
fn condition(
    gate: &GateId,
    requirement: Option<&GateRequirement>,
    state: ConditionState,
    code: &str,
    message: impl Into<String>,
    pins: Vec<SourcePin>,
) -> CompletionCondition {
    CompletionCondition {
        kind: if requirement.is_some() {
            ConditionKind::Criterion
        } else {
            ConditionKind::Gate
        },
        subject: SubjectRef::Gate(gate.clone()),
        related_subject: requirement.map(|r| r.criterion.subject()),
        state,
        reason_code: code.into(),
        message: message.into(),
        path: requirement
            .map(|r| {
                vec![
                    SubjectRef::Gate(gate.clone()),
                    r.criterion.owner.subject(),
                    r.criterion.subject(),
                ]
            })
            .unwrap_or_else(|| vec![SubjectRef::Gate(gate.clone())]),
        source_pins: pins,
        basis: "declared".into(),
    }
}
fn state(conditions: impl Iterator<Item = ConditionState>) -> ConditionState {
    let mut unknown = false;
    for state in conditions {
        match state {
            ConditionState::Unsatisfied => return ConditionState::Unsatisfied,
            ConditionState::Unknown => unknown = true,
            ConditionState::Satisfied => (),
        }
    }
    if unknown {
        ConditionState::Unknown
    } else {
        ConditionState::Satisfied
    }
}
fn criterion_conditions(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    gate: &GateId,
    r: &GateRequirement,
) -> Vec<CompletionCondition> {
    match criteria::resolve(root, snapshot, config, &r.criterion.owner, &r.criterion.id) {
        Err(e) => vec![condition(
            gate,
            Some(r),
            ConditionState::Unknown,
            "criterion_unresolved",
            e.message,
            vec![],
        )],
        Ok(resolved) => {
            let pins = vec![resolved.source];
            let mut conditions = vec![];
            if resolved.retired {
                conditions.push(condition(
                    gate,
                    Some(r),
                    ConditionState::Unknown,
                    "criterion_retired",
                    "criterion owner is archived or retired",
                    pins.clone(),
                ));
            }
            if resolved.reference != r.criterion {
                conditions.push(condition(
                    gate,
                    Some(r),
                    ConditionState::Unknown,
                    "criterion_definition_changed",
                    "criterion definition differs from the gate pin",
                    pins.clone(),
                ));
            }
            if resolved.declaration == CriterionDeclaration::Unchecked {
                conditions.push(condition(
                    gate,
                    Some(r),
                    ConditionState::Unsatisfied,
                    "criterion_unchecked",
                    "issue criterion is declared unchecked",
                    pins.clone(),
                ));
            }
            if conditions.is_empty() {
                conditions.push(condition(
                    gate,
                    Some(r),
                    ConditionState::Unknown,
                    "criterion_declared",
                    "criterion declaration alone is not execution evidence",
                    pins,
                ));
            }
            conditions
        }
    }
}
impl Repository {
    pub fn assess_gate(&self, input: &GateAssessmentRequest) -> Result<GateAssessment> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            assess(self.root(), snapshot, &config, input)
        })
    }
}
pub(crate) fn assess(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    input: &GateAssessmentRequest,
) -> Result<GateAssessment> {
    let record = load_gate(snapshot, config, &input.gate)?;
    let records = crate::evidence::store::load_evidence(snapshot, config)?;
    let active = crate::evidence::store::active_at(&records, input.as_of);
    let mut conditions = vec![];
    let gate_pin = SourcePin {
        path: record.path.clone(),
        content: record.source.content.clone(),
    };
    let mut all_pins = BTreeMap::from([(gate_pin.path.clone(), gate_pin.content.clone())]);
    // Membership and all evidence bytes are part of the assessment fingerprint,
    // including supersession after as_of, so the result describes one capture.
    for evidence in &records {
        all_pins.insert(evidence.path.clone(), evidence.content.clone());
    }
    if input.subject.repository != config.repository {
        conditions.push(condition(
            &input.gate,
            None,
            ConditionState::Unknown,
            "subject_repository_mismatch",
            "exact subject belongs to another repository",
            vec![],
        ));
    }
    let retired = crate::retirement::read_tombstone(
        root,
        snapshot,
        config,
        &RetirementTarget::new(RetirementKind::Gate, input.gate.as_str())?,
    )?
    .is_some();
    if retired || record.definition.archived {
        conditions.push(condition(
            &input.gate,
            None,
            ConditionState::Unknown,
            "gate_retired",
            "gate is archived or retired",
            vec![gate_pin.clone()],
        ));
    }
    let mut requirements = Vec::new();
    for r in &record.definition.requirements {
        let mut reasons = criterion_conditions(root, snapshot, config, &input.gate, r);
        let candidates = active
            .iter()
            .copied()
            .filter(|e| {
                e.reference.declaration.criterion.owner == r.criterion.owner
                    && e.reference.declaration.criterion.id == r.criterion.id
            })
            .collect::<Vec<_>>();
        let mut ids = vec![];
        if candidates.is_empty() {
            reasons.push(condition(
                &input.gate,
                Some(r),
                ConditionState::Unknown,
                "evidence_missing",
                "no active evidence reference existed at as_of",
                vec![],
            ));
        }
        for evidence in candidates {
            let declaration = &evidence.reference.declaration;
            ids.push(evidence.reference.id.clone());
            let pins = vec![SourcePin {
                path: evidence.path.clone(),
                content: evidence.content.clone(),
            }];
            let mismatch = if declaration.criterion != r.criterion {
                Some((
                    "evidence_definition_mismatch",
                    "evidence cites another criterion definition",
                ))
            } else if declaration.subject != input.subject {
                Some((
                    "evidence_subject_mismatch",
                    "evidence cites another exact source or artifact",
                ))
            } else if declaration.producer != r.producer {
                Some((
                    "evidence_producer_mismatch",
                    "producer identity or definition differs",
                ))
            } else if declaration.check != r.check {
                Some((
                    "evidence_check_mismatch",
                    "check identity or definition differs",
                ))
            } else if declaration.observed_at > input.as_of {
                Some((
                    "evidence_not_observed",
                    "evidence observation follows as_of",
                ))
            } else if declaration
                .expires_at
                .is_some_and(|time| time <= input.as_of)
                || r.max_age_seconds.is_some_and(|age| {
                    input
                        .as_of
                        .signed_duration_since(declaration.observed_at)
                        .num_seconds()
                        > age as i64
                })
            {
                Some((
                    "evidence_stale",
                    "evidence is expired or exceeds the allowed age",
                ))
            } else {
                None
            };
            if let Some((code, message)) = mismatch {
                reasons.push(condition(
                    &input.gate,
                    Some(r),
                    ConditionState::Unknown,
                    code,
                    message,
                    pins,
                ));
            } else {
                reasons.push(condition(
                    &input.gate,
                    Some(r),
                    ConditionState::Unknown,
                    "producer_unadmitted",
                    "no supported evaluator has admitted this producer result",
                    pins.clone(),
                ));
                reasons.push(condition(
                    &input.gate,
                    Some(r),
                    ConditionState::Unknown,
                    "evidence_declared_only",
                    "this retained reference is a declaration, not a verified result",
                    pins,
                ));
            }
        }
        for reason in &reasons {
            for pin in &reason.source_pins {
                all_pins.insert(pin.path.clone(), pin.content.clone());
            }
        }
        requirements.push(GateRequirementAssessment {
            requirement: r.id.clone(),
            state: state(reasons.iter().map(|c| c.state)),
            conditions: reasons,
            evidence: ids,
        });
    }
    if let Some(bytes) = snapshot.read_bounded(
        Path::new("config.yml"),
        crate::documents::MAX_DOCUMENT_BYTES,
    )? {
        all_pins.insert("config.yml".into(), ContentHash::of(&bytes));
    }
    let source_pins = all_pins
        .into_iter()
        .map(|(path, content)| SourcePin { path, content })
        .collect::<Vec<_>>();
    let state = state(
        conditions
            .iter()
            .map(|c| c.state)
            .chain(requirements.iter().map(|r| r.state)),
    );
    let fingerprint = canonical_hash(
        &json!({"request":input,"conditions":conditions,"requirements":requirements,"source_pins":source_pins}),
    )?;
    Ok(GateAssessment {
        gate: input.gate.clone(),
        subject: input.subject.clone(),
        subject_origin: "caller_declared".into(),
        as_of: input.as_of,
        state,
        requirements,
        conditions,
        source_pins,
        fingerprint,
    })
}
pub(crate) fn issue_conditions(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueRecord,
) -> Result<Vec<CompletionCondition>> {
    let mut conditions = vec![];
    for id in &issue.metadata.gates {
        match load_gate(snapshot, config, id) {
            Err(e) => conditions.push(condition(
                id,
                None,
                ConditionState::Unknown,
                "gate_unresolved",
                e.message,
                vec![],
            )),
            Ok(gate) => {
                let pin = SourcePin {
                    path: gate.path,
                    content: gate.source.content,
                };
                let retired = crate::retirement::read_tombstone(
                    root,
                    snapshot,
                    config,
                    &RetirementTarget::new(RetirementKind::Gate, id.as_str())?,
                )?
                .is_some();
                if retired || gate.definition.archived {
                    conditions.push(condition(
                        id,
                        None,
                        ConditionState::Unknown,
                        "gate_retired",
                        "gate is archived or retired",
                        vec![pin.clone()],
                    ));
                }
                for r in &gate.definition.requirements {
                    conditions.extend(criterion_conditions(root, snapshot, config, id, r));
                }
                conditions.push(condition(id,None,ConditionState::Unknown,"subject_required","completion requires an admitted exact source or artifact subject; none is available",vec![pin]));
            }
        }
    }
    Ok(conditions)
}
pub(crate) fn validate_associations(
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: &[GateId],
    next: &[GateId],
) -> Result<()> {
    for id in next.iter().filter(|id| !old.contains(id)) {
        let record = load_gate(snapshot, config, id)?;
        let retired = crate::retirement::read_tombstone(
            Path::new(""),
            snapshot,
            config,
            &RetirementTarget::new(RetirementKind::Gate, id.as_str())?,
        )?
        .is_some();
        if record.definition.archived || retired {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "new gate associations require active nonretired gates",
            ));
        }
    }
    Ok(())
}
pub(crate) fn validate_issue_associations(
    _root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: Option<&IssueMetadata>,
    next: &IssueMetadata,
) -> Result<()> {
    validate_associations(
        snapshot,
        config,
        old.map(|o| o.gates.as_slice()).unwrap_or_default(),
        &next.gates,
    )
}
pub(crate) fn retirement_blockers(
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<Vec<RecordReferenceBlocker>> {
    let mut blockers = vec![];
    for gate in load_gates(snapshot, config)? {
        for requirement in &gate.definition.requirements {
            let matches = match &requirement.criterion.owner {
                CriterionOwner::Issue(id) => {
                    target.kind == RetirementKind::Issue && target.id == id.as_str()
                }
                CriterionOwner::Feature(id) => {
                    target.kind == RetirementKind::Feature && target.id == id.as_str()
                }
                CriterionOwner::Milestone(id) => {
                    target.kind == RetirementKind::Milestone && target.id == *id
                }
                CriterionOwner::Project(id) => {
                    target.kind == RetirementKind::Project && target.id == *id
                }
            };
            if matches {
                blockers.push(RecordReferenceBlocker {
                    kind: RetirementKind::Gate,
                    id: gate.definition.id.to_string(),
                    path: gate.path.clone(),
                    field: format!("requirements.{}.criterion", requirement.id),
                    source: gate.source.clone(),
                });
            }
        }
    }
    Ok(blockers)
}
pub(crate) fn diagnostics(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<Vec<String>> {
    let mut warnings = vec![];
    for gate in load_gates(snapshot, config)? {
        for requirement in &gate.definition.requirements {
            match criteria::resolve(root,snapshot,config,&requirement.criterion.owner,&requirement.criterion.id){
    Ok(criterion) if criterion.reference!=requirement.criterion || criterion.retired=>warnings.push(format!("{} requirement {} retains a stale or retired criterion definition; assessment remains unqualified",gate.path.display(),requirement.id)),
    Err(e)=>warnings.push(format!("{} requirement {} cannot resolve its criterion: {}",gate.path.display(),requirement.id,e.message)),
    _=>()
   }
        }
    }
    Ok(warnings)
}
