use super::*;
use crate::{
    documents::MarkdownDocument,
    repository::config_from_snapshot,
    transactions::{
        ChangedPath, FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot,
        canonical_hash,
    },
    *,
};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
const MAX_TOTAL: usize = 32 * 1024 * 1024;
pub(crate) fn load_questions(
    _root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<Vec<QuestionRecord>> {
    let mut records = Vec::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("questions"), MAX_QUESTIONS * 2)? {
        if records.len() >= MAX_QUESTIONS {
            return Err(invalid("question catalog exceeds 4096 records"));
        }
        let bytes = snapshot
            .read_bounded(&path, MAX_QUESTION_BYTES)?
            .ok_or_else(|| invalid("question disappeared").at(&path))?;
        total = total.saturating_add(bytes.len());
        if total > MAX_TOTAL {
            return Err(invalid("question catalog exceeds 32 MiB"));
        }
        records.push(validation::parse(&path, &bytes, &config.repository)?);
    }
    validate_chains(&records)?;
    records.sort_by(|a, b| a.metadata.id.cmp(&b.metadata.id));
    Ok(records)
}
pub(crate) fn load_question(
    _root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &QuestionId,
) -> Result<QuestionRecord> {
    let path = validation::path(id);
    let bytes = snapshot
        .read_bounded(&path, MAX_QUESTION_BYTES)?
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "question not found").at(&path))?;
    validation::parse(&path, &bytes, &config.repository)
}
fn validate_chains(records: &[QuestionRecord]) -> Result<()> {
    let by_id = records
        .iter()
        .map(|r| (&r.metadata.id, r))
        .collect::<BTreeMap<_, _>>();
    if by_id.len() != records.len() {
        return Err(invalid("duplicate question identity"));
    }
    let mut used = BTreeSet::new();
    for record in records {
        if let Some(s) = &record.metadata.supersession {
            let replacement = by_id
                .get(&s.replacement)
                .ok_or_else(|| invalid("replacement question missing"))?;
            if !used.insert(&s.replacement) {
                return Err(invalid("question supersession cannot fork"));
            }
            if s.superseded_at < replacement.metadata.created_at {
                return Err(invalid("question supersession predates replacement"));
            }
            let a = record
                .metadata
                .subjects
                .iter()
                .map(|s| &s.subject)
                .collect::<BTreeSet<_>>();
            let b = replacement
                .metadata
                .subjects
                .iter()
                .map(|s| &s.subject)
                .collect::<BTreeSet<_>>();
            if a != b {
                return Err(invalid(
                    "replacement question must retain the affected subjects",
                ));
            }
        }
        let mut visited = BTreeSet::new();
        let mut next = Some(record);
        while let Some(r) = next {
            if !visited.insert(&r.metadata.id) {
                return Err(invalid("question supersession cannot cycle"));
            }
            next = r
                .metadata
                .supersession
                .as_ref()
                .and_then(|s| by_id.get(&s.replacement).copied());
        }
    }
    Ok(())
}
fn render(
    previous: Option<&QuestionRecord>,
    metadata: &QuestionMetadata,
    body: &str,
) -> Result<QuestionRecord> {
    let path = validation::path(&metadata.id);
    let text = if let Some(old) = previous {
        let mut doc = MarkdownDocument::parse(&path, &old.document)?;
        let next = serde_yaml_ng::to_value(metadata).map_err(|e| invalid(e.to_string()))?;
        let next = next.as_mapping().expect("metadata mapping");
        let mut patch = serde_yaml_ng::Mapping::new();
        for key in doc.metadata().keys().chain(next.keys()) {
            patch.insert(
                key.clone(),
                next.get(key).cloned().unwrap_or(serde_yaml_ng::Value::Null),
            );
        }
        doc.patch(&patch)?;
        doc.render()
    } else {
        format!(
            "---\n{}---\n{}",
            serde_yaml_ng::to_string(metadata).map_err(|e| invalid(e.to_string()))?,
            body
        )
    };
    validation::parse(&path, text.as_bytes(), &metadata.repository)
}
fn apply(
    old: &QuestionMetadata,
    mutation: &QuestionMutation,
    now: Timestamp,
) -> Result<QuestionMetadata> {
    let mut next = old.clone();
    if old.state == QuestionState::Superseded {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "question is already superseded",
        ));
    }
    match mutation {
        QuestionMutation::Answer {
            actor,
            body,
            decision_refs,
        } => {
            if old.state != QuestionState::Open {
                return Err(PmError::new(
                    ErrorCode::PolicyBlocked,
                    "answer already recorded; create and supersede with a new question",
                ));
            }
            next.state = QuestionState::Answered;
            next.answer = Some(RecordedAnswer {
                actor: actor.clone(),
                body: body.clone(),
                answered_at: now,
                decision_refs: decision_refs.clone(),
            });
        }
        QuestionMutation::Supersede {
            actor,
            reason,
            replacement,
            replacement_source,
        } => {
            next.state = QuestionState::Superseded;
            next.supersession = Some(QuestionSupersession {
                actor: actor.clone(),
                reason: reason.clone(),
                replacement: replacement.clone(),
                replacement_source: replacement_source.clone(),
                superseded_at: now,
            });
        }
    }
    next.revision = old.revision.next()?;
    next.updated_at = now;
    Ok(next)
}
fn actor(mutation: &QuestionMutation) -> &str {
    match mutation {
        QuestionMutation::Answer { actor, .. } | QuestionMutation::Supersede { actor, .. } => actor,
    }
}
fn live_mutation(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    old: &QuestionRecord,
    mutation: &QuestionMutation,
    records: &[QuestionRecord],
) -> Result<()> {
    crate::organization::validate_actor(snapshot, &config.repository, actor(mutation))?;
    match mutation {
        QuestionMutation::Answer { decision_refs, .. } => {
            let status =
                applicability(root, snapshot, config, std::slice::from_ref(old), &[])?.remove(0);
            if !status.answer.allowed {
                return Err(PmError::new(
                    ErrorCode::PolicyBlocked,
                    "question cannot be answered against its stale or closed basis",
                )
                .details(json!(status)));
            }
            validation::pins(decision_refs)?;
            let mut remaining = 32 * 1024 * 1024usize;
            for pin in decision_refs {
                let bytes = snapshot
                    .read_bounded(
                        &pin.path,
                        crate::documents::MAX_DOCUMENT_BYTES.min(remaining),
                    )?
                    .ok_or_else(|| {
                        PmError::new(ErrorCode::NotFound, "decision source missing").at(&pin.path)
                    })?;
                remaining = remaining.saturating_sub(bytes.len());
                if ContentHash::of(&bytes) != pin.content {
                    return Err(
                        PmError::new(ErrorCode::StaleSource, "decision source changed")
                            .at(&pin.path),
                    );
                }
            }
        }
        QuestionMutation::Supersede {
            replacement,
            replacement_source,
            ..
        } => {
            let target = records
                .iter()
                .find(|r| &r.metadata.id == replacement)
                .ok_or_else(|| {
                    PmError::new(ErrorCode::NotFound, "replacement question not found")
                })?;
            if target.source != *replacement_source {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "replacement question changed",
                ));
            }
            if target.metadata.state != QuestionState::Open {
                return Err(PmError::new(
                    ErrorCode::PolicyBlocked,
                    "replacement question must be open",
                ));
            }
            let status =
                applicability(root, snapshot, config, std::slice::from_ref(target), &[])?.remove(0);
            if status.freshness != QuestionFreshness::Current {
                return Err(PmError::new(
                    ErrorCode::StaleSource,
                    "replacement question has a stale basis",
                )
                .details(json!(status)));
            }
        }
    }
    Ok(())
}
impl Repository {
    pub fn questions(&self, query: &QuestionQuery) -> Result<Vec<QuestionRecord>> {
        if query.subjects.len() > 64 {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                "question queries support at most 64 subjects",
            ));
        }
        for s in &query.subjects {
            validation::validate_subject(s)?;
        }
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            Ok(load_questions(self.root(), snapshot, &config)?
                .into_iter()
                .filter(|q| {
                    query.state.is_none_or(|s| q.metadata.state == s)
                        && (query.subjects.is_empty()
                            || q.metadata
                                .subjects
                                .iter()
                                .any(|s| query.subjects.contains(&s.subject)))
                })
                .collect())
        })
    }
    pub fn question(&self, id: &QuestionId) -> Result<QuestionRecord> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            load_questions(self.root(), snapshot, &config)?
                .into_iter()
                .find(|r| &r.metadata.id == id)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "question not found"))
        })
    }
    pub fn question_applicability(&self, id: &QuestionId) -> Result<QuestionApplicability> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let record = load_questions(self.root(), snapshot, &config)?
                .into_iter()
                .find(|r| &r.metadata.id == id)
                .ok_or_else(|| PmError::new(ErrorCode::NotFound, "question not found"))?;
            Ok(applicability(self.root(), snapshot, &config, &[record], &[])?.remove(0))
        })
    }
    pub fn create_question(
        &self,
        input: &CreateQuestion,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.write_question(
            &QuestionRequest::Create {
                input: input.clone(),
            },
            request,
            |_| Ok(()),
        )
    }
    pub fn mutate_question(
        &self,
        id: &QuestionId,
        expected: &SourceToken,
        mutation: &QuestionMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_question_with_faults(id, expected, mutation, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn mutate_question_with_faults(
        &self,
        id: &QuestionId,
        expected: &SourceToken,
        mutation: &QuestionMutation,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.write_question(
            &QuestionRequest::Mutate {
                id: id.clone(),
                expected: expected.clone(),
                mutation: mutation.clone(),
            },
            request,
            fault,
        )
    }
    fn write_question(
        &self,
        input: &QuestionRequest,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let operation = match input {
            QuestionRequest::Create { .. } => "question.create",
            QuestionRequest::Mutate {
                mutation: QuestionMutation::Answer { .. },
                ..
            } => "question.answer",
            QuestionRequest::Mutate { .. } => "question.supersede",
        };
        let receipt = self.store()?.transact_with_faults(
            request,
            operation,
            &json!(input),
            |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                let mut records = load_questions(self.root(), snapshot, &config)?;
                let (record, previous) = match input {
                    QuestionRequest::Create { input } => {
                        validation::live_input(self.root(), snapshot, &config, input)?;
                        if records.len() >= MAX_QUESTIONS {
                            return Err(invalid("question catalog exceeds 4096 records"));
                        }
                        let now = chrono::Utc::now();
                        let metadata = QuestionMetadata {
                            schema: SchemaVersion::CURRENT,
                            repository: config.repository.clone(),
                            id: QuestionId::new(),
                            revision: Revision::INITIAL,
                            actor: input.actor.clone(),
                            created_at: now,
                            updated_at: now,
                            subjects: input.subjects.clone(),
                            requirements: input.requirements.clone(),
                            blocks_work: input.blocks_work,
                            state: QuestionState::Open,
                            answer: None,
                            supersession: None,
                            custom: input.custom.clone(),
                            extra: input.extra.clone(),
                        };
                        (render(None, &metadata, &input.body)?, None)
                    }
                    QuestionRequest::Mutate {
                        id,
                        expected,
                        mutation,
                    } => {
                        let old = records
                            .iter()
                            .find(|r| &r.metadata.id == id)
                            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "question not found"))?
                            .clone();
                        if old.source != *expected {
                            return Err(PmError::new(
                                ErrorCode::StaleSource,
                                "question source changed",
                            ));
                        }
                        live_mutation(self.root(), snapshot, &config, &old, mutation, &records)?;
                        let next = apply(
                            &old.metadata,
                            mutation,
                            chrono::Utc::now().max(old.metadata.updated_at),
                        )?;
                        let next = render(Some(&old), &next, &old.body)?;
                        (next, Some(old))
                    }
                };
                records.retain(|r| r.metadata.id != record.metadata.id);
                records.push(record.clone());
                validate_chains(&records)?;
                if records.iter().map(|r| r.document.len()).sum::<usize>() > MAX_TOTAL {
                    return Err(invalid("question catalog exceeds 32 MiB"));
                }
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: record.path.clone(),
                        expected: previous.as_ref().map(|r| r.source.content.clone()),
                        content: Some(record.document.as_bytes().to_vec()),
                    }],
                    result: json!(QuestionMutationResult {
                        question: record,
                        input: input.clone(),
                        previous
                    }),
                })
            },
            fault,
        )?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if !receipt.operation.starts_with("question.") {
        return Ok(());
    }
    let result: QuestionMutationResult =
        serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
    let record = &result.question;
    if validation::parse(
        &record.path,
        record.document.as_bytes(),
        &record.metadata.repository,
    )? != *record
        || receipt.repository.as_ref() != Some(&record.metadata.repository)
        || canonical_hash(&json!(result.input))? != receipt.input_hash
    {
        return Err(invalid(
            "question receipt differs from exact source or request intent",
        ));
    }
    let (operation, before) = match &result.input {
        QuestionRequest::Create { input } => {
            if result.previous.is_some()
                || validation::input(record) != *input
                || record.metadata.state != QuestionState::Open
                || record.metadata.revision != Revision::INITIAL
                || record.metadata.updated_at != record.metadata.created_at
            {
                return Err(invalid(
                    "question creation proof differs from original intent",
                ));
            }
            ("question.create", None)
        }
        QuestionRequest::Mutate {
            id,
            expected,
            mutation,
        } => {
            let old = result
                .previous
                .as_ref()
                .ok_or_else(|| invalid("question mutation proof lacks previous document"))?;
            if validation::parse(
                &old.path,
                old.document.as_bytes(),
                &record.metadata.repository,
            )? != *old
                || old.source != *expected
                || &old.metadata.id != id
                || record.metadata != apply(&old.metadata, mutation, record.metadata.updated_at)?
                || old.body != record.body
                || old.path != record.path
            {
                return Err(invalid(
                    "question mutation proof differs from previous source or action",
                ));
            }
            (
                match mutation {
                    QuestionMutation::Answer { .. } => "question.answer",
                    QuestionMutation::Supersede { .. } => "question.supersede",
                },
                Some(old.source.content.clone()),
            )
        }
    };
    if receipt.operation != operation
        || receipt.changed
            != vec![ChangedPath {
                path: record.path.clone(),
                before,
                after: Some(record.source.content.clone()),
            }]
    {
        return Err(invalid(
            "question receipt changed paths differ from exact history",
        ));
    }
    Ok(())
}
pub(crate) fn inspect(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<PmError>, Vec<PmError>) {
    let paths = match snapshot.list_bounded(Path::new("questions"), MAX_QUESTIONS * 2) {
        Ok(p) => p,
        Err(e) => return (0, vec![e], vec![]),
    };
    let count = paths.len();
    let mut errors = Vec::new();
    let mut records = Vec::new();
    let mut total = 0usize;
    for path in paths {
        match snapshot
            .read_bounded(&path, MAX_QUESTION_BYTES)
            .and_then(|b| b.ok_or_else(|| invalid("question disappeared")))
            .and_then(|b| {
                total = total.saturating_add(b.len());
                validation::parse(&path, &b, &config.repository)
            }) {
            Ok(r) => records.push(r),
            Err(e) => errors.push(e.at(&path)),
        }
        if total > MAX_TOTAL || records.len() > MAX_QUESTIONS {
            errors.push(invalid("question catalog exceeds bounds"));
            break;
        }
    }
    if let Err(e) = validate_chains(&records) {
        errors.push(e);
    }
    let mut warnings = Vec::new();
    match applicability(root, snapshot, config, &records, &[]) {
        Ok(states) => {
            for state in states {
                for reason in state.stale_reasons {
                    warnings.push(
                        PmError::new(
                            ErrorCode::StaleSource,
                            format!("{}: {}", reason.code, reason.message),
                        )
                        .at(validation::path(&state.question_id)),
                    );
                }
            }
        }
        Err(e) => errors.push(e),
    }
    (count, errors, warnings)
}
pub(crate) fn retirement_blockers(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<Vec<QuestionReferenceBlocker>> {
    let subject = match target.kind {
        RetirementKind::Issue => SubjectRef::Issue(target.id.parse()?),
        RetirementKind::Feature => SubjectRef::Feature(target.id.parse()?),
        RetirementKind::Gate => SubjectRef::Gate(target.id.parse()?),
        RetirementKind::Milestone => SubjectRef::Milestone(target.id.clone()),
        RetirementKind::Project => SubjectRef::Project(target.id.clone()),
        _ => return Ok(vec![]),
    };
    Ok(load_questions(root, snapshot, config)?
        .into_iter()
        .filter(|q| {
            q.metadata.state == QuestionState::Open
                && q.metadata.subjects.iter().any(|s| s.subject == subject)
        })
        .map(|q| QuestionReferenceBlocker {
            question: q.metadata.id,
            path: q.path,
            field: "subjects".into(),
            source: q.source,
        })
        .collect())
}
pub(crate) fn validate_import(
    root: &Path,
    current: &Snapshot<'_>,
    projected: &Snapshot<'_>,
    config: &Config,
    path: &Path,
) -> Result<()> {
    let id = validation::validate_path(path)?;
    let next = load_question(root, projected, config, &id)?;
    if let Some(bytes) = current.read_bounded(path, MAX_QUESTION_BYTES)? {
        let old = validation::parse(path, &bytes, &config.repository)?;
        let mutation = if let Some(s) = &next.metadata.supersession {
            QuestionMutation::Supersede {
                actor: s.actor.clone(),
                reason: s.reason.clone(),
                replacement: s.replacement.clone(),
                replacement_source: s.replacement_source.clone(),
            }
        } else if let Some(a) = &next.metadata.answer {
            QuestionMutation::Answer {
                actor: a.actor.clone(),
                body: a.body.clone(),
                decision_refs: a.decision_refs.clone(),
            }
        } else {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "question import cannot rewrite authored question content",
            ));
        };
        if next.metadata != apply(&old.metadata, &mutation, next.metadata.updated_at)?
            || next.body != old.body
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "question import must be one explicit answer or supersession preserving authored history",
            ));
        }
        live_mutation(
            root,
            projected,
            config,
            &old,
            &mutation,
            &load_questions(root, projected, config)?,
        )?;
    } else {
        validation::live_input(root, projected, config, &validation::input(&next))?;
        if let Some(a) = &next.metadata.answer {
            crate::organization::validate_actor(projected, &config.repository, &a.actor)?;
        }
        if let Some(s) = &next.metadata.supersession {
            crate::organization::validate_actor(projected, &config.repository, &s.actor)?;
        }
    }
    Ok(())
}
