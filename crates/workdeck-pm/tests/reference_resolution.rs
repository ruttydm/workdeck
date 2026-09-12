use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
use workdeck_pm::{
    CreateIssue, CreatePlanning, ErrorCode, IssueMutation, IssueRecord, PlanningKind,
    ReferenceRetirementOutcome, Repository, RequestId, RetirementInput, RetirementKind,
    RetirementTarget, UpdateIssue,
};

fn fixture(kind: PlanningKind) -> (tempfile::TempDir, Repository, RetirementTarget) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let mut input = CreatePlanning::new("Retained planning");
    input.id = Some("old".into());
    repo.create_planning(kind, &input, &RequestId::new())
        .unwrap();
    (
        temp,
        repo,
        RetirementTarget::new(kind.into(), "old").unwrap(),
    )
}

fn issue(repo: &Repository, fields: BTreeMap<String, Value>) -> IssueRecord {
    let historical_alias = fields
        .get("labels")
        .and_then(Value::as_array)
        .is_some_and(|labels| labels.contains(&json!("OLD")));
    let mut authored = fields;
    if let Some(labels) = authored.get_mut("labels").and_then(Value::as_array_mut) {
        for label in labels {
            if *label == "OLD" {
                *label = json!("old");
            }
            let id = label.as_str().unwrap();
            if repo.planning_record(PlanningKind::Label, id).is_err() {
                let mut input = CreatePlanning::new(id);
                input.id = Some(id.into());
                repo.create_planning(PlanningKind::Label, &input, &RequestId::new())
                    .unwrap();
            }
        }
    }
    let record: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: "Preserve this issue".into(),
                body: "Authored **Markdown**.\n".into(),
                fields: authored,
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    if historical_alias {
        // Native writes now require canonical IDs; retain the existing regression
        // for old case aliases through an explicit historical source fixture.
        let path = repo.root().join(&record.path);
        let raw = fs::read_to_string(&path).unwrap();
        fs::write(path, raw.replace("- old", "- OLD")).unwrap();
        return repo.show_issue(record.metadata.id.as_str()).unwrap();
    }
    record
}

fn fields(target: &RetirementTarget) -> BTreeMap<String, Value> {
    match target.kind {
        RetirementKind::Project => BTreeMap::from([("project".into(), json!("old"))]),
        RetirementKind::Cycle => BTreeMap::from([("cycle".into(), json!("old"))]),
        RetirementKind::Label => {
            BTreeMap::from([("labels".into(), json!(["keep", "OLD", "other"]))])
        }
        _ => unreachable!(),
    }
}

#[test]
fn reviewed_resolution_clears_every_member_preserves_history_and_replays_original_result() {
    for kind in [
        PlanningKind::Project,
        PlanningKind::Cycle,
        PlanningKind::Label,
    ] {
        let (_temp, repo, target) = fixture(kind);
        let first = issue(&repo, fields(&target));
        let second = issue(&repo, fields(&target));
        repo.add_comment(
            first.metadata.id.as_str(),
            &first.source,
            "test",
            "Keep discussion",
            &RequestId::new(),
        )
        .unwrap();
        let comments = repo.comments(first.metadata.id.as_str()).unwrap();
        let preview = repo.reference_retirement_preview(&target).unwrap();
        assert!(preview.allowed);
        assert_eq!(preview.affected.len(), 2);
        let input = RetirementInput {
            target: target.clone(),
            expected: Some(preview.source.clone()),
            expected_preview: Some(preview.fingerprint.clone()),
        };
        let request = RequestId::new();
        let receipt = repo.retire_reference(&input, &request).unwrap();
        let result: ReferenceRetirementOutcome =
            serde_json::from_value(receipt.result.clone()).unwrap();
        assert_eq!(result.plan, preview);
        assert_eq!(result.affected.len(), 2);
        for original in [&first, &second] {
            let current = repo.show_issue(original.metadata.id.as_str()).unwrap();
            assert_eq!(current.body, original.body);
            assert_eq!(
                current.metadata.revision,
                original.metadata.revision.next().unwrap()
            );
            match kind {
                PlanningKind::Project => assert_eq!(current.metadata.project, None),
                PlanningKind::Cycle => assert_eq!(current.metadata.cycle, None),
                PlanningKind::Label => assert_eq!(current.metadata.labels, ["keep", "other"]),
                _ => unreachable!("fixture only covers baseline reference kinds"),
            }
        }
        assert_eq!(repo.comments(first.metadata.id.as_str()).unwrap(), comments);
        assert!(repo.tombstone(&target).unwrap().is_some());
        assert!(repo.doctor().unwrap().valid);
        repo.mutate_issue(
            first.metadata.id.as_str(),
            None,
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: BTreeMap::from([("title".into(), json!("Later title"))]),
                    body: None,
                },
            },
            &RequestId::new(),
        )
        .unwrap();
        assert_eq!(repo.retire_reference(&input, &request).unwrap(), receipt);
        assert_eq!(
            repo.show_issue(first.metadata.id.as_str())
                .unwrap()
                .metadata
                .title,
            "Later title"
        );
    }
}

