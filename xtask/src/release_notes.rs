//! MIT translation of Hunk's scripts/verify-pr-release-notes.ts.
//! Cargo metadata and the native fragment gate replace the package/Changesets runtime.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::cmp::Ordering;
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

const PRE_STATE: &str = "release/prerelease.json";

fn generated_release_path(path: &str) -> bool {
    let leaf = |prefix: &str, suffix: &str| {
        path.strip_prefix(prefix)
            .and_then(|tail| tail.strip_suffix(suffix))
            .is_some_and(|name| !name.is_empty() && !name.contains('/'))
    };
    matches!(
        path,
        PRE_STATE | "CHANGELOG.md" | "Cargo.toml" | "Cargo.lock" | "crates/workdeck-cli/Cargo.toml"
    ) || leaf("release/fragments/", ".md")
        || leaf("benchmarks/release/bench-", ".json")
}

fn generated_preparation(paths: &[String]) -> bool {
    paths.iter().any(|path| path == PRE_STATE)
        && paths.iter().all(|path| generated_release_path(path))
}

fn changed_paths(repo: &Path, base: &str, head: &str) -> Result<Vec<String>> {
    let args = [
        "diff",
        "--name-only",
        "--no-renames",
        "-z",
        base,
        head,
        "--",
    ];
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .stderr(Stdio::inherit())
        .output()?;
    if !output.status.success() {
        bail!(
            "git {} failed with exit {}",
            args.join(" "),
            output.status.code().unwrap_or(-1)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect())
}

#[derive(Debug, PartialEq, Eq)]
enum Verification {
    ChangesetStatus,
    GeneratedPrerelease,
}

fn verify_pr_with_status(
    repo: &Path,
    base: &str,
    head: &str,
    status: impl FnOnce(&str) -> Result<()>,
) -> Result<Verification> {
    let paths = changed_paths(repo, base, head)?;
    if !generated_preparation(&paths) || !repo.join(PRE_STATE).exists() {
        status(base)?;
        return Ok(Verification::ChangesetStatus);
    }
    validate_local(repo, std::iter::empty())?;
    Ok(Verification::GeneratedPrerelease)
}

pub(super) fn verify_pr(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    let base = args.next().unwrap_or_default();
    let head = args.next().unwrap_or_else(|| "HEAD".into());
    if base.is_empty() || base.starts_with('-') || head.starts_with('-') {
        bail!("Usage: cargo xtask release verify-pr-notes <base-revision> [head-revision]");
    }
    verify_pr_with_status(repo, &base, &head, |base| {
        super::release_status::run(repo, [format!("--since={base}")].into_iter())
    })?;
    Ok(())
}

/// Verify the native pull-request workflow replacing the pinned Bun/Node
/// matrix. Release-note status, Windows compatibility, compiled portability,
/// and full validation remain explicit Rust jobs in the final workflow.
pub(crate) fn verify_pr_workflow(repo: &Path) -> Result<()> {
    const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
    let source = crate::git_stdout_bytes(
        repo,
        ["show", &format!("{BASELINE}:.github/workflows/pr-ci.yml")],
    )?;
    anyhow::ensure!(
        source.len() == 7_272,
        "pinned PR CI workflow changed size: {} != 7272",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "name: CI",
        "pull_request:",
        "SKIP_INSTALL_SIMPLE_GIT_HOOKS",
        "pr-ci-${{ github.workflow }}-${{ github.ref }}",
        "changes:",
        "changeset-status:",
        "windows-compat:",
        "compiled-headless-portability:",
        "pr-validate:",
        "Set up Bun",
        "Set up Node",
        "Install Jujutsu",
        "Install Sapling",
        "bun install --frozen-lockfile",
        "bun run format:check",
        "bun run lint",
        "bun run typecheck",
        "bun run deps:check",
        "bun run test:theme-contrast",
        "bun run test",
        "bun run test:session-broker-node",
        "bun run test:integration",
        "bun run test:tty-smoke",
        "bun run build:npm",
        "bun run build:prebuilt:npm",
        "bun run build:bin",
        "Compiled headless portability",
        "Verify compiled headless commands skip OpenTUI",
        "Verify compiled binary watch mode",
    ] {
        anyhow::ensure!(
            source.contains(marker),
            "pinned PR CI workflow lost marker {marker:?}"
        );
    }
    let native = std::fs::read_to_string(repo.join(".github/workflows/pr-ci.yml"))?;
    for marker in [
        "name: CI",
        "permissions:",
        "contents: read",
        "pr-ci-${{ github.workflow }}-${{ github.ref }}",
        "changes:",
        "cargo xtask ci-changes \"$BASE_SHA\" \"$HEAD_SHA\"",
        "changeset-status:",
        "cargo xtask release verify-pr-notes",
        "windows-compat:",
        "cargo test --locked --workspace --all-targets",
        "cargo clippy --locked --workspace --all-targets -- -D warnings",
        "compiled-headless-portability:",
        "cargo build --locked --release --target",
        "cargo test --locked -p workdeck-cli --test cli --test terminal_lifecycle",
        "pr-validate:",
        "cargo xtask verify",
        "cargo xtask site check",
        "cargo xtask test",
        "target/release/workdeck --help",
    ] {
        anyhow::ensure!(
            native.contains(marker),
            "native PR CI workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        anyhow::ensure!(
            !pattern.is_match(&native),
            "native PR CI workflow retains forbidden runtime token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/pr-ci-migration.md"))?;
    for marker in [
        "pr-ci.yml",
        "release-note",
        "Windows",
        "portability",
        "native validation",
        "npm",
        "not retained",
    ] {
        anyhow::ensure!(
            migration.contains(marker),
            "PR CI migration is missing {marker:?}"
        );
    }
    Ok(())
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn stable_parts(value: &str) -> Option<[f64; 3]> {
    let parts = value.split('.').collect::<Vec<_>>();
    if parts.len() != 3
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()))
    {
        return None;
    }
    Some([
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ])
}

