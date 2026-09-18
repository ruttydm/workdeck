#![cfg(unix)]

//! A bounded CLI fault matrix for the PM source boundary.
//!
//! Each case uses a temporary repository and checks that malformed, unsafe or
//! stale inputs fail with a structured error before they can create a second
//! authority or mutate an unrelated file.

use assert_cmd::prelude::*;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    process::{Command, Output},
};
use tempfile::TempDir;

fn git_init(root: &Path) {
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .status()
            .unwrap()
            .success()
    );
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .args(args)
        .output()
        .unwrap()
}

fn success(root: &Path, args: &[&str]) -> Value {
    let output = run(root, args);
    assert!(
        output.status.success(),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1, "{value}");
    assert_eq!(value["ok"], true, "{value}");
    value
}

fn failure(root: &Path, args: &[&str], code: &str, exit: i32) -> Value {
    let output = run(root, args);
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1, "{value}");
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}

fn files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(base: &Path, current: &Path, output: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let mut entries = fs::read_dir(current)
            .unwrap_or_else(|error| panic!("read {}: {error}", current.display()))
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                visit(base, &path, output);
            } else if metadata.is_file() {
                output.insert(
                    path.strip_prefix(base).unwrap().to_owned(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut output = BTreeMap::new();
    visit(root, root, &mut output);
    output
}

fn common_prefix(left: &str, right: &str) -> String {
    left.chars()
        .zip(right.chars())
        .take_while(|(left, right)| left == right)
        .map(|(left, _)| left)
        .collect()
}

#[test]
fn malformed_unsafe_and_stale_inputs_fail_closed_without_cross_file_effects() {
    let root = TempDir::new().unwrap();
    git_init(root.path());
    success(root.path(), &["init", "--json"]);
    let created = success(root.path(), &["issue", "create", "Fault fixture", "--json"]);
    let id = created["result"]["metadata"]["id"].as_str().unwrap();
    let relative = created["result"]["path"].as_str().unwrap();
    let issue_path = root.path().join(".workdeck").join(relative);
    let original = fs::read(&issue_path).unwrap();
    let native_before = files(&root.path().join(".workdeck"));

    // Malformed YAML is reported at the source boundary and does not create a
    // replacement record or rewrite the native configuration.
    fs::write(&issue_path, b"---\nschema: [\n---\n").unwrap();
    let malformed = failure(
        root.path(),
        &["issue", "show", id, "--json"],
        "invalid_schema",
        2,
    );
    assert!(
        malformed["error"]["path"]
            .as_str()
            .unwrap()
            .contains("issues")
    );
    fs::write(&issue_path, &original).unwrap();
    assert_eq!(files(&root.path().join(".workdeck")), native_before);

    // A future schema is distinct from malformed syntax and remains a typed
    // unsupported-schema failure.
    let unsupported =
        String::from_utf8(original.clone())
            .unwrap()
            .replacen("schema: 1", "schema: 999", 1);
    fs::write(&issue_path, unsupported).unwrap();
    failure(
        root.path(),
        &["issue", "show", id, "--json"],
        "unsupported_schema",
        2,
    );
    fs::write(&issue_path, &original).unwrap();

    // A linked source cannot redirect a read outside the repository.
    let outside = root.path().join("outside.md");
    fs::write(&outside, b"outside bytes").unwrap();
    fs::remove_file(&issue_path).unwrap();
    symlink(&outside, &issue_path).unwrap();
    failure(
        root.path(),
        &["issue", "show", id, "--json"],
        "unsafe_path",
        1,
    );
    assert_eq!(fs::read(&outside).unwrap(), b"outside bytes");
    fs::remove_file(&issue_path).unwrap();
    fs::write(&issue_path, &original).unwrap();

    // A permission failure is observable and does not silently fall back to a
    // cached or legacy source. Restore the mode before the next case.
    let mode = fs::metadata(&issue_path).unwrap().permissions().mode();
    fs::set_permissions(&issue_path, fs::Permissions::from_mode(0o0)).unwrap();
    failure(root.path(), &["issue", "show", id, "--json"], "io", 1);
    fs::set_permissions(&issue_path, fs::Permissions::from_mode(mode)).unwrap();
    assert_eq!(
        success(root.path(), &["issue", "show", id, "--json"])["result"]["metadata"]["id"],
        id
    );

    // A source token captured before an edit cannot be reused for a stale
    // mutation, even when the requested field is otherwise valid.
    let inspected = success(root.path(), &["issue", "show", id, "--json"]);
    let revision = inspected["result"]["source"]["revision"].to_string();
    let content = inspected["result"]["source"]["content"].as_str().unwrap();
    success(
        root.path(),
        &["issue", "update", id, "--title", "Current", "--json"],
    );
    failure(
        root.path(),
        &[
            "issue",
            "update",
            id,
            "--title",
            "Stale retry",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
        "stale_source",
        4,
    );
    assert_eq!(
        success(root.path(), &["issue", "show", id, "--json"])["result"]["metadata"]["title"],
        "Current"
    );

    // Short references remain ambiguous after two records share a prefix;
    // the failed read cannot mutate either record.
    let second = success(
        root.path(),
        &["issue", "create", "Second fault fixture", "--json"],
    );
    let second_id = second["result"]["metadata"]["id"].as_str().unwrap();
    let prefix = common_prefix(id, second_id);
    assert!(prefix.len() >= 4 && prefix.len() < id.len());
    failure(
        root.path(),
        &["issue", "show", &prefix, "--json"],
        "ambiguous_reference",
        1,
    );
    let listed = success(root.path(), &["issue", "list", "--json"]);
    assert_eq!(listed["result"].as_array().unwrap().len(), 2);
}
