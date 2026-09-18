use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

fn run(root: &Path, args: &[&str], input: Option<&Value>) -> Output {
    let mut child = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("test-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(input) = input {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(serde_json::to_string(input).unwrap().as_bytes())
            .unwrap();
    }
    child.wait_with_output().unwrap()
}

fn success(root: &Path, args: &[&str], input: Option<&Value>) -> Value {
    let output = run(root, args, input);
    assert!(
        output.status.success(),
        "{args:?}: {}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
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
    success(root.path(), &["init", "--json"], None);
    root
}

#[test]
fn legacy_status_aliases_work_in_create_update_move_and_list() {
    let root = fixture();
    for (input, canonical) in [
        ("progress", "in_progress"),
        ("IN PROGRESS", "in_progress"),
        ("inprogress", "in_progress"),
        ("Review", "in_review"),
        ("in-review", "in_review"),
        ("To Do", "ready"),
    ] {
        let created = success(
            root.path(),
            &["issue", "create", "Alias", "--status", input, "--json"],
            None,
        );
        assert_eq!(created["result"]["metadata"]["status"], canonical);
        let listed = success(
            root.path(),
            &["issue", "list", "--status", input, "--json"],
            None,
        );
        assert!(
            listed["result"]
                .as_array()
                .unwrap()
                .iter()
                .all(|issue| issue["metadata"]["status"] == canonical)
        );
    }
    let issue = success(
        root.path(),
        &["issue", "create", "Lifecycle", "--json"],
        None,
    );
    let id = issue["result"]["metadata"]["id"].as_str().unwrap();
    let updated = success(
        root.path(),
        &["issue", "update", id, "--status", "IN REVIEW", "--json"],
        None,
    );
    assert_eq!(updated["result"]["metadata"]["status"], "in_review");
    let done = success(
        root.path(),
        &["issue", "move", id, "--status", "CLOSED", "--json"],
        None,
    );
    assert_eq!(done["result"]["metadata"]["status"], "done");
    assert!(done["result"]["metadata"]["completed_at"].is_string());
    let closed = success(
        root.path(),
        &["issue", "list", "--status", "closed", "--json"],
        None,
    );
    assert_eq!(closed["result"].as_array().unwrap().len(), 1);
    let rejected = run(
        root.path(),
        &["issue", "move", id, "--status", "TO DO", "--json"],
        None,
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&rejected.stdout).unwrap()["error"]["code"],
        "policy_blocked"
    );
}

#[test]
fn legacy_priority_aliases_work_in_create_update_and_list_with_canonical_output() {
    let root = fixture();
    for (input, canonical) in [
        ("no", "none"),
        ("Med", "medium"),
        ("CRITICAL", "urgent"),
        ("L-o_w", "low"),
        ("HIGH", "high"),
    ] {
        let created = success(
            root.path(),
            &[
                "issue",
                "create",
                "Priority alias",
                "--priority",
                input,
                "--json",
            ],
            None,
        );
        assert_eq!(created["result"]["metadata"]["priority"], canonical);
        let listed = success(
            root.path(),
            &["issue", "list", "--priority", input, "--json"],
            None,
        );
        assert!(!listed["result"].as_array().unwrap().is_empty());
        assert!(
            listed["result"]
                .as_array()
                .unwrap()
                .iter()
                .all(|issue| issue["metadata"]["priority"] == canonical)
        );
        let id = created["result"]["metadata"]["id"].as_str().unwrap();
        let updated = success(
            root.path(),
            &["issue", "update", id, "--priority", "m_e-d", "--json"],
            None,
        );
        assert_eq!(updated["result"]["metadata"]["priority"], "medium");
    }
}

#[test]
fn json_priority_aliases_obey_cli_precedence_and_explicit_removal() {
    let root = fixture();
    let created = success(
        root.path(),
        &[
            "issue",
            "create",
            "--from-json",
            "-",
            "--priority",
            "Critical",
            "--json",
        ],
        Some(&json!({"title":"JSON alias","priority":"no"})),
    );
    assert_eq!(created["result"]["metadata"]["priority"], "urgent");
    let id = created["result"]["metadata"]["id"].as_str().unwrap();
    let updated = success(
        root.path(),
        &["issue", "update", id, "--from-json", "-", "--json"],
        Some(&json!({"fields":{"priority":"NO"}})),
    );
    assert_eq!(updated["result"]["metadata"]["priority"], "none");
    let removed = success(
        root.path(),
        &["issue", "update", id, "--from-json", "-", "--json"],
        Some(&json!({"fields":{"priority":null}})),
    );
    assert_eq!(removed["result"]["metadata"]["priority"], "medium");
}

