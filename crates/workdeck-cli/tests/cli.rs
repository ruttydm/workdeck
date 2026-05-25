use assert_cmd::prelude::*;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn workdeck() -> Command {
    let mut command = Command::cargo_bin("workdeck").unwrap();
    command.env("HOME", "/nonexistent/workdeck-test-home");
    command
}

#[test]
fn help_renders() {
    workdeck()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Terminal-native sidecar"));
}

#[test]
fn version_renders() {
    workdeck()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn init_creates_agents_workdeck_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--init")
        .assert()
        .success()
        .stdout(predicate::str::contains(".agents/workdeck"));

    assert!(dir.path().join(".agents/workdeck/config.toml").exists());
    assert!(dir.path().join(".agents/workdeck/issues").is_dir());
    assert!(dir.path().join(".agents/workdeck/agents").is_dir());
    let config = fs::read_to_string(dir.path().join(".agents/workdeck/config.toml")).unwrap();
    assert!(config.contains("group_changes = \"g\""));
    assert!(config.contains("toggle_dirstat = \"w\""));
    assert!(config.contains("[git]"));
    assert!(config.contains("git = \"G\""));
    assert!(config.contains("recent_commits = 30"));
}

#[test]
fn status_json_reports_untracked_files_without_mutating_git() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::write(dir.path().join("new-file.txt"), "hello\n").unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--status-json")
        .assert()
        .success()
        .stdout(predicate::str::contains("new-file.txt"))
        .stdout(predicate::str::contains("untracked"))
        .stdout(predicate::str::contains("\"counts\""))
        .stdout(predicate::str::contains("\"groups\""));

    let status = Command::new("git")
        .arg("-C")
        .arg(dir.path())
        .arg("status")
        .arg("--short")
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&status.stdout), "?? new-file.txt\n");
}

#[test]
fn status_json_reports_compact_stage_label() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    git(
        dir.path(),
        &["config", "user.email", "workdeck@example.test"],
    );
    git(dir.path(), &["config", "user.name", "Workdeck Test"]);
    fs::write(dir.path().join("file.txt"), "one\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "initial"]);

    fs::write(dir.path().join("file.txt"), "one\ntwo\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    fs::write(dir.path().join("file.txt"), "one\ntwo\nthree\n").unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .arg("--status-json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"stage\": \"staged+unstaged\""))
        .stdout(predicate::str::contains("\"staged\": true"))
        .stdout(predicate::str::contains("\"unstaged\": true"));
}

#[test]
fn doctor_reports_valid_repo_without_creating_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"ok\": true"))
        .stdout(predicate::str::contains("\"name\": \"config\""));

    assert!(!dir.path().join(".agents/workdeck").exists());
}

#[test]
fn invalid_config_keybinding_fails_before_tui() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::create_dir_all(dir.path().join(".agents/workdeck")).unwrap();
    fs::write(
        dir.path().join(".agents/workdeck/config.toml"),
        r#"
        [keys]
        quit = "q"
        files = "q"
        "#,
    )
    .unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("fail  config"))
        .stdout(predicate::str::contains("duplicate key binding"))
        .stderr(predicate::str::contains("doctor found failed checks"));
}

#[test]
fn doctor_reports_corrupt_store_data_as_failed_checks() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::create_dir_all(dir.path().join(".agents/workdeck/issues")).unwrap();
    fs::write(
        dir.path().join(".agents/workdeck/issues/WD-1.toml"),
        "not valid toml =",
    )
    .unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor", "--json"])
        .assert()
        .failure()
        .stdout(predicate::str::contains("\"ok\": false"))
        .stdout(predicate::str::contains("\"code\": \"doctor_failed\""))
        .stdout(predicate::str::contains("\"name\": \"issues\""))
        .stdout(predicate::str::contains("failed to parse issue"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn issue_commands_manage_file_backed_issues() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "create",
            "Render changes",
            "--status",
            "in-progress",
            "--priority",
            "high",
            "--due-at",
            "2026-05-31",
            "--label",
            "git,mvp",
            "--commit",
            "abc123",
            "--file",
            "src/main.rs",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"WD-1\""))
        .stdout(predicate::str::contains("\"status\": \"in-progress\""))
        .stdout(predicate::str::contains("\"due_at\": \"2026-05-31\""))
        .stdout(predicate::str::contains("\"abc123\""))
        .stdout(predicate::str::contains("\"src/main.rs\""));

    assert!(
        dir.path()
            .join(".agents/workdeck/issues/WD-1.toml")
            .exists()
    );

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "update",
            "WD-1",
            "--title",
            "Render nested changes",
            "--priority",
            "urgent",
            "--commit",
            "def456,abc123",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("Render nested changes"))
        .stdout(predicate::str::contains("\"priority\": \"urgent\""))
        .stdout(predicate::str::contains("\"def456\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "link", "WD-1", "src/lib.rs", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"src/lib.rs\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WD-1"))
        .stdout(predicate::str::contains("Render nested changes"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "show", "WD-1", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"WD-1\""))
        .stdout(predicate::str::contains("\"src/main.rs\""))
        .stdout(predicate::str::contains("\"src/lib.rs\""))
        .stdout(predicate::str::contains("\"abc123\""))
        .stdout(predicate::str::contains("\"def456\""));
}

#[test]
fn issue_command_rejects_invalid_status() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "create", "Bad status", "--status", "wat"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown status"));
}

