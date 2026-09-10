//! Recoverable native installation of an authenticated bundled skills tree.
use anyhow::{Context, Result, ensure};
use std::{fs, io::Write, path::Path};

/// The caller must authenticate the source tree first. The destination parent
/// must exist and remain quiescent; recovery is a new directory retained on disk.
/// This transaction does not install the executable or metadata.
pub fn install_skill_tree(source: &Path, target: &Path, recovery: &Path) -> Result<()> {
    install(source, target, recovery, || Ok(()))
}

/// Install skills only after the complete release archive is authenticated.
/// Binary and metadata installation remain separate operations.
pub fn install_authenticated_skills(
    archive: &Path,
    checksums: &Path,
    target: &Path,
    recovery: &Path,
    expected_target: &str,
    identity: super::ReleaseIdentity<'_>,
) -> Result<()> {
    install_from_archive(target, recovery, expected_target, || {
        super::prepare_authenticated_archive(archive, checksums, identity)
    })
}

fn install_from_archive(
    target: &Path,
    recovery: &Path,
    expected_target: &str,
    stage: impl FnOnce() -> Result<tempfile::TempDir>,
) -> Result<()> {
    super::metadata::PrebuiltMetadata::for_target(expected_target)?;
    let staged = stage()?;
    let mut roots = fs::read_dir(staged.path())?;
    let root = roots
        .next()
        .context("authenticated archive is empty")??
        .path();
    ensure!(
        roots.next().is_none(),
        "authenticated archive has multiple roots"
    );
    let triple = root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("workdeck-"))
        .context("invalid native package wrapper")?;
    ensure!(
        triple == expected_target,
        "archive target does not match selected installation target"
    );
    use std::io::Read;
    let mut metadata = Vec::new();
    fs::File::open(root.join("metadata.json"))?
        .take(65537)
        .read_to_end(&mut metadata)?;
    super::metadata::PrebuiltMetadata::decode(&metadata, triple)?;
    install_skill_tree(&root.join("skills"), target, recovery)
}

fn rename_new(source: &Path, target: &Path) -> Result<()> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    rustix::fs::renameat_with(
        rustix::fs::CWD,
        source,
        rustix::fs::CWD,
        target,
        rustix::fs::RenameFlags::NOREPLACE,
    )?;
    #[cfg(windows)]
    fs::rename(source, target)?;
    #[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
    anyhow::bail!("exclusive directory installation is unsupported on this platform");
    Ok(())
}

fn copy_tree(
    source: &Path,
    target: &Path,
    count: &mut usize,
    bytes: &mut u64,
    depth: usize,
) -> Result<()> {
    ensure!(depth <= 64, "skills tree exceeds nesting limit");
    super::transaction::reject_reparse_point(&fs::symlink_metadata(source)?)?;
    ensure!(
        fs::symlink_metadata(source)?.file_type().is_dir(),
        "skills source must be a non-symlink directory"
    );
    fs::create_dir(target)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        *count += 1;
        ensure!(*count <= 10_000, "skills tree exceeds entry limit");
        let metadata = fs::symlink_metadata(entry.path())?;
        super::transaction::reject_reparse_point(&metadata)?;
        let destination = target.join(entry.file_name());
        if metadata.file_type().is_dir() {
            copy_tree(&entry.path(), &destination, count, bytes, depth + 1)?;
        } else {
            ensure!(
                metadata.file_type().is_file(),
                "skills tree contains a link or special file"
            );
            *bytes = bytes
                .checked_add(metadata.len())
                .context("skills size overflow")?;
            ensure!(*bytes <= 64 * 1024 * 1024, "skills tree exceeds byte limit");
            let (_, content) = super::transaction::read_binary(&entry.path())?;
            ensure!(
                content.len() as u64 == metadata.len(),
                "skill changed during copy"
            );
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(destination)?;
            output.write_all(&content)?;
            output.sync_all()?;
        }
    }
    Ok(())
}

