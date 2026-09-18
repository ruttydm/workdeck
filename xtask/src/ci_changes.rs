//! MIT translation of Hunk .github/scripts/detect-code-changes.sh.
//! Failed Git comparisons fail closed rather than silently skipping CI.

use anyhow::{Context, Result, bail, ensure};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";

/// Verify the complete native replacement for Hunk's code-change detector and
/// its Nix workflow. The source is inspected through Git; no shell mirror is
/// retained or executed by Workdeck.
pub(crate) fn verify_workflow(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:.github/scripts/detect-code-changes.sh"),
        ],
    )?;
    ensure!(
        source.len() == 1_266,
        "pinned detect-code-changes.sh changed size: {} != 1266",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "#!/usr/bin/env bash",
        "set -euo pipefail",
        "base_sha=\"${1:?base commit required}\"",
        "head_sha=\"${2:?head commit required}\"",
        "git hash-object -t tree /dev/null",
        "git cat-file -e",
        "git fetch --no-tags --depth=1 origin",
        "is_docs_only_path()",
        "*.md | docs/* | assets/* | LICENSE",
        "git diff --name-only --no-renames",
        "GITHUB_OUTPUT",
        "Code changes detected; expensive CI jobs should run.",
        "Only docs/assets metadata changes detected; expensive CI jobs can be skipped.",
    ] {
        ensure!(
            source.contains(marker),
            "pinned code-change detector lost marker {marker:?}"
        );
    }

    let native = std::fs::read_to_string(repo.join("xtask/src/ci_changes.rs"))?;
    for marker in [
        "fn validate_revision(",
        "fn ensure_object(",
        "fn docs_only_path(",
        "fn code_changed_in_output(",
        "fn detect(",
        "append_github_output",
        "GITHUB_OUTPUT",
        "port/hunk/oracles/ci-code-changes.json",
    ] {
        ensure!(
            native.contains(marker),
            "native code-change detector is missing {marker:?}"
        );
    }

    let workflow = std::fs::read_to_string(repo.join(".github/workflows/nix.yml"))?;
    for marker in [
        "name: Nix",
        "pull_request:",
        "branches:",
        "group: nix-${{ github.workflow }}-${{ github.ref }}",
        "name: Detect code changes",
        "cargo xtask ci-changes \"$BASE_SHA\" \"$HEAD_SHA\"",
        "outputs:",
        "name: Package",
        "if: needs.changes.outputs.code == 'true'",
        "nix flake check --no-write-lock-file --print-build-logs",
        "nix flake check --all-systems --no-build",
        "nix build .#default --print-build-logs",
        "./result/bin/workdeck --help",
        "skill path",
    ] {
        ensure!(
            workflow.contains(marker),
            "native Nix workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm", "hunk"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        ensure!(
            !pattern.is_match(&workflow),
            "native Nix workflow retains forbidden runtime token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/nix-workflow-migration.md"))?;
    for marker in [
        ".github/scripts/detect-code-changes.sh",
        ".github/workflows/nix.yml",
        "cargo xtask ci-changes",
        "Nix flake",
        "workdeck --help",
        "not copied or executed",
    ] {
        ensure!(
            migration.contains(marker),
            "Nix workflow migration is missing {marker:?}"
        );
    }
    Ok(())
}

