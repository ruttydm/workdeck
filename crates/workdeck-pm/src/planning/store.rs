use super::*;
use crate::{
    IssueId, Repository, RequestId, SourceToken,
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument, YamlDocument},
    transactions::{FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use chrono::Utc;
use serde_json::json;
use serde_yaml_ng::{Mapping, Value as YamlValue};
use std::path::Path;

pub(crate) const PLANNING_WRITABLE_FIELDS: &[&str] = &[
    "name",
    "status",
    "starts_at",
    "ends_at",
    "color",
    "custom",
    "initiative",
    "project",
    "lead",
    "scope",
    "goal",
    "targets",
    "exit_criteria",
    "outcomes",
];

impl PlanningKind {
    fn directory(self) -> &'static str {
        match self {
            Self::Initiative => "initiatives",
            Self::Project => "projects",
            Self::Milestone => "milestones",
            Self::Cycle => "cycles",
            Self::Target => "targets",
            Self::Label => "labels",
        }
    }
    fn prefix(self) -> &'static str {
        match self {
            Self::Initiative => "INI",
            Self::Project => "PRJ",
            Self::Milestone => "MIL",
            Self::Cycle => "CYC",
            Self::Target => "TGT",
            Self::Label => "LBL",
        }
    }
}

impl Repository {
    pub fn list_planning(&self, kind: PlanningKind) -> Result<Vec<PlanningRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let mut records = list_planning(self.root(), snapshot, kind)?;
            for record in &mut records {
                record.retirement = crate::retirement::read_tombstone(
                    self.root(),
                    snapshot,
                    &config,
                    &crate::RetirementTarget::new(kind.into(), record.metadata.id.clone())?,
                )?;
            }
            Ok(records)
        })
    }

    pub fn planning_record(&self, kind: PlanningKind, id: &str) -> Result<PlanningRecord> {
        validate_id(id)?;
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let mut record = load_planning(self.root(), snapshot, kind, id)?;
            record.retirement = crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &crate::RetirementTarget::new(kind.into(), id)?,
            )?;
            Ok(record)
        })
    }

    pub fn create_planning(
        &self,
        kind: PlanningKind,
        input: &CreatePlanning,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        let parameters = json!({"kind":kind,"input":input});
        self.store()?.transact(request, "planning.create", &parameters, |snapshot| {
            let id = match &input.id { Some(id) => id.clone(), None => IssueId::new(kind.prefix())?.to_string() };
            validate_id(&id)?;
            let config=crate::repository::config_from_snapshot(self.root(),snapshot)?;
            crate::retirement::ensure_writable(self.root(),snapshot,&config,&crate::RetirementTarget::new(kind.into(),id.clone())?)?;
            let records = list_planning(self.root(), snapshot, kind)?;
            if records.iter().any(|record| record.metadata.id.eq_ignore_ascii_case(&id)) || identity_was_used(snapshot, kind, &id)? {
                return Err(PmError::new(ErrorCode::Conflict, "planning identity already exists or was previously used; archive and restore the existing record"));
            }
            if kind == PlanningKind::Label && !input.body.is_empty() { return Err(invalid("labels do not have Markdown bodies")); }
            let now = Utc::now();
            let mut metadata = PlanningMetadata {
                schema: SchemaVersion::CURRENT, id, revision: Revision::INITIAL, name: input.name.clone(),
                status: (kind != PlanningKind::Label).then(String::new), starts_at: None, ends_at: None,
                color: (kind == PlanningKind::Label).then(String::new), created_at: Some(now), updated_at: Some(now),
                initiative: None, project: None, lead: None, scope: None, goal: None, targets: Vec::new(), exit_criteria: Vec::new(), outcomes: Vec::new(),
                imported: None, archived: false, custom: BTreeMap::new(), extra: BTreeMap::new(),
            };
            metadata = apply_fields(metadata, kind, &input.fields)?;
            metadata.validate(kind)?;
            hierarchy::validate_planning_change(self.root(), snapshot, &config, kind, None, &metadata)?;
            crate::organization::validate_planning_change(snapshot,&config,kind,None,&metadata,true)?;
            prepare_write(self.root(), snapshot, kind, None, metadata, &input.body)
        })
    }

    /// Decide whether to create or update inside the replay-aware transaction.
    /// Adapters must not implement save by reading and choosing two operations.
    pub fn save_planning(
        &self,
        kind: PlanningKind,
        input: &SavePlanning,
        expected: Option<&SourceToken>,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        let parameters = json!({"kind":kind,"input":input,"expected":expected});
        self.store()?.transact(request, "planning.save", &parameters, |snapshot| {
            validate_id(&input.id)?;
            let config=crate::repository::config_from_snapshot(self.root(),snapshot)?;
            crate::retirement::ensure_writable(self.root(),snapshot,&config,&crate::RetirementTarget::new(kind.into(),input.id.clone())?)?;
            let original = match load_planning(self.root(), snapshot, kind, &input.id) {
                Ok(record) => Some(record),
                Err(error) if error.code == ErrorCode::NotFound => None,
                Err(error) => return Err(error),
            };
            if expected.is_some_and(|token| original.as_ref().is_none_or(|record| record.source != *token)) {
                return Err(PmError::new(ErrorCode::StaleSource, "planning record changed or disappeared since it was read"));
            }
            let now = Utc::now();
            let mut metadata = if let Some(original) = &original {
                original.metadata.clone()
            } else {
                if list_planning(self.root(), snapshot, kind)?.iter().any(|record| record.metadata.id.eq_ignore_ascii_case(&input.id))
                    || identity_was_used(snapshot, kind, &input.id)? {
                    return Err(PmError::new(ErrorCode::Conflict, "planning identity already exists or was previously used; archive and restore the existing record"));
                }
                PlanningMetadata {
                    schema: SchemaVersion::CURRENT, id: input.id.clone(), revision: Revision::INITIAL,
                    name: input.name.clone(), status: (kind != PlanningKind::Label).then(String::new),
                    starts_at: None, ends_at: None, color: (kind == PlanningKind::Label).then(String::new),
                    created_at: Some(now), updated_at: Some(now), imported: None, archived: false,
                    initiative: None, project: None, lead: None, scope: None, goal: None, targets: Vec::new(), exit_criteria: Vec::new(), outcomes: Vec::new(),
                    custom: BTreeMap::new(), extra: BTreeMap::new(),
                }
            };
            metadata.name = input.name.clone();
            metadata = apply_fields(metadata, kind, &input.fields)?;
            let body = input.body.as_deref().unwrap_or_else(|| original.as_ref().map_or("", |record| &record.body));
            if kind == PlanningKind::Label && !body.is_empty() {
                return Err(invalid("labels do not have Markdown bodies"));
            }
            crate::organization::validate_planning_change(snapshot,&config,kind,original.as_ref().map(|o|&o.metadata),&metadata,true)?;
            if let Some(original) = &original {
                if metadata == original.metadata && body == original.body {
                    return Ok(PreparedOperation { changes: Vec::new(), result: to_json(original)? });
                }
                metadata.revision = metadata.revision.next()?;
                metadata.updated_at = Some(now.max(metadata.updated_at.or(metadata.created_at).unwrap_or(now)));
            }
            metadata.validate(kind)?;
            hierarchy::validate_planning_change(self.root(), snapshot, &config, kind, original.as_ref().map(|record| &record.metadata), &metadata)?;
            prepare_write(self.root(), snapshot, kind, original.as_ref(), metadata, body)
        })
    }

    /// Replay precedes lookup and optional source checks. Without an expected
    /// token, apply this semantic intent to the current locked source snapshot.
    pub fn mutate_planning(
        &self,
        kind: PlanningKind,
        id: &str,
        expected: Option<&SourceToken>,
        mutation: &PlanningMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        let parameters = json!({"kind":kind,"id":id,"expected":expected,"mutation":mutation});
        self.store()?
            .transact(request, "planning.mutate", &parameters, |snapshot| {
                let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                crate::retirement::ensure_writable(
                    self.root(),
                    snapshot,
                    &config,
                    &crate::RetirementTarget::new(kind.into(), id)?,
                )?;
                let original = load_planning(self.root(), snapshot, kind, id)?;
                if expected.is_some_and(|expected| *expected != original.source) {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "planning record changed since it was read (revision or content differs)",
                    )
                    .at(self.root().join(&original.path)));
                }
                let mut metadata = original.metadata.clone();
                let mut body = original.body.clone();
                match mutation {
                    PlanningMutation::PatchCustom { patch } => {
                        metadata.custom = patch.apply(&metadata.custom)?
                    }
                    PlanningMutation::Update {
                        fields,
                        body: replacement,
                    } => {
                        metadata = apply_fields(metadata, kind, fields)?;
                        if let Some(replacement) = replacement {
                            body = replacement.clone();
                        }
                    }
                    PlanningMutation::Archive { archived } => metadata.archived = *archived,
                    PlanningMutation::Complete { acceptance } => {
                        crate::completion_policy::validate_planning_transition(
                            self.root(),
                            snapshot,
                            &config,
                            kind,
                            id,
                            acceptance,
                        )?;
                        let terminal = config
                            .workflow
                            .states
                            .iter()
                            .find(|state| state.category == crate::WorkflowCategory::Completed)
                            .ok_or_else(|| invalid("workflow has no completed terminal state"))?;
                        metadata.status = Some(terminal.id.clone());
                    }
                }
                if kind == PlanningKind::Label && !body.is_empty() {
                    return Err(invalid("labels do not have Markdown bodies"));
                }
                crate::organization::validate_planning_change(
                    snapshot,
                    &config,
                    kind,
                    Some(&original.metadata),
                    &metadata,
                    !matches!(mutation, PlanningMutation::Archive { .. }),
                )?;
                if metadata == original.metadata && body == original.body {
                    return Ok(PreparedOperation {
                        changes: Vec::new(),
                        result: to_json(&original)?,
                    });
                }
                metadata.revision = metadata.revision.next()?;
                let now = Utc::now();
                metadata.updated_at =
                    Some(now.max(metadata.updated_at.or(metadata.created_at).unwrap_or(now)));
                metadata.validate(kind)?;
                hierarchy::validate_planning_change(
                    self.root(),
                    snapshot,
                    &config,
                    kind,
                    Some(&original.metadata),
                    &metadata,
                )?;
                prepare_write(
                    self.root(),
                    snapshot,
                    kind,
                    Some(&original),
                    metadata,
                    &body,
                )
            })
    }
}

