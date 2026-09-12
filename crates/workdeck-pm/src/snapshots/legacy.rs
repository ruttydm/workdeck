//! Strict recognition of the prototype's export shapes. Exact original bytes
//! are retained; private rows are never user-deserializable admission tokens.
use super::*;
use crate::{migration::MigrationKind, transactions::Snapshot};
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value;

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LegacyExportFormat {
    Json,
    Jsonl,
}

#[derive(Debug, Clone)]
pub enum ImportSource {
    Native(NativeSnapshot),
    Legacy(LegacyExport),
}

#[derive(Clone)]
pub struct LegacyExport {
    pub(super) raw: Vec<u8>,
    pub(super) format: LegacyExportFormat,
    pub(super) hash: ContentHash,
    pub(super) rows: Vec<LegacyRow>,
    root: Option<String>,
}
impl std::fmt::Debug for LegacyExport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LegacyExport")
            .field("format", &self.format)
            .field("content_hash", &self.hash)
            .field("bytes", &self.raw.len())
            .field("records", &self.rows.len())
            .finish()
    }
}
impl LegacyExport {
    pub fn content_hash(&self) -> &ContentHash {
        &self.hash
    }
    pub fn format(&self) -> LegacyExportFormat {
        self.format
    }
    pub fn raw_bytes(&self) -> &[u8] {
        &self.raw
    }
    pub fn record_count(&self) -> usize {
        self.rows.len()
    }
    /// Normalize only validated framing; row values and extras remain exact JSON values.
    pub fn canonical_document(&self) -> Value {
        let mut fields = serde_json::Map::new();
        if let Some(root) = &self.root {
            fields.insert("repo_root".into(), Value::String(root.clone()));
        }
        for (name, kind) in COLLECTIONS {
            fields.insert(
                (*name).into(),
                Value::Array(
                    self.rows
                        .iter()
                        .filter(|row| &row.kind == kind)
                        .map(|row| row.value.clone())
                        .collect(),
                ),
            );
        }
        Value::Object(fields)
    }
    pub fn source_path(&self) -> PathBuf {
        let extension = if self.format == LegacyExportFormat::Json {
            "json"
        } else {
            "jsonl"
        };
        PathBuf::from(format!(
            "imported-history/exports/{}.{extension}",
            self.hash
        ))
    }
}
#[derive(Debug, Clone)]
pub(super) struct LegacyRow {
    pub kind: MigrationKind,
    pub selector: String,
    pub value: Value,
}

