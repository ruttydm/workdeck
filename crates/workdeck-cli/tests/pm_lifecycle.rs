use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("test-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .stdin(Stdio::null())
        .args(args)
        .output()
        .unwrap()
}

fn success(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], true);
    value
}

fn failure(root: &Path, args: &[&str], code: &str, exit: i32) -> Value {
    let output = run(root, args);
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    success(root.path(), &["init", "--prefix", "WD", "--json"]);
    root
}

fn create(root: &Path, fields: Value) -> Value {
    fs::write(
        root.join("issue-input.json"),
        serde_json::to_vec(&json!({"title":"Acceptance subject", "fields":fields})).unwrap(),
    )
    .unwrap();
    success(
        root,
        &[
            "issue",
            "create",
            "--from-json",
            "issue-input.json",
            "--json",
        ],
    )
}

fn id(issue: &Value) -> &str {
    issue["result"]["metadata"]["id"].as_str().unwrap()
}

// Compare every authoritative file, including receipts. Coordination and cache
// files may change during a read or rejected mutation without changing authority.
fn authority(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, directory: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(base).unwrap();
            if [".tmp", ".index", ".local"]
                .iter()
                .any(|local| relative == Path::new(local))
            {
                continue;
            }
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                visit(base, &path, files);
            } else {
                assert!(kind.is_file(), "unexpected fixture path: {path:?}");
                files.insert(relative.to_path_buf(), fs::read(path).unwrap());
            }
        }
    }
    let base = root.join(".workdeck");
    let mut files = BTreeMap::new();
    visit(&base, &base, &mut files);
    files
}

fn require_policy(root: &Path, key: &str) {
    let path = root.join(".workdeck/config.yml");
    let before = fs::read_to_string(&path).unwrap();
    assert_eq!(before.matches("acceptance:\n").count(), 1);
    assert!(!before.contains(key));
    fs::write(
        path,
        before.replace(
            "acceptance:\n",
            &format!("acceptance:\n  {key}: [release-gate]\n"),
        ),
    )
    .unwrap();
}

#[test]
fn one_issue_completes_the_full_headless_lifecycle_in_one_repository() {
    let root = fixture();
    let created = create(
        root.path(),
        json!({"acceptance":[{"id":"round-trip", "description":"Lifecycle preserves authored content", "checked":false}]}),
    );
    let key = id(&created);
    let item = root
        .path()
        .join(".workdeck")
        .join(created["result"]["path"].as_str().unwrap());
    let before = authority(root.path());
    let blocked = success(root.path(), &["issue", "done", key, "--dry-run", "--json"]);
    assert_eq!(blocked["result"]["allowed"], false);
    assert!(
        blocked["result"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason.as_str().unwrap().contains("round-trip"))
    );
    assert_eq!(authority(root.path()), before);

    let original = fs::read_to_string(&item).unwrap();
    assert!(original.contains("\"checked\": false"));
    let draft = original
        .replace("Acceptance subject", "Reviewed lifecycle")
        .replace("\"checked\": false", "\"checked\": true")
        .replacen("---\n", "---\n# Keep the authored acceptance note\n", 1)
        + "\nLifecycle behavior has been reviewed.\n";
    fs::write(root.path().join("reviewed.md"), draft).unwrap();
    let edited = success(
        root.path(),
        &[
            "issue",
            "edit",
            key,
            "--from-file",
            "reviewed.md",
            "--no-input",
            "--json",
        ],
    );
    assert_eq!(edited["result"]["metadata"]["title"], "Reviewed lifecycle");
    assert_eq!(
        edited["result"]["metadata"]["acceptance"][0]["checked"],
        true
    );
    assert!(
        fs::read_to_string(&item)
            .unwrap()
            .contains("# Keep the authored acceptance note")
    );

    let item_before_comment = fs::read(&item).unwrap();
    let comment_args = [
        "issue",
        "comment",
        key,
        "Reviewed the complete lifecycle",
        "--author",
        "reviewer",
        "--request-id",
        "lifecycle-comment",
        "--json",
    ];
    let comment = success(root.path(), &comment_args);
    assert_eq!(success(root.path(), &comment_args), comment);
    assert_eq!(fs::read(&item).unwrap(), item_before_comment);
    for (command, reference) in [
        ("link-file", "src/lifecycle.rs"),
        ("link-commit", "abcdef1"),
        ("link-document", "docs/lifecycle.md#acceptance"),
    ] {
        success(root.path(), &["issue", command, key, reference, "--json"]);
    }
    let linked = success(root.path(), &["issue", "show", key, "--json"]);
    assert_eq!(
        linked["result"]["metadata"]["files"],
        json!([{"path":"src/lifecycle.rs"}])
    );
    assert_eq!(linked["result"]["metadata"]["commits"], json!(["abcdef1"]));
    assert_eq!(
        linked["result"]["metadata"]["documents"],
        json!(["docs/lifecycle.md#acceptance"])
    );
    let before_preview = authority(root.path());
    let preview = success(root.path(), &["issue", "done", key, "--dry-run", "--json"]);
    assert_eq!(preview["result"]["allowed"], true);
    assert_eq!(preview["result"]["basis"], "declared");
    assert_eq!(authority(root.path()), before_preview);
    let closed = success(
        root.path(),
        &[
            "issue",
            "close",
            key,
            "--request-id",
            "lifecycle-close",
            "--json",
        ],
    );
    assert_eq!(closed["result"]["metadata"]["status"], "done");
    assert!(closed["result"]["metadata"]["completed_at"].is_string());
    assert!(
        closed["result"]["metadata"]
            .get("manual_acceptance")
            .is_none()
    );
    let reopened = success(root.path(), &["issue", "reopen", key, "--json"]);
    assert_eq!(reopened["result"]["metadata"]["status"], "ready");
    assert!(reopened["result"]["metadata"].get("completed_at").is_none());
    let archived = success(root.path(), &["issue", "archive", key, "--json"]);
    assert_eq!(archived["result"]["metadata"]["archived"], true);
    assert_eq!(archived["result"]["metadata"]["id"], key);
    assert_eq!(archived["result"]["body"], edited["result"]["body"]);
    assert_eq!(
        archived["result"]["metadata"]["acceptance"],
        edited["result"]["metadata"]["acceptance"]
    );
    for field in ["files", "commits", "documents"] {
        assert_eq!(
            archived["result"]["metadata"][field],
            linked["result"]["metadata"][field]
        );
    }
    let comments = success(root.path(), &["issue", "comments", key, "--json"]);
    assert_eq!(comments["result"].as_array().unwrap().len(), 1);
    assert_eq!(
        comments["result"][0]["body"],
        "Reviewed the complete lifecycle"
    );
    assert_eq!(
        success(root.path(), &["issue", "show", key, "--json"])["result"],
        archived["result"]
    );
    assert!(!root.path().join(".agents/workdeck").exists());
}

