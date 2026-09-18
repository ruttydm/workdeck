//! Validated, serialized edits to canonical repository application preferences.
//! Explicit edits copy a selected legacy layer without modifying its source.
use crate::config::{resolve_repo_config_path, validate_repo_config_candidate};
use anyhow::{Context, Result, bail};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant},
};

const MAX_CONFIG_BYTES: u64 = 2 * 1024 * 1024;

pub fn initialize(repo_root: &Path) -> Result<PathBuf> {
    edit(repo_root, None)
}

pub fn set(repo_root: &Path, key: &str, value: &str) -> Result<PathBuf> {
    edit(repo_root, Some((key, value)))
}

fn edit(repo_root: &Path, setting: Option<(&str, &str)>) -> Result<PathBuf> {
    edit_before_publish(repo_root, setting, || Ok(()))
}

fn edit_before_publish(
    repo_root: &Path,
    setting: Option<(&str, &str)>,
    before_publish: impl FnOnce() -> Result<()>,
) -> Result<PathBuf> {
    let repo_root = repo_root.canonicalize()?;
    let source = resolve_repo_config_path(&repo_root)?;
    let path = repo_root.join(".workdeck/config.toml");
    check_path(&repo_root, &source)?;
    check_path(&repo_root, &path)?;
    // Validate before creating the native directory: its presence also selects
    // canonical extension discovery. Recheck these observations under the lock.
    let original = read_optional(&repo_root, &source)?;
    let destination = read_optional(&repo_root, &path)?;
    let mut document = match &original {
        Some(bytes) => std::str::from_utf8(bytes)?.parse::<toml_edit::DocumentMut>()?,
        None => "# Workdeck repository application preferences.\n".parse()?,
    };
    if let Some((key, text)) = setting {
        let parts = key.split('.').collect::<Vec<_>>();
        if parts.iter().any(|part| part.trim().is_empty()) {
            bail!("config key must contain nonempty dot-separated parts");
        }
        let mut table: &mut dyn toml_edit::TableLike = document.as_table_mut();
        for part in &parts[..parts.len() - 1] {
            if !table.contains_key(part) {
                table.insert(part, toml_edit::Item::Table(toml_edit::Table::new()));
            }
            table = table
                .get_mut(part)
                .and_then(toml_edit::Item::as_table_like_mut)
                .with_context(|| format!("config path {key} is not a table"))?;
        }
        let leaf = parts[parts.len() - 1];
        let mut value = text
            .parse::<toml_edit::Value>()
            .unwrap_or_else(|_| text.into());
        if let Some(old) = table.get(leaf).and_then(toml_edit::Item::as_value) {
            *value.decor_mut() = old.decor().clone();
        }
        if let Some(item) = table.get_mut(leaf) {
            *item = toml_edit::Item::Value(value);
        } else {
            table.insert(leaf, toml_edit::Item::Value(value));
        }
    }
    // An explicit init preserves even newline spelling and a missing final
    // newline. A setting edit changes only the requested TOML value.
    let raw = if setting.is_none() {
        original
            .as_ref()
            .map(|bytes| String::from_utf8(bytes.clone()))
            .transpose()?
            .unwrap_or_else(|| document.to_string())
    } else {
        document.to_string()
    };
    if raw.len() as u64 > MAX_CONFIG_BYTES {
        bail!("application config exceeds 2 MiB");
    }
    let candidate = toml::from_str(&raw)?;
    validate_repo_config_candidate(&repo_root, &candidate)?;
    let parent = path
        .parent()
        .context("application configuration has no parent")?;
    fs::create_dir_all(parent)?;
    let local = parent.join(".local");
    check_path(&repo_root, &local)?;
    fs::create_dir_all(&local)?;
    ensure_local_ignore(&repo_root, &local)?;
    let lock_path = local.join("app-config.lock");
    check_path(&repo_root, &lock_path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    no_follow(&mut options);
    let lock = options.open(&lock_path)?;
    if !lock.metadata()?.is_file() {
        bail!("application config lock must be a regular file");
    }
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(TryLockError::WouldBlock) => bail!(
                "application configuration is being edited; retry when the other writer finishes"
            ),
            Err(TryLockError::Error(error)) => return Err(error.into()),
        }
    }
    if resolve_repo_config_path(&repo_root)? != source
        || read_optional(&repo_root, &source)? != original
        || read_optional(&repo_root, &path)? != destination
    {
        bail!("application configuration source changed; retry the command");
    }
    validate_repo_config_candidate(&repo_root, &candidate)?;
    if destination.as_deref() == Some(raw.as_bytes()) {
        return Ok(path);
    }
    // A crash before publication must leave only ignored machine-local staging.
    let mut temporary = tempfile::NamedTempFile::new_in(&local)?;
    if original.is_some() {
        temporary
            .as_file()
            .set_permissions(fs::metadata(&source)?.permissions())?;
    }
    temporary.write_all(raw.as_bytes())?;
    temporary.as_file().sync_all()?;
    before_publish()?;
    if resolve_repo_config_path(&repo_root)? != source
        || read_optional(&repo_root, &source)? != original
        || read_optional(&repo_root, &path)? != destination
    {
        bail!("application configuration changed during editing; retry against the updated file");
    }
    if destination.is_some() {
        temporary.persist(&path)?;
    } else {
        temporary.persist_noclobber(&path)?;
    }
    #[cfg(unix)]
    File::open(parent)?.sync_all()?;
    Ok(path)
}

