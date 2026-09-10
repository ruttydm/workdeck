//! Recoverable binary image writes. Not a crash-atomic directory transaction.
use super::{Plan, check_parents, read_regular};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Component, Path},
};

fn write(path: &Path, bytes: &[u8], mode: Option<fs::Permissions>, absent: bool) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(path.parent().context("missing parent")?)?;
    file.write_all(bytes)?;
    if let Some(mode) = mode {
        file.as_file().set_permissions(mode)?;
    }
    file.as_file().sync_all()?;
    if absent {
        file.persist_noclobber(path)?;
    } else {
        file.persist(path)?;
    }
    Ok(())
}

pub(in crate::social_cards) fn apply(
    repo: &Path,
    plan: &Plan,
    backup: &Path,
    mut after_write: impl FnMut(usize) -> Result<()>,
) -> Result<()> {
    ensure!(
        plan.originals.keys().eq(plan.replacements.keys()),
        "publication plan keys differ"
    );
    let repo = repo.canonicalize()?;
    let parent = backup
        .parent()
        .context("backup parent missing")?
        .canonicalize()?;
    ensure!(
        !parent.starts_with(&repo),
        "publication backup must be outside repository"
    );
    let backup = parent.join(backup.file_name().context("backup filename missing")?);
    let mut modes = BTreeMap::new();
    for name in plan.replacements.keys() {
        ensure!(
            Path::new(name)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
                && (name.starts_with("site/static/changelog/og/")
                    || name == "site/static/extensions/og.png"),
            "invalid publication destination"
        );
        check_parents(&repo, name)?;
        ensure!(
            read_regular(&repo.join(name))? == plan.originals[name],
            "publication original changed: {name}"
        );
        let mode = if plan.originals[name].is_some() {
            Some(fs::symlink_metadata(repo.join(name))?.permissions())
        } else {
            None
        };
        modes.insert(name.clone(), mode);
    }
    fs::create_dir(&backup).context("publication backup must not already exist")?;
    write(
        &backup.join("recovery.json"),
        &serde_json::to_vec_pretty(plan)?,
        None,
        true,
    )?;
    let permissions: BTreeMap<_, _> = modes
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
                serde_json::json!({"readonly": mode.readonly(), "unixMode": unix_mode})
            });
            (name, value)
        })
        .collect();
    write(
        &backup.join("permissions.json"),
        &serde_json::to_vec_pretty(&permissions)?,
        None,
        true,
    )?;
    let mut written = Vec::new();
    let result = (|| -> Result<()> {
        for (name, replacement) in &plan.replacements {
            check_parents(&repo, name)?;
            let path = repo.join(name);
            fs::create_dir_all(path.parent().context("missing image parent")?)?;
            check_parents(&repo, name)?;
            ensure!(
                read_regular(&path)? == plan.originals[name],
                "publication changed before write: {name}"
            );
            match replacement {
                Some(bytes) => write(
                    &path,
                    bytes,
                    modes[name].clone(),
                    plan.originals[name].is_none(),
                )?,
                None => fs::remove_file(&path)?,
            }
            written.push(name);
            after_write(written.len())?;
        }
        for (name, bytes) in &plan.replacements {
            check_parents(&repo, name)?;
            ensure!(
                read_regular(&repo.join(name))? == *bytes,
                "publication changed after write: {name}"
            );
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut conflicts = Vec::new();
        for name in written.into_iter().rev() {
            let restore = (|| -> Result<()> {
                check_parents(&repo, name)?;
                let path = repo.join(name);
                ensure!(
                    read_regular(&path)? == plan.replacements[name],
                    "concurrent image change preserved"
                );
                match &plan.originals[name] {
                    Some(bytes) => write(
                        &path,
                        bytes,
                        modes[name].clone(),
                        plan.replacements[name].is_none(),
                    ),
                    None => Ok(fs::remove_file(path)?),
                }
            })();
            if let Err(error) = restore {
                conflicts.push(format!("{name}: {error}"));
            }
        }
        anyhow::bail!(
            "publication failed: {error}; rollback conflicts: {conflicts:?}; recovery: {}",
            backup.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn binary_application_recovers_each_partial_write() {
        for failure in 0..=3 {
            let outer = tempfile::tempdir().unwrap();
            let repo = outer.path().join("repo");
            fs::create_dir_all(repo.join("site/static/changelog/og")).unwrap();
            let a = "site/static/changelog/og/a.png".to_string();
            let b = "site/static/changelog/og/b.png".to_string();
            let c = "site/static/extensions/og.png".to_string();
            fs::write(repo.join(&a), [0, 255, 1]).unwrap();
            fs::write(repo.join(&b), b"stale").unwrap();
            let plan = Plan {
                originals: BTreeMap::from([
                    (a.clone(), Some(vec![0, 255, 1])),
                    (b.clone(), Some(b"stale".to_vec())),
                    (c.clone(), None),
                ]),
                replacements: BTreeMap::from([
                    (a, Some(vec![255, 0, 9])),
                    (b, None),
                    (c, Some(vec![0, 255])),
                ]),
            };
            let backup = outer.path().join("backup");
            let result = apply(&repo, &plan, &backup, |step| {
                ensure!(step != failure, "injected failure");
                Ok(())
            });
            assert_eq!(result.is_ok(), failure == 0);
            let expected = if failure == 0 {
                &plan.replacements
            } else {
                &plan.originals
            };
            for (name, bytes) in expected {
                assert_eq!(read_regular(&repo.join(name)).unwrap(), *bytes);
            }
            let saved: Plan =
                serde_json::from_slice(&fs::read(backup.join("recovery.json")).unwrap()).unwrap();
            assert_eq!(saved, plan);
            assert!(backup.join("permissions.json").is_file());
        }
    }
}
