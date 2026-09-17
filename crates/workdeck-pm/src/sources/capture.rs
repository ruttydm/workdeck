use super::*;
use crate::{Config, IssueQuery, IssueRecord, Result, transactions::Snapshot};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct SourceSnapshot {
    pub(super) identity: PlanningSourceIdentity,
    pub(super) files: BTreeMap<PathBuf, Vec<u8>>,
    pub(super) entries: Vec<SourceEntry>,
}
impl SourceSnapshot {
    pub fn identity(&self) -> &PlanningSourceIdentity {
        &self.identity
    }
    pub fn files(&self) -> &BTreeMap<PathBuf, Vec<u8>> {
        &self.files
    }
    pub fn entries(&self) -> &[SourceEntry] {
        &self.entries
    }
    pub(crate) fn with_snapshot<T>(
        &self,
        read: impl FnOnce(&Snapshot<'_>) -> Result<T>,
    ) -> Result<T> {
        read(&Snapshot::from_memory(
            Path::new("source-snapshot"),
            &self.files,
        ))
    }
    pub fn config(&self) -> Result<Config> {
        self.with_snapshot(|snapshot| {
            crate::repository::config_from_snapshot(Path::new("source-snapshot"), snapshot)
        })
    }
    pub fn query_issues(&self, query: &IssueQuery) -> Result<Vec<IssueRecord>> {
        self.with_snapshot(|snapshot| {
            let captured = crate::queries::capture(Path::new("source-snapshot"), snapshot)?;
            let indices = captured.select_indices(query)?;
            Ok(indices
                .into_iter()
                .map(|i| captured.issues()[i].clone())
                .collect())
        })
    }
    pub fn show_issue(&self, reference: &str) -> Result<IssueRecord> {
        self.with_snapshot(|snapshot| {
            let root = Path::new("source-snapshot");
            let config = crate::repository::config_from_snapshot(root, snapshot)?;
            let mut issue = crate::issues::resolve_issue(root, snapshot, &config, reference)?;
            issue.retirement = crate::retirement::read_tombstone(
                root,
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
}
#[derive(Debug, Clone)]
pub struct PlanningSourceView {
    pub observation: SourceObservation,
    pub snapshot: SourceSnapshot,
    pub(super) guard: super::capture_core::CaptureGuard,
}
impl PlanningSourceView {
    /// The captured publication destination, without credentials or live recapture.
    /// Local and staged views do not authorize a shared publication destination.
    pub fn publication_binding(&self) -> Option<&crate::ContentHash> {
        self.guard.publication_binding()
    }
    pub fn revalidate(&self) -> Result<()> {
        self.guard.verify()
    }
    pub(crate) fn revalidate_before(&self, deadline: std::time::Instant) -> Result<()> {
        self.guard.verify_before(deadline)
    }
}
pub fn capture(
    worktree: &Path,
    selector: &SourceSelector,
    limits: &SourceCaptureLimits,
) -> Result<PlanningSourceView> {
    capture_with_deadline(
        worktree,
        selector,
        limits,
        std::time::Instant::now() + std::time::Duration::from_secs(limits.timeout_seconds),
    )
}
pub(crate) fn capture_with_deadline(
    worktree: &Path,
    selector: &SourceSelector,
    limits: &SourceCaptureLimits,
    deadline: std::time::Instant,
) -> Result<PlanningSourceView> {
    let view = super::capture_core::capture(worktree, selector, limits, deadline)?;
    if std::time::Instant::now() >= deadline {
        return Err(crate::PmError::new(
            crate::ErrorCode::Io,
            "source capture exceeded its total timeout",
        ));
    }
    Ok(view)
}
