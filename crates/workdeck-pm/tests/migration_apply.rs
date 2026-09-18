use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;
use workdeck_pm::{
    Config, ErrorCode, IssueRecord, PmError, Repository, RequestId, UpdateIssue,
    migration::{MigrationFault, PreviewOptions, apply, apply_with_faults, preview, resume},
    transactions::{FaultPoint, TransactionStore},
};

fn setup() -> (TempDir, workdeck_pm::migration::MigrationPreview) {
    let temp = TempDir::new().unwrap();
    fs::create_dir(temp.path().join(".git")).unwrap();
    let source = temp.path().join(".agents/workdeck");
    fs::create_dir_all(source.join("issues")).unwrap();
    let input = "key = \"WD-1\"\ntitle = \"Legacy issue\"\ncreated_at = \"2026-09-01T00:00:00Z\"\nupdated_at = \"2026-09-02T00:00:00Z\"\n";
    fs::write(source.join("issues/WD-1.toml"), input).unwrap();
    let context = PreviewOptions {
        config: Config::new("WD").unwrap(),
        imported_at: "2026-09-09T00:00:00Z".parse().unwrap(),
    };
    let plan = preview(&source, &temp.path().join(".workdeck"), &context).unwrap();
    assert!(plan.complete, "{:?}", plan.blockers);
    (temp, plan)
}
fn request() -> RequestId {
    "migration-1".parse().unwrap()
}
fn interruption() -> PmError {
    PmError::new(ErrorCode::Canceled, "injected migration interruption")
}

