use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn command(root: &Path, args: &[&str], code: Option<&str>) -> Value {
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("test-config"))
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        code.is_none(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    if let Some(code) = code {
        assert_eq!(value["error"]["code"], code, "{value}");
    }
    value
}
fn ok(root: &Path, args: &[&str]) -> Value {
    command(root, args, None)
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
fn native_agent_annotations_cover_the_lifecycle_without_creating_runtime_or_check_evidence() {
    let root = repository();
    let args = [
        "agent",
        "record",
        "Recorded implementation",
        "--id",
        "session-a",
        "--agent",
        "tester",
        "--request-id",
        "record-once",
        "--json",
    ];
    let recorded = ok(root.path(), &args);
    assert_eq!(recorded["result"]["session"]["id"], "session-a");
    ok(
        root.path(),
        &[
            "agent",
            "update",
            "session-a",
            "--goal",
            "Explain this work",
            "--json",
        ],
    );
    for (action, text) in [
        ("append-plan", "Inspect"),
        ("add-command", "printf historical"),
        ("add-test", "claimed green"),
        ("add-note", "Resume from here"),
    ] {
        let args = [
            "agent",
            action,
            "session-a",
            text,
            "--request-id",
            action,
            "--json",
        ];
        let first = ok(root.path(), &args);
        assert_eq!(ok(root.path(), &args)["receipt"], first["receipt"]);
    }
    ok(
        root.path(),
        &[
            "agent",
            "add-file",
            "session-a",
            "src/lib.rs",
            "--change-type",
            "modified",
            "--json",
        ],
    );
    let finished = ok(
        root.path(),
        &[
            "agent",
            "finish",
            "session-a",
            "--summary",
            "Historical summary",
            "--json",
        ],
    );
    assert_eq!(finished["result"]["session"]["status"], "done");
    assert!(
        !finished["result"]["session"]["ended_at"]
            .as_str()
            .unwrap()
            .is_empty()
    );
    assert_eq!(finished["result"]["evidence"], "historical_annotation");
    assert_eq!(ok(root.path(), &args)["receipt"], recorded["receipt"]);
    assert_eq!(
        ok(root.path(), &["agent", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let issue = ok(
        root.path(),
        &["issue", "create", "No manufactured qualification", "--json"],
    );
    assert!(
        issue["result"]["metadata"]
            .get("manual_acceptance")
            .is_none()
    );
    assert!(!root.path().join(".workdeck/check-runs").exists());
    assert!(!root.path().join(".agents").exists());
    ok(
        root.path(),
        &[
            "agent",
            "delete",
            "session-a",
            "--yes",
            "--request-id",
            "retire-session",
            "--json",
        ],
    );
    assert!(
        ok(root.path(), &["agent", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        ok(root.path(), &["agent", "show", "session-a", "--json"])["result"]["retired"],
        true
    );
    command(
        root.path(),
        &["agent", "record", "Reuse", "--id", "session-a", "--json"],
        Some("policy_blocked"),
    );
}

#[test]
fn native_agent_import_is_atomic_bounded_and_preserves_extra_historical_fields() {
    let root = repository();
    fs::write(root.path().join("sessions.jsonl"), "{\"id\":\"one\",\"title\":\"First\",\"custom_source\":{\"revision\":3}}\n{\"id\":\"../escape\",\"title\":\"Invalid\"}\n").unwrap();
    command(
        root.path(),
        &["agent", "import", "sessions.jsonl", "--json"],
        Some("invalid_input"),
    );
    assert!(
        ok(root.path(), &["agent", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::write(
        root.path().join("sessions.jsonl"),
        "{\"id\":\"one\",\"title\":\"First\",\"custom_source\":{\"revision\":3}}\n",
    )
    .unwrap();
    let args = [
        "agent",
        "import",
        "sessions.jsonl",
        "--request-id",
        "import-sessions",
        "--json",
    ];
    let first = ok(root.path(), &args);
    assert_eq!(ok(root.path(), &args)["receipt"], first["receipt"]);
    assert_eq!(
        ok(root.path(), &["agent", "show", "one", "--json"])["result"]["session"]["custom_source"]
            ["revision"],
        3
    );
}

#[test]
fn native_events_keep_imported_annotations_separate_from_mutation_receipts() {
    let root = repository();
    let issue = ok(
        root.path(),
        &["issue", "create", "Recorded mutation", "--json"],
    );
    fs::create_dir_all(root.path().join(".workdeck/imported-history")).unwrap();
    fs::write(
        root.path().join(".workdeck/imported-history/events.jsonl"),
        "{\"kind\":\"test_passed\",\"payload\":{\"unverified\":true},\"extra\":\"preserved\"}\n",
    )
    .unwrap();
    let events = ok(root.path(), &["events", "list", "--json"]);
    assert_eq!(
        events["result"]["historical_events"][0]["extra"],
        "preserved"
    );
    assert_eq!(events["result"]["evidence"], "historical_annotation");
    assert!(
        events["result"]["mutation_receipts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|receipt| receipt["operation_id"] == issue["receipt"]["operation_id"])
    );
    fs::write(
        root.path().join(".workdeck/imported-history/events.jsonl"),
        "not json\n",
    )
    .unwrap();
    command(
        root.path(),
        &["events", "list", "--json"],
        Some("invalid_schema"),
    );
}

#[test]
fn native_history_import_preserves_nested_extras_and_rejects_oversized_record_counts_atomically() {
    let root = repository();
    let input = root.path().join("incoming.jsonl");
    let text = (0..10_001)
        .map(|id| format!("{{\"id\":\"session-{id}\",\"title\":\"Incoming\"}}\n"))
        .collect::<String>();
    fs::write(&input, text).unwrap();
    command(
        root.path(),
        &[
            "agent",
            "import",
            "incoming.jsonl",
            "--request-id",
            "too-many",
            "--json",
        ],
        Some("invalid_input"),
    );
    assert!(
        ok(root.path(), &["agent", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fs::read_dir(root.path().join(".workdeck/operations")).map_or(0, |entries| entries.count()),
        0
    );
    fs::write(&input, "{\"id\":\"one\",\"title\":\"Incoming\",\"touched_files\":[{\"path\":\"src/lib.rs\",\"producer\":{\"revision\":7}}]}\n").unwrap();
    let original = ok(
        root.path(),
        &[
            "agent",
            "import",
            "incoming.jsonl",
            "--request-id",
            "incoming",
            "--json",
        ],
    );
    assert_eq!(
        original["result"][0]["session"]["touched_files"][0]["producer"]["revision"],
        7
    );
    ok(root.path(), &["agent", "delete", "one", "--yes", "--json"]);
    assert_eq!(
        ok(
            root.path(),
            &[
                "agent",
                "import",
                "incoming.jsonl",
                "--request-id",
                "incoming",
                "--json"
            ]
        )["receipt"],
        original["receipt"]
    );
    fs::remove_file(
        root.path()
            .join(".workdeck/imported-history/deleted-sessions/one.yml"),
    )
    .unwrap();
    command(
        root.path(),
        &["agent", "list", "--json"],
        Some("corrupt_store"),
    );
    assert!(!root.path().join(".agents").exists());
}

#[cfg(unix)]
#[test]
fn native_history_fifo_import_returns_bounded_error_without_source_mutation() {
    use std::time::{Duration, Instant};
    let root = repository();
    let input = root.path().join("incoming.jsonl");
    assert!(
        Command::new("mkfifo")
            .arg(&input)
            .status()
            .unwrap()
            .success()
    );
    let mut child = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .env("XDG_CONFIG_HOME", root.path().join("test-config"))
        .args(["agent", "import", "incoming.jsonl", "--json"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while child.try_wait().unwrap().is_none() {
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("FIFO history import blocked beyond 10s");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["error"]["code"], "unsafe_path");
    assert!(
        ok(root.path(), &["agent", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}
