//! Verified, private archive staging. Does not replace an installed executable.
use super::*;
use std::io::{Read, Seek, Write};

fn prepare(archive: &Path, checksums: &Path) -> Result<tempfile::TempDir> {
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
    let mut snapshot = tempfile::tempfile()?;
    let copied = std::io::copy(&mut source.take(size + 1), &mut snapshot)?;
    anyhow::ensure!(copied == size, "Archive size changed during staging");
    snapshot.rewind()?;
    let actual = hash_archive_bytes(&mut snapshot, size)?;
    anyhow::ensure!(
        actual == expected,
        "Checksum verification failed for {name}"
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

pub(crate) fn stage(mut args: impl Iterator<Item = String>) -> Result<()> {
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
    let staged = prepare(Path::new(&archive), Path::new(&checksums))?;
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
        let hash = crate::sha256_file(&archive).unwrap();
        let checksums = input.path().join("SHA256SUMS");
        std::fs::write(&checksums, format!("{hash} workdeck.zip\n")).unwrap();
        let staged = prepare(&archive, &checksums).unwrap();
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
        assert!(prepare(&archive, &checksums).is_err());
        assert_eq!(std::fs::read_dir(input.path()).unwrap().count(), 2);
    }
}
