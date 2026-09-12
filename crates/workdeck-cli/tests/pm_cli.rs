use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
use tempfile::{TempDir, tempdir};

fn repository() -> TempDir {
    let directory = tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(directory.path())
            .status()
            .unwrap()
            .success()
    );
    directory
}

fn run(directory: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(directory)
        .env("XDG_CONFIG_HOME", directory.join("test-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .args(args)
        .output()
        .unwrap()
}

fn success(directory: &Path, args: &[&str]) -> Value {
    let output = run(directory, args);
    assert!(
        output.status.success(),
        "{args:?}: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], true);
    value
}

fn failure(directory: &Path, args: &[&str], code: &str, exit: i32) -> Value {
    let output = run(directory, args);
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}

fn init(directory: &Path) -> Value {
    success(directory, &["init", "--prefix", "WD", "--json"])
}

fn create(directory: &Path) -> Value {
    success(
        directory,
        &["issue", "create", "Implement parser", "--json"],
    )
}

fn id(value: &Value) -> &str {
    value["result"]["metadata"]["id"].as_str().unwrap()
}

#[test]
fn init_is_explicit_repeatable_and_leaves_legacy_root_absent() {
    let directory = repository();
    let first = init(directory.path());
    let config = fs::read(directory.path().join(".workdeck/config.yml")).unwrap();
    let repeated = init(directory.path());
    assert_eq!(first["source"], repeated["source"]);
    assert!(
        first["source"]["repository"]
            .as_str()
            .unwrap()
            .starts_with("repo-")
    );
    assert_eq!(
        fs::read(directory.path().join(".workdeck/config.yml")).unwrap(),
        config
    );
    assert!(!directory.path().join(".agents/workdeck").exists());
    failure(
        directory.path(),
        &["init", "--prefix", "OTHER", "--json"],
        "conflict",
        4,
    );
}

#[test]
fn native_reference_save_and_reads_never_fork_a_legacy_store() {
    let directory = repository();
    init(directory.path());
    for kind in ["project", "cycle", "label"] {
        let saved = success(directory.path(), &[kind, "save", "Release One", "--json"]);
        assert_eq!(saved["result"]["metadata"]["id"], "release-one");
        let shown = success(directory.path(), &[kind, "show", "release-one", "--json"]);
        assert_eq!(shown["result"], saved["result"]);
        let listed = success(directory.path(), &[kind, "list", "--json"]);
        assert_eq!(listed["result"][0], saved["result"]);
        assert!(!directory.path().join(".agents/workdeck").exists());
    }
    success(directory.path(), &["issue", "list", "--json"]);
}

#[test]
fn native_reference_commands_replay_check_source_and_retain_archived_identity() {
    let directory = repository();
    init(directory.path());
    for (kind, prefix) in [("project", "PRJ-"), ("cycle", "CYC-"), ("label", "LBL-")] {
        let args = [kind, "create", "First", "--request-id", kind, "--json"];
        let created = success(directory.path(), &args);
        let key = created["result"]["metadata"]["id"].as_str().unwrap();
        assert!(key.starts_with(prefix));
        let revision = created["result"]["source"]["revision"].to_string();
        let content = created["result"]["source"]["content"].as_str().unwrap();
        let updated = success(
            directory.path(),
            &[
                kind,
                "update",
                key,
                "--name",
                "Changed",
                "--expected-revision",
                &revision,
                "--expected-content",
                content,
                "--json",
            ],
        );
        assert_eq!(updated["result"]["metadata"]["name"], "Changed");
        assert_eq!(success(directory.path(), &args), created);
        failure(
            directory.path(),
            &[
                kind,
                "update",
                key,
                "--name",
                "Stale",
                "--expected-revision",
                &revision,
                "--expected-content",
                content,
                "--json",
            ],
            "stale_source",
            4,
        );
        let archived = success(directory.path(), &[kind, "archive", key, "--json"]);
        assert_eq!(archived["result"]["metadata"]["archived"], true);
        failure(
            directory.path(),
            &[kind, "create", "Reuse", "--id", key, "--json"],
            "conflict",
            4,
        );
        let restored = success(
            directory.path(),
            &[kind, "archive", key, "--restore", "--json"],
        );
        assert_eq!(restored["result"]["metadata"]["archived"], false);
    }
    assert!(!directory.path().join(".agents/workdeck").exists());
}

#[test]
fn native_reference_save_preserves_body_and_replay_result_after_updates() {
    let directory = repository();
    init(directory.path());
    let args = [
        "project",
        "save",
        "Release",
        "--description",
        "# Scope\nOriginal.\n",
        "--request-id",
        "save-once",
        "--json",
    ];
    let first = success(directory.path(), &args);
    let updated = success(
        directory.path(),
        &[
            "project",
            "save",
            "Release renamed",
            "--id",
            "release",
            "--status",
            "active",
            "--json",
        ],
    );
    assert_eq!(updated["result"]["body"], first["result"]["body"]);
    assert_eq!(success(directory.path(), &args), first);
    fs::write(
        directory.path().join("description.md"),
        "## Body file\nNew.\n",
    )
    .unwrap();
    let edited = success(
        directory.path(),
        &[
            "project",
            "update",
            "release",
            "--body-file",
            "description.md",
            "--json",
        ],
    );
    assert_eq!(edited["result"]["body"], "## Body file\nNew.\n");
    let filtered = success(
        directory.path(),
        &["project", "list", "--status", "active", "--json"],
    );
    assert_eq!(filtered["result"].as_array().unwrap().len(), 1);
    failure(
        directory.path(),
        &["label", "create", "Invalid", "--status", "active", "--json"],
        "invalid_schema",
        2,
    );
    assert!(!directory.path().join(".workdeck/labels.yml").exists());
}

#[test]
fn reference_native_flags_require_initialization_and_bad_sources_do_not_fall_back() {
    let directory = repository();
    for kind in ["project", "cycle", "label"] {
        failure(
            directory.path(),
            &[kind, "list", "--json"],
            "not_initialized",
            3,
        );
        failure(
            directory.path(),
            &[kind, "create", "New", "--json"],
            "not_initialized",
            3,
        );
        failure(
            directory.path(),
            &[kind, "save", "New", "--request-id", "native", "--json"],
            "not_initialized",
            3,
        );
    }
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".agents/workdeck").exists());
    init(directory.path());
    fs::write(
        directory.path().join(".workdeck/config.yml"),
        "schema: 999\n",
    )
    .unwrap();
    for kind in ["project", "cycle", "label"] {
        let output = run(directory.path(), &[kind, "save", "New", "--json"]);
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(error["api_version"], 1);
    }
    assert!(!directory.path().join(".agents/workdeck").exists());
}

#[test]
fn explicit_staging_preserves_unrelated_index_and_reports_committed_mutation_on_failure() {
    let directory = repository();
    init(directory.path());
    fs::write(directory.path().join("unrelated.txt"), "already staged\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "unrelated.txt"])
            .current_dir(directory.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(directory.path().join("unrelated.txt"), "unstaged edit\n").unwrap();
    let index_path = directory.path().join(".git/index");
    let index_before = fs::read(&index_path).unwrap();
    let lock = directory.path().join(".git/index.lock");
    fs::write(&lock, "other Git process").unwrap();
    let args = [
        "issue",
        "create",
        "Stage precisely",
        "--request-id",
        "stage-once",
        "--stage",
        "--json",
    ];
    let failed = failure(directory.path(), &args, "locked", 4);
    assert_eq!(failed["error"]["details"]["mutation_committed"], true);
    let receipt = failed["error"]["details"]["receipt"].clone();
    assert_eq!(fs::read(&index_path).unwrap(), index_before);
    fs::remove_file(&lock).unwrap();
    let staged = success(directory.path(), &args);
    assert_eq!(staged["receipt"], receipt);
    assert_eq!(staged["staging"]["index_changed"], true);
    let replayed = success(directory.path(), &args);
    assert_eq!(replayed["receipt"], receipt);
    assert_eq!(replayed["staging"]["index_changed"], false);
    let staged_names = Command::new("git")
        .args(["diff", "--cached", "--name-only", "-z"])
        .current_dir(directory.path())
        .output()
        .unwrap();
    let names: Vec<_> = staged_names
        .stdout
        .split(|b| *b == 0)
        .filter(|p| !p.is_empty())
        .collect();
    assert_eq!(names.len(), 3); // unrelated.txt, item.md, this operation receipt
    assert!(!names.contains(&b".workdeck/config.yml".as_slice()));
    let original = Command::new("git")
        .args(["show", ":unrelated.txt"])
        .current_dir(directory.path())
        .output()
        .unwrap();
    assert_eq!(original.stdout, b"already staged\n");
    assert_eq!(
        fs::read(directory.path().join("unrelated.txt")).unwrap(),
        b"unstaged edit\n"
    );
    failure(
        directory.path(),
        &["issue", "list", "--stage", "--json"],
        "invalid_input",
        2,
    );
    let issues = success(directory.path(), &["issue", "list", "--json"]);
    assert_eq!(issues["result"].as_array().unwrap().len(), 1);
}

#[test]
fn document_links_are_inert_unique_and_retry_safe() {
    let directory = repository();
    init(directory.path());
    let created = create(directory.path());
    let key = id(&created);
    let link = [
        "issue",
        "link-document",
        key,
        "docs/design.md#scope",
        "--request-id",
        "document-once",
        "--json",
    ];
    let first = success(directory.path(), &link);
    assert_eq!(
        first["result"]["metadata"]["documents"],
        json!(["docs/design.md#scope"])
    );
    success(
        directory.path(),
        &[
            "issue",
            "link-document",
            key,
            "https://example.invalid/spec",
            "--json",
        ],
    );
    assert_eq!(success(directory.path(), &link), first);
    let current = success(directory.path(), &["issue", "show", key, "--json"]);
    assert_eq!(
        current["result"]["metadata"]["documents"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let removed = success(
        directory.path(),
        &[
            "issue",
            "unlink-document",
            key,
            "docs/design.md#scope",
            "--json",
        ],
    );
    assert_eq!(
        removed["result"]["metadata"]["documents"],
        json!(["https://example.invalid/spec"])
    );
    assert!(!directory.path().join("docs/design.md").exists());
    failure(
        directory.path(),
        &[
            "issue",
            "link-document",
            key,
            "invalid\nreference",
            "--json",
        ],
        "invalid_input",
        2,
    );
    assert_eq!(
        success(directory.path(), &["issue", "show", key, "--json"])["result"],
        removed["result"]
    );
}

#[test]
fn reads_do_not_initialize_and_legacy_json_stays_compatible() {
    let directory = repository();
    failure(
        directory.path(),
        &["issue", "list", "--json"],
        "not_initialized",
        3,
    );
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".agents/workdeck").exists());
    failure(
        directory.path(),
        &["issue", "--no-input", "done", "WD-1", "--dry-run", "--json"],
        "not_initialized",
        3,
    );
    assert!(!directory.path().join(".workdeck").exists());
    // Legacy output is preserved for a real, explicitly existing legacy source.
    fs::create_dir_all(directory.path().join(".agents/workdeck/issues")).unwrap();
    for kind in ["issue", "project", "cycle", "label"] {
        let output = run(directory.path(), &[kind, "list", "--json"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["data"], json!([]));
        assert!(value.get("api_version").is_none());
    }
    assert!(!directory.path().join(".workdeck").exists());
}

#[test]
fn create_and_update_replay_returns_original_receipt_even_after_other_edits() {
    let directory = repository();
    init(directory.path());
    let args = [
        "issue",
        "--request-id",
        "create-once",
        "create",
        "Original",
        "--json",
    ];
    let first = success(directory.path(), &args);
    assert_eq!(success(directory.path(), &args), first);
    let key = id(&first);
    let update = [
        "issue",
        "update",
        key,
        "--title",
        "Changed",
        "--request-id",
        "update-once",
        "--json",
    ];
    let changed = success(directory.path(), &update);
    success(
        directory.path(),
        &["issue", "assign", key, "person", "--json"],
    );
    assert_eq!(success(directory.path(), &update), changed);
    assert_eq!(success(directory.path(), &args), first);
    failure(
        directory.path(),
        &[
            "issue",
            "create",
            "Different",
            "--request-id",
            "create-once",
            "--json",
        ],
        "idempotency_conflict",
        4,
    );
    let listed = success(directory.path(), &["issue", "list", "--json"]);
    assert_eq!(listed["result"].as_array().unwrap().len(), 1);
}

#[test]
fn expected_revision_and_content_detect_editor_changes_and_replay() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let revision = first["result"]["source"]["revision"].to_string();
    let content = first["result"]["source"]["content"].as_str().unwrap();
    let args = [
        "issue",
        "update",
        key,
        "--title",
        "Changed",
        "--expected-revision",
        &revision,
        "--expected-content",
        content,
        "--request-id",
        "explicit-once",
        "--json",
    ];
    let changed = success(directory.path(), &args);
    assert_eq!(success(directory.path(), &args), changed);
    failure(
        directory.path(),
        &[
            "issue",
            "update",
            key,
            "--title",
            "Stale",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
        "stale_source",
        4,
    );
    failure(
        directory.path(),
        &[
            "issue",
            "update",
            key,
            "--title",
            "Invalid",
            "--expected-revision",
            &revision,
            "--json",
        ],
        "invalid_input",
        2,
    );
    let current = success(directory.path(), &["issue", "show", key, "--json"]);
    let path = directory
        .path()
        .join(".workdeck")
        .join(current["result"]["path"].as_str().unwrap());
    let original = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{original}\nEditor changed the body.\n")).unwrap();
    let revision = current["result"]["source"]["revision"].to_string();
    let content = current["result"]["source"]["content"].as_str().unwrap();
    failure(
        directory.path(),
        &[
            "issue",
            "update",
            key,
            "--title",
            "Stale",
            "--expected-revision",
            &revision,
            "--expected-content",
            content,
            "--json",
        ],
        "stale_source",
        4,
    );
}

#[test]
fn comments_are_durable_idempotent_and_do_not_rewrite_issue() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let args = [
        "issue",
        "comment",
        key,
        "Keep original IDs",
        "--author",
        "reviewer",
        "--request-id",
        "comment-once",
        "--json",
    ];
    let comment = success(directory.path(), &args);
    assert_eq!(success(directory.path(), &args), comment);
    let comments = success(directory.path(), &["issue", "comments", key, "--json"]);
    assert_eq!(comments["result"].as_array().unwrap().len(), 1);
    assert_eq!(comments["result"][0]["body"], "Keep original IDs");
    let shown = success(directory.path(), &["issue", "show", key, "--json"]);
    assert_eq!(shown["result"]["source"], first["result"]["source"]);
}

#[test]
fn completion_preview_close_alias_reopen_and_archive_share_library_rules() {
    let directory = repository();
    init(directory.path());
    fs::write(directory.path().join("issue.json"), serde_json::to_vec(&json!({"title":"Acceptance issue", "fields":{"acceptance":[{"id":"tested","description":"Test passes","checked":false}]}})).unwrap()).unwrap();
    let first = success(
        directory.path(),
        &["issue", "create", "--from-json", "issue.json", "--json"],
    );
    let key = id(&first);
    let preview = success(
        directory.path(),
        &["issue", "done", key, "--dry-run", "--json"],
    );
    assert_eq!(preview["result"]["allowed"], false);
    failure(
        directory.path(),
        &["issue", "close", key, "--json"],
        "policy_blocked",
        5,
    );
    let ordinary = create(directory.path());
    let key = id(&ordinary);
    let completed = success(directory.path(), &["issue", "close", key, "--json"]);
    assert_eq!(completed["result"]["metadata"]["status"], "done");
    assert_eq!(
        success(directory.path(), &["issue", "reopen", key, "--json"])["result"]["metadata"]["status"],
        "ready"
    );
    assert_eq!(
        success(directory.path(), &["issue", "archive", key, "--json"])["result"]["metadata"]["archived"],
        true
    );
    let retired = success(
        directory.path(),
        &["issue", "delete", key, "--yes", "--json"],
    );
    assert_eq!(retired["result"]["tombstone"]["target"]["id"], key);
    assert_eq!(
        success(directory.path(), &["issue", "show", key, "--json"])["result"]["retirement"]["target"]
            ["id"],
        key
    );
}

#[test]
fn link_assignment_and_label_commands_keep_atomic_replay_and_existing_commits() {
    let directory = repository();
    init(directory.path());
    success(
        directory.path(),
        &["label", "create", "Bug", "--id", "bug", "--json"],
    );
    let first = create(directory.path());
    let key = id(&first);
    let file_args = [
        "issue",
        "link-file",
        key,
        "src/lib.rs",
        "--request-id",
        "link-once",
        "--json",
    ];
    let link = success(directory.path(), &file_args);
    success(
        directory.path(),
        &["issue", "link", key, "src/main.rs", "--json"],
    );
    assert_eq!(success(directory.path(), &file_args), link);
    success(
        directory.path(),
        &["issue", "link-commit", key, "abcdef1", "--json"],
    );
    let update_args = [
        "issue",
        "update",
        key,
        "--title",
        "Both commits",
        "--commit",
        "abcdef2",
        "--request-id",
        "update-append",
        "--json",
    ];
    let updated = success(directory.path(), &update_args);
    assert_eq!(
        updated["result"]["metadata"]["commits"],
        json!(["abcdef1", "abcdef2"])
    );
    success(
        directory.path(),
        &["issue", "label", "add", key, "bug", "--json"],
    );
    assert_eq!(success(directory.path(), &update_args), updated);
    success(
        directory.path(),
        &["issue", "assign", key, "agent", "--json"],
    );
    let unassigned = success(directory.path(), &["issue", "unassign", key, "--json"]);
    assert!(unassigned["result"]["metadata"].get("assignee").is_none());
    let removed = success(
        directory.path(),
        &["issue", "unlink-file", key, "src/lib.rs", "--json"],
    );
    assert_eq!(
        removed["result"]["metadata"]["files"],
        json!([{"path":"src/main.rs"}])
    );
}

#[test]
fn body_files_and_reporter_reviewer_fields_work_without_interaction() {
    let directory = repository();
    init(directory.path());
    fs::write(directory.path().join("body.md"), "# Context\n\nDetails.\n").unwrap();
    let first = success(
        directory.path(),
        &[
            "issue",
            "--no-input",
            "create",
            "Body test",
            "--body-file",
            "body.md",
            "--reporter",
            "person",
            "--reviewer",
            "maintainer",
            "--json",
        ],
    );
    assert_eq!(first["result"]["body"], "# Context\n\nDetails.\n");
    assert_eq!(first["result"]["metadata"]["reporter"], "person");
    assert_eq!(first["result"]["metadata"]["reviewer"], "maintainer");
}

#[test]
fn malformed_native_source_never_falls_back_to_legacy_mutation() {
    let directory = repository();
    init(directory.path());
    fs::write(
        directory.path().join(".workdeck/config.yml"),
        "schema: [broken",
    )
    .unwrap();
    failure(
        directory.path(),
        &["issue", "create", "Must not create", "--json"],
        "invalid_schema",
        2,
    );
    assert!(!directory.path().join(".agents/workdeck").exists());
}

#[test]
fn missing_native_config_never_falls_back_to_legacy_writer() {
    let directory = repository();
    init(directory.path());
    create(directory.path());
    fs::remove_file(directory.path().join(".workdeck/config.yml")).unwrap();
    failure(
        directory.path(),
        &["issue", "create", "Must preserve damaged source", "--json"],
        "not_initialized",
        3,
    );
    assert!(!directory.path().join(".agents/workdeck").exists());
}

#[cfg(unix)]
#[test]
fn body_and_json_inputs_reject_special_files_without_waiting_for_a_writer() {
    use std::time::Duration;
    let directory = repository();
    init(directory.path());
    assert!(
        Command::new("mkfifo")
            .arg(directory.path().join("input"))
            .status()
            .unwrap()
            .success()
    );
    for option in ["--body-file", "--from-json"] {
        let output = assert_cmd::Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(directory.path())
            .timeout(Duration::from_secs(5))
            .args([
                "issue",
                "create",
                "Special input",
                option,
                "input",
                "--json",
            ])
            .assert()
            .code(1)
            .get_output()
            .clone();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], "unsafe_path");
    }
    assert!(
        success(directory.path(), &["issue", "list", "--json"])["result"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn binary_attachments_list_replay_and_leave_issue_source_unchanged() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let bytes = b"\0\xff\x1b]52;c;never-render-this\x07";
    fs::write(directory.path().join("capture.bin"), bytes).unwrap();
    let revision = first["result"]["source"]["revision"].to_string();
    let content = first["result"]["source"]["content"].as_str().unwrap();
    let args = [
        "issue",
        "attach",
        key,
        "capture.bin",
        "--author",
        "reviewer",
        "--name",
        "evidence.bin",
        "--media-type",
        "application/octet-stream",
        "--request-id",
        "attach-once",
        "--expected-revision",
        &revision,
        "--expected-content",
        content,
        "--json",
    ];
    let attached = success(directory.path(), &args);
    assert_eq!(attached["result"]["name"], "evidence.bin");
    assert_eq!(attached["result"]["size"], bytes.len());
    assert!(!attached.to_string().contains("never-render-this"));
    let stored = directory
        .path()
        .join(".workdeck")
        .join(attached["result"]["content_path"].as_str().unwrap());
    assert_eq!(fs::read(stored).unwrap(), bytes);
    let shown = success(directory.path(), &["issue", "show", key, "--json"]);
    assert_eq!(shown["result"]["source"], first["result"]["source"]);
    success(
        directory.path(),
        &["issue", "update", key, "--title", "Later edit", "--json"],
    );
    assert_eq!(success(directory.path(), &args), attached);
    let listed = success(directory.path(), &["issue", "attachments", key, "--json"]);
    assert_eq!(listed["result"], json!([attached["result"]]));
    failure(
        directory.path(),
        &[
            "issue",
            "attach",
            key,
            "capture.bin",
            "--author",
            "reviewer",
            "--name",
            "../escape.bin",
            "--json",
        ],
        "invalid_input",
        2,
    );
    let oversized = fs::File::create(directory.path().join("large.bin")).unwrap();
    oversized.set_len(20 * 1024 * 1024 + 1).unwrap();
    failure(
        directory.path(),
        &[
            "issue",
            "attach",
            key,
            "large.bin",
            "--author",
            "reviewer",
            "--json",
        ],
        "invalid_input",
        2,
    );
}

#[test]
fn structured_input_supports_cli_overrides_and_explicit_field_removal() {
    let directory = repository();
    init(directory.path());
    fs::write(
        directory.path().join("input.json"),
        r#"{"fields":{"priority":"urgent"},"body":"from JSON"}"#,
    )
    .unwrap();
    let first = success(
        directory.path(),
        &[
            "issue",
            "create",
            "CLI title",
            "--from-json",
            "input.json",
            "--json",
        ],
    );
    assert_eq!(first["result"]["metadata"]["title"], "CLI title");
    assert_eq!(first["result"]["metadata"]["priority"], "urgent");
    let key = id(&first);
    fs::write(
        directory.path().join("input.json"),
        r#"{"fields":{"title":"JSON title","assignee":"agent"},"body":"updated body"}"#,
    )
    .unwrap();
    let updated = success(
        directory.path(),
        &[
            "issue",
            "update",
            key,
            "--from-json",
            "input.json",
            "--title",
            "CLI override",
            "--json",
        ],
    );
    assert_eq!(updated["result"]["metadata"]["title"], "CLI override");
    assert_eq!(updated["result"]["body"], "updated body");
    fs::write(
        directory.path().join("input.json"),
        r#"{"fields":{"assignee":null}}"#,
    )
    .unwrap();
    let removed = success(
        directory.path(),
        &[
            "issue",
            "update",
            key,
            "--from-json",
            "input.json",
            "--json",
        ],
    );
    assert!(removed["result"]["metadata"].get("assignee").is_none());
    assert_eq!(removed["result"]["body"], "updated body");
}

#[test]
fn edit_from_file_is_replayable_after_later_edits_and_preserves_header_comments() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let path = directory
        .path()
        .join(".workdeck")
        .join(first["result"]["path"].as_str().unwrap());
    let original = fs::read_to_string(&path).unwrap();
    let draft = original
        .replace("Implement parser", "Edited parser")
        .replacen("---\n", "---\n# editor note\n", 1)
        + "\nEditor body.\n";
    fs::write(directory.path().join("draft.md"), &draft).unwrap();
    let args = [
        "issue",
        "edit",
        key,
        "--from-file",
        "draft.md",
        "--no-input",
        "--request-id",
        "file-edit-once",
        "--json",
    ];
    let edited = success(directory.path(), &args);
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("# editor note\n")
    );
    assert!(
        edited["result"]["body"]
            .as_str()
            .unwrap()
            .contains("Editor body.")
    );
    success(
        directory.path(),
        &["issue", "update", key, "--title", "Later change", "--json"],
    );
    assert_eq!(success(directory.path(), &args), edited);
    assert_eq!(
        fs::read_to_string(directory.path().join("draft.md")).unwrap(),
        draft
    );
}

#[test]
fn edit_without_input_rejects_no_input_and_keeps_authority_unchanged() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    failure(
        directory.path(),
        &["issue", "edit", key, "--no-input", "--json"],
        "invalid_input",
        2,
    );
    let error = failure(
        directory.path(),
        &[
            "issue",
            "edit",
            key,
            "--request-id",
            "interactive-retry",
            "--json",
        ],
        "invalid_input",
        2,
    );
    assert!(
        error["error"]["hint"]
            .as_str()
            .unwrap()
            .contains("--from-file")
    );
    assert_eq!(
        success(directory.path(), &["issue", "show", key, "--json"])["result"]["source"],
        first["result"]["source"]
    );
}

