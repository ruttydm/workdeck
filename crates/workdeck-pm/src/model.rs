//! Schema-1 PM records. Validation is explicit because the source configuration
//! supplies workflow semantics and future policy declarations are not evidence.

use crate::{
    ContentHash, ErrorCode, IssueId, PmError, RepositoryId, Result, Revision, SchemaVersion,
    Timestamp, Workflow, WorkflowCategory,
    identity::{valid_prefix, valid_slug},
};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}

fn nonempty_line(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} must be nonempty text without control characters"
        )));
    }
    Ok(())
}

fn optional_line(value: &Option<String>, field: &str) -> Result<()> {
    value
        .as_deref()
        .map_or(Ok(()), |value| nonempty_line(value, field))
}

fn unique_lines(values: &[String], field: &str) -> Result<()> {
    let mut seen = BTreeSet::new();
    for value in values {
        nonempty_line(value, field)?;
        if !seen.insert(value) {
            return Err(invalid(format!(
                "{field} contains duplicate value {value:?}"
            )));
        }
    }
    Ok(())
}

fn extensions(extra: &BTreeMap<String, Value>, reserved: &[&str]) -> Result<()> {
    for key in extra.keys() {
        if key.strip_prefix("x-").is_some_and(valid_slug) {
            continue;
        }
        if reserved.contains(&key.as_str()) {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                format!("field {key:?} is reserved for a later PM capability"),
            )
            .hint("Remove the unsupported declaration or use an implementation that supports it; extension metadata belongs under custom or x- names."));
        }
        return Err(invalid(format!("unknown field {key:?}"))
            .hint("Check the field spelling. Custom metadata belongs under custom or an x- prefixed name."));
    }
    Ok(())
}

fn custom_fields(custom: &BTreeMap<String, Value>) -> Result<()> {
    for key in custom.keys() {
        nonempty_line(key, "custom field name")?;
    }
    Ok(())
}

