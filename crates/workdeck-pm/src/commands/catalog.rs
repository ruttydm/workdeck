use super::validation::{header, invalid};
use crate::{
    documents::YamlDocument,
    transactions::{Snapshot, canonical_hash},
    *,
};
use std::path::{Path, PathBuf};
pub(crate) const MAX_DEFINITION_BYTES: usize = 256 * 1024;
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;
const MAX_CATALOG_RECORDS: usize = 4096;
impl Repository {
    pub fn command_catalog(&self) -> Result<CommandCatalogSnapshot> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            capture(snapshot, &config)
        })
    }
    pub fn command(&self, id: &str) -> Result<CommandRecord> {
        super::validation::id(id)?;
        self.command_catalog()?
            .commands
            .into_iter()
            .find(|r| r.definition.id == id)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "command not found"))
    }
    pub fn check(&self, id: &str) -> Result<CheckRecord> {
        super::validation::id(id)?;
        self.command_catalog()?
            .checks
            .into_iter()
            .find(|r| r.definition.id == id)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "check not found"))
    }
    pub fn check_profile(&self, id: &str) -> Result<CheckProfileRecord> {
        super::validation::id(id)?;
        self.command_catalog()?
            .profiles
            .into_iter()
            .find(|r| r.definition.id == id)
            .ok_or_else(|| PmError::new(ErrorCode::NotFound, "check profile not found"))
    }
    pub fn validate_command_catalog(&self) -> Result<CatalogValidation> {
        self.store()?.with_snapshot(|snapshot| {
            let config = crate::repository::config_from_snapshot(self.root(), snapshot)?;
            let (catalog, errors) = scan(snapshot, &config);
            Ok(CatalogValidation {
                valid: errors.is_empty(),
                checked_records: catalog.commands.len()
                    + catalog.checks.len()
                    + catalog.profiles.len()
                    + errors.len(),
                errors,
            })
        })
    }
}
pub(crate) fn definition_document(path: &Path, bytes: &[u8]) -> Result<YamlDocument> {
    if bytes.len() > MAX_DEFINITION_BYTES {
        return Err(invalid("definition exceeds256KiB").at(path));
    }
    let text =
        std::str::from_utf8(bytes).map_err(|_| invalid("definition must be UTF-8").at(path))?;
    let doc = YamlDocument::parse(path, text)?;
    if let Some(schema) = doc
        .metadata()
        .get("schema")
        .and_then(serde_yaml_ng::Value::as_u64)
        && schema != 1
    {
        return Err(PmError::new(
            ErrorCode::UnsupportedSchema,
            format!("unsupported definition schema {schema}"),
        )
        .at(path));
    }
    Ok(doc)
}
fn validate_path(path: &Path, namespace: &str, id: &str) -> Result<()> {
    if path != Path::new(namespace).join(format!("{id}.yml")) {
        return Err(
            invalid("definition identity must exactly match its canonical filename").at(path),
        );
    }
    Ok(())
}
pub(crate) fn parse_command(
    path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<CommandRecord> {
    let doc = definition_document(path, bytes)?;
    let definition: CommandDefinition = doc.deserialize()?;
    header(
        &definition.repository,
        repository,
        &definition.id,
        &definition.name,
    )?;
    definition.validate()?;
    validate_path(path, "commands", &definition.id)?;
    Ok(CommandRecord {
        definition,
        path: path.into(),
        content: ContentHash::of(bytes),
        document: doc.render(),
    })
}
pub(crate) fn parse_check(
    path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<CheckRecord> {
    let doc = definition_document(path, bytes)?;
    let definition: CheckDefinition = doc.deserialize()?;
    header(
        &definition.repository,
        repository,
        &definition.id,
        &definition.name,
    )?;
    definition.validate()?;
    validate_path(path, "checks", &definition.id)?;
    Ok(CheckRecord {
        definition,
        path: path.into(),
        content: ContentHash::of(bytes),
        document: doc.render(),
    })
}
pub(crate) fn parse_profile(
    path: &Path,
    bytes: &[u8],
    repository: &RepositoryId,
) -> Result<CheckProfileRecord> {
    let doc = definition_document(path, bytes)?;
    let definition: CheckProfileDefinition = doc.deserialize()?;
    header(
        &definition.repository,
        repository,
        &definition.id,
        &definition.name,
    )?;
    definition.validate()?;
    validate_path(path, "check-profiles", &definition.id)?;
    Ok(CheckProfileRecord {
        definition,
        path: path.into(),
        content: ContentHash::of(bytes),
        document: doc.render(),
    })
}
fn empty(config: &Config) -> CommandCatalogSnapshot {
    CommandCatalogSnapshot {
        schema: SchemaVersion::CURRENT,
        repository: config.repository.clone(),
        commands: Vec::new(),
        checks: Vec::new(),
        profiles: Vec::new(),
        fingerprint: ContentHash::of(b""),
    }
}
pub(crate) fn scan(
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (CommandCatalogSnapshot, Vec<PmError>) {
    let mut catalog = empty(config);
    let mut errors = Vec::new();
    let mut count = 0;
    let mut total = 0;
    for namespace in ["commands", "checks", "check-profiles"] {
        let paths = match snapshot.list_bounded(Path::new(namespace), MAX_CATALOG_RECORDS) {
            Ok(paths) => paths,
            Err(e) => {
                errors.push(e);
                continue;
            }
        };
        for path in paths {
            count += 1;
            if count > MAX_CATALOG_RECORDS {
                errors.push(invalid("catalog exceeds4096 records"));
                return (catalog, errors);
            }
            let result = (|| {
                if path.components().count() != 2 || path.extension().is_none_or(|e| e != "yml") {
                    return Err(invalid("catalog records must be direct slug.yml files").at(&path));
                }
                let bytes = snapshot
                    .read_bounded(&path, MAX_DEFINITION_BYTES)?
                    .ok_or_else(|| invalid("definition disappeared").at(&path))?;
                total += bytes.len();
                if total > MAX_CATALOG_BYTES {
                    return Err(invalid("catalog exceeds16MiB").at(&path));
                }
                match namespace {
                    "commands" => {
                        catalog
                            .commands
                            .push(parse_command(&path, &bytes, &config.repository)?)
                    }
                    "checks" => {
                        catalog
                            .checks
                            .push(parse_check(&path, &bytes, &config.repository)?)
                    }
                    _ => catalog
                        .profiles
                        .push(parse_profile(&path, &bytes, &config.repository)?),
                }
                Ok(())
            })();
            if let Err(error) = result {
                errors.push(error.at(&path))
            }
            if total > MAX_CATALOG_BYTES {
                return (catalog, errors);
            }
        }
    }
    errors.extend(crate::checks::validation::references(&catalog));
    match catalog_hash(&catalog) {
        Ok(hash) => catalog.fingerprint = hash,
        Err(error) => errors.push(error),
    }
    (catalog, errors)
}
pub(crate) fn catalog_hash(catalog: &CommandCatalogSnapshot) -> Result<ContentHash> {
    canonical_hash(
        &serde_json::json!({"schema":catalog.schema,"repository":catalog.repository,"commands":catalog.commands.iter().map(|r|(&r.definition.id,&r.content)).collect::<Vec<_>>(),"checks":catalog.checks.iter().map(|r|(&r.definition.id,&r.content)).collect::<Vec<_>>(),"profiles":catalog.profiles.iter().map(|r|(&r.definition.id,&r.content)).collect::<Vec<_>>()}),
    )
}
pub(crate) fn capture(snapshot: &Snapshot<'_>, config: &Config) -> Result<CommandCatalogSnapshot> {
    let (catalog, errors) = scan(snapshot, config);
    if let Some(error) = errors.into_iter().next() {
        Err(error)
    } else {
        Ok(catalog)
    }
}
/// Pure historical validation: exact record documents, identities and reference closure.
pub(crate) fn validate_snapshot(catalog: &CommandCatalogSnapshot) -> Result<()> {
    let mut paths = std::collections::BTreeSet::<PathBuf>::new();
    for record in &catalog.commands {
        if parse_command(
            &record.path,
            record.document.as_bytes(),
            &catalog.repository,
        )? != *record
            || !paths.insert(record.path.clone())
        {
            return Err(invalid("command catalog document/pin mismatch"));
        }
    }
    for record in &catalog.checks {
        if parse_check(
            &record.path,
            record.document.as_bytes(),
            &catalog.repository,
        )? != *record
            || !paths.insert(record.path.clone())
        {
            return Err(invalid("check catalog document/pin mismatch"));
        }
    }
    for record in &catalog.profiles {
        if parse_profile(
            &record.path,
            record.document.as_bytes(),
            &catalog.repository,
        )? != *record
            || !paths.insert(record.path.clone())
        {
            return Err(invalid("profile catalog document/pin mismatch"));
        }
    }
    if let Some(error) = crate::checks::validation::references(catalog)
        .into_iter()
        .next()
    {
        return Err(error);
    }
    if catalog_hash(catalog)? != catalog.fingerprint {
        return Err(invalid("catalog fingerprint mismatch"));
    }
    Ok(())
}