#[test]
fn apply_cuts_over_preserves_legacy_and_replays_after_later_native_edits() {
    let (temp, plan) = setup();
    fs::create_dir_all(&plan.destination_root).unwrap();
    fs::write(plan.destination_root.join("unrelated.txt"), "keep").unwrap();
    let source = fs::read(plan.source_root.join("issues/WD-1.toml")).unwrap();
    let receipt = apply(&plan, &request()).unwrap();
    let repository = Repository::discover(temp.path()).unwrap();
    assert_eq!(repository.identity(), &plan.options.config.repository);
    let issue = repository.show_issue("WD-1").unwrap();
    let changed = repository
        .update_issue(
            "WD-1",
            &issue.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::new(),
                body: Some("Later native edit".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    let changed: IssueRecord = serde_json::from_value(changed.result).unwrap();
    assert_eq!(apply(&plan, &request()).unwrap(), receipt);
    assert_eq!(resume(&plan.destination_root, &request()).unwrap(), receipt);
    assert_eq!(repository.show_issue("WD-1").unwrap(), changed);
    assert_eq!(
        fs::read(plan.source_root.join("issues/WD-1.toml")).unwrap(),
        source
    );
    assert_eq!(
        fs::read_to_string(plan.destination_root.join("unrelated.txt")).unwrap(),
        "keep"
    );
    assert!(
        receipt
            .changed
            .iter()
            .any(|change| change.path == Path::new("issues/WD-1/item.md"))
    );
}

#[test]
fn interrupted_bootstrap_batches_and_cutover_resume_without_exposing_partial_sources() {
    for point in [
        MigrationFault::AfterBarrier,
        MigrationFault::AfterBootstrap,
        MigrationFault::Batch {
            index: 0,
            point: FaultPoint::AfterChange(0),
        },
        MigrationFault::AfterBatch(0),
        MigrationFault::Manifest(FaultPoint::AfterReceipt),
        MigrationFault::Cutover(FaultPoint::AfterChange(0)),
    ] {
        let (temp, plan) = setup();
        let error = apply_with_faults(&plan, &request(), |at| {
            if at == point {
                Err(interruption())
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
        assert_eq!(
            Repository::discover(temp.path()).unwrap_err().code,
            ErrorCode::RecoveryRequired,
            "{point:?}"
        );
        assert_eq!(
            Repository::open_source(&plan.destination_root)
                .unwrap_err()
                .code,
            ErrorCode::RecoveryRequired,
            "{point:?}"
        );
        if let Ok(store) = TransactionStore::open(&plan.destination_root) {
            assert_eq!(
                store.with_snapshot(|_| Ok(())).unwrap_err().code,
                ErrorCode::RecoveryRequired
            );
        }
        let receipt = resume(&plan.destination_root, &request()).unwrap();
        assert_eq!(apply(&plan, &request()).unwrap(), receipt);
        assert_eq!(
            Repository::discover(temp.path())
                .unwrap()
                .list_issues()
                .unwrap()
                .len(),
            1
        );
    }
}

#[test]
fn source_and_destination_conflicts_block_resume_without_overwriting_external_edits() {
    for source_conflict in [true, false] {
        let (_temp, plan) = setup();
        apply_with_faults(&plan, &request(), |point| {
            if point == MigrationFault::AfterBootstrap {
                Err(interruption())
            } else {
                Ok(())
            }
        })
        .unwrap_err();
        let target = if source_conflict {
            plan.source_root.join("issues/WD-1.toml")
        } else {
            plan.destination_root.join("issues/WD-1/item.md")
        };
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, "external edit must survive").unwrap();
        assert!(resume(&plan.destination_root, &request()).is_err());
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "external edit must survive"
        );
        assert_eq!(
            Repository::open_source(&plan.destination_root)
                .unwrap_err()
                .code,
            ErrorCode::RecoveryRequired
        );
    }
}

#[test]
fn destination_edits_during_final_publication_never_become_completed_migrations() {
    for at in [
        MigrationFault::Manifest(FaultPoint::BeforeJournal),
        MigrationFault::Manifest(FaultPoint::BeforeReceipt),
        MigrationFault::Cutover(FaultPoint::BeforeJournal),
        MigrationFault::Cutover(FaultPoint::BeforeReceipt),
    ] {
        let (_temp, plan) = setup();
        let target = plan.destination_root.join("issues/WD-1/item.md");
        let error = apply_with_faults(&plan, &request(), |point| {
            if point == at {
                fs::write(&target, "external edit during publication").unwrap();
            }
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired, "{at:?}");
        assert_eq!(
            fs::read_to_string(&target).unwrap(),
            "external edit during publication"
        );
        assert!(resume(&plan.destination_root, &request()).is_err());
        assert_eq!(
            Repository::open_source(&plan.destination_root)
                .unwrap_err()
                .code,
            ErrorCode::RecoveryRequired
        );
    }
}

#[test]
fn completed_admission_requires_the_durable_cutover_receipt() {
    let (_temp, plan) = setup();
    apply(&plan, &request()).unwrap();
    for entry in fs::read_dir(plan.destination_root.join("operations")).unwrap() {
        let path = entry.unwrap().path();
        let receipt: workdeck_pm::transactions::MutationReceipt =
            serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
        if receipt.operation == "migration.cutover" {
            fs::remove_file(path).unwrap();
        }
    }
    assert_eq!(
        Repository::open_source(&plan.destination_root)
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(
        resume(&plan.destination_root, &request()).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
}

#[test]
fn pending_cutover_journal_must_match_the_recorded_migration_intent() {
    let (_temp, plan) = setup();
    apply_with_faults(&plan, &request(), |point| {
        if point == MigrationFault::Cutover(FaultPoint::AfterJournal) {
            Err(interruption())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    let path = fs::read_dir(plan.destination_root.join(".tmp/journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut value: serde_yaml_ng::Value =
        serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
    value["receipt"]["input_hash"] =
        serde_yaml_ng::to_value(workdeck_pm::ContentHash::of(b"different intent")).unwrap();
    fs::write(&path, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
    let marker = fs::read(plan.destination_root.join("migration.yml")).unwrap();
    assert_eq!(
        resume(&plan.destination_root, &request()).unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    assert_eq!(
        fs::read(plan.destination_root.join("migration.yml")).unwrap(),
        marker
    );
    assert!(path.exists());
}

#[test]
fn interruption_before_barrier_publishes_no_authority_and_can_be_retried() {
    let (_temp, plan) = setup();
    assert_eq!(
        apply_with_faults(&plan, &request(), |point| {
            if point == MigrationFault::BeforeBarrier {
                Err(interruption())
            } else {
                Ok(())
            }
        })
        .unwrap_err()
        .code,
        ErrorCode::Canceled
    );
    assert!(!plan.destination_root.join("migration.yml").exists());
    assert!(!plan.destination_root.join("config.yml").exists());
    assert!(!plan.destination_root.join("migrations").exists());
    let receipt = apply(&plan, &request()).unwrap();
    assert_eq!(resume(&plan.destination_root, &request()).unwrap(), receipt);
}

#[test]
fn completed_replay_needs_no_legacy_source_and_changed_intent_conflicts() {
    let (_temp, plan) = setup();
    let receipt = apply(&plan, &request()).unwrap();
    let mut context = plan.options.clone();
    context.imported_at = "2026-09-10T00:00:00Z".parse().unwrap();
    // Another valid plan can target the same destination without carrying the
    // original after-state: this intent is rejected before live source lookup.
    let source = plan.source_root.clone();
    let second = preview(&source, &plan.destination_root, &context).unwrap();
    assert_eq!(
        apply(&second, &request()).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(
        apply(&plan, &RequestId::new()).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    fs::remove_dir_all(&source).unwrap();
    assert_eq!(apply(&plan, &request()).unwrap(), receipt);
    assert_eq!(resume(&plan.destination_root, &request()).unwrap(), receipt);
}

#[test]
fn immutable_plan_manifest_and_identity_tampering_prevents_admission() {
    for target in ["plan", "manifest", "repository"] {
        let (_temp, plan) = setup();
        let receipt = apply(&plan, &request()).unwrap();
        let path = match target {
            "plan" => plan
                .destination_root
                .join(format!("migrations/{}/plan.json", receipt.migration_id)),
            "manifest" => plan.destination_root.join(&receipt.manifest_path),
            _ => plan.destination_root.join("config.yml"),
        };
        if target == "repository" {
            let mut config = plan.options.config.clone();
            config.repository = Config::new("WD").unwrap().repository;
            fs::write(&path, serde_yaml_ng::to_string(&config).unwrap()).unwrap();
        } else {
            fs::write(&path, "tampered immutable metadata").unwrap();
        }
        assert!(
            Repository::open_source(&plan.destination_root).is_err(),
            "{target}"
        );
        assert!(
            resume(&plan.destination_root, &request()).is_err(),
            "{target}"
        );
        assert!(apply(&plan, &request()).is_err(), "{target}");
    }
}

#[test]
fn journal_and_final_receipt_fault_boundaries_resume() {
    for point in [
        MigrationFault::Batch {
            index: 0,
            point: FaultPoint::BeforeJournal,
        },
        MigrationFault::Batch {
            index: 0,
            point: FaultPoint::AfterJournal,
        },
        MigrationFault::Batch {
            index: 0,
            point: FaultPoint::BeforeChange(1),
        },
        MigrationFault::Batch {
            index: 0,
            point: FaultPoint::BeforeReceipt,
        },
        MigrationFault::Batch {
            index: 0,
            point: FaultPoint::AfterReceipt,
        },
        MigrationFault::Manifest(FaultPoint::BeforeJournal),
        MigrationFault::Manifest(FaultPoint::AfterChange(0)),
        MigrationFault::Cutover(FaultPoint::AfterJournal),
        MigrationFault::Cutover(FaultPoint::BeforeReceipt),
        MigrationFault::Cutover(FaultPoint::AfterReceipt),
        MigrationFault::AfterCutover,
    ] {
        let (_temp, plan) = setup();
        assert_eq!(
            apply_with_faults(&plan, &request(), |at| if at == point {
                Err(interruption())
            } else {
                Ok(())
            })
            .unwrap_err()
            .code,
            ErrorCode::RecoveryRequired,
            "{point:?}"
        );
        let receipt = resume(&plan.destination_root, &request()).unwrap();
        assert_eq!(apply(&plan, &request()).unwrap(), receipt);
    }
}

#[test]
fn multi_batch_payloads_resume_with_bounded_journals_and_exact_bytes() {
    let (_temp, initial) = setup();
    fs::create_dir(initial.source_root.join("handoffs")).unwrap();
    let payload = vec![0xa5; 9 * 1024 * 1024];
    for name in ["first.md", "second.md"] {
        fs::write(initial.source_root.join("handoffs").join(name), &payload).unwrap();
    }
    let plan = preview(
        &initial.source_root,
        &initial.destination_root,
        &initial.options,
    )
    .unwrap();
    assert!(plan.complete, "{:?}", plan.blockers);
    let error = apply_with_faults(&plan, &request(), |point| {
        if point
            == (MigrationFault::Batch {
                index: 1,
                point: FaultPoint::AfterChange(0),
            })
        {
            Err(interruption())
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    for entry in fs::read_dir(plan.destination_root.join(".tmp")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|extension| extension == "yml") {
            assert!(fs::metadata(path).unwrap().len() < 64 * 1024 * 1024);
        }
    }
    let receipt = resume(&plan.destination_root, &request()).unwrap();
    assert!(receipt.batches.len() >= 3);
    for name in ["first.md", "second.md"] {
        assert_eq!(
            fs::read(plan.destination_root.join("imported-handoffs").join(name)).unwrap(),
            payload
        );
    }
}

#[test]
fn competing_threads_share_one_durable_migration() {
    let (_temp, plan) = setup();
    let plan = std::sync::Arc::new(plan);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
    let handles = (0..2)
        .map(|_| {
            let plan = plan.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                apply(&plan, &request()).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let mut receipts = handles.into_iter().map(|handle| handle.join().unwrap());
    assert_eq!(receipts.next().unwrap(), receipts.next().unwrap());
}

#[test]
fn migration_subprocess_worker() {
    let Ok(plan_path) = std::env::var("WORKDECK_MIGRATION_TEST_PLAN") else {
        return;
    };
    let plan = serde_json::from_slice(&fs::read(plan_path).unwrap()).unwrap();
    let receipt = apply_with_faults(&plan, &request(), |point| {
        if std::env::var_os("WORKDECK_MIGRATION_TEST_CRASH").is_some()
            && point
                == (MigrationFault::Batch {
                    index: 0,
                    point: FaultPoint::AfterChange(0),
                })
        {
            std::process::exit(73);
        }
        Ok(())
    })
    .unwrap();
    fs::write(
        std::env::var("WORKDECK_MIGRATION_TEST_RESULT").unwrap(),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();
}

#[test]
fn ordinary_mutations_cannot_change_migration_metadata_or_cutover_identity() {
    use workdeck_pm::{
        ContentHash,
        transactions::{FileChange, PreparedOperation},
    };
    let (_temp, plan) = setup();
    let receipt = apply(&plan, &request()).unwrap();
    let store = TransactionStore::open(&plan.destination_root).unwrap();
    for path in [
        PathBuf::from("migration.yml"),
        receipt.manifest_path,
        PathBuf::from("migrations/new.yml"),
    ] {
        let expected = fs::read(plan.destination_root.join(&path))
            .ok()
            .as_deref()
            .map(ContentHash::of);
        let error = store
            .transact(&RequestId::new(), "edit", &serde_json::json!({}), |_| {
                Ok(PreparedOperation {
                    changes: vec![FileChange {
                        path: path.clone(),
                        expected,
                        content: Some(b"unauthorized metadata change".to_vec()),
                    }],
                    result: serde_json::json!({}),
                })
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::UnsafePath);
    }
    let current = fs::read(plan.destination_root.join("config.yml")).unwrap();
    let replacement = serde_yaml_ng::to_string(&Config::new("WD").unwrap()).unwrap();
    let error = store
        .transact(&RequestId::new(), "edit", &serde_json::json!({}), |_| {
            Ok(PreparedOperation {
                changes: vec![FileChange {
                    path: "config.yml".into(),
                    expected: Some(ContentHash::of(&current)),
                    content: Some(replacement.into_bytes()),
                }],
                result: serde_json::json!({}),
            })
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_eq!(
        fs::read(plan.destination_root.join("config.yml")).unwrap(),
        current
    );
}

#[test]
fn subprocess_competition_and_crash_release_both_coordination_and_writer_locks() {
    use std::process::{Command, Stdio};
    for crash in [false, true] {
        let (temp, plan) = setup();
        let saved = temp.path().join("reviewed.json");
        fs::write(&saved, serde_json::to_vec(&plan).unwrap()).unwrap();
        let spawn = |name: &str| {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", "migration_subprocess_worker", "--nocapture"])
                .env("WORKDECK_MIGRATION_TEST_PLAN", &saved)
                .env("WORKDECK_MIGRATION_TEST_RESULT", temp.path().join(name))
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            if crash {
                command.env("WORKDECK_MIGRATION_TEST_CRASH", "1");
            }
            command.spawn().unwrap()
        };
        let first = spawn("first.json");
        if crash {
            let output = first.wait_with_output().unwrap();
            assert_eq!(
                output.status.code(),
                Some(73),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(Repository::open_source(&plan.destination_root).is_err());
            let receipt = resume(&plan.destination_root, &request()).unwrap();
            assert_eq!(apply(&plan, &request()).unwrap(), receipt);
        } else {
            let second = spawn("second.json");
            for child in [first, second] {
                let output = child.wait_with_output().unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            assert_eq!(
                fs::read(temp.path().join("first.json")).unwrap(),
                fs::read(temp.path().join("second.json")).unwrap()
            );
        }
    }
}
