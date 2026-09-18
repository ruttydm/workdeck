//! Disposable, source-qualified read projections. Native documents remain authority.
mod cache;
mod live;
mod projector;
mod query;
mod schema;
mod storage;
mod storage_types;
#[cfg(test)]
mod tests;
mod types;
pub use storage::{ProjectionReadView, ProjectionRefresh, ProjectionStore};
pub use storage_types::{
    PROJECTION_SCHEMA_VERSION, ProjectionFaultPoint, ProjectionLimits, ProjectionRefreshRequest,
    ProjectionState, ProjectionStatus, ProjectionViewId,
};
pub(crate) use storage_types::{ProjectionFile, ProjectionManifest};
pub use types::{
    IssueGroupBy, ProjectionActivityQuery, ProjectionBoardColumn, ProjectionBoardRequest,
    ProjectionDetail, ProjectionFeatureQuery, ProjectionGroup, ProjectionPage,
    ProjectionPlanningQuery, ProjectionQuery, ProjectionQueryHandle, ProjectionRecordKey,
    ProjectionRelation, ProjectionRow, ProjectionRowToken, ProjectionTreePosition,
};

pub(super) fn sql_error(error: rusqlite::Error) -> crate::PmError {
    crate::PmError::new(
        crate::ErrorCode::InvalidSchema,
        format!("projection database: {error}"),
    )
}