#[cfg(unix)]
fn controlled_editor(directory: &Path, key: &str, script: &str, extra: &[&str]) -> Output {
    use std::{os::unix::fs::PermissionsExt, time::Duration};
    let editor_path = directory.join("controlled editor.sh");
    fs::write(&editor_path, format!("#!/bin/sh\nset -eu\n{script}\n")).unwrap();
    fs::set_permissions(&editor_path, fs::Permissions::from_mode(0o700)).unwrap();
    let editor = format!("'{}' 'argument with spaces'", editor_path.display());
    assert_cmd::Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(directory)
        .env("XDG_CONFIG_HOME", directory.join("test-config"))
        .env("EDITOR", editor)
        .env_remove("VISUAL")
        .timeout(Duration::from_secs(10))
        .args(["issue", "edit", key, "--json"])
        .args(extra)
        .output()
        .unwrap()
}

#[cfg(unix)]
fn retained_editor_draft(value: &Value) -> std::path::PathBuf {
    let hint = value["error"]["hint"].as_str().unwrap();
    let path = hint
        .lines()
        .find_map(|line| line.strip_prefix("Draft retained at: "))
        .unwrap();
    let path = std::path::PathBuf::from(path);
    assert!(path.is_file(), "{hint}");
    path
}

#[cfg(unix)]
#[test]
fn controlled_editor_publishes_valid_edit_with_separate_path_argument() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let output = controlled_editor(
        directory.path(),
        key,
        r#"
[ "$1" = 'argument with spaces' ]
printf '\nExternal editor body.\n' >> "$2"
printf '%s' "$2" > last-draft-path
"#,
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        value["result"]["body"]
            .as_str()
            .unwrap()
            .contains("External editor body.")
    );
    let draft = fs::read_to_string(directory.path().join("last-draft-path")).unwrap();
    assert!(!Path::new(&draft).exists());
}

