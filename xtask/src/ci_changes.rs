//! MIT translation of Hunk .github/scripts/detect-code-changes.sh.
//! Failed Git comparisons fail closed rather than silently skipping CI.

use anyhow::{Context, Result, bail};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

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
