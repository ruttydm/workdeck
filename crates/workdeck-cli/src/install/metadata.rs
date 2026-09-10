//! Native prebuilt metadata derived from Hunk's MIT build-prebuilt-artifact.ts.
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path};

/// Install caller-authenticated metadata; preserve exact previous bytes in a new
/// recovery JSON record. Parents must exist and remain quiescent during replacement.
pub fn install(bytes: &[u8], triple: &str, destination: &Path, recovery: &Path) -> Result<()> {
    install_with(bytes, triple, destination, recovery, || Ok(()))
}

fn read_previous(path: &Path) -> Result<Option<(fs::Metadata, Vec<u8>)>> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(Some(super::transaction::read_file_limited(path, 65536)?)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn install_with(
    bytes: &[u8],
    triple: &str,
    destination: &Path,
    recovery: &Path,
    before_commit: impl FnOnce() -> Result<()>,
) -> Result<()> {
    PrebuiltMetadata::decode(bytes, triple)?;
    ensure!(
        destination
            .file_name()
            .is_some_and(|name| name == "metadata.json"),
        "unexpected metadata filename"
    );
    let parent = destination
        .parent()
        .context("metadata needs a parent")?
        .canonicalize()?;
    let destination = parent.join("metadata.json");
    let recovery_parent = recovery
        .parent()
        .context("recovery needs a parent")?
        .canonicalize()?;
    let recovery = recovery_parent.join(recovery.file_name().context("recovery needs a filename")?);
    ensure!(
        destination != recovery,
        "metadata recovery must differ from destination"
    );
    let _lock = super::transaction::lock_directory(&parent)?;
    let previous = read_previous(&destination)?;
    let mut saved = tempfile::NamedTempFile::new_in(&recovery_parent)?;
    serde_json::to_writer(
        saved.as_file_mut(),
        &serde_json::json!({"schema":1,"path":destination,"original":previous.as_ref().map(|(_, bytes)| bytes)}),
    )?;
    saved.as_file().sync_all()?;
    saved
        .persist_noclobber(&recovery)
        .context("metadata recovery exists or cannot be created")?;
    let mut replacement = tempfile::NamedTempFile::new_in(&parent)?;
    replacement.write_all(bytes)?;
    if let Some((metadata, _)) = &previous {
        replacement
            .as_file()
            .set_permissions(metadata.permissions())?;
    }
    replacement.as_file().sync_all()?;
    before_commit()?;
    let current = read_previous(&destination)?;
    ensure!(
        current
            .as_ref()
            .map(|(metadata, bytes)| (metadata.permissions(), bytes))
            == previous
                .as_ref()
                .map(|(metadata, bytes)| (metadata.permissions(), bytes)),
        "metadata changed before installation; recovery retained"
    );
    if previous.is_none() {
        replacement.persist_noclobber(&destination)?;
    } else {
        replacement.persist(&destination)?;
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrebuiltMetadata {
    pub package_name: String,
    pub os: String,
    pub cpu: String,
    pub binary_name: String,
}

impl PrebuiltMetadata {
    pub fn for_target(target: &str) -> Result<Self> {
        let (os, cpu, binary) = match target {
            "aarch64-apple-darwin" => ("darwin", "arm64", "workdeck"),
            "x86_64-apple-darwin" => ("darwin", "x64", "workdeck"),
            "aarch64-unknown-linux-gnu" => ("linux", "arm64", "workdeck"),
            "x86_64-unknown-linux-gnu" => ("linux", "x64", "workdeck"),
            "x86_64-pc-windows-msvc" => ("windows", "x64", "workdeck.exe"),
            _ => bail!("unsupported release package target {target:?}"),
        };
        Ok(Self {
            package_name: format!("workdeck-{target}"),
            os: os.into(),
            cpu: cpu.into(),
            binary_name: binary.into(),
        })
    }

    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut bytes = serde_json::to_vec_pretty(self)?;
        bytes.push(b'\n');
        Ok(bytes)
    }

    pub fn decode(bytes: &[u8], target: &str) -> Result<Self> {
        ensure!(
            bytes.len() <= 64 * 1024,
            "prebuilt metadata exceeds size limit"
        );
        let metadata: Self = serde_json::from_slice(bytes)?;
        ensure!(
            metadata == Self::for_target(target)?,
            "prebuilt metadata does not match release target"
        );
        Ok(metadata)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_install_preserves_recovery_and_concurrent_changes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metadata.json");
        let recovery = dir.path().join("recovery.json");
        let triple = "aarch64-apple-darwin";
        let bytes = PrebuiltMetadata::for_target(triple)
            .unwrap()
            .encode()
            .unwrap();
        assert!(install(b"{}", triple, &path, &recovery).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
        install(&bytes, triple, &path, &recovery).unwrap();
        assert_eq!(fs::read(&path).unwrap(), bytes);
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(&recovery).unwrap()).unwrap();
        assert!(record["original"].is_null());
        fs::write(&path, [255, 10]).unwrap();
        let recovery = dir.path().join("previous.json");
        install(&bytes, triple, &path, &recovery).unwrap();
        let record: serde_json::Value =
            serde_json::from_slice(&fs::read(&recovery).unwrap()).unwrap();
        assert_eq!(record["original"], serde_json::json!([255, 10]));
        assert!(install(&bytes, triple, &path, &recovery).is_err());
        let result = install_with(&bytes, triple, &path, &dir.path().join("race.json"), || {
            fs::write(&path, b"concurrent edit")?;
            Ok(())
        });
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("changed before installation")
        );
        assert_eq!(fs::read(&path).unwrap(), b"concurrent edit");
    }
    #[test]
    fn metadata_rejects_mismatched_duplicate_unknown_and_oversized_fields() {
        let target = "aarch64-apple-darwin";
        let expected = PrebuiltMetadata::for_target(target).unwrap();
        assert_eq!(
            PrebuiltMetadata::decode(&expected.encode().unwrap(), target).unwrap(),
            expected
        );
        let mut wrong = expected.clone();
        wrong.binary_name = "../workdeck".into();
        assert!(PrebuiltMetadata::decode(&wrong.encode().unwrap(), target).is_err());
        assert!(
            PrebuiltMetadata::decode(&expected.encode().unwrap(), "x86_64-apple-darwin").is_err()
        );
        let json = String::from_utf8(expected.encode().unwrap()).unwrap();
        for field in ["\"cpu\":\"arm64\",", "\"unknown\":true,"] {
            assert!(
                PrebuiltMetadata::decode(
                    json.replacen('{', &format!("{{{field}"), 1).as_bytes(),
                    target
                )
                .is_err()
            );
        }
        assert!(PrebuiltMetadata::decode(&vec![b' '; 65537], target).is_err());
    }
}