/// Versioned PM configuration; app preferences remain a separate TOML document.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub schema: SchemaVersion,
    pub repository: RepositoryId,
    pub prefix: String,
    #[serde(default)]
    pub workflow: Workflow,
    #[serde(default)]
    pub acceptance: AcceptancePolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<crate::sources::SharedSources>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claims: Option<crate::sources::ClaimPolicy>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Config {
    pub fn new(prefix: &str) -> Result<Self> {
        let config = Self {
            schema: SchemaVersion::CURRENT,
            repository: RepositoryId::new(),
            prefix: prefix.into(),
            workflow: Workflow::default(),
            acceptance: AcceptancePolicy::default(),
            sources: None,
            claims: None,
            custom: BTreeMap::new(),
            extra: BTreeMap::new(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if !valid_prefix(&self.prefix) {
            return Err(invalid("prefix must contain 1–12 uppercase ASCII letters"));
        }
        self.workflow.validate()?;
        let initial = self.workflow.state(&self.workflow.initial)?;
        if matches!(
            initial.category,
            WorkflowCategory::Completed | WorkflowCategory::Canceled
        ) {
            return Err(invalid(
                "initial workflow state cannot be completed or canceled",
            ));
        }
        self.acceptance.validate()?;
        if let Some(sources) = &self.sources {
            sources.validate()?;
        }
        if let Some(claims) = &self.claims {
            claims.validate()?;
        }
        custom_fields(&self.custom)?;
        extensions(&self.extra, &["coordination", "custom_fields", "providers"])
    }
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    None,
    Low,
    #[default]
    Medium,
    High,
    Urgent,
}

impl Priority {
    /// Preserve historical CLI aliases without admitting them to stored schema.
    pub fn parse_input(input: &str) -> Result<Self> {
        match crate::workflow::normalize_input(input).as_str() {
            "none" | "no" => Ok(Self::None),
            "low" => Ok(Self::Low),
            "medium" | "med" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            "urgent" | "critical" => Ok(Self::Urgent),
            _ => Err(PmError::new(
                ErrorCode::InvalidInput,
                format!("unknown priority {input:?}"),
            )),
        }
    }
}

impl std::str::FromStr for Priority {
    type Err = PmError;
    fn from_str(input: &str) -> Result<Self> {
        Self::parse_input(input)
    }
}

#[cfg(test)]
mod priority_input_tests {
    use super::*;

    #[test]
    fn legacy_inputs_round_trip_as_canonical_storage_values() {
        for (input, priority, canonical) in [
            ("NO", Priority::None, "none"),
            ("Med", Priority::Medium, "medium"),
            ("CRITICAL", Priority::Urgent, "urgent"),
            ("h-i_g h", Priority::High, "high"),
            ("Low", Priority::Low, "low"),
        ] {
            assert_eq!(Priority::parse_input(input).unwrap(), priority);
            assert_eq!(input.parse::<Priority>().unwrap(), priority);
            assert_eq!(serde_json::to_value(priority).unwrap(), canonical);
        }
        assert!(serde_json::from_value::<Priority>(serde_json::json!("med")).is_err());
        assert!(Priority::parse_input("unrecognized").is_err());
    }
}

/// Declarative requirements. Loading a future check/profile reference does not
/// implement its evaluator or allow it to become a passing completion result.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptancePolicy {
    #[serde(default = "default_true")]
    pub require_all_criteria: bool,
    #[serde(default)]
    pub require_description: bool,
    #[serde(default)]
    pub allow_prerequisite_waivers: bool,
    #[serde(default = "default_true")]
    pub require_completed_children: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_profiles: Vec<String>,
}

const fn default_true() -> bool {
    true
}

impl Default for AcceptancePolicy {
    fn default() -> Self {
        Self {
            require_all_criteria: true,
            require_description: false,
            allow_prerequisite_waivers: false,
            require_completed_children: true,
            required_checks: Vec::new(),
            required_profiles: Vec::new(),
        }
    }
}

impl AcceptancePolicy {
    pub fn validate(&self) -> Result<()> {
        for (name, values) in [
            ("required_checks", &self.required_checks),
            ("required_profiles", &self.required_profiles),
        ] {
            unique_lines(values, name)?;
            if values.iter().any(|value| !valid_slug(value)) {
                return Err(invalid(format!(
                    "{name} requires stable lowercase slug references"
                )));
            }
        }
        Ok(())
    }

    /// Completion callers must invoke this before evaluating baseline criteria.
    /// Declaration-only completion cannot admit required check/profile results.
    pub fn ensure_supported_for_completion(&self) -> Result<()> {
        self.validate()?;
        if !self.required_checks.is_empty() || !self.required_profiles.is_empty() {
            return Err(PmError::new(
                ErrorCode::Unsupported,
                "this completion path has no authenticated check/profile admission",
            )
            .hint("Standalone red/green completion supports issue done --verification-file; ordinary/manual declarations cannot satisfy required checks."));
        }
        Ok(())
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub description: String,
    #[serde(default)]
    pub checked: bool,
}

impl AcceptanceCriterion {
    pub fn validate(&self) -> Result<()> {
        if !valid_slug(&self.id) {
            return Err(invalid(
                "acceptance criterion IDs must be stable lowercase slugs",
            ));
        }
        nonempty_line(&self.description, "acceptance criterion description")
    }
}

/// Explicit human acceptance attribution, never a trusted check or CI result.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManualAcceptance {
    pub actor: String,
    pub reason: String,
    pub accepted_at: Timestamp,
}

impl ManualAcceptance {
    pub fn validate(&self) -> Result<()> {
        nonempty_line(&self.actor, "manual acceptance actor")?;
        nonempty_line(&self.reason, "manual acceptance reason")
    }
}

/// Historical status copied from a specific legacy source. It does not establish
/// a completion time, manual acceptance, or any verification result.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportedCompletion {
    pub source_path: String,
    pub source_content: ContentHash,
    pub imported_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_at: Option<Timestamp>,
}

impl ImportedCompletion {
    pub fn new(source_path: String, source_content: ContentHash, imported_at: Timestamp) -> Self {
        Self {
            source_path,
            source_content,
            imported_at,
            superseded_at: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.superseded_at.is_none()
    }

    pub fn validate(&self) -> Result<()> {
        SourceLink {
            path: self.source_path.clone(),
            line: None,
            end_line: None,
        }
        .validate()?;
        if self
            .superseded_at
            .is_some_and(|time| time < self.imported_at)
        {
            return Err(invalid(
                "imported completion cannot be superseded before it was imported",
            ));
        }
        Ok(())
    }
}

/// A lexical repository-relative link. This method never reads the filesystem.
/// Source adapters separately enforce containment when resolving actual files,
/// including rejecting symlink escapes before reads or writes.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceLink {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
}

