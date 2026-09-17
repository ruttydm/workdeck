use assert_cmd::prelude::*;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use workdeck_pm::{CreateIssue, CreatePlanning, IssueRecord, PlanningKind, Repository, RequestId};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .stdin(Stdio::null())
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
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["api_version"], 1);
    value
}
fn fail(root: &Path, args: &[&str], code: &str) -> Value {
    let out = run(root, args);
    assert!(!out.status.success(), "{args:?}");
    let value: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "{args:?}: {} {}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}
fn root() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    dir
}
fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let dir = root();
    let repo = Repository::init(dir.path(), "WD").unwrap();
    let mut cycle = CreatePlanning::new("Current cycle");
    cycle.id = Some("current".into());
    repo.create_planning(PlanningKind::Cycle, &cycle, &RequestId::new())
        .unwrap();
    let mut input = CreateIssue::new("Time subject", "Keep item bytes\n");
    input
        .fields
        .insert("cycle".into(), serde_json::json!("current"));
    let issue =
        serde_json::from_value(repo.create_issue(&input, &RequestId::new()).unwrap().result)
            .unwrap();
    (dir, repo, issue)
}
fn log_args(id: &str) -> Vec<&str> {
    vec![
        "time",
        "log",
        id,
        "--seconds",
        "60",
        "--worked-at",
        "2026-09-01T10:00:00Z",
        "--user",
        "worker",
        "--actor",
        "recorder",
        "--request-id",
        "time-once",
        "--json",
    ]
}
fn authority(repo: &Repository) -> BTreeMap<PathBuf, Vec<u8>> {
    repo.export_snapshot()
        .unwrap()
        .files
        .into_iter()
        .map(|file| (file.path, file.content))
        .collect()
}

