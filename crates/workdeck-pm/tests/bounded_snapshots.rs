use serde_json::json;
use std::{fs, path::Path};
use workdeck_pm::{
    ErrorCode, PmError, RequestId,
    transactions::{FaultPoint, FileChange, PreparedOperation, TransactionStore},
};

fn fixture() -> (tempfile::TempDir, TransactionStore) {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("config.yml"), "schema_version: 1\n").unwrap();
    let store = TransactionStore::open(temp.path()).unwrap();
    (temp, store)
}

#[test]
fn empty_directories_count_toward_the_traversal_budget() {
    let (temp, store) = fixture();
    for n in 0..5 {
        fs::create_dir_all(temp.path().join(format!("records/{n}"))).unwrap();
    }
    let error = store
        .with_snapshot(|snapshot| snapshot.list_bounded(Path::new("records"), 4))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Unsupported);
    assert!(error.message.contains("4"));
    assert_eq!(
        store
            .with_snapshot(|s| s.list_bounded(Path::new("records"), 5))
            .unwrap(),
        Vec::<std::path::PathBuf>::new()
    );
}

#[test]
fn a_cached_listing_does_not_bypass_a_stricter_later_budget() {
    let (temp, store) = fixture();
    fs::create_dir_all(temp.path().join("records/a/b")).unwrap();
    fs::write(temp.path().join("records/a/b/item.md"), "data").unwrap();
    store
        .with_snapshot(|snapshot| {
            assert_eq!(snapshot.list_bounded(Path::new("records"), 3)?.len(), 1);
            assert_eq!(
                snapshot
                    .list_bounded(Path::new("records"), 2)
                    .unwrap_err()
                    .code,
                ErrorCode::Unsupported
            );
            Ok(())
        })
        .unwrap();
}

#[test]
fn ignored_local_state_does_not_exhaust_the_authoritative_budget() {
    let (temp, store) = fixture();
    fs::create_dir_all(temp.path().join(".local/a/b/c")).unwrap();
    assert_eq!(
        store
            .with_snapshot(|s| s.list_bounded(Path::new(""), 1))
            .unwrap(),
        vec![Path::new("config.yml")]
    );
    assert_eq!(
        store
            .with_snapshot(|s| s.list_bounded(Path::new("config.yml"), 1))
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
}

fn operation() -> PreparedOperation {
    PreparedOperation {
        changes: vec![FileChange {
            path: "records/new/nested/item.md".into(),
            expected: None,
            content: Some(b"new content\n".to_vec()),
        }],
        result: json!({"written": true}),
    }
}

#[test]
fn a_full_budget_still_allows_recovery_of_the_operations_own_new_paths() {
    for point in [
        FaultPoint::AfterJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::AfterReceipt,
    ] {
        let (temp, store) = fixture();
        fs::create_dir(temp.path().join("records")).unwrap();
        fs::write(temp.path().join("records/old.md"), "old").unwrap();
        let key = RequestId::new();
        let error = store
            .transact_with_faults(
                &key,
                "bounded-write",
                &json!({}),
                |snapshot| {
                    assert_eq!(snapshot.list_bounded(Path::new("records"), 1)?.len(), 1);
                    Ok(operation())
                },
                |visited| {
                    if point == visited {
                        Err(PmError::new(ErrorCode::Canceled, "test interruption"))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired);
        let receipts = store.recover().unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(
            fs::read(temp.path().join("records/new/nested/item.md")).unwrap(),
            b"new content\n"
        );
        assert_eq!(
            store
                .transact(&key, "bounded-write", &json!({}), |_| panic!(
                    "replay must not prepare"
                ))
                .unwrap(),
            receipts[0]
        );
    }
}

#[test]
fn recovery_refuses_external_tree_growth_and_preserves_it() {
    let (temp, store) = fixture();
    fs::create_dir(temp.path().join("records")).unwrap();
    fs::write(temp.path().join("records/old.md"), "old").unwrap();
    store
        .transact_with_faults(
            &RequestId::new(),
            "bounded-write",
            &json!({}),
            |snapshot| {
                snapshot.list_bounded(Path::new("records"), 1)?;
                Ok(operation())
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
    for n in 0..20 {
        fs::create_dir(temp.path().join(format!("records/external-{n}"))).unwrap();
    }
    assert_eq!(
        store.recover().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert!(!temp.path().join("records/new/nested/item.md").exists());
    assert!(temp.path().join("records/external-19").is_dir());
}

#[test]
fn previous_journals_without_explicit_listing_limits_remain_recoverable() {
    let (temp, store) = fixture();
    fs::create_dir(temp.path().join("records")).unwrap();
    fs::write(temp.path().join("records/old.md"), "old").unwrap();
    store
        .transact_with_faults(
            &RequestId::new(),
            "bounded-write",
            &json!({}),
            |snapshot| {
                snapshot.list_bounded(Path::new("records"), 1)?;
                Ok(operation())
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
    let path = fs::read_dir(temp.path().join(".tmp/journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut journal: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    for listing in journal["listings"].as_sequence_mut().unwrap() {
        listing
            .as_mapping_mut()
            .unwrap()
            .remove(serde_yaml_ng::Value::String("max_entries".into()));
    }
    fs::write(&path, serde_yaml_ng::to_string(&journal).unwrap()).unwrap();
    assert_eq!(store.recover().unwrap().len(), 1);
    assert_eq!(
        fs::read(temp.path().join("records/new/nested/item.md")).unwrap(),
        b"new content\n"
    );
}
