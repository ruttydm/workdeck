//! Immutable time declarations and explicit, non-forking amendments.
use crate::{
    Config, ErrorCode, PmError, RetirementKind, RetirementTarget,
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument, YamlDocument},
    issues::resolve_issue,
    repository::config_from_snapshot,
    transactions::{FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use crate::{
    ContentHash, RecordId, Repository, RepositoryId, RequestId, Result, SchemaVersion, SourceToken,
    Timestamp,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

pub const MAX_TIME_ENTRY_BYTES: usize = 64 * 1024;
const MAX_TIME_ENTRIES: usize = 4096;
const MAX_TIME_CONTENT_BYTES: usize = 16 * 1024 * 1024;
const MAX_TIME_TRAVERSAL: usize = 20_000;

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeEntryInput {
    pub user: String,
    pub actor: String,
    pub seconds: u64,
    pub worked_at: Timestamp,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeEntryAmendment {
    pub entry: TimeEntryInput,
    pub supersedes: RecordId,
    pub expected: ContentHash,
    pub reason: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeEntry {
    pub schema: SchemaVersion,
    pub id: RecordId,
    pub repository: RepositoryId,
    pub issue: RecordId,
    pub user: String,
    pub actor: String,
    pub seconds: u64,
    pub worked_at: Timestamp,
    pub recorded_at: Timestamp,
    pub cycle: Option<String>,
    pub supersedes: Option<RecordId>,
    pub reason: Option<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeEntryRecord {
    pub entry: TimeEntry,
    pub path: PathBuf,
    pub content: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeReportQuery {
    pub issue: Option<String>,
    pub user: Option<String>,
    pub cycle: Option<String>,
    /// Inclusive work timestamp, applied after amendment resolution.
    pub from: Option<Timestamp>,
    /// Exclusive work timestamp, applied after amendment resolution.
    pub to: Option<Timestamp>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimeReport {
    pub entries: Vec<TimeEntryRecord>,
    pub total_seconds: u64,
    pub by_issue: BTreeMap<String, u64>,
    pub by_user: BTreeMap<String, u64>,
    pub by_cycle: BTreeMap<String, u64>,
    pub unassigned_cycle_seconds: u64,
}

impl Repository {
    /// Record a declaration of work. Identical work with a different request is
    /// a distinct entry; only the durable request identity deduplicates writes.
    pub fn log_time(
        &self,
        reference: &str,
        expected_issue: Option<&SourceToken>,
        input: &TimeEntryInput,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.log_time_with_faults(reference, expected_issue, input, request, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn log_time_with_faults(
        &self,
        reference: &str,
        expected_issue: Option<&SourceToken>,
        input: &TimeEntryInput,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.write_time(reference, expected_issue, input, None, request, fault)
    }

    /// Append a full replacement of an active entry. The old file and captured
    /// cycle remain unchanged. Zero seconds can explicitly correct a void entry.
    pub fn amend_time(
        &self,
        reference: &str,
        expected_issue: Option<&SourceToken>,
        input: &TimeEntryAmendment,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.amend_time_with_faults(reference, expected_issue, input, request, |_| Ok(()))
    }

    #[doc(hidden)]
    pub fn amend_time_with_faults(
        &self,
        reference: &str,
        expected_issue: Option<&SourceToken>,
        input: &TimeEntryAmendment,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        self.write_time(
            reference,
            expected_issue,
            &input.entry,
            Some(input),
            request,
            fault,
        )
    }

    fn write_time(
        &self,
        reference: &str,
        expected_issue: Option<&SourceToken>,
        input: &TimeEntryInput,
        amendment: Option<&TimeEntryAmendment>,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let operation = if amendment.is_some() {
            "issue.amend_time"
        } else {
            "issue.log_time"
        };
        let intent = json!({"reference":reference,"expected_issue":expected_issue,"entry":input,"amendment":amendment});
        let receipt = self.store()?.transact_with_faults(
            request,
            operation,
            &intent,
            |snapshot| {
                validate_input(input)?;
                crate::organization::validate_actor(snapshot, self.identity(), &input.actor)?;
                let config = config_from_snapshot(self.root(), snapshot)?;
                let mut records = load_all(self.root(), snapshot, &config)?;
                let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
                crate::retirement::ensure_writable(
                    self.root(),
                    snapshot,
                    &config,
                    &RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.as_str())?,
                )?;
                if expected_issue.is_some_and(|expected| expected != &issue.source) {
                    return Err(PmError::new(
                        ErrorCode::StaleSource,
                        "issue changed since the time request was prepared",
                    )
                    .at(&issue.path));
                }
                let (cycle, supersedes, reason, previous_time) = if let Some(amendment) = amendment
                {
                    validate_text("amendment reason", &amendment.reason, 4000)?;
                    parse_time_id(amendment.supersedes.as_str())?;
                    let previous = records
                        .iter()
                        .find(|record| {
                            record.entry.id == amendment.supersedes
                                && record.entry.issue == issue.metadata.id
                        })
                        .ok_or_else(|| {
                            PmError::new(
                                ErrorCode::NotFound,
                                "amended time entry does not belong to this issue",
                            )
                        })?;
                    if previous.entry.user != input.user {
                        crate::organization::validate_actor(
                            snapshot,
                            self.identity(),
                            &input.user,
                        )?;
                    }
                    if previous.content != amendment.expected {
                        return Err(PmError::new(
                            ErrorCode::StaleSource,
                            "time entry changed since amendment was prepared",
                        )
                        .at(&previous.path));
                    }
                    if records.iter().any(|record| {
                        record.entry.supersedes.as_ref() == Some(&amendment.supersedes)
                    }) {
                        return Err(PmError::new(
                            ErrorCode::Conflict,
                            "time entry already has an amendment; amend its current successor",
                        ));
                    }
                    (
                        previous.entry.cycle.clone(),
                        Some(previous.entry.id.clone()),
                        Some(amendment.reason.clone()),
                        previous.entry.recorded_at,
                    )
                } else {
                    crate::organization::validate_actor(snapshot, self.identity(), &input.user)?;
                    if let Some(cycle) = &issue.metadata.cycle {
                        // Existing archived cycles remain useful historical attribution.
                        crate::planning::store::load_planning(
                            self.root(),
                            snapshot,
                            crate::PlanningKind::Cycle,
                            cycle,
                        )?;
                        crate::retirement::ensure_writable(
                            self.root(),
                            snapshot,
                            &config,
                            &RetirementTarget::new(RetirementKind::Cycle, cycle)?,
                        )?;
                    }
                    (
                        issue.metadata.cycle.clone(),
                        None,
                        None,
                        issue.metadata.created_at,
                    )
                };
                let entry = TimeEntry {
                    schema: SchemaVersion::CURRENT,
                    id: RecordId::new("TIME")?,
                    repository: config.repository.clone(),
                    issue: issue.metadata.id,
                    user: input.user.clone(),
                    actor: input.actor.clone(),
                    seconds: input.seconds,
                    worked_at: input.worked_at,
                    recorded_at: Utc::now().max(previous_time),
                    cycle,
                    supersedes,
                    reason,
                };
                validate_entry(&entry)?;
                let path = entry_path(&entry);
                let bytes = encode(&entry)?;
                let previous_bytes = records.iter().try_fold(0usize, |total, record| {
                    snapshot
                        .read_bounded(&record.path, MAX_TIME_ENTRY_BYTES)?
                        .map(|bytes| total + bytes.len())
                        .ok_or_else(|| invalid("time entry disappeared").at(&record.path))
                })?;
                let record = TimeEntryRecord {
                    entry,
                    path: path.clone(),
                    content: ContentHash::of(&bytes),
                };
                records.push(record.clone());
                validate_graph(&records)?;
                if records.len() > MAX_TIME_ENTRIES
                    || bytes.len() > MAX_TIME_ENTRY_BYTES
                    || previous_bytes + bytes.len() > MAX_TIME_CONTENT_BYTES
                {
                    return Err(PmError::new(
                        ErrorCode::InvalidInput,
                        "time entry collection exceeds its supported capacity",
                    ));
                }
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path,
                        expected: None,
                        content: Some(bytes),
                    }],
                    result: json!(record),
                })
            },
            fault,
        )?;
        validate_receipt_result(&receipt, self.identity())?;
        Ok(receipt)
    }

    /// All authored records, including superseded entries and retired issues.
    pub fn time_entries(&self, reference: &str) -> Result<Vec<TimeEntryRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let records = load_all(self.root(), snapshot, &config)?;
            let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.as_str())?,
            )?;
            Ok(records
                .into_iter()
                .filter(|record| record.entry.issue == issue.metadata.id)
                .collect())
        })
    }

    /// Resolve complete amendment chains before applying filters. Otherwise a
    /// corrected user or work date could leave the old value counted too.
    pub fn time_report(&self, query: &TimeReportQuery) -> Result<TimeReport> {
        self.store()?.with_snapshot(|snapshot| {
            if query
                .from
                .zip(query.to)
                .is_some_and(|(from, to)| from >= to)
            {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "time report requires from before to",
                ));
            }
            if let Some(user) = &query.user {
                validate_text("user", user, 256)?;
            }
            if let Some(cycle) = &query.cycle {
                crate::planning::validate_id(cycle)?;
            }
            let config = config_from_snapshot(self.root(), snapshot)?;
            let records = load_all(self.root(), snapshot, &config)?;
            let issue = query
                .issue
                .as_deref()
                .map(|reference| resolve_issue(self.root(), snapshot, &config, reference))
                .transpose()?;
            let superseded: BTreeSet<_> = records
                .iter()
                .filter_map(|record| record.entry.supersedes.clone())
                .collect();
            let entries = records
                .into_iter()
                .filter(|record| {
                    let entry = &record.entry;
                    !superseded.contains(&entry.id)
                        && issue
                            .as_ref()
                            .is_none_or(|issue| issue.metadata.id == entry.issue)
                        && query.user.as_ref().is_none_or(|user| user == &entry.user)
                        && query
                            .cycle
                            .as_ref()
                            .is_none_or(|cycle| entry.cycle.as_ref() == Some(cycle))
                        && query.from.is_none_or(|from| entry.worked_at >= from)
                        && query.to.is_none_or(|to| entry.worked_at < to)
                })
                .collect::<Vec<_>>();
            let mut report = TimeReport {
                entries,
                total_seconds: 0,
                by_issue: BTreeMap::new(),
                by_user: BTreeMap::new(),
                by_cycle: BTreeMap::new(),
                unassigned_cycle_seconds: 0,
            };
            for record in &report.entries {
                let entry = &record.entry;
                add_seconds(&mut report.total_seconds, entry.seconds)?;
                add_seconds(
                    report.by_issue.entry(entry.issue.to_string()).or_default(),
                    entry.seconds,
                )?;
                add_seconds(
                    report.by_user.entry(entry.user.clone()).or_default(),
                    entry.seconds,
                )?;
                if let Some(cycle) = &entry.cycle {
                    add_seconds(
                        report.by_cycle.entry(cycle.clone()).or_default(),
                        entry.seconds,
                    )?;
                } else {
                    add_seconds(&mut report.unassigned_cycle_seconds, entry.seconds)?;
                }
            }
            Ok(report)
        })
    }
}

