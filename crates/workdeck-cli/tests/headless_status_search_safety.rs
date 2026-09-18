use assert_cmd::prelude::*;
use git2::{Repository, Signature};
use serde_json::Value;
use std::{
    fs,
    io::Read,
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn fixture() -> (TempDir, Repository) {
    let root = TempDir::new().unwrap();
    let repo = Repository::init(root.path()).unwrap();
    (root, repo)
}
fn commit(repo: &Repository) {
    let mut index = repo.index().unwrap();
    index
        .add_all(["*"], git2::IndexAddOption::DEFAULT, None)
        .unwrap();
    index.write().unwrap();
    let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
    let signature = Signature::now("Synthetic", "test@example.invalid").unwrap();
    repo.commit(Some("HEAD"), &signature, &signature, "Fixture", &tree, &[])
        .unwrap();
}
fn run(root: &Path, args: &[&str]) -> Output {
    let mut command = Command::cargo_bin("workdeck").unwrap();
    command
        .current_dir(root)
        .args(args)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).unwrap();
        bytes
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).unwrap();
        bytes
    });
    // Allow concurrent Cargo jobs and macOS's first launch after relinking;
    // an actual FIFO open still cannot outlive this subprocess deadline.
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut blocked = false;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= deadline {
            blocked = true;
            child.kill().unwrap();
            break child.wait().unwrap();
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let output = Output {
        status,
        stdout: out.join().unwrap(),
        stderr: err.join().unwrap(),
    };
    assert!(!blocked, "command blocked: {args:?}");
    output
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

fn legacy(root: &Path, directory: &str) {
    fs::create_dir_all(root.join(directory).join("issues")).unwrap();
    fs::create_dir_all(root.join(directory).join("agents")).unwrap();
    let issue = workdeck_cli::store::Issue::new("WD-1".into(), "Planning needle".into());
    fs::write(
        root.join(directory).join("issues/WD-1.toml"),
        toml::to_string(&issue).unwrap(),
    )
    .unwrap();
    fs::write(root.join(directory).join("agents/session-1.toml"),
        "id='session-1'\ntitle='Historical needle'\nagent='recorded-tool'\ncommands_run=['touch MUST_NOT_EXECUTE']\n").unwrap();
}

#[cfg(unix)]
#[test]
fn status_and_legacy_search_reject_fifo_ignore_sources_promptly() {
    let (root, _) = fixture();
    legacy(root.path(), ".agents/workdeck");
    let path = std::ffi::CString::new(root.path().join(".gitignore").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    for args in [
        vec!["status", "--json"],
        vec!["search", "needle", "--target", "files", "--json"],
    ] {
        assert!(!run(root.path(), &args).status.success());
    }
}

#[cfg(unix)]
fn helpers_remain_inert(args: &[&str]) {
    let (root, repo) = fixture();
    fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
    commit(&repo);
    legacy(root.path(), ".agents/workdeck");
    fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
    fs::write(root.path().join(".gitattributes"), "*.txt diff=unsafe\n").unwrap();
    let marker = root.path().join("helper-executed");
    let script = root.path().join("helper.sh");
    fs::write(
        &script,
        format!("#!/bin/sh\ntouch '{}'\ncat \"$1\"\n", marker.display()),
    )
    .unwrap();
    let command = format!("sh '{}'", script.display());
    repo.config()
        .unwrap()
        .set_str("diff.unsafe.textconv", &command)
        .unwrap();
    repo.config()
        .unwrap()
        .set_str("diff.external", &command)
        .unwrap();
    success(root.path(), args);
    assert!(
        !marker.exists(),
        "inspection executed a configured Git helper"
    );
    assert!(!root.path().join("MUST_NOT_EXECUTE").exists());
}

#[cfg(unix)]
#[test]
fn status_never_executes_configured_helpers() {
    helpers_remain_inert(&["status", "--json"]);
}

#[cfg(unix)]
#[test]
fn legacy_search_never_executes_configured_helpers() {
    helpers_remain_inert(&["search", "needle", "--json"]);
}

#[cfg(unix)]
#[test]
fn status_and_legacy_symbol_search_never_read_symlink_targets() {
    let (root, _) = fixture();
    legacy(root.path(), ".agents/workdeck");
    let outside = TempDir::new().unwrap();
    fs::write(
        outside.path().join("source.rs"),
        "fn outside_secret_symbol() {}\n".repeat(300),
    )
    .unwrap();
    std::os::unix::fs::symlink(
        outside.path().join("source.rs"),
        root.path().join("linked.rs"),
    )
    .unwrap();
    let status = success(root.path(), &["status", "--json"]);
    let row = status["data"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == "linked.rs")
        .unwrap();
    assert!(
        row["additions"].as_u64().unwrap() <= 1,
        "counted external target content"
    );
    let search = success(
        root.path(),
        &[
            "search",
            "outside_secret_symbol",
            "--target",
            "files",
            "--json",
        ],
    );
    assert!(search["data"].as_array().unwrap().is_empty());
}

#[test]
fn legacy_search_filters_before_limit_and_keeps_issue_session_json_shape() {
    let (root, _) = fixture();
    legacy(root.path(), ".agents/workdeck");
    for index in 0..150 {
        fs::write(
            root.path().join(format!("needle-{index:03}.txt")),
            "visible\n",
        )
        .unwrap();
    }
    for (group, kind, field, id) in [
        ("issues", "issue", "key", "WD-1"),
        ("agents", "agent", "id", "session-1"),
    ] {
        let value = success(
            root.path(),
            &["search", "needle", "--target", group, "--json"],
        );
        assert_eq!(value["kind"], "search_results");
        let rows = value["data"].as_array().unwrap();
        assert!(
            rows.iter()
                .any(|row| row["target"]["kind"] == kind && row["target"][field] == id),
            "missing {group}: {value}"
        );
        assert!(rows.iter().all(|row| row["target"]["kind"] == kind));
    }
    assert!(!root.path().join(".workdeck").exists());
}

#[test]
fn status_shape_counts_and_explicit_custom_legacy_source_are_preserved() {
    let (root, repo) = fixture();
    fs::write(root.path().join("tracked.txt"), "one\n").unwrap();
    commit(&repo);
    fs::write(root.path().join("tracked.txt"), "one\nstaged\n").unwrap();
    let mut index = repo.index().unwrap();
    index.add_path(Path::new("tracked.txt")).unwrap();
    index.write().unwrap();
    fs::write(root.path().join("tracked.txt"), "one\nstaged\nunstaged\n").unwrap();
    let status = success(root.path(), &["status", "--json"]);
    assert_eq!(status["kind"], "status");
    assert!(status["data"]["groups"].is_array());
    let row = status["data"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["path"] == "tracked.txt")
        .unwrap();
    assert_eq!(row["stage"], "staged+unstaged");
    assert_eq!(row["additions"], 2);
    legacy(root.path(), ".legacy");
    fs::create_dir(root.path().join(".workdeck")).unwrap();
    fs::write(
        root.path().join(".workdeck/config.toml"),
        "[paths]\ndata_dir='.legacy'\n",
    )
    .unwrap();
    let value = success(
        root.path(),
        &["search", "needle", "--target", "issues", "--json"],
    );
    assert!(
        value["data"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["target"]["key"] == "WD-1")
    );
    assert!(!root.path().join(".workdeck/config.yml").exists());
    assert!(!root.path().join(".agents").exists());
}