impl SourceLink {
    pub fn validate(&self) -> Result<()> {
        let unsafe_path = || {
            PmError::new(
                ErrorCode::UnsafePath,
                format!(
                    "source path {:?} must be portable and repository-relative",
                    self.path
                ),
            )
        };
        if self.path.is_empty()
            || self.path.chars().any(|c| {
                c.is_control() || matches!(c, '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
            })
        {
            return Err(unsafe_path());
        }
        for component in self.path.split('/') {
            if component.is_empty()
                || matches!(component, "." | "..")
                || component.ends_with(['.', ' '])
            {
                return Err(unsafe_path());
            }
            let stem = component
                .split('.')
                .next()
                .unwrap_or(component)
                .to_ascii_uppercase();
            if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ["COM", "LPT"].iter().any(|prefix| {
                    stem.strip_prefix(prefix).is_some_and(|suffix| {
                        matches!(
                            suffix,
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                        )
                    })
                })
            {
                return Err(unsafe_path());
            }
        }
        if self.line == Some(0)
            || self
                .end_line
                .is_some_and(|end| self.line.is_none_or(|start| end < start))
        {
            return Err(invalid(
                "source line ranges are one-based and require end_line >= line",
            ));
        }
        Ok(())
    }
}

/// Structured frontmatter only. The document layer preserves the Markdown body
/// and source formatting; repository operations own revision/content preconditions.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IssueMetadata {
    pub schema: SchemaVersion,
    pub id: IssueId,
    pub revision: Revision,
    pub title: String,
    pub status: String,
    #[serde(default)]
    pub priority: Priority,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignee: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reporter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    /// Date-only YYYY-MM-DD or an RFC 3339 instant with an explicit offset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<Timestamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canceled_at: Option<Timestamp>,
    #[serde(default)]
    pub archived: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<IssueId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prerequisites: Vec<IssueId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub features: Vec<crate::FeatureId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gates: Vec<crate::GateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate: Option<crate::Estimate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<SourceLink>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub commits: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub documents: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acceptance: Vec<AcceptanceCriterion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_acceptance: Option<ManualAcceptance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_completion: Option<ImportedCompletion>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub custom: BTreeMap<String, Value>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl IssueMetadata {
    pub fn new(config: &Config, title: &str, now: Timestamp) -> Result<Self> {
        config.validate()?;
        let issue = Self {
            schema: SchemaVersion::CURRENT,
            id: IssueId::new(&config.prefix)?,
            revision: Revision::INITIAL,
            title: title.trim().into(),
            status: config.workflow.initial.clone(),
            priority: Priority::default(),
            created_at: now,
            updated_at: now,
            assignee: None,
            reporter: None,
            reviewer: None,
            due_at: None,
            completed_at: None,
            canceled_at: None,
            archived: false,
            project: None,
            cycle: None,
            milestone: None,
            parent: None,
            prerequisites: Vec::new(),
            features: Vec::new(),
            gates: Vec::new(),
            estimate: None,
            targets: Vec::new(),
            labels: Vec::new(),
            files: Vec::new(),
            commits: Vec::new(),
            documents: Vec::new(),
            acceptance: Vec::new(),
            manual_acceptance: None,
            imported_completion: None,
            custom: BTreeMap::new(),
            extra: BTreeMap::new(),
        };
        issue.validate(config)?;
        Ok(issue)
    }

    pub fn validate(&self, config: &Config) -> Result<()> {
        config.validate()?;
        nonempty_line(&self.title, "title")?;
        let state = config.workflow.state(&self.status)?;
        if self.updated_at < self.created_at {
            return Err(invalid("updated_at cannot precede created_at"));
        }
        for (field, timestamp) in [
            ("completed_at", self.completed_at),
            ("canceled_at", self.canceled_at),
        ] {
            if timestamp.is_some_and(|value| value < self.created_at || value > self.updated_at) {
                return Err(invalid(format!(
                    "{field} must fall between created_at and updated_at"
                )));
            }
        }
        let completed = state.category == WorkflowCategory::Completed;
        let canceled = state.category == WorkflowCategory::Canceled;
        let imported_completion = self
            .imported_completion
            .as_ref()
            .is_some_and(ImportedCompletion::is_active);
        if let Some(imported) = &self.imported_completion {
            imported.validate()?;
        }
        if imported_completion && (self.completed_at.is_some() || self.manual_acceptance.is_some())
        {
            return Err(invalid(
                "historical imported completion cannot claim a completion timestamp or manual acceptance",
            ));
        }
        if (self.completed_at.is_some() || imported_completion) != completed
            || self.canceled_at.is_some() != canceled
        {
            return Err(invalid(
                "completed_at and canceled_at must match the workflow status category",
            ));
        }
        for (field, value) in [
            ("assignee", &self.assignee),
            ("reporter", &self.reporter),
            ("reviewer", &self.reviewer),
            ("project", &self.project),
            ("cycle", &self.cycle),
            ("milestone", &self.milestone),
        ] {
            optional_line(value, field)?;
        }
        if let Some(due) = &self.due_at {
            let date = NaiveDate::parse_from_str(due, "%Y-%m-%d")
                .is_ok_and(|value| value.format("%Y-%m-%d").to_string() == *due);
            if !date && DateTime::parse_from_rfc3339(due).is_err() {
                return Err(invalid(
                    "due_at must be YYYY-MM-DD or an RFC 3339 instant with an explicit offset",
                ));
            }
        }
        for (field, values) in [
            ("labels", &self.labels),
            ("targets", &self.targets),
            ("commits", &self.commits),
            ("documents", &self.documents),
        ] {
            unique_lines(values, field)?;
        }
        for source in &self.files {
            source.validate()?;
        }
        let mut criteria = BTreeSet::new();
        for criterion in &self.acceptance {
            criterion.validate()?;
            if !criteria.insert(&criterion.id) {
                return Err(invalid(format!(
                    "duplicate acceptance criterion {:?}",
                    criterion.id
                )));
            }
        }
        if let Some(acceptance) = &self.manual_acceptance {
            acceptance.validate()?;
            if acceptance.accepted_at < self.created_at || acceptance.accepted_at > self.updated_at
            {
                return Err(invalid(
                    "manual acceptance accepted_at must fall between created_at and updated_at",
                ));
            }
        }
        if let Some(estimate) = &self.estimate {
            estimate.validate()?;
        }
        crate::graph::validate_metadata(self)?;
        if let Some(field) = self.extra.keys().find(|field| {
            MetadataAuthority::for_issue_field(field) == Some(MetadataAuthority::Derived)
        }) {
            return Err(invalid(format!(
                "derived issue field {field:?} cannot be stored in frontmatter"
            )));
        }
        custom_fields(&self.custom)?;
        extensions(
            &self.extra,
            &[
                "type",
                "related",
                "evidence",
                "target",
                "claim",
                "review_session",
                "last_activity_at",
            ],
        )
    }
}

/// A field's authority is distinct from whether schema 1 allows it in an issue.
/// Claims and review sessions have independent records, not issue frontmatter.
#[derive(schemars::JsonSchema, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataAuthority {
    Authoritative,
    Derived,
    Local,
    Disposable,
    Coordination,
    ReviewReference,
}

impl MetadataAuthority {
    pub fn for_issue_field(field: &str) -> Option<Self> {
        match field {
            "schema"
            | "id"
            | "revision"
            | "title"
            | "status"
            | "priority"
            | "created_at"
            | "updated_at"
            | "assignee"
            | "reporter"
            | "reviewer"
            | "due_at"
            | "completed_at"
            | "canceled_at"
            | "archived"
            | "project"
            | "cycle"
            | "parent"
            | "prerequisites"
            | "features"
            | "gates"
            | "milestone"
            | "targets"
            | "labels"
            | "files"
            | "commits"
            | "documents"
            | "acceptance"
            | "manual_acceptance"
            | "imported_completion"
            | "estimate"
            | "custom" => Some(Self::Authoritative),
            "last_activity_at" | "blocked_by" | "children" | "dependents" | "related" => {
                Some(Self::Derived)
            }
            "selected_issue" | "scroll_position" | "draft" => Some(Self::Local),
            "index_row" | "search_score" => Some(Self::Disposable),
            "claim" => Some(Self::Coordination),
            "review_session" => Some(Self::ReviewReference),
            field if field.strip_prefix("x-").is_some_and(valid_slug) => Some(Self::Authoritative),
            _ => None,
        }
    }
}
