//! Publish a complete authenticated first installation without replacing old state.
use anyhow::{Context, Result, ensure};
use std::{fs, io::Read, path::Path};

/// Create a new native installation root. Its parent must exist. Existing roots
/// are never replaced; shell PATH edits are deliberately a separate operation.
pub fn create_authenticated_installation(
    archive: &Path,
    checksums: &Path,
    destination: &Path,
    expected_target: &str,
    identity: super::ReleaseIdentity<'_>,
) -> Result<()> {
    publish(
        destination,
        expected_target,
        || super::prepare_authenticated_archive(archive, checksums, identity),
        || Ok(()),
    )
}

fn publish(
    destination: &Path,
    expected_target: &str,
    stage: impl FnOnce() -> Result<tempfile::TempDir>,
    before_publish: impl FnOnce() -> Result<()>,
) -> Result<()> {
    let expected = super::metadata::PrebuiltMetadata::for_target(expected_target)?;
    let staged = stage()?;
    let source = staged.path().join(&expected.package_name);
    ensure!(
        fs::read_dir(staged.path())?.count() == 1 && source.is_dir(),
        "archive wrapper does not match selected target"
    );
    let mut metadata = Vec::new();
    fs::File::open(source.join("metadata.json"))?
        .take(65537)
        .read_to_end(&mut metadata)?;
    super::metadata::PrebuiltMetadata::decode(&metadata, expected_target)?;
    for skill in workdeck_core::BUNDLED_SKILL_NAMES {
        ensure!(
            source.join("skills").join(skill).join("SKILL.md").is_file(),
            "missing bundled skill {skill}"
        );
    }
    let name = destination
        .file_name()
        .context("installation root requires a name")?;
    let parent = destination
        .parent()
        .context("installation root requires a parent")?
        .canonicalize()?;
    let destination = parent.join(name);
    match fs::symlink_metadata(&destination) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
        Ok(_) => anyhow::bail!("installation root already exists"),
    }
    let temporary = tempfile::Builder::new()
        .prefix(".workdeck-installation-")
        .tempdir_in(&parent)?;
    let prepared = temporary.path().join("installation");
    super::assets::copy_tree(
        &source,
        &prepared,
        &mut 0,
        &mut 0,
        0,
        2 * 1024 * 1024 * 1024,
    )?;
    fs::create_dir(prepared.join("bin"))?;
    let binary = prepared.join("bin").join(&expected.binary_name);
    super::assets::rename_new(&prepared.join(&expected.binary_name), &binary)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))?;
    }
    fs::File::open(&binary)?.sync_all()?;
    before_publish()?;
    super::assets::rename_new(&prepared, &destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn staged() -> Result<tempfile::TempDir> {
        let stage = tempfile::tempdir()?;
        let metadata =
            super::super::metadata::PrebuiltMetadata::for_target("aarch64-apple-darwin")?;
        let root = stage.path().join(&metadata.package_name);
        fs::create_dir(&root)?;
        fs::write(root.join("metadata.json"), metadata.encode()?)?;
        for name in [
            "workdeck",
            "LICENSE",
            "THIRD_PARTY_NOTICES",
            "licenses.json",
            "sbom.cdx.json",
            "provenance.json",
        ] {
            fs::write(root.join(name), name)?;
        }
        for skill in workdeck_core::BUNDLED_SKILL_NAMES {
            let path = root.join("skills").join(skill);
            fs::create_dir_all(&path)?;
            fs::write(path.join("SKILL.md"), skill)?;
        }
        Ok(stage)
    }
    #[test]
    fn complete_first_install_publishes_together_and_preserves_competing_roots() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join(".workdeck");
        let target = "aarch64-apple-darwin";
        assert!(
            publish(
                &root,
                target,
                || anyhow::bail!("authentication rejected"),
                || Ok(())
            )
            .is_err()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        assert!(
            publish(&root, target, staged, || anyhow::bail!(
                "prepublication failure"
            ))
            .is_err()
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        publish(&root, target, staged, || Ok(())).unwrap();
        assert_eq!(fs::read(root.join("bin/workdeck")).unwrap(), b"workdeck");
        assert!(root.join("skills/workdeck-review/SKILL.md").is_file());
        assert!(root.join("metadata.json").is_file());
        assert!(root.join("LICENSE").is_file());
        assert!(!root.join("workdeck").exists());
        assert!(publish(&root, target, staged, || Ok(())).is_err());
        let competing = dir.path().join("competing");
        assert!(
            publish(&competing, target, staged, || {
                fs::create_dir(&competing)?;
                Ok(())
            })
            .is_err()
        );
        assert_eq!(fs::read_dir(competing).unwrap().count(), 0);
    }
}