#[test]
fn reference_commands_manage_projects_cycles_and_labels() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "project",
            "save",
            "Workdeck MVP",
            "--description",
            "Initial local release",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"workdeck-mvp\""))
        .stdout(predicate::str::contains("Initial local release"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "cycle",
            "save",
            "MVP",
            "--id",
            "mvp",
            "--starts-at",
            "2026-05-24",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"starts_at\": \"2026-05-24\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["label", "save", "Git", "--color", "green", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"git\""))
        .stdout(predicate::str::contains("\"color\": \"green\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["project", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("workdeck-mvp"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["doctor", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "1 project(s), 1 cycle(s), 1 label(s)",
        ));

    assert!(dir.path().join(".agents/workdeck/projects.toml").exists());
    assert!(dir.path().join(".agents/workdeck/cycles.toml").exists());
    assert!(dir.path().join(".agents/workdeck/labels.toml").exists());
}

#[test]
fn agent_commands_record_list_and_show_sessions() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "agent",
            "record",
            "Implement shell",
            "--id",
            "session-1",
            "--agent",
            "codex",
            "--status",
            "done",
            "--goal",
            "Build TUI shell",
            "--summary",
            "Implemented tabs",
            "--plan",
            "Inspect repo",
            "--plan",
            "Build shell",
            "--file",
            "src/main.rs",
            "--command",
            "cargo test",
            "--test",
            "cargo test",
            "--note",
            "Continue with previews",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"session-1\""))
        .stdout(predicate::str::contains("\"src/main.rs\""))
        .stdout(predicate::str::contains("Inspect repo"));

    assert!(
        dir.path()
            .join(".agents/workdeck/agents/session-1.toml")
            .exists()
    );

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session-1"))
        .stdout(predicate::str::contains("Implement shell"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "show", "session-1", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"agent\": \"codex\""))
        .stdout(predicate::str::contains("Continue with previews"));
}

#[test]
fn agent_import_reads_jsonl_sessions() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    let jsonl = dir.path().join("sessions.jsonl");
    fs::write(
        &jsonl,
        r#"{"session":{"id":"session-jsonl-1","title":"Imported JSONL","agent":"codex","cwd":"/tmp/workdeck","status":"done","started_at":"2026-05-24T12:00:00Z","goal":"Import logs","plan":["parse jsonl"],"touched_files":[{"path":"src/main.rs","change_type":"modified"}],"tests_run":["cargo test"],"handoff_notes":["review import"]}}
{"id":"session-jsonl-2","title":"Imported direct","agent":"codex","status":"active","started_at":"2026-05-24T13:00:00Z"}
"#,
    )
    .unwrap();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "import"])
        .arg(&jsonl)
        .arg("--json")
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"session-jsonl-1\""))
        .stdout(predicate::str::contains("\"id\": \"session-jsonl-2\""))
        .stdout(predicate::str::contains("parse jsonl"));

    assert!(
        dir.path()
            .join(".agents/workdeck/agents/session-jsonl-1.toml")
            .exists()
    );

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("session-jsonl-1"))
        .stdout(predicate::str::contains("session-jsonl-2"));
}

#[test]
fn export_emits_json_and_jsonl_without_mutating_empty_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["export"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"issues\": []"))
        .stdout(predicate::str::contains("\"agent_sessions\": []"));

    assert!(!dir.path().join(".agents/workdeck").exists());

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "create",
            "Export local data",
            "--project",
            "workdeck",
            "--json",
        ])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "record", "Export run", "--id", "export-run"])
        .assert()
        .success();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["export", "--jsonl"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"kind\":\"issue\""))
        .stdout(predicate::str::contains("\"kind\":\"agent_session\""))
        .stdout(predicate::str::contains("\"kind\":\"event\""));
}

