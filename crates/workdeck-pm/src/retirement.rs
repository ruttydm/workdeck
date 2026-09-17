//! Permanent retirement is distinct from reversible archival. Authored records
//! remain readable; a durable tombstone closes their ordinary write surface.
use crate::{
    Config, ContentHash, ErrorCode, IssueId, IssueMetadata, IssueRecord, PlanningKind,
    PlanningRecord, PmError, Repository, RepositoryId, RequestId, Result, SchemaVersion,
    SourceToken, Timestamp,
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument, YamlDocument},
    issues::{load_issues, record_from_document, replace_metadata, resolve_issue},
    planning::store::{load_planning, prepare_write},
    repository::config_from_snapshot,
    transactions::{FaultPoint, FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

const OPERATION: &str = "record.retire";

mod index;
pub(crate) use index::RetirementIndex;
mod resolution;
pub use resolution::{
    ReferenceRetirementChange, ReferenceRetirementOutcome, ReferenceRetirementPlan,
};

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetirementKind {
    Issue,
    Feature,
    Gate,
    Initiative,
    Project,
    Milestone,
    Cycle,
    Target,
    Label,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetirementTarget {
    pub kind: RetirementKind,
    pub id: String,
}

impl RetirementTarget {
    pub fn new(kind: RetirementKind, id: impl Into<String>) -> Result<Self> {
        let target = Self {
            kind,
            id: id.into(),
        };
        match kind {
            RetirementKind::Issue => {
                target.id.parse::<IssueId>()?;
            }
            RetirementKind::Feature => {
                target.id.parse::<crate::FeatureId>()?;
            }
            RetirementKind::Gate => {
                target.id.parse::<crate::GateId>()?;
            }
            _ => crate::planning::validate_id(&target.id)?,
        }
        Ok(target)
    }

    fn validate(&self) -> Result<()> {
        Self::new(self.kind, self.id.clone()).map(|_| ())
    }

    fn path(&self) -> PathBuf {
        format!("tombstones/{}/{}.yml", self.kind.directory(), self.id).into()
    }
}

impl RetirementKind {
    fn directory(self) -> &'static str {
        match self {
            Self::Issue => "issues",
            Self::Feature => "features",
            Self::Gate => "gates",
            Self::Initiative => "initiatives",
            Self::Project => "projects",
            Self::Milestone => "milestones",
            Self::Cycle => "cycles",
            Self::Target => "targets",
            Self::Label => "labels",
        }
    }

    fn planning(self) -> Option<PlanningKind> {
        match self {
            Self::Issue | Self::Feature | Self::Gate => None,
            Self::Initiative => Some(PlanningKind::Initiative),
            Self::Project => Some(PlanningKind::Project),
            Self::Milestone => Some(PlanningKind::Milestone),
            Self::Cycle => Some(PlanningKind::Cycle),
            Self::Target => Some(PlanningKind::Target),
            Self::Label => Some(PlanningKind::Label),
        }
    }
}

impl From<PlanningKind> for RetirementKind {
    fn from(kind: PlanningKind) -> Self {
        match kind {
            PlanningKind::Initiative => Self::Initiative,
            PlanningKind::Project => Self::Project,
            PlanningKind::Milestone => Self::Milestone,
            PlanningKind::Cycle => Self::Cycle,
            PlanningKind::Target => Self::Target,
            PlanningKind::Label => Self::Label,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetirementBlocker {
    pub issue: IssueId,
    pub path: PathBuf,
    pub field: String,
    pub source: SourceToken,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningReferenceBlocker {
    pub kind: PlanningKind,
    pub id: String,
    pub path: PathBuf,
    pub field: String,
    pub source: SourceToken,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecordReferenceBlocker {
    pub kind: RetirementKind,
    pub id: String,
    pub path: PathBuf,
    pub field: String,
    pub source: SourceToken,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetirementPreview {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub target: RetirementTarget,
    pub source: SourceToken,
    pub fingerprint: ContentHash,
    pub allowed: bool,
    pub blockers: Vec<RetirementBlocker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub planning_blockers: Vec<PlanningReferenceBlocker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub record_blockers: Vec<RecordReferenceBlocker>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub question_blockers: Vec<crate::QuestionReferenceBlocker>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetirementInput {
    pub target: RetirementTarget,
    #[serde(default)]
    pub expected: Option<SourceToken>,
    #[serde(default)]
    pub expected_preview: Option<ContentHash>,
}

impl RetirementInput {
    pub fn new(target: RetirementTarget) -> Self {
        Self {
            target,
            expected: None,
            expected_preview: None,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetainedFile {
    pub path: PathBuf,
    pub content: ContentHash,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tombstone {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub target: RetirementTarget,
    pub request_id: RequestId,
    pub retired_at: Timestamp,
    pub record_path: PathBuf,
    pub previous: SourceToken,
    pub retained: SourceToken,
    pub record_hash: ContentHash,
    pub history: Vec<RetainedFile>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "record", rename_all = "snake_case")]
pub enum RetiredRecord {
    Issue(Box<IssueRecord>),
    Feature(Box<crate::FeatureRecord>),
    Gate(Box<crate::GateRecord>),
    Planning(Box<PlanningRecord>),
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetirementOutcome {
    pub tombstone: Tombstone,
    pub record: RetiredRecord,
}

impl Repository {
    pub fn retirement_preview_issue(&self, reference: &str) -> Result<RetirementPreview> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            let target =
                RetirementTarget::new(RetirementKind::Issue, issue.metadata.id.to_string())?;
            build_preview(self.root(), snapshot, &config, &target).map(|(preview, _)| preview)
        })
    }

    pub fn retire_issue(
        &self,
        reference: &str,
        expected: Option<&SourceToken>,
        expected_preview: Option<&ContentHash>,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        let intent = json!({"issue_reference":reference,"expected":expected,"expected_preview":expected_preview});
        let receipt = self
            .store()?
            .transact(request, OPERATION, &intent, |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
                let input = RetirementInput {
                    target: RetirementTarget::new(
                        RetirementKind::Issue,
                        issue.metadata.id.to_string(),
                    )?,
                    expected: expected.cloned(),
                    expected_preview: expected_preview.cloned(),
                };
                prepare_retirement(self.root(), snapshot, &config, &input, request)
            })?;
        validate_receipt(&receipt, self.identity())?;
        Ok(receipt)
    }

    pub fn retirement_preview(&self, target: &RetirementTarget) -> Result<RetirementPreview> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            build_preview(self.root(), snapshot, &config, target).map(|(preview, _)| preview)
        })
    }

    pub fn retire_record(
        &self,
        input: &RetirementInput,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.retire_record_with_faults(input, request, |_| Ok(()))
    }

    /// Explicit fault instrumentation uses the same transaction recovery protocol.
    #[doc(hidden)]
    pub fn retire_record_with_faults(
        &self,
        input: &RetirementInput,
        request: &RequestId,
        fault: impl FnMut(FaultPoint) -> Result<()>,
    ) -> Result<MutationReceipt> {
        let receipt = self.store()?.transact_with_faults(
            request,
            OPERATION,
            &value(input)?,
            |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                prepare_retirement(self.root(), snapshot, &config, input, request)
            },
            fault,
        )?;
        // This validates the returned historical result itself, including replay;
        // it performs no fresh source lookup before idempotency resolution.
        validate_receipt(&receipt, self.identity())?;
        Ok(receipt)
    }

    pub fn tombstone(&self, target: &RetirementTarget) -> Result<Option<Tombstone>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            read_tombstone(self.root(), snapshot, &config, target)
        })
    }
}

fn prepare_retirement(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    input: &RetirementInput,
    request: &RequestId,
) -> Result<PreparedOperation> {
    let (preview, original) = build_preview(root, snapshot, config, &input.target)?;
    if input
        .expected
        .as_ref()
        .is_some_and(|expected| expected != &preview.source)
        || input
            .expected_preview
            .as_ref()
            .is_some_and(|expected| expected != &preview.fingerprint)
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "retirement source or reviewed reference membership has changed",
        )
        .at(original.path()));
    }
    if !preview.allowed {
        return Err(PmError::new(ErrorCode::PolicyBlocked, "retirement has incoming references; archive the record or explicitly resolve those references first").details(value(&preview)?));
    }
    prepare_tombstone(root, snapshot, config, &input.target, &original, request)
}

