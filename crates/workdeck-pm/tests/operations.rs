use serde_json::json;
use std::{fs, path::Path};
use tempfile::TempDir;
use workdeck_pm::{
    CreateIssue, ErrorCode, OperationId, PmError, Repository, RepositoryId, RequestId,
    transactions::{FaultPoint, FileChange, PreparedOperation, TransactionStore},
};

#[test]
fn durable_history_returns_source_qualified_mutations_in_stable_order() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let first = repository
        .create_issue(&CreateIssue::new("First", ""), &RequestId::new())
        .unwrap();
    let second = repository
        .create_issue(&CreateIssue::new("Second", ""), &RequestId::new())
        .unwrap();
    let history = repository.operation_history().unwrap();
    assert_eq!(history.len(), 2);
    assert!(history.contains(&first));
    assert!(history.contains(&second));
    assert!(history[0].operation_id >= history[1].operation_id);
    assert!(
        history
            .iter()
            .all(|receipt| receipt.repository.as_ref() == Some(repository.identity()))
    );
}

#[test]
fn durable_history_rejects_foreign_aliased_and_duplicate_request_receipts() {
    for corruption in ["foreign", "aliased", "duplicate_request", "malformed"] {
        let temp = TempDir::new().unwrap();
        let repository = Repository::init(temp.path(), "WD").unwrap();
        let mut receipt = repository
            .create_issue(&CreateIssue::new("History", ""), &RequestId::new())
            .unwrap();
        let original = repository
            .root()
            .join(format!("operations/{}.yml", receipt.operation_id));
        match corruption {
            "foreign" => {
                receipt.repository = Some(RepositoryId::new());
                fs::write(&original, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
            }
            "aliased" => {
                fs::rename(&original, repository.root().join("operations/renamed.yml")).unwrap();
            }
            "duplicate_request" => {
                receipt.operation_id = OperationId::new();
                fs::write(
                    repository
                        .root()
                        .join(format!("operations/{}.yml", receipt.operation_id)),
                    serde_yaml_ng::to_string(&receipt).unwrap(),
                )
                .unwrap();
            }
            "malformed" => {
                fs::write(&original, "schema_version: broken\n").unwrap();
            }
            _ => unreachable!(),
        }
        assert_eq!(
            repository.operation_history().unwrap_err().code,
            ErrorCode::CorruptStore,
            "{corruption}"
        );
    }
}

#[test]
fn pending_inspection_is_read_only_and_explicit_recovery_is_repeatable() {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let store = TransactionStore::open(repository.root()).unwrap();
    let error = store
        .transact_with_faults(
            &RequestId::new(),
            "test.publish",
            &json!({}),
            |_| {
                Ok(PreparedOperation {
                    changes: vec![
                        FileChange {
                            path: "records/one.yml".into(),
                            expected: None,
                            content: Some(b"one\n".to_vec()),
                        },
                        FileChange {
                            path: "records/two.yml".into(),
                            expected: None,
                            content: Some(b"two\n".to_vec()),
                        },
                    ],
                    result: json!({"record":"two"}),
                })
            },
            |point| {
                if point == FaultPoint::AfterChange(0) {
                    Err(PmError::new(ErrorCode::Canceled, "test interruption"))
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert_eq!(
        Repository::open_source(repository.root()).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let source = Repository::open_for_recovery(repository.root()).unwrap();
    let pending = source.pending_operations().unwrap();
    assert_eq!(pending.len(), 1);
    assert!(pending[0].recoverable);
    assert_eq!(pending[0].applied_paths, vec![Path::new("records/one.yml")]);
    assert_eq!(
        pending[0].remaining_paths,
        vec![Path::new("records/two.yml")]
    );
    assert!(!source.root().join("records/two.yml").exists());
    assert_eq!(
        source.list_issues().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let recovered = source.recover_operations().unwrap();
    assert_eq!(recovered, vec![pending[0].receipt.clone()]);
    assert_eq!(
        fs::read(source.root().join("records/two.yml")).unwrap(),
        b"two\n"
    );
    assert!(source.pending_operations().unwrap().is_empty());
    assert!(source.recover_operations().unwrap().is_empty());
    assert!(Repository::open_source(source.root()).is_ok());
}

#[test]
fn pending_inspection_explains_conflicts_without_advancing_any_file() {
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
                        path: "records/one.yml".into(),
                        expected: None,
                        content: Some(b"planned\n".to_vec()),
                    }],
                    result: json!({}),
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
    fs::create_dir(repository.root().join("records")).unwrap();
    fs::write(repository.root().join("records/one.yml"), "external\n").unwrap();
    let source = Repository::open_for_recovery(repository.root()).unwrap();
    let pending = source.pending_operations().unwrap();
    assert!(!pending[0].recoverable);
    assert_eq!(pending[0].errors[0].code, ErrorCode::StaleSource);
    assert_eq!(
        source.recover_operations().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(
        fs::read(repository.root().join("records/one.yml")).unwrap(),
        b"external\n"
    );
}