pub(crate) fn list_planning(
    root: &Path,
    snapshot: &Snapshot<'_>,
    kind: PlanningKind,
) -> Result<Vec<PlanningRecord>> {
    if kind == PlanningKind::Label {
        let Some((document, metadata)) = load_labels(root, snapshot)? else {
            return Ok(Vec::new());
        };
        let bytes = document.render();
        return Ok(metadata
            .labels
            .into_iter()
            .map(|metadata| {
                record(
                    kind,
                    metadata,
                    String::new(),
                    "labels.yml".into(),
                    bytes.as_bytes(),
                )
            })
            .collect());
    }
    let mut records = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for path in planning_paths(snapshot, kind)? {
        if path.components().count() != 3 || path.file_name().is_none_or(|name| name != "item.md") {
            return Err(
                invalid("planning records belong in <kind>/<id>/item.md").at(root.join(path))
            );
        }
        let id = path
            .parent()
            .and_then(Path::file_name)
            .and_then(|part| part.to_str())
            .ok_or_else(|| invalid("planning identity must be UTF-8").at(root.join(&path)))?;
        if !ids.insert(id.to_ascii_lowercase()) {
            return Err(
                invalid("duplicate or case-colliding planning identity").at(root.join(path))
            );
        }
        // This complete, bounded listing already validates the namespace and
        // case-folded uniqueness. Reuse it rather than scanning once per record.
        let bytes = snapshot
            .read_bounded(&path, MAX_DOCUMENT_BYTES)?
            .ok_or_else(|| not_found(root, kind, id))?;
        records.push(parse_planning_bytes(root, kind, id, &bytes)?);
    }
    records.sort_by(|a, b| a.metadata.id.cmp(&b.metadata.id));
    Ok(records)
}

