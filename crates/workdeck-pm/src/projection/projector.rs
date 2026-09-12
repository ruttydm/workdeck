use super::{
    schema::sql_error,
    storage_types::{ProjectionBuild, ProjectionLimits, ProjectionManifest},
};
use crate::{Result, SnapshotKind, SourceSnapshot};
use rusqlite::params;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[path = "extract.rs"]
pub(super) mod extract;

pub(super) fn project(
    tx: &rusqlite::Transaction<'_>,
    source: &SourceSnapshot,
    previous: Option<&ProjectionManifest>,
    limits: &ProjectionLimits,
) -> Result<ProjectionBuild> {
    limits.validate()?;
    let report = crate::snapshots::validation::inspect_source_files(
        source.files(),
        &source.identity().repository,
        limits.source.max_entries,
        limits.source.max_total_bytes,
    )?;
    let manifest = super::storage::manifest(source)?;
    let mut diagnostics = report.warnings;
    for violation in report.policy_violations {
        diagnostics.push(
            crate::PmError::new(crate::ErrorCode::PolicyBlocked, violation.message)
                .at(violation.path),
        );
    }
    let config = source.config()?;
    let changed = previous
        .map(|previous| changed_paths(previous, &manifest))
        .unwrap_or_else(|| manifest.files.keys().cloned().collect());
    for path in changed
        .iter()
        .filter(|path| !manifest.files.contains_key(*path))
    {
        // A deleted source record cannot retain derived memberships.
        remove(tx, path, false)?;
    }
    let rebuild_effective_targets =
        previous.is_none_or(|_| changed.iter().any(|path| target_membership_path(path)));
    let rebuild_retirements = previous.is_none_or(|previous| {
        let previous_has_tombstones = previous.files.keys().any(|path| tombstone_path(path));
        let current_has_tombstones = manifest.files.keys().any(|path| tombstone_path(path));
        current_has_tombstones != previous_has_tombstones
            || changed.iter().any(|path| tombstone_path(path))
    });
    for path in changed
        .iter()
        .filter(|path| manifest.files.contains_key(*path))
    {
        // A rebuild starts with an empty database. Removing an absent path
        // needlessly scans FTS for every insertion and makes cold work quadratic.
        if previous.is_some_and(|previous| previous.files.contains_key(path)) {
            remove(tx, path, !rebuild_effective_targets)?;
        }
        let bytes = &source.files()[path];
        if let Some(kind) = crate::snapshots::validation::classify(path)? {
            extract::insert(tx, path, kind, bytes)?;
        }
    }
    // Derived memberships and retirements use complete captured authority,
    // including source files outside any selected query/filter. Unrelated
    // feature, label, cycle, and evidence edits retain the existing derived
    // rows instead of rebuilding the entire issue hierarchy.
    if rebuild_effective_targets {
        tx.execute(
            "DELETE FROM projection_values WHERE field='effective_targets'",
            [],
        )
        .map_err(sql_error)?;
        source.with_snapshot(|snapshot| {
            let capture = crate::queries::capture(Path::new("source-snapshot"), snapshot)?;
            for (target, members) in capture.target_memberships() {
                for issue in members {
                    tx.execute(
                        "INSERT INTO projection_values(key,field,value) VALUES(?1,'effective_targets',?2)",
                        params![extract::key("issue", issue.as_str()), target],
                    )
                    .map_err(sql_error)?;
                }
            }
            Ok(())
        })?;
    }
    if rebuild_retirements {
        tx.execute("UPDATE projection_records SET retired=0", [])
            .map_err(sql_error)?;
        for (path, bytes) in source.files() {
            if crate::snapshots::validation::classify(path)? == Some(SnapshotKind::Tombstone) {
                let marker: crate::Tombstone = serde_yaml_ng::from_slice(bytes).map_err(|e| {
                    crate::PmError::new(crate::ErrorCode::InvalidSchema, e.to_string())
                })?;
                let kind = match marker.target.kind {
                    crate::RetirementKind::Issue => SnapshotKind::Issue,
                    crate::RetirementKind::Feature => SnapshotKind::Feature,
                    crate::RetirementKind::Gate => SnapshotKind::Gate,
                    crate::RetirementKind::Initiative => SnapshotKind::Initiative,
                    crate::RetirementKind::Project => SnapshotKind::Project,
                    crate::RetirementKind::Milestone => SnapshotKind::Milestone,
                    crate::RetirementKind::Cycle => SnapshotKind::Cycle,
                    crate::RetirementKind::Target => SnapshotKind::Target,
                    crate::RetirementKind::Label => SnapshotKind::Labels,
                };
                tx.execute(
                    "UPDATE projection_records SET retired=1 WHERE kind=?1 AND id=?2",
                    params![extract::kind_name(kind), marker.target.id],
                )
                .map_err(sql_error)?;
            }
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO projection_configuration(id,document) VALUES(1,?1)",
        [serde_json::to_string(&config)
            .map_err(|e| crate::PmError::new(crate::ErrorCode::InvalidSchema, e.to_string()))?],
    )
    .map_err(sql_error)?;
    let mut counts = BTreeMap::new();
    let mut statement = tx
        .prepare("SELECT kind,COUNT(*) FROM projection_records GROUP BY kind ORDER BY kind")
        .map_err(sql_error)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(sql_error)?;
    for row in rows {
        let (kind, count) = row.map_err(sql_error)?;
        counts.insert(
            kind,
            u64::try_from(count).map_err(|_| {
                crate::PmError::new(
                    crate::ErrorCode::InvalidSchema,
                    "projection count is negative",
                )
            })?,
        );
    }
    Ok(ProjectionBuild {
        manifest,
        counts,
        diagnostics,
    })
}

fn remove(
    tx: &rusqlite::Transaction<'_>,
    path: &Path,
    preserve_effective_targets: bool,
) -> Result<()> {
    let path = path.to_string_lossy();
    let values = if preserve_effective_targets {
        "DELETE FROM projection_values WHERE field <> 'effective_targets' AND key IN(SELECT key FROM projection_records WHERE path=?1)"
    } else {
        "DELETE FROM projection_values WHERE key IN(SELECT key FROM projection_records WHERE path=?1)"
    };
    for sql in [
        values,
        "DELETE FROM projection_search WHERE key IN(SELECT key FROM projection_records WHERE path=?1)",
        "DELETE FROM projection_records WHERE path=?1",
        "DELETE FROM projection_documents WHERE path=?1",
        "DELETE FROM projection_edges WHERE path=?1",
    ] {
        tx.execute(sql, [path.as_ref()]).map_err(sql_error)?;
    }
    Ok(())
}

fn changed_paths(
    previous: &ProjectionManifest,
    current: &ProjectionManifest,
) -> std::collections::BTreeSet<PathBuf> {
    previous
        .files
        .keys()
        .chain(current.files.keys())
        .filter(|path| previous.files.get(*path) != current.files.get(*path))
        .cloned()
        .collect()
}

fn target_membership_path(path: &Path) -> bool {
    matches!(
        path.components()
            .next()
            .and_then(|component| component.as_os_str().to_str()),
        Some("issues" | "projects" | "milestones" | "targets")
    )
}

fn tombstone_path(path: &Path) -> bool {
    path.components()
        .next()
        .and_then(|component| component.as_os_str().to_str())
        == Some("tombstones")
}
