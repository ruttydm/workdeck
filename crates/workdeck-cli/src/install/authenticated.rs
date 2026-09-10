//! Compose local archive staging, publisher authentication and binary replacement.
use super::*;
use std::io::Read;

pub struct ReleaseIdentity<'a> {
    pub repository: &'a str,
    pub commit: &'a str,
    pub tag_ref: &'a str,
}

/// Replace an existing binary from a local release archive after authentication.
/// Release identity must come from trusted release selection, not archive metadata.
/// Accompanying assets and Windows running-process handoff are not installed here.
pub fn install_authenticated_archive(
    archive: &Path,
    checksums: &Path,
    target: &Path,
    backup: &Path,
    identity: ReleaseIdentity<'_>,
) -> Result<()> {
    // Validate identity before reading or staging the archive.
    attestation::verification_args(identity.repository, identity.commit, identity.tag_ref)?;
    install_with(
        archive,
        checksums,
        target,
        backup,
        |binary, bundle, name| {
            attestation::authenticate_binary(
                binary,
                bundle,
                name,
                identity.repository,
                identity.commit,
                identity.tag_ref,
            )
        },
    )
}

fn install_with(
    archive: &Path,
    checksums: &Path,
    target: &Path,
    backup: &Path,
    authenticate: impl FnOnce(Vec<u8>, &[u8], &str) -> Result<Vec<u8>>,
) -> Result<()> {
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("target requires a UTF-8 executable name"))?;
    anyhow::ensure!(
        matches!(name, "workdeck" | "workdeck.exe"),
        "unexpected executable name"
    );
    let staged = prepare_verified_archive(archive, checksums)?;
    let root = std::fs::read_dir(staged.path())?
        .next()
        .ok_or_else(|| anyhow::anyhow!("staged package is empty"))??
        .path();
    let (_, binary) = transaction::read_binary(&root.join(name))?;
    let mut bundle = Vec::new();
    open_archive_input(&root.join("provenance.sigstore.json"))?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bundle)?;
    anyhow::ensure!(
        bundle.len() <= 1024 * 1024,
        "attestation bundle exceeds 1 MiB"
    );
    let authenticated = authenticate(binary, &bundle, name)?;
    replace_binary_with_backup(target, &authenticated, backup)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn authentication_failure_precedes_every_destination_write_and_success_keeps_backup() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("package.zip");
        let checksums = dir.path().join("checksums");
        let target = dir.path().join("workdeck");
        let backup = dir.path().join("backup");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for name in [
            "workdeck",
            "LICENSE",
            "THIRD_PARTY_NOTICES",
            "licenses.json",
            "sbom.cdx.json",
            "provenance.json",
            "provenance.sigstore.json",
        ] {
            zip.start_file(
                format!("root/{name}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(name.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let file = open_archive_input(&archive).unwrap();
        let hash = hash_archive_bytes(&file, file.metadata().unwrap().len()).unwrap();
        std::fs::write(&checksums, format!("{hash} package.zip\n")).unwrap();
        std::fs::write(&target, b"old binary").unwrap();
        let result = install_with(
            &archive,
            &checksums,
            &target,
            &backup,
            |binary, bundle, name| {
                assert_eq!(binary, b"workdeck");
                assert_eq!(bundle, b"provenance.sigstore.json");
                assert_eq!(name, "workdeck");
                anyhow::bail!("rejected publisher identity")
            },
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("publisher identity")
        );
        assert_eq!(std::fs::read(&target).unwrap(), b"old binary");
        assert!(!backup.exists());
        assert!(!dir.path().join(".workdeck-install.lock").exists());
        install_with(&archive, &checksums, &target, &backup, |binary, _, _| {
            Ok(binary)
        })
        .unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"workdeck");
        assert_eq!(std::fs::read(&backup).unwrap(), b"old binary");
    }
}
