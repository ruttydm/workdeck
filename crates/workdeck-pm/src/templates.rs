//! Versioned issue templates. Loading and applying a template does not execute
//! repository commands, initialize directories, or establish acceptance evidence.

use crate::{
    Config, ContentHash, CreateIssue, ErrorCode, IssueMetadata, PmError, Repository, Result,
    SchemaVersion, SourceLink, WorkflowCategory, documents::MarkdownDocument, identity::valid_slug,
    repository::config_from_snapshot, transactions::Snapshot,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const ISSUE_TEMPLATES: &str = "templates/issues";

#[derive(schemars::JsonSchema, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TemplateMetadata {
    schema: SchemaVersion,
    id: String,
    name: String,
    #[serde(default)]
    defaults: BTreeMap<String, Value>,
}

/// A validated source snapshot for display or preparing explicit user input.
/// Creation from a template ID must reload it inside the creation transaction;
/// a cached copy must not bypass replay or source-content preconditions.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueTemplate {
    pub schema: SchemaVersion,
    pub id: String,
    pub name: String,
    pub defaults: BTreeMap<String, Value>,
    pub body: String,
    pub path: PathBuf,
    pub content: ContentHash,
}

impl IssueTemplate {
    /// Caller overrides replace whole top-level fields, matching issue creation.
    /// None inherits the template body; Some("") intentionally clears it. The
    /// resulting request still passes through normal issue-creation validation.
    pub fn apply(
        &self,
        title: String,
        body: Option<String>,
        overrides: BTreeMap<String, Value>,
    ) -> CreateIssue {
        let mut input = CreateIssue::new(title, body.unwrap_or_else(|| self.body.clone()));
        input.fields = self.defaults.clone();
        input.fields.extend(overrides);
        input
    }
}

impl Repository {
    pub fn list_issue_templates(&self) -> Result<Vec<IssueTemplate>> {
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            let mut templates = Vec::new();
            for path in snapshot.list(Path::new(ISSUE_TEMPLATES))? {
                if path.extension().is_none_or(|extension| extension != "md") {
                    continue;
                }
                if path.components().count() != 3 {
                    return Err(PmError::new(
                        ErrorCode::InvalidSchema,
                        "issue templates belong directly in templates/issues/<slug>.md",
                    )
                    .at(self.root().join(path)));
                }
                let id = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| {
                        PmError::new(
                            ErrorCode::InvalidSchema,
                            "issue template filename must be a UTF-8 slug",
                        )
                        .at(self.root().join(&path))
                    })?;
                templates.push(load_template(self.root(), snapshot, &config, id)?);
            }
            templates.sort_by(|left, right| left.id.cmp(&right.id));
            Ok(templates)
        })
    }

    pub fn issue_template(&self, id: &str) -> Result<IssueTemplate> {
        // Validate before opening a source or resolving a caller-supplied path.
        template_path(id)?;
        self.store()?.with_snapshot(|snapshot| {
            let config = config_from_snapshot(self.root(), snapshot)?;
            load_template(self.root(), snapshot, &config, id)
        })
    }
}

/// Resolve through the caller's snapshot. The transaction engine records this
/// read so an editor change invalidates the request before publication.
pub(crate) fn load_template(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &str,
) -> Result<IssueTemplate> {
    let template = load_template_structural(root, snapshot, config, id)?;
    let candidate = validate_defaults(config, &template.defaults)?;
    crate::organization::validate_template(snapshot, config, &candidate)?;
    Ok(template)
}

