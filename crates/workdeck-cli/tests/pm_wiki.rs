use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use workdeck_pm::Repository;

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
    let temp = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(temp.path())
            .status()
            .unwrap()
            .success()
    );
    temp
}

#[test]
fn wiki_authoring_show_update_link_and_replay_use_native_files() {
    let temp = root();
    let root = temp.path();
    Repository::init(root, "WD").unwrap();
    fs::write(
        root.join("body.md"),
        "# Exact\r\n<script>inert()</script>\r\n",
    )
    .unwrap();
    let create = [
        "wiki",
        "create",
        "architecture/overview.md",
        "--body-file",
        "body.md",
        "--request-id",
        "wiki-once",
        "--json",
    ];
    let first = ok(root, &create);
    assert_eq!(
        first["result"]["body"],
        "# Exact\r\n<script>inert()</script>\r\n"
    );
    let show = ok(
        root,
        &["wiki", "show", "architecture/overview.md", "--json"],
    );
    assert_eq!(first["result"], show["result"]);
    ok(
        root,
        &[
            "wiki",
            "update",
            "architecture/overview.md",
            "--expected-content",
            show["result"]["content_hash"].as_str().unwrap(),
            "--body",
            "# Edited",
            "--json",
        ],
    );
    assert_eq!(ok(root, &create)["receipt"], first["receipt"]);
    assert_eq!(
        ok(root, &["wiki", "list", "--json"])["result"][0]["body"],
        "# Edited"
    );
    let issue = ok(root, &["issue", "create", "Read architecture", "--json"]);
    let id = issue["result"]["metadata"]["id"].as_str().unwrap();
    ok(
        root,
        &[
            "issue",
            "link-document",
            id,
            ".workdeck/wiki/architecture/overview.md",
            "--json",
        ],
    );
    assert!(!root.join(".agents").exists());
    let schema = ok(root, &["schema", "wiki-write", "--json"]);
    assert_eq!(schema["ok"], true);
}

#[test]
fn rejected_wiki_requests_preserve_content_and_explain_source_requirements() {
    let temp = root();
    let root = temp.path();
    let unavailable = run(root, &["wiki", "list", "--json"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&unavailable.stdout).unwrap()["error"]["code"],
        "not_initialized"
    );
    assert!(!root.join(".workdeck").exists());
    Repository::init(root, "WD").unwrap();
    ok(
        root,
        &[
            "wiki",
            "create",
            "overview.md",
            "--body",
            "original",
            "--json",
        ],
    );
    for args in [
        vec![
            "wiki",
            "create",
            "overview.md",
            "--body",
            "overwrite",
            "--json",
        ],
        vec![
            "wiki",
            "update",
            "overview.md",
            "--expected-content",
            "0000000000000000000000000000000000000000000000000000000000000000",
            "--body",
            "overwrite",
            "--json",
        ],
        vec!["wiki", "show", "overview.md", "--stage", "--json"],
    ] {
        assert!(!run(root, &args).status.success());
    }
    assert_eq!(
        fs::read_to_string(root.join(".workdeck/wiki/overview.md")).unwrap(),
        "original"
    );
}

#[test]
fn wiki_staging_retains_unrelated_index_and_stages_exact_receipt() {
    let temp = root();
    let root = temp.path();
    Repository::init(root, "WD").unwrap();
    fs::write(root.join("unrelated.txt"), "staged original").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "unrelated.txt"])
            .current_dir(root)
            .status()
            .unwrap()
            .success()
    );
    fs::write(root.join("unrelated.txt"), "unstaged later").unwrap();
    let result = ok(
        root,
        &[
            "wiki",
            "create",
            "overview.md",
            "--body",
            "# Overview",
            "--stage",
            "--json",
        ],
    );
    assert!(
        result["staging"]["paths"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p.as_str().is_some_and(|p| p.ends_with("wiki/overview.md")))
    );
    let staged = Command::new("git")
        .args(["show", ":unrelated.txt"])
        .current_dir(root)
        .output()
        .unwrap();
    assert_eq!(staged.stdout, b"staged original");
    let names = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root)
        .output()
        .unwrap();
    let names = String::from_utf8(names.stdout).unwrap();
    assert!(names.contains(".workdeck/wiki/overview.md"));
    assert!(names.contains(".workdeck/operations/"));
    assert!(!names.contains(".tmp"));
}

