//! Shared issue predicates and deterministic ordering over an immutable source
//! capture. Cached filtering does not read files or refresh mutation tokens.
use crate::{
    Config, ErrorCode, IssueId, IssueRecord, PmError, Priority, Repository, RepositoryId, Result,
};
use serde::{Deserialize, Serialize};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum ArchiveFilter {
    #[default]
    Active,
    All,
    Archived,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum TargetMatch {
    #[default]
    All,
    Any,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum SortDirection {
    #[default]
    Ascending,
    Descending,
}

#[derive(
    schemars::JsonSchema,
    Debug,
    Clone,
    Copy,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum IssueSortField {
    #[default]
    CreatedAt,
    UpdatedAt,
    Priority,
    Title,
    Id,
}

#[derive(
    schemars::JsonSchema, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize,
)]
#[serde(default, deny_unknown_fields)]
pub struct IssueSort {
    pub field: IssueSortField,
    pub direction: SortDirection,
}

/// All populated scalar predicates combine with AND. IDs are exact stored
/// references; unknown filter IDs produce no matches rather than a source read.
#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IssueQuery {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<IssueId>>,
    pub status: Option<String>,
    pub priority: Option<Priority>,
    pub assignee: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub workflow_categories: Vec<crate::WorkflowCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_before: Option<crate::Timestamp>,
    pub label: Option<String>,
    pub project: Option<String>,
    pub cycle: Option<String>,
    pub milestone: Option<String>,
    pub targets: Vec<String>,
    pub target_match: TargetMatch,
    pub due_at: Option<String>,
    pub archive: ArchiveFilter,
    pub sort: Vec<IssueSort>,
}

impl Default for IssueQuery {
    fn default() -> Self {
        Self {
            query: String::new(),
            ids: None,
            status: None,
            priority: None,
            assignee: None,
            reviewer: None,
            workflow_categories: Vec::new(),
            due_before: None,
            label: None,
            project: None,
            cycle: None,
            milestone: None,
            targets: Vec::new(),
            target_match: TargetMatch::All,
            due_at: None,
            archive: ArchiveFilter::Active,
            sort: vec![IssueSort::default()],
        }
    }
}

impl IssueQuery {
    /// Existing native CLI/list APIs include archived and retired records.
    pub fn all() -> Self {
        Self {
            archive: ArchiveFilter::All,
            ..Self::default()
        }
    }

    /// Validate input bounds independently of source-specific query evaluation.
    pub fn validate(&self) -> Result<()> {
        if let Some(ids) = &self.ids
            && (ids.len() > 10_000 || ids.iter().collect::<BTreeSet<_>>().len() != ids.len())
        {
            return Err(invalid("issue ID filters must be unique and at most 10000"));
        }
        if self.query.len() > 4096 || self.query.chars().any(char::is_control) {
            return Err(invalid(
                "issue text query must be at most 4096 bytes without control characters",
            ));
        }
        for (field, value) in [
            ("status", &self.status),
            ("assignee", &self.assignee),
            ("reviewer", &self.reviewer),
            ("label", &self.label),
            ("project", &self.project),
            ("cycle", &self.cycle),
            ("milestone", &self.milestone),
            ("due_at", &self.due_at),
        ] {
            if let Some(value) = value {
                filter_value(field, value)?;
            }
        }
        if self.workflow_categories.len() > 9
            || self
                .workflow_categories
                .iter()
                .enumerate()
                .any(|(index, value)| self.workflow_categories[..index].contains(value))
        {
            return Err(invalid(
                "workflow categories must be unique and at most nine",
            ));
        }
        if self.targets.len() > 256 {
            return Err(invalid(
                "an issue query supports at most 256 target filters",
            ));
        }
        let mut targets = BTreeSet::new();
        for target in &self.targets {
            filter_value("target", target)?;
            if !targets.insert(target) {
                return Err(invalid("target filters must be unique"));
            }
        }
        if self.sort.len() > 5 {
            return Err(invalid("an issue query supports at most five sort fields"));
        }
        let mut fields = BTreeSet::new();
        for sort in &self.sort {
            if !fields.insert(sort.field) {
                return Err(invalid("issue sort fields must be unique"));
            }
        }
        Ok(())
    }
}

/// A validated point-in-time capture. Private construction prevents arbitrary
/// caller records from being presented as repository-qualified query results.
#[derive(Debug, Clone)]
pub struct IssueQuerySnapshot {
    config: Config,
    issues: Vec<IssueRecord>,
    target_memberships: BTreeMap<String, BTreeSet<IssueId>>,
}

impl IssueQuerySnapshot {
    pub fn repository(&self) -> &RepositoryId {
        &self.config.repository
    }
    pub fn issues(&self) -> &[IssueRecord] {
        &self.issues
    }

    pub(crate) fn target_memberships(&self) -> &BTreeMap<String, BTreeSet<IssueId>> {
        &self.target_memberships
    }

