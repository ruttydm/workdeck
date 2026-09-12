//! Read-only legacy export and import feasibility inspection. No legacy write
//! contract remains; actual imports use native project-management transactions.
//! Records/aggregates are bounded to 2 MiB, events to 2 MiB per line, and
//! captured/projected files to 64 MiB total and 10,000 entries. JSON values
//! outside TOML's representation and ambiguous source records fail explicitly.
use super::*;
use crate::bounded_files::{self, Kind};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const RECORD_LIMIT: usize = 2 * 1024 * 1024;
const TOTAL_LIMIT: usize = 64 * 1024 * 1024;
const COLLECTIONS: [&str; 6] = [
    "issues",
    "projects",
    "cycles",
    "labels",
    "agent_sessions",
    "events",
];

#[derive(Debug, Serialize)]
pub struct LegacyImportSummary {
    pub dry_run: bool,
    pub read_only: bool,
    pub issues: usize,
    pub projects: usize,
    pub cycles: usize,
    pub labels: usize,
    pub agent_sessions: usize,
    pub events: usize,
}

#[derive(PartialEq, Eq)]
struct Captured {
    identity: String,
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl WorkdeckStore {
    /// Read the prototype export schema without projecting away unknown fields.
    pub fn legacy_export_document(&self) -> Result<Value> {
        self.ensure_legacy_format()?;
        match fs::symlink_metadata(self.root()) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // The established empty export is useful before initialization;
                // it neither opens nor creates a planning authority.
                return Ok(Value::Object(serde_json::Map::from_iter(
                    COLLECTIONS.into_iter().map(|name| (name.into(), json!([]))),
                )));
            }
            Err(error) => return Err(error.into()),
            Ok(_) => {}
        }
        let before = capture(self)?;
        let document = document(&before)?;
        if capture(self)? != before {
            bail!("legacy source changed during export; reload before retrying");
        }
        Ok(document)
    }

    /// Validate all rows, destination records, path collisions and projected
    /// serialization without creating, replacing or deleting any files.
    pub fn preview_legacy_import(
        &self,
        input: &Value,
        replace: bool,
    ) -> Result<LegacyImportSummary> {
        let before = capture(self)?;
        let current = document(&before)?;
        validate_document(input)?;
        let count = |name: &str| rows(input, name).map(<[Value]>::len);
        let summary = LegacyImportSummary {
            dry_run: true,
            read_only: true,
            issues: count("issues")?,
            projects: count("projects")?,
            cycles: count("cycles")?,
            labels: count("labels")?,
            agent_sessions: count("agent_sessions")?,
            events: count("events")?,
        };
        let mut merged = serde_json::Map::new();
        for name in COLLECTIONS {
            let incoming = rows(input, name)?;
            let mut values = if replace {
                Vec::new()
            } else {
                rows(&current, name)?.to_vec()
            };
            if name == "events" {
                values.extend_from_slice(incoming);
            } else {
                for record in incoming {
                    let id = identity(name, record)?;
                    if let Some(old) = values
                        .iter_mut()
                        .find(|old| identity(name, old).ok() == Some(id))
                    {
                        *old = record.clone();
                    } else {
                        values.push(record.clone());
                    }
                }
                values.sort_by(|a, b| {
                    identity(name, a)
                        .expect("validated identity")
                        .cmp(identity(name, b).expect("validated identity"))
                });
            }
            merged.insert(name.into(), Value::Array(values));
        }
        let merged = Value::Object(merged);
        validate_document(&merged)?;
        let writes = serialize_document(&merged)?;
        // Validate each projected path, including unexpected aliases, without
        // publishing it. A direct edit during preparation invalidates the result.
        for path in writes.keys() {
            require_content(self, &before.identity, path, before.files.get(path))?;
        }
        if capture(self)? != before {
            bail!("legacy destination changed while preparing import; reload before retrying");
        }
        Ok(summary)
    }
}

fn require_content(
    store: &WorkdeckStore,
    identity: &str,
    path: &Path,
    expected: Option<&Vec<u8>>,
) -> Result<()> {
    store.ensure_legacy_format()?;
    if root_identity(store.root())? != identity {
        bail!("legacy import source directory changed during preview; reload before retrying");
    }
    if read_optional(store.root(), path, limit(path))?.as_ref() != expected {
        bail!(
            "legacy import target changed during preview {}; reload before retrying",
            path.display()
        );
    }
    Ok(())
}

fn limit(path: &Path) -> usize {
    if path == Path::new("events.jsonl") {
        TOTAL_LIMIT
    } else {
        RECORD_LIMIT
    }
}

fn read_optional(root: &Path, path: &Path, bound: usize) -> Result<Option<Vec<u8>>> {
    match bounded_files::read(root, path, bound) {
        Ok(file) if !file.truncated => Ok(Some(file.bytes)),
        Ok(_) => bail!(
            "legacy transfer file exceeds its {bound}-byte limit: {}",
            path.display()
        ),
        Err(failure)
            if failure
                .chain()
                .filter_map(|cause| cause.downcast_ref::<std::io::Error>())
                .any(|cause| cause.kind() == std::io::ErrorKind::NotFound) =>
        {
            Ok(None)
        }
        Err(failure) => Err(failure),
    }
}

