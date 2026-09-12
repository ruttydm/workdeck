use super::*;
use crate::{documents::MarkdownDocument, transactions::Snapshot, *};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};
pub(crate) fn text(value: &str, label: &str, max: usize, multiline: bool) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > max
        || value
            .chars()
            .any(|c| c.is_control() && !(multiline && matches!(c, '\n' | '\r' | '\t')))
    {
        return Err(invalid(format!(
            "{label} is empty, oversized, or contains unsupported controls"
        )));
    }
    Ok(())
}
pub(crate) fn extensions(
    custom: &BTreeMap<String, Value>,
    extra: &BTreeMap<String, Value>,
) -> Result<()> {
    for key in custom.keys() {
        text(key, "custom key", 128, false)?;
    }
    for key in extra.keys() {
        if !key
            .strip_prefix("x-")
            .is_some_and(crate::identity::valid_slug)
        {
            return Err(invalid(format!(
                "unknown metadata {key:?}; use custom or x- extensions"
            )));
        }
    }
    Ok(())
}
pub(crate) fn validate_subject(subject: &SubjectRef) -> Result<()> {
    match subject {
        SubjectRef::Milestone(id) | SubjectRef::Project(id) => crate::planning::validate_id(id),
        SubjectRef::Criterion { .. } => Err(invalid(
            "question subjects must be concrete; use requirements for criteria",
        )),
        _ => Ok(()),
    }
}
pub(crate) fn pins(pins: &[SourcePin]) -> Result<()> {
    if pins.len() > 128 {
        return Err(invalid("at most 128 source references are supported"));
    }
    let mut paths = BTreeSet::new();
    for pin in pins {
        let path = pin
            .path
            .to_str()
            .ok_or_else(|| invalid("source reference must be UTF-8"))?;
        SourceLink {
            path: path.into(),
            line: None,
            end_line: None,
        }
        .validate()?;
        if !paths.insert(&pin.path) {
            return Err(invalid("duplicate source reference"));
        }
    }
    Ok(())
}
pub(crate) fn validate_input(input: &CreateQuestion, repository: &RepositoryId) -> Result<()> {
    text(&input.actor, "question actor", 256, false)?;
    text(&input.body, "question body", 64 * 1024, true)?;
    extensions(&input.custom, &input.extra)?;
    if input.subjects.is_empty() || input.subjects.len() > 64 || input.requirements.len() > 256 {
        return Err(invalid(
            "question requires 1..64 subjects and at most 256 criteria",
        ));
    }
    let mut subjects = BTreeSet::new();
    for s in &input.subjects {
        validate_subject(&s.subject)?;
        if !subjects.insert(&s.subject) {
            return Err(invalid("duplicate question subject"));
        }
    }
    let mut criteria = BTreeSet::new();
    for c in &input.requirements {
        c.validate()?;
        if &c.repository != repository
            || !subjects.contains(&c.owner.subject())
            || !criteria.insert((&c.owner, &c.id))
        {
            return Err(invalid(
                "question criteria require unique IDs, the same repository, and a bound owner subject",
            ));
        }
    }
    Ok(())
}
pub(crate) fn input(record: &QuestionRecord) -> CreateQuestion {
    let m = &record.metadata;
    CreateQuestion {
        actor: m.actor.clone(),
        body: record.body.clone(),
        subjects: m.subjects.clone(),
        requirements: m.requirements.clone(),
        blocks_work: m.blocks_work,
        custom: m.custom.clone(),
        extra: m.extra.clone(),
    }
}
pub(crate) fn validate_metadata(m: &QuestionMetadata, body: &str) -> Result<()> {
    validate_input(
        &CreateQuestion {
            actor: m.actor.clone(),
            body: body.into(),
            subjects: m.subjects.clone(),
            requirements: m.requirements.clone(),
            blocks_work: m.blocks_work,
            custom: m.custom.clone(),
            extra: m.extra.clone(),
        },
        &m.repository,
    )?;
    if m.updated_at < m.created_at {
        return Err(invalid("question timestamps are reversed"));
    }
    if (m.state == QuestionState::Open && (m.answer.is_some() || m.supersession.is_some()))
        || (m.state == QuestionState::Answered && (m.answer.is_none() || m.supersession.is_some()))
        || (m.state == QuestionState::Superseded && m.supersession.is_none())
    {
        return Err(invalid("question state and answer/supersession disagree"));
    }
    if let Some(a) = &m.answer {
        text(&a.actor, "answer actor", 256, false)?;
        text(&a.body, "answer", 64 * 1024, true)?;
        pins(&a.decision_refs)?;
        if a.answered_at < m.created_at || a.answered_at > m.updated_at {
            return Err(invalid("answer timestamp lies outside question history"));
        }
    }
    if let Some(s) = &m.supersession {
        text(&s.actor, "supersession actor", 256, false)?;
        text(&s.reason, "supersession reason", 4096, true)?;
        if s.replacement == m.id
            || s.superseded_at < m.created_at
            || s.superseded_at > m.updated_at
            || m.answer
                .as_ref()
                .is_some_and(|a| a.answered_at > s.superseded_at)
        {
            return Err(invalid(
                "invalid question supersession identity or timestamp",
            ));
        }
    }
    Ok(())
}
pub(crate) fn path(id: &QuestionId) -> PathBuf {
    Path::new("questions").join(format!("{id}.md"))
}
pub(crate) fn validate_path(value: &Path) -> Result<QuestionId> {
    let id: QuestionId = value
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("question path must be UTF-8"))?
        .parse()?;
    if path(&id) != value {
        return Err(invalid("question path must be questions/<Q-ID>.md").at(value));
    }
    Ok(id)
}
pub(crate) fn parse(
    path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<QuestionRecord> {
    let id = validate_path(path)?;
    if bytes.len() > MAX_QUESTION_BYTES {
        return Err(invalid("question exceeds 128 KiB").at(path));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("question must be UTF-8"))?;
    let doc = MarkdownDocument::parse(path, text)?;
    schema(doc.metadata(), path)?;
    let metadata: QuestionMetadata = doc.deserialize()?;
    validate_metadata(&metadata, doc.body())?;
    if metadata.id != id || &metadata.repository != repository {
        return Err(invalid("question identity or repository differs from path/source").at(path));
    }
    Ok(QuestionRecord {
        source: SourceToken::new(metadata.revision, bytes),
        metadata,
        body: doc.body().into(),
        path: path.into(),
        document: text.into(),
    })
}
pub(crate) fn schema(metadata: &serde_yaml_ng::Mapping, path: &Path) -> Result<()> {
    let schema = metadata
        .get(serde_yaml_ng::Value::String("schema".into()))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| invalid("schema must be an integer").at(path))?;
    SchemaVersion::try_from(schema).map_err(|e| e.at(path))?;
    Ok(())
}
pub(crate) fn subject_record(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    subject: &SubjectRef,
) -> Result<(SourceToken, PathBuf, bool)> {
    validate_subject(subject)?;
    let (source, path, archived, target) = match subject {
        SubjectRef::Issue(id) => {
            let r = crate::issues::resolve_issue(root, snapshot, config, id.as_str())?;
            (
                r.source,
                r.path,
                r.metadata.archived,
                RetirementTarget::new(RetirementKind::Issue, id.as_str())?,
            )
        }
        SubjectRef::Feature(id) => {
            let r = crate::features::load_feature(snapshot, config, id)?;
            (
                r.source,
                r.path,
                r.metadata.archived,
                RetirementTarget::new(RetirementKind::Feature, id.as_str())?,
            )
        }
        SubjectRef::Gate(id) => {
            let r = crate::gates::load_gate(snapshot, config, id)?;
            (
                r.source,
                r.path,
                r.definition.archived,
                RetirementTarget::new(RetirementKind::Gate, id.as_str())?,
            )
        }
        SubjectRef::Milestone(id) | SubjectRef::Project(id) => {
            let kind = if matches!(subject, SubjectRef::Project(_)) {
                PlanningKind::Project
            } else {
                PlanningKind::Milestone
            };
            let r = crate::planning::store::load_planning(root, snapshot, kind, id)?;
            (
                r.source,
                r.path,
                r.metadata.archived,
                RetirementTarget::new(kind.into(), id)?,
            )
        }
        SubjectRef::Criterion { .. } => unreachable!("validated concrete subject"),
    };
    let retired = crate::retirement::read_tombstone(root, snapshot, config, &target)?.is_some();
    Ok((source, path, archived || retired))
}
pub(crate) fn live_input(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    input: &CreateQuestion,
) -> Result<()> {
    validate_input(input, &config.repository)?;
    crate::organization::validate_actor(snapshot, &config.repository, &input.actor)?;
    for subject in &input.subjects {
        let (source, _, inactive) = subject_record(root, snapshot, config, &subject.subject)?;
        if inactive {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "new question requires active subjects",
            ));
        }
        if source != subject.source {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "question subject source changed",
            ));
        }
    }
    for pin in &input.requirements {
        let current = crate::gates::resolve_criterion(root, snapshot, config, &pin.owner, &pin.id)?;
        if current.retired || current.reference != *pin {
            return Err(PmError::new(
                ErrorCode::StaleSource,
                "question requirement definition changed",
            ));
        }
    }
    Ok(())
}