fn prepare_tombstone(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
    original: &RetiredRecord,
    request: &RequestId,
) -> Result<PreparedOperation> {
    let history = history(snapshot, original)?;
    let (mut prepared, mut retained, retired_at) =
        prepare_archive(root, snapshot, config, original)?;
    let tombstone = Tombstone {
        schema: SchemaVersion::CURRENT,
        repository: config.repository.clone(),
        target: target.clone(),
        request_id: request.clone(),
        retired_at,
        record_path: retained.path().to_owned(),
        previous: original.source().clone(),
        retained: retained.source().clone(),
        record_hash: retained.record_hash()?,
        history,
    };
    validate_shape(&tombstone)?;
    retained.set_retirement(Some(tombstone.clone()));
    prepared.changes.push(FileChange {
        path: target.path(),
        expected: None,
        content: Some(
            serde_yaml_ng::to_string(&tombstone)
                .map_err(|error| invalid(error.to_string()))?
                .into_bytes(),
        ),
    });
    prepared.result = value(&RetirementOutcome {
        tombstone,
        record: retained,
    })?;
    Ok(prepared)
}

impl RetiredRecord {
    fn source(&self) -> &SourceToken {
        match self {
            Self::Issue(record) => &record.source,
            Self::Planning(record) => &record.source,
            Self::Feature(record) => &record.source,
            Self::Gate(record) => &record.source,
        }
    }
    fn path(&self) -> &Path {
        match self {
            Self::Issue(record) => &record.path,
            Self::Planning(record) => &record.path,
            Self::Feature(record) => &record.path,
            Self::Gate(record) => &record.path,
        }
    }
    fn record_hash(&self) -> Result<ContentHash> {
        let record = match self {
            Self::Issue(record) => json!({"metadata":record.metadata,"body":record.body}),
            Self::Planning(record) => json!({"metadata":record.metadata,"body":record.body}),
            Self::Feature(record) => json!({"metadata":record.metadata,"body":record.body}),
            Self::Gate(record) => json!({"definition":record.definition}),
        };
        hash(&record)
    }
    fn set_retirement(&mut self, retirement: Option<Tombstone>) {
        match self {
            Self::Issue(record) => record.retirement = retirement,
            Self::Planning(record) => record.retirement = retirement,
            Self::Feature(record) => record.retirement = retirement,
            Self::Gate(record) => record.retirement = retirement,
        }
    }
    fn archived(&self) -> bool {
        match self {
            Self::Issue(record) => record.metadata.archived,
            Self::Planning(record) => record.metadata.archived,
            Self::Feature(record) => record.metadata.archived,
            Self::Gate(record) => record.definition.archived,
        }
    }
    fn updated_at(&self) -> Option<Timestamp> {
        match self {
            Self::Issue(record) => Some(record.metadata.updated_at),
            Self::Planning(record) => record.metadata.updated_at,
            Self::Feature(record) => Some(record.metadata.updated_at),
            Self::Gate(record) => Some(record.definition.updated_at),
        }
    }
}

