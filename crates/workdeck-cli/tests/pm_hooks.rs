#![cfg(unix)]
use assert_cmd::prelude::*;
use serde_json::Value;
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use workdeck_pm::{
    CreateIssue, ErrorCode, HookApply, HookFaultPoint, HookMode, IssueRecord, PmError, Repository,
    RequestId,
};
fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
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
fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .env_remove("GIT_INDEX_FILE")
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
fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "--quiet"]);
    git(root.path(), &["config", "core.hooksPath", ".git/hooks"]);
    let repo = Repository::init(root.path(), "WD").unwrap();
    let issue = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Hook candidate", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    (root, repo, issue)
}
#[test]
fn installed_pre_commit_runs_actual_workdeck_with_git_effective_candidate_index() {
    let (root, repo, issue) = fixture();
    let valid_index = root.path().join("valid.index");
    fs::copy(root.path().join(".git/index"), &valid_index).unwrap();
    let preview = run(root.path(), &["hooks", "preview", "pre-commit", "--json"]);
    assert!(preview.status.success(), "{:?}", value(&preview));
    let data = value(&preview);
    let hash = data["result"]["fingerprint"].as_str().unwrap();
    let request = RequestId::new().to_string();
    let args = [
        "hooks",
        "install",
        "pre-commit",
        "--expected-plan",
        hash,
        "--request-id",
        &request,
        "--json",
        "--no-input",
    ];
    let applied = run(root.path(), &args);
    assert!(applied.status.success(), "{:?}", value(&applied));
    assert_eq!(value(&applied), value(&run(root.path(), &args)));
    let hook = data["result"]["target"].as_str().unwrap();
    let path = repo.root().join(&issue.path);
    let original = fs::read(&path).unwrap();
    fs::write(&path, "---\ninvalid: [\n---\n").unwrap();
    git(root.path(), &["add", "--", ".workdeck"]);
    fs::write(&path, &original).unwrap();
    let invalid_index = root.path().join("invalid.index");
    fs::copy(root.path().join(".git/index"), &invalid_index).unwrap();
    let binary = Command::cargo_bin("workdeck")
        .unwrap()
        .get_program()
        .to_owned();
    let mut paths = vec![Path::new(&binary).parent().unwrap().to_owned()];
    paths.extend(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    ));
    let path_env = std::env::join_paths(paths).unwrap();
    let invoke = |index: &Path| {
        Command::new(hook)
            .current_dir(root.path())
            .env("PATH", &path_env)
            .env("GIT_INDEX_FILE", index)
            .env("XDG_CONFIG_HOME", root.path().join("isolated-config"))
            .output()
            .unwrap()
    };
    let valid = invoke(&valid_index);
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    let invalid = invoke(&invalid_index);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("staged planning validation"));
    assert_eq!(fs::read(path).unwrap(), original);
}
#[test]
fn cli_recovery_uses_original_local_request_without_replanning_and_unmanaged_hook_is_preserved() {
    let (root, _, _) = fixture();
    let plan = workdeck_pm::hook_preview(root.path(), HookMode::Install).unwrap();
    let request = RequestId::new();
    workdeck_pm::apply_hook_with_faults(
        root.path(),
        &HookApply {
            mode: HookMode::Install,
            expected_plan: plan.fingerprint,
        },
        &request,
        |point| {
            if point == HookFaultPoint::AfterJournal {
                Err(PmError::new(ErrorCode::Io, "interrupt"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    let recovered = run(
        root.path(),
        &[
            "hooks",
            "recover",
            "--request-id",
            &request.to_string(),
            "--json",
        ],
    );
    assert!(recovered.status.success(), "{:?}", value(&recovered));
    assert_eq!(
        value(&recovered)["result"]["request_id"],
        request.to_string()
    );
    fs::write(&plan.target, b"#!/bin/sh\nexit 9\n").unwrap();
    let preview = run(
        root.path(),
        &["hooks", "preview", "--mode", "update", "--json"],
    );
    assert!(preview.status.success());
    let data = value(&preview);
    assert_eq!(data["result"]["allowed"], false);
    let output = run(
        root.path(),
        &[
            "hooks",
            "update",
            "--expected-plan",
            data["result"]["fingerprint"].as_str().unwrap(),
            "--request-id",
            &RequestId::new().to_string(),
            "--json",
        ],
    );
    assert!(!output.status.success());
    assert_eq!(value(&output)["error"]["code"], "policy_blocked");
    assert_eq!(fs::read(plan.target).unwrap(), b"#!/bin/sh\nexit 9\n");
}