#[test]
fn explicit_manual_acceptance_records_provenance_replays_and_clears_on_reopen() {
    let root = fixture();
    let created = create(
        root.path(),
        json!({"acceptance":[{"id":"reviewed", "description":"Reviewed declared behavior", "checked":true}]}),
    );
    let key = id(&created);
    let args = [
        "issue",
        "done",
        key,
        "--manual-actor",
        "reviewer@example.test",
        "--manual-reason",
        "I reviewed the described behavior",
        "--request-id",
        "manual-once",
        "--json",
    ];
    let completed = success(root.path(), &args);
    let metadata = &completed["result"]["metadata"];
    assert_eq!(metadata["status"], "done");
    assert_eq!(
        metadata["manual_acceptance"]["actor"],
        "reviewer@example.test"
    );
    assert_eq!(
        metadata["manual_acceptance"]["reason"],
        "I reviewed the described behavior"
    );
    assert_eq!(
        metadata["manual_acceptance"]["accepted_at"],
        metadata["completed_at"]
    );
    assert_eq!(
        metadata["manual_acceptance"]["accepted_at"],
        metadata["updated_at"]
    );
    assert!(metadata.get("evidence").is_none());
    let before = authority(root.path());
    assert_eq!(success(root.path(), &args), completed);
    let report = success(root.path(), &["issue", "done", key, "--dry-run", "--json"]);
    assert_eq!(report["result"]["allowed"], true);
    assert_eq!(report["result"]["basis"], "manual");
    assert_eq!(authority(root.path()), before);
    let reopened = success(root.path(), &["issue", "reopen", key, "--json"]);
    assert!(
        reopened["result"]["metadata"]
            .get("manual_acceptance")
            .is_none()
    );
    assert!(reopened["result"]["metadata"].get("completed_at").is_none());
    let after_reopen = authority(root.path());
    assert_eq!(success(root.path(), &args), completed);
    assert_eq!(authority(root.path()), after_reopen);
    assert_eq!(
        success(root.path(), &["issue", "show", key, "--json"])["result"],
        reopened["result"]
    );
    let report = success(root.path(), &["issue", "done", key, "--dry-run", "--json"]);
    assert_eq!(report["result"]["basis"], "declared");
}

#[test]
fn incomplete_manual_attribution_and_manual_dry_run_never_write_authority() {
    let root = fixture();
    let created = create(root.path(), json!({}));
    let key = id(&created);
    for flags in [
        vec!["--manual-actor", "reviewer"],
        vec!["--manual-reason", "Reviewed behavior"],
        vec![
            "--dry-run",
            "--manual-actor",
            "reviewer",
            "--manual-reason",
            "Reviewed behavior",
        ],
    ] {
        let before = authority(root.path());
        let mut args = vec!["issue", "done", key, "--json"];
        args.extend(flags);
        failure(root.path(), &args, "invalid_input", 2);
        assert_eq!(authority(root.path()), before);
    }
}

