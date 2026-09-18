//! Recorded session and event annotations. None of these records establish a
//! live process, a claim, a check outcome, or completion qualification.
use crate::{
    ContentHash, ErrorCode, PmError, Repository, RepositoryId, RequestId, Result, SchemaVersion,
    Timestamp,
    repository::config_from_snapshot,
    transactions::{FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_RECORDS: usize = 10_000;

/// Validate descriptor-captured historical files without reopening their paths.
/// The caller supplies config.yml plus canonical session, retirement and receipt
/// files from one bound repository snapshot; no filesystem access occurs here.
pub fn validate_recorded_sessions_snapshot(
    root: &Path,
    expected_repository: &RepositoryId,
    files: &BTreeMap<PathBuf, Vec<u8>>,
) -> Result<Vec<SessionRecord>> {
    if files.len() > MAX_RECORDS {
        return Err(invalid("historical snapshot exceeds 10,000 files"));
    }
    let mut total = 0usize;
    let mut paths = BTreeSet::new();
    for (path, bytes) in files {
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("historical snapshot size overflow"))?;
        if total > 64 * 1024 * 1024 {
            return Err(invalid("historical snapshot exceeds 64 MiB"));
        }
        let text = path
            .to_str()
            .ok_or_else(|| schema("historical paths must be UTF-8").at(path))?;
        let canonical = text == "config.yml"
            || path
                .file_stem()
                .and_then(|value| value.to_str())
                .is_some_and(|id| {
                    session_path(id).is_ok_and(|expected| expected.to_str() == Some(text))
                        || marker_path(id).is_ok_and(|expected| expected.to_str() == Some(text))
                        || id
                            .parse::<crate::OperationId>()
                            .is_ok_and(|id| text == format!("operations/{id}.yml"))
                });
        if !canonical || !paths.insert(text.to_ascii_lowercase()) {
            return Err(schema("historical snapshot requires unique canonical paths").at(path));
        }
    }
    let snapshot = Snapshot::from_memory(root, files);
    let config = config_from_snapshot(root, &snapshot)?;
    if &config.repository != expected_repository {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "historical snapshot belongs to another repository",
        )
        .at(root.join("config.yml")));
    }
    load_sessions(root, &snapshot, expected_repository)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedSession {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub agent: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default)]
    pub ended_at: String,
    #[serde(default)]
    pub goal: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub plan: Vec<String>,
    #[serde(default)]
    pub commands_run: Vec<String>,
    #[serde(default)]
    pub tests_run: Vec<String>,
    #[serde(default)]
    pub handoff_notes: Vec<String>,
    #[serde(default)]
    pub touched_files: Vec<RecordedFile>,
    #[serde(default, flatten, skip_serializing_if = "toml::Table::is_empty")]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordedFile {
    pub path: String,
    #[serde(default)]
    pub change_type: String,
    #[serde(default, flatten, skip_serializing_if = "toml::Table::is_empty")]
    pub extra: toml::Table,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRecord {
    pub session: RecordedSession,
    pub path: PathBuf,
    pub source: ContentHash,
    pub retired: bool,
    pub evidence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<SessionRetirement>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewRecordedSession {
    pub id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionCollection {
    Plan,
    Commands,
    Tests,
    Notes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum SessionMutation {
    Update {
        fields: BTreeMap<String, Value>,
    },
    Finish {
        summary: Option<String>,
    },
    Append {
        field: SessionCollection,
        text: String,
    },
    AddFile {
        path: String,
        change_type: String,
    },
    Delete,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionRetirement {
    schema: SchemaVersion,
    repository: RepositoryId,
    id: String,
    content: ContentHash,
    request_id: RequestId,
    retired_at: Timestamp,
    record_hash: ContentHash,
}

impl Repository {
    pub fn recorded_sessions(&self) -> Result<Vec<SessionRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            config_from_snapshot(self.root(), snapshot)?;
            load_sessions(self.root(), snapshot, self.identity())
        })
    }

    pub fn recorded_session(&self, id: &str) -> Result<SessionRecord> {
        self.store()?.with_snapshot(|snapshot| {
            config_from_snapshot(self.root(), snapshot)?;
            load_session(self.root(), snapshot, self.identity(), id)
        })
    }

    pub fn create_recorded_session(
        &self,
        input: &NewRecordedSession,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        if input.title.len() > MAX_BYTES {
            return Err(invalid("recorded session title exceeds 2 MiB"));
        }
        bounded_fields(&input.fields)?;
        let input_value = serde_json::to_value(input).map_err(|e| invalid(e.to_string()))?;
        bounded_json(&input_value)?;
        let receipt = self.store()?.transact_with_preflight(
            request,
            "history.session.create",
            &input_value,
            history_layout,
            |snapshot| {
                config_from_snapshot(self.root(), snapshot)?;
                let index = HistoryIndex::load(snapshot, self.identity())?;
                let id = match &input.id {
                    Some(id) => {
                        index.ensure_unused(id)?;
                        id.clone()
                    }
                    None => generated_id(&index, || format!("SES-{}", ulid::Ulid::new()))?,
                };
                let mut fields = input.fields.clone();
                if fields.contains_key("id") || fields.contains_key("title") {
                    return Err(invalid("id and title are dedicated session fields"));
                }
                fields.insert("id".into(), json!(id));
                fields.insert("title".into(), json!(input.title));
                fields.entry("status".into()).or_insert(json!("active"));
                fields
                    .entry("started_at".into())
                    .or_insert(json!(chrono::Utc::now().to_rfc3339()));
                let session: RecordedSession =
                    serde_json::from_value(json!(fields)).map_err(|e| invalid(e.to_string()))?;
                let bytes = serialize(&session)?;
                let record = parse_session(&session_path(&id)?, &bytes, false)?;
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: record.path.clone(),
                        expected: None,
                        content: Some(bytes),
                    }],
                    result: json!(record),
                })
            },
        )?;
        let records = validate_history_receipt(&receipt, self.identity())?;
        let record = &records[0];
        if input.id.as_ref().is_some_and(|id| id != &record.session.id)
            || record.session.title != input.title
            || input.id.is_none()
                && record
                    .session
                    .id
                    .strip_prefix("SES-")
                    .is_none_or(|id| id.parse::<ulid::Ulid>().is_err())
            || input
                .fields
                .iter()
                .any(|(key, value)| receipt.result["session"].get(key) != Some(value))
            || !input.fields.contains_key("status") && record.session.status != "active"
            || !input.fields.contains_key("started_at")
                && chrono::DateTime::parse_from_rfc3339(&record.session.started_at).is_err()
        {
            return Err(corrupt("session creation receipt differs from its input"));
        }
        Ok(receipt)
    }

    pub fn import_recorded_sessions(
        &self,
        sessions: &[RecordedSession],
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        if sessions.is_empty() || sessions.len() > MAX_RECORDS {
            return Err(invalid(
                "session import requires between one and 10,000 records",
            ));
        }
        let mut total = 0usize;
        let mut encoded = Vec::with_capacity(sessions.len());
        let mut seen = BTreeSet::new();
        for session in sessions {
            if !seen.insert(session.id.to_ascii_lowercase()) {
                return Err(invalid("session import repeats a portable identity"));
            }
            let bytes = serialize(session)?;
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| invalid("session import size overflow"))?;
            if total > 32 * 1024 * 1024 {
                return Err(invalid("session import exceeds 32 MiB"));
            }
            encoded.push(bytes);
        }
        let receipt = self.store()?.transact_with_preflight(
            request,
            "history.session.import",
            &json!(sessions),
            history_layout,
            |snapshot| {
                config_from_snapshot(self.root(), snapshot)?;
                let index = HistoryIndex::load(snapshot, self.identity())?;
                let mut changes = Vec::with_capacity(sessions.len());
                let mut records = Vec::with_capacity(sessions.len());
                for (session, bytes) in sessions.iter().zip(encoded) {
                    index.ensure_unused(&session.id)?;
                    let record = parse_session(&session_path(&session.id)?, &bytes, false)?;
                    changes.push(FileChange {
                        path: record.path.clone(),
                        expected: None,
                        content: Some(bytes),
                    });
                    records.push(record);
                }
                Ok(PreparedOperation {
                    changes,
                    result: json!(records),
                })
            },
        )?;
        let records = validate_history_receipt(&receipt, self.identity())?;
        if records.len() != sessions.len()
            || records
                .iter()
                .zip(sessions)
                .any(|(record, session)| record.session != *session)
        {
            return Err(corrupt("session import receipt differs from its input"));
        }
        Ok(receipt)
    }

    pub fn mutate_recorded_session(
        &self,
        id: &str,
        expected: Option<&ContentHash>,
        mutation: &SessionMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        session_path(id)?;
        if let SessionMutation::Update { fields } = mutation {
            bounded_fields(fields)?;
        }
        let input = json!({"id":id,"expected":expected,"mutation":mutation});
        bounded_json(&input)?;
        let receipt = self.store()?.transact_with_preflight(request, "history.session.mutate", &input, history_layout, |snapshot| {
            config_from_snapshot(self.root(), snapshot)?;
            let original = load_session(self.root(), snapshot, self.identity(), id)?;
            if original.retired { return Err(PmError::new(ErrorCode::PolicyBlocked, "recorded session is retired; its history is read-only")); }
            if expected.is_some_and(|expected| expected != &original.source) {
                return Err(PmError::new(ErrorCode::StaleSource, "recorded session content changed").at(&original.path));
            }
            if matches!(mutation, SessionMutation::Delete) {
                let mut retired = original;
                retired.retired = true;
                let marker = SessionRetirement { schema: SchemaVersion::CURRENT, repository: self.identity().clone(), id: id.into(),
                    content: retired.source.clone(), request_id: request.clone(), retired_at: chrono::Utc::now(), record_hash: record_hash(&retired)? };
                retired.retirement = Some(marker.clone());
                return Ok(PreparedOperation { changes: vec![FileChange { path: marker_path(id)?, expected: None, content: Some(marker_bytes(&marker)?) }], result: json!(retired) });
            }
            let mut session = original.session.clone();
            match mutation {
                SessionMutation::Update { fields } => {
                    for (key, value) in fields {
                        let target = match key.as_str() {
                            "title" => &mut session.title, "agent" => &mut session.agent, "status" => &mut session.status,
                            "goal" => &mut session.goal, "summary" => &mut session.summary, "cwd" => &mut session.cwd,
                            _ => return Err(invalid("session update supports title, agent, status, goal, summary and cwd")),
                        };
                        *target = value.as_str().ok_or_else(|| invalid("session update values must be strings"))?.to_owned();
                    }
                }
                SessionMutation::Finish { summary } => {
                    session.status = "done".into(); session.ended_at = chrono::Utc::now().to_rfc3339();
                    if let Some(summary) = summary { session.summary = summary.clone(); }
                }
                SessionMutation::Append { field, text } => {
                    if text.trim().is_empty() { return Err(invalid("historical annotation cannot be blank")); }
                    match field { SessionCollection::Plan => &mut session.plan, SessionCollection::Commands => &mut session.commands_run,
                        SessionCollection::Tests => &mut session.tests_run, SessionCollection::Notes => &mut session.handoff_notes }.push(text.clone());
                }
                SessionMutation::AddFile { path, change_type } => {
                    if path.trim().is_empty() { return Err(invalid("historical file path cannot be blank")); }
                    session.touched_files.push(RecordedFile { path: path.clone(), change_type: change_type.clone(), extra: toml::Table::new() });
                }
                SessionMutation::Delete => unreachable!(),
            }
            let bytes = patch_session(snapshot, &original, &session)?;
            let record = parse_session(&original.path, &bytes, false)?;
            if record.session != session { return Err(invalid("edited historical document does not preserve its semantic values")); }
            Ok(PreparedOperation { changes: vec![FileChange { path: original.path, expected: Some(original.source), content: Some(bytes) }], result: json!(record) })
        })?;
        let records = validate_history_receipt(&receipt, self.identity())?;
        let record = &records[0];
        let matches = match mutation {
            SessionMutation::Update { fields } => fields
                .iter()
                .all(|(key, value)| receipt.result["session"].get(key) == Some(value)),
            SessionMutation::Finish { summary } => {
                record.session.status == "done"
                    && chrono::DateTime::parse_from_rfc3339(&record.session.ended_at).is_ok()
                    && summary
                        .as_ref()
                        .is_none_or(|summary| summary == &record.session.summary)
            }
            SessionMutation::Append { field, text } => {
                match field {
                    SessionCollection::Plan => &record.session.plan,
                    SessionCollection::Commands => &record.session.commands_run,
                    SessionCollection::Tests => &record.session.tests_run,
                    SessionCollection::Notes => &record.session.handoff_notes,
                }
                .last()
                    == Some(text)
            }
            SessionMutation::AddFile { path, change_type } => record
                .session
                .touched_files
                .last()
                .is_some_and(|file| &file.path == path && &file.change_type == change_type),
            SessionMutation::Delete => record.retired,
        };
        let expected_matches = expected.is_none_or(|expected| {
            if record.retired {
                &record.source == expected
            } else {
                receipt.changed[0].before.as_ref() == Some(expected)
            }
        });
        if record.session.id != id
            || !matches
            || !expected_matches
            || record.retired != matches!(mutation, SessionMutation::Delete)
        {
            return Err(corrupt("session mutation receipt differs from its input"));
        }
        Ok(receipt)
    }

    pub fn historical_events(&self) -> Result<Vec<Value>> {
        self.store()?.with_snapshot(|snapshot| {
            config_from_snapshot(self.root(), snapshot)?;
            events(snapshot)
        })
    }
}

