use crate::{
    Config, ErrorCode, IssueId, IssueMetadata, ManualAcceptance, PmError, QualifiedRef, Repository,
    RequestId, Result, Revision, SchemaVersion, SourceToken, Timestamp, WorkflowCategory,
    documents::{MAX_DOCUMENT_BYTES, MarkdownDocument},
    repository::config_from_snapshot,
    transactions::{FileChange, MutationReceipt, PreparedOperation, Snapshot},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use serde_yaml_ng::{Mapping, Value as YamlValue};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueRecord {
    pub metadata: IssueMetadata,
    pub body: String,
    pub path: PathBuf,
    pub source: SourceToken,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<crate::Tombstone>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateIssue {
    pub title: String,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}
impl CreateIssue {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            fields: BTreeMap::new(),
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateIssueInput {
    pub template: String,
    pub title: String,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateIssue {
    #[serde(default)]
    pub fields: BTreeMap<String, Value>,
    #[serde(default)]
    pub body: Option<String>,
}

#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCollection {
    Labels,
    Files,
    Commits,
    Documents,
}

impl IssueCollection {
    fn key(self) -> &'static str {
        match self {
            Self::Labels => "labels",
            Self::Files => "files",
            Self::Commits => "commits",
            Self::Documents => "documents",
        }
    }
}

/// Semantic intents keep retries stable. Adapters never need to read a record
/// and manufacture a different update payload merely to add a label or link.
#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum IssueMutation {
    PatchCustom {
        patch: crate::CustomPatch,
    },
    EditDocument {
        markdown: String,
    },
    Update {
        input: UpdateIssue,
    },
    UpdateAndAdd {
        input: UpdateIssue,
        field: IssueCollection,
        values: Vec<Value>,
    },
    Complete {
        manual: Option<ManualAcceptanceInput>,
    },
    Cancel,
    Reopen,
    Archive {
        archived: bool,
    },
    Comment {
        author: String,
        body: String,
    },
    Add {
        field: IssueCollection,
        value: Value,
    },
    Remove {
        field: IssueCollection,
        value: Value,
    },
}

impl IssueMutation {
    fn operation(&self) -> &'static str {
        match self {
            Self::PatchCustom { .. } => "issue.custom",
            Self::EditDocument { .. } => "issue.edit",
            Self::Update { .. } | Self::UpdateAndAdd { .. } => "issue.update",
            Self::Complete { .. } => "issue.done",
            Self::Cancel => "issue.cancel",
            Self::Reopen => "issue.reopen",
            Self::Archive { .. } => "issue.archive",
            Self::Comment { .. } => "issue.comment",
            Self::Add { .. } => "issue.add",
            Self::Remove { .. } => "issue.remove",
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualAcceptanceInput {
    pub actor: String,
    pub reason: String,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionReport {
    pub issue: IssueId,
    pub source: SourceToken,
    pub allowed: bool,
    /// `declared` and `manual` are not verified local checks or CI qualification.
    pub basis: String,
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<crate::CompletionCondition>,
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommentRecord {
    pub schema: SchemaVersion,
    pub id: IssueId,
    pub issue: IssueId,
    pub author: String,
    pub created_at: Timestamp,
    pub revision: Revision,
    pub body: String,
    pub path: PathBuf,
}

/// Only persisted frontmatter belongs here. Body and path come from the source
/// document, so injected fields cannot silently shadow those derived values.
#[derive(schemars::JsonSchema, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CommentHeader {
    schema: SchemaVersion,
    id: IssueId,
    issue: IssueId,
    author: String,
    created_at: Timestamp,
    revision: Revision,
}

impl Repository {
    pub fn list_issues(&self) -> Result<Vec<IssueRecord>> {
        self.query_issues(&crate::IssueQuery::all())
    }

    pub fn show_issue(&self, reference: &str) -> Result<IssueRecord> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let mut issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            issue.retirement = crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &crate::RetirementTarget::new(
                    crate::RetirementKind::Issue,
                    issue.metadata.id.as_str(),
                )?,
            )?;
            Ok(issue)
        })
    }

    pub fn issue_markdown(&self, reference: &str) -> Result<String> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &crate::RetirementTarget::new(
                    crate::RetirementKind::Issue,
                    issue.metadata.id.as_str(),
                )?,
            )?;
            let bytes = snapshot
                .read(&issue.path)?
                .ok_or_else(|| PmError::new(ErrorCode::StaleSource, "issue disappeared"))?;
            String::from_utf8(bytes).map_err(|_| {
                PmError::new(ErrorCode::InvalidSchema, "issue must be UTF-8").at(issue.path)
            })
        })
    }

    pub fn create_issue(
        &self,
        input: &CreateIssue,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.store()?
            .transact(request, "issue.create", &to_json(input)?, |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                prepare_create_issue(self.root(), snapshot, &config, input)
            })
    }

    pub fn create_issue_from_template(
        &self,
        input: &TemplateIssueInput,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.store()?.transact(
            request,
            "issue.create_template",
            &to_json(input)?,
            |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                let template = crate::templates::load_template(
                    self.root(),
                    snapshot,
                    &config,
                    &input.template,
                )?;
                let resolved = template.apply(
                    input.title.clone(),
                    input.body.clone(),
                    input.fields.clone(),
                );
                prepare_create_issue(self.root(), snapshot, &config, &resolved)
            },
        )
    }

    pub fn update_issue(
        &self,
        reference: &str,
        expected: &SourceToken,
        input: &UpdateIssue,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_issue(
            reference,
            Some(expected),
            &IssueMutation::Update {
                input: input.clone(),
            },
            request,
        )
    }

    pub fn completion_report(&self, reference: &str) -> Result<CompletionReport> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            crate::graph::preflight(snapshot)?;
            let mut issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            issue.retirement = crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &crate::RetirementTarget::new(
                    crate::RetirementKind::Issue,
                    issue.metadata.id.as_str(),
                )?,
            )?;
            let mut report = completion(self.root(), snapshot, &config, &issue)?;
            if let Err(error) = crate::organization::validate_issue_change(
                snapshot,
                &config,
                Some(&issue.metadata),
                &issue.metadata,
                true,
            ) {
                report.allowed = false;
                report.reasons.push(error.message);
            }
            Ok(report)
        })
    }

    pub fn complete_issue(
        &self,
        reference: &str,
        expected: &SourceToken,
        manual: Option<&ManualAcceptanceInput>,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_issue(
            reference,
            Some(expected),
            &IssueMutation::Complete {
                manual: manual.cloned(),
            },
            request,
        )
    }

    pub fn reopen_issue(
        &self,
        reference: &str,
        expected: &SourceToken,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_issue(reference, Some(expected), &IssueMutation::Reopen, request)
    }

    pub fn archive_issue(
        &self,
        reference: &str,
        expected: &SourceToken,
        archived: bool,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_issue(
            reference,
            Some(expected),
            &IssueMutation::Archive { archived },
            request,
        )
    }

    pub fn add_comment(
        &self,
        reference: &str,
        expected: &SourceToken,
        author: &str,
        body: &str,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        self.mutate_issue(
            reference,
            Some(expected),
            &IssueMutation::Comment {
                author: author.into(),
                body: body.into(),
            },
            request,
        )
    }

    /// A missing expected token means "apply this intent to the current locked
    /// snapshot", not "overwrite unconditionally". Replay lookup precedes all
    /// source resolution, generated IDs, collection edits and workflow choices.
    pub fn mutate_issue(
        &self,
        reference: &str,
        expected: Option<&SourceToken>,
        mutation: &IssueMutation,
        request: &RequestId,
    ) -> Result<MutationReceipt> {
        let parameters = json!({"reference":reference,"expected":expected,"mutation":mutation});
        self.store()?
            .transact(request, mutation.operation(), &parameters, |snapshot| {
                let config = config_from_snapshot(self.root(), snapshot)?;
                crate::graph::preflight(snapshot)?;
                let original = resolve_issue(self.root(), snapshot, &config, reference)?;
                prepare_issue_mutation(self.root(), snapshot, &config, original, expected, mutation)
            })
    }

    pub fn comments(&self, reference: &str) -> Result<Vec<CommentRecord>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let issue = resolve_issue(self.root(), snapshot, &config, reference)?;
            crate::retirement::read_tombstone(
                self.root(),
                snapshot,
                &config,
                &crate::RetirementTarget::new(
                    crate::RetirementKind::Issue,
                    issue.metadata.id.as_str(),
                )?,
            )?;
            load_comments(self.root(), snapshot, &issue.metadata.id)
        })
    }
}