#[cfg(unix)]
#[test]
fn controlled_editor_failures_retain_drafts_and_never_publish_invalid_changes() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let path = directory
        .path()
        .join(".workdeck")
        .join(first["result"]["path"].as_str().unwrap());
    let original = fs::read(&path).unwrap();
    for (script, code, exit) in [
        (
            "printf '%s\\n' '---' 'title: [broken' '---' > \"$2\"",
            "invalid_schema",
            2,
        ),
        (
            "sed 's/^revision: .*/revision: 999/' \"$2\" > \"$2.new\"; mv \"$2.new\" \"$2\"",
            "invalid_input",
            2,
        ),
        (
            "printf '\\nAttempt before failure.\\n' >> \"$2\"; exit 7",
            "canceled",
            130,
        ),
    ] {
        let output = controlled_editor(directory.path(), key, script, &[]);
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["error"]["code"], code);
        assert_eq!(fs::read(&path).unwrap(), original);
        let draft = retained_editor_draft(&value);
        fs::remove_dir_all(draft.parent().unwrap()).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn controlled_editor_detects_external_source_changes_during_edit() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let path = directory
        .path()
        .join(".workdeck")
        .join(first["result"]["path"].as_str().unwrap());
    fs::write(
        directory.path().join("authoritative-path"),
        path.to_str().unwrap(),
    )
    .unwrap();
    let output = controlled_editor(
        directory.path(),
        key,
        r#"
printf '\nEditor draft.\n' >> "$2"
source_path=$(cat authoritative-path)
printf '\nConcurrent human edit.\n' >> "$source_path"
"#,
        &[],
    );
    assert_eq!(output.status.code(), Some(4));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "stale_source");
    let authoritative = fs::read_to_string(&path).unwrap();
    assert!(authoritative.contains("Concurrent human edit."));
    assert!(!authoritative.contains("Editor draft."));
    let draft = retained_editor_draft(&value);
    assert!(
        fs::read_to_string(&draft)
            .unwrap()
            .contains("Editor draft.")
    );
    fs::remove_dir_all(draft.parent().unwrap()).unwrap();
}