/// Verify the native replacement for Hunk's main CI workflow. The source
/// workflow is inspected through Git so every validation, smoke, packaging,
/// and cross-platform job has an explicit disposition without retaining a
/// Bun/Node/npm workflow in the final tree.
pub(crate) fn verify_main_workflow(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        ["show", &format!("{BASELINE}:.github/workflows/ci.yml")],
    )?;
    ensure!(
        source.len() == 7_628,
        "pinned main CI workflow changed size: {} != 7628",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "name: Main CI",
        "push:",
        "branches:",
        "main-ci-${{ github.workflow }}-${{ github.ref }}",
        "SKIP_INSTALL_SIMPLE_GIT_HOOKS",
        "jobs:",
        "changes:",
        "validate:",
        "tty-smoke:",
        "pack-npm:",
        "prebuilt-npm:",
        "build-bin:",
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
        "bun run test:session-broker-node",
        "bun run test:integration",
        "bun run test:tty-smoke",
        "bun run build:npm",
        "bun run build:prebuilt:npm",
        "bun run build:bin",
        "qemu-x86_64-static -cpu Nehalem",
        "actions/upload-artifact@",
    ] {
        ensure!(
            source.contains(marker),
            "pinned main CI workflow lost marker {marker:?}"
        );
    }

    let native = std::fs::read_to_string(repo.join(".github/workflows/ci.yml"))?;
    for marker in [
        "name: CI",
        "permissions:",
        "contents: read",
        "group: ci-${{ github.workflow }}-${{ github.ref }}",
        "changes:",
        "cargo xtask ci-changes \"$BASE_SHA\" \"$HEAD_SHA\"",
        "outputs:",
        "validate:",
        "Native validation and parity",
        "cargo test --locked --workspace --all-targets",
        "cargo fmt --all --check",
        "cargo clippy --locked --workspace --all-targets -- -D warnings",
        "cargo xtask verify",
        "cargo xtask site check",
        "cargo xtask licenses",
        "tty-smoke:",
        "terminal_lifecycle",
        "terminal_pager",
        "package:",
        "Native package smoke",
        "cargo build --locked --release --package workdeck-cli --bin workdeck",
        "target/release/workdeck --help",
        "cargo xtask architecture check",
        "cargo xtask licenses --output dist/licenses.json",
    ] {
        ensure!(
            native.contains(marker),
            "native main CI workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        ensure!(
            !pattern.is_match(&native),
            "native main CI workflow retains forbidden runtime token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/main-ci-migration.md"))?;
    for marker in [
        ".github/workflows/ci.yml",
        "changes",
        "native validation",
        "terminal smoke",
        "package smoke",
        "Bun",
        "npm",
        "not retained",
    ] {
        ensure!(
            migration.contains(marker),
            "main CI migration is missing {marker:?}"
        );
    }
    Ok(())
}

fn git(repo: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(output.stdout)
}

fn validate_revision(revision: &str) -> Result<()> {
    if revision.is_empty() || revision.starts_with('-') || revision.contains(['\0', '\n', '\r']) {
        bail!("comparison requires a non-option commit or tree revision");
    }
    Ok(())
}

fn ensure_object(repo: &Path, revision: &str) -> Result<()> {
    for kind in ["commit", "tree"] {
        if git(repo, &["cat-file", "-e", &format!("{revision}^{{{kind}}}")]).is_ok() {
            return Ok(());
        }
    }
    git(
        repo,
        &["fetch", "--no-tags", "--depth=1", "origin", revision],
    )?;
    Ok(())
}

fn docs_only_path(path: &[u8]) -> bool {
    path.ends_with(b".md")
        || path.starts_with(b"docs/")
        || path.starts_with(b"assets/")
        || path == b"LICENSE"
}

fn code_changed_in_output(output: &[u8]) -> bool {
    // Match the source script's read -r over Git's default quoted name output.
    // Do not trim spaces or reinterpret escape sequences in filenames.
    output
        .split(|byte| *byte == b'\n')
        .any(|path| !path.is_empty() && !docs_only_path(path))
}

fn detect(repo: &Path, base: &str, head: &str) -> Result<bool> {
    validate_revision(base)?;
    validate_revision(head)?;
    let base = if base.bytes().all(|byte| byte == b'0') {
        String::from_utf8(git(repo, &["hash-object", "-t", "tree", "--stdin"])?)?
            .trim()
            .to_owned()
    } else {
        base.to_owned()
    };
    ensure_object(repo, &base)?;
    ensure_object(repo, head)?;
    let output = git(
        repo,
        &["diff", "--name-only", "--no-renames", &base, head, "--"],
    )?;
    Ok(code_changed_in_output(&output))
}

fn message(changed: bool) -> &'static str {
    if changed {
        "Code changes detected; expensive CI jobs should run."
    } else {
        "Only docs/assets metadata changes detected; expensive CI jobs can be skipped."
    }
}

