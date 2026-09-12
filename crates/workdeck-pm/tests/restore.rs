use std::{collections::BTreeMap, fs};
use workdeck_pm::{
    restore::{RestoreFault, restore_snapshot_with_faults},
    *,
};
fn source() -> (
    tempfile::TempDir,
    Repository,
    CreateIssue,
    RequestId,
    transactions::MutationReceipt,
) {
    let root = tempfile::tempdir().unwrap();
    let repository = Repository::init(root.path(), "WD").unwrap();
    let input = CreateIssue {
        title: "Retain original issue history".into(),
        body: "Exact body\n".into(),
        fields: BTreeMap::new(),
    };
    let request: RequestId = "original-create".parse().unwrap();
    let receipt = repository.create_issue(&input, &request).unwrap();
    (root, repository, input, request, receipt)
}
#[test]
fn fresh_restore_preserves_repository_original_receipts_and_exact_request_replay() {
    let (_source, repository, issue, create_request, original) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join(".workdeck");
    let preview = preview_snapshot_restore(&destination, &snapshot).unwrap();
    assert!(preview.allowed, "{:?}", preview.blockers);
    assert!(!destination.exists(), "dry-run initialized destination");
    let request: RequestId = "restore-original".parse().unwrap();
    let result = restore_snapshot(
        &destination,
        &snapshot,
        Some(&preview.fingerprint),
        &request,
    )
    .unwrap();
    assert!(!destination.join("restore.yml").exists());
    let restored = Repository::open_source(&destination).unwrap();
    assert_eq!(restored.identity(), repository.identity());
    for file in &snapshot.files {
        assert_eq!(
            fs::read(destination.join(&file.path)).unwrap(),
            file.content,
            "{}",
            file.path.display()
        );
    }
    assert_eq!(
        restored.create_issue(&issue, &create_request).unwrap(),
        original
    );
    assert!(
        result
            .restored
            .iter()
            .any(|change| change.path.starts_with("operations"))
    );
    assert!(
        result
            .receipt
            .changed
            .iter()
            .all(|change| !change.path.starts_with("operations"))
    );
    restored
        .create_issue(
            &CreateIssue {
                title: "Later work".into(),
                ..issue
            },
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(
        restore_snapshot(
            &destination,
            &snapshot,
            Some(&preview.fingerprint),
            &request
        )
        .unwrap(),
        result
    );
}

#[test]
fn existing_directory_at_incoming_authority_is_a_preview_blocker() {
    let (_source, repository, ..) = source();
    let mut label = CreatePlanning::new("Preserved label");
    label.id = Some("example".into());
    repository
        .create_planning(PlanningKind::Label, &label, &RequestId::new())
        .unwrap();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    fs::create_dir_all(root.join("labels.yml")).unwrap();
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(
        !plan.allowed,
        "directory at planned file was omitted from restoration preflight"
    );
    assert!(plan.blockers.iter().any(|error| {
        error.code == ErrorCode::UnsafePath
            && error
                .path
                .as_ref()
                .is_some_and(|path| path.ends_with("labels.yml"))
    }));
    assert_eq!(
        restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert!(!root.join("config.yml").exists());
    assert!(!root.join("restore.yml").exists());
    assert!(!root.join(".gitignore").exists());
    assert!(!root.join(".tmp/restorations").exists());
    assert!(root.join("labels.yml").is_dir());
}
#[test]
fn changed_destination_and_conflicting_authority_are_never_overwritten() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join(".workdeck");
    let preview = preview_snapshot_restore(&destination, &snapshot).unwrap();
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("config.toml"), "theme = 'changed'\n").unwrap();
    assert_eq!(
        restore_snapshot(
            &destination,
            &snapshot,
            Some(&preview.fingerprint),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert!(!destination.join("config.yml").exists());
    let different = Config::new("OTHER").unwrap();
    let bytes = serde_yaml_ng::to_string(&different).unwrap();
    fs::write(destination.join("config.yml"), &bytes).unwrap();
    let blocked = preview_snapshot_restore(&destination, &snapshot).unwrap();
    assert!(!blocked.allowed);
    assert!(restore_snapshot(&destination, &snapshot, None, &RequestId::new()).is_err());
    assert_eq!(
        fs::read_to_string(destination.join("config.yml")).unwrap(),
        bytes
    );
}
#[test]
fn interrupted_restore_blocks_normal_admission_and_recovers_without_configuration() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    for point in [
        RestoreFault::AfterBarrier,
        RestoreFault::AfterChange(0),
        RestoreFault::BeforeReceipt,
        RestoreFault::AfterReceipt,
    ] {
        let target = tempfile::tempdir().unwrap();
        let destination = target.path().join(".workdeck");
        let request = RequestId::new();
        let error =
            restore_snapshot_with_faults(&destination, &snapshot, None, &request, |event| {
                if event == point {
                    Err(PmError::new(ErrorCode::Io, "injected restore crash"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(
            error.code,
            ErrorCode::RecoveryRequired,
            "{point:?}: {error}"
        );
        assert!(destination.join("restore.yml").exists());
        assert_eq!(
            Repository::open_source(&destination).unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
        assert_eq!(
            Repository::init(target.path(), "WD").unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
        let receipt = resume_snapshot_restore(&destination, &request).unwrap();
        assert_eq!(
            restore_snapshot(&destination, &snapshot, None, &request).unwrap(),
            receipt
        );
        assert!(
            Repository::open_source(&destination)
                .unwrap()
                .doctor()
                .unwrap()
                .valid
        );
    }
}

#[test]
fn missing_configuration_and_original_receipts_can_be_repaired_without_rewriting_retained_files() {
    let (_source, repository, input, original_request, original_receipt) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    fs::create_dir(&root).unwrap();
    for file in snapshot.files.iter().filter(|file| {
        file.kind != SnapshotKind::Operation && file.kind != SnapshotKind::Configuration
    }) {
        let path = root.join(&file.path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, &file.content).unwrap();
    }
    assert!(Repository::open_source(&root).is_err());
    let preview = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(preview.allowed, "{:?}", preview.blockers);
    let restored = restore_snapshot(
        &root,
        &snapshot,
        Some(&preview.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    assert!(
        restored
            .restored
            .iter()
            .any(|change| change.path == std::path::Path::new("config.yml"))
    );
    let repository = Repository::open_source(&root).unwrap();
    assert_eq!(
        repository.create_issue(&input, &original_request).unwrap(),
        original_receipt
    );
}

#[test]
fn exact_input_retry_and_incoming_request_collisions_have_no_new_authority_effects() {
    let (_source, repository, _, original_request, _) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    assert_eq!(
        restore_snapshot(&root, &snapshot, None, &original_request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert!(!root.join("restore.yml").exists());
    assert!(!root.join("config.yml").exists());
    let request = RequestId::new();
    let original = restore_snapshot(&root, &snapshot, None, &request).unwrap();
    let mut changed = snapshot.clone();
    changed.files[0].content.push(b' ');
    assert_eq!(
        restore_snapshot(&root, &changed, None, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let other = Config::new("OTHER").unwrap();
    fs::write(
        root.join("config.yml"),
        serde_yaml_ng::to_string(&other).unwrap(),
    )
    .unwrap();
    assert_eq!(
        restore_snapshot(&root, &snapshot, None, &request)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        resume_snapshot_restore(&root, &request).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert!(
        root.join(format!("operations/{}.yml", original.receipt.operation_id))
            .exists()
    );
}

#[test]
fn returned_restore_proof_rejects_forged_original_receipt_paths_and_hashes_without_live_reads() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let restored = restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap();
    fs::remove_dir_all(&root).unwrap();
    assert_eq!(
        validate_restore_receipt(&restored.receipt).unwrap(),
        restored
    );
    for location in [
        "/restored/0/after",
        "/source/files/0/content",
        "/plan/destination",
    ] {
        let mut forged = restored.receipt.clone();
        *forged.result.pointer_mut(location).unwrap() =
            serde_json::json!(ContentHash::of(b"forged"));
        assert!(validate_restore_receipt(&forged).is_err(), "{location}");
    }
    let mut forged = restored.receipt.clone();
    forged.result["restored"][0]["path"] = serde_json::json!("../../outside");
    assert!(validate_restore_receipt(&forged).is_err());
}

#[test]
fn conflicting_edits_during_partial_restore_remain_visible_and_recovery_does_not_overwrite_them() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let request = RequestId::new();
    let error = restore_snapshot_with_faults(&root, &snapshot, None, &request, |point| {
        if point == RestoreFault::AfterChange(1) {
            fs::write(root.join("config.yml"), "external config edit").unwrap();
        }
        Ok(())
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert!(resume_snapshot_restore(&root, &request).is_err());
    assert_eq!(
        fs::read_to_string(root.join("config.yml")).unwrap(),
        "external config edit"
    );
    let config = snapshot
        .files
        .iter()
        .find(|file| file.kind == SnapshotKind::Configuration)
        .unwrap();
    fs::write(root.join("config.yml"), &config.content).unwrap();
    assert!(resume_snapshot_restore(&root, &request).is_ok());
}

#[test]
fn completed_migration_authority_restores_with_retained_legacy_siblings() {
    let original = tempfile::tempdir().unwrap();
    let legacy = original.path().join(".agents/workdeck");
    fs::create_dir_all(legacy.join("issues")).unwrap();
    fs::write(legacy.join("issues/WD-1.toml"),"key='WD-1'\ntitle='Historical work'\nstatus='done'\ncreated_at='2026-09-01T00:00:00Z'\nupdated_at='2026-09-02T00:00:00Z'\n").unwrap();
    let options = migration::PreviewOptions {
        config: Config::new("WD").unwrap(),
        imported_at: "2026-09-09T00:00:00Z".parse().unwrap(),
    };
    let plan = migration::preview(&legacy, &original.path().join(".workdeck"), &options).unwrap();
    assert!(plan.complete, "{:?}", plan.blockers);
    migration::apply(&plan, &RequestId::new()).unwrap();
    let source = Repository::discover(original.path()).unwrap();
    let snapshot = source.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    fs::create_dir_all(target.path().join(".agents/workdeck/issues")).unwrap();
    let root = target.path().join(".workdeck");
    assert!(preview_snapshot_restore(&root, &snapshot).unwrap().allowed);
    restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap();
    assert_eq!(
        Repository::discover(target.path()).unwrap().identity(),
        source.identity()
    );
    for file in snapshot
        .files
        .iter()
        .filter(|file| matches!(file.kind, SnapshotKind::Migration | SnapshotKind::Operation))
    {
        assert_eq!(fs::read(root.join(&file.path)).unwrap(), file.content);
    }
    let (_other, unmigrated, ..) = self::source();
    let blocked = tempfile::tempdir().unwrap();
    fs::create_dir_all(blocked.path().join(".agents/workdeck/issues")).unwrap();
    let root = blocked.path().join(".workdeck");
    let preview = preview_snapshot_restore(&root, &unmigrated.export_snapshot().unwrap()).unwrap();
    assert!(!preview.allowed);
    assert!(
        preview
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::LegacyStore)
    );
    assert!(!root.exists());
}

#[test]
fn ordinary_transactions_cannot_publish_restore_barriers_or_original_receipts() {
    let (_source, repository, ..) = source();
    let store = transactions::TransactionStore::open(repository.root()).unwrap();
    for path in [
        "restore.yml",
        "operations/OP-00000000000000000000000000.yml",
    ] {
        assert_eq!(
            store
                .transact(
                    &RequestId::new(),
                    "snapshot.restore",
                    &serde_json::json!({}),
                    |_| Ok(transactions::PreparedOperation {
                        changes: vec![transactions::FileChange {
                            path: path.into(),
                            expected: None,
                            content: Some(b"forged".to_vec())
                        }],
                        result: serde_json::json!({}),
                    })
                )
                .unwrap_err()
                .code,
            ErrorCode::UnsafePath
        );
    }
    assert!(!repository.root().join("restore.yml").exists());
}

#[test]
fn restore_child_process_crash_helper() {
    let Some(path) = std::env::var_os("WORKDECK_RESTORE_CRASH_FIXTURE") else {
        return;
    };
    let directory = std::path::PathBuf::from(path);
    let input: NativeSnapshot =
        serde_json::from_slice(&fs::read(directory.join("input.json")).unwrap()).unwrap();
    let request: RequestId = "process-crash".parse().unwrap();
    let _ = restore_snapshot_with_faults(
        &directory.join(".workdeck"),
        &input,
        None,
        &request,
        |point| {
            if point == RestoreFault::AfterBarrier {
                std::process::exit(37);
            }
            Ok(())
        },
    );
    panic!("restore child did not reach durable barrier");
}
#[test]
fn actual_process_exit_releases_writer_and_preserves_config_independent_recovery() {
    let (_source, repository, ..) = source();
    let input = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(target.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(
        target.path().join("input.json"),
        serde_json::to_vec(&input).unwrap(),
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "restore_child_process_crash_helper"])
        .env("WORKDECK_RESTORE_CRASH_FIXTURE", target.path())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(37));
    let root = target.path().join(".workdeck");
    assert!(!root.join("config.yml").exists());
    let ignored = std::process::Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".workdeck/.tmp",
        ])
        .current_dir(target.path())
        .output()
        .unwrap();
    assert!(ignored.status.success());
    assert!(
        ignored.stdout.is_empty(),
        "crashed restore exposed local state: {}",
        String::from_utf8_lossy(&ignored.stdout)
    );
    assert_eq!(
        Repository::open_source(&root).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    let result = resume_snapshot_restore(&root, &"process-crash".parse().unwrap()).unwrap();
    assert_eq!(result.plan.repository, input.repository);
    assert!(
        Repository::open_source(&root)
            .unwrap()
            .doctor()
            .unwrap()
            .valid
    );
}

#[test]
fn existing_ignore_rules_require_explicit_local_state_exclusions_without_silent_rewrite() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(target.path())
            .status()
            .unwrap()
            .success()
    );
    let root = target.path().join(".workdeck");
    fs::create_dir(&root).unwrap();
    let preferences = "theme = 'one-dark'\n";
    fs::write(root.join("config.toml"), preferences).unwrap();
    let original = "# Keep custom rules\n/cache/\n";
    fs::write(root.join(".gitignore"), original).unwrap();
    let preview = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(!preview.allowed, "restore accepted exposed local staging");
    assert!(
        preview
            .blockers
            .iter()
            .any(|error| error.path.as_deref() == Some(".gitignore"))
    );
    assert!(restore_snapshot(&root, &snapshot, None, &RequestId::new()).is_err());
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        original
    );
    assert!(!root.join(".tmp/restorations").exists());
    assert!(!root.join("restore.yml").exists());
    assert!(!root.join("config.yml").exists());
    assert_eq!(
        fs::read_to_string(root.join("config.toml")).unwrap(),
        preferences
    );
    let status = std::process::Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".workdeck/.tmp",
        ])
        .current_dir(target.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(
        status.stdout.is_empty(),
        "blocked restore exposed local state: {}",
        String::from_utf8_lossy(&status.stdout)
    );
}

#[test]
fn conflicting_local_recovery_ignore_rules_are_preserved_before_publication() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    fs::create_dir_all(root.join(".tmp")).unwrap();
    let original = b"# custom local policy\n!restorations/\n";
    fs::write(root.join(".tmp/.gitignore"), original).unwrap();
    let error = restore_snapshot(&root, &snapshot, None, &RequestId::new()).unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(fs::read(root.join(".tmp/.gitignore")).unwrap(), original);
    assert!(!root.join(".tmp/writer.lock").exists());
    assert!(!root.join(".tmp/restorations").exists());
    assert!(!root.join("restore.yml").exists());
    assert!(!root.join("config.yml").exists());
}

#[test]
fn restore_receipt_capacity_is_a_preview_blocker_before_any_authority_publication() {
    let (_source, repository, ..) = source();
    fs::create_dir(repository.root().join("imported-handoffs")).unwrap();
    for index in 0..4093 {
        fs::write(
            repository
                .root()
                .join(format!("imported-handoffs/{index}.txt")),
            "x",
        )
        .unwrap();
    }
    let snapshot = repository.export_snapshot().unwrap();
    assert_eq!(snapshot.files.len(), 4096);
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let preview = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(!preview.allowed);
    assert!(
        preview
            .blockers
            .iter()
            .any(|error| error.code == ErrorCode::Unsupported)
    );
    assert!(!root.exists());
    assert!(restore_snapshot(&root, &snapshot, None, &RequestId::new()).is_err());
    assert!(!root.join("restore.yml").exists());
    assert!(!root.join("config.yml").exists());
}

#[test]
fn concurrent_identical_restore_requests_publish_one_receipt() {
    let (_source, repository, ..) = source();
    let snapshot = std::sync::Arc::new(repository.export_snapshot().unwrap());
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let jobs = (0..2)
        .map(|_| {
            let root = root.clone();
            let snapshot = snapshot.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                restore_snapshot(&root, &snapshot, None, &"one-restore".parse().unwrap()).unwrap()
            })
        })
        .collect::<Vec<_>>();
    let results = jobs
        .into_iter()
        .map(|job| job.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results[0], results[1]);
    assert_eq!(fs::read_dir(root.join("operations")).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn static_fifo_barriers_and_symlink_local_roots_are_rejected_before_open_or_publication() {
    use std::os::unix::fs::symlink;
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    fs::create_dir(&root).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(root.join("restore.yml"))
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        preview_snapshot_restore(&root, &snapshot).unwrap_err().code,
        ErrorCode::UnsafePath
    );
    fs::remove_file(root.join("restore.yml")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), root.join(".tmp")).unwrap();
    assert_eq!(
        restore_snapshot(&root, &snapshot, None, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::UnsafePath
    );
    assert!(!root.join("config.yml").exists());
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn before_barrier_failure_is_retryable_and_changed_staging_remains_a_recovery_error() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let request = RequestId::new();
    let error = restore_snapshot_with_faults(&root, &snapshot, None, &request, |point| {
        if point == RestoreFault::BeforeBarrier {
            Err(PmError::new(ErrorCode::Io, "before barrier"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::Io);
    assert!(!root.join("restore.yml").exists());
    assert!(!root.join("config.yml").exists());
    let error = restore_snapshot_with_faults(&root, &snapshot, None, &request, |point| {
        if point == RestoreFault::AfterBarrier {
            Err(PmError::new(ErrorCode::Io, "after barrier"))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    let marker_path = root.join("restore.yml");
    let mut marker: serde_json::Value =
        serde_yaml_ng::from_slice(&fs::read(&marker_path).unwrap()).unwrap();
    let stage = root.join(marker["stage"].as_str().unwrap());
    let mut staged: serde_json::Value = serde_json::from_slice(&fs::read(&stage).unwrap()).unwrap();
    staged["snapshot"]["files"][0]["content"] = serde_json::json!("YmFk");
    let bytes = serde_json::to_vec(&staged).unwrap();
    fs::write(stage, &bytes).unwrap();
    marker["stage_hash"] = serde_json::json!(ContentHash::of(&bytes));
    fs::write(marker_path, serde_yaml_ng::to_string(&marker).unwrap()).unwrap();
    assert_eq!(
        resume_snapshot_restore(&root, &request).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert!(!root.join("config.yml").exists());
}

#[test]
fn resume_does_not_claim_an_imported_restore_receipt_belongs_to_this_destination() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let first = tempfile::tempdir().unwrap();
    let first_root = first.path().join(".workdeck");
    let request = RequestId::new();
    restore_snapshot(&first_root, &snapshot, None, &request).unwrap();
    let exported = Repository::open_source(&first_root)
        .unwrap()
        .export_snapshot()
        .unwrap();
    let next = tempfile::tempdir().unwrap();
    let next_root = next.path().join(".workdeck");
    restore_snapshot(&next_root, &exported, None, &RequestId::new()).unwrap();
    assert_eq!(
        resume_snapshot_restore(&next_root, &request)
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
}

#[test]
fn later_negated_ignore_rules_cannot_expose_retained_restore_bundles_to_git() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let required = fs::read_to_string(repository.root().join(".gitignore")).unwrap();
    let target = tempfile::tempdir().unwrap();
    assert!(
        std::process::Command::new("git")
            .arg("init")
            .arg("-q")
            .current_dir(target.path())
            .status()
            .unwrap()
            .success()
    );
    let root = target.path().join(".workdeck");
    fs::create_dir(&root).unwrap();
    let custom = format!("# Retain custom policy\n{required}!/.tmp/\n");
    fs::write(root.join(".gitignore"), &custom).unwrap();
    let blocked = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(
        !blocked.allowed,
        "later negation exposed local restoration stage"
    );
    let approved = format!("{custom}{required}");
    fs::write(root.join(".gitignore"), &approved).unwrap();
    let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new()).unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".gitignore")).unwrap(),
        approved
    );
    let status = std::process::Command::new("git")
        .args([
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--",
            ".workdeck/.tmp",
        ])
        .current_dir(target.path())
        .output()
        .unwrap();
    assert!(status.status.success());
    assert!(
        status.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&status.stdout)
    );
}

#[test]
fn static_planned_leaf_and_parent_shape_conflicts_never_publish_authority() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let item = snapshot
        .files
        .iter()
        .find(|file| file.kind == SnapshotKind::Issue)
        .unwrap()
        .path
        .clone();
    let operation = snapshot
        .files
        .iter()
        .find(|file| file.kind == SnapshotKind::Operation)
        .unwrap()
        .path
        .clone();
    for (path, directory) in [
        (std::path::PathBuf::from("config.yml"), true),
        (std::path::PathBuf::from(".gitignore"), true),
        (std::path::PathBuf::from("restore.yml"), true),
        (operation, true),
        (item.clone(), true),
        (item.parent().unwrap().to_owned(), false),
        (std::path::PathBuf::from("operations"), false),
    ] {
        let target = tempfile::tempdir().unwrap();
        let root = target.path().join(".workdeck");
        let obstacle = root.join(&path);
        fs::create_dir_all(obstacle.parent().unwrap()).unwrap();
        if directory {
            fs::create_dir(&obstacle).unwrap();
        } else {
            fs::write(&obstacle, b"keep conflicting parent").unwrap();
        }
        let preview = preview_snapshot_restore(&root, &snapshot);
        assert!(
            !preview.is_ok_and(|plan| plan.allowed),
            "{path:?} was allowed"
        );
        assert!(
            restore_snapshot(&root, &snapshot, None, &RequestId::new()).is_err(),
            "{path:?}"
        );
        assert!(!root.join("config.yml").is_file(), "{path:?}");
        assert!(!root.join("restore.yml").is_file(), "{path:?}");
        assert!(!root.join(".gitignore").is_file(), "{path:?}");
        assert!(!root.join(".tmp/restorations").exists(), "{path:?}");
        if directory {
            assert!(obstacle.is_dir());
        } else {
            assert_eq!(fs::read(obstacle).unwrap(), b"keep conflicting parent");
        }
    }
}

#[test]
fn harmless_empty_directories_do_not_change_reviewed_fingerprints_or_historical_receipts() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let original = preview_snapshot_restore(&root, &snapshot).unwrap();
    fs::create_dir_all(root.join("unused/keep/nested")).unwrap();
    fs::create_dir_all(root.join("issues/unused-empty-issue")).unwrap();
    let current = preview_snapshot_restore(&root, &snapshot).unwrap();
    assert_eq!(original, current);
    let request = RequestId::new();
    let returned =
        restore_snapshot(&root, &snapshot, Some(&original.fingerprint), &request).unwrap();
    assert!(root.join("unused/keep/nested").is_dir());
    assert!(root.join("issues/unused-empty-issue").is_dir());
    assert_eq!(
        validate_restore_receipt(&returned.receipt).unwrap(),
        returned
    );
    fs::create_dir_all(root.join("more/unrelated/empty")).unwrap();
    assert_eq!(
        restore_snapshot(&root, &snapshot, Some(&original.fingerprint), &request).unwrap(),
        returned
    );
}

#[test]
fn directories_added_after_review_or_at_pre_barrier_checkpoint_remain_unpublished() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    let incoming_operation = snapshot
        .files
        .iter()
        .find(|file| file.kind == SnapshotKind::Operation)
        .unwrap()
        .path
        .clone();
    for during_apply in [false, true] {
        for path in [
            std::path::PathBuf::from("config.yml"),
            std::path::PathBuf::from(".gitignore"),
            incoming_operation.clone(),
        ] {
            let target = tempfile::tempdir().unwrap();
            let root = target.path().join(".workdeck");
            let plan = preview_snapshot_restore(&root, &snapshot).unwrap();
            if !during_apply {
                fs::create_dir_all(root.join(&path)).unwrap();
            }
            let error = restore_snapshot_with_faults(
                &root,
                &snapshot,
                Some(&plan.fingerprint),
                &RequestId::new(),
                |point| {
                    if during_apply && point == RestoreFault::BeforeBarrier {
                        fs::create_dir_all(root.join(&path)).unwrap();
                    }
                    Ok(())
                },
            )
            .unwrap_err();
            assert!(
                matches!(error.code, ErrorCode::UnsafePath | ErrorCode::StaleSource),
                "{during_apply}/{path:?}: {error:?}"
            );
            assert!(!root.join("restore.yml").exists());
            assert!(!root.join("config.yml").is_file());
            assert!(root.join(path).is_dir());
        }
    }
}

#[test]
fn allocated_receipt_and_barrier_directory_races_fail_before_authority_publication() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    for receipt in [false, true] {
        let target = tempfile::tempdir().unwrap();
        let root = target.path().join(".workdeck");
        let error =
            restore_snapshot_with_faults(&root, &snapshot, None, &RequestId::new(), |point| {
                if point == RestoreFault::BeforeBarrier {
                    let path = if receipt {
                        let staged = fs::read_dir(root.join(".tmp/restorations"))
                            .unwrap()
                            .next()
                            .unwrap()
                            .unwrap();
                        root.join("operations")
                            .join(format!("{}.yml", staged.file_name().to_string_lossy()))
                    } else {
                        root.join("restore.yml")
                    };
                    fs::create_dir_all(path).unwrap();
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafePath);
        assert!(!root.join("restore.yml").is_file());
        assert!(!root.join("config.yml").exists());
        assert!(!root.join(".gitignore").exists());
    }
}

#[test]
fn planned_directory_conflicts_after_barrier_require_recovery_without_overwrite() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    for partial in [false, true] {
        let target = tempfile::tempdir().unwrap();
        let root = target.path().join(".workdeck");
        let request = RequestId::new();
        let obstacle = snapshot
            .files
            .iter()
            .find(|file| file.kind == SnapshotKind::Issue)
            .unwrap()
            .path
            .clone();
        let error = restore_snapshot_with_faults(&root, &snapshot, None, &request, |point| {
            if point
                == if partial {
                    RestoreFault::AfterChange(0)
                } else {
                    RestoreFault::AfterBarrier
                }
            {
                fs::create_dir_all(root.join(&obstacle)).unwrap();
                if partial {
                    return Err(PmError::new(ErrorCode::Io, "interrupted directory race"));
                }
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired);
        assert_eq!(
            Repository::open_source(&root).unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
        assert_eq!(
            resume_snapshot_restore(&root, &request).unwrap_err().code,
            ErrorCode::RecoveryRequired
        );
        assert!(root.join(&obstacle).is_dir());
        fs::remove_dir(root.join(&obstacle)).unwrap();
        let restored = resume_snapshot_restore(&root, &request).unwrap();
        assert_eq!(restored.plan.repository, snapshot.repository);
        assert!(root.join(obstacle).is_file());
        assert!(
            Repository::open_source(&root)
                .unwrap()
                .doctor()
                .unwrap()
                .valid
        );
    }
}

#[test]
fn changed_or_directory_staging_source_is_detected_before_durable_barrier() {
    let (_source, repository, ..) = source();
    let snapshot = repository.export_snapshot().unwrap();
    for directory in [false, true] {
        let target = tempfile::tempdir().unwrap();
        let root = target.path().join(".workdeck");
        let error =
            restore_snapshot_with_faults(&root, &snapshot, None, &RequestId::new(), |point| {
                if point == RestoreFault::BeforeBarrier {
                    let stage = fs::read_dir(root.join(".tmp/restorations"))
                        .unwrap()
                        .next()
                        .unwrap()
                        .unwrap()
                        .path()
                        .join("bundle.json");
                    if directory {
                        fs::remove_file(&stage).unwrap();
                        fs::create_dir(stage).unwrap();
                    } else {
                        fs::write(stage, b"changed input").unwrap();
                    }
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(
            error.code,
            if directory {
                ErrorCode::UnsafePath
            } else {
                ErrorCode::StaleSource
            }
        );
        assert!(!root.join("restore.yml").exists());
        assert!(!root.join("config.yml").exists());
        assert!(!root.join(".gitignore").exists());
    }
}
