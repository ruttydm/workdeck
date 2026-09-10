//! Local release preparation. Backups survive success, rollback, and interruption.
use super::build_plan;
use anyhow::{Context, Result, ensure};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(in crate::changelog) fn apply_plan(
    repo: &Path,
    mut args: impl Iterator<Item = String>,
) -> Result<()> {
    let saved = args.next().context("saved plan path required")?;
    let backup = args.next().context("new backup directory required")?;
    ensure!(
        args.next().is_none(),
        "apply-plan accepts a plan and backup directory"
    );
    let saved = serde_json::from_slice(&fs::read(saved)?)?;
    apply(repo, &saved, Path::new(&backup), |_| Ok(()))?;
    println!("{}", serde_json::json!({"applied":true,"backup":backup}));
    Ok(())
}

fn replace(path: &Path, bytes: &[u8], permissions: Option<fs::Permissions>) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    temporary.write_all(bytes)?;
    if let Some(permissions) = permissions {
        temporary.as_file().set_permissions(permissions)?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}

struct Original {
    bytes: Vec<u8>,
    permissions: fs::Permissions,
}

fn release_lock(path: &Path) -> Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let lock = options.open(path).context("open release lock")?;
    ensure!(
        lock.metadata()?.is_file(),
        "release lock must be a regular file"
    );
    lock.try_lock()
        .context("another release application holds the lock")?;
    Ok(lock)
}

fn require_safe_target(repo: &Path, name: &str) -> Result<()> {
    let relative = Path::new(name);
    ensure!(
        relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "release target must be repository-relative"
    );
    ensure!(!name.is_empty(), "release target cannot be empty");
    let mut current = repo.to_path_buf();
    let components: Vec<_> = relative.components().collect();
    for component in components.iter().take(components.len() - 1) {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current)?;
        ensure!(
            metadata.is_dir() && !metadata.file_type().is_symlink(),
            "release target parent must be a real directory: {}",
            current.display()
        );
    }
    Ok(())
}

fn require_contents(path: &Path, expected: Option<&[u8]>) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && expected.is_none() => Ok(()),
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {
            ensure!(
                expected == Some(fs::read(path)?.as_slice()),
                "release target changed: {}",
                path.display()
            );
            Ok(())
        }
        _ => anyhow::bail!(
            "release target missing, replaced, or inaccessible: {}",
            path.display()
        ),
    }
}