fn append_github_output(path: &Path, changed: bool) -> Result<()> {
    writeln!(
        OpenOptions::new().create(true).append(true).open(path)?,
        "code_changed={changed}"
    )?;
    Ok(())
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let base = args
        .next()
        .context("ci-changes requires base and head revisions")?;
    let head = args.next().context("ci-changes requires a head revision")?;
    if args.next().is_some() {
        bail!("ci-changes accepts exactly base and head revisions");
    }
    let changed = detect(&super::repo_root()?, &base, &head)?;
    if let Some(path) = std::env::var_os("GITHUB_OUTPUT").filter(|path| !path.is_empty()) {
        append_github_output(Path::new(&path), changed)?;
    }
    println!("{}", message(changed));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_nix_workflow_replaces_the_complete_pinned_shell_detector() {
        let repo = super::super::repo_root().unwrap();
        super::verify_workflow(&repo).unwrap();
    }

    #[test]
    fn native_main_ci_replaces_the_complete_pinned_bun_matrix() {
        let repo = super::super::repo_root().unwrap();
        super::verify_main_workflow(&repo).unwrap();
    }

    #[test]
    fn frozen_source_results_match_messages_and_appended_github_output() {
        let oracle: serde_json::Value =
            serde_json::from_str(include_str!("../../port/hunk/oracles/ci-code-changes.json"))
                .unwrap();
        let runs = oracle["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 2);
        for run in runs {
            let cases = run["cases"].as_array().unwrap();
            assert_eq!(cases.len(), 5);
            for case in cases {
                let changed = case["codeChanged"].as_bool().unwrap();
                assert_eq!(case["stdout"], format!("{}\n", message(changed)));
                assert_eq!(case["stderr"], "");
                assert_eq!(case["exitCode"], 0);
                let temp = tempfile::tempdir().unwrap();
                let output = temp.path().join("output");
                std::fs::write(&output, "existing=value\n").unwrap();
                append_github_output(&output, changed).unwrap();
                assert_eq!(
                    case["githubOutput"],
                    std::fs::read_to_string(output).unwrap()
                );
            }
        }
    }

    #[test]
    fn source_path_classification_preserves_case_spaces_and_quoting() {
        for path in [
            "README.md",
            "nested/a.md",
            "docs/a.rs",
            "assets/icon.svg",
            "LICENSE",
            " padded.md",
        ] {
            assert!(docs_only_path(path.as_bytes()), "{path}");
        }
        for path in [
            "README.MD",
            "LICENSE.txt",
            "nested/LICENSE",
            "docs",
            "assets",
            "src/main.rs",
            "\"docs/quoted\\t.md\"",
            "a.md ",
        ] {
            assert!(!docs_only_path(path.as_bytes()), "{path}");
        }
        assert!(!code_changed_in_output(b"\nREADME.md\ndocs/a.rs\n"));
        assert!(code_changed_in_output(b"README.md\nsrc/a.rs\n"));
        assert!(!code_changed_in_output(b""));
    }

    fn commit(repo: &Path) -> String {
        git(repo, &["add", "--all"]).unwrap();
        git(
            repo,
            &[
                "-c",
                "user.name=Parity",
                "-c",
                "user.email=parity@example.invalid",
                "commit",
                "-qm",
                "fixture",
            ],
        )
        .unwrap();
        String::from_utf8(git(repo, &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_owned()
    }

    #[test]
    fn comparison_covers_docs_code_zero_base_and_code_to_docs_rename() {
        let temp = tempfile::tempdir().unwrap();
        let repo = temp.path();
        git(repo, &["init", "-q"]).unwrap();
        std::fs::write(repo.join("README.md"), "readme\n").unwrap();
        let docs = commit(repo);
        assert!(!detect(repo, "0000000000000000000000000000000000000000", &docs).unwrap());
        assert!(!detect(repo, &docs, &docs).unwrap());
        std::fs::write(repo.join("code.rs"), "fn main() {}\n").unwrap();
        let code = commit(repo);
        assert!(detect(repo, &docs, &code).unwrap());
        assert!(detect(repo, "0000", &code).unwrap());
        std::fs::create_dir(repo.join("docs")).unwrap();
        std::fs::rename(repo.join("code.rs"), repo.join("docs/code.rs")).unwrap();
        let renamed = commit(repo);
        assert!(detect(repo, &code, &renamed).unwrap());
        std::fs::write(repo.join("docs/code.rs"), "updated docs\n").unwrap();
        let updated = commit(repo);
        assert!(!detect(repo, &renamed, &updated).unwrap());
    }

    #[test]
    fn invalid_revisions_and_missing_objects_do_not_report_docs_only() {
        for revision in ["", "--help", "a\nb", "a\rb", "a\0b"] {
            assert!(validate_revision(revision).is_err());
        }
        let temp = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-q"]).unwrap();
        assert!(detect(temp.path(), "missing", "also-missing").is_err());
    }

    #[test]
    fn shallow_checkout_fetches_missing_comparison_commit_from_origin() {
        let temp = tempfile::tempdir().unwrap();
        let origin = temp.path().join("origin");
        std::fs::create_dir(&origin).unwrap();
        git(&origin, &["init", "-q"]).unwrap();
        std::fs::write(origin.join("code.rs"), "fn main() {}\n").unwrap();
        let base = commit(&origin);
        std::fs::write(origin.join("README.md"), "documentation\n").unwrap();
        let head = commit(&origin);
        git(
            temp.path(),
            &[
                "clone",
                "--quiet",
                "--no-local",
                "--depth=1",
                "origin",
                "checkout",
            ],
        )
        .unwrap();
        let checkout = temp.path().join("checkout");
        assert!(git(&checkout, &["cat-file", "-e", &base]).is_err());
        assert!(!detect(&checkout, &base, &head).unwrap());
        assert!(git(&checkout, &["cat-file", "-e", &base]).is_ok());
    }

    #[test]
    #[ignore = "oracle capture requires Bash and preserved pinned Hunk refs"]
    fn capture_pinned_ci_change_oracles() {
        let root = super::super::repo_root().unwrap();
        let mut runs = Vec::new();
        for source in [
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd",
        ] {
            let script = String::from_utf8(
                git(
                    &root,
                    &[
                        "show",
                        &format!("{source}:.github/scripts/detect-code-changes.sh"),
                    ],
                )
                .unwrap(),
            )
            .unwrap();
            let temp = tempfile::tempdir().unwrap();
            let repo = temp.path();
            git(repo, &["init", "-q"]).unwrap();
            std::fs::write(repo.join("README.md"), "readme\n").unwrap();
            let docs = commit(repo);
            std::fs::write(repo.join("code.rs"), "fn main() {}\n").unwrap();
            let code = commit(repo);
            std::fs::create_dir(repo.join("docs")).unwrap();
            std::fs::rename(repo.join("code.rs"), repo.join("docs/code.rs")).unwrap();
            let renamed = commit(repo);
            std::fs::write(repo.join("docs/code.rs"), "documentation\n").unwrap();
            let updated = commit(repo);
            let mut cases = Vec::new();
            for (name, base, head) in [
                (
                    "zero-base-docs",
                    "0000000000000000000000000000000000000000",
                    docs.as_str(),
                ),
                ("unchanged", docs.as_str(), docs.as_str()),
                ("code-added", docs.as_str(), code.as_str()),
                ("code-to-docs-rename", code.as_str(), renamed.as_str()),
                ("docs-edit", renamed.as_str(), updated.as_str()),
            ] {
                let output_path = repo.join("github-output");
                std::fs::write(&output_path, "existing=value\n").unwrap();
                let output = Command::new("bash")
                    .args(["-c", &script, "detect-code-changes", base, head])
                    .current_dir(repo)
                    .env("GITHUB_OUTPUT", &output_path)
                    .output()
                    .unwrap();
                assert!(output.status.success());
                let changed = detect(repo, base, head).unwrap();
                let github_output = std::fs::read_to_string(&output_path).unwrap();
                assert_eq!(
                    github_output,
                    format!("existing=value\ncode_changed={changed}\n")
                );
                cases.push(serde_json::json!({
                    "name": name, "codeChanged": changed, "exitCode": output.status.code(),
                    "stdout": String::from_utf8(output.stdout).unwrap(),
                    "stderr": String::from_utf8(output.stderr).unwrap(),
                    "githubOutput": github_output,
                }));
            }
            runs.push(serde_json::json!({"sourceCommit": source, "cases": cases}));
        }
        println!(
            "CI_CHANGE_ORACLE={}",
            serde_json::json!({"schemaVersion": 1, "runs": runs})
        );
    }
}