fn read_record(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<RetiredRecord> {
    target.validate()?;
    if target.kind == RetirementKind::Feature {
        return crate::features::load_feature(snapshot, config, &target.id.parse()?)
            .map(|record| RetiredRecord::Feature(Box::new(record)));
    }
    if target.kind == RetirementKind::Gate {
        return crate::gates::load_gate(snapshot, config, &target.id.parse()?)
            .map(|record| RetiredRecord::Gate(Box::new(record)));
    }
    if let Some(kind) = target.kind.planning() {
        load_planning(root, snapshot, kind, &target.id)
            .map(|record| RetiredRecord::Planning(Box::new(record)))
    } else {
        resolve_issue(root, snapshot, config, &target.id)
            .map(|record| RetiredRecord::Issue(Box::new(record)))
    }
}

fn build_preview(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<(RetirementPreview, RetiredRecord)> {
    ensure_writable(root, snapshot, config, target)?;
    let record = read_record(root, snapshot, config, target)?;
    let issues = load_issues(root, snapshot, config)?;
    let mut blockers = Vec::new();
    let mut references = Vec::new();
    for issue in &issues {
        references.push(json!({"issue":issue.metadata.id,"path":issue.path,"source":issue.source}));
        let field = match target.kind {
            RetirementKind::Feature
                if issue
                    .metadata
                    .features
                    .iter()
                    .any(|id| id.as_str() == target.id) =>
            {
                Some("features")
            }
            RetirementKind::Gate
                if issue
                    .metadata
                    .gates
                    .iter()
                    .any(|id| id.as_str() == target.id) =>
            {
                Some("gates")
            }
            RetirementKind::Project
                if issue
                    .metadata
                    .project
                    .as_ref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                Some("project")
            }
            RetirementKind::Milestone
                if issue
                    .metadata
                    .milestone
                    .as_ref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                Some("milestone")
            }
            RetirementKind::Target
                if issue
                    .metadata
                    .targets
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                Some("targets")
            }
            RetirementKind::Cycle
                if issue
                    .metadata
                    .cycle
                    .as_ref()
                    .is_some_and(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                Some("cycle")
            }
            RetirementKind::Label
                if issue
                    .metadata
                    .labels
                    .iter()
                    .any(|id| id.eq_ignore_ascii_case(&target.id)) =>
            {
                Some("labels")
            }
            _ => None,
        };
        if let Some(field) = field {
            blockers.push(RetirementBlocker {
                issue: issue.metadata.id.clone(),
                path: issue.path.clone(),
                field: field.into(),
                source: issue.source.clone(),
            });
        }
    }
    let (graph_blockers, graph_fingerprint) =
        crate::graph::retirement_blockers(root, snapshot, config, target)?;
    blockers.extend(graph_blockers);
    let planning_blockers = crate::planning::hierarchy::incoming(root, snapshot, target)?;
    let mut record_blockers = crate::features::incoming(snapshot, config, target)?;
    record_blockers.extend(crate::gates::retirement_blockers(snapshot, config, target)?);
    let question_blockers = crate::questions::retirement_blockers(root, snapshot, config, target)?;
    let config_bytes = snapshot
        .read(Path::new("config.yml"))?
        .ok_or_else(|| corrupt("config disappeared"))?;
    let mut fingerprint_input = json!({"repository":config.repository,"target":target,"source":record.source(),"record_hash":record.record_hash()?,"references":references,"history":history(snapshot,&record)?,"config":ContentHash::of(&config_bytes)});
    if let Some(fingerprint) = graph_fingerprint {
        fingerprint_input["issue_graph"] = value(&fingerprint)?;
    }
    // Omit empty additive membership so old successful proofs retain their shape.
    if !planning_blockers.is_empty() {
        fingerprint_input["planning_references"] = value(&planning_blockers)?;
    }
    if !record_blockers.is_empty() {
        fingerprint_input["record_references"] = value(&record_blockers)?;
    }
    if !question_blockers.is_empty() {
        fingerprint_input["question_references"] = value(&question_blockers)?;
    }
    let fingerprint = hash(&fingerprint_input)?;
    Ok((
        RetirementPreview {
            schema: SchemaVersion::CURRENT,
            repository: config.repository.clone(),
            target: target.clone(),
            source: record.source().clone(),
            fingerprint,
            allowed: blockers.is_empty()
                && planning_blockers.is_empty()
                && record_blockers.is_empty()
                && question_blockers.is_empty(),
            blockers,
            planning_blockers,
            record_blockers,
            question_blockers,
        },
        record,
    ))
}

