use super::*;
use crate::{RepositoryId, SchemaVersion};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const FORMAT: &str = "workdeck.native-snapshot";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StreamHeader {
    format: String,
    version: SchemaVersion,
    repository: RepositoryId,
    fingerprint: ContentHash,
    file_count: usize,
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    content = "payload",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum StreamFrame {
    Snapshot(StreamHeader),
    File(SnapshotFile),
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotKind {
    Attestation,
    ContractReview,
    RunIntent,
    Claim,
    CoordinationMarker,
    RunResult,
    CommandDefinition,
    CheckDefinition,
    CheckProfile,
    Question,
    Handoff,
    Gate,
    Evidence,
    Configuration,
    Users,
    OrganizationSchema,
    Issue,
    Comment,
    TimeEntry,
    IssueRelation,
    PrerequisiteWaiver,
    AttachmentMetadata,
    AttachmentContent,
    Initiative,
    Project,
    Milestone,
    Cycle,
    Target,
    Labels,
    IssueTemplate,
    Wiki,
    SavedView,
    Feature,
    FeatureRelation,
    Tombstone,
    Operation,
    Migration,
    ImportedSession,
    ImportedHistory,
    ImportedHandoff,
}

#[derive(schemars::JsonSchema, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFile {
    pub kind: SnapshotKind,
    pub path: PathBuf,
    pub content_hash: ContentHash,
    /// Exact file bytes, encoded as canonical standard base64 in JSON.
    #[serde(with = "encoded")]
    #[schemars(with = "String")]
    pub content: Vec<u8>,
}

impl std::fmt::Debug for SnapshotFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SnapshotFile")
            .field("kind", &self.kind)
            .field("path", &self.path)
            .field("content_hash", &self.content_hash)
            .field("content_bytes", &self.content.len())
            .finish()
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSnapshot {
    pub format: String,
    pub version: SchemaVersion,
    pub repository: RepositoryId,
    pub files: Vec<SnapshotFile>,
    pub fingerprint: ContentHash,
}

impl NativeSnapshot {
    /// One required header followed by one exact file frame per manifest entry.
    /// These frames are distinct from legacy issue/session JSONL records.
    pub fn to_jsonl(&self) -> Result<String> {
        self.validate()?;
        let header = StreamHeader {
            format: self.format.clone(),
            version: self.version,
            repository: self.repository.clone(),
            fingerprint: self.fingerprint.clone(),
            file_count: self.files.len(),
        };
        let mut output =
            serde_json::to_string(&serde_json::json!({"kind":"snapshot","payload":header}))
                .map_err(|error| invalid(error.to_string()))?;
        output.push('\n');
        for file in &self.files {
            output.push_str(
                &serde_json::to_string(&serde_json::json!({"kind":"file","payload":file}))
                    .map_err(|error| invalid(error.to_string()))?,
            );
            output.push('\n');
            if output.len() > MAX_SNAPSHOT_INPUT_BYTES {
                return Err(unsupported("snapshot JSONL exceeds 48 MiB"));
            }
        }
        Ok(output)
    }

    pub(super) fn build(
        repository: RepositoryId,
        files: BTreeMap<PathBuf, Vec<u8>>,
    ) -> Result<Self> {
        let files = files
            .into_iter()
            .map(|(path, content)| {
                let kind = validation::classify(&path)?
                    .ok_or_else(|| invalid("non-PM paths cannot be included in a snapshot"))?;
                Ok(SnapshotFile {
                    kind,
                    path,
                    content_hash: ContentHash::of(&content),
                    content,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let mut snapshot = Self {
            format: FORMAT.into(),
            version: SchemaVersion::CURRENT,
            repository,
            files,
            fingerprint: ContentHash::of(&[]),
        };
        snapshot.fingerprint = snapshot.calculate_fingerprint()?;
        Ok(snapshot)
    }

    pub fn validate(&self) -> Result<()> {
        let files = self.checked_files()?;
        validation::validate_files(&files, &self.repository)
    }

    pub(super) fn calculate_fingerprint(&self) -> Result<ContentHash> {
        let files = self
            .files
            .iter()
            .map(|file| {
                (
                    &file.kind,
                    &file.path,
                    &file.content_hash,
                    file.content.len(),
                )
            })
            .collect::<Vec<_>>();
        hash(&(self.format.as_str(), self.version, &self.repository, files))
    }

    pub(crate) fn checked_files(&self) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
        if self.format != FORMAT {
            return Err(unsupported("unsupported snapshot format"));
        }
        if self.files.len() > MAX_SNAPSHOT_FILES {
            return Err(unsupported("snapshot exceeds 4096 files"));
        }
        let mut files = BTreeMap::new();
        let mut portable = std::collections::BTreeSet::new();
        let mut total = 0usize;
        for file in &self.files {
            if validation::classify(&file.path)? != Some(file.kind) {
                return Err(
                    invalid("snapshot kind disagrees with its canonical path").at(&file.path)
                );
            }
            if file.content.len() > validation::file_limit(file.kind) {
                return Err(
                    unsupported("snapshot file exceeds its record-kind size limit").at(&file.path),
                );
            }
            total = total
                .checked_add(file.content.len())
                .ok_or_else(|| invalid("snapshot size overflow"))?;
            if total > MAX_SNAPSHOT_CONTENT_BYTES {
                return Err(unsupported("snapshot exceeds 32 MiB decoded content"));
            }
            if ContentHash::of(&file.content) != file.content_hash {
                return Err(
                    invalid("snapshot content hash does not match its bytes").at(&file.path)
                );
            }
            if !portable.insert(file.path.to_string_lossy().to_lowercase())
                || files
                    .insert(file.path.clone(), file.content.clone())
                    .is_some()
            {
                return Err(invalid("duplicate or case-colliding snapshot paths").at(&file.path));
            }
        }
        for path in &portable {
            if Path::new(path).ancestors().skip(1).any(|parent| {
                parent
                    .to_str()
                    .is_some_and(|parent| portable.contains(parent))
            }) {
                return Err(invalid("snapshot file is also another file's parent").at(path));
            }
        }
        if self
            .files
            .windows(2)
            .any(|pair| pair[0].path >= pair[1].path)
        {
            return Err(invalid(
                "snapshot files must use canonical sorted path order",
            ));
        }
        if self.calculate_fingerprint()? != self.fingerprint {
            return Err(invalid("snapshot fingerprint does not match its manifest"));
        }
        Ok(files)
    }
}

/// Native bare objects and the two existing success envelope shapes are
/// deliberate formats. Legacy exports are recognized but never read as empty.
pub fn decode_snapshot(bytes: &[u8]) -> Result<NativeSnapshot> {
    if bytes.len() > MAX_SNAPSHOT_INPUT_BYTES {
        return Err(unsupported("snapshot JSON exceeds 48 MiB"));
    }
    let text = std::str::from_utf8(bytes).map_err(|error| invalid(error.to_string()))?;
    if let Some(first) = text
        .lines()
        .next()
        .and_then(|line| serde_json::from_str::<Value>(line).ok())
    {
        if matches!(
            first.get("kind").and_then(Value::as_str),
            Some("repo" | "issue" | "project" | "cycle" | "label" | "agent_session" | "event")
        ) {
            return Err(unsupported(
                "legacy JSONL export was recognized; typed legacy conversion is not implemented",
            ));
        }
        if first.get("kind").is_some() && first.get("ok").is_none() {
            return decode_jsonl(text);
        }
    }
    let probe: Value = serde_json::from_slice(bytes).map_err(|error| {
        invalid(format!(
            "expected one JSON snapshot object or a framed native JSONL stream: {error}"
        ))
    })?;
    let payload = if probe.get("ok").is_some() {
        if probe.get("ok") != Some(&Value::Bool(true))
            || probe.get("kind").and_then(Value::as_str) != Some("export")
        {
            return Err(invalid("only successful export envelopes are importable"));
        }
        if probe.get("data").is_some() == probe.get("result").is_some() {
            return Err(invalid(
                "export envelope requires exactly one data or result payload",
            ));
        }
        probe
            .get("data")
            .or_else(|| probe.get("result"))
            .expect("one checked")
    } else {
        &probe
    };
    if payload.get("format").is_none() && payload.get("issues").is_some() {
        return Err(unsupported(
            "legacy JSON export was recognized; it requires the explicit shared migration conversion increment",
        ));
    }
    if payload.get("format").and_then(Value::as_str) != Some(FORMAT) {
        return Err(invalid("unrecognized snapshot shape"));
    }
    if let Some(version) = payload.get("version").and_then(Value::as_u64) {
        SchemaVersion::try_from(version)?;
    }
    let snapshot: NativeSnapshot = if probe.get("ok").is_some() {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Envelope {
            #[serde(default)]
            api_version: Option<u64>,
            ok: bool,
            kind: String,
            #[serde(default)]
            source: Option<Value>,
            #[serde(default)]
            data: Option<NativeSnapshot>,
            #[serde(default)]
            result: Option<NativeSnapshot>,
            #[serde(default)]
            action: Option<String>,
        }
        let envelope: Envelope =
            serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
        if envelope.api_version.is_some_and(|version| version != 1) {
            return Err(unsupported("unsupported export envelope API version"));
        }
        if !envelope.ok || envelope.kind != "export" || envelope.action.is_some() {
            return Err(invalid("invalid export envelope"));
        }
        let snapshot = envelope.data.or(envelope.result).expect("payload checked");
        if let Some(repository) = envelope
            .source
            .as_ref()
            .and_then(|source| source.get("repository"))
            .filter(|repository| !repository.is_null())
            && repository.as_str() != Some(snapshot.repository.to_string().as_str())
        {
            return Err(invalid(
                "export envelope and snapshot repository identities disagree",
            ));
        }
        snapshot
    } else {
        serde_json::from_slice(bytes).map_err(|error| invalid(error.to_string()))?
    };
    snapshot.validate()?;
    Ok(snapshot)
}

fn decode_jsonl(text: &str) -> Result<NativeSnapshot> {
    let mut lines = text.lines().enumerate();
    let (_, first) = lines
        .next()
        .ok_or_else(|| invalid("snapshot JSONL is empty"))?;
    let probe: Value = serde_json::from_str(first).map_err(|error| invalid(error.to_string()))?;
    if let Some(version) = probe.pointer("/payload/version").and_then(Value::as_u64) {
        SchemaVersion::try_from(version)?;
    }
    let StreamFrame::Snapshot(header) = serde_json::from_str(first)
        .map_err(|error| invalid(format!("invalid first snapshot JSONL frame: {error}")))?
    else {
        return Err(invalid(
            "snapshot JSONL requires its header exactly once and first",
        ));
    };
    if header.file_count > MAX_SNAPSHOT_FILES {
        return Err(unsupported("snapshot JSONL header exceeds 4096 files"));
    }
    let mut files = Vec::new();
    for (index, line) in lines {
        let frame: StreamFrame = serde_json::from_str(line).map_err(|error| {
            let mut error = invalid(format!("invalid snapshot JSONL frame: {error}"));
            error.line = Some(index + 1);
            error
        })?;
        match frame {
            StreamFrame::File(file) => files.push(file),
            StreamFrame::Snapshot(_) => {
                return Err(invalid("snapshot JSONL has more than one header"));
            }
        }
        if files.len() > header.file_count {
            return Err(invalid(
                "snapshot JSONL contains more files than its header declares",
            ));
        }
    }
    if files.len() != header.file_count {
        return Err(invalid(
            "snapshot JSONL is truncated or its file count is incorrect",
        ));
    }
    let snapshot = NativeSnapshot {
        format: header.format,
        version: header.version,
        repository: header.repository,
        fingerprint: header.fingerprint,
        files,
    };
    snapshot.validate()?;
    Ok(snapshot)
}

mod encoded {
    use super::*;
    pub fn serialize<S: serde::Serializer>(
        bytes: &[u8],
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }
    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Vec<u8>, D::Error> {
        let text = String::deserialize(deserializer)?;
        if text.len() > MAX_SNAPSHOT_INPUT_BYTES {
            return Err(serde::de::Error::custom(
                "encoded snapshot file exceeds limit",
            ));
        }
        STANDARD.decode(text).map_err(serde::de::Error::custom)
    }
}