/// Independent-record readers and doctor share the same snapshot validation.
pub(crate) fn load_comments(
    root: &Path,
    snapshot: &Snapshot<'_>,
    issue: &IssueId,
) -> Result<Vec<CommentRecord>> {
    let prefix = PathBuf::from(format!("issues/{issue}/comments"));
    let mut comments = snapshot
        .list(&prefix)?
        .iter()
        .map(|path| load_comment(root, snapshot, path, issue))
        .collect::<Result<Vec<_>>>()?;
    comments.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then(left.id.cmp(&right.id))
    });
    Ok(comments)
}

/// Validate one canonical comment without requiring its parent issue to parse.
/// Doctor can report multiple independent-record errors in a single snapshot.
pub(crate) fn load_comment(
    root: &Path,
    snapshot: &Snapshot<'_>,
    path: &Path,
    issue: &IssueId,
) -> Result<CommentRecord> {
    let absolute = root.join(path);
    let prefix = PathBuf::from(format!("issues/{issue}/comments"));
    if path.parent() != Some(prefix.as_path())
        || path.extension().is_none_or(|extension| extension != "md")
    {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "comments belong directly in issues/<issue>/comments/<COM-ID>.md",
        )
        .at(&absolute));
    }
    let id = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| stem.parse::<IssueId>().ok())
        .filter(|id| id.as_str().starts_with("COM-"))
        .ok_or_else(|| {
            PmError::new(
                ErrorCode::InvalidSchema,
                "comment filename must contain a full COM-prefixed ULID",
            )
            .at(&absolute)
        })?;
    let bytes = snapshot
        .read_bounded(path, MAX_DOCUMENT_BYTES)?
        .ok_or_else(|| PmError::new(ErrorCode::StaleSource, "comment disappeared").at(&absolute))?;
    let text = std::str::from_utf8(&bytes).map_err(|_| {
        PmError::new(ErrorCode::InvalidSchema, "comment must be UTF-8").at(&absolute)
    })?;
    let document = MarkdownDocument::parse(&absolute, text)?;
    validate_document_schema(&absolute, &document)?;
    let header: CommentHeader = document.deserialize()?;
    if header.id != id || header.issue != *issue {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "comment identity or parent issue disagrees with its canonical path",
        )
        .at(&absolute));
    }
    validate_comment_content(&header.author, document.body())
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.message).at(&absolute))?;
    Ok(CommentRecord {
        schema: header.schema,
        id: header.id,
        issue: header.issue,
        author: header.author,
        created_at: header.created_at,
        revision: header.revision,
        body: document.body().into(),
        path: path.to_owned(),
    })
}

