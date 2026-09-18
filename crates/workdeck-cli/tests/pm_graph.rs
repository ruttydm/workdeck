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
fn failure(root: &Path, args: &[&str], code: &str) -> Value {
    let output = run(root, args);
    assert!(!output.status.success(), "{args:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{error}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}
fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    root
}
fn issue(root: &Path, title: &str) -> String {
    success(root, &["issue", "create", title, "--json"])["result"]["metadata"]["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn graph_commands_explain_edges_and_keep_soft_links_out_of_readiness() {
    let root = fixture();
    let root = root.path();
    let parent = issue(root, "Parent");
    let child = issue(root, "Child");
    let prerequisite = issue(root, "Prerequisite");
    let related = issue(root, "Related");
    success(root, &["issue", "parent", &child, &parent, "--json"]);
    success(
        root,
        &[
            "issue",
            "prerequisite",
            &child,
            "add",
            &prerequisite,
            "--json",
        ],
    );
    let child_before = success(root, &["issue", "show", &child, "--json"]);
    success(root, &["issue", "relate", &child, &related, "--json"]);
    assert_eq!(
        success(root, &["issue", "show", &child, "--json"])["result"]["source"],
        child_before["result"]["source"]
    );
    let relations = success(root, &["issue", "relations", &child, "--json"]);
    assert_eq!(relations["result"]["parent"], parent);
    assert_eq!(relations["result"]["prerequisites"], json!([prerequisite]));
    assert_eq!(relations["result"]["related"], json!([related]));
    assert_eq!(
        success(root, &["issue", "relations", &parent, "--json"])["result"]["children"],
        json!([child])
    );
    assert_eq!(
        success(root, &["issue", "relations", &related, "--json"])["result"]["related"],
        json!([child])
    );
    assert_eq!(
        success(root, &["issue", "ready", &related, "--json"])["result"]["ready"],
        true
    );
    let readiness = success(root, &["issue", "ready", &child, "--json"]);
    assert_eq!(readiness["result"]["ready"], false);
    assert!(
        readiness["result"]["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["reason_code"] == "incomplete_prerequisite")
    );
    assert_eq!(
        success(
            root,
            &["issue", "dependency-path", &child, &prerequisite, "--json"]
        )["result"]["path"],
        json!([child, prerequisite])
    );
    assert_eq!(
        success(
            root,
            &["issue", "dependency-path", &child, &related, "--json"]
        )["result"]["found"],
        false
    );
    failure(
        root,
        &[
            "issue",
            "prerequisite",
            &prerequisite,
            "add",
            &child,
            "--json",
        ],
        "policy_blocked",
    );
    success(root, &["issue", "unrelate", &related, &child, "--json"]);
    assert_eq!(
        success(root, &["issue", "relations", &child, "--json"])["result"]["related"],
        json!([])
    );
    success(root, &["issue", "parent", &child, "--clear", "--json"]);
    assert!(success(root, &["issue", "relations", &child, "--json"])["result"]["parent"].is_null());
}

#[test]
fn graph_mutations_bind_reviewed_graph_and_replay_original_intent() {
    let root = fixture();
    let root = root.path();
    let a = issue(root, "A");
    let b = issue(root, "B");
    let c = issue(root, "C");
    let inspected = success(root, &["issue", "relations", &a, "--json"]);
    let fingerprint = inspected["result"]["fingerprint"].as_str().unwrap();
    let revision = inspected["result"]["source"]["revision"].to_string();
    let content = inspected["result"]["source"]["content"].as_str().unwrap();
    let args = [
        "issue",
        "parent",
        &a,
        &b,
        "--expected-graph",
        fingerprint,
        "--expected-revision",
        &revision,
        "--expected-content",
        content,
        "--request-id",
        "parent-once",
        "--json",
    ];
    let committed = success(root, &args);
    success(root, &["issue", "parent", &a, &c, "--json"]);
    assert_eq!(success(root, &args)["result"], committed["result"]);
    assert_eq!(
        success(root, &["issue", "relations", &a, "--json"])["result"]["parent"],
        c
    );
    failure(
        root,
        &[
            "issue",
            "relate",
            &a,
            &b,
            "--expected-graph",
            fingerprint,
            "--json",
        ],
        "stale_source",
    );
    failure(
        root,
        &[
            "issue",
            "parent",
            &a,
            &c,
            "--request-id",
            "parent-once",
            "--json",
        ],
        "idempotency_conflict",
    );
}

#[test]
fn canceled_prerequisites_require_explicit_reasoned_resolution() {
    let root = fixture();
    let root = root.path();
    let a = issue(root, "A");
    let b = issue(root, "B");
    let c = issue(root, "C");
    success(root, &["issue", "prerequisite", &a, "add", &b, "--json"]);
    success(root, &["issue", "cancel", &b, "--json"]);
    let ready = success(root, &["issue", "ready", &a, "--json"]);
    assert_eq!(ready["result"]["ready"], false);
    assert!(
        ready["result"]["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["reason_code"] == "canceled_requirement")
    );
    failure(
        root,
        &[
            "issue",
            "prerequisite",
            &a,
            "waive",
            &b,
            "--actor",
            "agent",
            "--reason",
            "Reviewed",
            "--json",
        ],
        "policy_blocked",
    );
    failure(
        root,
        &[
            "issue",
            "prerequisite",
            &a,
            "remove",
            &b,
            "--reason",
            "",
            "--json",
        ],
        "invalid_input",
    );
    success(
        root,
        &[
            "issue",
            "prerequisite",
            &a,
            "replace",
            &b,
            &c,
            "--reason",
            "Replacement implements the requirement",
            "--json",
        ],
    );
    assert_eq!(
        success(root, &["issue", "relations", &a, "--json"])["result"]["prerequisites"],
        json!([c])
    );
    success(
        root,
        &[
            "issue",
            "prerequisite",
            &a,
            "remove",
            &c,
            "--reason",
            "Requirement withdrawn after review",
            "--json",
        ],
    );
    assert_eq!(
        success(root, &["issue", "ready", &a, "--json"])["result"]["ready"],
        true
    );
}

#[test]
fn graph_reads_do_not_initialize_or_accept_mutation_flags() {
    let root = tempfile::tempdir().unwrap();
    failure(
        root.path(),
        &["issue", "relations", "WD-1", "--json"],
        "not_initialized",
    );
    assert!(!root.path().join(".workdeck").exists());
    assert!(!root.path().join(".agents").exists());
    let root = fixture();
    let root = root.path();
    let a = issue(root, "A");
    failure(
        root,
        &[
            "issue",
            "ready",
            &a,
            "--request-id",
            "not-a-write",
            "--json",
        ],
        "invalid_input",
    );
    let orphan = tempfile::tempdir().unwrap();
    fs::create_dir_all(orphan.path().join(".workdeck/relations/issues")).unwrap();
    fs::write(
        orphan
            .path()
            .join(".workdeck/relations/issues/existing.yml"),
        "orphaned graph",
    )
    .unwrap();
    assert!(!run(orphan.path(), &["init", "--json"]).status.success());
    assert!(!orphan.path().join(".workdeck/config.yml").exists());
}
