use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn run(root: &Path, args: &[&str], expected: Option<&str>) -> Value {
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("test-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        expected.is_none(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    if let Some(code) = expected {
        assert_eq!(value["error"]["code"], code, "{value}");
    }
    value
}
fn ok(root: &Path, args: &[&str]) -> Value {
    run(root, args, None)
}
fn repository() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    ok(root.path(), &["init", "--json"]);
    root
}

#[test]
fn issue_delete_previews_retires_and_replays_while_retaining_history() {
    let root = repository();
    let created = ok(root.path(), &["issue", "create", "Retire safely", "--json"]);
    let id = created["result"]["metadata"]["id"].as_str().unwrap();
    let short = &id[..10];
    ok(
        root.path(),
        &[
            "issue",
            "comment",
            id,
            "Historical discussion",
            "--author",
            "tester",
            "--json",
        ],
    );
    let preview = ok(
        root.path(),
        &["issue", "delete", short, "--dry-run", "--json"],
    );
    assert_eq!(preview["result"]["target"]["id"], id);
    assert_eq!(preview["result"]["allowed"], true);
    assert!(
        !root
            .path()
            .join(format!(".workdeck/tombstones/issues/{id}.yml"))
            .exists()
    );
    run(
        root.path(),
        &["issue", "delete", short, "--json"],
        Some("invalid_input"),
    );
    let fingerprint = preview["result"]["fingerprint"].as_str().unwrap();
    let args = [
        "issue",
        "delete",
        short,
        "--yes",
        "--expected-preview",
        fingerprint,
        "--request-id",
        "retire-once",
        "--json",
    ];
    let retired = ok(root.path(), &args);
    assert_eq!(retired["result"]["tombstone"]["target"]["id"], id);
    let replay = ok(root.path(), &args);
    assert_eq!(retired["result"], replay["result"]);
    assert_eq!(retired["receipt"], replay["receipt"]);
    let shown = ok(root.path(), &["issue", "show", id, "--json"]);
    assert_eq!(shown["result"]["metadata"]["archived"], true);
    assert_eq!(shown["result"]["retirement"]["request_id"], "retire-once");
    let comments = ok(root.path(), &["issue", "comments", id, "--json"]);
    assert_eq!(comments["result"].as_array().unwrap().len(), 1);
    run(
        root.path(),
        &["issue", "update", id, "--title", "Resurrect", "--json"],
        Some("policy_blocked"),
    );
    assert!(
        root.path()
            .join(format!(".workdeck/issues/{id}/item.md"))
            .exists()
    );
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn planning_delete_preserves_ids_and_retired_records_for_every_reference_kind() {
    let root = repository();
    for kind in ["project", "cycle", "label"] {
        ok(
            root.path(),
            &[
                kind,
                "create",
                "Retained reference",
                "--id",
                "stable-id",
                "--json",
            ],
        );
        let preview = ok(
            root.path(),
            &[kind, "delete", "stable-id", "--dry-run", "--json"],
        );
        assert_eq!(preview["result"]["allowed"], true);
        let args = [
            kind,
            "delete",
            "stable-id",
            "--yes",
            "--request-id",
            kind,
            "--json",
        ];
        let deleted = ok(root.path(), &args);
        assert_eq!(deleted["result"], ok(root.path(), &args)["result"]);
        let shown = ok(root.path(), &[kind, "show", "stable-id", "--json"]);
        assert_eq!(shown["result"]["metadata"]["archived"], true);
        assert_eq!(shown["result"]["retirement"]["target"]["kind"], kind);
        run(
            root.path(),
            &[
                kind,
                "create",
                "Identity reuse",
                "--id",
                "stable-id",
                "--json",
            ],
            Some("policy_blocked"),
        );
    }
}

#[test]
fn deletion_rechecks_reviewed_membership_and_exposes_incoming_references() {
    let root = repository();
    ok(
        root.path(),
        &[
            "project",
            "create",
            "Referenced",
            "--id",
            "project-a",
            "--json",
        ],
    );
    let preview = ok(
        root.path(),
        &["project", "delete", "project-a", "--dry-run", "--json"],
    );
    let issue = ok(
        root.path(),
        &[
            "issue",
            "create",
            "Incoming edge",
            "--project",
            "project-a",
            "--json",
        ],
    );
    let id = issue["result"]["metadata"]["id"].as_str().unwrap();
    run(
        root.path(),
        &[
            "project",
            "delete",
            "project-a",
            "--yes",
            "--expected-preview",
            preview["result"]["fingerprint"].as_str().unwrap(),
            "--json",
        ],
        Some("stale_source"),
    );
    let blocked = run(
        root.path(),
        &["project", "delete", "project-a", "--yes", "--json"],
        Some("policy_blocked"),
    );
    assert_eq!(blocked["error"]["details"]["blockers"][0]["issue"], id);
    run(
        root.path(),
        &[
            "project",
            "delete",
            "project-a",
            "--yes",
            "--force",
            "--json",
        ],
        Some("invalid_input"),
    );
    fs::write(
        root.path().join("remove-project.json"),
        serde_json::to_vec(&json!({"fields":{"project":null}})).unwrap(),
    )
    .unwrap();
    ok(
        root.path(),
        &[
            "issue",
            "update",
            id,
            "--from-json",
            "remove-project.json",
            "--json",
        ],
    );
    ok(
        root.path(),
        &["project", "delete", "project-a", "--yes", "--json"],
    );
    assert_eq!(
        ok(root.path(), &["doctor", "--json"])["result"]["valid"],
        true
    );
}

#[test]
fn deletion_stages_only_its_receipt_paths_and_keeps_an_unrelated_index_entry() {
    let root = repository();
    let issue = ok(
        root.path(),
        &["issue", "create", "Stage retirement", "--json"],
    );
    let id = issue["result"]["metadata"]["id"].as_str().unwrap();
    fs::write(root.path().join("unrelated.txt"), "already staged\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "unrelated.txt"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    let deleted = ok(
        root.path(),
        &[
            "issue",
            "delete",
            id,
            "--yes",
            "--request-id",
            "stage-retirement",
            "--stage",
            "--json",
        ],
    );
    assert_eq!(deleted["staging"]["index_changed"], true, "{deleted}");
    let output = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let paths = String::from_utf8(output.stdout).unwrap();
    assert!(paths.lines().any(|path| path == "unrelated.txt"));
    assert!(
        paths
            .lines()
            .any(|path| path == format!(".workdeck/tombstones/issues/{id}.yml"))
    );
    assert!(!paths.lines().any(|path| path == ".workdeck/config.yml"));
}
