//! Non-overwriting binary creation and replacement with a retained original backup.
use anyhow::{Context, Result, ensure};
use std::fs;
use std::io::{Read, Write};
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

/// Create a binary from already authenticated bytes in an existing directory.
/// Never replaces an existing entry, including dangling symlinks. Accompanying
/// assets, parent creation and parent-directory race coordination belong to the caller.
pub fn create_binary(target: &Path, payload: &[u8]) -> Result<()> {
    create(target, payload, || Ok(()))
}

fn create(target: &Path, payload: &[u8], before_commit: impl FnOnce() -> Result<()>) -> Result<()> {
    ensure!(
        !payload.is_empty() && payload.len() <= 2 * 1024 * 1024 * 1024,
        "invalid installation binary size"
    );
    let name = target
        .file_name()
        .context("binary target requires a filename")?;
    ensure!(
        name == "workdeck" || name == "workdeck.exe",
        "unexpected binary target name"
    );
    let parent = target
        .parent()
        .context("binary target requires a parent")?
        .canonicalize()?;
    let target = parent.join(name);
    match fs::symlink_metadata(&target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(_) => anyhow::bail!("installation target already exists"),
    }
    let _lock = lock_directory(&parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(&parent)?;
    temporary.write_all(payload)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temporary
            .as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    temporary.as_file().sync_all()?;
    before_commit()?;
    temporary
        .persist_noclobber(&target)
        .context("binary creation failed; target was not overwritten")?;
    Ok(())
}

fn regular(path: &Path) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    reject_reparse_point(&metadata)?;
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

pub(super) fn reject_reparse_point(metadata: &fs::Metadata) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        ensure!(
            metadata.file_attributes()
                & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT
                == 0,
            "installation path must not be a reparse point"
        );
    }
    #[cfg(not(windows))]
    let _ = metadata;
    Ok(())
}

fn open_reparse_point_itself(options: &mut fs::OpenOptions) {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    #[cfg(not(windows))]
    let _ = options;
}

fn bounded_bytes(reader: impl Read, expected: u64) -> Result<Vec<u8>> {
    ensure!(
        expected <= 2 * 1024 * 1024 * 1024,
        "binary exceeds installation limit"
    );
    let mut bytes = Vec::new();
    reader.take(expected + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 == expected,
        "binary size changed during read"
    );
    Ok(bytes)
}

pub(super) fn read_binary(path: &Path) -> Result<(fs::Metadata, Vec<u8>)> {
    regular(path)?;
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    open_reparse_point_itself(&mut options);
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    reject_reparse_point(&metadata)?;
    ensure!(
        metadata.is_file(),
        "opened binary target must be a regular file"
    );
    let bytes = bounded_bytes(file, metadata.len())?;
    Ok((metadata, bytes))
}

pub(super) fn lock_directory(parent: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(
            (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
        );
    }
    open_reparse_point_itself(&mut options);
    let file = options.open(parent.join(".workdeck-install.lock"))?;
    reject_reparse_point(&file.metadata()?)?;
    ensure!(
        file.metadata()?.is_file(),
        "installation lock must be a regular file"
    );
    file.try_lock()
        .context("another native installation holds the destination lock")?;
    // Keep the lock file after releasing its handle: unlinking it could let
    // another writer lock a different inode while an existing waiter owns this one.
    Ok(file)
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
    let _lock = lock_directory(&parent)?;
    let backup_parent = backup
        .parent()
        .context("backup requires a parent")?
        .canonicalize()?;
    let backup = backup_parent.join(backup.file_name().context("backup requires a filename")?);
    ensure!(target != backup, "backup must differ from binary target");
    let (metadata, original) = read_binary(&target)?;
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
    let (current, current_bytes) = read_binary(&target)?;
    ensure!(
        current.permissions() == metadata.permissions() && current_bytes == original,
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
    fn first_install_never_overwrites_an_existing_or_racing_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("workdeck");
        create_binary(&target, b"first binary").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"first binary");
        assert!(create_binary(&target, b"second binary").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"first binary");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&target).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        let other = tempfile::tempdir().unwrap();
        let target = other.path().join("workdeck");
        assert!(
            create(&target, b"ours", || {
                fs::write(&target, b"concurrent installation")?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read(&target).unwrap(), b"concurrent installation");
        assert_eq!(fs::read_dir(other.path()).unwrap().count(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn first_install_rejects_dangling_symlinks_without_creating_their_targets() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("workdeck");
        let referent = dir.path().join("missing");
        std::os::unix::fs::symlink(&referent, &target).unwrap();
        assert!(create_binary(&target, b"binary").is_err());
        assert!(!referent.exists());
        assert!(
            fs::symlink_metadata(&target)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn binary_reads_reject_growth_truncation_and_oversized_declarations() {
        assert_eq!(bounded_bytes(b"binary".as_slice(), 6).unwrap(), b"binary");
        assert!(bounded_bytes(b"binary".as_slice(), 5).is_err());
        assert!(bounded_bytes(b"binary".as_slice(), 7).is_err());
        assert!(bounded_bytes(std::io::empty(), 2 * 1024 * 1024 * 1024 + 1).is_err());
        let mut endless = std::io::repeat(0);
        assert!(bounded_bytes(&mut endless, 8).is_err());
    }

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
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[test]
    fn cooperating_replacements_are_serialized_without_removing_lock_file() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("workdeck");
        let backup = dir.path().join("backup");
        let competing_backup = dir.path().join("competing-backup");
        fs::write(&target, b"original").unwrap();
        replace(&target, b"replacement", &backup, || {
            let failure =
                replace_binary_with_backup(&target, b"competing", &competing_backup).unwrap_err();
            assert!(failure.to_string().contains("destination lock"));
            assert!(!competing_backup.exists());
            assert_eq!(fs::read(&target)?, b"original");
            Ok(())
        })
        .unwrap();
        assert!(dir.path().join(".workdeck-install.lock").is_file());
        replace_binary_with_backup(&target, b"subsequent", &competing_backup).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"subsequent");
        assert_eq!(fs::read(&competing_backup).unwrap(), b"replacement");
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
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_lock_without_touching_binary_or_link_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("workdeck");
        let outside = dir.path().join("other-file");
        let backup = dir.path().join("backup");
        fs::write(&target, b"original").unwrap();
        fs::write(&outside, b"unrelated").unwrap();
        std::os::unix::fs::symlink(&outside, dir.path().join(".workdeck-install.lock")).unwrap();
        assert!(replace_binary_with_backup(&target, b"replacement", &backup).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"original");
        assert_eq!(fs::read(&outside).unwrap(), b"unrelated");
        assert!(!backup.exists());
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
