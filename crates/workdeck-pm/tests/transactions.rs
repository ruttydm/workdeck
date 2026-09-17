use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Barrier, mpsc};
use std::time::Duration;
use tempfile::TempDir;
use workdeck_pm::transactions::{FaultPoint, FileChange, PreparedOperation, TransactionStore};
use workdeck_pm::{ContentHash, ErrorCode, PmError, RequestId};

fn fixture() -> (TempDir, TransactionStore) {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("config.yml"), "schema_version: 1\n").unwrap();
    let store = TransactionStore::open(temp.path()).unwrap();
    (temp, store)
}

fn put(path: &str, before: Option<&[u8]>, after: Option<&[u8]>) -> FileChange {
    FileChange {
        path: path.into(),
        expected: before.map(ContentHash::of),
        content: after.map(<[u8]>::to_vec),
    }
}

fn prepared(changes: Vec<FileChange>) -> PreparedOperation {
    PreparedOperation {
        changes,
        result: json!({"accepted":true}),
    }
}

fn request(name: &str) -> RequestId {
    name.parse().unwrap()
}

#[test]
fn local_configuration_never_enters_authoritative_snapshots_or_mutations() {
    let (temp, store) = fixture();
    for name in [
        "settings.local.yml",
        "config.local.yml",
        "config.local.toml",
    ] {
        fs::write(temp.path().join(name), "local-only").unwrap();
        let error = store
            .with_snapshot(|snapshot| snapshot.read(Path::new(name)))
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafePath, "{name}");
        let error = store
            .transact(&RequestId::new(), "edit", &json!({}), |_| {
                Ok(prepared(vec![put(
                    name,
                    Some(b"local-only"),
                    Some(b"wrong"),
                )]))
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafePath, "{name}");
    }
    let paths = store
        .with_snapshot(|snapshot| snapshot.list(Path::new("")))
        .unwrap();
    assert_eq!(paths, vec![PathBuf::from("config.yml")]);
}

#[test]
fn transactions_cannot_delete_the_configuration_needed_to_reopen_for_recovery() {
    let (temp, store) = fixture();
    for name in ["config.yml", "CONFIG.YML"] {
        let result = store.transact_with_faults(
            &RequestId::new(),
            "delete",
            &json!({}),
            |_| {
                Ok(prepared(vec![
                    put(name, Some(b"schema_version: 1\n"), None),
                    put("issues/WD-1/item.md", None, Some(b"other file")),
                ]))
            },
            |_| panic!("configuration deletion must be rejected before journal publication"),
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::InvalidInput);
        assert_eq!(
            fs::read(temp.path().join("config.yml")).unwrap(),
            b"schema_version: 1\n"
        );
        assert!(!temp.path().join("issues/WD-1/item.md").exists());
        assert!(
            TransactionStore::open(temp.path())
                .unwrap()
                .recover()
                .unwrap()
                .is_empty()
        );
    }
}

fn interrupt() -> PmError {
    PmError::new(ErrorCode::Canceled, "injected interruption")
}

