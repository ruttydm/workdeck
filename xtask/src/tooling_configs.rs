//! Verify configuration-only Hunk artifacts have explicit native replacements.
//!
//! These checks deliberately read the pinned blobs through Git. They do not copy or execute
//! the JavaScript toolchain and fail if an upstream configuration changes shape unexpectedly.

use anyhow::{Result, ensure};
use std::path::Path;

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const MIGRATION_DOC: &str = "port/hunk/tooling-config-migrations.md";

const CONFIGS: &[(&str, usize, &[&str])] = &[
    ("CLAUDE.md", 9, &["docs/ARCHITECTURE.md", MIGRATION_DOC]),
    (".oxfmtrc.json", 27, &["rust-toolchain.toml", MIGRATION_DOC]),
    (".oxlintrc.json", 51, &["Cargo.toml", MIGRATION_DOC]),
    (
        ".env.test",
        56,
        &["xtask/src/main.rs", "docs/test-git-isolation.md"],
    ),
    (
        "website/tsconfig.json",
        42,
        &["site/config.toml", MIGRATION_DOC],
    ),
    (
        "website/.gitignore",
        85,
        &[".gitignore", "xtask/src/site_preview.rs", MIGRATION_DOC],
    ),
    (
        "bunfig.toml",
        92,
        &["Cargo.lock", "rust-toolchain.toml", MIGRATION_DOC],
    ),
];

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    let migration = std::fs::read_to_string(repo.join(MIGRATION_DOC))?;
    for (path, expected_bytes, destinations) in CONFIGS {
        let bytes = crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{path}")])?;
        ensure!(
            bytes.len() == *expected_bytes,
            "pinned {path} changed size: {} != {expected_bytes}",
            bytes.len()
        );
        let source = std::str::from_utf8(&bytes)?;
        match *path {
            "CLAUDE.md" => ensure!(source == "AGENTS.md", "CLAUDE.md pointer changed"),
            ".oxfmtrc.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value == serde_json::json!({"ignorePatterns": []}),
                    "Oxfmt configuration changed"
                );
            }
            ".oxlintrc.json" => {
                let value: serde_json::Value = serde_json::from_str(source)?;
                ensure!(
                    value["rules"]["no-control-regex"].as_str() == Some("off"),
                    "Oxlint control-regex rule changed"
                );
            }
            ".env.test" => ensure!(
                source == "GIT_CONFIG_GLOBAL=/dev/null\nGIT_CONFIG_SYSTEM=/dev/null\n",
                "test Git isolation configuration changed"
            ),
            "website/tsconfig.json" => ensure!(
                source == "{\n  \"extends\": \"astro/tsconfigs/strict\"\n}\n",
                "Astro TypeScript configuration changed"
            ),
            "website/.gitignore" => ensure!(
                source.lines().collect::<Vec<_>>()
                    == [
                        "node_modules/",
                        "dist/",
                        ".astro/",
                        ".pagefind/",
                        "test-results/",
                        "playwright-report/",
                        "blob-report/",
                    ],
                "website ignore policy changed"
            ),
            "bunfig.toml" => ensure!(
                source
                    == "[install]\n# Only install packages published at least 7 days ago.\nminimumReleaseAge = 604800\n",
                "Bun install policy changed"
            ),
            other => ensure!(false, "unknown configuration {other}"),
        }
        ensure!(
            migration.contains(&format!("| `{path}` |")),
            "{path} is missing a dedicated migration entry"
        );
        for destination in *destinations {
            ensure!(
                repo.join(destination).exists(),
                "native replacement for {path} is missing: {destination}"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_configuration_blobs_have_explicit_native_replacements() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        verify(repo, BASELINE).unwrap();
    }
}
