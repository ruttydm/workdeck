//! Recoverable application of regenerated, validated release artifact plans.
use super::ArtifactPlan;
use anyhow::{Context, Result, ensure};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

fn parents(repo: &Path, relative: &str, create: bool) -> Result<()> {
    let mut current = repo.to_owned();
    for component in Path::new(relative)
        .parent()
        .context("artifact parent missing")?
        .components()
    {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !metadata.file_type().is_symlink(),
                "artifact parent is not a real directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if create {
                    fs::create_dir(&current)?;
                }
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn contents(path: &Path) -> Result<Option<Vec<u8>>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "artifact must be a regular file"
            );
            Ok(Some(fs::read(path)?))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn write(path: &Path, bytes: &[u8], mode: Option<fs::Permissions>, absent: bool) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    temporary.write_all(bytes)?;
    if let Some(mode) = mode {
        temporary.as_file().set_permissions(mode)?;
    }
    temporary.as_file().sync_all()?;
    if absent {
        temporary.persist_noclobber(path)?;
    } else {
        temporary.persist(path)?;
    }
    Ok(())
}

pub(super) fn apply(
    repo: &Path,
    plan: &ArtifactPlan,
    backup: &Path,
    mut after_write: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    plan.validate()?;
    ensure!(!plan.edits.is_empty(), "artifact plan contains no changes");
    let repo = repo.canonicalize()?;
    let parent = backup
        .parent()
        .context("backup parent missing")?
        .canonicalize()?;
    ensure!(
        !parent.starts_with(&repo),
        "artifact backup must be outside the repository"
    );
    let backup = parent.join(backup.file_name().context("backup filename missing")?);
    let mut permissions = BTreeMap::new();
    for name in plan.edits.keys() {
        parents(&repo, name, false)?;
        let path = repo.join(name);
        ensure!(
            contents(&path)? == plan.originals[name],
            "artifact changed since planning: {name}"
        );
        let mode = if plan.originals[name].is_some() {
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("read original artifact permissions: {name}"))?;
            ensure!(
                metadata.is_file() && !metadata.file_type().is_symlink(),
                "artifact changed while reading permissions: {name}"
            );
            Some(metadata.permissions())
        } else {
            None
        };
        permissions.insert(name.clone(), mode);
    }
    fs::create_dir(&backup).context("artifact backup must not already exist")?;
    write(
        &backup.join("recovery.json"),
        &serde_json::to_vec_pretty(plan)?,
        None,
        true,
    )?;
    let modes: BTreeMap<_, _> = permissions
        .iter()
        .map(|(name, mode)| {
            let value = mode.as_ref().map(|mode| {
                #[cfg(unix)]
                let unix_mode = {
                    use std::os::unix::fs::PermissionsExt;
                    Some(mode.mode())
                };
                #[cfg(not(unix))]
                let unix_mode: Option<u32> = None;
                serde_json::json!({"readonly":mode.readonly(), "unixMode":unix_mode})
            });
            (name, value)
        })
        .collect();
    write(
        &backup.join("permissions.json"),
        &serde_json::to_vec_pretty(&modes)?,
        None,
        true,
    )?;
    let mut written: Vec<&String> = Vec::new();
    let result = (|| -> Result<()> {
        for (name, replacement) in &plan.edits {
            parents(&repo, name, true)?;
            let path = repo.join(name);
            ensure!(
                contents(&path)? == plan.originals[name],
                "artifact changed before application: {name}"
            );
            match replacement {
                Some(text) => write(
                    &path,
                    text.as_bytes(),
                    permissions[name].clone(),
                    plan.originals[name].is_none(),
                )?,
                None => fs::remove_file(&path)?,
            }
            written.push(name);
            after_write(written.len())?;
        }
        for (name, replacement) in &plan.edits {
            parents(&repo, name, false)?;
            ensure!(
                contents(&repo.join(name))?.as_deref()
                    == replacement.as_ref().map(|s| s.as_bytes()),
                "artifact changed after application: {name}"
            );
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut conflicts = Vec::new();
        for name in written.into_iter().rev() {
            let restore = (|| -> Result<()> {
                parents(&repo, name, false)?;
                let path = repo.join(name);
                ensure!(
                    contents(&path)?.as_deref() == plan.edits[name].as_ref().map(|s| s.as_bytes()),
                    "concurrent change preserved"
                );
                match &plan.originals[name] {
                    Some(bytes) => write(
                        &path,
                        bytes,
                        permissions[name].clone(),
                        plan.edits[name].is_none(),
                    ),
                    None => Ok(fs::remove_file(path)?),
                }
            })();
            if let Err(error) = restore {
                conflicts.push(format!("{name}: {error}"));
            }
        }
        anyhow::bail!(
            "artifact application failed: {error}; rollback conflicts: {conflicts:?}; recovery: {}",
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
    fn original_permissions_survive_application_and_rollback_and_are_recorded() {
        use std::os::unix::fs::PermissionsExt;
        for fail in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let repo = root.path().join("repo");
            let name = "site/content/changelog/index.md".to_string();
            fs::create_dir_all(repo.join("site/content/changelog")).unwrap();
            let path = repo.join(&name);
            fs::write(&path, b"old").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
            let plan = ArtifactPlan {
                schema: 1,
                edits: BTreeMap::from([(name.clone(), Some("new".into()))]),
                originals: BTreeMap::from([(name.clone(), Some(b"old".to_vec()))]),
            };
            let backup = root.path().join("backup");
            let result = apply(&repo, &plan, &backup, |_| {
                ensure!(!fail, "injected failure");
                Ok(())
            });
            assert_eq!(result.is_err(), fail);
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o640
            );
            assert_eq!(fs::read(&path).unwrap(), if fail { b"old" } else { b"new" });
            let modes: serde_json::Value =
                serde_json::from_slice(&fs::read(backup.join("permissions.json")).unwrap())
                    .unwrap();
            assert_eq!(modes[&name]["unixMode"].as_u64().unwrap() & 0o777, 0o640);
            assert_eq!(modes[&name]["readonly"], false);
            assert_eq!(
                fs::metadata(backup.join("recovery.json"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o077,
                0
            );
        }
    }
    #[cfg(unix)]
    #[test]
    fn redirected_artifact_parents_are_rejected_before_backup_or_writes() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let outside = root.path().join("outside");
        fs::create_dir(&repo).unwrap();
        fs::create_dir_all(outside.join("content/changelog")).unwrap();
        let name = "site/content/changelog/index.md".to_string();
        fs::write(outside.join("content/changelog/index.md"), b"original").unwrap();
        std::os::unix::fs::symlink(&outside, repo.join("site")).unwrap();
        let plan = ArtifactPlan {
            schema: 1,
            edits: BTreeMap::from([(name.clone(), Some("replacement".into()))]),
            originals: BTreeMap::from([(name, Some(b"original".to_vec()))]),
        };
        let backup = root.path().join("backup");
        assert!(apply(&repo, &plan, &backup, |_| panic!("must not write")).is_err());
        assert!(!backup.exists());
        assert_eq!(
            fs::read(outside.join("content/changelog/index.md")).unwrap(),
            b"original"
        );
    }

    #[test]
    fn stale_originals_and_internal_backups_fail_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        fs::create_dir_all(repo.join("site/content/changelog")).unwrap();
        let name = "site/content/changelog/index.md".to_string();
        let path = repo.join(&name);
        fs::write(&path, b"concurrent").unwrap();
        let mut plan = ArtifactPlan {
            schema: 1,
            edits: BTreeMap::from([(name.clone(), Some("replacement".into()))]),
            originals: BTreeMap::from([(name.clone(), Some(b"old".to_vec()))]),
        };
        let external = root.path().join("external-backup");
        assert!(apply(&repo, &plan, &external, |_| panic!("must not write")).is_err());
        assert!(!external.exists());
        plan.originals.insert(name, Some(b"concurrent".to_vec()));
        let internal = repo.join("backup");
        assert!(apply(&repo, &plan, &internal, |_| panic!("must not write")).is_err());
        assert!(!internal.exists());
        assert_eq!(fs::read(path).unwrap(), b"concurrent");
    }
    #[test]
    fn rollback_preserves_a_concurrent_editor_change() {
        let root = tempfile::tempdir().unwrap();
        let repo = root.path().join("repo");
        let name = "site/content/changelog/index.md".to_string();
        fs::create_dir_all(repo.join("site/content/changelog")).unwrap();
        fs::write(repo.join(&name), b"old").unwrap();
        let plan = ArtifactPlan {
            schema: 1,
            edits: BTreeMap::from([(name.clone(), Some("new".into()))]),
            originals: BTreeMap::from([(name.clone(), Some(b"old".to_vec()))]),
        };
        let backup = root.path().join("backup");
        let error = apply(&repo, &plan, &backup, |_| {
            fs::write(repo.join(&name), b"external edit")?;
            anyhow::bail!("injected interruption")
        })
        .unwrap_err();
        assert!(error.to_string().contains("concurrent change preserved"));
        assert_eq!(fs::read(repo.join(&name)).unwrap(), b"external edit");
        assert!(backup.join("permissions.json").is_file());
        let recovery: serde_json::Value =
            serde_json::from_slice(&fs::read(backup.join("recovery.json")).unwrap()).unwrap();
        assert_eq!(
            recovery["originals"][&name],
            serde_json::json!(b"old".to_vec())
        );
    }
    #[test]
    fn applies_creations_replacements_and_removals_with_recovery_and_rollback() {
        for failure in [None, Some(1), Some(2), Some(3)] {
            let temp = tempfile::tempdir().unwrap();
            let repo = temp.path().join("repo");
            fs::create_dir_all(repo.join("site/content/changelog")).unwrap();
            let old = "site/content/changelog/0.1.md".to_string();
            let index = "site/content/changelog/index.md".to_string();
            let latest = "site/data/releases/latest.json".to_string();
            fs::write(repo.join(&old), b"old page").unwrap();
            fs::write(repo.join(&index), b"old index").unwrap();
            let plan = ArtifactPlan {
                schema: 1,
                edits: BTreeMap::from([
                    (old.clone(), None),
                    (index.clone(), Some("new index".into())),
                    (latest.clone(), Some("{}\n".into())),
                ]),
                originals: BTreeMap::from([
                    (old.clone(), Some(b"old page".to_vec())),
                    (index.clone(), Some(b"old index".to_vec())),
                    (latest.clone(), None),
                ]),
            };
            let backup = temp.path().join("backup");
            let result = apply(&repo, &plan, &backup, |step| {
                ensure!(failure != Some(step), "injected failure");
                Ok(())
            });
            assert_eq!(result.is_err(), failure.is_some());
            assert!(backup.join("recovery.json").is_file());
            for name in plan.edits.keys() {
                let expected = if failure.is_some() {
                    plan.originals[name].clone()
                } else {
                    plan.edits[name].as_ref().map(|s| s.as_bytes().to_vec())
                };
                assert_eq!(contents(&repo.join(name)).unwrap(), expected);
            }
            assert!(apply(&repo, &plan, &backup, |_| Ok(())).is_err());
        }
    }
}
