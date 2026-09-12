#![cfg(unix)]
use assert_cmd::prelude::*;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use workdeck_pm::{CreateIssue, IssueRecord, Repository, RequestId};
fn run(root: &Path, args: &[&str], index: Option<&Path>) -> Output {
    let mut command = Command::cargo_bin("workdeck").unwrap();
    command
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .env_remove("GIT_INDEX_FILE");
    if let Some(index) = index {
        command.env("GIT_INDEX_FILE", index);
    }
    command.output().unwrap()
}
fn git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
        }
    }
    let output = command
        .current_dir(root)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .args([
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} / {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}
fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    let repo = Repository::init(root.path(), "WD").unwrap();
    let issue = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Staged issue", "Body"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    (root, repo, issue)
}
#[test]
fn staged_cli_ignores_invalid_working_configuration_and_reports_candidate_source() {
    let (root, repo, _) = fixture();
    let before = fs::read(root.path().join(".git/index")).unwrap();
    fs::write(repo.root().join("config.yml"), "invalid: [\n").unwrap();
    let output = run(root.path(), &["doctor", "--staged", "--json"], None);
    assert!(output.status.success(), "{:?}", value(&output));
    let data = value(&output);
    assert_eq!(data["result"]["valid"], true);
    assert_eq!(data["source"]["role"], "staged");
    assert_eq!(
        data["source"]["repository"],
        serde_json::json!(repo.identity())
    );
    assert_eq!(
        data["source"]["observation"]["identity"]["repository"],
        serde_json::json!(repo.identity())
    );
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), before);
    assert_eq!(
        fs::read(repo.root().join("config.yml")).unwrap(),
        b"invalid: [\n"
    );
}
#[test]
fn effective_hook_index_overrides_broken_default_index_without_working_tree_fallback() {
    let (root, repo, issue) = fixture();
    let alternate = root.path().join("candidate.index");
    fs::copy(root.path().join(".git/index"), &alternate).unwrap();
    let path = repo.root().join(&issue.path);
    let original = fs::read(&path).unwrap();
    fs::write(&path, "---\ninvalid: [\n---\n").unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    fs::write(&path, &original).unwrap();
    let invalid = run(root.path(), &["doctor", "--staged", "--json"], None);
    assert!(!invalid.status.success());
    let data = value(&invalid);
    assert_eq!(data["ok"], false);
    assert_eq!(data["error"]["code"], "invalid_input");
    assert_eq!(data["source"]["role"], "staged");
    let valid = run(
        root.path(),
        &["doctor", "--staged", "--json"],
        Some(&alternate),
    );
    assert!(valid.status.success(), "{:?}", value(&valid));
    assert_eq!(fs::read(path).unwrap(), original);
}
#[test]
fn staged_read_on_uninitialized_repository_never_initializes_planning_or_legacy_source() {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    let output = run(root.path(), &["doctor", "--staged", "--json"], None);
    assert!(!output.status.success());
    assert_eq!(value(&output)["ok"], false);
    assert!(!root.path().join(".workdeck").exists());
    assert!(!root.path().join(".agents").exists());
    assert!(!root.path().join(".git/index").exists());
}