fn install(
    source: &Path,
    target: &Path,
    recovery: &Path,
    before_publish: impl FnOnce() -> Result<()>,
) -> Result<()> {
    ensure!(
        target.file_name().is_some_and(|name| name == "skills"),
        "unexpected skills target name"
    );
    let parent = target
        .parent()
        .context("skills target needs a parent")?
        .canonicalize()?;
    let target = parent.join("skills");
    ensure!(
        !parent.starts_with(source.canonicalize()?),
        "skills source contains installation staging directory"
    );
    let temporary = tempfile::Builder::new()
        .prefix(".workdeck-skills-")
        .tempdir_in(&parent)?;
    let prepared = temporary.path().join("skills");
    copy_tree(source, &prepared, &mut 0, &mut 0, 0)?;
    for name in workdeck_core::BUNDLED_SKILL_NAMES {
        ensure!(
            prepared.join(name).join("SKILL.md").is_file(),
            "missing bundled skill {name}"
        );
    }
    let _lock = super::transaction::lock_directory(&parent)?;
    let exists = match fs::symlink_metadata(&target) {
        Ok(metadata) => {
            super::transaction::reject_reparse_point(&metadata)?;
            ensure!(
                metadata.file_type().is_dir(),
                "installed skills must be a non-symlink directory"
            );
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    fs::create_dir(recovery).context("recovery directory exists or cannot be created")?;
    let backup = recovery.join("skills");
    if exists {
        rename_new(&target, &backup)?;
    }
    let result = before_publish().and_then(|()| rename_new(&prepared, &target));
    if let Err(error) = result {
        if exists {
            rename_new(&backup, &target).with_context(|| {
                format!(
                    "installation failed ({error}); restore failed; recovery retained at {}",
                    backup.display()
                )
            })?;
        }
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn authenticated_archive_precedes_destination_writes_and_installs_exact_skills() {
        let input = tempfile::tempdir().unwrap();
        let archive = input.path().join("workdeck.zip");
        let checksums = input.path().join("checksums");
        let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
        let mut entries: Vec<String> = [
            "workdeck",
            "LICENSE",
            "THIRD_PARTY_NOTICES",
            "licenses.json",
            "sbom.cdx.json",
            "provenance.json",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        entries.extend(
            workdeck_core::BUNDLED_SKILL_NAMES
                .iter()
                .map(|name| format!("skills/{name}/SKILL.md")),
        );
        for entry in &entries {
            writer
                .start_file(
                    format!("workdeck-aarch64-apple-darwin/{entry}"),
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
            writer.write_all(entry.as_bytes()).unwrap();
        }
        writer
            .start_file(
                "workdeck-aarch64-apple-darwin/metadata.json",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(
                &super::super::metadata::PrebuiltMetadata::for_target("aarch64-apple-darwin")
                    .unwrap()
                    .encode()
                    .unwrap(),
            )
            .unwrap();
        writer.finish().unwrap();
        let file = fs::File::open(&archive).unwrap();
        let hash = super::super::hash_archive_bytes(&file, file.metadata().unwrap().len()).unwrap();
        fs::write(&checksums, format!("{hash} workdeck.zip\n")).unwrap();
        let destination = tempfile::tempdir().unwrap();
        let target = destination.path().join("skills");
        let recovery = destination.path().join("recovery");
        assert!(
            install_from_archive(&target, &recovery, "aarch64-apple-darwin", || {
                super::super::staging::prepare_archive(&archive, &checksums, |_| {
                    anyhow::bail!("untrusted archive")
                })
            })
            .is_err()
        );
        assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
        assert!(
            install_from_archive(&target, &recovery, "aarch64-apple-darwin", || {
                let staged =
                    super::super::staging::prepare_archive(&archive, &checksums, |_| Ok(()))?;
                fs::write(
                    staged
                        .path()
                        .join("workdeck-aarch64-apple-darwin/metadata.json"),
                    b"{}",
                )?;
                Ok(staged)
            })
            .is_err()
        );
        assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
        let wrong_platform =
            install_from_archive(&target, &recovery, "x86_64-pc-windows-msvc", || {
                super::super::staging::prepare_archive(&archive, &checksums, |_| Ok(()))
            });
        assert!(
            wrong_platform
                .unwrap_err()
                .to_string()
                .contains("selected installation target")
        );
        assert_eq!(fs::read_dir(destination.path()).unwrap().count(), 0);
        let invalid_target = install_from_archive(&target, &recovery, "invalid-target", || {
            panic!("unsupported target must be rejected before staging")
        });
        assert!(invalid_target.is_err());
        install_from_archive(&target, &recovery, "aarch64-apple-darwin", || {
            super::super::staging::prepare_archive(&archive, &checksums, |_| Ok(()))
        })
        .unwrap();
        for name in workdeck_core::BUNDLED_SKILL_NAMES {
            assert_eq!(
                fs::read(target.join(name).join("SKILL.md")).unwrap(),
                format!("skills/{name}/SKILL.md").as_bytes()
            );
        }
        assert!(recovery.is_dir());
    }
    #[test]
    fn installs_complete_tree_retains_old_tree_and_restores_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        for name in workdeck_core::BUNDLED_SKILL_NAMES {
            let path = source.join(name);
            fs::create_dir_all(&path).unwrap();
            fs::write(path.join("SKILL.md"), name).unwrap();
        }
        let target = dir.path().join("skills");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("old"), b"keep me").unwrap();
        let recovery = dir.path().join("backup");
        install(&source, &target, &recovery, || {
            anyhow::bail!("injected failure")
        })
        .unwrap_err();
        assert_eq!(fs::read(target.join("old")).unwrap(), b"keep me");
        let recovery = dir.path().join("success");
        install_skill_tree(&source, &target, &recovery).unwrap();
        assert_eq!(fs::read(recovery.join("skills/old")).unwrap(), b"keep me");
        for name in workdeck_core::BUNDLED_SKILL_NAMES {
            assert_eq!(
                fs::read(target.join(name).join("SKILL.md")).unwrap(),
                name.as_bytes()
            );
        }
        assert!(install_skill_tree(&source, &target, &recovery).is_err());
        let conflict_recovery = dir.path().join("conflict");
        let result = install(&source, &target, &conflict_recovery, || {
            fs::create_dir(&target)?;
            fs::write(target.join("concurrent"), b"preserve concurrent tree")?;
            Ok(())
        });
        assert!(result.unwrap_err().to_string().contains("restore failed"));
        assert_eq!(
            fs::read(target.join("concurrent")).unwrap(),
            b"preserve concurrent tree"
        );
        assert!(
            conflict_recovery
                .join("skills/workdeck-review/SKILL.md")
                .is_file()
        );
    }
}