pub(crate) fn load_planning(
    root: &Path,
    snapshot: &Snapshot<'_>,
    kind: PlanningKind,
    id: &str,
) -> Result<PlanningRecord> {
    validate_id(id)?;
    if kind == PlanningKind::Label {
        return list_planning(root, snapshot, kind)?
            .into_iter()
            .find(|record| record.metadata.id == id)
            .ok_or_else(|| not_found(root, kind, id));
    }
    for candidate in planning_paths(snapshot, kind)? {
        if let Some(other) = candidate
            .components()
            .nth(1)
            .and_then(|component| component.as_os_str().to_str())
            && other != id
            && other.eq_ignore_ascii_case(id)
        {
            return Err(invalid(
                "planning identity must match exact case and cannot have a case-colliding alias",
            )
            .at(root.join(candidate)));
        }
    }
    let path = record_path(kind, id);
    let bytes = snapshot
        .read_bounded(&path, MAX_DOCUMENT_BYTES)?
        .ok_or_else(|| not_found(root, kind, id))?;
    parse_planning_bytes(root, kind, id, &bytes)
}

pub(crate) fn parse_planning_bytes(
    root: &Path,
    kind: PlanningKind,
    id: &str,
    bytes: &[u8],
) -> Result<PlanningRecord> {
    validate_id(id)?;
    let path = record_path(kind, id);
    let absolute = root.join(&path);
    let text = text(&absolute, bytes)?;
    let document = MarkdownDocument::parse(&absolute, text)?;
    check_schema(&absolute, document.metadata())?;
    let metadata: PlanningMetadata = document.deserialize()?;
    metadata
        .validate(kind)
        .map_err(|error| error.at(&absolute))?;
    if metadata.id != id {
        return Err(invalid("planning directory and metadata identity disagree").at(absolute));
    }
    Ok(record(kind, metadata, document.body().into(), path, bytes))
}