fn prepare_create_issue(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    input: &CreateIssue,
) -> Result<PreparedOperation> {
    let metadata = IssueMetadata::new(config, &input.title, Utc::now())?;
    let mut document = new_document(Path::new("new-issue.md"), &metadata, &input.body)?;
    apply_fields(&mut document, &input.fields)?;
    let mut metadata: IssueMetadata = document.deserialize()?;
    metadata.status = config.workflow.canonical_status(&metadata.status)?.into();
    let category = config.workflow.state(&metadata.status)?.category;
    if matches!(
        category,
        WorkflowCategory::Completed | WorkflowCategory::Canceled
    ) {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "create an active issue before completing or canceling it",
        ));
    }
    metadata.validate(config)?;
    crate::organization::validate_issue_change(snapshot, config, None, &metadata, true)?;
    crate::retirement::ensure_writable(
        root,
        snapshot,
        config,
        &crate::RetirementTarget::new(crate::RetirementKind::Issue, metadata.id.as_str())?,
    )?;
    crate::retirement::validate_associations(root, snapshot, config, &metadata)?;
    crate::planning::hierarchy::validate_issue_change(root, snapshot, config, None, &metadata)?;
    crate::graph::validate_issue_change(root, snapshot, config, None, &metadata)?;
    crate::features::validate_issue_associations(root, snapshot, config, None, &metadata)?;
    crate::gates::validate_issue_associations(root, snapshot, config, None, &metadata)?;
    replace_metadata(&mut document, &metadata)?;
    let path = PathBuf::from(format!("issues/{}/item.md", metadata.id));
    let bytes = document.render().into_bytes();
    let record = record_from_document(metadata, &document, path.clone());
    Ok(PreparedOperation {
        changes: vec![FileChange {
            path,
            expected: None,
            content: Some(bytes),
        }],
        result: to_json(&record)?,
    })
}