#[test]
fn completed_member_blocks_resolution_until_explicit_reopen_without_side_effects() {
    let (_temp, repo, target) = fixture(PlanningKind::Project);
    let current = issue(&repo, fields(&target));
    repo.mutate_issue(
        current.metadata.id.as_str(),
        None,
        &IssueMutation::Complete { manual: None },
        &RequestId::new(),
    )
    .unwrap();
    let raw = fs::read(repo.root().join(&current.path)).unwrap();
    let plan = repo.reference_retirement_preview(&target).unwrap();
    assert!(!plan.allowed);
    assert_eq!(plan.blockers[0].code, ErrorCode::PolicyBlocked);
    assert!(plan.blockers[0].message.contains("reopen"));
    let input = RetirementInput {
        target,
        expected: None,
        expected_preview: Some(plan.fingerprint),
    };
    assert_eq!(
        repo.retire_reference(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(fs::read(repo.root().join(&current.path)).unwrap(), raw);
}

#[test]
fn removing_the_last_label_has_an_explicit_empty_collection_result() {
    let (_temp, repo, target) = fixture(PlanningKind::Label);
    let original = issue(&repo, BTreeMap::from([("labels".into(), json!(["old"]))]));
    let plan = repo.reference_retirement_preview(&target).unwrap();
    assert_eq!(plan.affected[0].after, json!([]));
    let input = RetirementInput {
        target,
        expected: None,
        expected_preview: Some(plan.fingerprint),
    };
    let receipt = repo.retire_reference(&input, &RequestId::new()).unwrap();
    let outcome: ReferenceRetirementOutcome = serde_json::from_value(receipt.result).unwrap();
    assert!(outcome.affected[0].metadata.labels.is_empty());
    assert!(
        repo.show_issue(original.metadata.id.as_str())
            .unwrap()
            .metadata
            .labels
            .is_empty()
    );
}

fn reviewed(repo: &Repository, target: &RetirementTarget) -> RetirementInput {
    let plan = repo.reference_retirement_preview(target).unwrap();
    RetirementInput {
        target: target.clone(),
        expected: Some(plan.source),
        expected_preview: Some(plan.fingerprint),
    }
}

fn authority(repo: &Repository) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn visit(
        root: &std::path::Path,
        path: &std::path::Path,
        files: &mut BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        for entry in fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            let relative = path.strip_prefix(root).unwrap();
            if [".tmp", ".index", ".local"]
                .iter()
                .any(|local| relative == std::path::Path::new(local))
            {
                continue;
            }
            if entry.file_type().unwrap().is_dir() {
                visit(root, &path, files);
            } else {
                files.insert(relative.to_owned(), fs::read(path).unwrap());
            }
        }
    }
    let mut files = BTreeMap::new();
    visit(repo.root(), repo.root(), &mut files);
    files
}

