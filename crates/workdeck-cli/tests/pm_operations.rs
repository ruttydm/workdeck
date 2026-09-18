use assert_cmd::prelude::*;
use serde_json::{Value, json};
use std::{fs, process::Command};
use tempfile::TempDir;
use workdeck_pm::{
    ErrorCode, PmError, Repository, RequestId,
    transactions::{FaultPoint, FileChange, PreparedOperation, TransactionStore},
};

fn run(path: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(path)
        .env("XDG_CONFIG_HOME", path.join("test-config"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn cli_inspects_and_recovers_interrupted_operation_without_implicit_recovery() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let store = TransactionStore::open(repository.root()).unwrap();
    store
        .transact_with_faults(
            &RequestId::new(),
            "test.publish",
            &json!({}),
            |_| {
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: "documents/example.md".into(),
                        expected: None,
                        content: Some(b"Recovered\n".to_vec()),
                    }],
                    result: json!({"title":"Example"}),
                })
            },
            |point| {
                if point == FaultPoint::AfterJournal {
                    Err(PmError::new(ErrorCode::Canceled, "test interruption"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    for args in [
        vec!["operation", "pending", "--json"],
        vec!["operation", "recover", "--dry-run", "--json"],
    ] {
        let output = run(temp.path(), &args);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["result"][0]["recoverable"], true);
        assert!(
            value["source"]["repository"]
                .as_str()
                .unwrap()
                .starts_with("repo-")
        );
        assert!(!repository.root().join("documents/example.md").exists());
    }
    let output = run(temp.path(), &["operation", "recover", "--json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["result"].as_array().unwrap().len(), 1);
    assert_eq!(
        fs::read(repository.root().join("documents/example.md")).unwrap(),
        b"Recovered\n"
    );
    let value: Value =
        serde_json::from_slice(&run(temp.path(), &["operation", "recover", "--json"]).stdout)
            .unwrap();
    assert_eq!(value["result"], json!([]));
}

#[test]
fn operation_inspection_does_not_initialize_an_empty_directory() {
    let temp = TempDir::new().unwrap();
    let output = run(temp.path(), &["operation", "pending", "--json"]);
    assert_eq!(output.status.code(), Some(3));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["error"]["code"], "not_initialized");
    assert!(!temp.path().join(".workdeck").exists());
}