fn native_issue_template(directory: &Path) -> std::path::PathBuf {
    let path = directory.join(".workdeck/templates/issues/bug.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "---\nschema: 1\nid: bug\nname: Bug report\ndefaults:\n  priority: high\n  labels: [bug]\n---\n# Reproduction\n\nTemplate body.\n").unwrap();
    path
}

#[test]
fn templates_list_and_create_preserve_defaults_overrides_and_replay() {
    let directory = repository();
    init(directory.path());
    for label in ["bug", "cli"] {
        success(
            directory.path(),
            &["label", "create", label, "--id", label, "--json"],
        );
    }
    let empty = success(directory.path(), &["issue", "templates", "--json"]);
    assert_eq!(empty["result"], json!([]));
    assert!(!directory.path().join(".workdeck/templates").exists());
    let template_path = native_issue_template(directory.path());
    let templates = success(directory.path(), &["issue", "templates", "--json"]);
    assert_eq!(templates["result"][0]["id"], "bug");
    let args = [
        "issue",
        "create",
        "From template",
        "--template",
        "bug",
        "--request-id",
        "template-once",
        "--json",
    ];
    let created = success(directory.path(), &args);
    assert_eq!(created["result"]["metadata"]["priority"], "high");
    assert_eq!(created["result"]["metadata"]["labels"], json!(["bug"]));
    assert_eq!(
        created["result"]["body"],
        "# Reproduction\n\nTemplate body.\n"
    );
    let overridden = success(
        directory.path(),
        &[
            "issue",
            "create",
            "Override defaults",
            "--template",
            "bug",
            "--priority",
            "urgent",
            "--label",
            "cli",
            "--description",
            "Own body",
            "--json",
        ],
    );
    assert_eq!(overridden["result"]["metadata"]["priority"], "urgent");
    assert_eq!(overridden["result"]["metadata"]["labels"], json!(["cli"]));
    assert_eq!(overridden["result"]["body"], "Own body");
    fs::write(
        template_path,
        "Broken template after initial successful request",
    )
    .unwrap();
    assert_eq!(success(directory.path(), &args), created);
    failure(
        directory.path(),
        &[
            "issue",
            "create",
            "Different input",
            "--template",
            "bug",
            "--request-id",
            "template-once",
            "--json",
        ],
        "idempotency_conflict",
        4,
    );
}

#[test]
fn template_creation_distinguishes_omitted_and_explicitly_empty_bodies() {
    let directory = repository();
    init(directory.path());
    success(
        directory.path(),
        &["label", "create", "Bug", "--id", "bug", "--json"],
    );
    native_issue_template(directory.path());
    fs::write(
        directory.path().join("fields.json"),
        r#"{"fields":{"priority":"low"}}"#,
    )
    .unwrap();
    let inherited = success(
        directory.path(),
        &[
            "issue",
            "create",
            "JSON fields",
            "--template",
            "bug",
            "--from-json",
            "fields.json",
            "--json",
        ],
    );
    assert_eq!(
        inherited["result"]["body"],
        "# Reproduction\n\nTemplate body.\n"
    );
    fs::write(directory.path().join("empty.md"), "").unwrap();
    fs::write(
        directory.path().join("empty.json"),
        r#"{"fields":{},"body":""}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("legacy-empty.json"),
        r#"{"description":""}"#,
    )
    .unwrap();
    for (option, value) in [
        ("--description", ""),
        ("--body-file", "empty.md"),
        ("--from-json", "empty.json"),
        ("--from-json", "legacy-empty.json"),
    ] {
        let cleared = success(
            directory.path(),
            &[
                "issue",
                "create",
                "Explicit empty",
                "--template",
                "bug",
                option,
                value,
                "--json",
            ],
        );
        assert_eq!(cleared["result"]["body"], "", "{option} {value}");
    }
}

#[cfg(unix)]
#[test]
fn editor_banner_is_separate_from_json_stdout() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let output = controlled_editor(
        directory.path(),
        id(&first),
        r#"
printf 'editor banner\n'
printf '\nEdited with banner.\n' >> "$2"
"#,
        &[],
    );
    assert!(output.status.success());
    let value: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{error}: {}", String::from_utf8_lossy(&output.stdout)));
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], true);
    assert!(String::from_utf8_lossy(&output.stderr).contains("editor banner\n"));
}