#[test]
fn repo_read_only_commands_do_not_create_store() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "ignore me\n").unwrap();

    let output = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["files", "list", "--json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let files: Value = serde_json::from_slice(&output).unwrap();
    assert!(files.to_string().contains("README.md"));
    assert!(files.to_string().contains("src"));
    assert!(!files.to_string().contains("ignored.txt"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["files", "show", "README.md", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"content\": \"hello\\n\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"changes\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["changes", "list", "--group", "status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"untracked\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["search", "main", "--target", "files", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("src/main.rs"));

    assert!(!dir.path().join(".agents/workdeck").exists());
}

#[test]
fn json_commands_use_success_and_error_envelopes() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    fs::write(dir.path().join("README.md"), "hello\n").unwrap();

    let status = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["status", "--json"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let status_json: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status_json["ok"], true);
    assert_eq!(status_json["kind"], "status");
    assert!(status_json["data"]["changes"].is_array());

    let created = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "create", "Envelope issue", "--json"])
        .output()
        .unwrap();
    assert!(created.status.success());
    let created_json: Value = serde_json::from_slice(&created.stdout).unwrap();
    assert_eq!(created_json["ok"], true);
    assert_eq!(created_json["kind"], "issue");
    assert_eq!(created_json["action"], "create");
    assert_eq!(created_json["data"]["key"], "WD-1");

    let missing = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "show", "WD-404", "--json"])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(3));
    let missing_json: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert_eq!(missing_json["ok"], false);
    assert_eq!(missing_json["error"]["code"], "not_found");
    assert!(
        missing_json["error"]["message"]
            .as_str()
            .unwrap()
            .contains("WD-404")
    );
    assert!(String::from_utf8_lossy(&missing.stderr).is_empty());
}

#[test]
fn issue_commands_cover_headless_lifecycle() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "create", "Ship CLI", "--file", "src/main.rs"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WD-1"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "assign", "WD-1", "rutger"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "label", "add", "WD-1", "cli"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "link-commit", "WD-1", "abc123"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "close", "WD-1"])
        .assert()
        .success();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue", "list", "--status", "done", "--label", "cli", "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"WD-1\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "unlink-file", "WD-1", "src/main.rs"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "unlink-commit", "WD-1", "abc123"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "reopen", "WD-1"])
        .assert()
        .success()
        .stdout(predicate::str::contains("WD-1"));

    let issue_json = dir.path().join("issue.json");
    fs::write(
        &issue_json,
        r#"{
          "title": "JSON issue",
          "description": "Created without shell quoting",
          "status": "todo",
          "labels": ["json"],
          "linked_files": ["src/json.rs"]
        }"#,
    )
    .unwrap();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args([
            "issue",
            "create",
            "--from-json",
            issue_json.to_str().unwrap(),
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"title\": \"JSON issue\""))
        .stdout(predicate::str::contains("src/json.rs"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "show", "WD-404"])
        .assert()
        .code(3)
        .stderr(predicate::str::contains("issue WD-404 does not exist"));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["issue", "delete", "WD-1", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("deleted WD-1"));
}

#[test]
fn reference_agent_config_events_and_import_commands_are_headless() {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["project", "save", "Workdeck", "--id", "workdeck"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["cycle", "save", "MVP", "--id", "mvp"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["label", "save", "CLI", "--id", "cli", "--color", "green"])
        .assert()
        .success();

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["project", "show", "workdeck", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"workdeck\""));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["label", "list", "--color", "green", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"id\": \"cli\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "record", "Run CLI", "--id", "run-cli"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "append-plan", "run-cli", "Add commands"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "add-file", "run-cli", "src/main.rs"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["agent", "finish", "run-cli", "--summary", "Done", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"done\""));

    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "set", "ui.preview", "false", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"set\": true"));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "get", "ui.preview", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"value\": false"));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "set", "keys.tasks", "I", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\": \"keys.issues\""));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["config", "get", "keys.issues", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"value\": \"I\""));
    workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["events", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("agent_session_saved"));

    let export = workdeck()
        .arg("--cwd")
        .arg(dir.path())
        .args(["export"])
        .output()
        .unwrap();
    assert!(export.status.success());
    let export_path = dir.path().join("export.json");
    fs::write(&export_path, export.stdout).unwrap();

    let import_dir = tempdir().unwrap();
    git(import_dir.path(), &["init"]);
    workdeck()
        .arg("--cwd")
        .arg(import_dir.path())
        .args([
            "import",
            export_path.to_str().unwrap(),
            "--dry-run",
            "--json",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"projects\": 1"));
    workdeck()
        .arg("--cwd")
        .arg(import_dir.path())
        .args(["import", export_path.to_str().unwrap(), "--replace"])
        .assert()
        .success();
    workdeck()
        .arg("--cwd")
        .arg(import_dir.path())
        .args(["agent", "show", "run-cli"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Run CLI"));
}

fn git(cwd: &std::path::Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success());
}