fn load_labels(
    root: &Path,
    snapshot: &Snapshot<'_>,
) -> Result<Option<(YamlDocument, LabelsMetadata)>> {
    let path = root.join("labels.yml");
    let Some(bytes) = snapshot.read_bounded(Path::new("labels.yml"), MAX_DOCUMENT_BYTES)? else {
        return Ok(None);
    };
    let document = YamlDocument::parse(&path, text(&path, &bytes)?)?;
    check_schema(&path, document.metadata())?;
    if let Some(labels) = document
        .metadata()
        .get("labels")
        .and_then(YamlValue::as_sequence)
    {
        for label in labels {
            if let Some(mapping) = label.as_mapping() {
                check_schema(&path, mapping)?;
            }
        }
    }
    let metadata: LabelsMetadata = document.deserialize()?;
    metadata.validate().map_err(|error| error.at(&path))?;
    Ok(Some((document, metadata)))
}

pub(crate) fn prepare_write(
    root: &Path,
    snapshot: &Snapshot<'_>,
    kind: PlanningKind,
    original: Option<&PlanningRecord>,
    metadata: PlanningMetadata,
    body: &str,
) -> Result<PreparedOperation> {
    let path = record_path(kind, &metadata.id);
    let (source, expected) = if kind == PlanningKind::Label {
        let existing = load_labels(root, snapshot)?;
        let (document, mut labels, expected) = if let Some((document, labels)) = existing {
            let expected = Some(ContentHash::of(document.render().as_bytes()));
            (document, labels, expected)
        } else {
            (
                YamlDocument::parse(&root.join(&path), "schema: 1\nlabels: []\n")?,
                LabelsMetadata {
                    schema: SchemaVersion::CURRENT,
                    labels: Vec::new(),
                    custom: BTreeMap::new(),
                    extra: BTreeMap::new(),
                },
                None,
            )
        };
        let index = labels
            .labels
            .iter()
            .position(|label| label.id == metadata.id);
        if let Some(index) = index {
            labels.labels[index] = metadata.clone();
        } else {
            labels.labels.push(metadata.clone());
        }
        labels.validate()?;
        (patch_label(&document, &labels, &metadata, index)?, expected)
    } else {
        let document = if let Some(original) = original {
            let bytes = snapshot
                .read_bounded(&path, MAX_DOCUMENT_BYTES)?
                .ok_or_else(|| not_found(root, kind, &metadata.id))?;
            let mut document =
                MarkdownDocument::parse(&root.join(&path), text(&root.join(&path), &bytes)?)?;
            document.patch(&metadata_changes(document.metadata(), &metadata)?)?;
            if document.body() != body {
                document.set_body(body.into())?;
            }
            (document, Some(original.source.content.clone()))
        } else {
            let yaml =
                serde_yaml_ng::to_string(&metadata).map_err(|error| invalid(error.to_string()))?;
            (
                MarkdownDocument::parse(&root.join(&path), &format!("---\n{yaml}---\n{body}"))?,
                None,
            )
        };
        (document.0.render(), document.1)
    };
    let record = record(kind, metadata, body.into(), path.clone(), source.as_bytes());
    Ok(PreparedOperation {
        changes: vec![FileChange {
            path,
            expected,
            content: Some(source.into_bytes()),
        }],
        result: to_json(&record)?,
    })
}

