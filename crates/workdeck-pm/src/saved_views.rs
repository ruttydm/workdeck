//! Versioned saved predicates. A view stores intent, never cached membership or
//! evidence. Evaluation captures the definition and issue sources under one lock.
use crate::{
    documents::YamlDocument,
    transactions::{ChangedPath, FileChange, MutationReceipt, PreparedOperation, Snapshot},
    *,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
const MAX_VIEWS: usize = 1024;
const MAX_VIEW_BYTES: usize = 64 * 1024;
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedViewDefinition {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub archived: bool,
    pub query: IssueQuery,
    #[serde(default)]
    pub custom: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedViewRecord {
    pub definition: SavedViewDefinition,
    pub path: PathBuf,
    pub content: ContentHash,
    /// Exact bounded YAML, retained so historical receipts can validate their
    /// full source definition without consulting today's mutable view file.
    pub document: String,
}
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteSavedView {
    pub id: String,
    pub name: String,
    pub query: IssueQuery,
    #[serde(default)]
    pub archived: bool,
    pub expected: Option<ContentHash>,
}
#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SavedViewResult {
    pub view: SavedViewRecord,
    pub issues: Vec<IssueRecord>,
}
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
pub(crate) fn path(id: &str) -> Result<PathBuf> {
    if !crate::identity::valid_slug(id) {
        return Err(invalid("saved-view ID must be a lowercase portable slug"));
    }
    Ok(Path::new("views").join(format!("{id}.yml")))
}
fn validate(definition: &SavedViewDefinition, repository: &RepositoryId) -> Result<()> {
    path(&definition.id)?;
    if &definition.repository != repository {
        return Err(invalid("saved view belongs to another repository"));
    }
    if definition.name.trim().is_empty()
        || definition.name.len() > 512
        || definition.name.chars().any(char::is_control)
    {
        return Err(invalid(
            "saved-view name must be nonempty text up to 512 bytes",
        ));
    }
    if definition.extra.keys().any(|key| !key.starts_with("x-")) {
        return Err(invalid(
            "unknown saved-view fields must use the x- prefix or custom mapping",
        ));
    }
    definition.query.validate()
}
fn read(snapshot: &Snapshot<'_>, repository: &RepositoryId, id: &str) -> Result<SavedViewRecord> {
    let path = path(id)?;
    let bytes = snapshot
        .read_bounded(&path, MAX_VIEW_BYTES)?
        .ok_or_else(|| PmError::new(ErrorCode::NotFound, "saved view not found").at(&path))?;
    parse(&path, &bytes, repository)
}
fn parse(path: &Path, bytes: &[u8], repository: &RepositoryId) -> Result<SavedViewRecord> {
    if bytes.len() > MAX_VIEW_BYTES {
        return Err(invalid("saved view exceeds 64 KiB").at(path));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| invalid("saved view must be UTF-8").at(path))?;
    let document = YamlDocument::parse(path, text)?;
    let definition: SavedViewDefinition = document.deserialize()?;
    validate(&definition, repository).map_err(|error| error.at(path))?;
    if self::path(&definition.id)? != path {
        return Err(invalid("saved-view identity must match its filename").at(path));
    }
    Ok(SavedViewRecord {
        definition,
        path: path.into(),
        content: ContentHash::of(bytes),
        document: text.into(),
    })
}
pub(crate) fn inspect(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<PmError>, Vec<PmError>) {
    let mut count = 0;
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    match snapshot.list_bounded(Path::new("views"), MAX_VIEWS) {
        Err(error) => errors.push(error),
        Ok(paths) => {
            for path in paths {
                count += 1;
                let result = (|| {
                    let bytes = snapshot
                        .read_bounded(&path, MAX_VIEW_BYTES)?
                        .ok_or_else(|| invalid("saved view disappeared"))?;
                    let record = parse(&path, &bytes, &config.repository)?;
                    if let Err(error) = validate_status(&record.definition, config) {
                        warnings.push(error.at(root.join(&path)).hint("Repair the saved status filter to use the current workflow. Retained definitions are not rewritten when workflow statuses change."));
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    errors.push(error);
                }
            }
        }
    }
    (count, errors, warnings)
}

fn validate_status(definition: &SavedViewDefinition, config: &Config) -> Result<()> {
    if let Some(status) = &definition.query.status {
        config.workflow.canonical_status(status)?;
    }
    Ok(())
}

/// Prospective authoring admission is deliberately separate from historical
/// parsing so an exact snapshot can preserve a view whose workflow has changed.
pub(crate) fn validate_import(snapshot: &Snapshot<'_>, config: &Config, path: &Path) -> Result<()> {
    let bytes = snapshot
        .read_bounded(path, MAX_VIEW_BYTES)?
        .ok_or_else(|| invalid("imported saved view disappeared").at(path))?;
    let record = parse(path, &bytes, &config.repository)?;
    validate_status(&record.definition, config).map_err(|error| error.at(path))
}
impl Repository {
    pub fn saved_view(&self, id: &str) -> Result<SavedViewRecord> {
        path(id)?;
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            read(snapshot, &config.repository, id)
        })
    }
    pub fn saved_views(&self) -> Result<Vec<SavedViewRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let mut views = Vec::new();
            for path in snapshot.list_bounded(Path::new("views"), MAX_VIEWS)? {
                let bytes = snapshot
                    .read_bounded(&path, MAX_VIEW_BYTES)?
                    .ok_or_else(|| invalid("saved view disappeared"))?;
                views.push(parse(&path, &bytes, &config.repository)?);
            }
            views.sort_by(|a, b| a.definition.id.cmp(&b.definition.id));
            Ok(views)
        })
    }
    pub fn query_saved_view(&self, id: &str) -> Result<SavedViewResult> {
        path(id)?;
        self.store()?.with_snapshot(|snapshot| {
            let capture = crate::queries::capture(self.root(), snapshot)?;
            let view = read(snapshot, capture.repository(), id)?;
            let indices = capture.select_indices(&view.definition.query)?;
            let issues = indices
                .into_iter()
                .map(|index| capture.issues()[index].clone())
                .collect();
            Ok(SavedViewResult { view, issues })
        })
    }
    pub fn write_saved_view(
        &self,
        input: &WriteSavedView,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.write_saved_view_with_faults(input, request, |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn write_saved_view_with_faults(
        &self,
        input: &WriteSavedView,
        request: &RequestId,
        fault: impl FnMut(transactions::FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let path = path(&input.id)?;
        let parameters = json!(input);
        let receipt = self.store()?.transact_with_faults(
            request,
            "view.write",
            &parameters,
            |snapshot| {
                let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
                let paths = snapshot.list_bounded(Path::new("views"), MAX_VIEWS)?;
                let before = snapshot.read_bounded(&path, MAX_VIEW_BYTES)?;
                let actual = before.as_deref().map(ContentHash::of);
                if actual != input.expected {
                    return Err(PmError::new(
                        if input.expected.is_some() {
                            ErrorCode::StaleSource
                        } else {
                            ErrorCode::Conflict
                        },
                        "saved view differs from expected content; read it before updating",
                    )
                    .at(&path));
                }
                if before.is_none() && paths.len() >= MAX_VIEWS {
                    return Err(invalid("saved-view limit reached"));
                }
                let current = before
                    .as_deref()
                    .map(|bytes| parse(&path, bytes, &config.repository))
                    .transpose()?;
                let definition = SavedViewDefinition {
                    schema: SchemaVersion::CURRENT,
                    repository: config.repository.clone(),
                    id: input.id.clone(),
                    name: input.name.clone(),
                    archived: input.archived,
                    query: input.query.clone(),
                    custom: current
                        .as_ref()
                        .map(|v| v.definition.custom.clone())
                        .unwrap_or_default(),
                    extra: current
                        .as_ref()
                        .map(|v| v.definition.extra.clone())
                        .unwrap_or_default(),
                };
                validate(&definition, &config.repository)?;
                // Canonical status aliases are validated against this exact workflow.
                validate_status(&definition, &config)?;
                let yaml =
                    serde_yaml_ng::to_value(&definition).map_err(|e| invalid(e.to_string()))?;
                let bytes = if let Some(before) = &before {
                    let mut document = YamlDocument::parse(
                        &path,
                        std::str::from_utf8(before)
                            .map_err(|_| invalid("saved view must be UTF-8"))?,
                    )?;
                    document.patch(yaml.as_mapping().expect("definition is a map"))?;
                    document.render().into_bytes()
                } else {
                    serde_yaml_ng::to_string(&definition)
                        .map_err(|e| invalid(e.to_string()))?
                        .into_bytes()
                };
                if bytes.len() > MAX_VIEW_BYTES {
                    return Err(invalid("saved view exceeds 64 KiB"));
                }
                let record = parse(&path, &bytes, &config.repository)?;
                if record.definition != definition {
                    return Err(invalid("saved-view edit changed unexpected metadata"));
                }
                let changes = if before.as_deref() == Some(bytes.as_slice()) {
                    Vec::new()
                } else {
                    vec![FileChange {
                        path: path.clone(),
                        expected: actual,
                        content: Some(bytes),
                    }]
                };
                let result = json!(record);
                let changed = changes
                    .iter()
                    .map(|change| ChangedPath {
                        path: change.path.clone(),
                        before: change.expected.clone(),
                        after: change.content.as_deref().map(ContentHash::of),
                    })
                    .collect::<Vec<_>>();
                validate_result(
                    &result,
                    &changed,
                    &crate::transactions::canonical_hash(&parameters)?,
                    &config.repository,
                )?;
                Ok(PreparedOperation { changes, result })
            },
            fault,
        )?;
        validate_receipt(&receipt)?;
        Ok(receipt)
    }
}

/// Historical source/result/intent consistency; this does not authenticate
/// user-editable receipts, and never compares them to today's view definition.
pub(crate) fn validate_receipt(receipt: &MutationReceipt) -> Result<()> {
    if receipt.operation != "view.write" {
        return Ok(());
    }
    let repository = receipt.repository.as_ref().ok_or_else(corrupt_receipt)?;
    validate_result(
        &receipt.result,
        &receipt.changed,
        &receipt.input_hash,
        repository,
    )
}

fn corrupt_receipt() -> PmError {
    PmError::new(
        ErrorCode::CorruptStore,
        "saved-view receipt source, definition, changes, or original input proof is invalid",
    )
}

fn validate_result(
    result: &Value,
    changed: &[ChangedPath],
    input_hash: &ContentHash,
    repository: &RepositoryId,
) -> Result<()> {
    let record: SavedViewRecord =
        serde_json::from_value(result.clone()).map_err(|_| corrupt_receipt())?;
    let parsed = parse(&record.path, record.document.as_bytes(), repository)
        .map_err(|_| corrupt_receipt())?;
    if record != parsed {
        return Err(corrupt_receipt());
    }
    let expected = match changed {
        [] => Some(record.content.clone()),
        [change]
            if change.path == record.path
                && change.after.as_ref() == Some(&record.content)
                && change.before != change.after =>
        {
            change.before.clone()
        }
        _ => return Err(corrupt_receipt()),
    };
    let input = WriteSavedView {
        id: record.definition.id,
        name: record.definition.name,
        query: record.definition.query,
        archived: record.definition.archived,
        expected,
    };
    if &crate::transactions::canonical_hash(&json!(input))? != input_hash {
        return Err(corrupt_receipt());
    }
    Ok(())
}