fn prepare_comment(issue: &IssueRecord, author: &str, body: &str) -> Result<PreparedOperation> {
    validate_comment_content(author, body)?;
    let id = IssueId::new("COM")?;
    let now = Utc::now().max(issue.metadata.created_at);
    let path = PathBuf::from(format!("issues/{}/comments/{id}.md", issue.metadata.id));
    let header = CommentHeader {
        schema: SchemaVersion::CURRENT,
        id: id.clone(),
        issue: issue.metadata.id.clone(),
        author: author.into(),
        created_at: now,
        revision: Revision::INITIAL,
    };
    let document = new_document(&path, &header, body)?;
    let comment = CommentRecord {
        schema: SchemaVersion::CURRENT,
        id,
        issue: issue.metadata.id.clone(),
        author: author.into(),
        created_at: now,
        revision: Revision::INITIAL,
        body: body.into(),
        path: path.clone(),
    };
    Ok(PreparedOperation {
        changes: vec![FileChange {
            path,
            expected: None,
            content: Some(document.render().into_bytes()),
        }],
        result: json!({"issue":issue,"comment":comment}),
    })
}

fn validate_comment_content(author: &str, body: &str) -> Result<()> {
    if author.trim().is_empty() || author.chars().any(char::is_control) || body.trim().is_empty() {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "comments require a nonempty body and a nonempty author without control characters",
        ));
    }
    Ok(())
}

fn cancellation_status(config: &Config, current: &str) -> Result<String> {
    if config.workflow.state(current)?.category == WorkflowCategory::Canceled {
        return Ok(current.into());
    }
    let mut targets = config
        .workflow
        .states
        .iter()
        .filter(|state| state.category == WorkflowCategory::Canceled);
    if let Some(default) = targets.clone().find(|state| state.id == "canceled") {
        return Ok(default.id.clone());
    }
    let first = targets
        .next()
        .ok_or_else(|| PmError::new(ErrorCode::InvalidSchema, "workflow has no canceled state"))?;
    if targets.next().is_some() {
        return Err(PmError::new(ErrorCode::AmbiguousReference, "workflow has multiple canceled states and no canonical canceled target").hint("Use issue move with an exact canceled state ID, or configure a canonical canceled state."));
    }
    Ok(first.id.clone())
}

fn change_collection(
    document: &mut MarkdownDocument,
    field: IssueCollection,
    value: &Value,
    add: bool,
) -> Result<()> {
    if field == IssueCollection::Files {
        let link: crate::SourceLink = serde_json::from_value(value.clone())
            .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?;
        link.validate()?;
    } else if !value
        .as_str()
        .is_some_and(|text| !text.trim().is_empty() && !text.chars().any(char::is_control))
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "collection values must be nonempty text without control characters",
        ));
    }
    let value = serde_yaml_ng::to_value(value)
        .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?;
    let mut values = match document.metadata().get(field.key()) {
        None => Vec::new(),
        Some(YamlValue::Sequence(values)) => values.clone(),
        Some(_) => {
            return Err(PmError::new(
                ErrorCode::InvalidSchema,
                format!("{} must be a sequence", field.key()),
            ));
        }
    };
    if add {
        if !values.contains(&value) {
            values.push(value);
        }
    } else if field == IssueCollection::Files
        && value
            .as_mapping()
            .is_some_and(|mapping| mapping.len() == 1 && mapping.contains_key("path"))
    {
        let path = value.get("path");
        values.retain(|existing| existing.get("path") != path);
    } else {
        values.retain(|existing| existing != &value);
    }
    let changes = Mapping::from_iter([(
        YamlValue::String(field.key().into()),
        YamlValue::Sequence(values),
    )]);
    document.patch(&changes)?;
    Ok(())
}

