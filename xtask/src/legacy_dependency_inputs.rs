//! Verify the dependency manifests that were present in the pinned Hunk tree.
//!
//! Workdeck deliberately does not retain or execute the JavaScript package
//! manager.  The source manifests are therefore represented by a generated
//! inventory containing their exact pinned bytes, hashes, and dependency-graph
//! shape.  Reading the source through Git keeps the oracle available without
//! adding a source mirror to the product tree.

use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const MIGRATION_DOC: &str = "port/hunk/legacy-dependency-inputs.md";
const INVENTORY: &str = "port/hunk/legacy-dependency-inventory.json";

#[derive(Debug, Deserialize)]
struct Inventory {
    schema_version: u32,
    source_commit: String,
    inputs: Vec<Input>,
}

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
    bytes: usize,
    sha256: String,
    lines: usize,
    format: String,
    package_entries: usize,
}

const INPUTS: &[(&str, usize, &str, usize, &str, usize)] = &[
    (
        "bun.lock",
        83627,
        "b2b43bcbac57d09213916d9bec04f883f06e482074dd754fde5b8f8bbd379bd9",
        808,
        "bun-jsonc",
        361,
    ),
    (
        "website/bun.lock",
        149651,
        "c68ef2a2eac3c8474d80991b8df49e7a5b89a55038b3b9219931d94f114c4bfd",
        1193,
        "bun-jsonc",
        584,
    ),
    (
        "test/cli/install-vm/controller-deps/package-lock.json",
        133490,
        "683bae96b7ec6b1c216a95758025c44a9367ab2ad00ac91fb68b8e8f7c334698",
        3628,
        "json",
        316,
    ),
    (
        "package.json",
        7280,
        "4161e385ae7f69a741e457b068ff0be822639b75ceb9d454303015fe1d6f2446",
        184,
        "json",
        0,
    ),
    (
        "website/package.json",
        684,
        "2da7ecf1b1de6996a8a193e9f8fbf6a49930b5a752c6c4fc89dcfb09a14ecb51",
        27,
        "json",
        0,
    ),
    (
        "test/cli/install-vm/controller-deps/package.json",
        135,
        "d43498eafbb820e25623384033c9ed2c04b57b07f165830632b93fd0a98cceca",
        8,
        "json",
        0,
    ),
];

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }

    let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))
        .with_context(|| format!("read {MIGRATION_DOC}"))?;
    let inventory: Inventory = serde_json::from_slice(
        &std::fs::read(repo.join(INVENTORY)).with_context(|| format!("read {INVENTORY}"))?,
    )
    .with_context(|| format!("parse {INVENTORY}"))?;
    ensure!(
        inventory.schema_version == 1,
        "legacy dependency inventory schema changed"
    );
    ensure!(
        inventory.source_commit == BASELINE,
        "legacy dependency inventory pin changed"
    );
    ensure!(
        inventory.inputs.len() == INPUTS.len(),
        "legacy dependency inventory count changed"
    );

    for (path, bytes, expected_sha, lines, format, expected_package_entries) in INPUTS {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{path}")])?;
        ensure!(
            source.len() == *bytes,
            "pinned {path} changed size: {} != {bytes}",
            source.len()
        );
        let sha256 = format!("{:x}", Sha256::digest(&source));
        ensure!(sha256 == *expected_sha, "pinned {path} hash changed");
        ensure!(
            source.iter().filter(|byte| **byte == b'\n').count() == *lines,
            "pinned {path} line count changed"
        );
        ensure!(
            migration.contains(&format!("`{path}`")),
            "{path} is missing a migration entry"
        );

        let record = inventory
            .inputs
            .iter()
            .find(|record| record.path == *path)
            .with_context(|| format!("{path} is missing from {INVENTORY}"))?;
        ensure!(
            record.bytes == *bytes,
            "inventory byte count changed for {path}"
        );
        ensure!(
            record.sha256 == *expected_sha,
            "inventory hash changed for {path}"
        );
        ensure!(
            record.lines == *lines,
            "inventory line count changed for {path}"
        );
        ensure!(
            record.format == *format,
            "inventory format changed for {path}"
        );
        ensure!(
            record.package_entries == *expected_package_entries,
            "inventory package count changed for {path}"
        );

        let actual_entries = package_entries(&source, format)?;
        ensure!(
            actual_entries == *expected_package_entries,
            "pinned {path} package shape changed"
        );
    }
    Ok(())
}

fn package_entries(source: &[u8], format: &str) -> Result<usize> {
    if format == "bun-jsonc" {
        let text = std::str::from_utf8(source)?;
        return Ok(text
            .lines()
            .filter(|line| line.starts_with("    \"") && line.contains("\": ["))
            .count());
    }
    let value: serde_json::Value = serde_json::from_slice(source)?;
    Ok(value
        .get("packages")
        .and_then(serde_json::Value::as_object)
        .map_or(0, serde_json::Map::len))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_dependency_inputs_have_exact_native_inventory() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        verify(repo, BASELINE).unwrap();
    }

    #[test]
    fn bun_jsonc_entry_counter_is_strict_about_package_rows() {
        let source = br#"{
  "packages": {
    "alpha": ["alpha@1.0.0"],
  }
}
"#;
        assert_eq!(package_entries(source, "bun-jsonc").unwrap(), 1);
    }
}