fn session_path(id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(invalid(
            "session identity must be 1–96 ASCII letters, digits, underscores or hyphens",
        ));
    }
    Ok(format!("imported-sessions/{id}.toml").into())
}
fn marker_path(id: &str) -> Result<PathBuf> {
    session_path(id)?;
    Ok(format!("imported-history/deleted-sessions/{id}.yml").into())
}
fn generated_id(index: &HistoryIndex, mut generate: impl FnMut() -> String) -> Result<String> {
    for _ in 0..16 {
        let id = generate();
        session_path(&id)?;
        if !index.reserved.contains(&id.to_ascii_lowercase()) {
            return Ok(id);
        }
    }
    Err(PmError::new(
        ErrorCode::Conflict,
        "could not allocate an unused recorded session identity",
    ))
}

/// Admit only layout here: historical retries must not depend on today's TOML.
/// Each prefix permits at most 10,000 traversed entries (directories included);
/// the three prefixes together permit at most 10,000 authoritative files.
fn history_layout(snapshot: &Snapshot<'_>) -> Result<()> {
    let mut total = 0usize;
    for directory in [
        "imported-sessions",
        "imported-history/deleted-sessions",
        "operations",
    ] {
        let mut seen = BTreeSet::new();
        for path in snapshot.list_bounded(Path::new(directory), MAX_RECORDS)? {
            total += 1;
            if total > MAX_RECORDS {
                return Err(invalid("historical source exceeds 10,000 files"));
            }
            let text = path
                .to_str()
                .ok_or_else(|| schema("historical paths must be UTF-8").at(&path))?;
            let id = path
                .file_stem()
                .and_then(|id| id.to_str())
                .ok_or_else(|| schema("invalid historical path").at(&path))?;
            let expected = match directory {
                "imported-sessions" => session_path(id)?,
                "operations" => {
                    format!("operations/{}.yml", id.parse::<crate::OperationId>()?).into()
                }
                _ => marker_path(id)?,
            };
            if expected.to_str() != Some(text) || !seen.insert(text.to_ascii_lowercase()) {
                return Err(schema("historical files require unique canonical paths").at(path));
            }
        }
    }
    Ok(())
}

