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
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(lock_path)?;
    lock.try_lock()
        .context("another release application holds the lock")?;
    ensure!(
        *saved == build_plan(&repo)?,
        "saved release plan is stale or modified"
    );
    let mut targets: BTreeMap<String, Option<String>> =
        serde_json::from_value(saved["edits"].clone())?;
    for fragment in saved["fragments"].as_array().context("missing fragments")? {
        let id = fragment["id"].as_str().context("missing fragment id")?;
        targets.insert(format!("changes/{id}.md"), None);
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
            let path = repo.join(name);
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
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for name in written.into_iter().rev() {
            let path = repo.join(name);
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
        fs::create_dir(repo.join("changes")).unwrap();
        fs::write(
            repo.join("changes/fix.md"),
            "---\nworkdeck: patch\n---\nFix.\n",
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
            assert!(backup.join("originals/changes/fix.md").is_file());
            assert!(!repo.join("CHANGELOG.md").exists());
            assert_eq!(build_plan(&repo).unwrap(), plan);
        }
        let backup = directory.path().join("success");
        apply(&repo, &plan, &backup, |_| Ok(())).unwrap();
        for (name, contents) in plan["edits"].as_object().unwrap() {
            assert_eq!(
                fs::read_to_string(repo.join(name)).unwrap(),
                contents.as_str().unwrap()
            );
        }
        assert!(!repo.join("changes/fix.md").exists());
        assert!(backup.join("originals/changes/fix.md").exists());
        let retry = directory.path().join("retry");
        assert!(apply(&repo, &plan, &retry, |_| Ok(())).is_err());
        assert!(!retry.exists());
    }
}
