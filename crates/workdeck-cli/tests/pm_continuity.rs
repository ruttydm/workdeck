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
fn ok(root: &Path, args: &[&str]) -> Value {
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
fn error(root: &Path, args: &[&str], code: &str) -> Value {
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
fn input(root: &Path, name: &str, value: &Value) {
    fs::write(root.join(name), serde_json::to_vec(value).unwrap()).unwrap();
}

#[test]
fn oversized_diagnostics_are_bounded_and_keep_machine_recovery_contract() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    let issue = ok(root, &["issue", "create", "Bounded diagnostics", "--json"])["result"].clone();
    let mut value = json!({"actor":"agent","body":"Which input?","subjects":[{"subject":{"kind":"issue","reference":issue["metadata"]["id"]},"source":issue["source"]}]});
    value["oversized_unknown_field_".repeat(4000)] = json!(true);
    input(root, "bad.json", &value);
    let output = run(root, &["question", "create", "bad.json", "--json"]);
    assert!(!output.status.success());
    assert!(
        output.stdout.len() <= 16 * 1024,
        "diagnostic output must remain bounded, got {} bytes",
        output.stdout.len()
    );
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "invalid_schema");
    assert_eq!(error["retryable"], false);
    assert!(error["recovery_actions"].is_array());
    assert_eq!(error["diagnostic_truncated"], true);
    assert_eq!(
        ok(root, &["question", "list", "--json"])["result"]["total"],
        0
    );
}

#[test]
fn questions_retain_inspected_sources_and_answers_do_not_rewrite_requirements() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    let issue = ok(root, &["issue", "create", "Clarify scope", "--json"])["result"].clone();
    let id = issue["metadata"]["id"].as_str().unwrap();
    input(
        root,
        "question.json",
        &json!({"actor":"agent","body":"Which source format is required?","subjects":[{"subject":{"kind":"issue","reference":id},"source":issue["source"]}],"blocks_work":true}),
    );
    let created = ok(
        root,
        &[
            "question",
            "create",
            "question.json",
            "--request-id",
            "question-1",
            "--json",
            "--no-input",
        ],
    );
    let question = &created["result"]["question"];
    let qid = question["metadata"]["id"].as_str().unwrap();
    assert_eq!(
        ok(root, &["question", "show", qid, "--json"])["result"],
        *question
    );
    let applicability = ok(root, &["question", "applicability", qid, "--json"]);
    assert_eq!(applicability["result"]["blocks_implementation"], true);
    fs::write(root.join("answer.md"), "Use the current schema.").unwrap();
    error(
        root,
        &[
            "question",
            "answer",
            qid,
            "--actor",
            "owner",
            "--body-file",
            "answer.md",
            "--json",
        ],
        "invalid_input",
    );
    let revision = question["source"]["revision"].to_string();
    let content = question["source"]["content"].as_str().unwrap();
    let args = [
        "question",
        "answer",
        qid,
        "--actor",
        "owner",
        "--body-file",
        "answer.md",
        "--expected-revision",
        &revision,
        "--expected-content",
        content,
        "--request-id",
        "answer-1",
        "--json",
    ];
    let answered = ok(root, &args);
    assert_eq!(
        answered["result"]["question"]["metadata"]["state"],
        "answered"
    );
    assert_eq!(ok(root, &args)["receipt"], answered["receipt"]);
    assert_eq!(ok(root, &["issue", "show", id, "--json"])["result"], issue);
    ok(
        root,
        &["issue", "update", id, "--title", "Changed scope", "--json"],
    );
    assert_eq!(
        ok(root, &["question", "applicability", qid, "--json"])["result"]["freshness"],
        "stale"
    );
    assert_eq!(
        ok(
            root,
            &[
                "question",
                "create",
                "question.json",
                "--request-id",
                "question-1",
                "--json"
            ]
        )["receipt"],
        created["receipt"]
    );
}

#[test]
fn handoff_resumption_keeps_declarations_and_exposes_a_changed_context_basis() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    let issue = ok(root, &["issue", "create", "Resume this task", "--json"])["result"].clone();
    let id = issue["metadata"]["id"].as_str().unwrap();
    let packet = ok(
        root,
        &["context", "--issue", id, "--budget", "16384", "--json"],
    )["result"]
        .clone();
    input(
        root,
        "handoff.json",
        &json!({"actor":"agent","anchor":packet["anchor"],"body":"Inspected the task; implementation is pending.","attempted":["Read requirements"],"uncertainties":["No verification has run"],"next_steps":["Implement the bounded change"]}),
    );
    let created = ok(
        root,
        &[
            "handoff",
            "create",
            "handoff.json",
            "--request-id",
            "handoff-1",
            "--json",
        ],
    );
    let hid = created["result"]["metadata"]["id"].as_str().unwrap();
    assert_eq!(
        ok(root, &["handoff", "show", hid, "--issue", id, "--json"])["result"],
        created["result"]
    );
    assert_eq!(
        ok(root, &["handoff", "list", "--issue", id, "--json"])["result"]["total"],
        1
    );
    ok(
        root,
        &[
            "issue",
            "update",
            id,
            "--title",
            "New task contract",
            "--json",
        ],
    );
    error(
        root,
        &[
            "handoff",
            "create",
            "handoff.json",
            "--request-id",
            "handoff-2",
            "--json",
        ],
        "stale_source",
    );
    assert_eq!(
        ok(
            root,
            &[
                "handoff",
                "create",
                "handoff.json",
                "--request-id",
                "handoff-1",
                "--json"
            ]
        )["receipt"],
        created["receipt"]
    );
    let resumed = ok(
        root,
        &["context", "--issue", id, "--budget", "16384", "--json"],
    );
    assert_ne!(
        resumed["result"]["anchor"]["fingerprint"],
        packet["anchor"]["fingerprint"]
    );
    assert!(
        resumed.to_string().contains("stale"),
        "old handoff must remain visibly stale: {resumed}"
    );
}