fn latest_stable(changelog: &str) -> Option<&str> {
    let mut versions = changelog
        .split(['\r', '\n', '\u{2028}', '\u{2029}'])
        .filter_map(|line| {
            let raw = line.strip_prefix("## ")?;
            Some((raw, stable_parts(raw)?))
        })
        .collect::<Vec<_>>();
    versions.sort_by(|(_, left), (_, right)| {
        for index in 0..3 {
            let difference = right[index] - left[index];
            if difference != 0.0 {
                return difference.partial_cmp(&0.0).unwrap_or(Ordering::Equal);
            }
        }
        Ordering::Equal
    });
    versions.first().map(|(raw, _)| *raw)
}

fn validate_generated(
    package: &str,
    version: &str,
    pre: &Value,
    changelog: &str,
    on_disk: &BTreeSet<String>,
) -> Result<()> {
    if pre.get("mode").and_then(Value::as_str) != Some("pre") {
        bail!(
            "Expected release pre mode, received {}",
            pre.get("mode")
                .map(Value::to_string)
                .unwrap_or_else(|| "undefined".into())
        );
    }
    let tag = pre
        .get("tag")
        .and_then(Value::as_str)
        .filter(|tag| valid_identifier(tag))
        .context("Release prerelease tag must be a non-empty channel tag")?;
    let initial = pre
        .get("initialVersions")
        .filter(|value| value.is_object() || value.is_array())
        .context("Release prerelease state is missing initialVersions")?;
    let initial_version = initial
        .get(package)
        .and_then(Value::as_str)
        .filter(|value| stable_parts(value).is_some())
        .with_context(|| format!("Missing stable initial version for package {package}"))?;
    let latest =
        latest_stable(changelog).context("Changelog does not contain a stable release heading")?;
    if initial_version != latest {
        bail!("Initial version {initial_version} does not match latest stable release {latest}");
    }
    let pattern = regex::Regex::new(&format!(
        r"^[0-9]+\.[0-9]+\.[0-9]+-{}\.[0-9]+$",
        regex::escape(tag)
    ))?;
    if !pattern.is_match(version) {
        bail!("Package version {version} does not match prerelease tag {tag}");
    }
    let changesets = pre
        .get("changesets")
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
        .context("Release prerelease state must record at least one consumed changeset")?;
    let ids = changesets
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| valid_identifier(id))
        .collect::<Vec<_>>();
    if ids.len() != changesets.len()
        || ids.iter().copied().collect::<BTreeSet<_>>().len() != ids.len()
    {
        bail!("Release prerelease state contains invalid or duplicate changeset IDs");
    }
    let missing = ids
        .iter()
        .filter(|id| !on_disk.contains(**id))
        .copied()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        bail!("Missing consumed changeset files: {}", missing.join(", "));
    }
    let heading = format!("## {version}");
    let has_heading = changelog.split_inclusive('\n').any(|line| {
        let line = if let Some(line) = line.strip_suffix('\n') {
            line.strip_suffix('\r').unwrap_or(line)
        } else {
            line
        };
        line == heading
    });
    if !has_heading {
        bail!("Changelog is missing the {version} release heading");
    }
    Ok(())
}

