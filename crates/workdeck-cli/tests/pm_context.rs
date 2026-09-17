use assert_cmd::prelude::*;
use serde_json::Value;
use std::{path::Path, process::Command};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn fresh_session_obtains_source_bound_context_within_the_complete_stdout_budget() {
    let root = tempfile::tempdir().unwrap();
    let repo = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let receipt = repo
        .create_issue(
            &workdeck_pm::CreateIssue::new(
                "Explain the selected task",
                "Preserve the exact accepted requirement and its source.",
            ),
            &workdeck_pm::RequestId::new(),
        )
        .unwrap();
    let id = receipt.result["metadata"]["id"].as_str().unwrap();
    let output = run(
        root.path(),
        &[
            "context",
            "--issue",
            id,
            "--budget",
            "8192",
            "--json",
            "--no-input",
        ],
    );
    assert!(
        output.status.success(),
        "context must be a first-class native command: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        output.stdout.len() <= 8192,
        "the envelope and newline count toward the budget"
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["source"]["repository"], repo.identity().as_str());
    assert_eq!(value["result"]["anchor"]["issue"], id);
    assert_eq!(
        value["result"]["anchor"]["issue_source"],
        receipt.result["source"]
    );
    assert!(value.to_string().contains("Explain the selected task"));
}

#[test]
fn issue_document_references_are_visible_in_bounded_agent_context() {
    let root = tempfile::tempdir().unwrap();
    let repo = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let receipt = repo
        .create_issue(
            &workdeck_pm::CreateIssue {
                fields: serde_json::from_value(serde_json::json!({
                    "documents": ["docs/accepted-design.md", "https://example.invalid/design"]
                }))
                .unwrap(),
                ..workdeck_pm::CreateIssue::new("Implement the linked design", "")
            },
            &workdeck_pm::RequestId::new(),
        )
        .unwrap();
    let id = receipt.result["metadata"]["id"].as_str().unwrap();
    let output = run(
        root.path(),
        &["context", "--issue", id, "--budget", "16384"],
    );
    assert!(output.status.success());
    assert!(output.stdout.len() <= 16384);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let sections = value["result"]["sections"].to_string();
    assert!(sections.contains("docs/accepted-design.md"), "{sections}");
    assert!(
        sections.contains("https://example.invalid/design"),
        "{sections}"
    );
    assert!(!root.path().join("docs").exists());
}

#[test]
fn fields_imply_machine_output_even_for_errors_and_discovery_reports_agent_operations() {
    let root = tempfile::tempdir().unwrap();
    let repo = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let receipt = repo
        .create_issue(
            &workdeck_pm::CreateIssue::new("Selected", "Scope"),
            &workdeck_pm::RequestId::new(),
        )
        .unwrap();
    let id = receipt.result["metadata"]["id"].as_str().unwrap();
    let output = run(
        root.path(),
        &[
            "next",
            "--issue",
            id,
            "--fields",
            "missing_field",
            "--no-input",
        ],
    );
    assert!(!output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout)
        .expect("field selection promises a machine-readable error");
    assert_eq!(value["error"]["code"], "invalid_input");
    let output = run(root.path(), &["capabilities", "--json"]);
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    for feature in [
        "task_context",
        "next_actions",
        "ready_work_selection",
        "questions",
        "handoffs",
    ] {
        assert_eq!(
            value["result"]["features"][feature], true,
            "missing capability {feature}"
        );
    }
}

