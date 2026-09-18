use assert_cmd::prelude::*;
use serde_json::Value;
use std::process::Command;
use workdeck_pm::Repository;
#[test]
fn source_status_is_explicit_local_observation_and_never_initializes_a_remote() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(temp.path())
        .env("XDG_CONFIG_HOME", temp.path().join("config"))
        .args(["source", "status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value["result"]["repository"],
        repository.identity().as_str()
    );
    assert_eq!(value["result"]["working"]["identity"]["role"], "local");
    assert_eq!(value["result"]["accepted"], Value::Null);
    assert_eq!(value["result"]["coordination"], Value::Null);
    assert!(!temp.path().join(".git").exists());
}

#[test]
fn source_and_claim_reads_reject_uninitialized_projects_without_creating_state() {
    let temp = tempfile::tempdir().unwrap();
    for args in [["source", "status", "--json"], ["claim", "list", "--json"]] {
        let output = Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(temp.path())
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .args(args)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "missing source must not return empty success: {output:?}"
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(value["error"].is_object(), "{value}");
        assert!(!temp.path().join(".workdeck").exists());
        assert!(!temp.path().join(".git").exists());
    }
}

#[cfg(unix)]
#[test]
fn refresh_requires_reviewed_remote_binding_and_preserves_developer_state() {
    use std::{fs, path::Path};
    use workdeck_pm::*;
    fn git(root: &Path, args: &[&str]) -> Vec<u8> {
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
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "{args:?}: {output:?}");
        output.stdout
    }
    let temp = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    let alternate = tempfile::tempdir().unwrap();
    git(temp.path(), &["init", "-b", "main"]);
    git(remote.path(), &["init", "--bare"]);
    git(temp.path(), &["config", "user.name", "Fixture"]);
    git(
        temp.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(
        temp.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new("Reviewed claim", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut config = repository.config().unwrap();
    config.sources = Some(SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
    });
    fs::write(
        repository.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    fs::write(temp.path().join("code.txt"), "accepted").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "accepted"]);
    git(temp.path(), &["push", "origin", "main"]);
    git(
        alternate.path(),
        &["clone", "--bare", remote.path().to_str().unwrap(), "."],
    );
    fs::write(temp.path().join("code.txt"), "staged").unwrap();
    git(temp.path(), &["add", "code.txt"]);
    fs::write(temp.path().join("code.txt"), "unstaged").unwrap();
    let index = fs::read(temp.path().join(".git/index")).unwrap();
    let head = git(temp.path(), &["rev-parse", "HEAD"]);
    let run = |args: &[&str]| {
        Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(temp.path())
            .env("XDG_CONFIG_HOME", temp.path().join("config"))
            .args(args)
            .output()
            .unwrap()
    };
    let status = run(&["source", "status", "--json"]);
    let value: Value = serde_json::from_slice(&status.stdout).unwrap();
    let config_hash = value["result"]["config"].as_str().unwrap();
    let binding = value["result"]["binding"].as_str().unwrap();
    let contract = run(&["claim", "contract", issue.metadata.id.as_str(), "--json"]);
    assert!(contract.status.success(), "{contract:?}");
    let contract: Value = serde_json::from_slice(&contract.stdout).unwrap();
    let contract_hash = contract["result"]["fingerprint"].as_str().unwrap();
    let unreviewed = run(&[
        "claim",
        "acquire",
        "--actor",
        "agent",
        "--contract",
        contract_hash,
        "--expected-contract",
        contract_hash,
        "--request-id",
        RequestId::new().as_str(),
        "--json",
    ]);
    assert!(
        !unreviewed.status.success(),
        "shared acquisition omitted reviewed destination: {unreviewed:?}"
    );
    let unreviewed: Value = serde_json::from_slice(&unreviewed.stdout).unwrap();
    assert_eq!(unreviewed["error"]["code"], "invalid_input", "{unreviewed}");
    let id = RequestId::new();
    let fetched = run(&[
        "source",
        "fetch",
        "--expected-config",
        config_hash,
        "--expected-binding",
        binding,
        "--request-id",
        id.as_str(),
        "--json",
    ]);
    assert!(fetched.status.success(), "{fetched:?}");
    git(
        temp.path(),
        &[
            "remote",
            "set-url",
            "origin",
            alternate.path().to_str().unwrap(),
        ],
    );
    let claim = run(&[
        "claim",
        "acquire",
        "--actor",
        "agent",
        "--contract",
        contract_hash,
        "--expected-contract",
        contract_hash,
        "--expected-binding",
        binding,
        "--request-id",
        RequestId::new().as_str(),
        "--json",
    ]);
    assert!(
        !claim.status.success(),
        "claim followed changed destination: {claim:?}"
    );
    let claim: Value = serde_json::from_slice(&claim.stdout).unwrap();
    assert_eq!(claim["error"]["code"], "stale_source", "{claim}");
    let rejected = run(&[
        "source",
        "sync",
        "--expected-config",
        config_hash,
        "--expected-binding",
        binding,
        "--request-id",
        RequestId::new().as_str(),
        "--json",
    ]);
    assert!(
        !rejected.status.success(),
        "reviewed destination changed: {rejected:?}"
    );
    let value: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(value["error"]["code"], "stale_source", "{value}");
    assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(temp.path().join("code.txt")).unwrap(), b"unstaged");
}
