use super::*;
use crate::{
    Config, Repository, RequestId,
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument},
    transactions::{FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use chrono::Utc;
use serde_json::json;
use serde_yaml_ng::{Mapping, Value as YamlValue};
use std::path::Path;

pub(super) const MAX_ENTRIES: usize = 100_000;
pub(super) const MAX_RECORDS: usize = 40_000;
pub(super) const MAX_BYTES: usize = 64 * 1024 * 1024;
const FIELDS: &[&str] = &[
    "name",
    "lead",
    "parent",
    "prerequisites",
    "projects",
    "milestones",
    "targets",
    "sources",
    "criteria",
    "gates",
    "decision",
    "maturity",
    "availability",
    "custom",
];

pub(crate) fn validate_path(path: &Path) -> Result<FeatureId> {
    let path = path
        .to_str()
        .ok_or_else(|| invalid("feature path must be UTF-8"))?;
    SourceLink {
        path: path.into(),
        line: None,
        end_line: None,
    }
    .validate()?;
    let parts = path.split('/').collect::<Vec<_>>();
    if path.len() > 1024
        || parts.len() < 2
        || parts.len() > 18
        || parts[0] != "features"
        || parts[1..parts.len() - 1]
            .iter()
            .any(|part| !crate::identity::valid_slug(part))
    {
        return Err(invalid(
            "feature paths must use bounded portable grouping directories below features",
        ));
    }
    parts
        .last()
        .and_then(|name| name.strip_suffix(".md"))
        .ok_or_else(|| invalid("feature filename must be <FEAT-ID>.md"))?
        .parse()
}
pub(super) fn feature_path(id: &FeatureId, directory: &str) -> Result<PathBuf> {
    let path = if directory.is_empty() {
        Path::new("features").join(format!("{id}.md"))
    } else {
        Path::new("features")
            .join(directory)
            .join(format!("{id}.md"))
    };
    if validate_path(&path)? != *id {
        return Err(invalid("feature location differs from identity"));
    }
    Ok(path)
}
pub(crate) fn parse(path: &Path, bytes: &[u8], repository: &RepositoryId) -> Result<FeatureRecord> {
    let id = validate_path(path)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(invalid("feature document exceeds 2 MiB").at(path));
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| invalid("feature document must be UTF-8").at(path))?;
    if text.contains('\0') {
        return Err(invalid("feature document cannot contain NUL").at(path));
    }
    let document = MarkdownDocument::parse(path, text)?;
    let metadata: FeatureMetadata = document.deserialize()?;
    metadata.validate().map_err(|error| error.at(path))?;
    if metadata.id != id || &metadata.repository != repository {
        return Err(invalid("feature identity, source repository and filename disagree").at(path));
    }
    Ok(FeatureRecord {
        source: SourceToken {
            revision: metadata.revision,
            content: ContentHash::of(bytes),
        },
        metadata,
        body: document.body().into(),
        path: path.into(),
        document: text.into(),
        retirement: None,
    })
}
pub(crate) fn validate_record(record: &FeatureRecord, repository: &RepositoryId) -> Result<()> {
    let parsed = parse(&record.path, record.document.as_bytes(), repository).map_err(|error| {
        PmError::new(
            ErrorCode::CorruptStore,
            format!("invalid feature proof: {error}"),
        )
    })?;
    let mut actual = record.clone();
    actual.retirement = None;
    if parsed != actual {
        return Err(PmError::new(
            ErrorCode::CorruptStore,
            "feature source, identity, metadata, and body disagree",
        ));
    }
    Ok(())
}
pub(crate) fn load_features(
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<Vec<FeatureRecord>> {
    let mut records = Vec::new();
    let mut ids = BTreeSet::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("features"), MAX_ENTRIES)? {
        if records.len() >= MAX_RECORDS {
            return Err(invalid("feature catalog exceeds 40000 records"));
        }
        let bytes = snapshot
            .read_bounded(&path, MAX_DOCUMENT_BYTES)?
            .ok_or_else(|| invalid("feature disappeared").at(&path))?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("feature catalog size overflow"))?;
        if total > MAX_BYTES {
            return Err(invalid("feature catalog exceeds 64 MiB"));
        }
        let record = parse(&path, &bytes, &config.repository)?;
        if !ids.insert(record.metadata.id.clone()) {
            return Err(invalid("feature identity appears in multiple files").at(path));
        }
        records.push(record);
    }
    records.sort_by(|a, b| a.metadata.id.cmp(&b.metadata.id));
    Ok(records)
}
pub(super) fn matches_reference(id: &FeatureId, reference: &str) -> bool {
    reference.len() >= 4
        && (id.as_str().starts_with(reference)
            || id
                .as_str()
                .strip_prefix("FEAT-")
                .is_some_and(|value| value.starts_with(reference)))
}
pub(super) fn resolve<'a>(
    records: &'a [FeatureRecord],
    reference: &str,
) -> Result<&'a FeatureRecord> {
    let matches = records
        .iter()
        .filter(|record| matches_reference(&record.metadata.id, reference))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [record] => Ok(record),
        [] => Err(PmError::new(ErrorCode::NotFound, "feature not found")),
        _ => Err(PmError::new(
            ErrorCode::AmbiguousReference,
            "feature reference is ambiguous",
        )),
    }
}
pub(crate) fn load_feature(
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &FeatureId,
) -> Result<FeatureRecord> {
    load_features(snapshot, config)?
        .into_iter()
        .find(|record| &record.metadata.id == id)
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "feature not found"))
}
fn apply_fields(
    metadata: FeatureMetadata,
    fields: &BTreeMap<String, Value>,
) -> Result<FeatureMetadata> {
    let mut value = json!(metadata);
    let object = value.as_object_mut().expect("feature object");
    for (key, value) in fields {
        if !FIELDS.contains(&key.as_str())
            && !key
                .strip_prefix("x-")
                .is_some_and(crate::identity::valid_slug)
        {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                format!("feature field {key:?} is not writable"),
            ));
        }
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.clone(), value.clone());
        }
    }
    let metadata: FeatureMetadata =
        serde_json::from_value(value).map_err(|error| invalid(error.to_string()))?;
    metadata.validate()?;
    Ok(metadata)
}
pub(super) fn initial(
    repository: RepositoryId,
    id: FeatureId,
    input: &CreateFeature,
    now: Timestamp,
) -> Result<FeatureMetadata> {
    apply_fields(
        FeatureMetadata {
            schema: SchemaVersion::CURRENT,
            repository,
            id,
            revision: Revision::INITIAL,
            name: input.name.trim().into(),
            created_at: now,
            updated_at: now,
            lead: None,
            parent: None,
            prerequisites: Vec::new(),
            projects: Vec::new(),
            milestones: Vec::new(),
            targets: Vec::new(),
            sources: Vec::new(),
            criteria: Vec::new(),
            gates: Vec::new(),
            decision: FeatureDecision::default(),
            maturity: FeatureMaturity::default(),
            availability: FeatureAvailability::default(),
            archived: false,
            custom: BTreeMap::new(),
            extra: BTreeMap::new(),
        },
        &input.fields,
    )
}
pub(super) fn candidate(
    original: &FeatureRecord,
    mutation: &FeatureMutation,
) -> Result<(FeatureMetadata, String, PathBuf)> {
    let mut metadata = original.metadata.clone();
    let mut body = original.body.clone();
    let mut path = original.path.clone();
    match mutation {
        FeatureMutation::Update {
            fields,
            body: replacement,
        } => {
            metadata = apply_fields(metadata, fields)?;
            if let Some(value) = replacement {
                body = value.clone();
            }
        }
        FeatureMutation::Reparent { parent } => metadata.parent = parent.clone(),
        FeatureMutation::Relocate { directory } => path = feature_path(&metadata.id, directory)?,
        FeatureMutation::Archive { archived } => metadata.archived = *archived,
        FeatureMutation::PatchCustom { patch } => {
            metadata.custom = patch.apply(&metadata.custom)?
        }
        FeatureMutation::Promote { maturity, .. } => metadata.maturity = *maturity,
    }
    metadata.validate()?;
    Ok((metadata, body, path))
}
fn render(
    original: Option<&FeatureRecord>,
    metadata: &FeatureMetadata,
    body: &str,
    path: &Path,
) -> Result<String> {
    if body.len() > MAX_DOCUMENT_BYTES || body.contains('\0') {
        return Err(invalid("feature body must be bounded text without NUL"));
    }
    let text = if let Some(original) = original {
        let mut document = MarkdownDocument::parse(&original.path, &original.document)?;
        let value = serde_yaml_ng::to_value(metadata).map_err(|e| invalid(e.to_string()))?;
        let next = value.as_mapping().expect("feature metadata mapping");
        let mut patch = Mapping::new();
        for (key, value) in next {
            if document.metadata().get(key) != Some(value) {
                patch.insert(key.clone(), value.clone());
            }
        }
        for key in document.metadata().keys() {
            if !next.contains_key(key) {
                patch.insert(key.clone(), YamlValue::Null);
            }
        }
        document.patch(&patch)?;
        if document.body() != body {
            document.set_body(body.into())?;
        }
        document.render()
    } else {
        format!(
            "---\n{}---\n{body}",
            serde_yaml_ng::to_string(metadata).map_err(|error| invalid(error.to_string()))?
        )
    };
    let parsed = parse(path, text.as_bytes(), &metadata.repository)?;
    if &parsed.metadata != metadata || parsed.body != body {
        return Err(invalid("feature edit changed unexpected document content"));
    }
    Ok(text)
}
pub(crate) fn prepare_record(
    original: Option<&FeatureRecord>,
    metadata: FeatureMetadata,
    body: &str,
    path: PathBuf,
) -> Result<(FeatureRecord, Vec<FileChange>)> {
    let document = render(original, &metadata, body, &path)?;
    let record = parse(&path, document.as_bytes(), &metadata.repository)?;
    let mut changes = Vec::new();
    if original.is_some_and(|original| original.path != path) {
        let before = original.expect("present");
        changes.push(FileChange {
            path: before.path.clone(),
            expected: Some(before.source.content.clone()),
            content: None,
        });
    }
    if original.is_none_or(|old| old.path != path || old.document != document) {
        changes.push(FileChange {
            path,
            expected: original
                .filter(|old| old.path == record.path)
                .map(|old| old.source.content.clone()),
            content: Some(document.into_bytes()),
        });
    }
    Ok((record, changes))
}

