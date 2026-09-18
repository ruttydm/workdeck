use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("test-config"))
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
fn native_export_bare_enveloped_and_jsonl_preserve_one_validated_snapshot() {
    let root = repository();
    ok(
        root.path(),
        &["issue", "create", "Export exact planning", "--json"],
    );
    ok(
        root.path(),
        &[
            "agent",
            "record",
            "Historical notes",
            "--id",
            "recorded",
            "--json",
        ],
    );
    let bare = run(root.path(), &["export"]);
    assert!(
        bare.status.success(),
        "{}",
        String::from_utf8_lossy(&bare.stdout)
    );
    let snapshot = workdeck_pm::decode_snapshot(&bare.stdout).unwrap();
    for args in [vec!["export", "--json"], vec!["export", "--jsonl"]] {
        let out = run(root.path(), &args);
        assert!(
            out.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert_eq!(workdeck_pm::decode_snapshot(&out.stdout).unwrap(), snapshot);
    }
    for file in &snapshot.files {
        assert_eq!(
            fs::read(root.path().join(".workdeck").join(&file.path)).unwrap(),
            file.content
        );
    }
    assert!(snapshot.files.iter().any(|file| {
        file.path
            .to_string_lossy()
            .contains("imported-sessions/recorded.toml")
    }));
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn native_import_previews_replays_and_explicitly_blocks_foreign_authority() {
    let root = repository();
    ok(root.path(), &["issue", "create", "Round trip", "--json"]);
    let snapshot = run(root.path(), &["export", "--json"]);
    assert!(snapshot.status.success());
    fs::write(root.path().join("snapshot.json"), &snapshot.stdout).unwrap();
    let preview = ok(
        root.path(),
        &["import", "snapshot.json", "--dry-run", "--json"],
    );
    assert_eq!(preview["result"]["allowed"], true);
    assert!(preview["result"]["changes"].as_array().unwrap().is_empty());
    let args = [
        "import",
        "snapshot.json",
        "--request-id",
        "import-once",
        "--expected-plan",
        preview["result"]["fingerprint"].as_str().unwrap(),
        "--json",
    ];
    let first = ok(root.path(), &args);
    assert_eq!(ok(root.path(), &args)["receipt"], first["receipt"]);
    let other = repository();
    fs::write(other.path().join("snapshot.json"), snapshot.stdout).unwrap();
    let before = fs::read(other.path().join(".workdeck/config.yml")).unwrap();
    let blocked = ok(
        other.path(),
        &[
            "import",
            "snapshot.json",
            "--replace",
            "--dry-run",
            "--json",
        ],
    );
    assert_eq!(blocked["result"]["allowed"], false);
    assert!(
        blocked["result"]["blockers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|error| error["code"] == "unsupported")
    );
    let out = run(
        other.path(),
        &["import", "snapshot.json", "--replace", "--json"],
    );
    assert!(!out.status.success());
    let error: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(error["error"]["code"], "policy_blocked");
    assert_eq!(
        fs::read(other.path().join(".workdeck/config.yml")).unwrap(),
        before
    );
}

#[test]
fn invalid_or_oversized_snapshot_input_cannot_become_an_empty_successful_import() {
    let root = repository();
    for (name, bytes) in [
        ("unknown.json", b"{}".to_vec()),
        (
            "bad.json",
            b"{\"ok\":true,\"kind\":\"export\",\"result\":{}}".to_vec(),
        ),
        (
            "large.json",
            vec![b' '; workdeck_pm::MAX_SNAPSHOT_INPUT_BYTES + 1],
        ),
    ] {
        fs::write(root.path().join(name), bytes).unwrap();
        let out = run(root.path(), &["import", name, "--dry-run", "--json"]);
        assert!(!out.status.success(), "accepted {name}");
        let error: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(error["api_version"], 1);
        assert_eq!(error["ok"], false);
    }
}