fn history(snapshot: &Snapshot<'_>, record: &RetiredRecord) -> Result<Vec<RetainedFile>> {
    if matches!(record, RetiredRecord::Feature(_) | RetiredRecord::Gate(_))
        || matches!(record,RetiredRecord::Planning(record) if record.kind==PlanningKind::Label)
    {
        return Ok(Vec::new());
    }
    let directory = record
        .path()
        .parent()
        .ok_or_else(|| invalid("record has no containing directory"))?;
    snapshot
        .list(directory)?
        .into_iter()
        .filter(|path| path != record.path())
        .map(|path| {
            let limit = if is_attachment_payload(record, &path) {
                crate::MAX_ATTACHMENT_BYTES
            } else {
                MAX_DOCUMENT_BYTES
            };
            let bytes = snapshot
                .read_bounded(&path, limit)?
                .ok_or_else(|| corrupt("retained history disappeared").at(&path))?;
            Ok(RetainedFile {
                path,
                content: ContentHash::of(&bytes),
            })
        })
        .collect()
}

fn prepare_archive(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    original: &RetiredRecord,
) -> Result<(PreparedOperation, RetiredRecord, Timestamp)> {
    match original {
        RetiredRecord::Feature(original) => {
            let mut metadata = original.metadata.clone();
            metadata.archived = true;
            metadata.revision = metadata.revision.next()?;
            metadata.updated_at = Utc::now().max(metadata.updated_at);
            let retired_at = metadata.updated_at;
            let (record, changes) = crate::features::prepare_record(
                Some(original),
                metadata,
                &original.body,
                original.path.clone(),
            )?;
            Ok((
                PreparedOperation {
                    changes,
                    result: Value::Null,
                },
                RetiredRecord::Feature(Box::new(record)),
                retired_at,
            ))
        }
        RetiredRecord::Gate(original) => {
            let prepared =
                crate::gates::prepare_archive(root, snapshot, config, &original.definition.id)?;
            let change = prepared
                .changes
                .iter()
                .find(|change| change.path == original.path)
                .ok_or_else(|| corrupt("gate archival did not prepare its source"))?;
            let bytes = change
                .content
                .as_ref()
                .ok_or_else(|| corrupt("gate archival removed its source"))?;
            let record = crate::gates::parse(&original.path, bytes, &config.repository)?;
            let retired_at = record.definition.updated_at;
            Ok((prepared, RetiredRecord::Gate(Box::new(record)), retired_at))
        }
        RetiredRecord::Issue(original) => {
            let bytes = snapshot
                .read(&original.path)?
                .ok_or_else(|| corrupt("issue disappeared"))?;
            let text = std::str::from_utf8(&bytes).map_err(|error| invalid(error.to_string()))?;
            let mut document = MarkdownDocument::parse(&root.join(&original.path), text)?;
            let mut metadata = original.metadata.clone();
            metadata.archived = true;
            metadata.revision = metadata.revision.next()?;
            metadata.updated_at = Utc::now().max(metadata.updated_at);
            metadata.validate(config)?;
            let retired_at = metadata.updated_at;
            replace_metadata(&mut document, &metadata)?;
            let record = record_from_document(metadata, &document, original.path.clone());
            let prepared = PreparedOperation {
                changes: vec![FileChange {
                    path: original.path.clone(),
                    expected: Some(original.source.content.clone()),
                    content: Some(document.render().into_bytes()),
                }],
                result: Value::Null,
            };
            Ok((prepared, RetiredRecord::Issue(Box::new(record)), retired_at))
        }
        RetiredRecord::Planning(original) => {
            let mut metadata = original.metadata.clone();
            metadata.archived = true;
            metadata.revision = metadata.revision.next()?;
            let now = Utc::now();
            let retired_at = now.max(metadata.updated_at.or(metadata.created_at).unwrap_or(now));
            metadata.updated_at = Some(retired_at);
            metadata.validate(original.kind)?;
            let prepared = prepare_write(
                root,
                snapshot,
                original.kind,
                Some(original),
                metadata,
                &original.body,
            )?;
            let record = serde_json::from_value(prepared.result.clone())
                .map_err(|error| invalid(error.to_string()))?;
            Ok((prepared, RetiredRecord::Planning(record), retired_at))
        }
    }
}