pub(crate) fn load_template_structural(
    root: &Path,
    snapshot: &Snapshot<'_>,
    config: &Config,
    id: &str,
) -> Result<IssueTemplate> {
    let path = template_path(id)?;
    let absolute = root.join(&path);
    let bytes = snapshot.read(&path)?.ok_or_else(|| {
        PmError::new(
            ErrorCode::NotFound,
            format!("issue template {id:?} was not found"),
        )
        .at(&absolute)
    })?;
    let source = std::str::from_utf8(&bytes).map_err(|_| {
        PmError::new(ErrorCode::InvalidSchema, "issue templates must be UTF-8").at(&absolute)
    })?;
    let document = MarkdownDocument::parse(&absolute, source)?;
    if let Some(version) = document
        .metadata()
        .get("schema")
        .and_then(serde_yaml_ng::Value::as_u64)
    {
        SchemaVersion::try_from(version).map_err(|error| error.at(&absolute))?;
    }
    let metadata: TemplateMetadata = document.deserialize()?;
    if metadata.id != id || !valid_slug(&metadata.id) {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "issue template identity must match its filename slug",
        )
        .at(&absolute));
    }
    if metadata.name.trim().is_empty() || metadata.name.chars().any(char::is_control) {
        return Err(PmError::new(
            ErrorCode::InvalidSchema,
            "issue template name must be nonempty text without control characters",
        )
        .at(&absolute));
    }
    validate_defaults(config, &metadata.defaults).map_err(|error| error.at(&absolute))?;
    Ok(IssueTemplate {
        schema: metadata.schema,
        id: metadata.id,
        name: metadata.name,
        defaults: metadata.defaults,
        body: document.body().into(),
        path,
        content: ContentHash::of(&bytes),
    })
}

fn template_path(id: &str) -> Result<PathBuf> {
    if !valid_slug(id) {
        return Err(PmError::new(
            ErrorCode::InvalidInput,
            "issue template IDs must be stable lowercase slugs without paths or extensions",
        ));
    }
    let relative = format!("{ISSUE_TEMPLATES}/{id}.md");
    SourceLink {
        path: relative.clone(),
        line: None,
        end_line: None,
    }
    .validate()?;
    Ok(relative.into())
}

pub(crate) fn validate_defaults(
    config: &Config,
    defaults: &BTreeMap<String, Value>,
) -> Result<IssueMetadata> {
    const CREATION_DEFAULTS: &[&str] = &[
        "status",
        "priority",
        "assignee",
        "reporter",
        "reviewer",
        "due_at",
        "project",
        "cycle",
        "milestone",
        "parent",
        "prerequisites",
        "features",
        "gates",
        "targets",
        "labels",
        "files",
        "commits",
        "documents",
        "acceptance",
        "custom",
        "estimate",
    ];
    for key in defaults.keys() {
        if !CREATION_DEFAULTS.contains(&key.as_str())
            && !key.strip_prefix("x-").is_some_and(valid_slug)
        {
            return Err(PmError::new(
                ErrorCode::InvalidSchema,
                format!("field {key:?} is not permitted in issue template defaults"),
            ).hint("Templates may supply creation defaults, not title, identity, internal timestamps, archival, or completion authority."));
        }
    }
    let candidate = IssueMetadata::new(config, "Template validation", Utc::now())?;
    let mut value = serde_json::to_value(candidate)
        .map_err(|error| PmError::new(ErrorCode::InvalidSchema, error.to_string()))?;
    let object = value
        .as_object_mut()
        .expect("IssueMetadata serializes as an object");
    for (key, value) in defaults {
        // Null matches the document patch API's explicit field-removal semantics.
        if value.is_null() {
            object.remove(key);
        } else {
            object.insert(key.clone(), value.clone());
        }
    }
    let mut candidate: IssueMetadata = serde_json::from_value(value).map_err(|error| {
        PmError::new(
            ErrorCode::InvalidSchema,
            format!("invalid issue template defaults: {error}"),
        )
    })?;
    candidate.status = config.workflow.canonical_status(&candidate.status)?.into();
    if matches!(
        config.workflow.state(&candidate.status)?.category,
        WorkflowCategory::Completed | WorkflowCategory::Canceled
    ) {
        return Err(PmError::new(
            ErrorCode::PolicyBlocked,
            "issue templates cannot create completed or canceled issues",
        ));
    }
    candidate.validate(config)?;
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::load_template;
    use crate::{ErrorCode, Repository, repository::config_from_snapshot};
    use std::fs;

    #[test]
    fn template_loading_participates_in_the_transaction_snapshot_read_set() {
        let temp = tempfile::TempDir::new().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let path = repository.root().join("templates/issues/bug.md");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let source = "---\nschema: 1\nid: bug\nname: Bug\ndefaults: {}\n---\nOriginal body\n";
        fs::write(&path, source).unwrap();
        let result = repository.store().unwrap().with_snapshot(|snapshot| {
            let config = config_from_snapshot(repository.root(), snapshot)?;
            let template = load_template(repository.root(), snapshot, &config, "bug")?;
            fs::write(&path, source.replace("Original body", "Edited body")).unwrap();
            Ok(template)
        });
        assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
    }
}