fn check_path(root: &Path, path: &Path) -> Result<()> {
    let mut current = root.to_owned();
    for part in path.strip_prefix(root)?.components() {
        let Component::Normal(part) = part else {
            bail!("unsafe application config path");
        };
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => bail!(
                "application config path contains a symlink: {}",
                current.display()
            ),
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn ensure_local_ignore(root: &Path, local: &Path) -> Result<()> {
    let path = local.join(".gitignore");
    check_path(root, &path)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    no_follow(&mut options);
    match options.open(&path) {
        Ok(mut file) => {
            file.write_all(b"*\n")?;
            file.sync_all()?;
            #[cfg(unix)]
            File::open(local)?.sync_all()?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    // Never replace existing rules. Requiring the final effective rule to cover
    // all local files also rejects a later negation that would expose the lock.
    let bytes = read_optional(root, &path)?.context("local ignore policy disappeared")?;
    let text = std::str::from_utf8(&bytes)?;
    let final_rule = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty() && !line.starts_with('#'));
    if final_rule != Some("*") {
        bail!(
            "local configuration state must remain ignored; add `*` as the final rule in {} and retry (existing rules were preserved)",
            path.display()
        );
    }
    Ok(())
}

fn read_optional(root: &Path, path: &Path) -> Result<Option<Vec<u8>>> {
    check_path(root, path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    no_follow(&mut options);
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !file.metadata()?.is_file() {
        bail!("application config must be a regular file");
    }
    let mut bytes = Vec::new();
    file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_CONFIG_BYTES {
        bail!("application config exceeds 2 MiB");
    }
    Ok(Some(bytes))
}

fn no_follow(options: &mut OpenOptions) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(not(unix))]
    let _ = options;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn legacy(root: &Path) -> PathBuf {
        let path = root.join(".agents/workdeck/config.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "# original\nmode = 'split'\n").unwrap();
        path
    }

    #[test]
    fn migration_rechecks_legacy_bytes_before_publishing() {
        let root = tempfile::tempdir().unwrap();
        let source = legacy(root.path());
        let changed = "# concurrent author\nmode = 'auto'\n";
        let error = edit_before_publish(root.path(), Some(("mode", "stack")), || {
            fs::write(&source, changed)?;
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("changed during editing"));
        assert_eq!(fs::read_to_string(source).unwrap(), changed);
        assert!(!root.path().join(".workdeck/config.toml").exists());
    }

    #[test]
    fn migration_does_not_overwrite_a_concurrently_created_canonical_layer() {
        let root = tempfile::tempdir().unwrap();
        let source = legacy(root.path());
        let target = root.path().join(".workdeck/config.toml");
        let original = fs::read(&source).unwrap();
        let changed = "# concurrent canonical choice\nmode = 'auto'\n";
        let error = edit_before_publish(root.path(), None, || {
            fs::write(&target, changed)?;
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("changed during editing"));
        assert_eq!(fs::read_to_string(target).unwrap(), changed);
        assert_eq!(fs::read(source).unwrap(), original);
    }

    #[test]
    fn interrupted_migration_preserves_source_and_leaves_destination_unpublished() {
        let root = tempfile::tempdir().unwrap();
        let source = legacy(root.path());
        let original = fs::read(&source).unwrap();
        let error = edit_before_publish(root.path(), None, || bail!("interrupted before publish"))
            .unwrap_err();
        assert_eq!(error.to_string(), "interrupted before publish");
        assert_eq!(fs::read(source).unwrap(), original);
        assert!(!root.path().join(".workdeck/config.toml").exists());
        let names = fs::read_dir(root.path().join(".workdeck"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>();
        assert_eq!(names, [".local"]);
    }

    #[test]
    fn config_local_ignore_policy_is_preserved_and_never_overridden() {
        let root = tempfile::tempdir().unwrap();
        let local = root.path().join(".workdeck/.local");
        fs::create_dir_all(&local).unwrap();
        let policy = local.join(".gitignore");
        let exposed = "# authored rule\n*\n!app-config.lock\n";
        fs::write(&policy, exposed).unwrap();
        let error = initialize(root.path()).unwrap_err();
        assert!(error.to_string().contains("final rule"));
        assert_eq!(fs::read_to_string(&policy).unwrap(), exposed);
        assert!(!local.join("app-config.lock").exists());
        assert!(!root.path().join(".workdeck/config.toml").exists());
        let ignored = "# authored rule\n*.tmp\n*\n# retain commentary\n";
        fs::write(&policy, ignored).unwrap();
        initialize(root.path()).unwrap();
        assert_eq!(fs::read_to_string(policy).unwrap(), ignored);
    }

    #[cfg(unix)]
    #[test]
    fn migration_preserves_source_permissions_and_uses_only_canonical_lock() {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let source = legacy(root.path());
        fs::set_permissions(&source, fs::Permissions::from_mode(0o640)).unwrap();
        let target = initialize(root.path()).unwrap();
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert!(!source.parent().unwrap().join(".local").exists());
        let lock = OpenOptions::new()
            .write(true)
            .open(root.path().join(".workdeck/.local/app-config.lock"))
            .unwrap();
        lock.lock().unwrap();
        let original = fs::read(&target).unwrap();
        let error = set(root.path(), "mode", "stack").unwrap_err();
        assert!(error.to_string().contains("being edited"));
        assert_eq!(fs::read(target).unwrap(), original);
    }
}
