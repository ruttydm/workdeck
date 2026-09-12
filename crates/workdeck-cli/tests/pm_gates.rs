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
    let value: Value = serde_json::from_slice(&output.stdout).unwrap_or_else(|e| {
        panic!(
            "{e}: {} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    });
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}
fn write(root: &Path, path: &str, value: &Value) {
    fs::write(root.join(path), serde_json::to_vec(value).unwrap()).unwrap();
}
fn hash(value: &str) -> Value {
    json!(workdeck_pm::ContentHash::of(value.as_bytes()))
}
struct Fixture {
    root: tempfile::TempDir,
    issue: String,
    gate: String,
    declaration: Value,
}
fn fixture() -> Fixture {
    let root = tempfile::tempdir().unwrap();
    let repo = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    write(
        root.path(),
        "issue.json",
        &json!({"title":"Qualify behavior", "fields":{"acceptance":[{"id":"behavior", "description":"Behavior is demonstrated", "checked":true}]}}),
    );
    let issue = success(
        root.path(),
        &["issue", "create", "--from-json", "issue.json", "--json"],
    )["result"]["metadata"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let criterion = success(
        root.path(),
        &["gate", "criterion", "issue", &issue, "behavior", "--json"],
    )["result"]["reference"]
        .clone();
    let producer = json!({"id":"local-test", "definition":hash("producer")});
    let check = json!({"id":"behavior", "definition":hash("check")});
    write(
        root.path(),
        "gate.json",
        &json!({"name":"Behavior gate", "requirements":[{"id":"behavior", "criterion":criterion, "producer":producer, "check":check, "max_age_seconds":3600}]}),
    );
    let gate = success(
        root.path(),
        &[
            "gate",
            "create",
            "gate.json",
            "--request-id",
            "gate-once",
            "--json",
        ],
    )["result"]["gate"]["definition"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let subject =
        json!({"repository":repo.identity(), "kind":"source", "content":hash("exact-source")});
    write(root.path(), "subject.json", &subject);
    let declaration = json!({"criterion":criterion, "subject":subject, "producer":producer, "check":check,
        "result":{"id":"result-one", "content":hash("report")}, "observed_at":chrono::Utc::now(),
        "provenance":{"actor":"test-agent", "reason":"Declared external report"}, "links":[], "custom":{"retained":true}});
    write(root.path(), "evidence.json", &declaration);
    Fixture {
        root,
        issue,
        gate,
        declaration,
    }
}

#[test]
fn declared_evidence_has_provenance_but_cannot_satisfy_gate_or_issue_completion() {
    let fixture = fixture();
    let root = fixture.root.path();
    let as_of = (chrono::Utc::now() + chrono::Duration::seconds(60)).to_rfc3339();
    let assess = [
        "gate",
        "assess",
        &fixture.gate,
        "--subject-file",
        "subject.json",
        "--as-of",
        &as_of,
        "--json",
    ];
    let missing = success(root, &assess);
    assert_eq!(missing["result"]["state"], "unknown");
    assert!(
        missing["result"]["requirements"][0]["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["reason_code"] == "evidence_missing")
    );
    let declared = success(
        root,
        &[
            "evidence",
            "declare",
            "evidence.json",
            "--request-id",
            "evidence-once",
            "--json",
        ],
    );
    assert_eq!(
        declared["result"]["reference"]["provenance_kind"],
        "declared"
    );
    let assessed = success(root, &assess);
    assert_eq!(assessed["result"]["state"], "unknown");
    assert_eq!(assessed["result"]["subject_origin"], "caller_declared");
    assert!(
        assessed["result"]["requirements"][0]["conditions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["reason_code"] == "evidence_declared_only")
    );
    assert_ne!(
        missing["result"]["fingerprint"],
        assessed["result"]["fingerprint"]
    );
    write(
        root,
        "issue-update.json",
        &json!({"fields":{"gates":[fixture.gate]}}),
    );
    success(
        root,
        &[
            "issue",
            "update",
            &fixture.issue,
            "--from-json",
            "issue-update.json",
            "--json",
        ],
    );
    failure(
        root,
        &["issue", "done", &fixture.issue, "--json"],
        "policy_blocked",
    );
    let mut forged = fixture.declaration.clone();
    forged["verified"] = json!(true);
    write(root, "forged.json", &forged);
    assert!(
        !run(root, &["evidence", "declare", "forged.json", "--json"])
            .status
            .success()
    );
    assert_eq!(
        success(root, &["evidence", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn evidence_supersession_preserves_history_and_request_identity() {
    let fixture = fixture();
    let root = fixture.root.path();
    let first_args = [
        "evidence",
        "declare",
        "evidence.json",
        "--request-id",
        "evidence-once",
        "--json",
    ];
    let first = success(root, &first_args);
    let id = first["result"]["reference"]["id"].as_str().unwrap();
    let content = first["result"]["content"].as_str().unwrap();
    let args = [
        "evidence",
        "supersede",
        id,
        "evidence.json",
        "--expected-evidence-content",
        content,
        "--reason",
        "Corrected attribution",
        "--request-id",
        "amend-once",
        "--json",
    ];
    let amended = success(root, &args);
    assert_ne!(amended["result"]["reference"]["id"], id);
    assert_eq!(
        amended["result"]["reference"]["declaration"]["supersedes"]["id"],
        id
    );
    assert_eq!(success(root, &args)["result"], amended["result"]);
    assert_eq!(success(root, &first_args)["result"], first["result"]);
    assert_eq!(
        success(root, &["evidence", "show", id, "--json"])["result"],
        first["result"]
    );
    assert_eq!(
        success(root, &["evidence", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    write(root, "query.json", &json!({"include_superseded":true}));
    assert_eq!(
        success(
            root,
            &["evidence", "list", "--query-file", "query.json", "--json"]
        )["result"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert!(
        !run(
            root,
            &[
                "evidence",
                "supersede",
                id,
                "evidence.json",
                "--expected-evidence-content",
                content,
                "--reason",
                "Attempted fork",
                "--json"
            ]
        )
        .status
        .success()
    );
}

#[test]
fn gate_changes_require_exact_inspected_source_and_replay_after_newer_changes() {
    let fixture = fixture();
    let root = fixture.root.path();
    let shown = success(root, &["gate", "show", &fixture.gate, "--json"]);
    let content = shown["result"]["source"]["content"].as_str().unwrap();
    let revision = shown["result"]["source"]["revision"].to_string();
    write(
        root,
        "patch.json",
        &json!({"name":"Renamed gate", "custom":{"unknown":"retain"}}),
    );
    failure(
        root,
        &["gate", "update", &fixture.gate, "patch.json", "--json"],
        "invalid_input",
    );
    let args = [
        "gate",
        "update",
        &fixture.gate,
        "patch.json",
        "--expected-revision",
        &revision,
        "--expected-content",
        content,
        "--request-id",
        "update-once",
        "--json",
    ];
    let updated = success(root, &args);
    failure(
        root,
        &[
            "gate",
            "archive",
            &fixture.gate,
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
        "stale_source",
    );
    let next = &updated["result"]["gate"]["source"];
    success(
        root,
        &[
            "gate",
            "archive",
            &fixture.gate,
            "--expected-revision",
            &next["revision"].to_string(),
            "--expected-content",
            next["content"].as_str().unwrap(),
            "--json",
        ],
    );
    assert_eq!(success(root, &args)["result"], updated["result"]);
    assert_eq!(
        success(root, &["gate", "show", &fixture.gate, "--json"])["result"]["definition"]["archived"],
        true
    );
}

#[test]
fn gate_retirement_checks_incoming_issue_references_and_keeps_history() {
    let fixture = fixture();
    let root = fixture.root.path();
    success(
        root,
        &[
            "issue",
            "update",
            &fixture.issue,
            "--gate",
            &fixture.gate,
            "--json",
        ],
    );
    let blocked = success(
        root,
        &["gate", "delete", &fixture.gate, "--dry-run", "--json"],
    );
    assert_eq!(blocked["result"]["allowed"], false);
    assert_eq!(blocked["result"]["blockers"][0]["issue"], fixture.issue);
    assert!(
        !run(root, &["gate", "delete", &fixture.gate, "--yes", "--json"])
            .status
            .success()
    );
    success(
        root,
        &[
            "issue",
            "update",
            &fixture.issue,
            "--clear",
            "gates",
            "--json",
        ],
    );
    let preview = success(
        root,
        &["gate", "delete", &fixture.gate, "--dry-run", "--json"],
    );
    assert_eq!(preview["result"]["allowed"], true);
    let fingerprint = preview["result"]["fingerprint"].as_str().unwrap();
    let args = [
        "gate",
        "delete",
        &fixture.gate,
        "--yes",
        "--expected-preview",
        fingerprint,
        "--request-id",
        "retire-gate",
        "--json",
    ];
    let retired = success(root, &args);
    assert_eq!(success(root, &args)["result"], retired["result"]);
    let shown = success(root, &["gate", "show", &fixture.gate, "--json"]);
    assert!(shown["result"]["retirement"].is_object());
    assert_eq!(shown["result"]["definition"]["id"], fixture.gate);
}