pub(crate) fn completion(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueRecord,
) -> Result<CompletionReport> {
    completion_admitted(root, snapshot, config, issue, None)
}
pub(crate) fn completion_admitted(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    issue: &IssueRecord,
    admission: Option<&crate::completion::Admission>,
) -> Result<CompletionReport> {
    let mut reasons = Vec::new();
    if issue.retirement.is_some() {
        reasons.push("retired work is permanently read-only and cannot be completed".into());
    }
    if admission.is_none()
        && let Err(error) = config.acceptance.ensure_supported_for_completion()
    {
        reasons.push(error.message);
    }
    if config.acceptance.require_description && issue.body.trim().is_empty() {
        reasons.push("a description is required".into());
    }
    if config.acceptance.require_all_criteria {
        for criterion in &issue.metadata.acceptance {
            if !criterion.checked {
                reasons.push(format!(
                    "acceptance criterion {} is incomplete: {}",
                    criterion.id, criterion.description
                ));
            }
        }
    }
    if config
        .workflow
        .state(&issue.metadata.status)
        .is_ok_and(|state| state.category == WorkflowCategory::Canceled)
    {
        reasons.push("canceled issues must be explicitly reopened before completion".into());
    }
    let mut conditions = crate::graph::completion_conditions(root, snapshot, config, issue)?;
    conditions.extend(if let Some(admission) = admission {
        admission.conditions(config, issue)?
    } else {
        crate::gates::issue_conditions(root, snapshot, config, issue)?
    });
    reasons.extend(
        conditions
            .iter()
            .filter(|c| c.state != crate::ConditionState::Satisfied)
            .map(|c| c.message.clone()),
    );
    Ok(CompletionReport {
        issue: issue.metadata.id.clone(),
        source: issue.source.clone(),
        allowed: reasons.is_empty(),
        basis: if admission.is_some() {
            "authenticated_checks"
        } else if issue.metadata.manual_acceptance.is_some() {
            "manual"
        } else if issue
            .metadata
            .imported_completion
            .as_ref()
            .is_some_and(crate::ImportedCompletion::is_active)
        {
            "imported"
        } else {
            "declared"
        }
        .into(),
        reasons,
        conditions,
    })
}

fn check_expected(issue: &IssueRecord, expected: &SourceToken) -> Result<()> {
    if &issue.source != expected {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "issue changed since it was read (revision or content differs)",
        )
        .at(&issue.path)
        .hint("Reload the issue, reconcile the change, and submit its current source token."));
    }
    Ok(())
}

pub(crate) const ISSUE_WRITABLE_FIELDS: &[&str] = &[
    "title",
    "status",
    "priority",
    "assignee",
    "reporter",
    "reviewer",
    "due_at",
    "archived",
    "project",
    "milestone",
    "parent",
    "prerequisites",
    "features",
    "gates",
    "targets",
    "cycle",
    "labels",
    "files",
    "commits",
    "documents",
    "acceptance",
    "custom",
    "estimate",
];

fn apply_fields(document: &mut MarkdownDocument, fields: &BTreeMap<String, Value>) -> Result<()> {
    for key in fields.keys() {
        if !ISSUE_WRITABLE_FIELDS.contains(&key.as_str()) && !key.starts_with("x-") {
            return Err(PmError::new(
                ErrorCode::InvalidInput,
                format!("field {key:?} cannot be changed through issue update"),
            ));
        }
    }
    let patch = serde_yaml_ng::to_value(fields)
        .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))?;
    document.patch(patch.as_mapping().ok_or_else(|| {
        PmError::new(
            ErrorCode::InvalidInput,
            "issue update fields must be a mapping",
        )
    })?)?;
    Ok(())
}

fn new_document(path: &Path, metadata: &impl Serialize, body: &str) -> Result<MarkdownDocument> {
    let yaml = serde_yaml_ng::to_string(metadata)
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
    Ok(MarkdownDocument::parse(
        path,
        &format!("---\n{yaml}---\n{body}"),
    )?)
}

pub(crate) fn replace_metadata(
    document: &mut MarkdownDocument,
    metadata: &IssueMetadata,
) -> Result<()> {
    let value = serde_yaml_ng::to_value(metadata)
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
    let updated = value.as_mapping().ok_or_else(|| {
        PmError::new(ErrorCode::InvalidSchema, "issue metadata must be a mapping")
    })?;
    let mut changes = Mapping::new();
    for (key, value) in updated {
        if document.metadata().get(key) != Some(value) {
            changes.insert(key.clone(), value.clone());
        }
    }
    for key in document.metadata().keys() {
        if !updated.contains_key(key) {
            changes.insert(key.clone(), YamlValue::Null);
        }
    }
    document.patch(&changes)?;
    Ok(())
}

pub(crate) fn record_from_document(
    metadata: IssueMetadata,
    document: &MarkdownDocument,
    path: PathBuf,
) -> IssueRecord {
    let source = SourceToken::new(metadata.revision, document.render().as_bytes());
    IssueRecord {
        metadata,
        body: document.body().into(),
        path,
        source,
        retirement: None,
    }
}

fn parse_markdown(path: &Path, bytes: &[u8]) -> Result<MarkdownDocument> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| PmError::new(ErrorCode::InvalidSchema, "issue must be UTF-8").at(path))?;
    Ok(MarkdownDocument::parse(path, text)?)
}

