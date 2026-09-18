use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{fs, path::Path, process::Command};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}
fn success(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    value
}
fn failure(root: &Path, args: &[&str], code: &str) {
    let output = run(root, args);
    assert!(!output.status.success(), "{args:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["error"]["code"], code, "{value}");
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    root
}
fn write(root: &Path, path: &str, value: Value) {
    fs::write(root.join(path), serde_json::to_vec(&value).unwrap()).unwrap();
}
fn feature(root: &Path, name: &str) -> String {
    success(root, &["feature", "create", name, "--json"])["result"]["record"]["metadata"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn feature_authoring_preserves_identity_metadata_and_replays_original_move() {
    let root = fixture();
    let root = root.path();
    write(
        root,
        "feature.json",
        json!({"name":"Capability", "body":"# Scope\n\nExact Markdown\n", "fields":{"decision":"accepted", "maturity":"specified", "availability":"experimental", "custom":{"unknown":{"keep":true}}, "x-owner-note":"Keep"}}),
    );
    let created = success(
        root,
        &[
            "feature",
            "create",
            "--from-json",
            "feature.json",
            "--request-id",
            "feature-once",
            "--json",
        ],
    );
    let record = &created["result"]["record"];
    let id = record["metadata"]["id"].as_str().unwrap();
    let content = record["source"]["content"].as_str().unwrap();
    let revision = record["source"]["revision"].to_string();
    let moved = success(
        root,
        &[
            "feature",
            "move",
            id,
            "platform/auth",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--request-id",
            "move-once",
            "--json",
        ],
    );
    let original_path = record["path"].as_str().unwrap();
    assert!(!root.join(".workdeck").join(original_path).exists());
    assert_eq!(
        moved["result"]["record"]["path"],
        format!("features/platform/auth/{id}.md")
    );
    success(
        root,
        &[
            "feature",
            "update",
            id,
            "--name",
            "Renamed capability",
            "--json",
        ],
    );
    assert_eq!(
        success(
            root,
            &[
                "feature",
                "move",
                id,
                "platform/auth",
                "--expected-revision",
                &revision,
                "--expected-content",
                content,
                "--request-id",
                "move-once",
                "--json"
            ]
        )["result"],
        moved["result"]
    );
    let shown = success(root, &["feature", "show", id, "--json"]);
    assert_eq!(shown["result"]["metadata"]["name"], "Renamed capability");
    assert_eq!(shown["result"]["metadata"]["id"], id);
    assert_eq!(
        shown["result"]["metadata"]["custom"]["unknown"]["keep"],
        true
    );
    assert_eq!(shown["result"]["metadata"]["x-owner-note"], "Keep");
    assert_eq!(shown["result"]["body"], record["body"]);
    failure(
        root,
        &[
            "feature",
            "archive",
            id,
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
        "stale_source",
    );
    success(
        root,
        &[
            "feature",
            "custom",
            id,
            "--set",
            "team=\"platform\"",
            "--json",
        ],
    );
    success(root, &["feature", "archive", id, "--json"]);
    assert_eq!(
        success(root, &["feature", "show", id, "--json"])["result"]["metadata"]["archived"],
        true
    );
    success(root, &["feature", "archive", id, "--restore", "--json"]);
    assert_eq!(
        success(root, &["feature", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn feature_coverage_is_many_to_many_and_completion_does_not_promote_maturity() {
    let root = fixture();
    let root = root.path();
    let a = feature(root, "A");
    let b = feature(root, "B");
    write(
        root,
        "issue.json",
        json!({"title":"Implement both", "fields":{"features":[a,b]}}),
    );
    let issue = success(
        root,
        &["issue", "create", "--from-json", "issue.json", "--json"],
    );
    let issue_id = issue["result"]["metadata"]["id"].as_str().unwrap();
    for id in [&a, &b] {
        let coverage = success(root, &["feature", "coverage", id, "--json"]);
        assert_eq!(coverage["result"]["issues"][0]["metadata"]["id"], issue_id);
        assert_eq!(
            coverage["result"]["feature"]["metadata"]["maturity"],
            "draft"
        );
    }
    success(root, &["issue", "done", issue_id, "--json"]);
    for id in [&a, &b] {
        let coverage = success(root, &["feature", "coverage", id, "--json"]);
        assert_eq!(
            coverage["result"]["issues"][0]["metadata"]["status"],
            "done"
        );
        assert_eq!(
            coverage["result"]["feature"]["metadata"]["maturity"],
            "draft"
        );
    }
    success(root, &["feature", "parent", &b, &a, "--json"]);
    assert_eq!(
        success(root, &["feature", "coverage", &a, "--json"])["result"]["children"][0]["metadata"]
            ["id"],
        b
    );
    success(root, &["feature", "parent", &b, "--clear", "--json"]);
    let source = success(root, &["feature", "show", &a, "--json"])["result"]["source"].clone();
    success(root, &["feature", "relate", &a, &b, "--json"]);
    assert_eq!(
        success(root, &["feature", "show", &a, "--json"])["result"]["source"],
        source
    );
    assert_eq!(
        success(root, &["feature", "coverage", &b, "--json"])["result"]["related"][0]["metadata"]["id"],
        a
    );
    success(root, &["feature", "unrelate", &b, &a, "--json"]);
    assert_eq!(
        success(root, &["feature", "coverage", &a, "--json"])["result"]["related"],
        json!([])
    );
    failure(
        root,
        &["issue", "update", issue_id, "--feature", &a, "--json"],
        "policy_blocked",
    );
    success(root, &["issue", "reopen", issue_id, "--json"]);
    success(
        root,
        &["issue", "update", issue_id, "--feature", &a, "--json"],
    );
    assert_eq!(
        success(root, &["feature", "coverage", &b, "--json"])["result"]["issues"],
        json!([])
    );
    success(
        root,
        &["issue", "update", issue_id, "--clear", "features", "--json"],
    );
    assert_eq!(
        success(root, &["feature", "coverage", &a, "--json"])["result"]["issues"],
        json!([])
    );
    assert!(
        success(root, &["feature", "show", &b, "--json"])["result"]["metadata"]["parent"].is_null()
    );
}

#[test]
fn feature_reads_require_native_source_and_mutation_inputs_are_explicit() {
    let empty = tempfile::tempdir().unwrap();
    failure(
        empty.path(),
        &["feature", "list", "--json"],
        "not_initialized",
    );
    assert!(!empty.path().join(".workdeck").exists());
    assert!(!empty.path().join(".agents").exists());
    let root = fixture();
    let root = root.path();
    let id = feature(root, "Keep");
    failure(
        root,
        &["feature", "show", &id, "--stage", "--json"],
        "invalid_input",
    );
    failure(root, &["feature", "update", &id, "--json"], "invalid_input");
    failure(
        root,
        &["feature", "move", &id, "../../outside", "--json"],
        "unsafe_path",
    );
    write(root, "fields.json", json!({"maturity":"verified"}));
    assert!(
        !run(
            root,
            &[
                "feature",
                "update",
                &id,
                "--fields-file",
                "fields.json",
                "--json"
            ]
        )
        .status
        .success()
    );
    assert_eq!(
        success(root, &["feature", "show", &id, "--json"])["result"]["metadata"]["maturity"],
        "draft"
    );
}

#[test]
fn feature_retirement_previews_incoming_records_and_preserves_reserved_identity() {
    let root = fixture();
    let root = root.path();
    let parent = feature(root, "Parent");
    let child = feature(root, "Child");
    let before_link = success(root, &["feature", "delete", &parent, "--dry-run", "--json"]);
    success(root, &["feature", "parent", &child, &parent, "--json"]);
    failure(
        root,
        &[
            "feature",
            "delete",
            &parent,
            "--yes",
            "--expected-preview",
            before_link["result"]["fingerprint"].as_str().unwrap(),
            "--json",
        ],
        "stale_source",
    );
    for flag in ["--stage", "--request-id"] {
        let mut args = vec!["feature", "delete", &parent, "--dry-run", flag];
        if flag == "--request-id" {
            args.push("preview-is-not-a-write");
        }
        args.push("--json");
        failure(root, &args, "invalid_input");
    }
    let blocked = success(root, &["feature", "delete", &parent, "--dry-run", "--json"]);
    assert_eq!(blocked["result"]["allowed"], false);
    assert_eq!(blocked["result"]["record_blockers"][0]["id"], child);
    assert!(
        !run(root, &["feature", "delete", &parent, "--yes", "--json"])
            .status
            .success()
    );
    success(root, &["feature", "parent", &child, "--clear", "--json"]);
    let preview = success(root, &["feature", "delete", &parent, "--dry-run", "--json"]);
    assert_eq!(preview["result"]["allowed"], true);
    let fingerprint = preview["result"]["fingerprint"].as_str().unwrap();
    let args = [
        "feature",
        "delete",
        &parent,
        "--yes",
        "--expected-preview",
        fingerprint,
        "--request-id",
        "retire-feature",
        "--json",
    ];
    let retired = success(root, &args);
    assert_eq!(success(root, &args)["result"], retired["result"]);
    let shown = success(root, &["feature", "show", &parent, "--json"]);
    assert!(shown["result"]["retirement"].is_object());
    assert_eq!(shown["result"]["metadata"]["id"], parent);
    assert!(
        root.join(".workdeck")
            .join(shown["result"]["path"].as_str().unwrap())
            .exists()
    );
    assert!(
        !run(
            root,
            &["feature", "archive", &parent, "--restore", "--json"]
        )
        .status
        .success()
    );
}

#[test]
fn feature_retirement_staging_failure_preserves_receipt_and_retries_exact_paths() {
    let root = fixture();
    let root = root.path();
    let git = |args: &[&str]| {
        let output = Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    };
    git(&["init", "--quiet"]);
    fs::write(root.join("unrelated.txt"), "already staged\n").unwrap();
    git(&["add", "unrelated.txt"]);
    fs::write(root.join("unrelated.txt"), "later unstaged edit\n").unwrap();
    let id = feature(root, "Retire with staging");
    let args = [
        "feature",
        "delete",
        &id,
        "--yes",
        "--stage",
        "--request-id",
        "stage-retire-feature",
        "--json",
    ];
    fs::write(root.join(".git/index.lock"), "held by controlled fixture\n").unwrap();
    let failed = run(root, &args);
    assert!(!failed.status.success());
    let failed: Value = serde_json::from_slice(&failed.stdout).unwrap();
    assert_eq!(
        failed["error"]["details"]["mutation_committed"], true,
        "{failed}"
    );
    let receipt = failed["error"]["details"]["receipt"].clone();
    fs::remove_file(root.join(".git/index.lock")).unwrap();
    let retried = success(root, &args);
    assert_eq!(retried["receipt"], receipt);
    let staged = git(&["diff", "--cached", "--name-only"]);
    assert!(staged.contains(&format!(".workdeck/features/{id}.md")));
    assert!(staged.contains(&format!(".workdeck/tombstones/features/{id}.yml")));
    assert!(!staged.contains(".workdeck/config.yml"));
    assert_eq!(git(&["show", ":unrelated.txt"]), "already staged\n");
    assert_eq!(
        fs::read_to_string(root.join("unrelated.txt")).unwrap(),
        "later unstaged edit\n"
    );
}

#[test]
fn feature_maturity_assessment_and_promotion_are_explicit_and_source_bound() {
    let root = fixture();
    let root = root.path();
    let id = feature(root, "Audited capability");
    write(
        root,
        "feature-fields.json",
        json!({
            "decision": "accepted",
            "criteria": [{"id": "verified", "description": "The capability works"}]
        }),
    );
    success(
        root,
        &[
            "feature",
            "update",
            &id,
            "--fields-file",
            "feature-fields.json",
            "--json",
        ],
    );
    write(
        root,
        "feature-issue.json",
        json!({"title":"Implement capability", "fields":{"features":[id]}}),
    );
    let issue = success(
        root,
        &[
            "issue",
            "create",
            "--from-json",
            "feature-issue.json",
            "--json",
        ],
    );
    let issue_id = issue["result"]["metadata"]["id"].as_str().unwrap();

    let initial = success(
        root,
        &["feature", "assess", &id, "--to", "specified", "--json"],
    );
    assert_eq!(initial["result"]["allowed"], false);
    assert_eq!(initial["result"]["current"], "draft");
    assert_eq!(initial["result"]["requested"], "specified");
    assert_eq!(initial["result"]["basis"], "declared");

    let shown = success(root, &["feature", "show", &id, "--json"]);
    let revision = shown["result"]["source"]["revision"].to_string();
    let content = shown["result"]["source"]["content"].as_str().unwrap();
    let promoted = success(
        root,
        &[
            "feature",
            "promote",
            &id,
            "--to",
            "specified",
            "--actor",
            "local",
            "--reason",
            "Accepted the capability criteria",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
    );
    assert_eq!(
        promoted["result"]["record"]["metadata"]["maturity"],
        "specified"
    );
    assert!(promoted["receipt"]["request_id"].is_string());

    let blocked = success(
        root,
        &["feature", "assess", &id, "--to", "implemented", "--json"],
    );
    assert_eq!(blocked["result"]["allowed"], false);
    assert!(
        blocked["result"]["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|condition| condition["reason_code"] == "issue_incomplete")
    );

    success(root, &["issue", "done", issue_id, "--json"]);
    let ready = success(
        root,
        &["feature", "assess", &id, "--to", "implemented", "--json"],
    );
    assert_eq!(ready["result"]["allowed"], false);
    assert_eq!(ready["result"]["basis"], "declared");
    let shown = success(root, &["feature", "show", &id, "--json"]);
    let revision = shown["result"]["source"]["revision"].to_string();
    let content = shown["result"]["source"]["content"].as_str().unwrap();
    let completed = success(
        root,
        &[
            "feature",
            "promote",
            &id,
            "--to",
            "implemented",
            "--actor",
            "local",
            "--reason",
            "Accepted the completed implementation",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
    );
    assert_eq!(
        completed["result"]["record"]["metadata"]["maturity"],
        "implemented"
    );
}