#[test]
fn time_cli_logs_amends_reports_and_replays_without_touching_issue_bytes() {
    let (dir, repo, issue) = fixture();
    let root = dir.path();
    let id = issue.metadata.id.as_str();
    let original = fs::read(repo.root().join(&issue.path)).unwrap();
    let mut log = log_args(id);
    let revision = issue.source.revision.get().to_string();
    log.extend([
        "--expected-revision",
        &revision,
        "--expected-content",
        issue.source.content.as_str(),
    ]);
    let first = ok(root, &log);
    let time_id = first["result"]["entry"]["id"].as_str().unwrap();
    let content = first["result"]["content"].as_str().unwrap();
    assert_eq!(first["result"]["entry"]["cycle"], "current");
    assert_eq!(first["receipt"]["changed"].as_array().unwrap().len(), 1);
    let amend = [
        "time",
        "amend",
        id,
        time_id,
        "--expected-entry-content",
        content,
        "--reason",
        "Correct duration",
        "--seconds",
        "90",
        "--worked-at",
        "2026-09-01T10:00:00Z",
        "--user",
        "worker",
        "--actor",
        "corrector",
        "--request-id",
        "amend-once",
        "--json",
    ];
    let second = ok(root, &amend);
    assert_eq!(second["result"]["entry"]["supersedes"], time_id);
    let before = authority(&repo);
    let report = ok(
        root,
        &[
            "time",
            "report",
            "--issue",
            id,
            "--user",
            "worker",
            "--cycle",
            "current",
            "--from",
            "2026-09-01T00:00:00Z",
            "--to",
            "2026-09-02T00:00:00Z",
            "--json",
        ],
    );
    assert_eq!(report["result"]["total_seconds"], 90);
    assert_eq!(report["result"]["entries"].as_array().unwrap().len(), 1);
    assert_eq!(
        ok(root, &["time", "list", id, "--json"])["result"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(authority(&repo), before);
    assert_eq!(fs::read(repo.root().join(&issue.path)).unwrap(), original);
    assert_eq!(repo.show_issue(id).unwrap().source, issue.source);
    ok(
        root,
        &["issue", "update", id, "--title", "Changed later", "--json"],
    );
    assert_eq!(ok(root, &log)["receipt"], first["receipt"]);
    assert_eq!(ok(root, &amend)["receipt"], second["receipt"]);
    assert_eq!(repo.time_entries(id).unwrap().len(), 2);
    // The explicit later issue edit is the only item change in this scenario.
    let before_edit = String::from_utf8(original).unwrap();
    assert!(before_edit.contains("Time subject"));
    assert!(
        repo.show_issue(id)
            .unwrap()
            .body
            .contains("Keep item bytes")
    );
    assert!(!root.join(".agents").exists());
}

#[test]
fn time_cli_rejects_invalid_input_and_read_mutation_options_without_writes() {
    let (dir, repo, issue) = fixture();
    let root = dir.path();
    let id = issue.metadata.id.as_str();
    let before = authority(&repo);
    let mut args = log_args(id);
    let i = args.iter().position(|arg| *arg == "60").unwrap();
    args[i] = "1.5";
    fail(root, &args, "invalid_input");
    args[i] = "60";
    let date = args
        .iter()
        .position(|arg| *arg == "2026-09-01T10:00:00Z")
        .unwrap();
    args[date] = "yesterday";
    fail(root, &args, "invalid_input");
    fail(
        root,
        &["time", "report", "--stage", "--json"],
        "invalid_input",
    );
    fail(
        root,
        &["time", "list", id, "--request-id", "read-id", "--json"],
        "invalid_input",
    );
    let mut half = log_args(id);
    half.extend(["--expected-revision", "1"]);
    fail(root, &half, "invalid_input");
    assert_eq!(authority(&repo), before);
}

#[test]
fn time_cli_requires_native_authority_for_fresh_and_legacy_sources() {
    let fresh = root();
    fail(
        fresh.path(),
        &["time", "report", "--json"],
        "not_initialized",
    );
    assert!(!fresh.path().join(".workdeck").exists());
    for custom in [false, true] {
        let dir = root();
        let path = if custom { "legacy" } else { ".agents/workdeck" };
        fs::create_dir_all(dir.path().join(path).join("issues")).unwrap();
        let raw = "key='WD-1'\ntitle='Do not mutate'\n";
        fs::write(dir.path().join(path).join("issues/WD-1.toml"), raw).unwrap();
        if custom {
            fs::create_dir(dir.path().join(".workdeck")).unwrap();
            fs::write(
                dir.path().join(".workdeck/config.toml"),
                "[paths]\ndata_dir='legacy'\n",
            )
            .unwrap();
        }
        let error = if custom {
            "not_initialized"
        } else {
            "legacy_store"
        };
        fail(dir.path(), &["time", "report", "--json"], error);
        fail(dir.path(), &log_args("WD-1"), error);
        assert_eq!(
            fs::read_to_string(dir.path().join(path).join("issues/WD-1.toml")).unwrap(),
            raw
        );
        assert!(!dir.path().join(".workdeck/config.yml").exists());
    }
}

#[test]
fn time_cli_staging_failure_retains_receipt_for_exact_request_replay() {
    let (dir, repo, issue) = fixture();
    let root = dir.path();
    let mut args = log_args(issue.metadata.id.as_str());
    args.push("--stage");
    fs::write(root.join(".git/index.lock"), "owned by another operation").unwrap();
    let error = fail(root, &args, "locked");
    assert_eq!(error["error"]["details"]["mutation_committed"], true);
    let receipt = error["error"]["details"]["receipt"].clone();
    assert_eq!(
        repo.time_entries(issue.metadata.id.as_str()).unwrap().len(),
        1
    );
    fs::remove_file(root.join(".git/index.lock")).unwrap();
    let staged = ok(root, &args);
    assert_eq!(staged["receipt"], receipt);
    assert_eq!(staged["staging"]["paths"].as_array().unwrap().len(), 2);
    assert_eq!(
        repo.time_entries(issue.metadata.id.as_str()).unwrap().len(),
        1
    );
    let before = authority(&repo);
    let output = run(
        root,
        &[
            "time",
            "amend",
            issue.metadata.id.as_str(),
            receipt["result"]["entry"]["id"].as_str().unwrap(),
            "--seconds",
            "90",
            "--worked-at",
            "2026-09-01T10:00:00Z",
            "--user",
            "worker",
            "--actor",
            "recorder",
            "--reason",
            "Fix",
            "--json",
        ],
    );
    assert!(
        !output.status.success(),
        "missing amendment hash must be rejected"
    );
    assert_eq!(authority(&repo), before);
}

#[test]
fn time_cli_catalog_describes_actual_commands_and_available_source() {
    let (dir, _repo, _issue) = fixture();
    let value = ok(dir.path(), &["capabilities", "--json"]);
    assert_eq!(value["result"]["features"]["time_entries"], true);
    let commands = value["result"]["commands"].as_array().unwrap();
    for name in ["time log", "time amend", "time list", "time report"] {
        assert!(
            commands.iter().any(|command| command["path"] == name
                && command["native_planning"]["implemented"] == true)
        );
    }
}
