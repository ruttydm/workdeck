//! Exact native PM snapshots. Export is complete for the supported record kinds;
//! import is deliberately limited to an existing copy of the same authority.
//! Restoring missing operation/cutover authority requires a separate protocol.
mod format;
mod importing;
mod legacy;
mod legacy_import;
mod origins;
pub(crate) mod validation;

pub use format::{NativeSnapshot, SnapshotFile, SnapshotKind, decode_snapshot};
pub use importing::{SnapshotImportMode, SnapshotImportPlan};
pub(crate) use legacy::inspect_export_artifacts;
pub use legacy::{ImportSource, LegacyExport, LegacyExportFormat, decode_transfer};
pub use legacy_import::{LegacyImportContext, LegacyImportPlan};

use crate::{ContentHash, ErrorCode, PmError, Repository, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

/// Serialized JSON input, decoded content, and file counts are independently
/// bounded. The complete prepared journal is checked against the engine limit.
pub const MAX_SNAPSHOT_INPUT_BYTES: usize = 48 * 1024 * 1024;
pub const MAX_SNAPSHOT_CONTENT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_SNAPSHOT_FILES: usize = 4096;
/// Bound traversal separately, including parent directories and non-exported paths.
pub const MAX_SNAPSHOT_ENTRIES: usize = 20_000;

impl Repository {
    /// Capture exact bytes in one locked source snapshot. Attachments are read
    /// and hash-qualified; annotation contents are never executed or promoted.
    pub fn export_snapshot(&self) -> Result<NativeSnapshot> {
        self.store()?.with_snapshot(|snapshot| {
            let files = capture(snapshot)?;
            let exported = NativeSnapshot::build(self.identity().clone(), files)?;
            exported.validate()?;
            Ok(exported)
        })
    }
}

fn capture(snapshot: &crate::transactions::Snapshot<'_>) -> Result<BTreeMap<PathBuf, Vec<u8>>> {
    let mut files = BTreeMap::new();
    let mut total = 0usize;
    for path in snapshot.list_bounded(Path::new(""), MAX_SNAPSHOT_ENTRIES)? {
        let Some(kind) = validation::classify(&path)? else {
            continue;
        };
        let limit = validation::file_limit(kind);
        let bytes = snapshot
            .read_bounded(&path, limit)?
            .ok_or_else(|| invalid("snapshot file disappeared").at(&path))?;
        total = total
            .checked_add(bytes.len())
            .ok_or_else(|| invalid("snapshot size overflow"))?;
        if total > MAX_SNAPSHOT_CONTENT_BYTES || files.len() >= MAX_SNAPSHOT_FILES {
            return Err(unsupported(
                "snapshot exceeds the 32 MiB decoded content or 4096 file limit",
            ));
        }
        files.insert(path, bytes);
    }
    Ok(files)
}

fn hash(value: &impl serde::Serialize) -> Result<ContentHash> {
    crate::transactions::canonical_hash(
        &serde_json::to_value(value).map_err(|error| invalid(error.to_string()))?,
    )
}
fn invalid(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::InvalidSchema, message)
}
fn unsupported(message: impl Into<String>) -> PmError {
    PmError::new(ErrorCode::Unsupported, message)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn projected_validation_never_reads_through_to_host_files() {
        let root = tempfile::TempDir::new().unwrap();
        std::fs::write(root.path().join("host-only.md"), "private host bytes").unwrap();
        let files = BTreeMap::from([(PathBuf::from("virtual.md"), b"projected bytes".to_vec())]);
        let snapshot = crate::transactions::Snapshot::from_memory(root.path(), &files);
        assert_eq!(snapshot.read(Path::new("host-only.md")).unwrap(), None);
        assert_eq!(
            snapshot.read(Path::new("virtual.md")).unwrap().unwrap(),
            b"projected bytes"
        );
        assert_eq!(
            snapshot.list(Path::new("")).unwrap(),
            vec![PathBuf::from("virtual.md")]
        );
        assert!(snapshot.read_bounded(Path::new("virtual.md"), 3).is_err());
        assert!(snapshot.read(Path::new("../escape")).is_err());
    }
}
