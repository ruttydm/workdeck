use super::*;
use crate::{
    documents::YamlDocument,
    repository::config_from_snapshot,
    transactions::{
        ChangedPath, FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot,
        canonical_hash,
    },
    *,
};
use serde_json::json;
use serde_yaml_ng::{Mapping, Value as YamlValue};
use std::path::{Path, PathBuf};
pub(crate) fn path(id: &GateId) -> PathBuf {
    Path::new("gates").join(format!("{id}.yml"))
}
pub(crate) fn validate_path(value: &Path) -> Result<GateId> {
    let id: GateId = value
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("invalid gate filename"))?
        .parse()?;
    if path(&id) != value {
        return Err(invalid("gate path must be gates/<GATE-ID>.yml").at(value));
    }
    Ok(id)
}
pub(crate) fn parse(path: &Path, bytes: &[u8], repository: &RepositoryId) -> Result<GateRecord> {
    let id = validate_path(path)?;
    if bytes.len() > MAX_GATE_BYTES {
        return Err(invalid("gate exceeds 128 KiB").at(path));
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("gate must be UTF-8"))?;
    let document = YamlDocument::parse(path, text)?;
    let schema = document
        .metadata()
        .get(YamlValue::String("schema".into()))
        .and_then(|v| v.as_u64())
        .ok_or_else(|| invalid("gate schema must be an integer"))?;
    SchemaVersion::try_from(schema)?;
    let definition: GateDefinition = document.deserialize()?;
    definition.validate()?;
    if definition.id != id || &definition.repository != repository {
        return Err(invalid("gate source, repository and identity differ").at(path));
    }
    Ok(GateRecord {
        source: SourceToken {
            revision: definition.revision,
            content: ContentHash::of(bytes),
        },
        definition,
        path: path.into(),
        document: text.into(),
        retirement: None,
    })
}
pub(crate) fn load_gates(snapshot: &Snapshot<'_>, config: &Config) -> Result<Vec<GateRecord>> {
    let mut records = Vec::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("gates"), MAX_GATE_ENTRIES * 2)? {
        if records.len() >= MAX_GATE_ENTRIES {
            return Err(invalid("gate catalog exceeds 4096 records"));
        }
        let bytes = snapshot
            .read_bounded(&path, MAX_GATE_BYTES)?
            .ok_or_else(|| invalid("gate disappeared"))?;
        total = total.saturating_add(bytes.len());
        if total > 32 * 1024 * 1024 {
            return Err(invalid("gate catalog exceeds 32 MiB"));
        }
        records.push(parse(&path, &bytes, &config.repository)?);
    }
    records.sort_by(|a, b| a.definition.id.cmp(&b.definition.id));
    Ok(records)
}
pub(crate) fn load_gate(
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &GateId,
) -> Result<GateRecord> {
    let path = path(id);
    let bytes = snapshot
        .read_bounded(&path, MAX_GATE_BYTES)?
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "gate not found").at(&path))?;
    parse(&path, &bytes, &config.repository)
}
fn attach(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    mut record: GateRecord,
) -> Result<GateRecord> {
    record.retirement = crate::retirement::read_tombstone(
        root,
        snapshot,
        config,
        &RetirementTarget::new(RetirementKind::Gate, record.definition.id.as_str())?,
    )?;
    Ok(record)
}
pub(crate) fn apply(
    definition: &GateDefinition,
    mutation: &GateMutation,
) -> Result<GateDefinition> {
    let mut next = definition.clone();
    match mutation {
        GateMutation::Archive { archived } => next.archived = *archived,
        GateMutation::PatchCustom { patch } => next.custom = patch.apply(&next.custom)?,
        GateMutation::Update { fields } => {
            let mut value = json!(next);
            let object = value.as_object_mut().expect("gate mapping");
            for (key, value) in fields {
                if !["name", "description", "requirements", "custom"].contains(&key.as_str())
                    && !key
                        .strip_prefix("x-")
                        .is_some_and(crate::identity::valid_slug)
                {
                    return Err(PmError::new(
                        ErrorCode::InvalidInput,
                        format!("gate field {key:?} is not writable"),
                    ));
                }
                if value.is_null() {
                    object.remove(key);
                } else {
                    object.insert(key.clone(), value.clone());
                }
            }
            next = serde_json::from_value(value).map_err(|e| invalid(e.to_string()))?;
        }
    }
    next.validate()?;
    Ok(next)
}
fn render(previous: Option<&GateRecord>, definition: &GateDefinition) -> Result<String> {
    let Some(previous) = previous else {
        return serde_yaml_ng::to_string(definition).map_err(|e| invalid(e.to_string()));
    };
    let mut document = YamlDocument::parse(&previous.path, &previous.document)?;
    let next = serde_yaml_ng::to_value(definition).map_err(|e| invalid(e.to_string()))?;
    let next = next.as_mapping().expect("gate map");
    let mut patch = Mapping::new();
    for key in document.metadata().keys().chain(next.keys()) {
        patch.insert(
            key.clone(),
            next.get(key).cloned().unwrap_or(YamlValue::Null),
        );
    }
    document.patch(&patch)?;
    Ok(document.render())
}
fn check_requirements(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    before: Option<&GateDefinition>,
    next: &GateDefinition,
) -> Result<()> {
    for requirement in &next.requirements {
        if before.is_some_and(|old| old.requirements.iter().any(|r| r == requirement)) {
            continue;
        }
        let criterion = criteria::resolve(
            root,
            snapshot,
            config,
            &requirement.criterion.owner,
            &requirement.criterion.id,
        )?;
        if criterion.retired || criterion.reference != requirement.criterion {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "new gate requirement must pin an existing active criterion definition",
            ));
        }
    }
    Ok(())
}
pub(crate) fn validate_change(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    before: Option<&GateRecord>,
    next: &GateRecord,
) -> Result<()> {
    if let Some(old) = before {
        if old.definition.id != next.definition.id
            || old.definition.repository != next.definition.repository
            || old.definition.created_at != next.definition.created_at
        {
            return Err(invalid("gate identity and creation time are immutable"));
        }
        if old.definition != next.definition
            && (next.definition.revision <= old.definition.revision
                || next.definition.updated_at < old.definition.updated_at)
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "native gate replacements must advance revision and preserve timestamp ordering",
            ));
        }
        if crate::retirement::read_tombstone(
            root,
            snapshot,
            config,
            &RetirementTarget::new(RetirementKind::Gate, old.definition.id.as_str())?,
        )?
        .is_some()
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "retired gate cannot be changed",
            ));
        }
    }
    check_requirements(
        root,
        snapshot,
        config,
        before.map(|r| &r.definition),
        &next.definition,
    )?;
    crate::organization::validate_gate_change(
        snapshot,
        config,
        before.map(|r| &r.definition),
        &next.definition,
        true,
    )
}
impl Repository {
    pub fn gate(&self, id: &GateId) -> Result<GateRecord> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            attach(
                self.root(),
                snapshot,
                &config,
                load_gate(snapshot, &config, id)?,
            )
        })
    }
    pub fn gates(&self) -> Result<Vec<GateRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            load_gates(snapshot, &config)?
                .into_iter()
                .map(|r| attach(self.root(), snapshot, &config, r))
                .collect()
        })
    }
    pub fn create_gate(&self, input: &CreateGate, request: &RequestId) -> Result<MutationReceipt> {
        self.write_gate_with_faults(
            &GateRequest::Create {
                input: input.clone(),
            },
            request,
            |_| Ok(()),
        )
    }
    pub fn mutate_gate(
        &self,
        id: &GateId,
        expected: &SourceToken,
        mutation: &GateMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.write_gate_with_faults(
            &GateRequest::Mutate {
                id: id.clone(),
                expected: expected.clone(),
                mutation: mutation.clone(),
            },
            request,
            |_| Ok(()),
        )
    }
    #[doc(hidden)]
    pub fn write_gate_with_faults(
        &self,
        input: &GateRequest,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let receipt = self.store()?.transact_with_faults(
            request,
            "gate.write",
            &json!(input),
            |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                let records = load_gates(snapshot, &config)?;
                let (previous, mut definition) = match input {
                    GateRequest::Create { input } => {
                        if records.len() >= MAX_GATE_ENTRIES {
                            return Err(invalid("gate catalog exceeds 4096 records"));
                        }
                        let now = chrono::Utc::now();
                        (
                            None,
                            GateDefinition {
                                schema: SchemaVersion::CURRENT,
                                repository: config.repository.clone(),
                                id: GateId::new(),
                                revision: Revision::INITIAL,
                                name: input.name.clone(),
                                description: input.description.clone(),
                                created_at: now,
                                updated_at: now,
                                requirements: input.requirements.clone(),
                                archived: false,
                                custom: input.custom.clone(),
                                extra: input.extra.clone(),
                            },
                        )
                    }
                    GateRequest::Mutate {
                        id,
                        expected,
                        mutation,
                    } => {
                        let old = attach(
                            self.root(),
                            snapshot,
                            &config,
                            load_gate(snapshot, &config, id)?,
                        )?;
                        if &old.source != expected {
                            return Err(PmError::new(
                                ErrorCode::StaleSource,
                                "gate source changed",
                            ));
                        }
                        if old.retirement.is_some() {
                            return Err(PmError::new(
                                ErrorCode::PolicyBlocked,
                                "retired gate cannot be changed",
                            ));
                        }
                        let next = apply(&old.definition, mutation)?;
                        (Some(old), next)
                    }
                };
                definition.validate()?;
                check_requirements(
                    self.root(),
                    snapshot,
                    &config,
                    previous.as_ref().map(|r| &r.definition),
                    &definition,
                )?;
                crate::organization::validate_gate_change(
                    snapshot,
                    &config,
                    previous.as_ref().map(|r| &r.definition),
                    &definition,
                    !matches!(
                        input,
                        GateRequest::Mutate {
                            mutation: GateMutation::Archive { .. },
                            ..
                        }
                    ),
                )?;
                let changed = previous
                    .as_ref()
                    .is_none_or(|old| old.definition != definition);
                if changed && let Some(old) = &previous {
                    definition.revision = old.definition.revision.next()?;
                    definition.updated_at = chrono::Utc::now().max(old.definition.updated_at);
                }
                let path = path(&definition.id);
                let document = render(previous.as_ref(), &definition)?;
                let gate = parse(&path, document.as_bytes(), &config.repository)?;
                let total = records
                    .iter()
                    .filter(|r| r.definition.id != definition.id)
                    .map(|r| r.document.len())
                    .sum::<usize>()
                    .saturating_add(document.len());
                if total > 32 * 1024 * 1024 {
                    return Err(invalid("gate catalog exceeds 32 MiB"));
                }
                let changes = if changed {
                    vec![FileChange {
                        path,
                        expected: previous.as_ref().map(|r| r.source.content.clone()),
                        content: Some(document.into_bytes()),
                    }]
                } else {
                    vec![]
                };
                Ok(PreparedOperation {
                    changes,
                    result: json!(GateMutationResult {
                        gate,
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
pub(crate) fn prepare_archive(
    _root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &GateId,
) -> Result<PreparedOperation> {
    let old = load_gate(snapshot, config, id)?;
    let mut definition = old.definition.clone();
    definition.archived = true;
    definition.revision = definition.revision.next()?;
    definition.updated_at = chrono::Utc::now().max(definition.updated_at);
    let document = render(Some(&old), &definition)?;
    let record = parse(&old.path, document.as_bytes(), &config.repository)?;
    Ok(PreparedOperation {
        changes: vec![FileChange {
            path: old.path,
            expected: Some(old.source.content),
            content: Some(document.into_bytes()),
        }],
        result: json!(record),
    })
}
/// Exact historical source+intent proof, independent of today's gate state.
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != "gate.write" {
        return Ok(());
    }
    let result: GateMutationResult =
        serde_json::from_value(receipt.result.clone()).map_err(|e| invalid(e.to_string()))?;
    let gate = &result.gate;
    if parse(
        &gate.path,
        gate.document.as_bytes(),
        &gate.definition.repository,
    )? != *gate
        || receipt.repository.as_ref() != Some(&gate.definition.repository)
        || canonical_hash(&json!(result.input))? != receipt.input_hash
    {
        return Err(invalid(
            "gate receipt differs from exact document or request intent",
        ));
    }
    let mut expected = match (&result.input, &result.previous) {
        (GateRequest::Create { input }, None) => GateDefinition {
            schema: SchemaVersion::CURRENT,
            repository: gate.definition.repository.clone(),
            id: gate.definition.id.clone(),
            revision: Revision::INITIAL,
            name: input.name.clone(),
            description: input.description.clone(),
            created_at: gate.definition.created_at,
            updated_at: gate.definition.created_at,
            requirements: input.requirements.clone(),
            archived: false,
            custom: input.custom.clone(),
            extra: input.extra.clone(),
        },
        (
            GateRequest::Mutate {
                id,
                expected,
                mutation,
            },
            Some(old),
        ) => {
            if parse(
                &old.path,
                old.document.as_bytes(),
                &gate.definition.repository,
            )? != *old
                || &old.definition.id != id
                || &old.source != expected
                || old.path != gate.path
            {
                return Err(invalid(
                    "gate receipt prior source differs from required expected token",
                ));
            }
            apply(&old.definition, mutation)?
        }
        _ => {
            return Err(invalid(
                "gate receipt is missing or invents its prior source",
            ));
        }
    };
    let changed = result
        .previous
        .as_ref()
        .is_none_or(|old| old.definition != expected);
    if changed && let Some(old) = &result.previous {
        expected.revision = old.definition.revision.next()?;
        expected.updated_at = gate.definition.updated_at;
        if expected.updated_at < old.definition.updated_at {
            return Err(invalid("gate update time predates prior source"));
        }
    }
    let changes = if changed {
        vec![ChangedPath {
            path: gate.path.clone(),
            before: result
                .previous
                .as_ref()
                .map(|old| old.source.content.clone()),
            after: Some(gate.source.content.clone()),
        }]
    } else {
        vec![]
    };
    if expected != gate.definition
        || render(result.previous.as_ref(), &expected)? != gate.document
        || changes != receipt.changed
    {
        return Err(invalid(
            "gate receipt output is not the authorized semantic mutation",
        ));
    }
    Ok(())
}
pub(crate) fn inspect_snapshot(snapshot: &Snapshot<'_>, config: &Config) -> (usize, Vec<PmError>) {
    let paths = match snapshot.list_bounded(Path::new("gates"), MAX_GATE_ENTRIES * 2) {
        Ok(p) => p,
        Err(e) => return (0, vec![e]),
    };
    let count = paths.len();
    let mut errors = vec![];
    let mut total = 0usize;
    for path in paths {
        let result = snapshot
            .read_bounded(&path, MAX_GATE_BYTES)
            .and_then(|b| b.ok_or_else(|| invalid("gate disappeared")))
            .and_then(|bytes| {
                total = total.saturating_add(bytes.len());
                parse(&path, &bytes, &config.repository)
            });
        if let Err(e) = result {
            errors.push(e.at(path));
        }
        if total > 32 * 1024 * 1024 {
            break;
        }
    }
    if count > MAX_GATE_ENTRIES || total > 32 * 1024 * 1024 {
        errors.push(invalid("gate catalog exceeds supported bounds"));
    }
    (count, errors)
}
pub(crate) fn validate_record(record: &GateRecord, repository: &RepositoryId) -> Result<()> {
    let mut bare = record.clone();
    bare.retirement = None;
    if parse(&record.path, record.document.as_bytes(), repository)? != bare {
        return Err(invalid("gate record differs from its exact source"));
    }
    Ok(())
}