fn root_identity(root: &Path) -> Result<String> {
    let root = root
        .canonicalize()
        .context("legacy transfer requires an existing source")?;
    let metadata = fs::metadata(&root)?;
    if !metadata.is_dir() {
        bail!("legacy source must be a directory");
    }
    #[cfg(unix)]
    let identity = {
        use std::os::unix::fs::MetadataExt;
        format!("{}:{}:{}", root.display(), metadata.dev(), metadata.ino())
    };
    #[cfg(not(unix))]
    let identity = root.display().to_string();
    Ok(identity)
}

fn capture(store: &WorkdeckStore) -> Result<Captured> {
    store.ensure_legacy_format()?;
    let identity = root_identity(store.root())?;
    let mut paths = Vec::new();
    for directory in ["issues", "agents"] {
        for entry in store.entries(directory)? {
            if entry.kind != Kind::File {
                bail!(
                    "legacy record entry must be an ordinary file: {directory}/{}",
                    entry.name
                );
            }
            if Path::new(&entry.name)
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
            {
                if Path::new(&entry.name)
                    .extension()
                    .is_none_or(|extension| extension != "toml")
                {
                    bail!(
                        "legacy record extension must be exactly .toml: {}",
                        entry.name
                    );
                }
                paths.push(Path::new(directory).join(entry.name));
            }
        }
    }
    paths.extend(
        [
            "projects.toml",
            "cycles.toml",
            "labels.toml",
            "events.jsonl",
        ]
        .into_iter()
        .map(PathBuf::from),
    );
    if paths.len() > 10_000 {
        bail!("legacy transfer exceeds 10,000 files");
    }
    let mut files = BTreeMap::new();
    let mut total = 0;
    for path in paths {
        if let Some(bytes) = read_optional(store.root(), &path, limit(&path))? {
            total += bytes.len();
            if total > TOTAL_LIMIT {
                bail!("legacy transfer exceeds 64 MiB");
            }
            files.insert(path, bytes);
        }
    }
    Ok(Captured { identity, files })
}

fn document(source: &Captured) -> Result<Value> {
    let mut result =
        serde_json::Map::from_iter(COLLECTIONS.into_iter().map(|name| (name.into(), json!([]))));
    for (path, bytes) in &source.files {
        let text = std::str::from_utf8(bytes)
            .with_context(|| format!("legacy file is not UTF-8: {}", path.display()))?;
        if path == Path::new("events.jsonl") {
            for line in text.lines().filter(|line| !line.trim().is_empty()) {
                if line.len() > RECORD_LIMIT {
                    bail!("legacy event exceeds 2 MiB");
                }
                // Reuse strict transfer framing so duplicate keys at any depth
                // are rejected before their values can be collapsed by Value.
                let framed = format!("{{\"events\":[{line}]}}");
                let workdeck_pm::ImportSource::Legacy(event) =
                    workdeck_pm::decode_transfer(framed.as_bytes())?
                else {
                    bail!("legacy event has invalid framing");
                };
                let event = event.canonical_document()["events"][0].clone();
                result
                    .get_mut("events")
                    .unwrap()
                    .as_array_mut()
                    .unwrap()
                    .push(event);
            }
        } else if let Some(directory) = path
            .parent()
            .and_then(Path::to_str)
            .filter(|parent| !parent.is_empty())
        {
            let name = if directory == "issues" {
                "issues"
            } else {
                "agent_sessions"
            };
            let record: toml::Value = toml::from_str(text)
                .with_context(|| format!("invalid legacy record {}", path.display()))?;
            let record = serde_json::to_value(record)?;
            let id = identity(name, &record)?;
            if *path != record_path(name, id)? {
                bail!(
                    "legacy record identity differs from its path: {}",
                    path.display()
                );
            }
            result
                .get_mut(name)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(record);
        } else {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .context("invalid aggregate path")?;
            if text.trim().is_empty() {
                continue;
            }
            let table: toml::Table = toml::from_str(text)
                .with_context(|| format!("invalid legacy aggregate {}", path.display()))?;
            if table.keys().any(|key| key != name) {
                bail!("legacy export cannot represent aggregate metadata outside {name} records");
            }
            if let Some(value) = table.get(name) {
                result.insert(name.into(), serde_json::to_value(value)?);
            }
        }
    }
    let result = Value::Object(result);
    validate_document(&result)?;
    Ok(result)
}

fn rows<'a>(document: &'a Value, name: &str) -> Result<&'a [Value]> {
    document
        .get(name)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .with_context(|| format!("legacy document requires the {name} array"))
}

fn identity<'a>(name: &str, record: &'a Value) -> Result<&'a str> {
    record
        .get(if name == "issues" { "key" } else { "id" })
        .and_then(Value::as_str)
        .with_context(|| format!("legacy {name} record requires an identity"))
}