fn apply(
    repo: &Path,
    saved: &serde_json::Value,
    backup: &Path,
    mut after_write: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    let repo = repo.canonicalize()?;
    let lock_path = std::process::Command::new("git")
        .args(["rev-parse", "--git-path", "workdeck-release.lock"])
        .current_dir(&repo)
        .output()?;
    ensure!(
        lock_path.status.success(),
        "release application requires a Git repository"
    );
    let lock_path = PathBuf::from(String::from_utf8(lock_path.stdout)?.trim());
    let lock_path = if lock_path.is_absolute() {
        lock_path
    } else {
        repo.join(lock_path)
    };
    let _lock = release_lock(&lock_path)?;
    ensure!(
        *saved == build_plan(&repo)?,
        "saved release plan is stale or modified"
    );
    let mut targets: BTreeMap<String, Option<String>> =
        serde_json::from_value(saved["edits"].clone())?;
    for fragment in saved["fragments"].as_array().context("missing fragments")? {
        let id = fragment["id"].as_str().context("missing fragment id")?;
        targets.insert(format!("release/fragments/{id}.md"), None);
    }
    ensure!(!targets.is_empty(), "release plan contains no changes");
    let parent = backup
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent.canonicalize()?;
    ensure!(
        !parent.starts_with(&repo),
        "backup directory must be outside the repository"
    );
    let backup = parent.join(
        backup
            .file_name()
            .context("backup directory name missing")?,
    );
    let mut originals = BTreeMap::new();
    for name in targets.keys() {
        require_safe_target(&repo, name)?;
        let path = repo.join(name);
        let original = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_file() && !metadata.file_type().is_symlink(),
                    "release target must be a regular file"
                );
                Some(Original {
                    bytes: fs::read(path)?,
                    permissions: metadata.permissions(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        originals.insert(name.clone(), original);
    }
    fs::create_dir(&backup).context("backup directory must not already exist")?;
    for (name, original) in &originals {
        if let Some(original) = original {
            let path = backup.join("originals").join(name);
            fs::create_dir_all(path.parent().unwrap())?;
            replace(&path, &original.bytes, Some(original.permissions.clone()))?;
        }
    }
    let absent: Vec<_> = originals
        .iter()
        .filter(|(_, value)| value.is_none())
        .map(|(name, _)| name)
        .collect();
    replace(
        &backup.join("recovery.json"),
        &serde_json::to_vec_pretty(
            &serde_json::json!({"repository":repo,"plan":saved,"originally_absent":absent}),
        )?,
        None,
    )?;
    // Backup completion precedes a second authoritative stale-plan check.
    ensure!(
        *saved == build_plan(&repo)?,
        "release inputs changed during backup; no changes applied"
    );
    let mut written = Vec::new();
    let result = (|| -> Result<()> {
        for (name, contents) in &targets {
            require_safe_target(&repo, name)?;
            let path = repo.join(name);
            require_contents(
                &path,
                originals[name]
                    .as_ref()
                    .map(|original| original.bytes.as_slice()),
            )?;
            match contents {
                Some(contents) => replace(
                    &path,
                    contents.as_bytes(),
                    originals[name].as_ref().map(|o| o.permissions.clone()),
                )?,
                None => fs::remove_file(&path)?,
            }
            written.push(name);
            after_write(written.len())?;
        }
        for (name, contents) in &targets {
            require_safe_target(&repo, name)?;
            require_contents(
                &repo.join(name),
                contents.as_ref().map(|contents| contents.as_bytes()),
            )?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for name in written.into_iter().rev() {
            let path = repo.join(name);
            if let Err(error) = require_safe_target(&repo, name) {
                failures.push(format!("{name}: parent conflict preserved: {error}"));
                continue;
            }
            if let Err(error) = require_contents(
                &path,
                targets[name].as_ref().map(|contents| contents.as_bytes()),
            ) {
                failures.push(format!("{name}: conflict preserved: {error}"));
                continue;
            }
            let restored = match &originals[name] {
                Some(original) => {
                    replace(&path, &original.bytes, Some(original.permissions.clone()))
                }
                None => fs::remove_file(path).map_err(anyhow::Error::from),
            };
            if let Err(error) = restored {
                failures.push(format!("{name}: {error}"));
            }
        }
        anyhow::bail!(
            "release apply failed: {error}; rollback errors: {failures:?}; backups: {}",
            backup.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn release_lock_is_private_persistent_and_rejects_symlinks() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let root = tempfile::tempdir().unwrap();
        let target = root.path().join("unrelated");
        fs::write(&target, b"preserve").unwrap();
        let redirected = root.path().join("redirected.lock");
        symlink(&target, &redirected).unwrap();
        assert!(release_lock(&redirected).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"preserve");
        assert!(release_lock(root.path()).is_err());
        let path = root.path().join("release.lock");
        let lock = release_lock(&path).unwrap();
        assert_eq!(lock.metadata().unwrap().permissions().mode() & 0o777, 0o600);
        assert!(release_lock(&path).is_err());
        drop(lock);
        assert!(path.is_file());
        release_lock(&path).unwrap();
    }

    #[test]
    fn release_target_paths_reject_traversal_and_non_directory_parents() {
        let repo = tempfile::tempdir().unwrap();
        fs::create_dir(repo.path().join("release")).unwrap();
        require_safe_target(repo.path(), "Cargo.toml").unwrap();
        require_safe_target(repo.path(), "release/new.md").unwrap();
        for name in [
            "",
            "../outside",
            "/outside",
            "release/../outside",
            "./Cargo.toml",
        ] {
            assert!(require_safe_target(repo.path(), name).is_err(), "{name}");
        }
        fs::write(repo.path().join("file"), "preserve").unwrap();
        assert!(require_safe_target(repo.path(), "file/target").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn release_target_paths_reject_symlinked_parent_directories() {
        let repo = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("target"), "preserve").unwrap();
        std::os::unix::fs::symlink(outside.path(), repo.path().join("release")).unwrap();
        assert!(require_safe_target(repo.path(), "release/target").is_err());
        assert_eq!(
            fs::read_to_string(outside.path().join("target")).unwrap(),
            "preserve"
        );
    }

    #[test]
    fn apply_restores_each_interrupted_step_and_retains_backups() {
        let directory = tempfile::tempdir().unwrap();
        let repo = directory.path().join("repo");
        fs::create_dir(&repo).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        fs::write(repo.join("Cargo.toml"), "[package]\nname = \"workdeck-cli\"\nversion = \"1.2.3\"\nedition = \"2024\"\n[workspace]\n").unwrap();
        fs::create_dir(repo.join("src")).unwrap();
        fs::write(repo.join("src/main.rs"), "fn main() {}\n").unwrap();
        assert!(
            std::process::Command::new("cargo")
                .args(["generate-lockfile", "--offline"])
                .current_dir(&repo)
                .status()
                .unwrap()
                .success()
        );
        fs::create_dir_all(repo.join("release/fragments")).unwrap();
        fs::write(
            repo.join("release/fragments/fix.md"),
            "---\nworkdeck-cli: patch\n---\nFix.\n",
        )
        .unwrap();
        let plan = build_plan(&repo).unwrap();
        for step in 1..=4 {
            let backup = directory.path().join(format!("backup-{step}"));
            let error = apply(&repo, &plan, &backup, |written| {
                ensure!(written != step, "injected failure");
                Ok(())
            })
            .unwrap_err();
            assert!(error.to_string().contains("injected failure"));
            assert!(backup.join("recovery.json").is_file());
            assert!(backup.join("originals/release/fragments/fix.md").is_file());
            assert!(!repo.join("CHANGELOG.md").exists());
            assert_eq!(build_plan(&repo).unwrap(), plan);
        }
        let history = repo.join("CHANGELOG.md");
        let conflict = apply(
            &repo,
            &plan,
            &directory.path().join("rollback-conflict"),
            |step| {
                if step == 2 {
                    fs::write(&history, "Concurrent editor history.\n")?;
                    anyhow::bail!("injected failure after edit");
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(conflict.to_string().contains("conflict preserved"));
        assert_eq!(
            fs::read_to_string(&history).unwrap(),
            "Concurrent editor history.\n"
        );
        fs::remove_file(&history).unwrap();
        assert_eq!(build_plan(&repo).unwrap(), plan);
        let fragment = repo.join("release/fragments/fix.md");
        let original = fs::read(&fragment).unwrap();
        let conflict = apply(
            &repo,
            &plan,
            &directory.path().join("apply-conflict"),
            |step| {
                if step == 1 {
                    fs::write(&fragment, "Concurrent editor fragment.\n")?;
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(conflict.to_string().contains("release target changed"));
        assert_eq!(
            fs::read_to_string(&fragment).unwrap(),
            "Concurrent editor fragment.\n"
        );
        assert!(!history.exists());
        fs::write(&fragment, original).unwrap();
        assert_eq!(build_plan(&repo).unwrap(), plan);
        let conflict = apply(
            &repo,
            &plan,
            &directory.path().join("final-conflict"),
            |step| {
                if step == 4 {
                    fs::write(&history, "Late editor history.\n")?;
                }
                Ok(())
            },
        )
        .unwrap_err();
        assert!(conflict.to_string().contains("conflict preserved"));
        assert_eq!(
            fs::read_to_string(&history).unwrap(),
            "Late editor history.\n"
        );
        fs::remove_file(&history).unwrap();
        assert_eq!(build_plan(&repo).unwrap(), plan);
        let backup = directory.path().join("success");
        apply(&repo, &plan, &backup, |_| Ok(())).unwrap();
        for (name, contents) in plan["edits"].as_object().unwrap() {
            assert_eq!(
                fs::read_to_string(repo.join(name)).unwrap(),
                contents.as_str().unwrap()
            );
        }
        assert!(!repo.join("release/fragments/fix.md").exists());
        assert!(backup.join("originals/release/fragments/fix.md").exists());
        let retry = directory.path().join("retry");
        assert!(apply(&repo, &plan, &retry, |_| Ok(())).is_err());
        assert!(!retry.exists());
    }
}
