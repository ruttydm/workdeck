use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;
use workdeck_pm::{
    ErrorCode, PmError, Repository, RequestId,
    transactions::{FaultPoint, FileChange, PreparedOperation, TransactionStore},
};

fn fixture() -> TempDir {
    let root = TempDir::new().unwrap();
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

fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("test-config"))
        .env("WORKDECK_MCP_DISABLE", "1")
        .stdin(Stdio::null())
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
    assert_eq!(value["ok"], true);
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
    assert_eq!(value["api_version"], 1);
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        if !current.exists() {
            return;
        }
        for entry in fs::read_dir(current).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                visit(root, &entry.path(), files);
            } else {
                assert!(entry.file_type().unwrap().is_file());
                files.insert(
                    entry.path().strip_prefix(root).unwrap().into(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    visit(root, root, &mut result);
    result
}

#[test]
fn fresh_issue_and_reference_commands_require_init_without_creating_either_store() {
    for args in [
        vec!["issue", "create", "Requires native init", "--json"],
        vec!["issue", "list", "--json"],
        vec!["project", "save", "Requires native init", "--json"],
        vec!["cycle", "save", "Requires native init", "--json"],
        vec!["label", "save", "Requires native init", "--json"],
        vec!["project", "list", "--json"],
    ] {
        let root = fixture();
        failure(root.path(), &args, "not_initialized", 3);
        assert!(!root.path().join(".agents").exists());
        assert!(!root.path().join(".workdeck").exists());
    }
}

#[test]
fn fresh_legacy_import_and_agent_mutations_require_init_but_inert_reads_stay_empty() {
    let root = fixture();
    fs::write(root.path().join("input.json"), "{}").unwrap();
    for args in [
        vec!["import", "input.json", "--replace", "--json"],
        vec!["agent", "record", "Needs initialization", "--json"],
    ] {
        failure(root.path(), &args, "not_initialized", 3);
        assert!(!root.path().join(".agents").exists());
        assert!(!root.path().join(".workdeck").exists());
    }
    for args in [
        vec!["export", "--json"],
        vec!["agent", "list", "--json"],
        vec!["events", "list", "--json"],
        vec!["search", "nothing", "--json"],
    ] {
        success(root.path(), &args);
        assert!(!root.path().join(".agents").exists());
        assert!(!root.path().join(".workdeck").exists());
    }
}

#[test]
fn native_import_rejection_and_agent_writes_never_use_legacy_authority() {
    let root = fixture();
    success(root.path(), &["init", "--json"]);
    success(
        root.path(),
        &["issue", "create", "Keep native source", "--json"],
    );
    fs::write(root.path().join("input.json"), "{}").unwrap();
    let native = root.path().join(".workdeck");
    let before = snapshot(&native);
    for (args, code, exit) in [
        (
            vec!["import", "input.json", "--replace", "--json"],
            "invalid_schema",
            2,
        ),
        (vec!["import", "missing.json", "--merge", "--json"], "io", 1),
        (
            vec!["import", "input.json", "--dry-run", "--json"],
            "invalid_schema",
            2,
        ),
    ] {
        failure(root.path(), &args, code, exit);
        assert_eq!(snapshot(&native), before);
        assert!(!root.path().join(".agents").exists());
    }
    let recorded = success(
        root.path(),
        &[
            "agent",
            "record",
            "Native annotation",
            "--id",
            "source-bound",
            "--json",
        ],
    );
    assert_eq!(recorded["api_version"], 1);
    assert_eq!(
        recorded["result"]["path"],
        "imported-sessions/source-bound.toml"
    );
    assert!(native.join("imported-sessions/source-bound.toml").exists());
    assert_eq!(
        fs::read(native.join("config.yml")).unwrap(),
        before[Path::new("config.yml")]
    );
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn native_reads_export_real_planning_without_reading_empty_legacy_data() {
    let root = fixture();
    success(root.path(), &["init", "--json"]);
    success(
        root.path(),
        &["issue", "create", "A real native issue", "--json"],
    );
    let native = root.path().join(".workdeck");
    let before = snapshot(&native);
    for args in [vec!["export", "--json"], vec!["export", "--jsonl"]] {
        let output = run(root.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let exported = workdeck_pm::decode_snapshot(&output.stdout).unwrap();
        assert!(
            exported
                .files
                .iter()
                .any(|file| file.kind == workdeck_pm::SnapshotKind::Issue
                    && String::from_utf8_lossy(&file.content).contains("A real native issue"))
        );
        assert_eq!(snapshot(&native), before);
    }
    for args in [
        vec!["agent", "list", "--json"],
        vec!["events", "list", "--json"],
    ] {
        let result = success(root.path(), &args);
        assert_eq!(result["api_version"], 1);
        assert!(result["source"]["repository"].is_string());
        assert_eq!(snapshot(&native), before);
    }
}

#[test]
fn damaged_native_source_diagnostics_precede_legacy_consumers() {
    for missing_config in [false, true] {
        let root = fixture();
        success(root.path(), &["init", "--json"]);
        success(
            root.path(),
            &["issue", "create", "Keep malformed source", "--json"],
        );
        let native = root.path().join(".workdeck");
        if missing_config {
            fs::remove_file(native.join("config.yml")).unwrap();
        } else {
            fs::write(native.join("config.yml"), "schema: [unterminated").unwrap();
        }
        let before = snapshot(&native);
        for args in [
            vec!["export", "--json"],
            vec!["events", "list", "--json"],
            vec!["import", "missing.json", "--replace", "--json"],
        ] {
            failure(
                root.path(),
                &args,
                if missing_config {
                    "not_initialized"
                } else {
                    "invalid_schema"
                },
                if missing_config { 3 } else { 2 },
            );
            assert_eq!(snapshot(&native), before);
            assert!(!root.path().join(".agents").exists());
        }
    }
}

#[test]
fn pending_native_operation_blocks_legacy_reads_and_replace_without_recovery() {
    let root = fixture();
    let repository = Repository::init(root.path(), "WD").unwrap();
    TransactionStore::open(repository.root())
        .unwrap()
        .transact_with_faults(
            &RequestId::new(),
            "test.pending",
            &json!({}),
            |_| {
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: "documents/pending.md".into(),
                        expected: None,
                        content: Some(b"Pending\n".to_vec()),
                    }],
                    result: json!({}),
                })
            },
            |point| {
                if point == FaultPoint::AfterJournal {
                    Err(PmError::new(ErrorCode::Canceled, "injected interruption"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    let before = snapshot(repository.root());
    for args in [
        vec!["export", "--json"],
        vec!["agent", "list", "--json"],
        vec!["import", "missing.json", "--replace", "--json"],
    ] {
        failure(root.path(), &args, "recovery_required", 6);
        assert_eq!(snapshot(repository.root()), before);
        assert!(!repository.root().join("documents/pending.md").exists());
    }
}

#[test]
fn actual_canonical_and_explicit_custom_legacy_sources_keep_legacy_operations() {
    for custom in [false, true] {
        let root = fixture();
        let legacy = root.path().join(if custom {
            "legacy-data"
        } else {
            ".agents/workdeck"
        });
        fs::create_dir_all(legacy.join("issues")).unwrap();
        if custom {
            fs::create_dir_all(root.path().join(".workdeck")).unwrap();
            fs::write(
                root.path().join(".workdeck/config.toml"),
                "[paths]\ndata_dir='legacy-data'\n",
            )
            .unwrap();
        }
        // Legacy compatibility is now read-only. Seed historical fixtures directly,
        // then assert reads retain their original source and writes diagnose migration.
        fs::write(legacy.join("issues/WD-1.toml"), "key='WD-1'\ntitle='Retained legacy'\ncreated_at='2026-09-01T00:00:00Z'\nupdated_at='2026-09-01T00:00:00Z'\n").unwrap();
        fs::write(legacy.join("projects.toml"), "[[projects]]\nid='retained'\nname='Retained project'\ncreated_at='2026-09-01T00:00:00Z'\nupdated_at='2026-09-01T00:00:00Z'\n").unwrap();
        let before = snapshot(&legacy);
        failure(
            root.path(),
            &["issue", "create", "No write", "--json"],
            "legacy_store",
            6,
        );
        failure(
            root.path(),
            &["project", "save", "No write", "--json"],
            "legacy_store",
            6,
        );
        assert_eq!(snapshot(&legacy), before);
        let exported = success(root.path(), &["export", "--json"]);
        assert_eq!(exported["data"]["issues"].as_array().unwrap().len(), 1);
        assert_eq!(
            success(root.path(), &["issue", "list", "--json"])["data"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        if custom {
            assert!(!root.path().join(".agents").exists());
            let app_preferences = snapshot(&root.path().join(".workdeck"));
            fs::write(
                root.path().join("export.json"),
                serde_json::to_vec(&exported["data"]).unwrap(),
            )
            .unwrap();
            failure(
                root.path(),
                &["import", "export.json", "--replace", "--json"],
                "legacy_store",
                6,
            );
            assert_eq!(snapshot(&legacy), before);
            assert_eq!(snapshot(&root.path().join(".workdeck")), app_preferences);
            assert!(legacy.join("issues/WD-1.toml").is_file());
        } else {
            assert!(!root.path().join(".workdeck").exists());
        }
    }
}

#[test]
fn configured_missing_or_native_descendant_paths_cannot_create_legacy_authority() {
    for configured in ["missing-legacy", ".workdeck/legacy", "."] {
        let root = fixture();
        let native = root.path().join(".workdeck");
        fs::create_dir_all(&native).unwrap();
        fs::write(
            native.join("config.toml"),
            format!("[paths]\ndata_dir='{configured}'\n"),
        )
        .unwrap();
        if configured.ends_with("/legacy") || configured == "." {
            fs::create_dir_all(root.path().join(configured).join("issues")).unwrap();
        }
        let before = snapshot(&native);
        for args in [
            vec!["issue", "create", "No implicit source", "--json"],
            vec!["project", "save", "No implicit source", "--json"],
            vec!["import", "missing.json", "--replace", "--json"],
        ] {
            let output = run(root.path(), &args);
            assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
            let value: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["api_version"], 1);
            assert_eq!(value["ok"], false);
            assert_eq!(snapshot(&native), before);
            assert!(!root.path().join(".agents").exists());
        }
        if configured == "missing-legacy" {
            assert!(!root.path().join(configured).exists());
        }
    }
}

#[test]
fn configured_native_copy_is_not_reinterpreted_as_a_custom_legacy_source() {
    let root = fixture();
    let repository = Repository::init(root.path(), "WD").unwrap();
    // An explicit source can live elsewhere, but the old CLI store must never
    // read or replace its authoritative YAML through a legacy data_dir override.
    let copied = root.path().join("native-copy");
    fs::rename(repository.root(), &copied).unwrap();
    let native = root.path().join(".workdeck");
    fs::create_dir_all(&native).unwrap();
    fs::write(
        native.join("config.toml"),
        "[paths]\ndata_dir='native-copy'\n",
    )
    .unwrap();
    let before = snapshot(&copied);
    for args in [
        vec!["issue", "list", "--json"],
        vec!["export", "--json"],
        vec!["import", "missing.json", "--replace", "--json"],
    ] {
        failure(root.path(), &args, "unsupported", 1);
        assert_eq!(snapshot(&copied), before);
    }
    assert!(!root.path().join(".agents").exists());
}