#[derive(Default)]
struct HistoryIndex {
    reserved: BTreeSet<String>,
    retired: BTreeMap<String, SessionRecord>,
}
impl HistoryIndex {
    fn load(snapshot: &Snapshot<'_>, repository: &RepositoryId) -> Result<Self> {
        history_layout(snapshot)?;
        let mut index = Self::default();
        let mut spellings = BTreeMap::new();
        for directory in ["imported-sessions", "imported-history/deleted-sessions"] {
            for path in snapshot.list_bounded(Path::new(directory), MAX_RECORDS)? {
                let id = path
                    .file_stem()
                    .and_then(|id| id.to_str())
                    .expect("layout validated");
                reserve(&mut index.reserved, &mut spellings, id)?;
            }
        }
        let mut total = 0usize;
        let mut requests = BTreeSet::new();
        for path in snapshot.list_bounded(Path::new("operations"), MAX_RECORDS)? {
            let bytes = snapshot
                .read_bounded(&path, 64 * 1024 * 1024 - total)?
                .ok_or_else(|| corrupt("operation disappeared").at(&path))?;
            total += bytes.len();
            let receipt: MutationReceipt =
                serde_yaml_ng::from_slice(&bytes).map_err(|e| corrupt(e.to_string()).at(&path))?;
            crate::transactions::validate_receipt(&receipt)?;
            if path.to_str() != Some(format!("operations/{}.yml", receipt.operation_id).as_str())
                || receipt.repository.as_ref() != Some(repository)
                || !requests.insert(receipt.request_id.clone())
            {
                return Err(
                    corrupt("operation path, repository or request identity is invalid").at(path),
                );
            }
            if !is_history_operation(&receipt.operation) {
                continue;
            }
            for record in validate_history_receipt(&receipt, repository)? {
                reserve(&mut index.reserved, &mut spellings, &record.session.id)?;
                if record.retired
                    && index
                        .retired
                        .insert(record.session.id.to_ascii_lowercase(), record)
                        .is_some()
                {
                    return Err(corrupt("duplicate recorded session retirement").at(path));
                }
            }
        }
        Ok(index)
    }
    fn ensure_unused(&self, id: &str) -> Result<()> {
        session_path(id)?;
        if self.reserved.contains(&id.to_ascii_lowercase()) {
            return Err(PmError::new(
                if self.retired.contains_key(&id.to_ascii_lowercase()) {
                    ErrorCode::PolicyBlocked
                } else {
                    ErrorCode::Conflict
                },
                "recorded session identity remains reserved by retained history or a durable operation",
            ));
        }
        Ok(())
    }
}
fn reserve(
    reserved: &mut BTreeSet<String>,
    spellings: &mut BTreeMap<String, String>,
    id: &str,
) -> Result<()> {
    let portable = id.to_ascii_lowercase();
    if spellings
        .insert(portable.clone(), id.into())
        .is_some_and(|old| old != id)
    {
        return Err(corrupt(
            "recorded session identities collide under portable case folding",
        ));
    }
    reserved.insert(portable);
    Ok(())
}
fn is_history_operation(operation: &str) -> bool {
    matches!(
        operation,
        "history.session.create" | "history.session.import" | "history.session.mutate"
    )
}
fn marker_bytes(marker: &SessionRetirement) -> Result<Vec<u8>> {
    serde_yaml_ng::to_string(marker)
        .map(String::into_bytes)
        .map_err(|e| invalid(e.to_string()))
}
fn record_hash(record: &SessionRecord) -> Result<ContentHash> {
    let mut value = serde_json::to_value(record).map_err(|e| corrupt(e.to_string()))?;
    value
        .as_object_mut()
        .expect("record is an object")
        .remove("retirement");
    crate::transactions::canonical_hash(&value)
}
fn validate_history_receipt(
    receipt: &MutationReceipt,
    repository: &RepositoryId,
) -> Result<Vec<SessionRecord>> {
    crate::transactions::validate_receipt(receipt)?;
    if receipt.repository.as_ref() != Some(repository) || !is_history_operation(&receipt.operation)
    {
        return Err(corrupt(
            "invalid historical receipt repository or operation",
        ));
    }
    let records: Vec<SessionRecord> = if receipt.operation == "history.session.import" {
        serde_json::from_value(receipt.result.clone())
    } else {
        serde_json::from_value(receipt.result.clone()).map(|record| vec![record])
    }
    .map_err(|e| corrupt(format!("invalid historical receipt result: {e}")))?;
    if records.is_empty() || records.len() > MAX_RECORDS || receipt.changed.len() != records.len() {
        return Err(corrupt(
            "historical receipt record count differs from its changes",
        ));
    }
    let mut seen = BTreeSet::new();
    for (record, change) in records.iter().zip(&receipt.changed) {
        if record.path != session_path(&record.session.id)?
            || record.session.title.trim().is_empty()
            || record.evidence != "historical_annotation"
            || !seen.insert(record.session.id.to_ascii_lowercase())
        {
            return Err(corrupt(
                "historical receipt result has an invalid identity, path or evidence",
            ));
        }
        if record.retired {
            let proof = record
                .retirement
                .as_ref()
                .ok_or_else(|| corrupt("retired session result lacks its proof"))?;
            if receipt.operation != "history.session.mutate"
                || proof.repository != *repository
                || proof.id != record.session.id
                || proof.request_id != receipt.request_id
                || proof.content != record.source
                || proof.record_hash != record_hash(record)?
                || change.path != marker_path(&record.session.id)?
                || change.before.is_some()
                || change.after.as_ref() != Some(&ContentHash::of(&marker_bytes(proof)?))
            {
                return Err(corrupt(
                    "historical retirement result does not match its durable change",
                ));
            }
        } else if record.retirement.is_some()
            || change.path != record.path
            || change.after.as_ref() != Some(&record.source)
            || (receipt.operation == "history.session.mutate") != change.before.is_some()
        {
            return Err(corrupt(
                "historical receipt result differs from its durable change",
            ));
        }
    }
    Ok(records)
}

