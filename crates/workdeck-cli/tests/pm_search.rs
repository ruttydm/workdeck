use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("HOME", root.join("test-home"))
        .env("XDG_CONFIG_HOME", root.join("test-home/config"))
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
    serde_json::from_slice(&output.stdout).unwrap()
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
    success(root.path(), &["init", "--json"]);
    root
}

#[test]
fn native_search_returns_current_planning_and_source_qualified_symbol_targets() {
    let root = repository();
    fs::write(root.path().join("main.rs"), "fn searchable_symbol() {}\n").unwrap();
    let issue = success(
        root.path(),
        &["issue", "create", "Searchable native issue", "--json"],
    );
    let result = success(
        root.path(),
        &[
            "search",
            "Searchable native",
            "--target",
            "issues",
            "--json",
        ],
    );
    assert_eq!(result["api_version"], 1);
    assert_eq!(result["source"], issue["source"]);
    assert_eq!(
        result["result"]["results"][0]["target"]["key"],
        issue["result"]["metadata"]["id"]
    );
    assert_eq!(result["result"]["truncated"], false);
    let symbols = success(
        root.path(),
        &["search", "searchable_symbol", "--target", "files", "--json"],
    );
    assert!(
        symbols["result"]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["target"]["kind"] == "symbol" && row["target"]["line"] == 1)
    );
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn malformed_native_search_cannot_report_empty_success_and_bad_target_is_explicit() {
    let root = repository();
    let invalid = run(
        root.path(),
        &["search", "value", "--target", "not-a-kind", "--json"],
    );
    assert!(!invalid.status.success());
    let result: Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(result["error"]["code"], "invalid_input");
    fs::create_dir(root.path().join(".workdeck/issues/WD-1")).unwrap();
    fs::write(
        root.path().join(".workdeck/issues/WD-1/item.md"),
        "not a valid issue",
    )
    .unwrap();
    let output = run(root.path(), &["search", "no-match", "--json"]);
    assert!(!output.status.success());
    let failure: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(failure["api_version"], 1);
    assert_eq!(failure["ok"], false);
    assert_ne!(failure["error"]["code"], "unsupported");
    assert!(
        failure["error"]["path"]
            .as_str()
            .unwrap()
            .contains("WD-1/item.md")
    );
}

#[test]
fn native_search_filters_targets_before_the_global_result_limit() {
    let root = repository();
    // Empty-query ranking presents files first. This exceeds the provider cap,
    // so filtering its already-truncated result would lose this real issue.
    for number in 0..10_010 {
        fs::write(root.path().join(format!("file-{number:05}.txt")), "").unwrap();
    }
    let issue = success(root.path(), &["issue", "create", "Still visible", "--json"]);
    let result = success(root.path(), &["search", "", "--target", "issues", "--json"]);
    let rows = result["result"]["results"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "{result}");
    assert_eq!(rows[0]["target"]["key"], issue["result"]["metadata"]["id"]);
}