#[test]
fn question_pages_reject_changed_queries_and_sources_and_projected_writes_keep_receipts() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    let issue = ok(root, &["issue", "create", "Page questions", "--json"])["result"].clone();
    let id = issue["metadata"]["id"].as_str().unwrap();
    let mut question = Value::Null;
    for index in 0..3 {
        input(
            root,
            "question.json",
            &json!({"actor":"agent","body":format!("Question {index}"),"subjects":[{"subject":{"kind":"issue","reference":id},"source":issue["source"]}]}),
        );
        let created = ok(
            root,
            &[
                "question",
                "create",
                "question.json",
                "--request-id",
                &format!("question-{index}"),
                "--fields",
                "question.metadata.id",
                "--no-input",
            ],
        );
        assert_eq!(
            created["source"]["repository"],
            created["receipt"]["repository"]
        );
        assert_eq!(
            created["result"]["question"]["metadata"]["id"],
            created["receipt"]["result"]["question"]["metadata"]["id"]
        );
        assert!(created["result"]["question"].get("body").is_none());
        question = created["receipt"]["result"]["question"].clone();
    }
    let first = ok(
        root,
        &[
            "question",
            "list",
            "--limit",
            "1",
            "--compact",
            "--no-input",
        ],
    );
    assert_eq!(first["result"]["total"], 3);
    assert_eq!(first["result"]["records"].as_array().unwrap().len(), 1);
    let cursor = first["result"]["next_cursor"].to_string();
    let second = ok(
        root,
        &[
            "question", "list", "--limit", "1", "--cursor", &cursor, "--json",
        ],
    );
    assert_ne!(
        first["result"]["records"][0]["metadata"]["id"],
        second["result"]["records"][0]["metadata"]["id"]
    );
    error(
        root,
        &[
            "question", "list", "--limit", "2", "--cursor", &cursor, "--json",
        ],
        "stale_source",
    );
    fs::write(root.join("answer.md"), "Explicit decision").unwrap();
    let qid = question["metadata"]["id"].as_str().unwrap();
    ok(
        root,
        &[
            "question",
            "answer",
            qid,
            "--actor",
            "owner",
            "--body-file",
            "answer.md",
            "--expected-revision",
            &question["source"]["revision"].to_string(),
            "--expected-content",
            question["source"]["content"].as_str().unwrap(),
            "--request-id",
            "answer-page",
            "--json",
        ],
    );
    error(
        root,
        &[
            "question", "list", "--limit", "1", "--cursor", &cursor, "--json",
        ],
        "stale_source",
    );
    error(
        root,
        &["question", "show", qid, "--limit", "1", "--json"],
        "invalid_input",
    );
}

#[test]
fn bounded_post_commit_projection_error_preserves_request_reconciliation() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    workdeck_pm::Repository::init(root, "WD").unwrap();
    let issue = ok(root, &["issue", "create", "Reconcile response", "--json"])["result"].clone();
    input(
        root,
        "question.json",
        &json!({"actor":"agent","body":"Long question detail. ".repeat(1500),"subjects":[{"subject":{"kind":"issue","reference":issue["metadata"]["id"]},"source":issue["source"]}]}),
    );
    let output = run(
        root,
        &[
            "question",
            "create",
            "question.json",
            "--request-id",
            "projection-reconcile",
            "--fields",
            "nonexistent",
            "--no-input",
        ],
    );
    assert!(!output.status.success());
    assert!(output.stdout.len() <= 16 * 1024);
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failure["error"]["details"]["mutation_committed"], true);
    assert_eq!(
        failure["error"]["details"]["receipt"]["request_id"],
        "projection-reconcile"
    );
    assert_eq!(
        failure["error"]["details"]["receipt"]["full_receipt_omitted"],
        true
    );
    let replay = ok(
        root,
        &[
            "question",
            "create",
            "question.json",
            "--request-id",
            "projection-reconcile",
            "--json",
            "--no-input",
        ],
    );
    assert_eq!(
        replay["receipt"]["operation_id"],
        failure["error"]["details"]["receipt"]["operation_id"]
    );
    assert_eq!(
        ok(root, &["question", "list", "--json"])["result"]["total"],
        1
    );
}
