use crate::{ContentHash, PlanningSourceIdentity, PmError, SourceCaptureLimits, SourceObservation};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// Projection versions are independent of the authoritative document schemas.
// Revalidate typed completion receipts previously accepted as generic operations.
pub const PROJECTION_SCHEMA_VERSION: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectionLimits {
    pub source: SourceCaptureLimits,
    pub max_database_bytes: usize,
    pub max_page_rows: usize,
    pub max_query_handles: usize,
    pub max_query_keys: usize,
    pub max_query_bytes: usize,
    pub max_detail_bytes: usize,
    pub query_timeout_ms: u64,
}
impl Default for ProjectionLimits {
    fn default() -> Self {
        Self {
            source: SourceCaptureLimits::default(),
            max_database_bytes: 256 * 1024 * 1024,
            max_page_rows: 500,
            max_query_handles: 16,
            max_query_keys: 100_000,
            max_query_bytes: 32 * 1024 * 1024,
            max_detail_bytes: 4 * 1024 * 1024,
            query_timeout_ms: 5_000,
        }
    }
}
impl ProjectionLimits {
    pub fn validate(&self) -> crate::Result<()> {
        self.source.validate()?;
        if !(4096..=1024 * 1024 * 1024).contains(&self.max_database_bytes)
            || !(1..=10_000).contains(&self.max_page_rows)
            || !(1..=256).contains(&self.max_query_handles)
            || !(1..=1_000_000).contains(&self.max_query_keys)
            || !(1024..=256 * 1024 * 1024).contains(&self.max_query_bytes)
            || !(1024..=64 * 1024 * 1024).contains(&self.max_detail_bytes)
            || !(1..=60_000).contains(&self.query_timeout_ms)
        {
            return Err(crate::PmError::new(
                crate::ErrorCode::InvalidInput,
                "projection limits exceed supported bounds",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionViewId {
    pub schema: u32,
    /// Local checkout and selected source slot; deliberately not portable.
    pub slot: ContentHash,
    pub source: PlanningSourceIdentity,
    pub generation: ContentHash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ProjectionState {
    Empty,
    Cached,
    Current,
    Stale,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectionStatus {
    pub state: ProjectionState,
    pub view: Option<ProjectionViewId>,
    pub observation: Option<SourceObservation>,
    pub publication_binding: Option<ContentHash>,
    pub diagnostics: Vec<PmError>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectionRefreshRequest {
    /// Rebuild from authoritative records even when a valid checkpoint exists.
    pub rebuild: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionFaultPoint {
    AfterCapture,
    AfterProject,
    BeforePublish,
    AfterCheckpointWritten,
    AfterPublish,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectionFile {
    pub content: ContentHash,
    pub bytes: usize,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectionManifest {
    pub files: BTreeMap<PathBuf, ProjectionFile>,
    pub fingerprint: ContentHash,
}

#[derive(Debug, Clone)]
pub(crate) struct ProjectionBuild {
    pub manifest: ProjectionManifest,
    pub counts: BTreeMap<String, u64>,
    pub diagnostics: Vec<PmError>,
}
