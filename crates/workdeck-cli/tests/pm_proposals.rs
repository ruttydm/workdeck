#![cfg(unix)]
use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::Path,
    process::{Command, Output},
};
use workdeck_pm::*;
fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|k| k.starts_with("GIT_")) {
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
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {output:?}"))
}
#[test]
fn proposal_preview_publish_and_resume_preserve_dirty_developer_state_and_original_plan() {
    let temp = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
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
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Accepted", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut config = repo.config().unwrap();
    config.sources = Some(SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
    });
    fs::write(
        repo.root().join("config.yml"),
        serde_json::to_vec(&config).unwrap(),
    )
    .unwrap();
    fs::write(temp.path().join("code.txt"), "accepted").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "accepted"]);
    git(temp.path(), &["push", "origin", "main"]);
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: BTreeMap::from([("title".into(), json!("Proposed"))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    fs::write(temp.path().join("code.txt"), "staged").unwrap();
    git(temp.path(), &["add", "code.txt"]);
    fs::write(temp.path().join("code.txt"), "unstaged").unwrap();
    let index = fs::read(temp.path().join(".git/index")).unwrap();
    let head = git(temp.path(), &["rev-parse", "HEAD"]);
    let preview = run(
        temp.path(),
        &[
            "source",
            "proposal",
            "preview",
            "--ref",
            "refs/heads/workdeck-proposals/cli",
            "--title",
            "Review planning",
            "--json",
        ],
    );
    assert!(preview.status.success(), "{preview:?}");
    let data = value(&preview);
    let hash = data["result"]["plan"]["fingerprint"].as_str().unwrap();
    let request = RequestId::new().to_string();
    let args = [
        "source",
        "proposal",
        "publish",
        "--plan",
        hash,
        "--expected-plan",
        hash,
        "--request-id",
        &request,
        "--json",
        "--no-input",
    ];
    let published = run(temp.path(), &args);
    assert!(published.status.success(), "{published:?}");
    let data = value(&published);
    assert_eq!(data["result"]["state"], "confirmed");
    assert_eq!(
        git(
            remote.path(),
            &["show", "refs/heads/workdeck-proposals/cli:code.txt"]
        ),
        b"accepted"
    );
    let now = repo.show_issue(issue.metadata.id.as_str()).unwrap();
    repo.update_issue(
        issue.metadata.id.as_str(),
        &now.source,
        &UpdateIssue {
            fields: BTreeMap::from([("title".into(), json!("Later draft"))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let replay = run(temp.path(), &args);
    assert!(replay.status.success(), "{replay:?}");
    assert_eq!(
        value(&replay)["result"]["candidate"],
        data["result"]["candidate"]
    );
    let resumed = run(
        temp.path(),
        &[
            "source",
            "proposal",
            "resume",
            "--request-id",
            &request,
            "--json",
        ],
    );
    assert!(resumed.status.success(), "{resumed:?}");
    assert_eq!(
        value(&resumed)["result"]["candidate"],
        data["result"]["candidate"]
    );
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(temp.path().join("code.txt")).unwrap(), b"unstaged");
}
