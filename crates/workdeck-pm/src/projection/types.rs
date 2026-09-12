use super::storage_types::ProjectionViewId;
use crate::{
    ArchiveFilter, ContentHash, FeatureAvailability, FeatureDecision, FeatureId, FeatureMaturity,
    IssueQuery, PlanningKind, Priority, RepositoryId, SnapshotKind,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRecordKey {
    pub repository: RepositoryId,
    pub kind: SnapshotKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum IssueGroupBy {
    Status,
    Priority,
    Assignee,
    Project,
    Cycle,
    Milestone,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectionFeatureQuery {
    pub tree: bool,
    pub collapsed: Vec<FeatureId>,
    pub query: String,
    pub archive: ArchiveFilter,
    pub parent: Option<FeatureId>,
    pub roots_only: bool,
    pub project: Option<String>,
    pub milestone: Option<String>,
    pub target: Option<String>,
    pub lead: Option<String>,
    pub decision: Option<FeatureDecision>,
    pub maturity: Option<FeatureMaturity>,
    pub availability: Option<FeatureAvailability>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionPlanningQuery {
    pub kind: PlanningKind,
    #[serde(default)]
    pub query: String,
    #[serde(default)]
    pub archive: ArchiveFilter,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectionActivityQuery {
    pub subject: Option<ProjectionRecordKey>,
    pub kinds: Vec<SnapshotKind>,
    pub after: Option<crate::Timestamp>,
    pub before: Option<crate::Timestamp>,
}

/// Issue text retains the existing literal Unicode substring contract. Records
/// search uses a separate literal-token full-text index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ProjectionQuery {
    Issues {
        query: IssueQuery,
        #[serde(default)]
        group_by: Option<IssueGroupBy>,
    },
    Features {
        query: ProjectionFeatureQuery,
    },
    Planning {
        query: ProjectionPlanningQuery,
    },
    Activity {
        query: ProjectionActivityQuery,
    },
    Records {
        #[serde(default)]
        family: Option<SnapshotKind>,
        #[serde(default)]
        query: String,
    },
}
impl Default for ProjectionQuery {
    fn default() -> Self {
        Self::Issues {
            query: IssueQuery::default(),
            group_by: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionQueryHandle {
    pub view: ProjectionViewId,
    pub query: ContentHash,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRowToken {
    pub view: ProjectionViewId,
    pub key: ProjectionRecordKey,
    pub path: PathBuf,
    pub content: ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRow {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tree: Option<ProjectionTreePosition>,
    pub token: ProjectionRowToken,
    pub title: String,
    pub archived: bool,
    pub retired: bool,
    pub status: Option<String>,
    pub priority: Option<Priority>,
    pub assignee: Option<String>,
    pub project: Option<String>,
    pub cycle: Option<String>,
    pub milestone: Option<String>,
    pub parent: Option<ProjectionRecordKey>,
    pub group: Option<String>,
    pub created_at: Option<String>,
    pub updated_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<FeatureDecision>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maturity: Option<FeatureMaturity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub availability: Option<FeatureAvailability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lead: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionPage {
    pub handle: ProjectionQueryHandle,
    pub offset: usize,
    pub rows: Vec<ProjectionRow>,
    pub next_offset: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionRelation {
    pub relation: String,
    pub from: ProjectionRecordKey,
    pub to: ProjectionRecordKey,
    pub path: PathBuf,
    pub content: ContentHash,
}

/// A bounded inert source excerpt. It is never a new mutation token or an
/// assessment of current claim ownership, remote confirmation or CI evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionDetail {
    pub row: ProjectionRow,
    pub document: Option<String>,
    pub document_bytes: usize,
    pub omitted_document_bytes: usize,
    pub relations: Vec<ProjectionRelation>,
    pub total_relations: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionGroup {
    pub value: Option<String>,
    pub count: usize,
}

/// One bounded horizontal board window, pinned to a query handle by the caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionBoardRequest {
    pub first_group: usize,
    pub columns: usize,
    pub rows: usize,
    pub selected: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionBoardColumn {
    pub group: ProjectionGroup,
    pub page: ProjectionPage,
}

/// Structure within this captured query. A filtered-out parent remains explicit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionTreePosition {
    pub depth: usize,
    pub children: usize,
    pub parent_outside_view: bool,
}