    /// Returned indices address `issues()` in query order. No I/O occurs here;
    /// aliases use the workflow captured with these exact issue records.
    pub fn select_indices(&self, query: &IssueQuery) -> Result<Vec<usize>> {
        query.validate()?;
        let status = query
            .status
            .as_deref()
            .map(|status| {
                self.config
                    .workflow
                    .canonical_status(status)
                    .map_err(|mut error| {
                        if error.code == ErrorCode::InvalidSchema {
                            error.code = ErrorCode::InvalidInput;
                        }
                        error
                    })
            })
            .transpose()?;
        let ids = query
            .ids
            .as_ref()
            .map(|ids| ids.iter().collect::<BTreeSet<_>>());
        let text = query.query.trim().to_lowercase();
        let mut indices = self
            .issues
            .iter()
            .enumerate()
            .filter_map(|(index, issue)| {
                let metadata = &issue.metadata;
                let archived = match query.archive {
                    ArchiveFilter::All => true,
                    ArchiveFilter::Active => !metadata.archived,
                    ArchiveFilter::Archived => metadata.archived,
                };
                let equal = |filter: &Option<String>, value: &Option<String>| {
                    filter
                        .as_ref()
                        .is_none_or(|filter| value.as_ref() == Some(filter))
                };
                let contains = |value: &str| value.to_lowercase().contains(&text);
                let in_target = |target: &String| {
                    self.target_memberships
                        .get(target)
                        .is_some_and(|issues| issues.contains(&metadata.id))
                };
                let target_matches = query.targets.is_empty()
                    || match query.target_match {
                        TargetMatch::All => query.targets.iter().all(in_target),
                        TargetMatch::Any => query.targets.iter().any(in_target),
                    };
                (archived
                    && ids.as_ref().is_none_or(|ids| ids.contains(&metadata.id))
                    && target_matches
                    && status.is_none_or(|status| metadata.status == status)
                    && query
                        .priority
                        .is_none_or(|priority| metadata.priority == priority)
                    && equal(&query.assignee, &metadata.assignee)
                    && equal(&query.reviewer, &metadata.reviewer)
                    && (query.workflow_categories.is_empty()
                        || self
                            .config
                            .workflow
                            .state(&metadata.status)
                            .is_ok_and(|state| {
                                query.workflow_categories.contains(&state.category)
                            }))
                    && query.due_before.as_ref().is_none_or(|instant| {
                        metadata
                            .due_at
                            .as_deref()
                            .is_some_and(|due| due_before(due, instant))
                    })
                    && equal(&query.project, &metadata.project)
                    && equal(&query.cycle, &metadata.cycle)
                    && equal(&query.milestone, &metadata.milestone)
                    && equal(&query.due_at, &metadata.due_at)
                    && query
                        .label
                        .as_ref()
                        .is_none_or(|label| metadata.labels.contains(label))
                    && (text.is_empty()
                        || contains(metadata.id.as_str())
                        || contains(&metadata.title)
                        || contains(&issue.body)
                        || metadata.labels.iter().any(|label| contains(label))
                        || metadata.assignee.as_deref().is_some_and(contains)))
                .then_some(index)
            })
            .collect::<Vec<_>>();
        let default_sort = [IssueSort::default()];
        let sorting = if query.sort.is_empty() {
            &default_sort[..]
        } else {
            &query.sort
        };
        indices.sort_by(|&left, &right| {
            let left = &self.issues[left].metadata;
            let right = &self.issues[right].metadata;
            for sort in sorting {
                let order = match sort.field {
                    IssueSortField::CreatedAt => left.created_at.cmp(&right.created_at),
                    IssueSortField::UpdatedAt => left.updated_at.cmp(&right.updated_at),
                    IssueSortField::Priority => {
                        priority_rank(left.priority).cmp(&priority_rank(right.priority))
                    }
                    IssueSortField::Title => {
                        left.title.to_lowercase().cmp(&right.title.to_lowercase())
                    }
                    IssueSortField::Id => left.id.cmp(&right.id),
                };
                let order = if sort.direction == SortDirection::Descending {
                    order.reverse()
                } else {
                    order
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
            left.id.cmp(&right.id)
        });
        Ok(indices)
    }
}

impl Repository {
    pub fn issue_query_snapshot(&self) -> Result<IssueQuerySnapshot> {
        self.store()?
            .with_snapshot(|snapshot| capture(self.root(), snapshot))
    }

    pub fn query_issues(&self, query: &IssueQuery) -> Result<Vec<IssueRecord>> {
        query.validate()?;
        let snapshot = self.issue_query_snapshot()?;
        let indices = snapshot.select_indices(query)?;
        Ok(indices
            .into_iter()
            .map(|index| snapshot.issues[index].clone())
            .collect())
    }
}

/// Reuse the caller's admitted source snapshot for planning membership views.
pub(crate) fn capture(
    root: &Path,
    snapshot: &crate::transactions::Snapshot<'_>,
) -> Result<IssueQuerySnapshot> {
    let config = crate::repository::config_from_snapshot(root, snapshot)?;
    let mut issues = crate::issues::load_issues(root, snapshot, &config)?;
    if !issues.is_empty() {
        let retirements = crate::retirement::RetirementIndex::capture(root, snapshot, &config)?;
        for issue in &mut issues {
            issue.retirement = retirements.get(&crate::RetirementTarget::new(
                crate::RetirementKind::Issue,
                issue.metadata.id.as_str(),
            )?)?;
        }
    }
    let target_memberships =
        crate::planning::hierarchy::target_memberships(root, snapshot, &config, &issues)?;
    Ok(IssueQuerySnapshot {
        config,
        issues,
        target_memberships,
    })
}

fn filter_value(field: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 1000 || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{field} filter must be nonempty text up to 1000 bytes without control characters"
        )));
    }
    Ok(())
}

fn priority_rank(priority: Priority) -> u8 {
    match priority {
        Priority::None => 0,
        Priority::Low => 1,
        Priority::Medium => 2,
        Priority::High => 3,
        Priority::Urgent => 4,
    }
}

fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidInput, message)
}

/// A date-only deadline remains due throughout that UTC calendar day. An instant
/// is overdue strictly after it, preserving offsets and nanosecond precision.
pub(crate) fn due_before(due: &str, instant: &crate::Timestamp) -> bool {
    if let Ok(date) = chrono::NaiveDate::parse_from_str(due, "%Y-%m-%d") {
        date < instant.date_naive()
    } else {
        chrono::DateTime::parse_from_rfc3339(due).is_ok_and(|due| due < *instant)
    }
}