fn parse_session(path: &Path, bytes: &[u8], retired: bool) -> Result<SessionRecord> {
    if bytes.len() > MAX_BYTES {
        return Err(invalid("recorded session exceeds 2 MiB").at(path));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| schema(e.to_string()).at(path))?;
    let session: RecordedSession = toml::from_str(text).map_err(|e: toml::de::Error| {
        let mut error = schema(e.to_string()).at(path);
        if let Some(span) = e.span() {
            let prefix = &text[..span.start.min(text.len())];
            error.line = Some(prefix.bytes().filter(|byte| *byte == b'\n').count() + 1);
            error.column = Some(
                prefix
                    .rsplit('\n')
                    .next()
                    .unwrap_or_default()
                    .chars()
                    .count()
                    + 1,
            );
        }
        error
    })?;
    bound_session(&session)?;
    if session_path(&session.id)?.as_os_str() != path.as_os_str() || session.title.trim().is_empty()
    {
        return Err(schema("session filename, identity or title is invalid").at(path));
    }
    Ok(SessionRecord {
        session,
        path: path.into(),
        source: ContentHash::of(bytes),
        retired,
        evidence: "historical_annotation".into(),
        retirement: None,
    })
}
fn serialize(session: &RecordedSession) -> Result<Vec<u8>> {
    bound_session(session)?;
    session_path(&session.id)?;
    if session.title.trim().is_empty() {
        return Err(invalid("recorded session title cannot be blank"));
    }
    let bytes = toml::to_string_pretty(session)
        .map_err(|e| invalid(e.to_string()))?
        .into_bytes();
    if bytes.len() > MAX_BYTES {
        return Err(invalid("recorded session exceeds 2 MiB"));
    }
    Ok(bytes)
}
fn load_session(
    root: &Path,
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
    id: &str,
) -> Result<SessionRecord> {
    let index = HistoryIndex::load(snapshot, repository)?;
    load_session_indexed(root, snapshot, &index, id)
}
fn load_session_indexed(
    root: &Path,
    snapshot: &Snapshot<'_>,
    index: &HistoryIndex,
    id: &str,
) -> Result<SessionRecord> {
    let path = session_path(id)?;
    let bytes = snapshot.read_bounded(&path, MAX_BYTES)?.ok_or_else(|| {
        PmError::new(ErrorCode::NotFound, "recorded session was not found").at(root.join(&path))
    })?;
    let mut record = parse_session(&path, &bytes, false)?;
    let marker = marker_path(id)?;
    let marker_bytes = snapshot.read_bounded(&marker, MAX_BYTES)?;
    let proof = index.retired.get(&id.to_ascii_lowercase());
    match (marker_bytes, proof) {
        (None, None) => {}
        (Some(bytes), Some(retired)) => {
            let value: SessionRetirement = crate::documents::YamlDocument::parse(
                &marker,
                std::str::from_utf8(&bytes).map_err(|e| schema(e.to_string()).at(&marker))?,
            )?
            .deserialize()?;
            record.retired = true;
            record.retirement = Some(value);
            if record != *retired
                || ContentHash::of(&bytes)
                    != ContentHash::of(&self::marker_bytes(
                        record.retirement.as_ref().expect("set above"),
                    )?)
            {
                return Err(corrupt(
                    "retired session history or marker differs from its durable receipt",
                )
                .at(root.join(marker)));
            }
        }
        _ => {
            return Err(corrupt(
                "session retirement marker and durable receipt must both remain present",
            )
            .at(root.join(marker)));
        }
    }
    Ok(record)
}
fn load_sessions(
    root: &Path,
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
) -> Result<Vec<SessionRecord>> {
    let index = HistoryIndex::load(snapshot, repository)?;
    let mut records = Vec::new();
    let mut total = 0usize;
    let mut seen = BTreeSet::new();
    for path in snapshot.list_bounded(Path::new("imported-sessions"), MAX_RECORDS)? {
        let id = path
            .file_stem()
            .and_then(|id| id.to_str())
            .expect("layout validated");
        let record = load_session_indexed(root, snapshot, &index, id)?;
        seen.insert(id.to_ascii_lowercase());
        total += snapshot
            .read_bounded(&path, MAX_BYTES)?
            .map_or(0, |bytes| bytes.len());
        if total > 64 * 1024 * 1024 {
            return Err(invalid("recorded sessions exceed 64 MiB"));
        }
        if !record.retired {
            records.push(record);
        }
    }
    for id in index.retired.keys().chain(
        snapshot
            .list_bounded(Path::new("imported-history/deleted-sessions"), MAX_RECORDS)?
            .iter()
            .filter_map(|path| path.file_stem().and_then(|id| id.to_str()))
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
            .iter(),
    ) {
        if !seen.contains(id) {
            return Err(corrupt("retired session has no retained historical record")
                .at(root.join("imported-sessions")));
        }
    }
    records.sort_by(|a, b| {
        b.session
            .started_at
            .cmp(&a.session.started_at)
            .then(a.session.id.cmp(&b.session.id))
    });
    Ok(records)
}
fn patch_session(
    snapshot: &Snapshot<'_>,
    original: &SessionRecord,
    updated: &RecordedSession,
) -> Result<Vec<u8>> {
    let bytes = snapshot
        .read_bounded(&original.path, MAX_BYTES)?
        .ok_or_else(|| schema("recorded session disappeared"))?;
    let mut document = std::str::from_utf8(&bytes)
        .map_err(|e| schema(e.to_string()))?
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| schema(e.to_string()))?;
    let before = toml::Value::try_from(&original.session).map_err(|e| invalid(e.to_string()))?;
    let after = toml::Value::try_from(updated).map_err(|e| invalid(e.to_string()))?;
    for (key, value) in after.as_table().expect("session serializes as a table") {
        if before.get(key) == Some(value) {
            continue;
        }
        let patch = toml::to_string(&BTreeMap::from([(key, value)]))
            .map_err(|e| invalid(e.to_string()))?
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| invalid(e.to_string()))?;
        let mut item = patch.get(key).expect("serialized field exists").clone();
        // Appending preserves every existing table/value, including producer
        // metadata, quoting and comments on earlier array elements.
        if let (Some(old_values), Some(new_values)) = (
            before.get(key).and_then(toml::Value::as_array),
            value.as_array(),
        ) && new_values.len() > old_values.len()
            && new_values.starts_with(old_values)
        {
            if let (Some(old), Some(new)) = (
                document
                    .get_mut(key)
                    .and_then(toml_edit::Item::as_array_of_tables_mut),
                item.as_array_of_tables(),
            ) {
                for table in new.iter().skip(old_values.len()) {
                    old.push(table.clone());
                }
                continue;
            }
            if let (Some(old), Some(new)) = (
                document
                    .get_mut(key)
                    .and_then(toml_edit::Item::as_array_mut),
                item.as_array(),
            ) {
                for value in new.iter().skip(old_values.len()) {
                    append_array_value(old, value.clone());
                }
                continue;
            }
            // A producer may use inline file tables while serialization selects
            // arrays-of-tables; convert only the newly appended tables.
            if let (Some(old), Some(new)) = (
                document
                    .get_mut(key)
                    .and_then(toml_edit::Item::as_array_mut),
                item.as_array_of_tables(),
            ) {
                for table in new.iter().skip(old_values.len()) {
                    append_array_value(
                        old,
                        toml_edit::Value::InlineTable(table.clone().into_inline_table()),
                    );
                }
                continue;
            }
        }
        if let Some(old) = document.get(key).and_then(toml_edit::Item::as_value)
            && let Some(new) = item.as_value_mut()
        {
            *new.decor_mut() = old.decor().clone();
        }
        if let Some(old) = document.get_mut(key) {
            *old = item;
        } else {
            document.insert(key, item);
        }
    }
    Ok(document.to_string().into_bytes())
}
fn append_array_value(array: &mut toml_edit::Array, mut value: toml_edit::Value) {
    // TOML stores a comment after the final comma as array trailing trivia.
    // Move that trivia before the appended value so it stays on the old row.
    if array.trailing_comma() {
        let trailing = array.trailing().as_str().unwrap_or_default();
        if trailing.contains('\n') {
            let indent = array
                .iter()
                .last()
                .and_then(|value| value.decor().prefix())
                .and_then(|prefix| prefix.as_str())
                .and_then(|prefix| prefix.rsplit('\n').next())
                .filter(|indent| indent.trim().is_empty())
                .unwrap_or_default();
            value.decor_mut().set_prefix(format!("{trailing}{indent}"));
            let closing = format!("\n{}", trailing.rsplit('\n').next().unwrap_or_default());
            array.set_trailing(closing);
        }
    }
    array.push_formatted(value);
}
fn events(snapshot: &Snapshot<'_>) -> Result<Vec<Value>> {
    let path = Path::new("imported-history/events.jsonl");
    let Some(bytes) = snapshot.read_bounded(path, 64 * 1024 * 1024)? else {
        return Ok(Vec::new());
    };
    let text = std::str::from_utf8(&bytes).map_err(|e| schema(e.to_string()).at(path))?;
    let mut events = Vec::new();
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: Value = serde_json::from_str(line).map_err(|e| {
            let mut error = schema(e.to_string()).at(path);
            error.line = Some(index + 1);
            error.column = Some(e.column());
            error
        })?;
        if !value.is_object()
            || value["kind"]
                .as_str()
                .is_none_or(|kind| kind.trim().is_empty())
        {
            return Err(schema("historical events require an object and a nonempty kind").at(path));
        }
        if events.len() >= MAX_RECORDS {
            return Err(invalid("historical events exceed 10,000 records").at(path));
        }
        events.push(value);
    }
    Ok(events)
}

