use assert_cmd::prelude::*;
use serde_json::Value;
use std::{fs, path::Path, process::Command};
use workdeck_pm::{CreateIssue, ErrorCode, PmError, Repository, RequestId, restore::RestoreFault};

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

#[test]
fn restore_preview_then_apply_preserves_identity_original_receipts_and_app_preferences() {
    let source = root();
    let repository = Repository::init(source.path(), "WD").unwrap();
    let input = CreateIssue::new("Preserved through restoration", "");
    let request = RequestId::new();
    let original = repository.create_issue(&input, &request).unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let destination = root();
    fs::create_dir(destination.path().join(".workdeck")).unwrap();
    let preferences = b"# local preference\neditor = 'vim'\n";
    fs::write(
        destination.path().join(".workdeck/config.toml"),
        preferences,
    )
    .unwrap();
    fs::write(
        destination.path().join("snapshot.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let preview = ok(
        destination.path(),
        &[
            "import",
            "snapshot.json",
            "--restore",
            "--dry-run",
            "--json",
        ],
    );
    assert_eq!(preview["result"]["allowed"], true);
    assert!(!destination.path().join(".workdeck/config.yml").exists());
    let args = [
        "import",
        "snapshot.json",
        "--restore",
        "--expected-plan",
        preview["result"]["fingerprint"].as_str().unwrap(),
        "--request-id",
        "restore-once",
        "--json",
    ];
    let first = ok(destination.path(), &args);
    assert_eq!(ok(destination.path(), &args)["receipt"], first["receipt"]);
    let restored = Repository::discover(destination.path()).unwrap();
    assert_eq!(restored.identity(), repository.identity());
    assert_eq!(restored.create_issue(&input, &request).unwrap(), original);
    for file in &snapshot.files {
        assert_eq!(
            fs::read(restored.root().join(&file.path)).unwrap(),
            file.content
        );
    }
    assert_eq!(
        fs::read(restored.root().join("config.toml")).unwrap(),
        preferences
    );
    assert!(!restored.root().join("restore.yml").exists());
    assert!(
        ok(destination.path(), &["doctor", "--json"])["result"]["valid"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn interrupted_restore_blocks_ordinary_reads_and_resumes_without_original_input_file() {
    let source = root();
    let repository = Repository::init(source.path(), "WD").unwrap();
    repository
        .create_issue(&CreateIssue::new("Recover me", ""), &RequestId::new())
        .unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let destination = root();
    let request: RequestId = "restore-interrupted".parse().unwrap();
    let error = workdeck_pm::restore::restore_snapshot_with_faults(
        &destination.path().join(".workdeck"),
        &snapshot,
        None,
        &request,
        |point| {
            if point == RestoreFault::AfterBarrier {
                Err(PmError::new(ErrorCode::Canceled, "test interruption"))
            } else {
                Ok(())
            }
        },
    )
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    let read = run(destination.path(), &["issue", "list", "--json"]);
    assert!(!read.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&read.stdout).unwrap()["error"]["code"],
        "recovery_required"
    );
    let resumed = ok(
        destination.path(),
        &[
            "import",
            "--restore",
            "--resume",
            "--request-id",
            "restore-interrupted",
            "--json",
        ],
    );
    assert_eq!(
        resumed["source"]["repository"],
        serde_json::to_value(repository.identity()).unwrap()
    );
    assert_eq!(
        Repository::discover(destination.path())
            .unwrap()
            .list_issues()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn restore_review_detects_changed_destination_preferences_before_authority_writes() {
    let source = root();
    let repository = Repository::init(source.path(), "WD").unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let destination = root();
    fs::write(
        destination.path().join("snapshot.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let preview = ok(
        destination.path(),
        &[
            "import",
            "snapshot.json",
            "--restore",
            "--dry-run",
            "--json",
        ],
    );
    assert!(!destination.path().join(".workdeck").exists());
    fs::create_dir(destination.path().join(".workdeck")).unwrap();
    fs::write(
        destination.path().join(".workdeck/config.toml"),
        "editor = 'nvim'\n",
    )
    .unwrap();
    let result = run(
        destination.path(),
        &[
            "import",
            "snapshot.json",
            "--restore",
            "--expected-plan",
            preview["result"]["fingerprint"].as_str().unwrap(),
            "--json",
        ],
    );
    assert!(!result.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["error"]["code"],
        "stale_source"
    );
    assert!(!destination.path().join(".workdeck/config.yml").exists());
}

#[test]
fn restoration_stages_original_receipts_and_preserves_unrelated_index_entries() {
    let source = root();
    let repository = Repository::init(source.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Stage restored authority", ""),
            &RequestId::new(),
        )
        .unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let destination = root();
    fs::write(destination.path().join("unrelated.txt"), "already staged\n").unwrap();
    assert!(
        Command::new("git")
            .args(["add", "unrelated.txt"])
            .current_dir(destination.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(destination.path().join("unrelated.txt"), "later unstaged\n").unwrap();
    fs::write(
        destination.path().join("snapshot.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let result = ok(
        destination.path(),
        &[
            "import",
            "snapshot.json",
            "--restore",
            "--stage",
            "--request-id",
            "restore-stage",
            "--json",
        ],
    );
    let staged = result["staging"]["paths"].as_array().unwrap();
    for file in &snapshot.files {
        assert!(
            staged
                .iter()
                .any(|path| path.as_str() == Some(&format!(".workdeck/{}", file.path.display()))),
            "missing {}",
            file.path.display()
        );
    }
    let index = Command::new("git")
        .args(["show", ":unrelated.txt"])
        .current_dir(destination.path())
        .output()
        .unwrap();
    assert!(index.status.success());
    assert_eq!(index.stdout, b"already staged\n");
    assert_eq!(
        fs::read(destination.path().join("unrelated.txt")).unwrap(),
        b"later unstaged\n"
    );
    assert!(!staged.iter().any(
        |path| path.as_str() == Some("snapshot.json") || path.as_str() == Some("unrelated.txt")
    ));
    assert_eq!(
        ok(
            destination.path(),
            &[
                "import",
                "snapshot.json",
                "--restore",
                "--stage",
                "--request-id",
                "restore-stage",
                "--json"
            ]
        )["receipt"],
        result["receipt"]
    );
}

#[test]
fn staging_a_replayed_restore_rejects_changed_original_receipt_bytes() {
    let source = root();
    let repository = Repository::init(source.path(), "WD").unwrap();
    let original = repository
        .create_issue(
            &CreateIssue::new("Keep receipt source", ""),
            &RequestId::new(),
        )
        .unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let destination = root();
    fs::write(
        destination.path().join("snapshot.json"),
        serde_json::to_vec(&snapshot).unwrap(),
    )
    .unwrap();
    let first = ok(
        destination.path(),
        &[
            "import",
            "snapshot.json",
            "--restore",
            "--request-id",
            "restore-stale",
            "--json",
        ],
    );
    let path = destination.path().join(format!(
        ".workdeck/operations/{}.yml",
        original.operation_id
    ));
    let mut changed = original;
    changed.result = serde_json::json!({"direct_editor_changed_this":true});
    // JSON is valid YAML; preserve the receipt shape while changing its result.
    fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    let result = run(
        destination.path(),
        &[
            "import",
            "snapshot.json",
            "--restore",
            "--stage",
            "--request-id",
            "restore-stale",
            "--json",
        ],
    );
    assert!(!result.status.success());
    let error: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(error["error"]["code"], "stale_source");
    assert_eq!(error["error"]["details"]["mutation_committed"], true);
    assert_eq!(error["error"]["details"]["receipt"], first["receipt"]);
    assert!(!destination.path().join(".git/index").exists());
    assert_eq!(
        fs::read(&path).unwrap(),
        serde_json::to_vec(&changed).unwrap()
    );
}
