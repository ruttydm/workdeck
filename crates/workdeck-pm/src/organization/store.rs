use super::policy::Policy;
use super::*;
use crate::{
    documents::YamlDocument,
    transactions::{FileChange, MutationReceipt, PreparedOperation, Snapshot},
    *,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

pub(crate) fn document(
    snapshot: &Snapshot<'_>,
    path: &Path,
) -> Result<Option<(YamlDocument, Vec<u8>)>> {
    let Some(bytes) = snapshot.read_bounded(path, MAX_ORGANIZATION_BYTES)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| invalid("organization records must be UTF-8").at(path))?;
    let document = YamlDocument::parse(path, text)?;
    if let Some(version) = document
        .metadata()
        .get("schema")
        .and_then(serde_yaml_ng::Value::as_u64)
    {
        SchemaVersion::try_from(version).map_err(|e| e.at(path))?;
    }
    Ok(Some((document, bytes)))
}
pub(crate) fn users(snapshot: &Snapshot<'_>, repository: &RepositoryId) -> Result<UsersRecord> {
    let path = Path::new("users.yml");
    let (registry, source) = if let Some((document, bytes)) = document(snapshot, path)? {
        let registry: UsersRegistry = document.deserialize()?;
        let source = SourceToken::new(registry.revision, &bytes);
        (registry, Some(source))
    } else {
        (
            UsersRegistry {
                schema: SchemaVersion::CURRENT,
                repository: repository.clone(),
                revision: Revision::INITIAL,
                mode: IdentityMode::Open,
                users: BTreeMap::new(),
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            None,
        )
    };
    registry.validate().map_err(|e| e.at(path))?;
    if &registry.repository != repository {
        return Err(invalid("users registry belongs to a different repository").at(path));
    }
    Ok(UsersRecord {
        registry,
        path: path.into(),
        source,
    })
}
pub(crate) fn schema(
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
) -> Result<OrganizationSchemaRecord> {
    let path = Path::new("schema.yml");
    let (definition, source) = if let Some((document, bytes)) = document(snapshot, path)? {
        let definition: OrganizationSchema = document.deserialize()?;
        let source = SourceToken::new(definition.revision, &bytes);
        (definition, Some(source))
    } else {
        (
            OrganizationSchema {
                schema: SchemaVersion::CURRENT,
                repository: repository.clone(),
                revision: Revision::INITIAL,
                fields: BTreeMap::new(),
                unit_mode: IdentityMode::Open,
                units: BTreeMap::new(),
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            None,
        )
    };
    definition.validate().map_err(|e| e.at(path))?;
    if &definition.repository != repository {
        return Err(invalid("organization schema belongs to a different repository").at(path));
    }
    Ok(OrganizationSchemaRecord {
        definition,
        path: path.into(),
        source,
    })
}
fn expected(actual: &Option<SourceToken>, expected: Option<&SourceToken>) -> Result<()> {
    if expected.is_some_and(|expected| Some(expected) != actual.as_ref()) {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "organization source changed since it was read",
        ));
    }
    Ok(())
}
fn value<T: Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|e| invalid(e.to_string()))
}
/// Update only changed collection entries. Unrelated comments, quoting and
/// extension values remain in the original syntax tree.
fn render<T: Serialize + DeserializeOwned>(
    snapshot: &Snapshot<'_>,
    path: &Path,
    next: &T,
    collections: &[&str],
) -> Result<Vec<u8>> {
    let desired = value(next)?;
    let Some((mut document, _)) = document(snapshot, path)? else {
        let bytes = serde_yaml_ng::to_string(next)
            .map(String::into_bytes)
            .map_err(|e| invalid(e.to_string()))?;
        if bytes.len() > MAX_ORGANIZATION_BYTES {
            return Err(invalid("organization record exceeds 512 KiB"));
        }
        return Ok(bytes);
    };
    for name in collections {
        let current: Value = document.deserialize()?;
        if current.get(*name) == desired.get(*name) {
            continue;
        }
        let tree: yaml_edit::YamlFile = document
            .render()
            .parse()
            .map_err(|e| invalid(format!("cannot edit organization YAML: {e}")))?;
        let root = tree
            .document()
            .and_then(|d| d.as_mapping())
            .ok_or_else(|| invalid("registry is not a mapping"))?;
        let child_node = root.get(*name);
        let child = child_node.as_ref().and_then(|v| v.as_mapping());
        let Some(child) = child else {
            let patch = serde_yaml_ng::to_value(BTreeMap::from([(*name, desired[*name].clone())]))
                .map_err(|e| invalid(e.to_string()))?;
            document.patch(patch.as_mapping().expect("mapping"))?;
            continue;
        };
        for (id, entry) in desired[*name]
            .as_object()
            .ok_or_else(|| invalid("registry collection is not a map"))?
        {
            if current[*name].get(id) == Some(entry) {
                continue;
            }
            let entry_text = format!(
                "{}: {}\n",
                serde_json::to_string(id).expect("string"),
                serde_json::to_string(entry).expect("JSON")
            );
            let replacement: yaml_edit::YamlFile = entry_text
                .parse()
                .map_err(|e| invalid(format!("cannot encode organization entry: {e}")))?;
            let mapping = replacement
                .document()
                .and_then(|d| d.as_mapping())
                .ok_or_else(|| invalid("entry is not a mapping"))?;
            child.set(
                mapping.keys().next().expect("key"),
                mapping.values().next().expect("value"),
            );
        }
        document = YamlDocument::parse(path, &tree.to_string())?;
    }
    let current: Value = document.deserialize()?;
    let patch: serde_yaml_ng::Value = serde_yaml_ng::to_value(
        desired
            .as_object()
            .expect("object")
            .iter()
            .filter(|(key, _)| !collections.contains(&key.as_str()))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
    .map_err(|e| invalid(e.to_string()))?;
    document.patch(patch.as_mapping().expect("mapping"))?;
    let actual: Value = document.deserialize()?;
    if actual != desired {
        return Err(invalid(format!(
            "organization edit changed unexpected values (original {} fields)",
            current.as_object().map_or(0, |o| o.len())
        )));
    }
    let bytes = document.render().into_bytes();
    if bytes.len() > MAX_ORGANIZATION_BYTES {
        return Err(invalid("organization record exceeds 512 KiB"));
    }
    Ok(bytes)
}
fn user_prepared(
    snapshot: &Snapshot<'_>,
    mut record: UsersRecord,
    next: UsersRegistry,
) -> Result<PreparedOperation> {
    if next == record.registry {
        return Ok(PreparedOperation {
            changes: vec![],
            result: value(&record)?,
        });
    }
    let mut next = next;
    next.revision = if record.source.is_some() {
        record.registry.revision.next()?
    } else {
        Revision::INITIAL
    };
    next.validate()?;
    let bytes = render(snapshot, &record.path, &next, &["users"])?;
    let expected = record.source.as_ref().map(|s| s.content.clone());
    record.registry = next;
    record.source = Some(SourceToken::new(record.registry.revision, &bytes));
    Ok(PreparedOperation {
        changes: vec![FileChange {
            path: record.path.clone(),
            expected,
            content: Some(bytes),
        }],
        result: value(&record)?,
    })
}
impl Repository {
    pub fn users(&self) -> Result<UsersRecord> {
        self.store()?
            .with_snapshot(|snapshot| users(snapshot, self.identity()))
    }
    pub fn user(&self, id: &str) -> Result<UserDefinition> {
        types::key(id, "user")?;
        self.users()?
            .registry
            .users
            .remove(id)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "user not found"))
    }
    pub fn mutate_user(
        &self,
        id: &str,
        token: Option<&SourceToken>,
        mutation: &UserMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.store()?.transact(
            request,
            "organization.user",
            &json!({"id":id,"expected":token,"mutation":mutation}),
            |snapshot| {
                types::key(id, "user")?;
                let current = users(snapshot, self.identity())?;
                expected(&current.source, token)?;
                let mut next = current.registry.clone();
                match mutation {
                    UserMutation::Patch { name, kind, custom } => {
                        let user = next
                            .users
                            .get_mut(id)
                            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "user not found"))?;
                        if let Some(name) = name {
                            user.name = name.clone();
                        }
                        if let Some(kind) = kind {
                            user.kind = *kind;
                        }
                        user.custom = custom.apply(&user.custom)?;
                    }
                    UserMutation::Create { user } => {
                        if next.users.keys().any(|old| old.eq_ignore_ascii_case(id))
                            || user_was_used(snapshot, self.identity(), id)?
                        {
                            return Err(PmError::new(
                                ErrorCode::Conflict,
                                "user identity already exists; update or unarchive it",
                            ));
                        }
                        next.users.insert(id.into(), user.clone());
                    }
                    UserMutation::Update { user } => {
                        if !next.users.contains_key(id) {
                            return Err(PmError::new(ErrorCode::NotFound, "user not found"));
                        }
                        next.users.insert(id.into(), user.clone());
                    }
                    UserMutation::Archive { archived } => {
                        next.users
                            .get_mut(id)
                            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "user not found"))?
                            .archived = *archived;
                    }
                }
                user_prepared(snapshot, current, next)
            },
        )
    }
    pub fn set_identity_mode(
        &self,
        mode: IdentityMode,
        token: Option<&SourceToken>,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.store()?.transact(request,"organization.identity_mode",&json!({"mode":mode,"expected":token}),|snapshot|{
            let current=users(snapshot,self.identity())?;expected(&current.source,token)?;let mut next=current.registry.clone();next.mode=mode;
            if mode!=current.registry.mode {let definition=schema(snapshot,self.identity())?;let policy=Policy{users:next.clone(),schema:definition.definition};let report=policy.compliance(self.root(),snapshot)?;if report.violations.iter().any(|v|!v.historical){return Err(blocked("registered identity mode requires resolving active organization policy violations").details(json!(report)));}}
            user_prepared(snapshot,current,next)
        })
    }
    pub fn organization_schema(&self) -> Result<OrganizationSchemaRecord> {
        self.store()?
            .with_snapshot(|snapshot| schema(snapshot, self.identity()))
    }
    pub fn preview_schema_change(&self, change: &SchemaChange) -> Result<SchemaChangePlan> {
        self.store()?
            .with_snapshot(|snapshot| plan(self.root(), snapshot, self.identity(), change))
    }
    pub fn apply_schema_change(
        &self,
        change: &SchemaChange,
        expected_plan: Option<&ContentHash>,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.apply_schema_change_with_faults(change, expected_plan, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn apply_schema_change_with_faults(
        &self,
        change: &SchemaChange,
        expected_plan: Option<&ContentHash>,
        request: &RequestId,
        fault: impl FnMut(transactions::FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.store()?.transact_with_faults(
            request,
            "organization.schema",
            &json!({"change":change,"expected_plan":expected_plan}),
            |snapshot| {
                let plan = plan(self.root(), snapshot, self.identity(), change)?;
                if expected_plan.is_some_and(|expected| expected != &plan.fingerprint) {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "organization schema preview is stale; preview the current source",
                    ));
                }
                if !plan.allowed {
                    return Err(blocked(
                        "schema activation would violate active organization policy",
                    )
                    .details(json!(plan)));
                }
                let current = schema(snapshot, self.identity())?;
                if current.definition == plan.definition {
                    return Ok(PreparedOperation {
                        changes: vec![],
                        result: value(&current)?,
                    });
                }
                let bytes = render(
                    snapshot,
                    &current.path,
                    &plan.definition,
                    &["fields", "units"],
                )?;
                let revision = plan.definition.revision;
                let next = OrganizationSchemaRecord {
                    definition: plan.definition,
                    path: current.path.clone(),
                    source: Some(SourceToken::new(revision, &bytes)),
                };
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: current.path,
                        expected: current.source.map(|s| s.content),
                        content: Some(bytes),
                    }],
                    result: value(&next)?,
                })
            },
            fault,
        )
    }
    pub fn organization_compliance(&self) -> Result<OrganizationCompliance> {
        self.store()?.with_snapshot(|snapshot| {
            Policy::load(snapshot, self.identity())?.compliance(self.root(), snapshot)
        })
    }
    pub fn estimate_report(&self, query: &IssueQuery) -> Result<EstimateReport> {
        self.store()?.with_snapshot(|snapshot| {
            policy::bound(snapshot)?;
            let capture = crate::queries::capture(self.root(), snapshot)?;
            let indices = capture.select_indices(query)?;
            let mut report = EstimateReport {
                by_unit: BTreeMap::new(),
                estimated: 0,
                unestimated: 0,
            };
            for index in indices {
                if let Some(estimate) = &capture.issues()[index].metadata.estimate {
                    let total = report
                        .by_unit
                        .entry(estimate.unit.clone())
                        .or_insert_with(DecimalAmount::zero);
                    *total = total.checked_add(&estimate.value)?;
                    report.estimated += 1;
                } else {
                    report.unestimated += 1;
                }
            }
            Ok(report)
        })
    }
}
fn plan(
    root: &Path,
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
    change: &SchemaChange,
) -> Result<SchemaChangePlan> {
    let current = schema(snapshot, repository)?;
    let mut next = current.definition.clone();
    next.fields.extend(change.fields.clone());
    next.units.extend(change.units.clone());
    if let Some(mode) = change.unit_mode {
        next.unit_mode = mode;
    }
    next.validate()?;
    for receipt in history(snapshot, repository)? {
        if let Some(value) = receipt.result.get("definition") {
            let old: OrganizationSchema =
                serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
            policy::schema_compatibility(&old, &next)?;
        }
    }
    policy::schema_compatibility(&current.definition, &next)?;
    if next != current.definition {
        next.revision = if current.source.is_some() {
            current.definition.revision.next()?
        } else {
            Revision::INITIAL
        };
    }
    let policy = Policy {
        schema: next.clone(),
        users: users(snapshot, repository)?.registry,
    };
    let compliance = policy.compliance(root, snapshot)?;
    let blockers = compliance
        .violations
        .iter()
        .filter(|v| !v.historical)
        .map(|v| blocked(format!("{}: {}", v.field, v.message)).at(&v.path))
        .collect::<Vec<_>>();
    let dependencies = policy::fingerprint_sources(snapshot)?;
    let fingerprint = crate::transactions::canonical_hash(
        &json!({"source":current.source,"definition":next,"dependencies":dependencies}),
    )?;
    Ok(SchemaChangePlan {
        repository: repository.clone(),
        source: current.source,
        definition: next,
        fingerprint,
        allowed: blockers.is_empty(),
        blockers,
        compliance,
    })
}