fn json_output(root: &Path, args: &[&str]) -> Value {
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
fn fresh_agent_selection_actions_and_pagination_preserve_source_preconditions() {
    use workdeck_pm::{CreateIssue, RequestId};
    let root = tempfile::tempdir().unwrap();
    let repo = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let empty = json_output(root.path(), &["issue", "next", "--json", "--no-input"]);
    assert!(empty["result"]["selected"].is_null());
    assert_eq!(empty["result"]["total"], 0);
    let make = |title: &str, priority: &str| {
        repo.create_issue(
            &CreateIssue {
                title: title.into(),
                body: "Accepted scope".into(),
                fields: serde_json::from_value(
                    serde_json::json!({"status":"ready","priority":priority}),
                )
                .unwrap(),
            },
            &RequestId::new(),
        )
        .unwrap()
    };
    let low = make("Low", "low");
    let urgent = make("Urgent", "urgent");
    let id = urgent.result["metadata"]["id"].as_str().unwrap();
    let page = json_output(root.path(), &["issue", "next", "--limit", "1", "--json"]);
    assert_eq!(page["result"]["selected"]["issue"], id);
    assert_eq!(page["result"]["candidates"].as_array().unwrap().len(), 1);
    let cursor = page["result"]["next_cursor"].to_string();
    let second = json_output(
        root.path(),
        &[
            "issue", "next", "--limit", "1", "--cursor", &cursor, "--json",
        ],
    );
    assert_eq!(
        second["result"]["candidates"][0]["issue"],
        low.result["metadata"]["id"]
    );
    let context = json_output(root.path(), &["context", "--issue", id, "--json"]);
    let fingerprint = context["result"]["anchor"]["fingerprint"].as_str().unwrap();
    let next = json_output(
        root.path(),
        &[
            "next",
            "--issue",
            id,
            "--expected-context",
            fingerprint,
            "--json",
        ],
    );
    let implement = next["result"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action["kind"] == "implement")
        .unwrap();
    assert_eq!(
        implement["preconditions"]["source"],
        urgent.result["source"]
    );
    assert_eq!(implement["preconditions"]["context"], fingerprint);
    let projection = json_output(
        root.path(),
        &[
            "next",
            "--issue",
            id,
            "--fields",
            "anchor.issue,anchor.fingerprint",
        ],
    );
    assert_eq!(projection["source"]["repository"], repo.identity().as_str());
    assert_eq!(projection["result"]["anchor"]["issue"], id);
    assert!(projection["result"].get("actions").is_none());
    make("Later", "medium");
    let stale = run(
        root.path(),
        &[
            "issue", "next", "--limit", "1", "--cursor", &cursor, "--json",
        ],
    );
    assert!(!stale.status.success());
    let stale: Value = serde_json::from_slice(&stale.stdout).unwrap();
    assert_eq!(stale["error"]["code"], "stale_source");
}

#[test]
fn minimum_context_budget_counts_transport_and_omissions_are_explicit() {
    let root = tempfile::tempdir().unwrap();
    let repo = workdeck_pm::Repository::init(root.path(), "WD").unwrap();
    let receipt = repo
        .create_issue(
            &workdeck_pm::CreateIssue::new("Budget café 日本語", "Detailed scope".repeat(100)),
            &workdeck_pm::RequestId::new(),
        )
        .unwrap();
    let id = receipt.result["metadata"]["id"].as_str().unwrap();
    let output = run(
        root.path(),
        &["context", "--issue", id, "--budget", "1024", "--json"],
    );
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    let minimum = error["error"]["details"]["minimum_stdout_bytes"]
        .as_u64()
        .unwrap();
    let output = run(
        root.path(),
        &[
            "context",
            "--issue",
            id,
            "--budget",
            &minimum.to_string(),
            "--json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(output.stdout.len() as u64 <= minimum);
    let packet: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        packet["result"]["budget"]["omitted_entries"]
            .as_u64()
            .unwrap()
            > 0
    );
}

#[test]
fn malformed_app_configuration_cannot_bypass_bounded_agent_diagnostics() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    std::fs::create_dir(root.join(".workdeck")).unwrap();
    let bytes = format!("[{}!", "z".repeat(50_000));
    std::fs::write(root.join(".workdeck/config.toml"), &bytes).unwrap();
    for args in [
        vec!["context", "--issue", "WD-1", "--compact"],
        vec!["next", "--issue", "WD-1", "--json"],
        vec!["question", "list", "--json"],
        vec!["issue", "next", "--json"],
    ] {
        let output = run(root, &args);
        assert!(!output.status.success());
        assert!(
            output.stdout.len() <= 16 * 1024,
            "{args:?}: {} bytes",
            output.stdout.len()
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["api_version"], 1, "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_schema");
        assert!(value["source"].is_object());
        assert_eq!(value["retryable"], false);
    }
    assert!(!root.join(".workdeck/config.yml").exists());
    assert_eq!(
        std::fs::read_to_string(root.join(".workdeck/config.toml")).unwrap(),
        bytes
    );
}

#[test]
fn new_agent_parser_failures_are_bounded_machine_errors_without_source_reads() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let invalid = "n".repeat(50_000);
    for args in [
        vec!["context", "--issue", "WD-1", "--budget", &invalid, "--json"],
        vec!["question", "answer", "--json"],
        vec!["issue", "--json", "next", "--limit", "bad"],
        vec!["--cwd", "missing-source", "next", "--json"],
    ] {
        let output = run(root, &args);
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.len() <= 16 * 1024);
        let value: Value = serde_json::from_slice(&output.stdout)
            .expect("parser failures must preserve the agent machine contract");
        assert_eq!(value["error"]["code"], "invalid_input");
        assert_eq!(value["api_version"], 1);
        assert!(value["source"]["repository"].is_null());
        assert_eq!(value["retryable"], false);
    }
    for args in [
        vec!["--cwd", "context", "issue", "show", "--json"],
        vec!["--cwd=question", "issue", "show", "--json"],
        vec!["context", "--", "--json"],
    ] {
        let output = run(root, &args);
        assert!(!output.status.success());
        assert!(
            output.stdout.is_empty(),
            "non-machine/older command was misclassified: {args:?}"
        );
    }
    let help = run(root, &["context", "--help"]);
    assert!(help.status.success());
    assert!(String::from_utf8_lossy(&help.stdout).contains("Usage:"));
    assert!(!root.join(".workdeck").exists());
}