pub(super) fn validate_local(repo: &Path, mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("release validate-prerelease accepts no arguments");
    }
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(repo.join("Cargo.toml"))
        .no_deps()
        .exec()?;
    let package = metadata
        .packages
        .iter()
        .find(|package| package.name.as_str() == "workdeck-cli")
        .context("workspace does not contain workdeck-cli")?;
    let pre = serde_json::from_slice(&fs::read(repo.join(PRE_STATE))?)?;
    let changelog = fs::read_to_string(repo.join("CHANGELOG.md"))?;
    let mut on_disk = BTreeSet::new();
    for entry in fs::read_dir(repo.join("release/fragments"))? {
        let entry = entry?;
        if entry.file_type()?.is_file()
            && let Some(name) = entry.file_name().to_str()
            && let Some(id) = name.strip_suffix(".md")
        {
            on_disk.insert(id.to_owned());
        }
    }
    validate_generated(
        "workdeck-cli",
        &package.version.to_string(),
        &pre,
        &changelog,
        &on_disk,
    )?;
    println!(
        "Validated generated prerelease notes for {}.",
        package.version
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_pr_workflow_replaces_the_complete_bun_validation_matrix() {
        let repo = super::super::repo_root().unwrap();
        super::verify_pr_workflow(&repo).unwrap();
    }

    fn generated_paths() -> Vec<String> {
        [
            PRE_STATE,
            "release/fragments/old-fix.md",
            "CHANGELOG.md",
            "Cargo.toml",
            "benchmarks/release/bench-0.18.0-beta.0.json",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    #[test]
    fn accepts_only_generated_prerelease_metadata_paths() {
        for path in generated_paths() {
            assert!(generated_release_path(&path));
        }
        for path in [
            "src/main.rs",
            "benchmarks/run.rs",
            "bun.lock",
            "release/fragments/nested/fix.md",
            "benchmarks/release/bench-.json",
        ] {
            assert!(!generated_release_path(path), "{path}");
        }
        assert!(generated_release_path("crates/workdeck-cli/Cargo.toml"));
        assert!(generated_release_path("Cargo.lock"));
    }

    #[test]
    fn selects_a_metadata_only_diff_that_changes_prerelease_state() {
        assert!(generated_preparation(&generated_paths()));
    }

    #[test]
    fn ordinary_changesets_stay_on_the_standard_status_path() {
        assert!(!generated_preparation(&[
            "src/main.rs".into(),
            "release/fragments/fix.md".into()
        ]));
        assert!(!generated_preparation(&[
            "CHANGELOG.md".into(),
            "Cargo.toml".into()
        ]));
        assert!(!generated_preparation(&[]));
    }

    #[test]
    fn release_preparation_mixed_with_source_changes_is_not_exempt() {
        let mut paths = generated_paths();
        paths.push("src/main.rs".into());
        assert!(!generated_preparation(&paths));
    }

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn manifest(root: &Path, version: &str) {
        fs::write(root.join("Cargo.toml"), format!("[package]\nname = \"workdeck-cli\"\nversion = \"{version}\"\nedition = \"2024\"\npublish = false\n")).unwrap();
    }

    fn repository() -> (tempfile::TempDir, String) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir_all(root.path().join("release/fragments")).unwrap();
        manifest(root.path(), "0.17.7");
        fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(root.path().join("CHANGELOG.md"), "# Changelog\n").unwrap();
        fs::write(
            root.path().join("release/fragments/new-feature.md"),
            "---\n---\n",
        )
        .unwrap();
        git(root.path(), &["init", "-q"]);
        git(
            root.path(),
            &["config", "user.email", "test@example.invalid"],
        );
        git(
            root.path(),
            &["config", "user.name", "Workdeck Release Test"],
        );
        git(root.path(), &["config", "commit.gpgsign", "false"]);
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "base"]);
        let base = git(root.path(), &["rev-parse", "HEAD"]);
        (root, base)
    }

    fn generated_prerelease(root: &Path, initial: &str) {
        manifest(root, "0.18.0-beta.0");
        fs::write(root.join(PRE_STATE), serde_json::to_vec_pretty(&serde_json::json!({"mode":"pre","tag":"beta","initialVersions":{"workdeck-cli":initial},"changesets":["new-feature"]})).unwrap()).unwrap();
        fs::write(
            root.join("CHANGELOG.md"),
            "# Changelog\n\n## 0.18.0-beta.0\n\n## 0.17.7\n",
        )
        .unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-qm", "prepare prerelease"]);
    }

    #[test]
    fn ordinary_diffs_route_to_status_with_the_exact_base_revision() {
        let (root, base) = repository();
        fs::write(
            root.path().join("source.rs"),
            "pub const CHANGED: bool = true;\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "ordinary change"]);
        let mut calls = Vec::new();
        let result = verify_pr_with_status(root.path(), &base, "HEAD", |base| {
            calls.push(format!("--since={base}"));
            Ok(())
        })
        .unwrap();
        assert_eq!(result, Verification::ChangesetStatus);
        assert_eq!(calls, [format!("--since={base}")]);
    }

    #[test]
    fn metadata_only_prerelease_output_routes_to_generated_validation() {
        let (root, base) = repository();
        generated_prerelease(root.path(), "0.17.7");
        let result = verify_pr_with_status(root.path(), &base, "HEAD", |_| {
            panic!("ordinary status must not run")
        })
        .unwrap();
        assert_eq!(result, Verification::GeneratedPrerelease);
        assert_eq!(git(root.path(), &["status", "--porcelain"]), "");
    }

    #[test]
    fn stable_promotion_removing_prerelease_state_routes_to_status() {
        let (root, _) = repository();
        generated_prerelease(root.path(), "0.17.7");
        let base = git(root.path(), &["rev-parse", "HEAD"]);
        manifest(root.path(), "0.18.0");
        fs::write(
            root.path().join("CHANGELOG.md"),
            "# Changelog\n\n## 0.18.0\n\n## 0.17.7\n",
        )
        .unwrap();
        fs::remove_file(root.path().join(PRE_STATE)).unwrap();
        fs::remove_file(root.path().join("release/fragments/new-feature.md")).unwrap();
        fs::write(
            root.path().join("release/fragments/stable-release.md"),
            "---\n---\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "prepare stable release"]);
        let mut calls = Vec::new();
        let result = verify_pr_with_status(root.path(), &base, "HEAD", |base| {
            calls.push(format!("--since={base}"));
            Ok(())
        })
        .unwrap();
        assert_eq!(result, Verification::ChangesetStatus);
        assert_eq!(calls, [format!("--since={base}")]);
        verify_pr(root.path(), [base].into_iter()).unwrap();
    }

    #[test]
    fn generated_pr_rejects_an_initial_version_older_than_its_stable_changelog() {
        let (root, base) = repository();
        generated_prerelease(root.path(), "0.17.6");
        let error = verify_pr_with_status(root.path(), &base, "HEAD", |_| {
            panic!("invalid generated state must not fall back to ordinary status")
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Initial version 0.17.6 does not match latest stable release 0.17.7")
        );
    }

    #[test]
    fn cli_routes_real_status_and_rejects_option_like_revisions() {
        let (root, base) = repository();
        fs::write(
            root.path().join("src/main.rs"),
            "fn main() { println!(\"changed\"); }\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "ordinary change"]);
        assert!(verify_pr(root.path(), [base.clone()].into_iter()).is_err());
        fs::write(
            root.path().join("release/fragments/maintenance.md"),
            "---\n---\n",
        )
        .unwrap();
        git(root.path(), &["add", "."]);
        git(root.path(), &["commit", "-qm", "maintenance note"]);
        verify_pr(root.path(), [base.clone()].into_iter()).unwrap();
        for args in [vec![], vec!["--help".into()], vec![base, "--help".into()]] {
            assert!(
                verify_pr(root.path(), args.into_iter())
                    .unwrap_err()
                    .to_string()
                    .contains("Usage:")
            );
        }
    }

    fn input() -> (Value, String, BTreeSet<String>) {
        (
            serde_json::json!({"mode":"pre","tag":"beta","initialVersions":{"workdeck-cli":"0.17.7"},"changesets":["new-feature","old-fix"]}),
            "# Changelog\n\n## 0.18.0-beta.0\n\n- Added a feature.\n\n## 0.17.7\n".into(),
            ["new-feature".into(), "old-fix".into()]
                .into_iter()
                .collect(),
        )
    }

    fn check(pre: &Value, changelog: &str, on_disk: &BTreeSet<String>) -> Result<()> {
        validate_generated("workdeck-cli", "0.18.0-beta.0", pre, changelog, on_disk)
    }

    #[test]
    fn accepts_coherent_generated_prerelease_state() {
        let (pre, changelog, on_disk) = input();
        check(&pre, &changelog, &on_disk).unwrap();
    }

    #[test]
    fn requires_package_version_and_tag_agreement() {
        let (pre, changelog, on_disk) = input();
        assert!(
            validate_generated("workdeck-cli", "0.18.0-next.0", &pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("does not match prerelease tag beta")
        );
    }

    #[test]
    fn requires_a_stable_initial_package_version() {
        let (mut pre, changelog, on_disk) = input();
        pre["initialVersions"]["workdeck-cli"] = "0.17.7-beta.1".into();
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("Missing stable initial version for package workdeck-cli")
        );
    }

    #[test]
    fn requires_the_latest_stable_changelog_version() {
        let (mut pre, mut changelog, on_disk) = input();
        pre["initialVersions"]["workdeck-cli"] = "0.17.6".into();
        changelog.push_str("\n## 0.17.6\n");
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("Initial version 0.17.6 does not match latest stable release 0.17.7")
        );
    }

    #[test]
    fn requires_every_consumed_changeset_to_remain_on_disk() {
        let (pre, changelog, mut on_disk) = input();
        on_disk.remove("old-fix");
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("Missing consumed changeset files: old-fix")
        );
    }

    #[test]
    fn rejects_duplicate_changeset_ids() {
        let (mut pre, changelog, on_disk) = input();
        pre["changesets"] = serde_json::json!(["old-fix", "old-fix"]);
        assert!(
            check(&pre, &changelog, &on_disk)
                .unwrap_err()
                .to_string()
                .contains("invalid or duplicate changeset IDs")
        );
    }

    #[test]
    fn requires_the_exact_package_version_heading() {
        let (pre, _, on_disk) = input();
        assert!(
            check(
                &pre,
                "# Changelog\n\n## 0.18.0-beta.1\n\n## 0.17.7\n",
                &on_disk
            )
            .unwrap_err()
            .to_string()
            .contains("Changelog is missing the 0.18.0-beta.0 release heading")
        );
    }

    #[test]
    fn malformed_prerelease_metadata_fails_before_release_validation() {
        let (pre, changelog, on_disk) = input();
        for (key, value, expected) in [
            (
                "mode",
                serde_json::json!("exit"),
                "Expected release pre mode",
            ),
            ("tag", serde_json::json!("beta.1"), "non-empty channel tag"),
            ("tag", serde_json::Value::Null, "non-empty channel tag"),
            (
                "initialVersions",
                serde_json::Value::Null,
                "missing initialVersions",
            ),
            ("changesets", serde_json::json!([]), "at least one consumed"),
            ("changesets", serde_json::json!([1]), "invalid or duplicate"),
            (
                "changesets",
                serde_json::json!(["../escape"]),
                "invalid or duplicate",
            ),
        ] {
            let mut altered = pre.clone();
            altered[key] = value;
            assert!(
                check(&altered, &changelog, &on_disk)
                    .unwrap_err()
                    .to_string()
                    .contains(expected)
            );
        }
        assert!(
            check(&pre, "## 0.18.0-beta.0\n", &on_disk)
                .unwrap_err()
                .to_string()
                .contains("does not contain a stable release")
        );
    }

    #[test]
    fn stable_headings_use_numeric_order_and_preserve_source_line_terminators() {
        assert_eq!(
            latest_stable("## 0.9.9\r\n## 0.10.0\n## 0.8.0\u{2028}## 0.11.0"),
            Some("0.11.0")
        );
        assert_eq!(
            latest_stable("## 0.17.7-beta.1\n## 00.17.7\n## 0.17.7"),
            Some("00.17.7")
        );
        let (pre, changelog, on_disk) = input();
        check(&pre, &changelog.replace('\n', "\r\n"), &on_disk).unwrap();
        assert!(check(&pre, "## 0.17.7\n## 0.18.0-beta.0\r", &on_disk).is_err());
    }

    #[test]
    fn local_validator_reads_a_real_cargo_prerelease_without_creating_repository_state() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("src")).unwrap();
        fs::create_dir_all(root.path().join("release/fragments")).unwrap();
        fs::write(root.path().join("Cargo.toml"), "[package]\nname = \"workdeck-cli\"\nversion = \"0.18.0-beta.0\"\nedition = \"2024\"\npublish = false\n").unwrap();
        fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
        let (pre, changelog, ids) = input();
        fs::write(
            root.path().join("release/prerelease.json"),
            serde_json::to_vec(&pre).unwrap(),
        )
        .unwrap();
        fs::write(root.path().join("CHANGELOG.md"), changelog).unwrap();
        for id in ids {
            fs::write(
                root.path().join(format!("release/fragments/{id}.md")),
                "---\n---\n",
            )
            .unwrap();
        }
        validate_local(root.path(), std::iter::empty()).unwrap();
        assert!(!root.path().join(".agents").exists());
        assert!(!root.path().join("Cargo.lock").exists());
        assert!(!root.path().join("target").exists());
        fs::remove_file(root.path().join("release/fragments/old-fix.md")).unwrap();
        assert!(
            validate_local(root.path(), std::iter::empty())
                .unwrap_err()
                .to_string()
                .contains("Missing consumed changeset files: old-fix")
        );
        assert!(validate_local(root.path(), ["unexpected".into()].into_iter()).is_err());
    }
}