/// Durable results prevent identity reuse even if a direct edit removed an
/// aggregate entry. This is source/Git authority, not cryptographic identity.
pub(crate) fn history(
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
) -> Result<Vec<MutationReceipt>> {
    let mut receipts = Vec::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new("operations"), MAX_ORGANIZATION_ENTRIES)? {
        let bytes = snapshot
            .read_bounded(&path, 32 * 1024 * 1024)?
            .ok_or_else(|| invalid("operation receipt disappeared"))?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("organization history size overflow"))?;
        if total > 32 * 1024 * 1024 {
            return Err(invalid("organization identity history exceeds 32 MiB"));
        }
        let receipt: MutationReceipt = serde_yaml_ng::from_slice(&bytes)
            .map_err(|e| invalid(format!("invalid operation receipt: {e}")).at(&path))?;
        if receipt.operation.starts_with("organization.") {
            if receipt.repository.as_ref() != Some(repository) {
                return Err(invalid("organization receipt repository mismatch").at(&path));
            }
            receipts.push(receipt);
        }
    }
    Ok(receipts)
}
fn user_was_used(snapshot: &Snapshot<'_>, repository: &RepositoryId, id: &str) -> Result<bool> {
    for receipt in history(snapshot, repository)? {
        if let Some(users) = receipt
            .result
            .pointer("/registry/users")
            .and_then(Value::as_object)
            && users.keys().any(|old| old.eq_ignore_ascii_case(id))
        {
            return Ok(true);
        }
    }
    Ok(false)
}