pub(crate) fn inspect_snapshot(
    root: &Path,
    snapshot: &Snapshot<'_>,
    repository: &RepositoryId,
) -> (usize, Vec<PmError>) {
    let mut count = 0;
    let mut errors = Vec::new();
    let inspect = || -> Result<(usize, Vec<PmError>)> {
        let index = HistoryIndex::load(snapshot, repository)?;
        let mut identities = BTreeSet::new();
        let mut count = 0;
        for prefix in ["imported-sessions", "imported-history/deleted-sessions"] {
            let paths = snapshot.list_bounded(Path::new(prefix), MAX_RECORDS)?;
            count += paths.len();
            identities.extend(
                paths
                    .iter()
                    .filter_map(|path| path.file_stem().and_then(|id| id.to_str()))
                    .map(str::to_owned),
            );
        }
        identities.extend(
            index
                .retired
                .values()
                .map(|record| record.session.id.clone()),
        );
        let mut errors = Vec::new();
        let mut total = 0usize;
        for id in identities {
            let mut exhausted = false;
            for path in [session_path(&id)?, marker_path(&id)?] {
                match snapshot.read_bounded(&path, MAX_BYTES.min(64 * 1024 * 1024 - total)) {
                    Ok(bytes) => total += bytes.map_or(0, |bytes| bytes.len()),
                    Err(error) => {
                        errors.push(error);
                        exhausted = true;
                        break;
                    }
                }
            }
            if exhausted {
                break;
            }
            if let Err(error) = load_session_indexed(root, snapshot, &index, &id) {
                errors.push(error);
            }
        }
        Ok((count, errors))
    };
    match inspect() {
        Ok((records, diagnostics)) => {
            count += records;
            errors.extend(diagnostics);
        }
        Err(error) => errors.push(error),
    }
    match events(snapshot) {
        Ok(events) => count += events.len(),
        Err(error) => errors.push(error),
    }
    (count, errors)
}
fn corrupt(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::CorruptStore, message)
}
fn bound_session(session: &RecordedSession) -> Result<()> {
    let strings = [
        &session.id,
        &session.title,
        &session.agent,
        &session.cwd,
        &session.status,
        &session.started_at,
        &session.ended_at,
        &session.goal,
        &session.summary,
    ]
    .into_iter()
    .chain(session.plan.iter())
    .chain(session.commands_run.iter())
    .chain(session.tests_run.iter())
    .chain(session.handoff_notes.iter())
    .chain(
        session
            .touched_files
            .iter()
            .flat_map(|file| [&file.path, &file.change_type]),
    );
    let mut total = 0usize;
    let mut nodes = 0usize;
    for text in strings {
        nodes += 1;
        total = total.saturating_add(text.len());
        if total > MAX_BYTES || nodes > 100_000 {
            return Err(invalid(
                "historical session exceeds byte or value count limits",
            ));
        }
    }
    let mut pending = Vec::new();
    for (key, value) in session
        .extra
        .iter()
        .chain(session.touched_files.iter().flat_map(|file| &file.extra))
    {
        total = total.saturating_add(key.len());
        if pending.len() + nodes >= 100_000 || total > MAX_BYTES {
            return Err(invalid(
                "historical metadata exceeds byte or value count limits",
            ));
        }
        pending.push((value, 0));
    }
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > 64 || nodes > 100_000 {
            return Err(invalid(
                "historical metadata exceeds nesting or value count limits",
            ));
        }
        match value {
            toml::Value::Table(values) => {
                if nodes + pending.len() + values.len() > 100_000 {
                    return Err(invalid("historical metadata exceeds value count limits"));
                }
                for (key, value) in values {
                    total = total.saturating_add(key.len());
                    pending.push((value, depth + 1));
                }
            }
            toml::Value::Array(values) => {
                if nodes + pending.len() + values.len() > 100_000 {
                    return Err(invalid("historical metadata exceeds value count limits"));
                }
                pending.extend(values.iter().map(|value| (value, depth + 1)))
            }
            toml::Value::String(value) => total = total.saturating_add(value.len()),
            toml::Value::Float(value) if !value.is_finite() => {
                return Err(invalid(
                    "non-finite TOML metadata cannot be represented in durable JSON receipts",
                ));
            }
            _ => total = total.saturating_add(32),
        }
        if total > MAX_BYTES {
            return Err(invalid("historical metadata exceeds 2 MiB"));
        }
    }
    Ok(())
}
fn bounded_fields(fields: &BTreeMap<String, Value>) -> Result<()> {
    if fields.len() > 100_000 {
        return Err(invalid("historical metadata exceeds field count limits"));
    }
    let bytes = fields
        .keys()
        .try_fold(0usize, |total, key| total.checked_add(key.len()))
        .ok_or_else(|| invalid("metadata size overflow"))?;
    bounded_values(fields.values(), bytes)
}
fn bounded_json(value: &Value) -> Result<()> {
    bounded_values(std::iter::once(value), 0)
}
fn bounded_values<'a>(
    values: impl IntoIterator<Item = &'a Value>,
    initial_bytes: usize,
) -> Result<()> {
    let mut pending: Vec<_> = values.into_iter().map(|value| (value, 0)).collect();
    let mut total = initial_bytes;
    let mut nodes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > 64 || nodes > 100_000 {
            return Err(invalid(
                "historical metadata exceeds nesting or value count limits",
            ));
        }
        match value {
            Value::Object(values) => {
                if nodes + pending.len() + values.len() > 100_000 {
                    return Err(invalid("historical metadata exceeds value count limits"));
                }
                for (key, value) in values {
                    total += key.len();
                    pending.push((value, depth + 1));
                }
            }
            Value::Array(values) => pending.extend(values.iter().map(|value| (value, depth + 1))),
            Value::String(value) => total += value.len(),
            _ => total += 8,
        }
        if total > MAX_BYTES {
            return Err(invalid("historical input exceeds 2 MiB"));
        }
    }
    Ok(())
}
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}
fn schema(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_identity_retries_collisions_and_stops_after_bound() {
        let index = HistoryIndex {
            reserved: BTreeSet::from(["ses-collision".into()]),
            ..Default::default()
        };
        let mut attempts = 0;
        let id = generated_id(&index, || {
            attempts += 1;
            if attempts == 1 {
                "SES-collision"
            } else {
                "SES-free"
            }
            .into()
        })
        .unwrap();
        assert_eq!(id, "SES-free");
        assert_eq!(attempts, 2);
        attempts = 0;
        assert!(
            generated_id(&index, || {
                attempts += 1;
                "SES-COLLISION".into()
            })
            .is_err()
        );
        assert_eq!(attempts, 16);
    }
}