pub(crate) fn parse_time_id(id: &str) -> Result<RecordId> {
    let id: RecordId = id.parse()?;
    if !id.as_str().starts_with("TIME-") {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "time identity must be a full TIME-prefixed ULID",
        ));
    }
    Ok(id)
}

fn entry_path(entry: &TimeEntry) -> PathBuf {
    PathBuf::from(format!("issues/{}/time/{}.yml", entry.issue, entry.id))
}

fn validate_text(name: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            format!("{name} must be nonempty, bounded text without control characters"),
        ));
    }
    Ok(())
}

fn validate_input(input: &TimeEntryInput) -> Result<()> {
    validate_text("user", &input.user, 256)?;
    validate_text("actor", &input.actor, 256)
}

fn validate_entry(entry: &TimeEntry) -> Result<()> {
    parse_time_id(entry.id.as_str())?;
    validate_text("user", &entry.user, 256)?;
    validate_text("actor", &entry.actor, 256)?;
    if let Some(cycle) = &entry.cycle {
        crate::planning::validate_id(cycle)?;
    }
    if entry.worked_at > entry.recorded_at {
        return Err(invalid(
            "time records describe past work, not future scheduled work",
        ));
    }
    match (&entry.supersedes, &entry.reason) {
        (Some(previous), Some(reason)) => {
            parse_time_id(previous.as_str())?;
            validate_text("amendment reason", reason, 4000)?;
        }
        (None, None) => {}
        _ => {
            return Err(invalid(
                "supersedes and amendment reason must be supplied together",
            ));
        }
    }
    Ok(())
}

