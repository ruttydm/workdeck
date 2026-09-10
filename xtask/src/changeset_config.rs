//! Verify the native replacement for Hunk's Changesets configuration artifacts.
//!
//! The pinned files are read through Git so the repository never needs to retain
//! an executable JavaScript configuration mirror.  Each source artifact has a
//! separate shape check and a documented native owner.

use anyhow::{Result, ensure};
use serde_json::Value;
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const MIGRATION_DOC: &str = "port/hunk/changeset-config-migration.md";

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))?;
    verify_readme(repo, &migration)?;
    verify_config(repo, &migration)?;
    verify_prerelease_state(repo, &migration)?;
    Ok(())
}

fn pinned(repo: &Path, path: &str) -> Result<Vec<u8>> {
    crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{path}")])
}

fn verify_readme(repo: &Path, migration: &str) -> Result<()> {
    let bytes = pinned(repo, ".changeset/README.md")?;
    ensure!(bytes.len() == 1454, "pinned Changesets README changed size");
    let source = std::str::from_utf8(&bytes)?;
    for marker in [
        "# Changesets",
        "release-note fragments",
        "bun run changeset",
        "release:version",
        "Homebrew",
    ] {
        ensure!(
            source.contains(marker),
            "Changesets README lost marker {marker}"
        );
    }
    ensure!(
        migration.contains("`.changeset/README.md`")
            && migration.contains("docs/native-release-fragments.md"),
        "Changesets README migration is undocumented"
    );
    ensure!(
        repo.join("docs/native-release-fragments.md").is_file(),
        "native release-fragment guide is missing"
    );
    Ok(())
}

fn verify_config(repo: &Path, migration: &str) -> Result<()> {
    let bytes = pinned(repo, ".changeset/config.json")?;
    ensure!(bytes.len() == 485, "pinned Changesets config changed size");
    let value: Value = serde_json::from_slice(&bytes)?;
    ensure!(
        value["$schema"] == "https://unpkg.com/@changesets/config@3.1.4/schema.json",
        "Changesets config schema changed"
    );
    ensure!(
        value["changelog"]
            == serde_json::json!([
                "@changesets/changelog-github",
                {"repo": "modem-dev/hunk", "disableThanks": true}
            ]),
        "Changesets changelog integration changed"
    );
    ensure!(value["commit"] == false, "Changesets commit policy changed");
    ensure!(
        value["fixed"] == serde_json::json!([]),
        "Changesets fixed policy changed"
    );
    ensure!(
        value["linked"] == serde_json::json!([]),
        "Changesets linked policy changed"
    );
    ensure!(
        value["access"] == "public",
        "Changesets access policy changed"
    );
    ensure!(
        value["baseBranch"] == "main",
        "Changesets base branch changed"
    );
    ensure!(
        value["updateInternalDependencies"] == "patch",
        "Changesets dependency bump policy changed"
    );
    ensure!(
        value["ignore"]
            == serde_json::json!([
                "@hunk/session-broker",
                "@hunk/session-broker-bun",
                "@hunk/session-broker-core",
                "@hunk/session-broker-node"
            ]),
        "Changesets ignored packages changed"
    );
    ensure!(
        migration.contains("`.changeset/config.json`")
            && migration.contains("Cargo workspace")
            && migration.contains("release/fragments"),
        "Changesets config migration is undocumented"
    );
    ensure!(repo.join("xtask/src/changelog/fragments.rs").is_file());
    Ok(())
}

fn verify_prerelease_state(repo: &Path, migration: &str) -> Result<()> {
    let bytes = pinned(repo, ".changeset/pre.json")?;
    ensure!(
        bytes.len() == 2336,
        "pinned Changesets prerelease state changed size"
    );
    let value: Value = serde_json::from_slice(&bytes)?;
    ensure!(value["mode"] == "pre", "Changesets prerelease mode changed");
    ensure!(value["tag"] == "beta", "Changesets prerelease tag changed");
    let versions = value["initialVersions"]
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Changesets initialVersions is not an object"))?;
    ensure!(
        versions.len() == 6,
        "Changesets initial version count changed"
    );
    ensure!(
        versions.get("hunkdiff") == Some(&Value::String("0.20.1".into())),
        "pinned Hunk prerelease version changed"
    );
    ensure!(
        value["changesets"]
            .as_array()
            .is_some_and(|items| items.len() == 68),
        "Changesets prerelease fragment count changed"
    );
    ensure!(
        migration.contains("`.changeset/pre.json`")
            && migration.contains("release/prerelease.json")
            && migration.contains("native prerelease validator"),
        "Changesets prerelease migration is undocumented"
    );
    ensure!(repo.join("xtask/src/release_notes.rs").is_file());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_changesets_artifacts_have_native_replacements() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        verify(repo, BASELINE).unwrap();
    }
}