fn validate_document_schema(path: &Path, document: &MarkdownDocument) -> Result<()> {
    if let Some(schema) = document
        .metadata()
        .get("schema")
        .and_then(YamlValue::as_u64)
    {
        SchemaVersion::try_from(schema).map_err(|error| error.at(path))?;
    }
    Ok(())
}

/// Preserve the unsupported-version category before serde converts a typed
/// schema error into a general document diagnostic.
pub(crate) fn parse_issue_metadata(
    path: &Path,
    document: &MarkdownDocument,
) -> Result<IssueMetadata> {
    validate_document_schema(path, document)?;
    Ok(document.deserialize()?)
}

pub(crate) fn load_issues(
    root: &Path,
    snapshot: &Snapshot,
    config: &Config,
) -> Result<Vec<IssueRecord>> {
    let mut issues = Vec::new();
    let mut ids = std::collections::BTreeSet::new();
    for path in snapshot.list(Path::new("issues"))? {
        if path.file_name().is_none_or(|name| name != "item.md") || path.components().count() != 3 {
            continue;
        }
        let bytes = snapshot
            .read(&path)?
            .ok_or_else(|| PmError::new(ErrorCode::StaleSource, "issue disappeared").at(&path))?;
        let document = parse_markdown(&root.join(&path), &bytes)?;
        let metadata = parse_issue_metadata(&root.join(&path), &document)?;
        metadata.validate(config).map_err(|error| error.at(&path))?;
        if path
            .parent()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            != Some(metadata.id.as_str())
            || !ids.insert(metadata.id.clone())
        {
            return Err(PmError::new(
                ErrorCode::InvalidSchema,
                "issue identity is duplicated or differs from its filename",
            )
            .at(&path));
        }
        issues.push(record_from_document(metadata, &document, path));
    }
    issues.sort_by(|left, right| {
        left.metadata
            .created_at
            .cmp(&right.metadata.created_at)
            .then(left.metadata.id.cmp(&right.metadata.id))
    });
    Ok(issues)
}

pub(crate) fn resolve_issue(
    root: &Path,
    snapshot: &Snapshot,
    config: &Config,
    reference: &str,
) -> Result<IssueRecord> {
    let reference = if reference.contains("::") {
        let qualified: QualifiedRef = reference.parse()?;
        if qualified.repository != config.repository {
            return Err(PmError::new(
                ErrorCode::NotFound,
                "reference belongs to a different planning repository",
            ));
        }
        qualified.record.to_string()
    } else {
        reference.into()
    };
    if reference.len() < 4
        || !reference
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "issue references require an ID or at least four unambiguous prefix characters",
        ));
    }
    let issues = load_issues(root, snapshot, config)?;
    if let Some(issue) = issues
        .iter()
        .find(|issue| issue.metadata.id.as_str() == reference)
    {
        return Ok(issue.clone());
    }
    let mut matches = issues
        .into_iter()
        .filter(|issue| issue.metadata.id.as_str().starts_with(&reference));
    let first = matches.next().ok_or_else(|| {
        PmError::new(
            ErrorCode::NotFound,
            format!("issue {reference:?} was not found"),
        )
    })?;
    if matches.next().is_some() {
        return Err(PmError::new(
            ErrorCode::AmbiguousReference,
            format!("issue prefix {reference:?} matches multiple issues"),
        ));
    }
    Ok(first)
}

fn to_json(value: &impl Serialize) -> Result<Value> {
    serde_json::to_value(value)
        .map_err(|error| PmError::new(ErrorCode::InvalidInput, error.to_string()))
}

