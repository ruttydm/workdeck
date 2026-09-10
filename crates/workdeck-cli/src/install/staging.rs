//! Verified, private archive staging. Does not replace an installed executable.
use super::*;
use std::io::{Read, Seek, Write};

/// Verify a local archive and return an owned staging directory.
/// The caller must authenticate the checksum manifest separately. Dropping the
/// returned handle removes the staged files; no executable is installed.
pub fn prepare_verified_archive(archive: &Path, checksums: &Path) -> Result<tempfile::TempDir> {
    prepare_archive(archive, checksums, |_| Ok(()))
}

/// Authenticate the complete archive, including assets, using GitHub's published
/// archive attestation. Release identity must be resolved independently.
pub fn prepare_authenticated_archive(
    archive: &Path,
    checksums: &Path,
    identity: ReleaseIdentity<'_>,
) -> Result<tempfile::TempDir> {
    let policy =
        attestation::verification_args(identity.repository, identity.commit, identity.tag_ref)?;
    prepare_archive(archive, checksums, |snapshot| {
        use std::process::{Command, Stdio};
        let mut verifier = Command::new("gh")
            .args(["attestation", "verify"])
            .arg(snapshot)
            .args(&policy)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .env("GH_PROMPT_DISABLED", "1")
            .spawn()?;
        attestation::wait_verifier(&mut verifier, std::time::Duration::from_secs(120))
    })
}

pub(super) fn prepare_archive(
    archive: &Path,
    checksums: &Path,
    authenticate: impl FnOnce(&Path) -> Result<()>,
) -> Result<tempfile::TempDir> {
    let name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow::anyhow!("Archive name is not valid UTF-8"))?;
    let expected = expected_checksum(&read_checksum_manifest(checksums)?, name)?;
    // Validate and extract a private snapshot, not a path that can be replaced
    // between checksum verification and extraction.
    let source = open_archive_input(archive)?;
    let metadata = source.metadata()?;
    validate_archive_input(&metadata)?;
    let size = metadata.len();
    let private = tempfile::Builder::new()
        .prefix("workdeck-archive-auth-")
        .tempdir()?;
    let snapshot_path = private.path().join(name);
    let mut snapshot = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(&snapshot_path)?;
    let copied = std::io::copy(&mut source.take(size + 1), &mut snapshot)?;
    anyhow::ensure!(copied == size, "Archive size changed during staging");
    snapshot.rewind()?;
    let actual = hash_archive_bytes(&mut snapshot, size)?;
    anyhow::ensure!(
        actual == expected,
        "Checksum verification failed for {name}"
    );
    snapshot.sync_all()?;
    authenticate(&snapshot_path)?;
    snapshot.rewind()?;
    anyhow::ensure!(
        hash_archive_bytes(&mut snapshot, size)? == expected,
        "archive snapshot changed during authentication"
    );
    let reopened = open_archive_input(&snapshot_path)?;
    anyhow::ensure!(
        reopened.metadata()?.len() == size && hash_archive_bytes(&reopened, size)? == expected,
        "archive snapshot path changed during authentication"
    );
    snapshot.rewind()?;
    let zip = archive
        .extension()
        .is_some_and(|extension| extension == "zip");
    let (paths, _) = inspect_archive_file(snapshot.try_clone()?, zip)?;
    verify_package_paths(&paths)?;
    snapshot.rewind()?;
    let staged = tempfile::Builder::new()
        .prefix("workdeck-install-")
        .tempdir()?;
    let extract = |name: &str, directory: bool, reader: &mut dyn Read| -> Result<()> {
        let path = staged.path().join(archive_entry_path(name)?);
        if directory {
            std::fs::create_dir_all(path)?;
        } else {
            std::fs::create_dir_all(path.parent().unwrap())?;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)?;
            std::io::copy(reader, &mut output)?;
            output.flush()?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let executable = path.file_name().is_some_and(|name| name == "workdeck");
                output.set_permissions(std::fs::Permissions::from_mode(if executable {
                    0o755
                } else {
                    0o644
                }))?;
            }
            output.sync_all()?;
        }
        Ok(())
    };
    if zip {
        let mut archive = zip::ZipArchive::new(snapshot)?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            let name = entry.name().to_owned();
            extract(&name, entry.is_dir(), &mut entry)?;
        }
    } else {
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(snapshot));
        for entry in archive.entries()? {
            let mut entry = entry?;
            let name = std::str::from_utf8(&entry.path_bytes())?.to_owned();
            extract(&name, entry.header().entry_type().is_dir(), &mut entry)?;
        }
    }
    Ok(staged)
}