pub fn decode_transfer(bytes: &[u8]) -> Result<ImportSource> {
    if bytes.len() > MAX_SNAPSHOT_INPUT_BYTES {
        return Err(unsupported("transfer input exceeds 48 MiB"));
    }
    let single = unique_json(bytes);
    if let Ok(value) = &single {
        if native_shape(value) {
            return decode_snapshot(bytes).map(ImportSource::Native);
        }
        // A one-frame JSONL repository header is a valid explicit empty export.
        if value.get("kind").and_then(Value::as_str) != Some("repo") {
            let rows = parse_object(value)?;
            return Ok(ImportSource::Legacy(LegacyExport {
                raw: bytes.to_vec(),
                format: LegacyExportFormat::Json,
                hash: ContentHash::of(bytes),
                root: root_annotation(value),
                rows,
            }));
        }
    }
    let text = std::str::from_utf8(bytes).map_err(|_| invalid("transfer input must be UTF-8"))?;
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .ok_or_else(|| invalid("empty transfer input"))?;
    let header = unique_json(first.as_bytes()).map_err(|_| {
        single
            .err()
            .unwrap_or_else(|| invalid("invalid transfer framing"))
    })?;
    if header.get("kind").and_then(Value::as_str) == Some("snapshot") {
        // Validate duplicate keys in every frame before the native decoder.
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            unique_json(line.as_bytes())?;
        }
        return decode_snapshot(bytes).map(ImportSource::Native);
    }
    let rows = parse_lines(text)?;
    Ok(ImportSource::Legacy(LegacyExport {
        raw: bytes.to_vec(),
        format: LegacyExportFormat::Jsonl,
        hash: ContentHash::of(bytes),
        root: header
            .pointer("/payload/root")
            .and_then(Value::as_str)
            .map(str::to_owned),
        rows,
    }))
}
fn root_annotation(value: &Value) -> Option<String> {
    value
        .get("repo_root")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or_else(|| value.get("data").and_then(root_annotation))
        .or_else(|| value.get("result").and_then(root_annotation))
}
fn native_shape(value: &Value) -> bool {
    value.get("format").is_some()
        || value.get("files").is_some()
        || value.get("data").is_some_and(native_shape)
        || value.get("result").is_some_and(native_shape)
}
const COLLECTIONS: &[(&str, MigrationKind)] = &[
    ("issues", MigrationKind::Issue),
    ("projects", MigrationKind::Project),
    ("cycles", MigrationKind::Cycle),
    ("labels", MigrationKind::Labels),
    ("agent_sessions", MigrationKind::ImportedSession),
    ("events", MigrationKind::ImportedEvents),
];
fn parse_object(value: &Value) -> Result<Vec<LegacyRow>> {
    let mut value = value;
    let mut prefix = String::new();
    if value.get("ok").is_some() {
        let envelope = value
            .as_object()
            .ok_or_else(|| invalid("export envelope must be an object"))?;
        if value.get("ok") != Some(&Value::Bool(true))
            || value.get("kind").and_then(Value::as_str) != Some("export")
        {
            return Err(invalid(
                "legacy import requires a successful export envelope",
            ));
        }
        for key in envelope.keys() {
            if !["ok", "kind", "data", "result", "api_version", "source"].contains(&key.as_str()) {
                return Err(invalid(format!("unknown export envelope field {key:?}")));
            }
        }
        if let Some(version) = value.get("api_version")
            && version.as_u64() != Some(1)
        {
            return Err(unsupported("unsupported export envelope version"));
        }
        if value.get("data").is_some() == value.get("result").is_some() {
            return Err(invalid(
                "export envelope requires exactly one data/result payload",
            ));
        }
        let field = if value.get("data").is_some() {
            "data"
        } else {
            "result"
        };
        prefix = format!("/{field}");
        value = &value[field];
    }
    let fields = value
        .as_object()
        .ok_or_else(|| invalid("legacy export payload must be an object"))?;
    if !COLLECTIONS
        .iter()
        .any(|(name, _)| fields.contains_key(*name))
    {
        return Err(invalid(
            "unrecognized export payload; expected legacy collections or native snapshot",
        ));
    }
    for key in fields.keys() {
        if key != "repo_root" && !COLLECTIONS.iter().any(|(name, _)| key == name) {
            return Err(unsupported(format!(
                "unsupported legacy export collection {key:?}"
            )));
        }
    }
    if fields
        .get("repo_root")
        .is_some_and(|value| !value.is_string())
    {
        return Err(invalid("legacy repo_root must be a string annotation"));
    }
    let mut rows = Vec::new();
    for (name, kind) in COLLECTIONS {
        if let Some(values) = fields.get(*name) {
            let values = values
                .as_array()
                .ok_or_else(|| invalid(format!("legacy {name} must be an array")))?;
            for (index, value) in values.iter().enumerate() {
                push_row(
                    &mut rows,
                    kind.clone(),
                    format!("{prefix}/{name}/{index}"),
                    value.clone(),
                )?;
            }
        }
    }
    Ok(rows)
}
fn parse_lines(text: &str) -> Result<Vec<LegacyRow>> {
    let mut rows = Vec::new();
    let mut header_seen = false;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let frame = unique_json(line.as_bytes()).map_err(|mut error| {
            error.line = Some(index + 1);
            error
        })?;
        let object = frame
            .as_object()
            .ok_or_else(|| invalid("JSONL frame must be an object"))?;
        if object.len() != 2 || !object.contains_key("kind") || !object.contains_key("payload") {
            return Err(invalid("JSONL frame requires exactly kind and payload"));
        }
        let kind = frame["kind"]
            .as_str()
            .ok_or_else(|| invalid("JSONL kind must be a string"))?;
        if !header_seen {
            if kind != "repo" {
                return Err(invalid("legacy JSONL requires its repository header first"));
            }
            let header = frame["payload"]
                .as_object()
                .ok_or_else(|| invalid("repository header must be an object"))?;
            if header.len() != 1 || !header.get("root").is_some_and(Value::is_string) {
                return Err(invalid(
                    "legacy repository header requires its root annotation",
                ));
            }
            header_seen = true;
            continue;
        }
        let kind = match kind {
            "issue" => MigrationKind::Issue,
            "project" => MigrationKind::Project,
            "cycle" => MigrationKind::Cycle,
            "label" => MigrationKind::Labels,
            "agent_session" => MigrationKind::ImportedSession,
            "event" => MigrationKind::ImportedEvents,
            "repo" => return Err(invalid("duplicate legacy JSONL repository header")),
            other => {
                return Err(unsupported(format!(
                    "unsupported legacy JSONL kind {other:?}"
                )));
            }
        };
        push_row(
            &mut rows,
            kind,
            format!("line:{}/payload", index + 1),
            frame["payload"].clone(),
        )?;
    }
    if !header_seen {
        return Err(invalid("missing legacy JSONL repository header"));
    }
    Ok(rows)
}
fn push_row(
    rows: &mut Vec<LegacyRow>,
    kind: MigrationKind,
    selector: String,
    value: Value,
) -> Result<()> {
    if !value.is_object() {
        return Err(invalid(format!(
            "legacy record {selector} must be an object"
        )));
    }
    if rows.len() >= MAX_SNAPSHOT_FILES {
        return Err(unsupported("legacy export exceeds 4096 records"));
    }
    rows.push(LegacyRow {
        kind,
        selector,
        value,
    });
    Ok(())
}

