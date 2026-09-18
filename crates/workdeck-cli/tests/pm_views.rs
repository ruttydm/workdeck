use assert_cmd::prelude::*;
use serde_json::Value;
use std::{path::Path, process::Command};
use workdeck_pm::{CreateIssue, Repository, RequestId};
fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
#[test]
fn view_authoring_replay_and_live_query_match_library() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let repo = Repository::init(root, "WD").unwrap();
    let create = [
        "view",
        "create",
        "ready",
        "--name",
        "Ready work",
        "--query",
        "{}",
        "--request-id",
        "view-once",
        "--json",
    ];
    let first = ok(root, &create);
    assert_eq!(
        first["result"],
        ok(root, &["view", "show", "ready", "--json"])["result"]
    );
    repo.create_issue(&CreateIssue::new("Next task", ""), &RequestId::new())
        .unwrap();
    let result = ok(root, &["view", "run", "ready", "--json"]);
    assert_eq!(
        result["result"],
        serde_json::to_value(repo.query_saved_view("ready").unwrap()).unwrap()
    );
    let hash = first["result"]["content"].as_str().unwrap();
    ok(
        root,
        &[
            "view",
            "update",
            "ready",
            "--name",
            "Archived view",
            "--query",
            "{}",
            "--archived",
            "--expected-content",
            hash,
            "--json",
        ],
    );
    assert_eq!(ok(root, &create)["receipt"], first["receipt"]);
    assert_eq!(
        ok(root, &["view", "list", "--json"])["result"][0]["definition"]["archived"],
        true
    );
    let stale = run(
        root,
        &[
            "view",
            "update",
            "ready",
            "--name",
            "Stale",
            "--query",
            "{}",
            "--expected-content",
            hash,
            "--json",
        ],
    );
    assert!(!stale.status.success());
    assert_eq!(
        repo.saved_view("ready").unwrap().definition.name,
        "Archived view"
    );
    assert!(
        !run(root, &["view", "run", "ready", "--stage", "--json"])
            .status
            .success()
    );
}
#[test]
fn view_discovery_is_native_and_does_not_initialize_fresh_sources() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let capabilities = ok(root, &["capabilities", "--json"]);
    let view = capabilities["result"]["commands"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["path"] == "view run")
        .unwrap();
    assert_eq!(view["native_planning"]["implemented"], true);
    assert!(ok(root, &["schema", "saved-view-write", "--json"])["result"].is_object());
    assert!(!run(root, &["view", "list", "--json"]).status.success());
    assert!(!root.join(".workdeck").exists());
    assert!(!root.join(".agents").exists());
}

#[test]
fn forged_saved_view_receipt_cannot_change_replay_or_history() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    let repository = Repository::init(root, "WD").unwrap();
    let create = [
        "view",
        "create",
        "ready",
        "--name",
        "Original view",
        "--query",
        "{}",
        "--request-id",
        "view-forgery",
        "--json",
    ];
    let original = ok(root, &create);
    let mut forged = original["receipt"].clone();
    forged["result"]["definition"]["name"] = serde_json::json!("Forged name");
    let path = repository
        .root()
        .join("operations")
        .join(format!("{}.yml", forged["operation_id"].as_str().unwrap()));
    std::fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
    let before = std::fs::read(repository.root().join("views/ready.yml")).unwrap();
    for args in [
        create.to_vec(),
        vec!["events", "list", "--json"],
        vec!["export", "--json"],
        {
            let mut args = create.to_vec();
            args.push("--stage");
            args
        },
    ] {
        let out = run(root, &args);
        assert!(!out.status.success(), "forged receipt accepted: {args:?}");
    }
    assert_eq!(
        std::fs::read(repository.root().join("views/ready.yml")).unwrap(),
        before
    );
    assert_eq!(
        repository.saved_view("ready").unwrap().definition.name,
        "Original view"
    );
    assert!(!root.join(".git/index").exists());
}

#[test]
fn saved_view_noop_replays_and_stages_only_receipt_after_later_update() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    Repository::init(root, "WD").unwrap();
    let first = ok(
        root,
        &[
            "view", "create", "ready", "--name", "Ready", "--query", "{}", "--json",
        ],
    );
    let hash = first["result"]["content"].as_str().unwrap();
    let noop = [
        "view",
        "update",
        "ready",
        "--name",
        "Ready",
        "--query",
        "{}",
        "--expected-content",
        hash,
        "--request-id",
        "view-noop",
        "--json",
    ];
    let original = ok(root, &noop);
    assert_eq!(original["receipt"]["changed"], serde_json::json!([]));
    ok(
        root,
        &[
            "view",
            "update",
            "ready",
            "--name",
            "Later",
            "--query",
            "{}",
            "--expected-content",
            hash,
            "--json",
        ],
    );
    let mut stage = noop.to_vec();
    stage.push("--stage");
    let replay = ok(root, &stage);
    assert_eq!(replay["receipt"], original["receipt"]);
    let paths = replay["staging"]["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 1);
    assert!(
        paths[0]
            .as_str()
            .unwrap()
            .starts_with(".workdeck/operations/")
    );
    assert_eq!(
        ok(root, &["view", "show", "ready", "--json"])["result"]["definition"]["name"],
        "Later"
    );
}
