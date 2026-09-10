//! Atomic replacement of an existing binary with a retained original backup.
use anyhow::{Context, Result, ensure};
use std::fs;
use std::io::Write;
use std::path::Path;

/// Replace an existing Workdeck binary using already authenticated bytes.
///
/// This does not authenticate the payload or install accompanying assets. The
/// destination must be quiescent: byte revalidation detects observed changes,
/// but cannot exclude a concurrent writer racing the final rename. A backup is
/// retained even when replacement fails after backup creation.
pub fn replace_binary_with_backup(target: &Path, payload: &[u8], backup: &Path) -> Result<()> {
    replace(target, payload, backup, || Ok(()))
}

fn regular(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "binary target must be a regular non-symlink file"
    );
    ensure!(
        metadata.len() <= 2 * 1024 * 1024 * 1024,
        "binary exceeds installation limit"
    );
    Ok(metadata)
}

fn replace(
    target: &Path,
    payload: &[u8],
    backup: &Path,
    before_commit: impl FnOnce() -> Result<()>,
) -> Result<()> {
    ensure!(
        !payload.is_empty() && payload.len() <= 2 * 1024 * 1024 * 1024,
        "invalid replacement binary size"
    );
    let name = target
        .file_name()
        .context("binary target requires a filename")?;
    ensure!(
        name == "workdeck" || name == "workdeck.exe",
        "unexpected binary target name"
    );
    // Resolve parents once, keeping all temporary writes on the destination
    // filesystem. Final path components may not be symlinks.
    let parent = target
        .parent()
        .context("binary target requires a parent")?
        .canonicalize()?;
    let target = parent.join(name);
    let backup_parent = backup
        .parent()
        .context("backup requires a parent")?
        .canonicalize()?;
    let backup = backup_parent.join(backup.file_name().context("backup requires a filename")?);
    ensure!(target != backup, "backup must differ from binary target");
    let metadata = regular(&target)?;
    let original = fs::read(&target)?;
    let mut replacement = tempfile::NamedTempFile::new_in(&parent)?;
    replacement.write_all(payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        replacement
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    replacement.as_file().sync_all()?;
    let mut saved = tempfile::NamedTempFile::new_in(&backup_parent)?;
    saved.write_all(&original)?;
    saved.as_file().set_permissions(metadata.permissions())?;
    saved.as_file().sync_all()?;
    saved
        .persist_noclobber(&backup)
        .context("backup already exists or cannot be created")?;
    before_commit()?;
    let current = regular(&target)?;
    ensure!(
        current.permissions() == metadata.permissions() && fs::read(&target)? == original,
        "binary changed before replacement; original backup retained"
    );
    replacement
        .persist(&target)
        .context("atomic binary replacement failed; original backup retained")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_binary_and_keeps_exact_backup_without_overwriting_existing_backup() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("workdeck");
        let backup = dir.path().join("workdeck.previous");
        fs::write(&target, b"old\0binary").unwrap();
        replace_binary_with_backup(&target, b"new\0binary", &backup).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"new\0binary");
        assert_eq!(fs::read(&backup).unwrap(), b"old\0binary");
        assert!(replace_binary_with_backup(&target, b"third", &backup).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"new\0binary");
        assert_eq!(fs::read(&backup).unwrap(), b"old\0binary");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn preserves_concurrent_edits_and_backup_on_precommit_failure() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("workdeck");
        let backup = dir.path().join("backup");
        fs::write(&target, b"original").unwrap();
        let result = replace(&target, b"replacement", &backup, || {
            fs::write(&target, b"external edit")?;
            Ok(())
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("changed before replacement")
        );
        assert_eq!(fs::read(&target).unwrap(), b"external edit");
        assert_eq!(fs::read(&backup).unwrap(), b"original");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_target_without_writing_backup() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let target = dir.path().join("workdeck");
        let backup = dir.path().join("backup");
        fs::write(&real, b"original").unwrap();
        std::os::unix::fs::symlink(&real, &target).unwrap();
        assert!(replace_binary_with_backup(&target, b"replacement", &backup).is_err());
        assert!(!backup.exists());
        assert_eq!(fs::read(&real).unwrap(), b"original");
    }
}