#[test]
fn human_malformed_document_errors_include_sanitized_source_location() {
    let directory = repository();
    init(directory.path());
    fs::write(
        directory.path().join(".workdeck/config.yml"),
        "schema: [broken\n",
    )
    .unwrap();
    let diagnostic = failure(
        directory.path(),
        &["issue", "list", "--json"],
        "invalid_schema",
        2,
    );
    let error = &diagnostic["error"];
    let location = format!(
        "{}:{}:{}",
        workdeck_diff::format_terminal_path(error["path"].as_str().unwrap()),
        error["line"],
        error["column"]
    );
    let output = run(directory.path(), &["issue", "list"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(&location),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn human_error_paths_escape_terminal_controls() {
    let directory = tempfile::Builder::new()
        .prefix("workdeck-\x1b[31m-path-")
        .tempdir()
        .unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(directory.path())
            .status()
            .unwrap()
            .success()
    );
    init(directory.path());
    fs::write(
        directory.path().join(".workdeck/config.yml"),
        "schema: [broken\n",
    )
    .unwrap();
    let output = run(directory.path(), &["issue", "list"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains('\x1b'),
        "terminal control leaked in diagnostic"
    );
    assert!(stderr.contains("workdeck-\\x1b[31m-path-"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn editor_cannot_publish_after_repository_identity_changes() {
    let directory = repository();
    init(directory.path());
    let first = create(directory.path());
    let key = id(&first);
    let item_path = directory
        .path()
        .join(".workdeck")
        .join(first["result"]["path"].as_str().unwrap());
    let original = fs::read(&item_path).unwrap();
    let replacement = repository();
    init(replacement.path());
    fs::copy(
        replacement.path().join(".workdeck/config.yml"),
        directory.path().join("replacement-config.yml"),
    )
    .unwrap();
    let output = controlled_editor(
        directory.path(),
        key,
        r#"
printf '\nWrong repository draft.\n' >> "$2"
cp replacement-config.yml .workdeck/config.yml
"#,
        &[],
    );
    assert_eq!(
        output.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "stale_source");
    assert_eq!(fs::read(item_path).unwrap(), original);
    let draft = retained_editor_draft(&value);
    fs::remove_dir_all(draft.parent().unwrap()).unwrap();
}