fn encode(entry: &TimeEntry) -> Result<Vec<u8>> {
    serde_yaml_ng::to_string(entry)
        .map(String::into_bytes)
        .map_err(|error| invalid(error.to_string()))
}

fn parse_entry(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    path: &Path,
) -> Result<(TimeEntryRecord, usize)> {
    let parts = path.components().collect::<Vec<_>>();
    if parts.len() != 4 || path.extension().is_none_or(|ext| ext != "yml") {
        return Err(invalid("time records belong in issues/<issue>/time/<TIME-ID>.yml").at(path));
    }
    let bytes = snapshot
        .read_bounded(path, MAX_TIME_ENTRY_BYTES)?
        .ok_or_else(|| invalid("time entry disappeared").at(path))?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("time entry must be UTF-8").at(path))?;
    let document = YamlDocument::parse(&root.join(path), text)?;
    if let Some(schema) = document
        .metadata()
        .get("schema")
        .and_then(serde_yaml_ng::Value::as_u64)
    {
        SchemaVersion::try_from(schema).map_err(|error| error.at(path))?;
    }
    let entry: TimeEntry = document.deserialize()?;
    validate_entry(&entry).map_err(|error| invalid(error.message).at(path))?;
    if entry.repository != config.repository || entry_path(&entry) != path {
        return Err(invalid("time repository, issue, or file identity disagrees").at(path));
    }
    let issue_path = PathBuf::from(format!("issues/{}/item.md", entry.issue));
    let issue_bytes = snapshot
        .read_bounded(&issue_path, MAX_DOCUMENT_BYTES)?
        .ok_or_else(|| invalid("time entry has no parent issue").at(&issue_path))?;
    let issue_text = std::str::from_utf8(&issue_bytes)
        .map_err(|_| invalid("parent issue must be UTF-8").at(&issue_path))?;
    let issue = MarkdownDocument::parse(&root.join(&issue_path), issue_text)?;
    let metadata = crate::issues::parse_issue_metadata(&root.join(&issue_path), &issue)?;
    metadata.validate(config)?;
    if metadata.id != entry.issue {
        return Err(invalid("parent issue identity disagrees").at(&issue_path));
    }
    if let Some(cycle) = &entry.cycle {
        crate::planning::store::load_planning(root, snapshot, crate::PlanningKind::Cycle, cycle)?;
    }
    Ok((
        TimeEntryRecord {
            entry,
            path: path.into(),
            content: ContentHash::of(&bytes),
        },
        bytes.len(),
    ))
}