/// Prepare one semantic issue intent inside an existing application transaction.
/// Composite operations use the same workflow, acceptance, and retirement rules.
pub(crate) fn prepare_issue_mutation(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    original: IssueRecord,
    expected: Option<&SourceToken>,
    mutation: &IssueMutation,
) -> Result<PreparedOperation> {
    prepare_issue_mutation_admitted(root, snapshot, config, original, expected, mutation, None)
}
pub(crate) fn prepare_issue_mutation_admitted(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    original: IssueRecord,
    expected: Option<&SourceToken>,
    mutation: &IssueMutation,
    admission: Option<&crate::completion::Admission>,
) -> Result<PreparedOperation> {
    crate::retirement::ensure_writable(
        root,
        snapshot,
        config,
        &crate::RetirementTarget::new(crate::RetirementKind::Issue, original.metadata.id.as_str())?,
    )?;
    if let Some(expected) = expected {
        check_expected(&original, expected)?;
    }
    if let IssueMutation::Comment { author, body } = mutation {
        let prepared = prepare_comment(&original, author, body)?;
        crate::organization::validate_actor(snapshot, &config.repository, author)?;
        return Ok(prepared);
    }
    let bytes = snapshot
        .read(&original.path)?
        .ok_or_else(|| PmError::new(ErrorCode::StaleSource, "issue disappeared"))?;
    let mut document = parse_markdown(&root.join(&original.path), &bytes)?;
    match mutation {
        IssueMutation::PatchCustom { patch } => {
            let custom = patch.apply(&original.metadata.custom)?;
            apply_fields(
                &mut document,
                &BTreeMap::from([("custom".into(), json!(custom))]),
            )?;
        }
        IssueMutation::EditDocument { markdown } => {
            let edited = MarkdownDocument::parse(&root.join(&original.path), markdown)?;
            let metadata = parse_issue_metadata(&root.join(&original.path), &edited)?;
            if metadata.schema != original.metadata.schema
                || metadata.id != original.metadata.id
                || metadata.revision != original.metadata.revision
                || metadata.created_at != original.metadata.created_at
                || metadata.updated_at != original.metadata.updated_at
                || metadata.completed_at != original.metadata.completed_at
                || metadata.canceled_at != original.metadata.canceled_at
                || metadata.manual_acceptance != original.metadata.manual_acceptance
                || metadata.imported_completion != original.metadata.imported_completion
            {
                return Err(PmError::new(
                    ErrorCode::InvalidInput,
                    "the editor may change planning fields and Markdown, but must preserve identity, revision, managed timestamps, and acceptance provenance",
                ));
            }
            document = edited;
        }
        IssueMutation::Update { input } | IssueMutation::UpdateAndAdd { input, .. } => {
            apply_fields(&mut document, &input.fields)?;
            if let Some(body) = &input.body {
                document.set_body(body.clone())?;
            }
            if let IssueMutation::UpdateAndAdd { field, values, .. } = mutation {
                for value in values {
                    change_collection(&mut document, *field, value, true)?;
                }
            }
        }
        IssueMutation::Add { field, value } => {
            change_collection(&mut document, *field, value, true)?
        }
        IssueMutation::Remove { field, value } => {
            change_collection(&mut document, *field, value, false)?
        }
        IssueMutation::Archive { archived } => apply_fields(
            &mut document,
            &BTreeMap::from([("archived".into(), json!(archived))]),
        )?,
        IssueMutation::Complete { .. } | IssueMutation::Cancel | IssueMutation::Reopen => {}
        IssueMutation::Comment { .. } => {
            unreachable!("comments were prepared without rewriting the issue")
        }
    }
    let mut metadata: IssueMetadata = document.deserialize()?;
    if matches!(mutation, IssueMutation::Complete { .. }) {
        metadata.status = config
            .workflow
            .states
            .iter()
            .find(|state| state.category == WorkflowCategory::Completed)
            .ok_or_else(|| {
                PmError::new(ErrorCode::InvalidSchema, "workflow has no completed state")
            })?
            .id
            .clone();
    } else if matches!(mutation, IssueMutation::Reopen) {
        metadata.status = config.workflow.initial.clone();
    } else if matches!(mutation, IssueMutation::Cancel) {
        metadata.status = cancellation_status(config, &original.metadata.status)?;
    }
    metadata.status = config.workflow.canonical_status(&metadata.status)?.into();
    let old_category = config.workflow.state(&original.metadata.status)?.category;
    let new_category = config.workflow.state(&metadata.status)?.category;
    let reopen = matches!(mutation, IssueMutation::Reopen);
    if reopen {
        if !matches!(
            old_category,
            WorkflowCategory::Completed | WorkflowCategory::Canceled
        ) {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "only completed or canceled issues can be reopened",
            ));
        }
        metadata.completed_at = None;
        metadata.canceled_at = None;
        metadata.manual_acceptance = None;
        if let Some(imported) = &mut metadata.imported_completion
            && imported.is_active()
        {
            imported.superseded_at = Some(Utc::now().max(imported.imported_at));
        }
    } else {
        config
            .workflow
            .transition(&original.metadata.status, &metadata.status)?;
    }

    // Accepted work cannot quietly become a different subject while its
    // old acceptance remains attached. Explicit reopen clears that state.
    let archive_only = matches!(mutation, IssueMutation::Archive { .. })
        || matches!(mutation, IssueMutation::Update { input } if input.body.is_none() && input.fields.keys().all(|key| key == "archived"));
    if old_category == WorkflowCategory::Completed
        && !reopen
        && !archive_only
        && (metadata != original.metadata || document.body() != original.body)
    {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "reopen completed work before changing its accepted content",
        ));
    }
    crate::organization::validate_issue_change(
        snapshot,
        config,
        Some(&original.metadata),
        &metadata,
        !archive_only && !reopen,
    )?;
    if let IssueMutation::Complete {
        manual: Some(manual),
    } = mutation
    {
        crate::organization::validate_actor(snapshot, &config.repository, &manual.actor)?;
    }
    if new_category == WorkflowCategory::Completed && !archive_only {
        let contract = |issue: &IssueMetadata| {
            issue
                .acceptance
                .iter()
                .map(|criterion| (criterion.id.clone(), criterion.description.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        if contract(&metadata) != contract(&original.metadata)
            || metadata.parent != original.metadata.parent
            || metadata.prerequisites != original.metadata.prerequisites
            || metadata.features != original.metadata.features
            || metadata.gates != original.metadata.gates
        {
            return Err(PmError::new(
                ErrorCode::PolicyBlocked,
                "acceptance requirements cannot be changed in the operation that completes the issue",
            ));
        }
        let prospective = record_from_document(metadata.clone(), &document, original.path.clone());
        let report = completion_admitted(root, snapshot, config, &prospective, admission)?;
        if !report.allowed {
            return Err(
                PmError::new(ErrorCode::PolicyBlocked, report.reasons.join("; "))
                    .hint("Run issue done --dry-run for the current acceptance requirements."),
            );
        }
    }
    let manual = match mutation {
        IssueMutation::Complete { manual } => manual.as_ref(),
        _ => None,
    };
    if old_category == WorkflowCategory::Completed && manual.is_some() {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "reopen work before replacing its manual acceptance",
        ));
    }
    if metadata == original.metadata
        && document.body() == original.body
        && manual.is_none()
        && (!matches!(mutation, IssueMutation::EditDocument { .. })
            || document.render().as_bytes() == bytes)
    {
        return Ok(PreparedOperation {
            changes: Vec::new(),
            result: to_json(&original)?,
        });
    }
    metadata.revision = metadata.revision.next()?;
    metadata.updated_at = Utc::now().max(original.metadata.updated_at);
    if let Some(superseded) = metadata
        .imported_completion
        .as_ref()
        .and_then(|imported| imported.superseded_at)
    {
        metadata.updated_at = metadata.updated_at.max(superseded);
    }
    if new_category == WorkflowCategory::Completed && old_category != WorkflowCategory::Completed {
        metadata.completed_at = Some(metadata.updated_at);
        metadata.canceled_at = None;
        if let Some(manual) = manual {
            metadata.manual_acceptance = Some(ManualAcceptance {
                actor: manual.actor.clone(),
                reason: manual.reason.clone(),
                accepted_at: metadata.updated_at,
            });
        }
    } else if new_category == WorkflowCategory::Canceled
        && old_category != WorkflowCategory::Canceled
    {
        metadata.canceled_at = Some(metadata.updated_at);
        metadata.completed_at = None;
        metadata.manual_acceptance = None;
    }
    metadata.validate(config)?;
    crate::retirement::validate_associations(root, snapshot, config, &metadata)?;
    crate::planning::hierarchy::validate_issue_change(
        root,
        snapshot,
        config,
        Some(&original.metadata),
        &metadata,
    )?;
    crate::graph::validate_issue_change(
        root,
        snapshot,
        config,
        Some(&original.metadata),
        &metadata,
    )?;
    crate::features::validate_issue_associations(
        root,
        snapshot,
        config,
        Some(&original.metadata),
        &metadata,
    )?;
    crate::gates::validate_issue_associations(
        root,
        snapshot,
        config,
        Some(&original.metadata),
        &metadata,
    )?;
    let mut graph_changes =
        crate::graph::invalidate_waivers(snapshot, &original.metadata, &metadata)?;
    replace_metadata(&mut document, &metadata)?;
    let record = record_from_document(metadata, &document, original.path.clone());
    graph_changes.push(FileChange {
        path: original.path,
        expected: Some(original.source.content),
        content: Some(document.render().into_bytes()),
    });
    Ok(PreparedOperation {
        changes: graph_changes,
        result: to_json(&record)?,
    })
}