fn record_path(name: &str, id: &str) -> Result<PathBuf> {
    let directory = if name == "issues" { "issues" } else { "agents" };
    let safe = sanitize_key(id);
    if safe.is_empty() {
        bail!("legacy record id has no safe filename");
    }
    let leaf = safe.to_ascii_uppercase();
    if matches!(leaf.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (leaf.len() == 4
            && (leaf.starts_with("COM") || leaf.starts_with("LPT"))
            && matches!(leaf.as_bytes()[3], b'1'..=b'9'))
    {
        bail!("legacy record id uses a reserved device filename");
    }
    Ok(Path::new(directory).join(format!("{safe}.toml")))
}

fn validate_document(document: &Value) -> Result<()> {
    let object = document
        .as_object()
        .context("legacy document must be an object")?;
    if object
        .keys()
        .any(|key| key != "repo_root" && !COLLECTIONS.contains(&key.as_str()))
    {
        bail!("legacy document contains unknown collections");
    }
    let mut all_paths = BTreeSet::new();
    for name in COLLECTIONS {
        let records = rows(document, name)?;
        if records.len() > 10_000 {
            bail!("legacy {name} exceeds 10,000 records");
        }
        let mut ids = BTreeSet::new();
        for record in records {
            if !record.is_object() {
                bail!("legacy {name} rows must be objects");
            }
            match name {
                "issues" => {
                    let item: Issue = serde_json::from_value(record.clone())?;
                    if !valid_issue_key(&item.key)
                        || sanitize_key(&item.key) != item.key
                        || item.title.trim().is_empty()
                    {
                        bail!("invalid legacy issue identity or title");
                    }
                }
                "projects" => {
                    let item: Project = serde_json::from_value(record.clone())?;
                    normalized_reference_id(Some(item.id), &item.name)?;
                    if item.name.trim().is_empty() {
                        bail!("project name cannot be empty");
                    }
                }
                "cycles" => {
                    let item: Cycle = serde_json::from_value(record.clone())?;
                    normalized_reference_id(Some(item.id), &item.name)?;
                    if item.name.trim().is_empty() {
                        bail!("cycle name cannot be empty");
                    }
                }
                "labels" => {
                    let item: Label = serde_json::from_value(record.clone())?;
                    normalized_reference_id(Some(item.id), &item.name)?;
                    if item.name.trim().is_empty() {
                        bail!("label name cannot be empty");
                    }
                }
                "agent_sessions" => {
                    let item: AgentSession = serde_json::from_value(record.clone())?;
                    normalized_reference_id(Some(item.id), &item.title)?;
                    if item.title.trim().is_empty() {
                        bail!("session title cannot be empty");
                    }
                }
                "events" => {
                    let item: StoreEvent = serde_json::from_value(record.clone())?;
                    if item.kind.trim().is_empty() {
                        bail!("event kind cannot be empty");
                    }
                    continue;
                }
                _ => unreachable!(),
            }
            let id = identity(name, record)?;
            if !ids.insert(id.to_ascii_lowercase()) {
                bail!("legacy {name} contains duplicate or case-alias identities");
            }
            if matches!(name, "issues" | "agent_sessions") {
                let path = record_path(name, id)?;
                if !all_paths.insert(path.to_string_lossy().to_ascii_lowercase()) {
                    bail!("legacy record identities collide after filename conversion");
                }
            }
            // Validate even unknown fields against the legacy TOML format.
            serialize_record(record)?;
        }
    }
    Ok(())
}

fn serialize_record(record: &Value) -> Result<Vec<u8>> {
    let value: toml::Value = serde_json::from_value(record.clone())
        .context("legacy TOML cannot represent this JSON value without loss")?;
    let bytes = toml::to_string_pretty(&value)?.into_bytes();
    if bytes.len() > RECORD_LIMIT {
        bail!("serialized legacy record exceeds 2 MiB");
    }
    Ok(bytes)
}

fn serialize_document(document: &Value) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    for name in COLLECTIONS {
        let records = rows(document, name)?;
        match name {
            "issues" | "agent_sessions" => {
                for record in records {
                    files.insert(
                        record_path(name, identity(name, record)?)?,
                        serialize_record(record)?,
                    );
                }
            }
            "events" => {
                let mut bytes = Vec::new();
                for event in records {
                    let line = serde_json::to_vec(event)?;
                    if line.len() > RECORD_LIMIT {
                        bail!("serialized legacy event exceeds 2 MiB");
                    }
                    bytes.extend_from_slice(&line);
                    bytes.push(b'\n');
                }
                files.insert(PathBuf::from("events.jsonl"), bytes);
            }
            _ => {
                files.insert(
                    PathBuf::from(format!("{name}.toml")),
                    serialize_record(&json!({name:records}))?,
                );
            }
        }
    }
    if files.values().map(Vec::len).sum::<usize>() > TOTAL_LIMIT {
        bail!("serialized legacy import exceeds 64 MiB");
    }
    if files.len() > 10_000 {
        bail!("serialized legacy import exceeds 10,000 files");
    }
    Ok(files)
}