fn scan(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<TimeEntryRecord>, Vec<PmError>) {
    let paths = match snapshot.list_bounded(Path::new("issues"), MAX_TIME_TRAVERSAL) {
        Ok(paths) => paths,
        Err(error) => return (0, Vec::new(), vec![error]),
    };
    let mut records = Vec::new();
    let mut errors = Vec::new();
    let mut count = 0;
    let mut bytes = 0;
    for path in paths.into_iter().filter(|path| {
        path.components()
            .nth(2)
            .is_some_and(|part| part.as_os_str() == "time")
    }) {
        count += 1;
        if count > MAX_TIME_ENTRIES {
            errors.push(invalid("time records exceed the 4096 entry limit"));
            break;
        }
        match parse_entry(root, snapshot, config, &path) {
            Ok((record, size)) => {
                bytes += size;
                if bytes > MAX_TIME_CONTENT_BYTES {
                    errors.push(invalid("time records exceed the 16 MiB content limit"));
                    break;
                }
                records.push(record);
            }
            Err(error) => errors.push(error.at(&path)),
        }
    }
    if let Err(error) = validate_graph(&records) {
        errors.push(error);
    }
    records.sort_by(|left, right| {
        left.entry
            .recorded_at
            .cmp(&right.entry.recorded_at)
            .then(left.entry.id.cmp(&right.entry.id))
    });
    (count, records, errors)
}