#[test]
fn configured_states_override_aliases_and_ambiguous_or_invalid_inputs_write_nothing() {
    let root = fixture();
    let source = workdeck_pm::Repository::open_source(&root.path().join(".workdeck")).unwrap();
    let mut config = source.config().unwrap();
    for id in ["progress", "qa_ready", "qa-ready"] {
        config.workflow.states.push(workdeck_pm::WorkflowState {
            id: id.into(),
            name: id.into(),
            category: workdeck_pm::WorkflowCategory::Review,
            transitions: Vec::new(),
        });
    }
    fs::write(
        source.root().join("config.yml"),
        serde_json::to_string_pretty(&config).unwrap(),
    )
    .unwrap();
    let created = success(
        root.path(),
        &[
            "issue",
            "create",
            "Custom state",
            "--status",
            "PROGRESS",
            "--json",
        ],
        None,
    );
    assert_eq!(created["result"]["metadata"]["status"], "progress");
    for (flag, value, code) in [
        ("--status", "QA Ready", "ambiguous_reference"),
        ("--status", "unknown", "invalid_schema"),
        ("--priority", "unknown", "invalid_input"),
    ] {
        let output = run(
            root.path(),
            &["issue", "create", "Invalid input", flag, value, "--json"],
            None,
        );
        assert!(!output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
            code
        );
    }
    assert_eq!(
        success(root.path(), &["issue", "list", "--json"], None)["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn cancel_due_and_reopen_commands_preserve_history_and_replay_the_original_receipt() {
    let root = fixture();
    let created = success(
        root.path(),
        &[
            "issue",
            "create",
            "Due lifecycle",
            "--due-at",
            "2026-09-30",
            "--json",
        ],
        None,
    );
    let id = created["result"]["metadata"]["id"].as_str().unwrap();
    let invalid = run(
        root.path(),
        &["issue", "update", id, "--due-at", "2026-02-29", "--json"],
        None,
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&invalid.stdout).unwrap()["error"]["code"],
        "invalid_schema"
    );
    assert_eq!(
        success(root.path(), &["issue", "show", id, "--json"], None)["result"],
        created["result"]
    );
    let cancel_args = [
        "issue",
        "cancel",
        id,
        "--request-id",
        "cancel-due-once",
        "--json",
    ];
    let canceled = success(root.path(), &cancel_args, None);
    assert_eq!(canceled["result"]["metadata"]["status"], "canceled");
    assert_eq!(canceled["receipt"]["operation"], "issue.cancel");
    assert!(canceled["result"]["metadata"]["canceled_at"].is_string());
    assert_eq!(canceled["result"]["metadata"]["due_at"], "2026-09-30");
    assert_eq!(success(root.path(), &cancel_args, None), canceled);
    let reopened = success(root.path(), &["issue", "reopen", id, "--json"], None);
    assert_eq!(reopened["result"]["metadata"]["status"], "ready");
    assert!(reopened["result"]["metadata"]["canceled_at"].is_null());
    assert_eq!(reopened["result"]["metadata"]["due_at"], "2026-09-30");
    assert_eq!(success(root.path(), &cancel_args, None), canceled);
    assert_eq!(
        success(root.path(), &["issue", "show", id, "--json"], None)["result"],
        reopened["result"]
    );
    let cleared = success(
        root.path(),
        &["issue", "update", id, "--from-json", "-", "--json"],
        Some(&json!({"fields":{"due_at":null}})),
    );
    assert!(cleared["result"]["metadata"]["due_at"].is_null());
    success(root.path(), &["issue", "close", id, "--json"], None);
    let forbidden = run(root.path(), &["issue", "cancel", id, "--json"], None);
    assert_eq!(
        serde_json::from_slice::<Value>(&forbidden.stdout).unwrap()["error"]["code"],
        "policy_blocked"
    );
}

#[test]
fn ambiguous_short_ids_are_structured_failures_without_accidental_mutation() {
    let root = fixture();
    let first = success(root.path(), &["issue", "create", "First", "--json"], None);
    let second = success(root.path(), &["issue", "create", "Second", "--json"], None);
    let first_id = first["result"]["metadata"]["id"].as_str().unwrap();
    let second_id = second["result"]["metadata"]["id"].as_str().unwrap();
    let prefix = first_id
        .chars()
        .zip(second_id.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a)
        .collect::<String>();
    assert!(prefix.len() >= 4 && prefix.len() < first_id.len());
    let before = success(root.path(), &["issue", "list", "--json"], None);
    for command in ["show", "cancel"] {
        let output = run(root.path(), &["issue", command, &prefix, "--json"], None);
        assert_eq!(output.status.code(), Some(1));
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], "ambiguous_reference");
        assert_eq!(value["api_version"], 1);
    }
    assert_eq!(
        success(root.path(), &["issue", "list", "--json"], None),
        before
    );
    assert_eq!(
        success(root.path(), &["issue", "show", first_id, "--json"], None)["result"],
        first["result"]
    );
}