fn rejected(root: &Path, args: &[&str], code: &str) -> Value {
    let output = run(root, args);
    assert!(
        !output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], code, "{args:?}: {value}");
    value
}
fn git_output(root: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
fn forged_wiki_receipt_is_rejected_by_real_cli_replay_events_export_and_staging() {
    let temp = root();
    let root = temp.path();
    Repository::init(root, "WD").unwrap();
    fs::write(root.join("unrelated.txt"), "Keep staged bytes").unwrap();
    git_output(root, &["add", "unrelated.txt"]);
    let create = [
        "wiki",
        "create",
        "overview.md",
        "--body",
        "Original wiki content",
        "--request-id",
        "wiki-proof",
        "--json",
    ];
    let original = ok(root, &create);
    let mut forged = original["receipt"].clone();
    let hash = workdeck_pm::ContentHash::of(b"Different claimed publication");
    forged["result"]["body"] = serde_json::json!("Different claimed publication");
    forged["result"]["content_hash"] = serde_json::json!(hash);
    forged["changed"][0]["after"] = serde_json::json!(hash);
    let receipt_path = root
        .join(".workdeck/operations")
        .join(format!("{}.yml", forged["operation_id"].as_str().unwrap()));
    // JSON is valid YAML; alter a coherent result while retaining its original
    // intent hash, rather than only making a malformed serialization fixture.
    let bytes = serde_json::to_vec_pretty(&forged).unwrap();
    fs::write(&receipt_path, &bytes).unwrap();
    let before_index = fs::read(root.join(".git/index")).unwrap();
    rejected(root, &create, "corrupt_store");
    let mut with_stage = create.to_vec();
    with_stage.push("--stage");
    let error = rejected(root, &with_stage, "corrupt_store");
    assert_ne!(error["error"]["details"]["mutation_committed"], true);
    rejected(root, &["events", "list", "--json"], "corrupt_store");
    rejected(root, &["export", "--json"], "corrupt_store");
    assert_eq!(
        ok(root, &["wiki", "show", "overview.md", "--json"])["result"]["body"],
        "Original wiki content"
    );
    assert_eq!(fs::read(&receipt_path).unwrap(), bytes);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), before_index);
    assert_eq!(
        fs::read_dir(root.join(".workdeck/operations"))
            .unwrap()
            .count(),
        1
    );
    assert!(!root.join(".agents").exists());
}

#[test]
fn noop_replay_stages_only_its_receipt_after_later_edit_while_changed_replay_staging_is_stale() {
    let temp = root();
    let root = temp.path();
    Repository::init(root, "WD").unwrap();
    fs::write(root.join("unrelated.txt"), "Already staged").unwrap();
    git_output(root, &["add", "unrelated.txt"]);
    fs::write(root.join("unrelated.txt"), "Later unstaged").unwrap();
    let create = [
        "wiki",
        "create",
        "overview.md",
        "--body",
        "First body",
        "--request-id",
        "wiki-create-original",
        "--json",
    ];
    let first = ok(root, &create);
    let hash = first["result"]["content_hash"].as_str().unwrap();
    let no_change = [
        "wiki",
        "update",
        "overview.md",
        "--body",
        "First body",
        "--expected-content",
        hash,
        "--request-id",
        "wiki-no-change",
        "--json",
    ];
    let noop = ok(root, &no_change);
    assert_eq!(noop["receipt"]["changed"], serde_json::json!([]));
    ok(
        root,
        &[
            "wiki",
            "update",
            "overview.md",
            "--body",
            "Later body",
            "--expected-content",
            hash,
            "--request-id",
            "wiki-later-edit",
            "--json",
        ],
    );
    let mut stage_noop = no_change.to_vec();
    stage_noop.push("--stage");
    let staged = ok(root, &stage_noop);
    assert_eq!(staged["receipt"], noop["receipt"]);
    let paths = staged["staging"]["paths"].as_array().unwrap();
    assert_eq!(paths.len(), 1);
    assert_eq!(
        paths[0],
        format!(
            ".workdeck/operations/{}.yml",
            noop["receipt"]["operation_id"].as_str().unwrap()
        )
    );
    assert_eq!(
        git_output(root, &["show", ":unrelated.txt"]),
        b"Already staged"
    );
    assert_eq!(
        fs::read(root.join("unrelated.txt")).unwrap(),
        b"Later unstaged"
    );
    let events = ok(root, &["events", "list", "--json"]);
    assert!(
        events["result"]["mutation_receipts"]
            .as_array()
            .unwrap()
            .contains(&noop["receipt"])
    );
    let exported = run(root, &["export", "--json"]);
    assert!(
        exported.status.success(),
        "{}",
        String::from_utf8_lossy(&exported.stdout)
    );
    workdeck_pm::decode_snapshot(&exported.stdout)
        .unwrap()
        .validate()
        .unwrap();
    let before_index = fs::read(root.join(".git/index")).unwrap();
    let mut stage_create = create.to_vec();
    stage_create.push("--stage");
    let failure = rejected(root, &stage_create, "stale_source");
    assert_eq!(failure["error"]["details"]["mutation_committed"], true);
    assert_eq!(failure["error"]["details"]["receipt"], first["receipt"]);
    assert_eq!(fs::read(root.join(".git/index")).unwrap(), before_index);
    assert_eq!(
        fs::read_to_string(root.join(".workdeck/wiki/overview.md")).unwrap(),
        "Later body"
    );
}
