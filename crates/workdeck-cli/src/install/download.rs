//! Bounded HTTPS release downloads into owned temporary storage.
use crate::update::{UpdatePlatform, direct_target, parse_update_version};
use anyhow::{Result, ensure};
use std::io::Read;
use std::path::Path;

pub struct DownloadedRelease {
    directory: tempfile::TempDir,
    archive_name: String,
}

impl DownloadedRelease {
    pub fn archive(&self) -> std::path::PathBuf {
        self.directory.path().join(&self.archive_name)
    }
    pub fn checksums(&self) -> std::path::PathBuf {
        self.directory
            .path()
            .join(format!("{}.sha256", self.archive_name))
    }
}

/// Download only; authentication and installation are separate required steps.
pub fn download_release(
    version: &str,
    platform: UpdatePlatform,
    architecture: &str,
) -> Result<DownloadedRelease> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(std::time::Duration::from_secs(120)))
        .max_redirects(10)
        .build()
        .into();
    download_with(version, platform, architecture, |url, path, limit| {
        let mut response = agent
            .get(url)
            .header("User-Agent", "workdeck-update")
            .call()?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)?;
        copy_bounded(response.body_mut().as_reader(), &mut output, limit)?;
        output.sync_all()?;
        Ok(())
    })
}

fn copy_bounded(input: impl Read, output: &mut impl std::io::Write, limit: u64) -> Result<()> {
    let count = std::io::copy(
        &mut input.take(
            limit
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("download limit overflow"))?,
        ),
        output,
    )?;
    ensure!(count <= limit, "release download exceeds size limit");
    Ok(())
}

fn download_with(
    version: &str,
    platform: UpdatePlatform,
    architecture: &str,
    mut fetch: impl FnMut(&str, &Path, u64) -> Result<()>,
) -> Result<DownloadedRelease> {
    let version = parse_update_version(version)?;
    let (target, extension) = direct_target(platform, architecture)?;
    let archive_name = format!("workdeck-{target}.{extension}");
    let directory = tempfile::Builder::new()
        .prefix("workdeck-download-")
        .tempdir()?;
    let base = format!("https://github.com/ruttydm/workdeck/releases/download/v{version}");
    for (name, limit) in [
        (&archive_name, 2 * 1024 * 1024 * 1024),
        (&format!("{archive_name}.sha256"), 1024 * 1024),
    ] {
        fetch(
            &format!("{base}/{name}"),
            &directory.path().join(name),
            limit,
        )?;
    }
    Ok(DownloadedRelease {
        directory,
        archive_name,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downloads_use_packaged_names_and_cleanup_on_success_or_failure() {
        for fail_checksum in [false, true] {
            let mut paths = Vec::new();
            let result = download_with(
                "v1.2.3",
                UpdatePlatform::Macos,
                "aarch64",
                |url, path, limit| {
                    let name = path.file_name().unwrap().to_str().unwrap();
                    assert_eq!(
                        url,
                        format!(
                            "https://github.com/ruttydm/workdeck/releases/download/v1.2.3/{name}"
                        )
                    );
                    assert!(name.starts_with("workdeck-aarch64-apple-darwin.tar.gz"));
                    paths.push(path.to_owned());
                    assert_eq!(
                        limit,
                        if name.ends_with("sha256") {
                            1024 * 1024
                        } else {
                            2 * 1024 * 1024 * 1024
                        }
                    );
                    if fail_checksum && name.ends_with("sha256") {
                        anyhow::bail!("network failure");
                    }
                    std::fs::write(path, name)?;
                    Ok(())
                },
            );
            assert_eq!(paths.len(), 2);
            if fail_checksum {
                assert!(result.is_err());
            } else {
                let downloaded = result.unwrap();
                assert_eq!(downloaded.archive(), paths[0]);
                assert_eq!(downloaded.checksums(), paths[1]);
                assert!(downloaded.archive().is_file());
                drop(downloaded);
            }
            assert!(!paths[0].parent().unwrap().exists());
        }
    }

    #[test]
    fn download_limits_are_bounded_even_without_content_length() {
        let mut output = Vec::new();
        assert!(copy_bounded(std::io::repeat(0), &mut output, 8).is_err());
        assert_eq!(output.len(), 9);
        assert!(copy_bounded(b"123".as_slice(), &mut Vec::new(), 3).is_ok());
    }
}
