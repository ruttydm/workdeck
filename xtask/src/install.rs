//! Tooling entry points delegate to the native product implementation.
pub(super) use workdeck_cli::install::{inspect, run, stage, verify};

/// Verify the native archive-installation workflow replacing Hunk's POSIX
/// install.sh E2E. The source workflow is read from Git; its download, PATH,
/// idempotency, custom-directory, and failure cases are covered by native
/// transaction/oracle tests without retaining a shell runtime.
pub(crate) fn verify_workflow(repo: &std::path::Path) -> anyhow::Result<()> {
    const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:.github/workflows/install-sh-e2e.yml"),
        ],
    )?;
    anyhow::ensure!(
        source.len() == 3_914,
        "pinned install-sh-e2e workflow changed size: {} != 3914",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "name: Install script E2E",
        "pull_request:",
        "install.sh",
        "schedule:",
        "workflow_dispatch",
        "permissions:",
        "contents: read",
        "install-e2e:",
        "ubuntu-latest",
        "macos-latest",
        "Resolve the newest published release",
        "gh release view",
        "HUNK_VERSION",
        "Install into a sandbox home",
        "GITHUB_PATH",
        "skill path hunk-review",
        "Re-run is idempotent",
        "Custom directory with spaces installs and resolves skills",
        "Unknown version fails loudly",
        "Could not download",
    ] {
        anyhow::ensure!(
            source.contains(marker),
            "pinned install-sh-e2e workflow lost marker {marker:?}"
        );
    }
    let native = std::fs::read_to_string(repo.join(".github/workflows/install-sh-e2e.yml"))?;
    for marker in [
        "name: Install archive E2E",
        "crates/workdeck-cli/src/install/**",
        "Native installation E2E",
        "ubuntu-latest",
        "macos-latest",
        "windows-latest",
        "cargo test --locked -p workdeck-cli install::",
        "cargo test --locked -p xtask install_oracle",
        "cargo xtask install-plan --no-modify-path",
        "cargo build --locked --release --package workdeck-cli --bin workdeck",
        "target/release/workdeck --version",
    ] {
        anyhow::ensure!(
            native.contains(marker),
            "native install workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm", "install.sh"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        anyhow::ensure!(
            !pattern.is_match(&native),
            "native install workflow retains forbidden token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/install-workflow-migration.md"))?;
    for marker in [
        "install-sh-e2e.yml",
        "native archive",
        "idempotency",
        "custom directory",
        "read-only installation plan",
        "no shell runtime",
    ] {
        anyhow::ensure!(
            migration.contains(marker),
            "install workflow migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Explicit first-install command; unlike inspection/staging this publishes a root.
pub(super) fn create(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    let args: Vec<_> = args.collect();
    anyhow::ensure!(
        args.len() == 6,
        "install-create requires ARCHIVE CHECKSUM_FILE DESTINATION TARGET COMMIT TAG_REF"
    );
    let [archive, checksums, destination, target, commit, tag] = args.as_slice() else {
        unreachable!("argument count checked")
    };
    workdeck_cli::install::create_authenticated_installation(
        std::path::Path::new(archive),
        std::path::Path::new(checksums),
        std::path::Path::new(destination),
        target,
        workdeck_cli::install::ReleaseIdentity {
            repository: "ruttydm/workdeck",
            commit,
            tag_ref: tag,
        },
    )?;
    println!(
        "{}",
        serde_json::json!({
            "installed": true,
            "installationRoot": std::path::absolute(destination)?,
            "target": target,
            "sourceCommit": commit,
            "tagRef": tag,
            "archiveSignatureVerified": true,
            "pathModified": false,
        })
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn native_install_workflow_replaces_the_complete_pinned_shell_e2e() {
        let repo = super::super::repo_root().unwrap();
        super::verify_workflow(&repo).unwrap();
    }
}