pub(super) fn qualified(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> Result<Vec<FeatureRecord>> {
    let mut records = load_features(snapshot, config)?;
    if !records.is_empty() {
        let retirements = crate::retirement::RetirementIndex::capture(root, snapshot, config)?;
        for record in &mut records {
            record.retirement = retirements.get(&crate::RetirementTarget::new(
                crate::RetirementKind::Feature,
                record.metadata.id.as_str(),
            )?)?;
        }
    }
    Ok(records)
}
impl Repository {
    pub fn list_features(&self) -> Result<Vec<FeatureRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            qualified(self.root(), snapshot, &config)
        })
    }
    pub fn feature(&self, reference: &str) -> Result<FeatureRecord> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            Ok(resolve(&qualified(self.root(), snapshot, &config)?, reference)?.clone())
        })
    }
    pub fn create_feature(
        &self,
        input: &CreateFeature,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.feature_operation(
            FeatureIntent::Create {
                input: input.clone(),
            },
            request,
            |_| Ok(()),
        )
    }
    pub fn mutate_feature(
        &self,
        reference: &str,
        expected: Option<&SourceToken>,
        mutation: &FeatureMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_feature_with_faults(reference, expected, mutation, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn mutate_feature_with_faults(
        &self,
        reference: &str,
        expected: Option<&SourceToken>,
        mutation: &FeatureMutation,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.feature_operation(
            FeatureIntent::Mutate {
                reference: reference.into(),
                expected: expected.cloned(),
                mutation: mutation.clone(),
            },
            request,
            fault,
        )
    }
    fn feature_operation(
        &self,
        intent: FeatureIntent,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let operation = match intent {
            FeatureIntent::Create { .. } => "feature.create",
            FeatureIntent::Mutate { .. } => "feature.mutate",
        };
        let receipt=self.store()?.transact_with_faults(request,operation,&json!(intent),|snapshot|{
            let config=crate::repository::config_from_snapshot(self.root(),snapshot)?;
            let records=load_features(snapshot,&config)?;
            let (before,metadata,body,path)=match &intent {
                FeatureIntent::Create{input}=>{
                    let id=FeatureId::new();let metadata=initial(config.repository.clone(),id.clone(),input,Utc::now())?;
                    if records.iter().any(|record|record.metadata.id==id){return Err(PmError::new(ErrorCode::Conflict,"generated feature identity is already present; retry with a fresh request"));}
                    (None,metadata,input.body.clone(),feature_path(&id,input.directory.as_deref().unwrap_or_default())?)
                },
                FeatureIntent::Mutate{reference,expected,mutation}=>{
                    let before=resolve(&records,reference)?.clone();
                    if expected.as_ref().is_some_and(|source|source!=&before.source){return Err(PmError::new(ErrorCode::StaleSource,"feature changed since the expected source").at(&before.path));}
                    let (mut metadata,body,path)=candidate(&before,mutation)?;
                    if metadata!=before.metadata || body!=before.body || path!=before.path {metadata.revision=metadata.revision.next()?;metadata.updated_at=Utc::now().max(metadata.updated_at);}
                    (Some(before),metadata,body,path)
                }
            };
            let target=crate::RetirementTarget::new(crate::RetirementKind::Feature,metadata.id.as_str())?;
            crate::retirement::ensure_writable(self.root(),snapshot,&config,&target)?;
            if let FeatureIntent::Mutate {
                mutation: FeatureMutation::Promote { acceptance, .. },
                ..
            } = &intent
            {
                let original = before
                    .as_ref()
                    .ok_or_else(|| invalid("feature maturity promotion requires an existing feature"))?;
                crate::completion_policy::validate_feature_transition(
                    self.root(),
                    snapshot,
                    &config,
                    original,
                    &metadata,
                    acceptance,
                )?;
            }
            validate_change(self.root(),snapshot,&config,&records,before.as_ref().map(|record|&record.metadata),&metadata,!matches!(&intent,FeatureIntent::Mutate{mutation:FeatureMutation::Archive{..},..}))?;
            let (record,changes)=prepare_record(before.as_ref(),metadata,&body,path)?;
            let count=records.len()+usize::from(before.is_none());
            let total=records.iter().filter(|old|old.metadata.id!=record.metadata.id).try_fold(record.document.len(),|total,old|total.checked_add(old.document.len())).ok_or_else(||invalid("feature catalog size overflow"))?;
            if count>MAX_RECORDS || total>MAX_BYTES {return Err(invalid("projected feature catalog exceeds 40000 records or 64 MiB"));}
            let result=json!(FeatureOutcome{record,before,intent:intent.clone()});
            proof::validate_result(operation,&config.repository,&crate::transactions::canonical_hash(&json!(intent))?,&result,&proof::changed_paths(&changes))?;
            Ok(PreparedOperation{changes,result})
        },fault)?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}
