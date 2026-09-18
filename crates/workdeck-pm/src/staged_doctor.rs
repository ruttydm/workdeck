//! Validation of Git's actual candidate planning snapshot. No working-tree
//! configuration is admitted as a substitute for the captured index contents.
use crate::{
    ContentHash, DoctorReport, ErrorCode, IndexSelection, PmError, Result, SourceCaptureLimits,
    SourceObservation, SourceSelector,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(schemars::JsonSchema, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StagedDoctorRequest {
    pub index: IndexSelection,
    pub expected_index: Option<ContentHash>,
}
impl Default for StagedDoctorRequest {
    fn default() -> Self {
        Self {
            index: IndexSelection::EffectiveHook,
            expected_index: None,
        }
    }
}

#[derive(schemars::JsonSchema, Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedDoctorReport {
    pub source: SourceObservation,
    pub report: DoctorReport,
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StagedDoctorFaultPoint {
    BeforeRevalidation,
}

pub fn doctor_staged(worktree: &Path, request: &StagedDoctorRequest) -> Result<StagedDoctorReport> {
    doctor_staged_with_faults(worktree, request, |_| Ok(()))
}

#[doc(hidden)]
pub fn doctor_staged_with_faults(
    worktree: &Path,
    request: &StagedDoctorRequest,
    mut fault: impl FnMut(StagedDoctorFaultPoint) -> Result<()>,
) -> Result<StagedDoctorReport> {
    let limits = SourceCaptureLimits::default();
    let view = crate::sources::capture(
        worktree,
        &SourceSelector::Staged {
            index: request.index.clone(),
        },
        &limits,
    )?;
    if request
        .expected_index
        .as_ref()
        .is_some_and(|expected| view.observation.identity.index_content.as_ref() != Some(expected))
    {
        return Err(PmError::new(
            ErrorCode::StaleSource,
            "the selected Git index differs from the reviewed index",
        )
        .hint("Run doctor --staged again and inspect the current index before retrying."));
    }
    view.revalidate()?;
    let report = inspect_source_files(
        view.snapshot.files(),
        &view.observation.identity.repository,
        &limits,
    )?;
    fault(StagedDoctorFaultPoint::BeforeRevalidation)?;
    view.revalidate()?;
    Ok(StagedDoctorReport {
        source: view.observation,
        report,
    })
}

/// Shared by hook/index and immutable commit validation.
pub(crate) fn inspect_source_files(
    files: &std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    repository: &crate::RepositoryId,
    limits: &SourceCaptureLimits,
) -> Result<DoctorReport> {
    let snapshot = crate::transactions::Snapshot::from_memory(Path::new("source-snapshot"), files);
    let mut report = crate::repository::inspect_snapshot(Path::new("source-snapshot"), &snapshot)?;
    // The staged tree may also contain app preferences or extension sources.
    // Only canonical planning authority enters portable-record proof validation.
    let mut authoritative = std::collections::BTreeMap::new();
    for (path, bytes) in files {
        match crate::snapshots::validation::classify(path) {
            Ok(Some(kind)) => {
                if bytes.len() > crate::snapshots::validation::file_limit(kind) {
                    report.errors.push(
                        PmError::new(
                            ErrorCode::Unsupported,
                            "planning record exceeds its supported byte limit",
                        )
                        .at(path),
                    );
                } else {
                    authoritative.insert(path.clone(), bytes.clone());
                }
            }
            Ok(None) => {}
            Err(error) => report.errors.push(error),
        }
    }
    if report.errors.is_empty()
        && let Err(error) = crate::snapshots::validation::validate_source_files(
            &authoritative,
            repository,
            limits.max_entries,
            limits.max_total_bytes,
        )
    {
        report.errors.push(error);
    }
    report.valid = report.errors.is_empty();
    Ok(report)
}