/// Called from every ordinary application writer while holding its snapshot.
pub(crate) fn ensure_writable(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<()> {
    if read_tombstone(root, snapshot, config, target)?.is_some() {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "this identity is permanently retired; its authored history is read-only",
        )
        .at(root.join(target.path())));
    }
    Ok(())
}

/// Existing unresolved legacy strings remain declarative, but no newly authored
/// association may target an identity known to be permanently retired.
pub(crate) fn validate_associations(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueMetadata,
) -> Result<()> {
    for (kind, ids) in [
        (
            RetirementKind::Project,
            issue.project.iter().collect::<Vec<_>>(),
        ),
        (RetirementKind::Cycle, issue.cycle.iter().collect()),
        (RetirementKind::Label, issue.labels.iter().collect()),
        (RetirementKind::Milestone, issue.milestone.iter().collect()),
        (RetirementKind::Target, issue.targets.iter().collect()),
    ] {
        for id in ids {
            if let Ok(target) = RetirementTarget::new(kind, id.clone()) {
                ensure_writable(root, snapshot, config, &target)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn read_tombstone(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    target: &RetirementTarget,
) -> Result<Option<Tombstone>> {
    target.validate()?;
    RetirementIndex::capture(root, snapshot, config)?.get(target)
}

fn retirement_receipts(snapshot: &Snapshot<'_>, config: &Config) -> Result<Vec<Tombstone>> {
    let mut markers = Vec::new();
    let mut identities = BTreeSet::new();
    for path in snapshot.list(Path::new("operations"))? {
        let bytes = snapshot
            .read(&path)?
            .ok_or_else(|| corrupt("operation receipt disappeared"))?;
        let receipt: MutationReceipt = serde_yaml_ng::from_slice(&bytes).map_err(|error| {
            corrupt(format!("operation receipt is malformed: {error}")).at(&path)
        })?;
        #[cfg(test)]
        RETIREMENT_PARSES.with(|count| count.set(count.get().saturating_add(1)));
        if receipt.operation != OPERATION && receipt.operation != resolution::OPERATION {
            continue;
        }
        let marker =
            validate_receipt(&receipt, &config.repository).map_err(|error| error.at(&path))?;
        if receipt.repository.as_ref() != Some(&config.repository)
            || marker.repository != config.repository
            || marker.request_id != receipt.request_id
            || path != Path::new(&format!("operations/{}.yml", receipt.operation_id))
        {
            return Err(corrupt(
                "retirement receipt has a mismatched repository, request, or filename",
            )
            .at(path));
        }
        if !identities.insert(format!(
            "{}/{}",
            marker.target.kind.directory(),
            marker.target.id.to_ascii_lowercase()
        )) {
            return Err(corrupt("identity has multiple retirement receipts").at(path));
        }
        markers.push(marker);
    }
    Ok(markers)
}

#[cfg(test)]
std::thread_local! {static RETIREMENT_PARSES: std::cell::Cell<usize> = const {std::cell::Cell::new(0)};}
#[cfg(test)]
pub(crate) fn take_retirement_parse_count() -> usize {
    RETIREMENT_PARSES.with(|count| count.replace(0))
}

fn validate_receipt(receipt: &MutationReceipt, repository: &RepositoryId) -> Result<Tombstone> {
    if receipt.operation == resolution::OPERATION {
        return resolution::validate_receipt(receipt, repository);
    }
    let outcome: RetirementOutcome = serde_json::from_value(receipt.result.clone())
        .map_err(|error| corrupt(format!("retirement receipt result is malformed: {error}")))?;
    let marker = outcome.tombstone;
    validate_shape(&marker)?;
    let target = match &outcome.record {
        RetiredRecord::Feature(record) => {
            crate::features::validate_record(record, repository)?;
            RetirementTarget::new(RetirementKind::Feature, record.metadata.id.as_str())?
        }
        RetiredRecord::Gate(record) => {
            crate::gates::validate_record(record, repository)?;
            RetirementTarget::new(RetirementKind::Gate, record.definition.id.as_str())?
        }
        RetiredRecord::Issue(record) => {
            RetirementTarget::new(RetirementKind::Issue, record.metadata.id.as_str())?
        }
        RetiredRecord::Planning(record) => {
            RetirementTarget::new(record.kind.into(), record.metadata.id.clone())?
        }
    };
    let embedded_marker = match &outcome.record {
        RetiredRecord::Issue(record) => record.retirement.as_ref(),
        RetiredRecord::Planning(record) => record.retirement.as_ref(),
        RetiredRecord::Feature(record) => record.retirement.as_ref(),
        RetiredRecord::Gate(record) => record.retirement.as_ref(),
    };
    let marker_bytes =
        serde_yaml_ng::to_string(&marker).map_err(|error| invalid(error.to_string()))?;
    let marker_hash = ContentHash::of(marker_bytes.as_bytes());
    if receipt.operation != OPERATION
        || receipt.repository.as_ref() != Some(repository)
        || marker.repository != *repository
        || marker.request_id != receipt.request_id
        || target != marker.target
        || embedded_marker != Some(&marker)
        || outcome.record.path() != marker.record_path
        || outcome.record.source() != &marker.retained
        || outcome.record.record_hash()? != marker.record_hash
        || !outcome.record.archived()
        || outcome.record.updated_at() != Some(marker.retired_at)
        || receipt.changed.len() != 2
        || !receipt.changed.iter().any(|change| {
            change.path == marker.record_path
                && change.before.as_ref() == Some(&marker.previous.content)
                && change.after.as_ref() == Some(&marker.retained.content)
        })
        || !receipt.changed.iter().any(|change| {
            change.path == marker.target.path()
                && change.before.is_none()
                && change.after.as_ref() == Some(&marker_hash)
        })
    {
        return Err(corrupt(
            "retirement receipt result or changed paths disagree with its retained record",
        ));
    }
    Ok(marker)
}

fn parse_tombstone(root: &Path, snapshot: &Snapshot<'_>, path: &Path) -> Result<Tombstone> {
    let expected = target_from_path(path)?;
    let bytes = snapshot
        .read_bounded(path, MAX_DOCUMENT_BYTES)?
        .ok_or_else(|| corrupt("retirement marker disappeared").at(path))?;
    let text = std::str::from_utf8(&bytes).map_err(|error| invalid(error.to_string()).at(path))?;
    let document = YamlDocument::parse(&root.join(path), text)?;
    if let Some(schema) = document
        .metadata()
        .get("schema")
        .and_then(serde_yaml_ng::Value::as_u64)
    {
        SchemaVersion::try_from(schema).map_err(|error| error.at(path))?;
    }
    let marker: Tombstone = document.deserialize()?;
    validate_shape(&marker)?;
    let canonical =
        serde_yaml_ng::to_string(&marker).map_err(|error| invalid(error.to_string()))?;
    if ContentHash::of(&bytes) != ContentHash::of(canonical.as_bytes()) {
        return Err(corrupt("generated retirement marker bytes have changed").at(path));
    }
    if marker.target != expected {
        return Err(invalid("retirement marker identity differs from its filename").at(path));
    }
    Ok(marker)
}

fn validate_shape(marker: &Tombstone) -> Result<()> {
    marker.target.validate()?;
    let expected_path = if marker.target.kind == RetirementKind::Feature {
        if crate::features::validate_path(&marker.record_path)?.as_str() != marker.target.id {
            return Err(invalid("feature retirement path differs from identity"));
        }
        marker.record_path.clone()
    } else if marker.target.kind == RetirementKind::Gate {
        PathBuf::from(format!("gates/{}.yml", marker.target.id))
    } else if marker.target.kind == RetirementKind::Label {
        PathBuf::from("labels.yml")
    } else {
        PathBuf::from(format!(
            "{}/{}/item.md",
            marker.target.kind.directory(),
            marker.target.id
        ))
    };
    if marker.record_path != expected_path
        || marker.retained.revision != marker.previous.revision.next()?
    {
        return Err(invalid(
            "retirement record path or revision transition is invalid",
        ));
    }
    let mut paths = BTreeSet::new();
    for retained in &marker.history {
        crate::SourceLink {
            path: retained.path.to_string_lossy().into_owned(),
            line: None,
            end_line: None,
        }
        .validate()?;
        if matches!(
            marker.target.kind,
            RetirementKind::Label | RetirementKind::Feature | RetirementKind::Gate
        ) || retained.path == marker.record_path
            || !retained
                .path
                .starts_with(marker.record_path.parent().expect("canonical record path"))
            || !paths.insert(&retained.path)
        {
            return Err(invalid(
                "retirement history must contain unique files inside the retained record directory",
            ));
        }
    }
    Ok(())
}

fn validate_retained(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    marker: &Tombstone,
) -> Result<()> {
    if marker.repository != config.repository {
        return Err(corrupt("retirement belongs to another repository"));
    }
    let record = read_record(root, snapshot, config, &marker.target)?;
    if record.path() != marker.record_path
        || !record.archived()
        || record.updated_at() != Some(marker.retired_at)
        || record.source().revision != marker.retained.revision
        || record.record_hash()? != marker.record_hash
        || (marker.target.kind != RetirementKind::Label && record.source() != &marker.retained)
        || !history_metadata_matches(snapshot, &record, &marker.history)?
    {
        return Err(
            corrupt("retained record or independent history changed after retirement")
                .at(root.join(&marker.record_path)),
        );
    }
    Ok(())
}

fn history_metadata_matches(
    snapshot: &Snapshot<'_>,
    record: &RetiredRecord,
    retained: &[RetainedFile],
) -> Result<bool> {
    if matches!(record, RetiredRecord::Feature(_) | RetiredRecord::Gate(_))
        || matches!(record, RetiredRecord::Planning(record) if record.kind == PlanningKind::Label)
    {
        return Ok(retained.is_empty());
    }
    let directory = record.path().parent().expect("canonical record path");
    let paths = snapshot
        .list(directory)?
        .into_iter()
        .filter(|path| path != record.path())
        .collect::<Vec<_>>();
    if paths.iter().ne(retained.iter().map(|file| &file.path)) {
        return Ok(false);
    }
    for file in retained {
        if is_attachment_payload(record, &file.path) {
            // Retained descriptor bytes and exact file membership are checked.
            // Actual payload integrity remains an explicit attachment read; a
            // metadata inspection must not turn into arbitrary binary loading.
            continue;
        }
        let bytes = snapshot
            .read_bounded(&file.path, MAX_DOCUMENT_BYTES)?
            .ok_or_else(|| corrupt("retained metadata disappeared").at(&file.path))?;
        if ContentHash::of(&bytes) != file.content {
            return Ok(false);
        }
    }
    Ok(true)
}

fn is_attachment_payload(record: &RetiredRecord, path: &Path) -> bool {
    if !matches!(record, RetiredRecord::Issue(_)) {
        return false;
    }
    let Some(relative) = record
        .path()
        .parent()
        .and_then(|directory| path.strip_prefix(directory).ok())
    else {
        return false;
    };
    let parts = relative
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>();
    parts
        .as_ref()
        .is_some_and(|parts| parts.len() == 4 && parts[0] == "attachments" && parts[2] == "content")
}

fn target_from_path(path: &Path) -> Result<RetirementTarget> {
    let parts = path
        .components()
        .map(|part| part.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| invalid("retirement path must be UTF-8"))?;
    if parts.len() != 3 || parts[0] != "tombstones" || !parts[2].ends_with(".yml") {
        return Err(invalid("retirement markers belong in tombstones/<kind>/<id>.yml").at(path));
    }
    let kind = match parts[1] {
        "issues" => RetirementKind::Issue,
        "features" => RetirementKind::Feature,
        "gates" => RetirementKind::Gate,
        "initiatives" => RetirementKind::Initiative,
        "projects" => RetirementKind::Project,
        "milestones" => RetirementKind::Milestone,
        "cycles" => RetirementKind::Cycle,
        "targets" => RetirementKind::Target,
        "labels" => RetirementKind::Label,
        _ => return Err(invalid("unknown retirement record kind").at(path)),
    };
    RetirementTarget::new(kind, parts[2].strip_suffix(".yml").expect("suffix checked"))
}

fn same_identity(left: &RetirementTarget, right: &RetirementTarget) -> bool {
    left.kind == right.kind && left.id.eq_ignore_ascii_case(&right.id)
}
fn hash(value: &impl Serialize) -> Result<ContentHash> {
    Ok(ContentHash::of(
        &serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))?,
    ))
}
fn value(value: &impl Serialize) -> Result<Value> {
    serde_json::to_value(value).map_err(|error| invalid(error.to_string()))
}
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn corrupt(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::CorruptStore, message)
}

pub(crate) fn doctor(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
) -> (usize, Vec<PmError>) {
    let mut count = 0;
    let mut errors = Vec::new();
    let mut seen = BTreeSet::new();
    match snapshot.list(Path::new("tombstones")) {
        Ok(paths) => {
            for path in paths {
                count += 1;
                let result = target_from_path(&path).and_then(|target| {
                    seen.insert(format!(
                        "{}/{}",
                        target.kind.directory(),
                        target.id.to_ascii_lowercase()
                    ));
                    read_tombstone(root, snapshot, config, &target).map(|_| ())
                });
                if let Err(error) = result {
                    errors.push(if error.path.is_some() {
                        error
                    } else {
                        error.at(root.join(path))
                    });
                }
            }
        }
        Err(error) => errors.push(error),
    }
    match retirement_receipts(snapshot, config) {
        Ok(markers) => {
            for marker in markers {
                if !seen.contains(&format!(
                    "{}/{}",
                    marker.target.kind.directory(),
                    marker.target.id.to_ascii_lowercase()
                )) {
                    count += 1;
                    errors.push(
                        corrupt("retirement receipt has no permanent marker")
                            .at(root.join(marker.target.path())),
                    );
                }
            }
        }
        Err(error) => errors.push(error),
    }
    (count, errors)
}