pub(crate) fn inspect_export_artifacts(
    root: &Path,
    snapshot: &Snapshot<'_>,
) -> (usize, Vec<PmError>) {
    let mut errors = Vec::new();
    let mut count = 0;
    let mut exports = BTreeMap::new();
    let paths =
        match snapshot.list_bounded(Path::new("imported-history/exports"), MAX_SNAPSHOT_ENTRIES) {
            Ok(paths) => paths,
            Err(error) => return (0, vec![error]),
        };
    for path in paths {
        count += 1;
        let result = (|| {
            let bytes = snapshot
                .read_bounded(&path, MAX_SNAPSHOT_CONTENT_BYTES)?
                .ok_or_else(|| invalid("retained export disappeared"))?;
            let ImportSource::Legacy(source) = decode_transfer(&bytes)? else {
                return Err(invalid("retained legacy artifact is not a legacy export"));
            };
            if source.source_path() != path {
                return Err(invalid(
                    "retained export filename, format, or source hash does not match its bytes",
                ));
            }
            exports.insert(path.clone(), source);
            Ok(())
        })();
        if let Err(error) = result {
            errors.push(error.at(root.join(path)));
        }
    }
    super::origins::inspect(root, snapshot, &exports, &mut errors);
    (count, errors)
}

// serde_json::Value normally accepts duplicate keys, silently discarding the
// earlier value. Transfer input must reject that ambiguity at every depth.
struct UniqueValue(Value);
impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueValue;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON value with unique object keys")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|v| UniqueValue(Value::Number(v)))
                    .ok_or_else(|| E::custom("nonfinite JSON number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::String(v)))
            }
            fn visit_unit<E: de::Error>(self) -> std::result::Result<Self::Value, E> {
                Ok(UniqueValue(Value::Null))
            }
            fn visit_seq<A: SeqAccess<'de>>(
                self,
                mut seq: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(UniqueValue(value)) = seq.next_element()? {
                    values.push(value);
                }
                Ok(UniqueValue(Value::Array(values)))
            }
            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> std::result::Result<Self::Value, A::Error> {
                let mut values = serde_json::Map::new();
                while let Some((key, UniqueValue(value))) =
                    map.next_entry::<String, UniqueValue>()?
                {
                    if values.insert(key.clone(), value).is_some() {
                        return Err(de::Error::custom(format!("duplicate JSON key {key:?}")));
                    }
                }
                Ok(UniqueValue(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}
fn unique_json(bytes: &[u8]) -> Result<Value> {
    serde_json::from_slice::<UniqueValue>(bytes)
        .map(|value| value.0)
        .map_err(|error| invalid(format!("invalid transfer JSON: {error}")))
}