fn apply_fields(
    metadata: PlanningMetadata,
    kind: PlanningKind,
    fields: &BTreeMap<String, Value>,
) -> Result<PlanningMetadata> {
    let mut value = to_json(&metadata)?;
    let object = value
        .as_object_mut()
        .expect("metadata serializes as object");
    for (key, value) in fields {
        if !PLANNING_WRITABLE_FIELDS.contains(&key.as_str())
            && !key
                .strip_prefix("x-")
                .is_some_and(crate::identity::valid_slug)
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                format!("field {key:?} cannot be changed through planning update"),
            ));
        }
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.clone(), value.clone());
        }
    }
    let metadata: PlanningMetadata =
        serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
    metadata.validate(kind)?;
    Ok(metadata)
}

fn identity_was_used(snapshot: &Snapshot<'_>, kind: PlanningKind, id: &str) -> Result<bool> {
    for path in snapshot.list(Path::new("operations"))? {
        let bytes = snapshot
            .read(&path)?
            .ok_or_else(|| PmError::new(ErrorCode::StaleSource, "operation receipt disappeared"))?;
        let receipt: MutationReceipt = serde_yaml_ng::from_slice(&bytes).map_err(|error| {
            PmError::new(
                ErrorCode::CorruptStore,
                format!("operation receipt cannot establish identity history: {error}"),
            )
            .at(&path)
        })?;
        if receipt.operation.starts_with("planning.")
            && receipt.result.get("kind") == Some(&to_json(&kind)?)
            && receipt
                .result
                .get("metadata")
                .and_then(|value| value.get("id"))
                .and_then(Value::as_str)
                .is_some_and(|used| used.eq_ignore_ascii_case(id))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn metadata_changes(original: &Mapping, metadata: &impl Serialize) -> Result<Mapping> {
    let updated = serde_yaml_ng::to_value(metadata).map_err(|error| invalid(error.to_string()))?;
    let updated = updated
        .as_mapping()
        .ok_or_else(|| invalid("planning metadata must be a mapping"))?;
    let mut changes = Mapping::new();
    for (key, value) in updated {
        if original.get(key) != Some(value) {
            changes.insert(key.clone(), value.clone());
        }
    }
    for key in original.keys() {
        if !updated.contains_key(key) {
            changes.insert(key.clone(), YamlValue::Null);
        }
    }
    Ok(changes)
}

/// Edit only the selected sequence entry, retaining other labels and outer YAML
/// comments/quotes. Parse and compare the complete semantic result before write.
fn patch_label(
    document: &YamlDocument,
    expected: &LabelsMetadata,
    metadata: &PlanningMetadata,
    index: Option<usize>,
) -> Result<String> {
    let original = document.render();
    let tree = yaml_edit::YamlFile::parse(&original).tree();
    let mapping = tree
        .document()
        .and_then(|document| document.as_mapping())
        .ok_or_else(|| invalid("labels file requires a mapping"))?;
    let sequence = mapping
        .get("labels")
        .and_then(|value| value.as_sequence().cloned())
        .ok_or_else(|| invalid("labels must be a sequence"))?;
    if let Some(index) = index {
        let target = sequence
            .get(index)
            .and_then(|value| value.as_mapping().cloned())
            .ok_or_else(|| invalid("label must be a mapping"))?;
        let previous: LabelsMetadata = document.deserialize()?;
        let previous = serde_yaml_ng::to_value(&previous.labels[index])
            .map_err(|error| invalid(error.to_string()))?;
        for (key, value) in
            metadata_changes(previous.as_mapping().expect("metadata mapping"), metadata)?
        {
            let key = key.as_str().expect("metadata keys are strings");
            if value.is_null() {
                target.remove(key);
            } else {
                let json =
                    serde_json::to_string(&value).map_err(|error| invalid(error.to_string()))?;
                let replacement = yaml_edit::YamlFile::parse(&format!(
                    "{}: {json}\n",
                    serde_json::to_string(key).expect("string serializes")
                ))
                .tree();
                let mapping = replacement
                    .document()
                    .and_then(|document| document.as_mapping())
                    .ok_or_else(|| invalid("label patch cannot be parsed"))?;
                target.set(
                    mapping
                        .keys()
                        .next()
                        .ok_or_else(|| invalid("label patch key missing"))?,
                    mapping
                        .values()
                        .next()
                        .ok_or_else(|| invalid("label patch value missing"))?,
                );
            }
        }
    } else {
        let replacement = yaml_edit::YamlFile::parse(&format!(
            "value: {}\n",
            serde_json::to_string(metadata).map_err(|error| invalid(error.to_string()))?
        ))
        .tree();
        let value = replacement
            .document()
            .and_then(|document| document.as_mapping())
            .and_then(|mapping| mapping.get("value"))
            .ok_or_else(|| invalid("label creation value cannot be parsed"))?;
        sequence.push(value);
    }
    let mut rendered = tree.to_string();
    if original.contains("\r\n") {
        rendered = rendered.replace("\r\n", "\n").replace('\n', "\r\n");
    }
    let candidate = YamlDocument::parse(Path::new("labels.yml"), &rendered)?;
    if candidate.deserialize::<LabelsMetadata>()? != *expected {
        return Err(invalid(
            "label edit changed values beyond the intended record",
        ));
    }
    Ok(rendered)
}

pub(crate) fn record_path(kind: PlanningKind, id: &str) -> PathBuf {
    if kind == PlanningKind::Label {
        "labels.yml".into()
    } else {
        format!("{}/{id}/item.md", kind.directory()).into()
    }
}
fn record(
    kind: PlanningKind,
    metadata: PlanningMetadata,
    body: String,
    path: PathBuf,
    bytes: &[u8],
) -> PlanningRecord {
    let source = SourceToken::new(metadata.revision, bytes);
    PlanningRecord {
        kind,
        metadata,
        body,
        path,
        source,
        retirement: None,
    }
}
fn check_schema(path: &Path, mapping: &Mapping) -> Result<()> {
    if let Some(schema) = mapping.get("schema").and_then(YamlValue::as_u64) {
        SchemaVersion::try_from(schema).map_err(|error| error.at(path))?;
    }
    Ok(())
}
fn text<'a>(path: &Path, bytes: &'a [u8]) -> Result<&'a str> {
    std::str::from_utf8(bytes).map_err(|_| invalid("planning source must be UTF-8").at(path))
}
fn not_found(root: &Path, kind: PlanningKind, id: &str) -> PmError {
    PmError::new(ErrorCode::NotFound, "planning record was not found")
        .at(root.join(record_path(kind, id)))
}
fn to_json(value: &impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| invalid(error.to_string()))
}