fn load_all(root: &Path, snapshot: &Snapshot<'_>, config: &Config) -> Result<Vec<TimeEntryRecord>> {
    let (_, records, errors) = scan(root, snapshot, config);
    if let Some(error) = errors.into_iter().next() {
        return Err(error);
    }
    Ok(records)
}

pub(crate) fn inspect_snapshot(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<PmError>) {
    let (count, _, errors) = scan(root, snapshot, config);
    (count, errors)
}

fn validate_graph(records: &[TimeEntryRecord]) -> Result<()> {
    let mut by_id = BTreeMap::new();
    let mut superseded = BTreeSet::new();
    for record in records {
        if by_id.insert(record.entry.id.clone(), record).is_some() {
            return Err(
                invalid("time identities must be unique across all issues").at(&record.path)
            );
        }
    }
    for record in records {
        if let Some(id) = &record.entry.supersedes {
            let previous = by_id
                .get(id)
                .ok_or_else(|| invalid("time amendment predecessor is missing").at(&record.path))?;
            if previous.entry.issue != record.entry.issue
                || previous.entry.cycle != record.entry.cycle
                || previous.entry.recorded_at > record.entry.recorded_at
            {
                return Err(invalid("time amendment must retain its issue and captured cycle, with nondecreasing recording time").at(&record.path));
            }
            if !superseded.insert(id.clone()) {
                return Err(invalid("time amendment chains cannot fork").at(&record.path));
            }
        }
    }
    let mut complete = BTreeSet::new();
    for record in records {
        let mut current = Some(&record.entry.id);
        let mut visiting = BTreeSet::new();
        while let Some(id) = current {
            if complete.contains(id) {
                break;
            }
            if !visiting.insert(id.clone()) {
                return Err(invalid("time amendment chains cannot cycle").at(&record.path));
            }
            current = by_id[id].entry.supersedes.as_ref();
        }
        complete.extend(visiting);
    }
    Ok(())
}

fn add_seconds(total: &mut u64, seconds: u64) -> Result<()> {
    *total = total
        .checked_add(seconds)
        .ok_or_else(|| invalid("time total exceeds integer seconds capacity"))?;
    Ok(())
}

pub(crate) fn validate_receipt_result(
    receipt: &MutationReceipt,
    repository: &RepositoryId,
) -> Result<()> {
    let record: TimeEntryRecord = serde_json::from_value(receipt.result.clone())
        .map_err(|error| invalid(error.to_string()))?;
    validate_entry(&record.entry)?;
    if receipt.repository.as_ref() != Some(repository)
        || &record.entry.repository != repository
        || record.path != entry_path(&record.entry)
        || record.content != ContentHash::of(&encode(&record.entry)?)
        || receipt.changed.len() != 1
        || receipt.changed[0].path != record.path
        || receipt.changed[0].before.is_some()
        || receipt.changed[0].after.as_ref() != Some(&record.content)
        || (receipt.operation == "issue.amend_time") != record.entry.supersedes.is_some()
    {
        return Err(PmError::new(
            ErrorCode::CorruptStore,
            "time receipt result does not match its published identity and content",
        ));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
