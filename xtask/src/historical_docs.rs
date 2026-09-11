//! Verify historical Markdown documents that were translated to the native tree.
//!
//! These documents are not executable source mirrors.  The verifier pins each
//! source blob through Git, checks its complete heading surface, and requires a
//! checked-in native destination and migration record.

use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const MIGRATION_DOC: &str = "port/hunk/historical-document-migrations.md";

struct DocSpec {
    source: &'static str,
    native: &'static str,
    bytes: usize,
    sha256: &'static str,
}

const DOCS: &[DocSpec] = &[DocSpec {
    source: "docs/extension-api-evaluation.md",
    native: "docs/extension-api-evaluation.md",
    bytes: 5068,
    sha256: "2b777c623572ba0bc08b46ab7894fa324e20653be71257f2d9a44f1426136ff4",
}];

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))
        .with_context(|| format!("read {MIGRATION_DOC}"))?;
    for spec in DOCS {
        let source =
            crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{}", spec.source)])?;
        ensure!(
            source.len() == spec.bytes,
            "pinned {} changed size",
            spec.source
        );
        ensure!(
            format!("{:x}", Sha256::digest(&source)) == spec.sha256,
            "pinned {} hash changed",
            spec.source
        );
        let source = std::str::from_utf8(&source)?;
        let native = std::fs::read_to_string(repo.join(spec.native))
            .with_context(|| format!("read {}", spec.native))?;
        ensure!(
            repo.join(spec.native).is_file(),
            "native page is missing: {}",
            spec.native
        );
        ensure!(
            migration.contains(&format!("`{}`", spec.source)),
            "{} is missing a migration entry",
            spec.source
        );
        for heading in headings(source) {
            ensure!(
                headings(&native)
                    .iter()
                    .any(|candidate| normalize(candidate) == normalize(heading)),
                "{} heading is missing from {}: {}",
                spec.source,
                spec.native,
                heading
            );
        }
    }
    Ok(())
}

fn headings(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix('#')?;
            let rest = rest.trim_start_matches('#').trim();
            (!rest.is_empty()).then_some(rest)
        })
        .collect()
}

fn normalize(value: &str) -> String {
    value
        .to_ascii_lowercase()
        .replace("hunk", "workdeck")
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_historical_documents_have_complete_native_heading_surfaces() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        verify(repo, BASELINE).unwrap();
    }
}