#[cfg(test)]
thread_local! {
    static NAMESPACE_SCANS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn planning_paths(snapshot: &Snapshot<'_>, kind: PlanningKind) -> Result<Vec<std::path::PathBuf>> {
    #[cfg(test)]
    NAMESPACE_SCANS.with(|count| count.set(count.get() + 1));
    snapshot.list_bounded(Path::new(kind.directory()), 20_000)
}

#[cfg(test)]
mod scale_tests {
    use super::*;

    #[test]
    fn listing_planning_scans_the_namespace_once() {
        let temporary = tempfile::tempdir().unwrap();
        let repository = Repository::init(temporary.path(), "WD").unwrap();
        for index in 0..80 {
            let id = format!("PRJ-{index:04}");
            let path = repository.root().join(format!("projects/{id}/item.md"));
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, format!("---\nschema: 1\nid: {id}\nrevision: 1\nname: Project {index}\ncreated_at: 2026-09-09T00:00:00Z\nupdated_at: 2026-09-09T00:00:00Z\n---\nBody\n")).unwrap();
        }
        NAMESPACE_SCANS.with(|count| count.set(0));
        let records = repository.list_planning(PlanningKind::Project).unwrap();
        assert_eq!(records.len(), 80);
        let scans = NAMESPACE_SCANS.with(|count| count.get());
        assert_eq!(
            scans, 1,
            "one snapshot's planning namespace was scanned {scans} times"
        );
        for record in records {
            assert_eq!(
                repository
                    .planning_record(PlanningKind::Project, &record.metadata.id)
                    .unwrap(),
                record
            );
        }
    }
}
