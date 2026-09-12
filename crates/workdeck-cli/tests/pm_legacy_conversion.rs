#[path = "support/legacy.rs"]
mod legacy_fixture;
use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use workdeck_cli::store::{IssueStatus, WorkdeckStore};

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
    let out = run(root, args);
    assert!(
        out.status.success(),
        "{args:?}: {} {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn root() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    root
}
fn native() -> tempfile::TempDir {
    let root = root();
    ok(root.path(), &["init", "--json"]);
    root
}
fn legacy() -> tempfile::TempDir {
    let root = root();
    let store = WorkdeckStore::new(root.path().join(".agents/workdeck"));
    legacy_fixture::init(store.root());
    let mut issue = legacy_fixture::issue(store.root(), "WD-1", "Imported completion");
    issue.status = IssueStatus::Done;
    issue.description = "Exact authored **body**.\n".into();
    issue.created_at = "2026-01-01T00:00:00Z".into();
    issue.updated_at = "2026-01-02T00:00:00Z".into();
    issue
        .extra
        .insert("historical".into(), toml::Value::String("kept".into()));
    legacy_fixture::write(store.root(), "issues/WD-1.toml", &issue);
    root
}

#[test]
fn actual_legacy_exports_convert_to_native_records_and_replay_after_later_edits() {
    let source = legacy();
    for export_args in [
        vec!["export"],
        vec!["export", "--json"],
        vec!["export", "--jsonl"],
    ] {
        let exported = run(source.path(), &export_args);
        assert!(exported.status.success());
        let destination = native();
        fs::write(destination.path().join("input.data"), &exported.stdout).unwrap();
        let args = [
            "import",
            "input.data",
            "--request-id",
            "convert-once",
            "--json",
        ];
        let first = ok(destination.path(), &args);
        let shown = ok(destination.path(), &["issue", "show", "WD-1", "--json"]);
        assert_eq!(shown["result"]["metadata"]["id"], "WD-1");
        assert_eq!(shown["result"]["metadata"]["title"], "Imported completion");
        assert_eq!(shown["result"]["body"], "Exact authored **body**.\n");
        assert_eq!(
            shown["result"]["metadata"]["created_at"],
            "2026-01-01T00:00:00Z"
        );
        assert!(shown["result"]["metadata"]["imported_completion"].is_object());
        let retained = first["result"]["source_path"].as_str().unwrap();
        assert_eq!(
            fs::read(destination.path().join(".workdeck").join(retained)).unwrap(),
            exported.stdout
        );
        ok(destination.path(), &["issue", "reopen", "WD-1", "--json"]);
        assert_eq!(ok(destination.path(), &args)["receipt"], first["receipt"]);
        assert!(
            ok(destination.path(), &["doctor", "--json"])["result"]["valid"]
                .as_bool()
                .unwrap()
        );
        assert!(!destination.path().join(".agents").exists());
    }
}

#[test]
fn reviewed_legacy_import_requires_the_same_timestamp_and_destination() {
    let source = legacy();
    let exported = run(source.path(), &["export"]);
    let destination = native();
    fs::write(destination.path().join("input.json"), exported.stdout).unwrap();
    let preview = ok(
        destination.path(),
        &["import", "input.json", "--dry-run", "--json"],
    );
    assert_eq!(preview["result"]["allowed"], true);
    let fingerprint = preview["result"]["fingerprint"].as_str().unwrap();
    let timestamp = preview["result"]["imported_at"].as_str().unwrap();
    let missing_context = run(
        destination.path(),
        &[
            "import",
            "input.json",
            "--expected-plan",
            fingerprint,
            "--json",
        ],
    );
    assert!(!missing_context.status.success());
    let error: Value = serde_json::from_slice(&missing_context.stdout).unwrap();
    assert_eq!(error["error"]["code"], "invalid_input");
    let first = ok(
        destination.path(),
        &[
            "import",
            "input.json",
            "--expected-plan",
            fingerprint,
            "--imported-at",
            timestamp,
            "--request-id",
            "reviewed-import",
            "--json",
        ],
    );
    assert_eq!(
        ok(
            destination.path(),
            &[
                "import",
                "input.json",
                "--expected-plan",
                fingerprint,
                "--imported-at",
                timestamp,
                "--request-id",
                "reviewed-import",
                "--json"
            ]
        )["receipt"],
        first["receipt"]
    );
}