#[test]
fn missing_review_token_and_issue_targets_are_rejected_without_mutation() {
    let (_temp, repo, target) = fixture(PlanningKind::Project);
    let current = issue(&repo, fields(&target));
    let before = authority(&repo);
    assert_eq!(
        repo.retire_reference(&RetirementInput::new(target), &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(
        repo.reference_retirement_preview(
            &RetirementTarget::new(RetirementKind::Issue, current.metadata.id.as_str()).unwrap()
        )
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(authority(&repo), before);
}

#[test]
fn stale_membership_config_target_history_and_affected_history_preserve_all_authority() {
    for changed in [
        "member",
        "source",
        "config",
        "comment",
        "attachment",
        "target_history",
    ] {
        let (_temp, repo, target) = fixture(PlanningKind::Project);
        let current = issue(&repo, fields(&target));
        let input = reviewed(&repo, &target);
        match changed {
            "member" => {
                issue(&repo, fields(&target));
            }
            "source" => {
                let path = repo.root().join(&current.path);
                let raw = fs::read_to_string(&path).unwrap();
                fs::write(path, format!("{raw}\nDirect editor text.\n")).unwrap();
            }
            "config" => {
                let path = repo.root().join("config.yml");
                let raw = fs::read_to_string(&path).unwrap();
                fs::write(path, format!("{raw}\n# Direct config edit\n")).unwrap();
            }
            "comment" => {
                repo.add_comment(
                    current.metadata.id.as_str(),
                    &current.source,
                    "test",
                    "New discussion",
                    &RequestId::new(),
                )
                .unwrap();
            }
            "attachment" => {
                repo.attach_issue(
                    current.metadata.id.as_str(),
                    Some(&current.source),
                    &workdeck_pm::AttachmentInput {
                        name: "evidence.bin".into(),
                        content: vec![0, 255, 1],
                        media_type: None,
                        actor: "test".into(),
                    },
                    &RequestId::new(),
                )
                .unwrap();
            }
            "target_history" => {
                fs::write(
                    repo.root().join("projects/old/notes.md"),
                    "New planning context",
                )
                .unwrap();
            }
            _ => unreachable!(),
        }
        let before = authority(&repo);
        assert_eq!(
            repo.retire_reference(&input, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::StaleSource,
            "{changed}"
        );
        assert_eq!(authority(&repo), before, "{changed}");
    }
}

#[test]
fn lossy_rewrites_are_avoided_and_independent_binary_and_markdown_history_are_preserved() {
    let (_temp, repo, target) = fixture(PlanningKind::Project);
    let mut extra = fields(&target);
    extra.insert("custom".into(), json!({"nested":{"values":[1,"two"]}}));
    extra.insert("x-editor".into(), json!({"multiline":"first\nsecond"}));
    let current = issue(&repo, extra);
    let path = repo.root().join(&current.path);
    let raw = fs::read_to_string(&path)
        .unwrap()
        .replacen("---\n", "---\n# Keep header comment\n", 1)
        .replace('\n', "\r\n");
    fs::write(&path, &raw).unwrap();
    let current = repo.show_issue(current.metadata.id.as_str()).unwrap();
    repo.add_comment(
        current.metadata.id.as_str(),
        &current.source,
        "test",
        "Keep discussion",
        &RequestId::new(),
    )
    .unwrap();
    repo.attach_issue(
        current.metadata.id.as_str(),
        Some(&current.source),
        &workdeck_pm::AttachmentInput {
            name: "proof.bin".into(),
            content: vec![0, 255, 10, 13, 128],
            media_type: None,
            actor: "test".into(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let before = authority(&repo);
    let input = reviewed(&repo, &target);
    let receipt = repo.retire_reference(&input, &RequestId::new()).unwrap();
    assert_eq!(receipt.changed.len(), 3);
    let after = authority(&repo);
    for (path, bytes) in before.iter().filter(|(path, _)| {
        path.starts_with(current.path.parent().unwrap()) && *path != &current.path
    }) {
        assert_eq!(after.get(path), Some(bytes));
    }
    let edited = fs::read_to_string(path).unwrap();
    assert!(edited.contains("# Keep header comment\r\n"));
    assert!(!edited.replace("\r\n", "").contains('\n'));
    let result = repo.show_issue(current.metadata.id.as_str()).unwrap();
    assert_eq!(result.metadata.custom, current.metadata.custom);
    assert_eq!(result.metadata.extra, current.metadata.extra);
    assert_eq!(result.body, current.body);
    assert!(repo.export_snapshot().unwrap().validate().is_ok());
}

#[test]
fn journal_faults_never_expose_successful_partial_reads_and_recovery_replays_one_receipt() {
    use workdeck_pm::transactions::{FaultPoint, TransactionStore};
    for point in [
        FaultPoint::AfterJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::BeforeReceipt,
        FaultPoint::AfterReceipt,
    ] {
        let (_temp, repo, target) = fixture(PlanningKind::Project);
        let first = issue(&repo, fields(&target));
        issue(&repo, fields(&target));
        let input = reviewed(&repo, &target);
        let request = RequestId::new();
        let error = repo
            .retire_reference_with_faults(&input, &request, |found| {
                if found == point {
                    Err(workdeck_pm::PmError::new(
                        ErrorCode::Io,
                        "interrupted resolution",
                    ))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired, "{point:?}");
        assert_eq!(
            repo.show_issue(first.metadata.id.as_str())
                .unwrap_err()
                .code,
            ErrorCode::RecoveryRequired
        );
        let recovered = TransactionStore::open(repo.root())
            .unwrap()
            .recover()
            .unwrap();
        assert_eq!(recovered.len(), 1);
        assert_eq!(
            repo.retire_reference(&input, &request).unwrap(),
            recovered[0]
        );
        assert!(
            repo.show_issue(first.metadata.id.as_str())
                .unwrap()
                .metadata
                .project
                .is_none()
        );
        assert!(repo.tombstone(&target).unwrap().is_some());
        assert!(repo.doctor().unwrap().valid);
    }
}

#[test]
fn direct_edit_before_journal_is_stale_and_during_partial_apply_requires_conflict_resolution() {
    use workdeck_pm::transactions::{FaultPoint, TransactionStore};
    for partial in [false, true] {
        let (_temp, repo, target) = fixture(PlanningKind::Project);
        let first = issue(&repo, fields(&target));
        let second = issue(&repo, fields(&target));
        let input = reviewed(&repo, &target);
        let request = RequestId::new();
        let path = repo.root().join(&second.path);
        let original = fs::read_to_string(&path).unwrap();
        let external = format!("{original}\nConcurrent editor work\n");
        let before_receipts = fs::read_dir(repo.root().join("operations"))
            .unwrap()
            .count();
        let error = repo
            .retire_reference_with_faults(&input, &request, |point| {
                if point
                    == if partial {
                        FaultPoint::AfterChange(0)
                    } else {
                        FaultPoint::BeforeJournal
                    }
                {
                    fs::write(&path, &external).unwrap();
                    if partial {
                        return Err(workdeck_pm::PmError::new(
                            ErrorCode::Io,
                            "interrupted after direct edit",
                        ));
                    }
                }
                Ok(())
            })
            .unwrap_err();
        assert_eq!(fs::read_to_string(&path).unwrap(), external);
        assert_eq!(
            fs::read_dir(repo.root().join("operations"))
                .unwrap()
                .count(),
            before_receipts
        );
        if partial {
            assert_eq!(error.code, ErrorCode::RecoveryRequired);
            assert_eq!(
                repo.show_issue(first.metadata.id.as_str())
                    .unwrap_err()
                    .code,
                ErrorCode::RecoveryRequired
            );
            let store = TransactionStore::open(repo.root()).unwrap();
            assert_eq!(
                store.recover().unwrap_err().code,
                ErrorCode::RecoveryRequired
            );
            assert_eq!(fs::read_to_string(&path).unwrap(), external);
            fs::write(&path, original).unwrap();
            store.recover().unwrap();
            repo.retire_reference(&input, &request).unwrap();
        } else {
            assert_eq!(error.code, ErrorCode::StaleSource);
            assert_eq!(
                repo.show_issue(first.metadata.id.as_str())
                    .unwrap()
                    .metadata
                    .project,
                Some("old".into())
            );
        }
    }
}

#[test]
fn forged_historical_results_and_missing_retirement_markers_cannot_resurrect_identity() {
    for tamper in ["result", "manifest", "marker", "missing_marker"] {
        let (_temp, repo, target) = fixture(PlanningKind::Project);
        issue(&repo, fields(&target));
        let input = reviewed(&repo, &target);
        let request = RequestId::new();
        let receipt = repo.retire_reference(&input, &request).unwrap();
        let path = repo
            .root()
            .join(format!("operations/{}.yml", receipt.operation_id));
        match tamper {
            "result" | "manifest" => {
                let mut forged = receipt.clone();
                if tamper == "result" {
                    forged.result["affected"][0]["metadata"]["title"] = json!("Forged title");
                } else {
                    forged.changed[0].path = "issues/WD-999/item.md".into();
                }
                fs::write(path, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
                assert_eq!(
                    repo.retire_reference(&input, &request).unwrap_err().code,
                    ErrorCode::CorruptStore
                );
            }
            "marker" => {
                let path = repo.root().join("tombstones/projects/old.yml");
                let mut marker: Value =
                    serde_yaml_ng::from_slice(&fs::read(&path).unwrap()).unwrap();
                marker["request_id"] = json!("forged-request");
                fs::write(path, serde_yaml_ng::to_string(&marker).unwrap()).unwrap();
            }
            "missing_marker" => {
                fs::remove_file(repo.root().join("tombstones/projects/old.yml")).unwrap()
            }
            _ => unreachable!(),
        }
        let before = authority(&repo);
        assert!(repo.tombstone(&target).is_err(), "{tamper}");
        let mut input = CreatePlanning::new("No reuse");
        input.id = Some("OLD".into());
        assert!(
            repo.create_planning(PlanningKind::Project, &input, &RequestId::new())
                .is_err(),
            "{tamper}"
        );
        assert_eq!(authority(&repo), before);
    }
}

#[test]
fn label_registry_siblings_remain_editable_after_each_membership_resolution() {
    let (_temp, repo, target) = fixture(PlanningKind::Label);
    let mut keep = CreatePlanning::new("Keep label");
    keep.id = Some("keep".into());
    repo.create_planning(PlanningKind::Label, &keep, &RequestId::new())
        .unwrap();
    let current = issue(&repo, fields(&target));
    let old_input = reviewed(&repo, &target);
    let old_request = RequestId::new();
    let old_receipt = repo.retire_reference(&old_input, &old_request).unwrap();
    let keep_target = RetirementTarget::new(RetirementKind::Label, "keep").unwrap();
    repo.retire_reference(&reviewed(&repo, &keep_target), &RequestId::new())
        .unwrap();
    assert!(repo.tombstone(&target).unwrap().is_some());
    assert!(repo.tombstone(&keep_target).unwrap().is_some());
    assert_eq!(
        repo.show_issue(current.metadata.id.as_str())
            .unwrap()
            .metadata
            .labels,
        ["other"]
    );
    assert_eq!(
        repo.retire_reference(&old_input, &old_request).unwrap(),
        old_receipt
    );
    assert!(repo.doctor().unwrap().valid);
}

#[test]
fn reopening_blocked_accepted_work_allows_a_new_plan_without_reusing_old_acceptance() {
    let (_temp, repo, target) = fixture(PlanningKind::Cycle);
    let current = issue(&repo, fields(&target));
    repo.mutate_issue(
        current.metadata.id.as_str(),
        None,
        &IssueMutation::Complete { manual: None },
        &RequestId::new(),
    )
    .unwrap();
    let blocked = repo.reference_retirement_preview(&target).unwrap();
    assert!(!blocked.allowed);
    repo.mutate_issue(
        current.metadata.id.as_str(),
        None,
        &IssueMutation::Reopen,
        &RequestId::new(),
    )
    .unwrap();
    let plan = repo.reference_retirement_preview(&target).unwrap();
    assert!(plan.allowed);
    assert_ne!(plan.fingerprint, blocked.fingerprint);
    let input = RetirementInput {
        target,
        expected: None,
        expected_preview: Some(plan.fingerprint),
    };
    repo.retire_reference(&input, &RequestId::new()).unwrap();
    let current = repo.show_issue(current.metadata.id.as_str()).unwrap();
    assert_eq!(current.metadata.status, "ready");
    assert!(current.metadata.completed_at.is_none());
    assert!(current.metadata.manual_acceptance.is_none());
    assert!(current.metadata.cycle.is_none());
}

#[test]
fn zero_members_still_retire_through_the_reviewed_protocol_and_reject_changed_retry_input() {
    let (_temp, repo, target) = fixture(PlanningKind::Project);
    let plan = repo.reference_retirement_preview(&target).unwrap();
    assert!(plan.allowed);
    assert!(plan.affected.is_empty());
    let input = reviewed(&repo, &target);
    let request = RequestId::new();
    let receipt = repo.retire_reference(&input, &request).unwrap();
    assert_eq!(receipt.changed.len(), 2);
    let mut different = input.clone();
    different.expected = None;
    assert_eq!(
        repo.retire_reference(&different, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    let mut config = repo.config().unwrap();
    config.prefix = "NEW".into();
    fs::write(
        repo.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    assert_eq!(repo.retire_reference(&input, &request).unwrap(), receipt);
    assert!(repo.doctor().unwrap().valid);
}

#[test]
fn reference_resolution_child_process_crash_helper() {
    let Some(directory) = std::env::var_os("WORKDECK_REFERENCE_RESOLUTION_CRASH") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    let input: RetirementInput =
        serde_json::from_slice(&fs::read(directory.join("input.json")).unwrap()).unwrap();
    let repo = Repository::open_source(&directory.join(".workdeck")).unwrap();
    let request = "process-crash".parse().unwrap();
    let _ = repo.retire_reference_with_faults(&input, &request, |point| {
        if point == workdeck_pm::transactions::FaultPoint::AfterChange(0) {
            std::process::exit(41);
        }
        Ok(())
    });
    panic!("did not reach partial issue resolution");
}

#[test]
fn process_exit_between_reference_clear_and_tombstone_remains_explicitly_recoverable() {
    let (temp, repo, target) = fixture(PlanningKind::Project);
    let current = issue(&repo, fields(&target));
    let input = reviewed(&repo, &target);
    fs::write(
        temp.path().join("input.json"),
        serde_json::to_vec(&input).unwrap(),
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "reference_resolution_child_process_crash_helper"])
        .env("WORKDECK_REFERENCE_RESOLUTION_CRASH", temp.path())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(41));
    assert!(!repo.root().join("tombstones/projects/old.yml").exists());
    assert_eq!(
        repo.show_issue(current.metadata.id.as_str())
            .unwrap_err()
            .code,
        ErrorCode::RecoveryRequired
    );
    let receipts = workdeck_pm::transactions::TransactionStore::open(repo.root())
        .unwrap()
        .recover()
        .unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(
        repo.retire_reference(&input, &"process-crash".parse().unwrap())
            .unwrap(),
        receipts[0]
    );
    assert!(
        repo.show_issue(current.metadata.id.as_str())
            .unwrap()
            .metadata
            .project
            .is_none()
    );
    assert!(repo.tombstone(&target).unwrap().is_some());
}

#[cfg(unix)]
#[test]
fn fifo_and_symlink_history_fail_without_opening_special_sources_or_writing_authority() {
    use std::os::unix::fs::symlink;
    for fifo in [false, true] {
        let (_temp, repo, target) = fixture(PlanningKind::Project);
        let current = issue(&repo, fields(&target));
        let path = repo
            .root()
            .join(current.path.parent().unwrap())
            .join("unsafe.md");
        if fifo {
            assert!(
                std::process::Command::new("mkfifo")
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success()
            );
        } else {
            symlink(repo.root().join("config.yml"), &path).unwrap();
        }
        let receipts = fs::read_dir(repo.root().join("operations"))
            .unwrap()
            .count();
        let started = std::time::Instant::now();
        assert_eq!(
            repo.reference_retirement_preview(&target).unwrap_err().code,
            ErrorCode::UnsafePath
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(
            fs::read_dir(repo.root().join("operations"))
                .unwrap()
                .count(),
            receipts
        );
        assert!(!repo.root().join("tombstones/projects/old.yml").exists());
    }
}

#[test]
fn oversized_membership_is_rejected_before_preparing_or_publishing_issue_changes() {
    let (_temp, repo, target) = fixture(PlanningKind::Project);
    let current = issue(&repo, fields(&target));
    let raw = fs::read_to_string(repo.root().join(&current.path)).unwrap();
    for _ in 0..1000 {
        let id = workdeck_pm::IssueId::new("WD").unwrap();
        let directory = repo.root().join("issues").join(id.as_str());
        fs::create_dir(&directory).unwrap();
        fs::write(
            directory.join("item.md"),
            raw.replace(current.metadata.id.as_str(), id.as_str()),
        )
        .unwrap();
    }
    let before = authority(&repo);
    assert_eq!(
        repo.reference_retirement_preview(&target).unwrap_err().code,
        ErrorCode::Unsupported
    );
    assert_eq!(authority(&repo), before);
}

#[test]
fn concurrent_identical_requests_publish_one_composite_result() {
    let (_temp, repo, target) = fixture(PlanningKind::Project);
    issue(&repo, fields(&target));
    let input = reviewed(&repo, &target);
    let request = RequestId::new();
    let count = fs::read_dir(repo.root().join("operations"))
        .unwrap()
        .count();
    std::thread::scope(|scope| {
        let mut tasks = Vec::new();
        for _ in 0..2 {
            let input = &input;
            let request = &request;
            let root = repo.root();
            tasks.push(scope.spawn(move || {
                Repository::open_source(root)
                    .unwrap()
                    .retire_reference(input, request)
                    .unwrap()
            }));
        }
        let first = tasks.remove(0).join().unwrap();
        let second = tasks.remove(0).join().unwrap();
        assert_eq!(first, second);
    });
    assert_eq!(
        fs::read_dir(repo.root().join("operations"))
            .unwrap()
            .count(),
        count + 1
    );
}