pub fn stage(mut args: impl Iterator<Item = String>) -> Result<()> {
    let archive = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("install-stage requires ARCHIVE CHECKSUM_FILE"))?;
    let checksums = args
        .next()
        .ok_or_else(|| anyhow::anyhow!("install-stage requires ARCHIVE CHECKSUM_FILE"))?;
    anyhow::ensure!(
        args.next().is_none(),
        "install-stage accepts exactly ARCHIVE CHECKSUM_FILE"
    );
    let staged = prepare_verified_archive(Path::new(&archive), Path::new(&checksums))?;
    let report = serde_json::to_string(&serde_json::json!({
        "stagingDirectory": staged.path(), "checksumVerified": true,
        "signatureVerified": false, "installed": false
    }))?;
    println!("{report}");
    let _ = staged.keep();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tar_staging_preserves_payloads_discards_special_modes_and_checks_structure() {
        let input = tempfile::tempdir().unwrap();
        let archive = input.path().join("workdeck.tar.gz");
        let checksums = input.path().join("SHA256SUMS");
        for variant in ["valid", "missing-license", "symlink"] {
            let encoder = flate2::write::GzEncoder::new(
                std::fs::File::create(&archive).unwrap(),
                flate2::Compression::default(),
            );
            let mut tar = tar::Builder::new(encoder);
            for name in [
                "workdeck",
                "LICENSE",
                "THIRD_PARTY_NOTICES",
                "licenses.json",
                "sbom.cdx.json",
                "provenance.json",
            ] {
                if variant == "missing-license" && name == "LICENSE" {
                    continue;
                }
                let payload = format!("{name}\0binary\n");
                let mut header = tar::Header::new_gnu();
                header.set_size(payload.len() as u64);
                header.set_mode(0o6777);
                header.set_cksum();
                tar.append_data(&mut header, format!("package/{name}"), payload.as_bytes())
                    .unwrap();
            }
            if variant == "symlink" {
                let mut header = tar::Header::new_gnu();
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_size(0);
                header.set_mode(0o777);
                tar.append_link(&mut header, "package/link", "../../outside")
                    .unwrap();
            }
            tar.into_inner().unwrap().finish().unwrap();
            let bytes = std::fs::read(&archive).unwrap();
            let hash = hash_archive_bytes(
                std::fs::File::open(&archive).unwrap(),
                std::fs::metadata(&archive).unwrap().len(),
            )
            .unwrap();
            std::fs::write(&checksums, format!("{hash} workdeck.tar.gz\n")).unwrap();
            let result = prepare_verified_archive(&archive, &checksums);
            if variant == "valid" {
                let mut observed = None;
                let rejected = prepare_archive(&archive, &checksums, |snapshot| {
                    observed = Some(snapshot.to_owned());
                    assert_eq!(std::fs::read(snapshot)?, bytes);
                    anyhow::bail!("publisher rejected")
                });
                assert!(
                    rejected
                        .unwrap_err()
                        .to_string()
                        .contains("publisher rejected")
                );
                assert!(!observed.unwrap().exists());
                assert!(
                    prepare_archive(&archive, &checksums, |snapshot| {
                        std::fs::write(snapshot, b"modified")?;
                        Ok(())
                    })
                    .is_err()
                );
                let authenticated = prepare_archive(&archive, &checksums, |snapshot| {
                    assert_eq!(std::fs::read(snapshot)?, bytes);
                    Ok(())
                })
                .unwrap();
                assert!(authenticated.path().join("package/LICENSE").is_file());
                let staged = result.unwrap();
                for name in ["workdeck", "LICENSE"] {
                    let path = staged.path().join("package").join(name);
                    assert_eq!(
                        std::fs::read(&path).unwrap(),
                        format!("{name}\0binary\n").as_bytes()
                    );
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        assert_eq!(
                            std::fs::metadata(path).unwrap().permissions().mode() & 0o7777,
                            if name == "workdeck" { 0o755 } else { 0o644 }
                        );
                    }
                }
                let path = staged.path().to_owned();
                drop(staged);
                assert!(!path.exists());
            } else {
                let error = result.unwrap_err().to_string();
                assert!(
                    error.contains(if variant == "symlink" {
                        "links and special files"
                    } else {
                        "missing required regular file"
                    }),
                    "{error}"
                );
            }
            assert_eq!(std::fs::read(&archive).unwrap(), bytes);
            assert_eq!(std::fs::read_dir(input.path()).unwrap().count(), 2);
        }
    }

    #[test]
    fn staging_extracts_verified_package_and_rejects_bad_checksum() {
        let input = tempfile::tempdir().unwrap();
        let archive = input.path().join("workdeck.zip");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&archive).unwrap());
        for name in [
            "workdeck",
            "LICENSE",
            "THIRD_PARTY_NOTICES",
            "licenses.json",
            "sbom.cdx.json",
            "provenance.json",
        ] {
            zip.start_file(
                format!("package/{name}"),
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            zip.write_all(name.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
        let hash = hash_archive_bytes(
            std::fs::File::open(&archive).unwrap(),
            std::fs::metadata(&archive).unwrap().len(),
        )
        .unwrap();
        let checksums = input.path().join("SHA256SUMS");
        std::fs::write(&checksums, format!("{hash} workdeck.zip\n")).unwrap();
        let staged = prepare_verified_archive(&archive, &checksums).unwrap();
        assert_eq!(
            std::fs::read(staged.path().join("package/workdeck")).unwrap(),
            b"workdeck"
        );
        assert_eq!(
            std::fs::read_dir(staged.path().join("package"))
                .unwrap()
                .count(),
            6
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(staged.path().join("package/workdeck"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o7777,
                0o755
            );
        }
        std::fs::write(&checksums, format!("{} workdeck.zip\n", "0".repeat(64))).unwrap();
        assert!(prepare_verified_archive(&archive, &checksums).is_err());
        assert_eq!(std::fs::read_dir(input.path()).unwrap().count(), 2);
    }
}
