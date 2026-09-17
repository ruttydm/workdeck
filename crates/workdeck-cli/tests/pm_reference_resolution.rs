use assert_cmd::prelude::*;
use serde_json::Value;
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
    if let Some(code) = expected {
        assert_eq!(value["error"]["code"], code, "{value}");
    }
    value
}

fn ok(root: &Path, args: &[&str]) -> Value {
    run(root, args, None)
}

fn fixture() -> tempfile::TempDir {
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
fn force_retirement_requires_a_reviewed_resolution_and_preserves_issue_history() {
    for kind in ["project", "cycle", "label"] {
        let fixture = fixture();
        let root = fixture.path();
        ok(
            root,
            &[kind, "create", "Retire me", "--id", "old", "--json"],
        );
        let flag = format!("--{kind}");
        let created = ok(
            root,
            &[
                "issue",
                "create",
                "Keep this issue",
                "--description",
                "Keep authored Markdown",
                &flag,
                "old",
                "--json",
            ],
        );
        let id = created["result"]["metadata"]["id"].as_str().unwrap();
        ok(
            root,
            &[
                "issue",
                "comment",
                id,
                "Retained discussion",
                "--author",
                "tester",
                "--json",
            ],
        );
        let comments = ok(root, &["issue", "comments", id, "--json"]);
        let preview = ok(
            root,
            &[kind, "delete", "old", "--force", "--dry-run", "--json"],
        );
        let plan = &preview["result"];
        assert_eq!(plan["allowed"], true);
        assert_eq!(plan["target"]["id"], "old");
        assert_eq!(plan["affected"].as_array().unwrap().len(), 1);
        assert_eq!(plan["affected"][0]["issue"], id);
        assert_eq!(
            plan["affected"][0]["field"],
            if kind == "label" { "labels" } else { kind }
        );
        run(
            root,
            &[kind, "delete", "old", "--force", "--yes", "--json"],
            Some("invalid_input"),
        );
        assert!(ok(root, &[kind, "show", "old", "--json"])["result"]["retirement"].is_null());
        let fingerprint = plan["fingerprint"].as_str().unwrap();
        let apply = [
            kind,
            "delete",
            "old",
            "--force",
            "--yes",
            "--expected-preview",
            fingerprint,
            "--request-id",
            "resolve-once",
            "--json",
        ];
        let retired = ok(root, &apply);
        let after = ok(root, &["issue", "show", id, "--json"]);
        let metadata = &after["result"]["metadata"];
        if kind == "label" {
            assert!(
                metadata["labels"].is_null() || metadata["labels"].as_array().unwrap().is_empty()
            );
        } else {
            assert!(metadata[kind].is_null());
        }
        assert_eq!(after["result"]["body"], "Keep authored Markdown");
        assert_eq!(metadata["title"], "Keep this issue");
        assert_eq!(
            ok(root, &["issue", "comments", id, "--json"])["result"],
            comments["result"]
        );
        assert_eq!(
            ok(root, &[kind, "show", "old", "--json"])["result"]["retirement"]["target"]["id"],
            "old"
        );
        ok(
            root,
            &["issue", "update", id, "--title", "Later edit", "--json"],
        );
        let replay = ok(root, &apply);
        assert_eq!(replay["receipt"], retired["receipt"]);
        assert_eq!(replay["result"], retired["result"]);
        assert_eq!(
            ok(root, &["issue", "show", id, "--json"])["result"]["metadata"]["title"],
            "Later edit"
        );
        assert_eq!(ok(root, &["doctor", "--json"])["result"]["valid"], true);
        assert!(!root.join(".agents").exists());
    }
}

#[test]
fn force_retirement_rejects_changed_membership_and_direct_editor_changes() {
    for direct_edit in [false, true] {
        let fixture = fixture();
        let root = fixture.path();
        ok(
            root,
            &["project", "create", "Retire me", "--id", "old", "--json"],
        );
        let created = ok(
            root,
            &[
                "issue",
                "create",
                "First member",
                "--project",
                "old",
                "--json",
            ],
        );
        let id = created["result"]["metadata"]["id"].as_str().unwrap();
        let preview = ok(
            root,
            &["project", "delete", "old", "--force", "--dry-run", "--json"],
        );
        let issue_path = root.join(format!(".workdeck/issues/{id}/item.md"));
        if direct_edit {
            let raw = fs::read_to_string(&issue_path).unwrap();
            fs::write(&issue_path, format!("{raw}\nDirect editor content.\n")).unwrap();
        } else {
            ok(
                root,
                &[
                    "issue",
                    "create",
                    "New member after preview",
                    "--project",
                    "old",
                    "--json",
                ],
            );
        }
        let before = fs::read(&issue_path).unwrap();
        let receipt_count = fs::read_dir(root.join(".workdeck/operations"))
            .unwrap()
            .count();
        run(
            root,
            &[
                "project",
                "delete",
                "old",
                "--force",
                "--yes",
                "--expected-preview",
                preview["result"]["fingerprint"].as_str().unwrap(),
                "--json",
            ],
            Some("stale_source"),
        );
        assert_eq!(fs::read(&issue_path).unwrap(), before);
        assert_eq!(
            fs::read_dir(root.join(".workdeck/operations"))
                .unwrap()
                .count(),
            receipt_count
        );
        assert!(ok(root, &["project", "show", "old", "--json"])["result"]["retirement"].is_null());
    }
}

#[test]
fn force_retirement_stages_its_complete_change_set_and_preserves_unrelated_index_entries() {
    let fixture = fixture();
    let root = fixture.path();
    ok(
        root,
        &["project", "create", "Retire me", "--id", "old", "--json"],
    );
    let issue = ok(
        root,
        &[
            "issue",
            "create",
            "Linked member",
            "--project",
            "old",
            "--json",
        ],
    );
    let id = issue["result"]["metadata"]["id"].as_str().unwrap();
    fs::write(root.join("unrelated.txt"), "already staged\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "unrelated.txt"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    let preview = ok(
        root,
        &["project", "delete", "old", "--force", "--dry-run", "--json"],
    );
    let applied = ok(
        root,
        &[
            "project",
            "delete",
            "old",
            "--force",
            "--yes",
            "--expected-preview",
            preview["result"]["fingerprint"].as_str().unwrap(),
            "--request-id",
            "stage-resolution",
            "--stage",
            "--json",
        ],
    );
    assert_eq!(applied["staging"]["index_changed"], true);
    let output = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(output.status.success());
    let paths = String::from_utf8(output.stdout).unwrap();
    let mut expected = std::collections::BTreeSet::from(["unrelated.txt".to_owned()]);
    for change in applied["receipt"]["changed"].as_array().unwrap() {
        expected.insert(format!(".workdeck/{}", change["path"].as_str().unwrap()));
    }
    expected.insert(format!(
        ".workdeck/operations/{}.yml",
        applied["receipt"]["operation_id"].as_str().unwrap()
    ));
    assert_eq!(
        paths
            .lines()
            .map(str::to_owned)
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    assert!(expected.contains(&format!(".workdeck/issues/{id}/item.md")));
    assert!(expected.contains(".workdeck/projects/old/item.md"));
    assert!(expected.contains(".workdeck/tombstones/projects/old.yml"));
    assert!(!expected.contains(".workdeck/config.yml"));
    assert_eq!(
        fs::read_to_string(root.join("unrelated.txt")).unwrap(),
        "already staged\n"
    );
}