#[test]
fn replay_returns_original_receipt_and_rejects_different_inputs() {
    let (temp, store) = fixture();
    let key = request("replay");
    let first = store
        .transact(&key, "create", &json!({"z":2,"a":{"b":1,"a":0}}), |_| {
            Ok(prepared(vec![put(
                "issues/WD-1/item.md",
                None,
                Some(b"hello"),
            )]))
        })
        .unwrap();
    let reordered: serde_json::Value =
        serde_json::from_str(r#"{"a":{"a":0,"b":1},"z":2}"#).unwrap();
    let replay = store
        .transact(&key, "create", &reordered, |_| {
            panic!("replay must not prepare")
        })
        .unwrap();
    assert_eq!(first, replay);
    assert_eq!(first.changed[0].before, None);
    assert_eq!(first.changed[0].after, Some(ContentHash::of(b"hello")));
    assert!(
        temp.path()
            .join(format!("operations/{}.yml", first.operation_id))
            .is_file()
    );
    let different = store
        .transact(&key, "create", &json!({"a":1}), |_| panic!())
        .unwrap_err();
    assert_eq!(different.code, ErrorCode::IdempotencyConflict);
    let operation = store
        .transact(&key, "delete", &reordered, |_| panic!())
        .unwrap_err();
    assert_eq!(operation.code, ErrorCode::IdempotencyConflict);
}

#[test]
fn direct_edit_without_revision_increment_is_stale() {
    let (temp, store) = fixture();
    let before = b"revision: 1\ntitle: old\n";
    let edited = b"revision: 1\ntitle: edited directly\n";
    fs::write(temp.path().join("item.md"), before).unwrap();
    fs::write(temp.path().join("item.md"), edited).unwrap();
    let error = store
        .transact(&request("stale"), "update", &json!({}), |_| {
            Ok(prepared(vec![put(
                "item.md",
                Some(before),
                Some(b"revision: 2"),
            )]))
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(fs::read(temp.path().join("item.md")).unwrap(), edited);
    assert!(store.recover().unwrap().is_empty());
}

#[test]
fn recovery_finishes_every_published_fault_boundary_and_replays() {
    for point in [
        FaultPoint::AfterJournal,
        FaultPoint::BeforeChange(0),
        FaultPoint::AfterChange(0),
        FaultPoint::BeforeChange(1),
        FaultPoint::AfterChange(1),
        FaultPoint::BeforeReceipt,
        FaultPoint::AfterReceipt,
    ] {
        let (temp, store) = fixture();
        fs::write(temp.path().join("old.md"), b"old").unwrap();
        let key = request("fault");
        let error = store
            .transact_with_faults(
                &key,
                "batch",
                &json!({}),
                |_| {
                    Ok(prepared(vec![
                        put("new/item.md", None, Some(b"new")),
                        put("old.md", Some(b"old"), None),
                    ]))
                },
                |visited| {
                    if point == visited {
                        Err(interrupt())
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired, "{point:?}");
        let read = store
            .with_snapshot(|snapshot| snapshot.read(Path::new("old.md")))
            .unwrap_err();
        assert_eq!(read.code, ErrorCode::RecoveryRequired, "{point:?}");
        let receipts = store.recover().unwrap();
        assert_eq!(receipts.len(), 1, "{point:?}");
        assert_eq!(fs::read(temp.path().join("new/item.md")).unwrap(), b"new");
        assert!(!temp.path().join("old.md").exists());
        assert!(store.recover().unwrap().is_empty());
        let replay = store
            .transact(&key, "batch", &json!({}), |_| panic!())
            .unwrap();
        assert_eq!(receipts[0], replay);
    }
}

#[test]
fn fault_before_journal_leaves_no_authority_changes_or_recovery() {
    let (temp, store) = fixture();
    let error = store
        .transact_with_faults(
            &request("early"),
            "create",
            &json!({}),
            |_| Ok(prepared(vec![put("item.md", None, Some(b"new"))])),
            |point| {
                if point == FaultPoint::BeforeJournal {
                    Err(interrupt())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Canceled);
    assert!(!temp.path().join("item.md").exists());
    assert!(store.recover().unwrap().is_empty());
}

#[test]
fn recovery_preserves_external_modification_and_does_not_advance_other_files() {
    let (temp, store) = fixture();
    fs::write(temp.path().join("a.md"), b"old-a").unwrap();
    fs::write(temp.path().join("b.md"), b"old-b").unwrap();
    store
        .transact_with_faults(
            &request("conflict"),
            "batch",
            &json!({}),
            |_| {
                Ok(prepared(vec![
                    put("a.md", Some(b"old-a"), Some(b"new-a")),
                    put("b.md", Some(b"old-b"), Some(b"new-b")),
                ]))
            },
            |point| {
                if point == FaultPoint::AfterJournal {
                    Err(interrupt())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    fs::write(temp.path().join("b.md"), b"external").unwrap();
    assert_eq!(
        store.recover().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(fs::read(temp.path().join("a.md")).unwrap(), b"old-a");
    assert_eq!(fs::read(temp.path().join("b.md")).unwrap(), b"external");
}

#[test]
fn rechecks_precondition_immediately_before_each_write() {
    let (temp, store) = fixture();
    fs::write(temp.path().join("b.md"), b"old").unwrap();
    let error = store
        .transact_with_faults(
            &request("late-edit"),
            "batch",
            &json!({}),
            |_| {
                Ok(prepared(vec![
                    put("a.md", None, Some(b"new")),
                    put("b.md", Some(b"old"), Some(b"replacement")),
                ]))
            },
            |point| {
                if point == FaultPoint::AfterChange(0) {
                    fs::write(temp.path().join("b.md"), b"external").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert_eq!(fs::read(temp.path().join("b.md")).unwrap(), b"external");
}

#[test]
fn competing_threads_have_one_compare_and_set_winner() {
    let (temp, _) = fixture();
    let gate = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|id| {
            let root = temp.path().to_owned();
            let gate = gate.clone();
            std::thread::spawn(move || {
                let store = TransactionStore::open(&root).unwrap();
                gate.wait();
                store.transact(
                    &request(&format!("race-{id}")),
                    "create",
                    &json!({}),
                    |_| Ok(prepared(vec![put("item.md", None, Some(b"winner"))])),
                )
            })
        })
        .collect();
    gate.wait();
    let results: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results.into_iter().find_map(Result::err).unwrap().code,
        ErrorCode::StaleSource
    );
}

#[test]
fn snapshot_reader_cannot_observe_half_of_a_change_set() {
    let (temp, _) = fixture();
    let (paused_tx, paused_rx) = mpsc::channel();
    let (continue_tx, continue_rx) = mpsc::channel();
    let root = temp.path().to_owned();
    let writer = std::thread::spawn(move || {
        TransactionStore::open(&root)
            .unwrap()
            .transact_with_faults(
                &request("atomic-reader"),
                "batch",
                &json!({}),
                |_| {
                    Ok(prepared(vec![
                        put("a.md", None, Some(b"a")),
                        put("b.md", None, Some(b"b")),
                    ]))
                },
                |point| {
                    if point == FaultPoint::AfterChange(0) {
                        paused_tx.send(()).unwrap();
                        continue_rx.recv().unwrap();
                    }
                    Ok(())
                },
            )
            .unwrap()
    });
    paused_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let store = TransactionStore::open(temp.path())
        .unwrap()
        .with_lock_timeout(Duration::from_millis(30));
    assert_eq!(
        store
            .with_snapshot(|snapshot| snapshot.list(Path::new("")))
            .unwrap_err()
            .code,
        ErrorCode::Locked
    );
    continue_tx.send(()).unwrap();
    writer.join().unwrap();
    store
        .with_snapshot(|snapshot| {
            assert_eq!(snapshot.read(Path::new("a.md"))?, Some(b"a".to_vec()));
            assert_eq!(snapshot.read(Path::new("b.md"))?, Some(b"b".to_vec()));
            Ok(())
        })
        .unwrap();
}

#[test]
fn rejects_reserved_traversal_and_duplicate_changes_without_writing() {
    let (temp, store) = fixture();
    for path in [
        "../outside",
        "/absolute",
        ".tmp/x",
        ".index/x",
        ".local/x",
        "index/x",
        "operations/x.yml",
        ".TMP/writer.lock",
        "Operations/x.yml",
        "settings.local.yml",
        "x/../../y",
        "x\\y",
    ] {
        let error = store
            .transact(&RequestId::new(), "bad", &json!({}), |_| {
                Ok(prepared(vec![put(path, None, Some(b"bad"))]))
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafePath, "{path}");
    }
    assert_eq!(
        store
            .transact(&RequestId::new(), "bad", &json!({}), |_| {
                Ok(prepared(vec![
                    put("same", None, Some(b"one")),
                    put("same", None, Some(b"two")),
                ]))
            })
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert!(!temp.path().join("same").exists());
}

#[test]
fn case_aliases_cannot_create_a_nonrecoverable_change_set() {
    let (_temp, store) = fixture();
    let error = store
        .transact(&request("case-alias"), "bad", &json!({}), |_| {
            Ok(prepared(vec![
                put("ITEM.md", None, Some(b"one")),
                put("item.md", None, Some(b"two")),
            ]))
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(store.recover().unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn rejects_symlink_root_parent_leaf_and_listing() {
    use std::os::unix::fs::symlink;
    let (temp, store) = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("item.md"), b"outside").unwrap();
    symlink(outside.path(), temp.path().join("escape")).unwrap();
    assert_eq!(
        store
            .with_snapshot(|snapshot| snapshot.read(Path::new("escape/item.md")))
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert_eq!(
        store
            .with_snapshot(|snapshot| snapshot.list(Path::new("")))
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert_eq!(
        store
            .transact(&request("symlink"), "write", &json!({}), |_| {
                Ok(prepared(vec![put(
                    "escape/item.md",
                    Some(b"outside"),
                    Some(b"bad"),
                )]))
            })
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    symlink(outside.path().join("item.md"), temp.path().join("leaf.md")).unwrap();
    assert_eq!(
        store
            .with_snapshot(|snapshot| snapshot.read(Path::new("leaf.md")))
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    let link = outside.path().join("root-link");
    symlink(temp.path(), &link).unwrap();
    assert_eq!(
        TransactionStore::open(&link).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    assert_eq!(
        fs::read(outside.path().join("item.md")).unwrap(),
        b"outside"
    );
}

#[test]
fn missing_root_and_missing_config_are_not_initialized_by_open() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("missing");
    assert_eq!(
        TransactionStore::open(&missing).unwrap_err().code,
        ErrorCode::NotInitialized
    );
    assert!(!missing.exists());
    assert_eq!(
        TransactionStore::open(temp.path()).unwrap_err().code,
        ErrorCode::NotInitialized
    );
    assert!(fs::read_dir(temp.path()).unwrap().next().is_none());
}

#[test]
fn snapshot_lists_sorted_authority_files_and_never_writes_them() {
    let (temp, store) = fixture();
    fs::create_dir_all(temp.path().join("issues/z")).unwrap();
    fs::create_dir_all(temp.path().join(".index")).unwrap();
    fs::write(temp.path().join("issues/z/item.md"), b"z").unwrap();
    fs::write(temp.path().join("issues/a.md"), b"a").unwrap();
    fs::write(temp.path().join(".index/cache"), b"local").unwrap();
    let before = fs::read(temp.path().join("config.yml")).unwrap();
    store
        .with_snapshot(|snapshot| {
            assert_eq!(
                snapshot.list(Path::new("issues"))?,
                [
                    PathBuf::from("issues/a.md"),
                    PathBuf::from("issues/z/item.md")
                ]
            );
            assert_eq!(
                snapshot.list(Path::new(""))?,
                [
                    PathBuf::from("config.yml"),
                    PathBuf::from("issues/a.md"),
                    PathBuf::from("issues/z/item.md")
                ]
            );
            assert_eq!(snapshot.read(Path::new("missing.md"))?, None);
            assert!(snapshot.list(Path::new("missing")).unwrap().is_empty());
            assert_eq!(
                snapshot.read(Path::new("issues")).unwrap_err().code,
                ErrorCode::UnsafePath
            );
            Ok(())
        })
        .unwrap();
    assert_eq!(fs::read(temp.path().join("config.yml")).unwrap(), before);
    assert!(!temp.path().join("operations").exists());
}

#[test]
fn corrupt_receipt_cannot_be_silently_skipped_for_a_new_request() {
    let (temp, store) = fixture();
    let receipt = store
        .transact(&request("original"), "create", &json!({}), |_| {
            Ok(prepared(vec![put("item.md", None, Some(b"content"))]))
        })
        .unwrap();
    fs::write(
        temp.path()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        b"not: a receipt",
    )
    .unwrap();
    let error = store
        .transact(&request("another"), "create", &json!({}), |_| {
            panic!("corrupt store must fail before prepare")
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::CorruptStore);
    assert_eq!(fs::read(temp.path().join("item.md")).unwrap(), b"content");
}

#[test]
fn corrupted_journal_and_receipt_conflict_preserve_unapplied_files() {
    let (temp, store) = fixture();
    store
        .transact_with_faults(
            &request("corrupt"),
            "batch",
            &json!({}),
            |_| {
                Ok(prepared(vec![
                    put("a.md", None, Some(b"a")),
                    put("b.md", None, Some(b"b")),
                ]))
            },
            |point| {
                if point == FaultPoint::AfterJournal {
                    Err(interrupt())
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
    let original = fs::read(&path).unwrap();
    fs::write(&path, b"corrupted: journal").unwrap();
    assert_eq!(
        store.recover().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert!(!temp.path().join("a.md").exists());
    fs::write(&path, &original).unwrap();
    let journal: serde_yaml_ng::Value = serde_yaml_ng::from_slice(&original).unwrap();
    let operation = journal["receipt"]["operation_id"].as_str().unwrap();
    fs::create_dir_all(temp.path().join("operations")).unwrap();
    fs::write(
        temp.path().join(format!("operations/{operation}.yml")),
        b"unrelated receipt content",
    )
    .unwrap();
    assert_eq!(
        store.recover().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert!(!temp.path().join("a.md").exists());
    assert!(!temp.path().join("b.md").exists());
}

#[test]
fn an_editor_changing_an_already_replaced_file_prevents_success_receipt() {
    let (temp, store) = fixture();
    let error = store
        .transact_with_faults(
            &request("late-old-file"),
            "batch",
            &json!({}),
            |_| {
                Ok(prepared(vec![
                    put("a.md", None, Some(b"a")),
                    put("b.md", None, Some(b"b")),
                ]))
            },
            |point| {
                if point == FaultPoint::AfterChange(1) {
                    fs::write(temp.path().join("a.md"), b"external").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert_eq!(fs::read(temp.path().join("a.md")).unwrap(), b"external");
    assert!(!temp.path().join("operations").exists());
}

#[test]
fn stale_unwritten_dependency_aborts_before_journal_publication() {
    let (temp, store) = fixture();
    let error = store
        .transact(&request("read-set"), "create", &json!({}), |snapshot| {
            assert!(snapshot.read(Path::new("config.yml"))?.is_some());
            fs::write(
                temp.path().join("config.yml"),
                b"schema_version: 1\npolicy: changed\n",
            )
            .unwrap();
            Ok(prepared(vec![put("item.md", None, Some(b"new issue"))]))
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert!(!temp.path().join("item.md").exists());
    assert!(store.recover().unwrap().is_empty());
}

#[test]
fn cached_reads_are_repeatable_and_external_edits_fail_read_callback() {
    let (temp, store) = fixture();
    let error = store
        .with_snapshot(|snapshot| {
            let first = snapshot.read(Path::new("config.yml"))?;
            fs::write(temp.path().join("config.yml"), b"changed: externally\n").unwrap();
            assert_eq!(snapshot.read(Path::new("config.yml"))?, first);
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
}

#[test]
fn directory_membership_is_repeatable_and_invalidates_prepared_operation() {
    let (temp, store) = fixture();
    fs::create_dir_all(temp.path().join("issues")).unwrap();
    let error = store
        .transact(&request("membership"), "create", &json!({}), |snapshot| {
            assert!(snapshot.list(Path::new("issues"))?.is_empty());
            fs::write(temp.path().join("issues/external.md"), b"external").unwrap();
            assert!(snapshot.list(Path::new("issues"))?.is_empty());
            Ok(prepared(vec![put(
                "issues/item.md",
                None,
                Some(b"new issue"),
            )]))
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert!(!temp.path().join("issues/item.md").exists());
    assert_eq!(
        fs::read(temp.path().join("issues/external.md")).unwrap(),
        b"external"
    );
    assert!(store.recover().unwrap().is_empty());
}

#[test]
fn recovery_rechecks_unwritten_dependencies_recorded_in_the_journal() {
    let (temp, store) = fixture();
    let config = fs::read(temp.path().join("config.yml")).unwrap();
    store
        .transact_with_faults(
            &request("journal-input"),
            "create",
            &json!({}),
            |snapshot| {
                snapshot.read(Path::new("config.yml"))?;
                Ok(prepared(vec![put("item.md", None, Some(b"new"))]))
            },
            |point| {
                if point == FaultPoint::AfterJournal {
                    Err(interrupt())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    fs::write(
        temp.path().join("config.yml"),
        b"policy: changed externally\n",
    )
    .unwrap();
    assert_eq!(
        store.recover().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert!(!temp.path().join("item.md").exists());
    fs::write(temp.path().join("config.yml"), config).unwrap();
    assert_eq!(store.recover().unwrap().len(), 1);
    assert_eq!(fs::read(temp.path().join("item.md")).unwrap(), b"new");
}

#[test]
fn recovery_membership_allows_own_changes_but_preserves_external_additions() {
    let (temp, store) = fixture();
    fs::create_dir_all(temp.path().join("issues")).unwrap();
    fs::write(temp.path().join("issues/old.md"), b"old").unwrap();
    store
        .transact_with_faults(
            &request("journal-membership"),
            "batch",
            &json!({}),
            |snapshot| {
                snapshot.list(Path::new(""))?;
                snapshot.read(Path::new("issues/old.md"))?;
                Ok(prepared(vec![
                    put("issues/old.md", Some(b"old"), None),
                    put("issues/new.md", None, Some(b"new")),
                ]))
            },
            |point| {
                if point == FaultPoint::AfterChange(0) {
                    Err(interrupt())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    fs::write(temp.path().join("issues/external.md"), b"external").unwrap();
    assert_eq!(
        store.recover().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert!(!temp.path().join("issues/new.md").exists());
    fs::remove_file(temp.path().join("issues/external.md")).unwrap();
    assert_eq!(store.recover().unwrap().len(), 1);
    assert_eq!(fs::read(temp.path().join("issues/new.md")).unwrap(), b"new");
}

#[test]
fn bounded_reads_reject_large_files_and_cached_values_over_the_callers_limit() {
    let (temp, store) = fixture();
    fs::write(temp.path().join("data.bin"), b"12345").unwrap();
    store
        .with_snapshot(|snapshot| {
            assert_eq!(
                snapshot.read_bounded(Path::new("data.bin"), 5)?,
                Some(b"12345".to_vec())
            );
            assert_eq!(
                snapshot
                    .read_bounded(Path::new("data.bin"), 4)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidSchema
            );
            Ok(())
        })
        .unwrap();
    let giant = fs::File::create(temp.path().join("giant.bin")).unwrap();
    giant
        .set_len(workdeck_pm::transactions::MAX_TRANSACTION_FILE_BYTES as u64 + 1)
        .unwrap();
    assert_eq!(
        store
            .with_snapshot(|snapshot| snapshot.read(Path::new("giant.bin")))
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
}

#[test]
fn content_codec_is_compact_byte_exact_and_reads_legacy_arrays() {
    let original = put("binary", None, Some(&vec![255; 1024]));
    let encoded = serde_yaml_ng::to_string(&original).unwrap();
    assert!(encoded.contains("base64:"));
    assert!(encoded.len() < 1600);
    assert_eq!(
        serde_yaml_ng::from_str::<FileChange>(&encoded).unwrap(),
        original
    );
    let legacy = "path: binary\nexpected: null\ncontent: [0, 255, 128]\n";
    assert_eq!(
        serde_yaml_ng::from_str::<FileChange>(legacy).unwrap(),
        put("binary", None, Some(&[0, 255, 128]))
    );
    assert!(
        serde_yaml_ng::from_str::<FileChange>("path: binary\nexpected: null\ncontent: bogus\n")
            .is_err()
    );
}

#[test]
fn total_journal_size_is_rejected_before_publication() {
    let (temp, store) = fixture();
    // 48 MiB becomes 64 MiB in base64, before journal/receipt metadata.
    let bytes = vec![1; 48 * 1024 * 1024];
    let error = store
        .transact(&request("oversized-journal"), "create", &json!({}), |_| {
            Ok(prepared(vec![FileChange {
                path: "large.bin".into(),
                expected: None,
                content: Some(bytes),
            }]))
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert!(!temp.path().join("large.bin").exists());
    assert!(store.recover().unwrap().is_empty());
}

#[test]
fn twenty_mib_binary_journal_recovers_after_metadata_write() {
    let (temp, store) = fixture();
    let bytes = vec![0xff; 20 * 1024 * 1024];
    let expected_hash = ContentHash::of(&bytes);
    store
        .transact_with_faults(
            &request("binary-recovery"),
            "attach",
            &json!({"size":bytes.len()}),
            |_| {
                Ok(prepared(vec![
                    put("metadata.yml", None, Some(b"synthetic metadata")),
                    FileChange {
                        path: "content.bin".into(),
                        expected: None,
                        content: Some(bytes),
                    },
                ]))
            },
            |point| {
                if point == FaultPoint::AfterChange(0) {
                    Err(interrupt())
                } else {
                    Ok(())
                }
            },
        )
        .unwrap_err();
    let journal = fs::read_dir(temp.path().join(".tmp/journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    assert!(
        journal.metadata().unwrap().len()
            < workdeck_pm::transactions::MAX_TRANSACTION_FILE_BYTES as u64
    );
    assert!(!temp.path().join("content.bin").exists());
    assert_eq!(store.recover().unwrap().len(), 1);
    assert_eq!(
        ContentHash::of(&fs::read(temp.path().join("content.bin")).unwrap()),
        expected_hash
    );
}

// The integration-test executable doubles as an isolated subprocess fixture.
// Parent invokes only this exact test and uses synthetic temp paths.
#[test]
fn transaction_subprocess_worker() {
    let Some(root) = std::env::var_os("WORKDECK_PM_TRANSACTION_TEST_ROOT") else {
        return;
    };
    let mode = std::env::var("WORKDECK_PM_TRANSACTION_TEST_MODE").unwrap();
    let root = PathBuf::from(root);
    let store = TransactionStore::open(&root).unwrap();
    if mode == "crash" {
        store
            .transact_with_faults(
                &request("crashed"),
                "batch",
                &json!({}),
                |_| {
                    Ok(prepared(vec![
                        put("a.md", None, Some(b"a")),
                        put("b.md", None, Some(b"b")),
                    ]))
                },
                |point| {
                    if point == FaultPoint::AfterChange(0) {
                        std::process::exit(81);
                    }
                    Ok(())
                },
            )
            .unwrap();
        panic!("fault not reached");
    } else {
        while !root.join("start").exists() {
            std::thread::sleep(Duration::from_millis(5));
        }
        let result = store.transact(&request(&mode), "create", &json!({}), |_| {
            Ok(prepared(vec![put("race.md", None, Some(mode.as_bytes()))]))
        });
        fs::write(
            root.join(format!("{mode}.result")),
            match result {
                Ok(_) => "won",
                Err(error) if error.code == ErrorCode::StaleSource => "stale",
                Err(error) => panic!("{error}"),
            },
        )
        .unwrap();
    }
}

fn child(root: &Path, mode: &str) -> std::process::Child {
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "transaction_subprocess_worker", "--nocapture"])
        .env("WORKDECK_PM_TRANSACTION_TEST_ROOT", root)
        .env("WORKDECK_PM_TRANSACTION_TEST_MODE", mode)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap()
}

#[test]
fn subprocess_race_and_process_exit_release_lock_for_recovery() {
    let (temp, _) = fixture();
    let mut left = child(temp.path(), "left");
    let mut right = child(temp.path(), "right");
    fs::write(temp.path().join("start"), b"go").unwrap();
    assert!(left.wait().unwrap().success());
    assert!(right.wait().unwrap().success());
    let mut results = [
        fs::read_to_string(temp.path().join("left.result")).unwrap(),
        fs::read_to_string(temp.path().join("right.result")).unwrap(),
    ];
    results.sort();
    assert_eq!(results, ["stale", "won"]);

    let (temp, store) = fixture();
    let status = child(temp.path(), "crash").wait().unwrap();
    assert_eq!(status.code(), Some(81));
    assert_eq!(
        store
            .with_snapshot(|snapshot| snapshot.read(Path::new("a.md")))
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(store.recover().unwrap().len(), 1);
    assert_eq!(fs::read(temp.path().join("a.md")).unwrap(), b"a");
    assert_eq!(fs::read(temp.path().join("b.md")).unwrap(), b"b");
}