#[test]
fn blank_manual_actor_or_reason_never_records_acceptance() {
    let root = fixture();
    let created = create(root.path(), json!({}));
    let key = id(&created);
    for (actor, reason) in [("  ", "Reviewed behavior"), ("reviewer", "  ")] {
        let before = authority(root.path());
        failure(
            root.path(),
            &[
                "issue",
                "done",
                key,
                "--manual-actor",
                actor,
                "--manual-reason",
                reason,
                "--json",
            ],
            "invalid_schema",
            2,
        );
        assert_eq!(authority(root.path()), before);
    }
}

#[test]
fn manual_acceptance_cannot_bypass_unchecked_criteria_or_required_description() {
    for description_required in [false, true] {
        let root = fixture();
        let created = create(
            root.path(),
            if description_required {
                json!({})
            } else {
                json!({"acceptance":[{"id":"not-done", "description":"Still incomplete", "checked":false}]})
            },
        );
        if description_required {
            let path = root.path().join(".workdeck/config.yml");
            let config = fs::read_to_string(&path).unwrap();
            assert!(config.contains("require_description: false"));
            fs::write(
                path,
                config.replace("require_description: false", "require_description: true"),
            )
            .unwrap();
        }
        let key = id(&created);
        let before = authority(root.path());
        let report = success(root.path(), &["issue", "done", key, "--dry-run", "--json"]);
        assert_eq!(report["result"]["allowed"], false);
        for manual in [false, true] {
            let mut args = vec!["issue", "done", key, "--json"];
            if manual {
                args.extend([
                    "--manual-actor",
                    "reviewer",
                    "--manual-reason",
                    "Reviewed behavior",
                ]);
            }
            failure(root.path(), &args, "policy_blocked", 5);
            assert_eq!(authority(root.path()), before);
        }
    }
}

fn declared_completion_never_satisfies_required_policy(policy: &str) {
    let root = fixture();
    let created = create(
        root.path(),
        json!({"acceptance":[{"id":"declared", "description":"Checked baseline criterion", "checked":true}]}),
    );
    require_policy(root.path(), policy);
    let key = id(&created);
    let before = authority(root.path());
    let report = success(root.path(), &["issue", "done", key, "--dry-run", "--json"]);
    assert_eq!(report["result"]["allowed"], false);
    assert_eq!(report["result"]["basis"], "declared");
    assert!(
        report["result"]["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason
                .as_str()
                .unwrap()
                .contains("no authenticated check/profile admission"))
    );
    assert_eq!(authority(root.path()), before);
    for command in ["done", "close"] {
        let error = failure(
            root.path(),
            &["issue", command, key, "--json"],
            "policy_blocked",
            5,
        );
        assert!(
            error["error"]["message"]
                .as_str()
                .unwrap()
                .contains("no authenticated check/profile admission")
        );
        assert_eq!(authority(root.path()), before);
    }
    failure(
        root.path(),
        &[
            "issue",
            "done",
            key,
            "--manual-actor",
            "reviewer",
            "--manual-reason",
            "Reviewed behavior",
            "--json",
        ],
        "policy_blocked",
        5,
    );
    assert_eq!(authority(root.path()), before);
    let shown = success(root.path(), &["issue", "show", key, "--json"]);
    assert_eq!(shown["result"]["metadata"]["status"], "ready");
    assert!(
        shown["result"]["metadata"]
            .get("manual_acceptance")
            .is_none()
    );
    assert!(shown["result"]["metadata"].get("completed_at").is_none());
}

#[test]
fn required_checks_block_close_done_and_manual_acceptance() {
    declared_completion_never_satisfies_required_policy("required_checks");
}

#[test]
fn required_profiles_block_close_done_and_manual_acceptance() {
    declared_completion_never_satisfies_required_policy("required_profiles");
}

#[test]
fn new_policy_invalidates_current_manual_qualification_without_rewriting_history() {
    let root = fixture();
    let created = create(root.path(), json!({}));
    let key = id(&created);
    success(
        root.path(),
        &[
            "issue",
            "done",
            key,
            "--manual-actor",
            "reviewer",
            "--manual-reason",
            "Reviewed the baseline",
            "--json",
        ],
    );
    require_policy(root.path(), "required_checks");
    let before = authority(root.path());
    let report = success(root.path(), &["issue", "done", key, "--dry-run", "--json"]);
    assert_eq!(report["result"]["allowed"], false);
    assert_eq!(report["result"]["basis"], "manual");
    for manual in [false, true] {
        let mut args = vec!["issue", "done", key, "--json"];
        if manual {
            args.extend([
                "--manual-actor",
                "reviewer",
                "--manual-reason",
                "Attempt to accept new checks",
            ]);
        }
        failure(root.path(), &args, "policy_blocked", 5);
        assert_eq!(authority(root.path()), before);
    }
    let shown = success(root.path(), &["issue", "show", key, "--json"]);
    assert_eq!(
        shown["result"]["metadata"]["manual_acceptance"]["reason"],
        "Reviewed the baseline"
    );
}
