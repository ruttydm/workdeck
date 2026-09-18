use serde_json::{Value, json};
use std::{collections::BTreeMap, fs};
use workdeck_pm::transactions::FaultPoint;
use workdeck_pm::{
    ContentHash, CreateIssue, CreatePlanning, ErrorCode, IssueMutation, IssueRecord, PlanningKind,
    PmError, RecordId, Repository, RequestId, SnapshotImportMode, TimeEntry, TimeEntryAmendment,
    TimeEntryInput, TimeEntryRecord, TimeReportQuery, UpdateIssue,
};

fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let dir = tempfile::tempdir().unwrap();
    let repo = Repository::init(dir.path(), "WD").unwrap();
    for id in ["first", "second"] {
        let mut cycle = CreatePlanning::new(id);
        cycle.id = Some(id.into());
        repo.create_planning(PlanningKind::Cycle, &cycle, &RequestId::new())
            .unwrap();
    }
    let mut input = CreateIssue::new("Time fixture", "Keep authored history.\n");
    input.fields.insert("cycle".into(), json!("first"));
    let issue =
        serde_json::from_value(repo.create_issue(&input, &RequestId::new()).unwrap().result)
            .unwrap();
    (dir, repo, issue)
}
fn input(seconds: u64) -> TimeEntryInput {
    TimeEntryInput {
        user: "worker".into(),
        actor: "recorder".into(),
        seconds,
        worked_at: "2026-09-01T10:00:00Z".parse().unwrap(),
    }
}
fn logged(repo: &Repository, issue: &IssueRecord, seconds: u64) -> TimeEntryRecord {
    serde_json::from_value(
        repo.log_time(
            issue.metadata.id.as_str(),
            None,
            &input(seconds),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}
fn update(repo: &Repository, issue: &IssueRecord, fields: BTreeMap<String, Value>) {
    repo.mutate_issue(
        issue.metadata.id.as_str(),
        None,
        &IssueMutation::Update {
            input: UpdateIssue { fields, body: None },
        },
        &RequestId::new(),
    )
    .unwrap();
}

#[test]
fn independent_time_files_capture_attribution_without_touching_issue_bytes() {
    let (_dir, repo, issue) = fixture();
    let original = fs::read(repo.root().join(&issue.path)).unwrap();
    let record = logged(&repo, &issue, 3700);
    assert!(record.entry.id.as_str().starts_with("TIME-"));
    assert_eq!(
        record.path.to_string_lossy(),
        format!("issues/{}/time/{}.yml", issue.metadata.id, record.entry.id)
    );
    assert_eq!(record.entry.cycle.as_deref(), Some("first"));
    assert_eq!(record.entry.user, "worker");
    assert_eq!(record.entry.actor, "recorder");
    assert_eq!(fs::read(repo.root().join(&issue.path)).unwrap(), original);
    assert_eq!(
        repo.show_issue(issue.metadata.id.as_str()).unwrap().source,
        issue.source
    );
    assert_eq!(
        repo.time_entries(issue.metadata.id.as_str()).unwrap(),
        [record]
    );
    assert!(repo.doctor().unwrap().valid);
}

#[test]
fn amendments_count_once_retain_cycle_and_never_deduplicate_equal_work() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    let original = fs::read(repo.root().join(&first.path)).unwrap();
    let duplicate_work = logged(&repo, &issue, 60);
    update(
        &repo,
        &issue,
        BTreeMap::from([("cycle".into(), json!("second"))]),
    );
    let mut replacement = input(90);
    replacement.user = "corrected-worker".into();
    let amendment = TimeEntryAmendment {
        entry: replacement,
        supersedes: first.entry.id.clone(),
        expected: first.content.clone(),
        reason: "Correct duration and attribution".into(),
    };
    let amended: TimeEntryRecord = serde_json::from_value(
        repo.amend_time(
            issue.metadata.id.as_str(),
            None,
            &amendment,
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    assert_eq!(amended.entry.cycle.as_deref(), Some("first"));
    assert_eq!(amended.entry.supersedes, Some(first.entry.id));
    let last = logged(&repo, &issue, 30);
    let report = repo.time_report(&TimeReportQuery::default()).unwrap();
    assert_eq!(report.total_seconds, 180);
    assert_eq!(report.entries.len(), 3);
    assert!(report.entries.contains(&duplicate_work));
    assert!(report.entries.contains(&amended));
    assert!(report.entries.contains(&last));
    assert_eq!(report.by_cycle["first"], 150);
    assert_eq!(report.by_cycle["second"], 30);
    assert_eq!(report.by_user["corrected-worker"], 90);
    assert_eq!(fs::read(repo.root().join(&first.path)).unwrap(), original);
}

#[test]
fn time_records_roundtrip_in_complete_native_snapshot_restoration() {
    let (_dir, repo, issue) = fixture();
    let entry = logged(&repo, &issue, 3600);
    let snapshot = repo.export_snapshot().unwrap();
    assert!(snapshot.files.iter().any(|file| file.path == entry.path));
    let target = tempfile::tempdir().unwrap();
    let root = target.path().join(".workdeck");
    let plan = workdeck_pm::preview_snapshot_restore(&root, &snapshot).unwrap();
    assert!(plan.allowed, "{:?}", plan.blockers);
    workdeck_pm::restore_snapshot(&root, &snapshot, Some(&plan.fingerprint), &RequestId::new())
        .unwrap();
    let restored = Repository::open_source(&root).unwrap();
    assert_eq!(
        restored.time_entries(issue.metadata.id.as_str()).unwrap(),
        [entry]
    );
    assert!(restored.doctor().unwrap().valid);
}

fn amendment(record: &TimeEntryRecord, seconds: u64) -> TimeEntryAmendment {
    TimeEntryAmendment {
        entry: input(seconds),
        supersedes: record.entry.id.clone(),
        expected: record.content.clone(),
        reason: "Correct recorded duration".into(),
    }
}
fn authority(repo: &Repository) -> BTreeMap<std::path::PathBuf, Vec<u8>> {
    repo.export_snapshot()
        .unwrap()
        .files
        .into_iter()
        .map(|file| (file.path, file.content))
        .collect()
}
fn write_entry(repo: &Repository, entry: &TimeEntry) -> std::path::PathBuf {
    let path = std::path::PathBuf::from(format!("issues/{}/time/{}.yml", entry.issue, entry.id));
    fs::create_dir_all(repo.root().join(&path).parent().unwrap()).unwrap();
    fs::write(
        repo.root().join(&path),
        serde_yaml_ng::to_string(entry).unwrap(),
    )
    .unwrap();
    path
}

#[test]
fn replay_precedes_later_issue_config_and_retirement_state() {
    let (_dir, repo, issue) = fixture();
    let reference = &issue.metadata.id.as_str()[..12];
    let request = RequestId::new();
    let receipt = repo
        .log_time(reference, Some(&issue.source), &input(60), &request)
        .unwrap();
    let first: TimeEntryRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    let amendment = amendment(&first, 90);
    let amended_request = RequestId::new();
    let amended = repo
        .amend_time(reference, Some(&issue.source), &amendment, &amended_request)
        .unwrap();
    update(
        &repo,
        &issue,
        BTreeMap::from([("cycle".into(), json!("second"))]),
    );
    let mut config = repo.config().unwrap();
    config.acceptance.required_checks = vec!["future-check".into()];
    fs::write(
        repo.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    let preview = repo.retirement_preview_issue(reference).unwrap();
    assert!(preview.allowed);
    repo.retire_issue(
        reference,
        Some(&preview.source),
        Some(&preview.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.log_time(reference, Some(&issue.source), &input(60), &request)
            .unwrap(),
        receipt
    );
    assert_eq!(
        repo.amend_time(reference, Some(&issue.source), &amendment, &amended_request)
            .unwrap(),
        amended
    );
    assert_eq!(repo.time_entries(reference).unwrap().len(), 2);
    assert_eq!(
        repo.time_report(&TimeReportQuery::default())
            .unwrap()
            .total_seconds,
        90
    );
    assert_eq!(
        repo.log_time(reference, None, &input(60), &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        repo.log_time(reference, Some(&issue.source), &input(61), &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn actor_reason_types_and_future_work_fail_without_authority_writes() {
    let (_dir, repo, issue) = fixture();
    let original = authority(&repo);
    for invalid in [
        TimeEntryInput {
            user: " ".into(),
            ..input(1)
        },
        TimeEntryInput {
            actor: "line\nbreak".into(),
            ..input(1)
        },
        TimeEntryInput {
            actor: "x".repeat(257),
            ..input(1)
        },
        TimeEntryInput {
            worked_at: "9999-01-01T00:00:00Z".parse().unwrap(),
            ..input(1)
        },
    ] {
        assert!(
            repo.log_time(
                issue.metadata.id.as_str(),
                None,
                &invalid,
                &RequestId::new()
            )
            .is_err()
        );
        assert_eq!(authority(&repo), original);
    }
    for seconds in [json!(-1), json!(1.5), json!("60")] {
        let mut value = serde_json::to_value(input(1)).unwrap();
        value["seconds"] = seconds;
        assert!(serde_json::from_value::<TimeEntryInput>(value).is_err());
    }
    let first = logged(&repo, &issue, 1);
    let original = authority(&repo);
    let mut invalid = amendment(&first, 0);
    invalid.reason = " ".into();
    assert!(
        repo.amend_time(
            issue.metadata.id.as_str(),
            None,
            &invalid,
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(authority(&repo), original);
}

#[test]
fn issue_and_entry_source_preconditions_detect_direct_edits() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    update(
        &repo,
        &issue,
        BTreeMap::from([("title".into(), json!("Later title"))]),
    );
    let original = authority(&repo);
    assert_eq!(
        repo.log_time(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &input(1),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    let mut changed = first.entry.clone();
    changed.seconds = 62;
    write_entry(&repo, &changed);
    assert_eq!(
        repo.amend_time(
            issue.metadata.id.as_str(),
            None,
            &amendment(&first, 90),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        repo.time_entries(issue.metadata.id.as_str()).unwrap().len(),
        1
    );
    assert_eq!(
        fs::read(repo.root().join(&issue.path)).unwrap(),
        original[&issue.path]
    );
}

#[test]
fn concurrent_retries_create_one_identity_and_competing_amendments_cannot_fork() {
    let (_dir, repo, issue) = fixture();
    let request = RequestId::new();
    let barrier = std::sync::Barrier::new(2);
    let results = std::thread::scope(|scope| {
        let handles = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    repo.log_time(
                        issue.metadata.id.as_str(),
                        Some(&issue.source),
                        &input(60),
                        &request,
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results[0], results[1]);
    let first: TimeEntryRecord = serde_json::from_value(results[0].result.clone()).unwrap();
    let barrier = std::sync::Barrier::new(2);
    let amended = std::thread::scope(|scope| {
        let handles = (0..2)
            .map(|_| {
                scope.spawn(|| {
                    barrier.wait();
                    repo.amend_time(
                        issue.metadata.id.as_str(),
                        None,
                        &amendment(&first, 90),
                        &RequestId::new(),
                    )
                })
            })
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(amended.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        amended.into_iter().find_map(Result::err).unwrap().code,
        ErrorCode::Conflict
    );
    assert_eq!(
        repo.time_entries(issue.metadata.id.as_str()).unwrap().len(),
        2
    );
    assert_eq!(
        repo.time_report(&TimeReportQuery::default())
            .unwrap()
            .total_seconds,
        90
    );
}

#[test]
fn report_filters_apply_after_complete_chain_resolution() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    let mut replacement = amendment(&first, 90);
    replacement.entry.user = "other".into();
    replacement.entry.worked_at = "2026-09-02T10:00:00Z".parse().unwrap();
    let second: TimeEntryRecord = serde_json::from_value(
        repo.amend_time(
            issue.metadata.id.as_str(),
            None,
            &replacement,
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let zero: TimeEntryRecord = serde_json::from_value(
        repo.amend_time(
            issue.metadata.id.as_str(),
            None,
            &amendment(&second, 0),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let report = repo
        .time_report(&TimeReportQuery {
            user: Some("other".into()),
            ..Default::default()
        })
        .unwrap();
    assert!(report.entries.is_empty());
    let report = repo
        .time_report(&TimeReportQuery {
            from: Some("2026-09-02T00:00:00Z".parse().unwrap()),
            ..Default::default()
        })
        .unwrap();
    assert!(
        report.entries.is_empty(),
        "date filtering revived a superseded record"
    );
    let report = repo
        .time_report(&TimeReportQuery {
            issue: Some(issue.metadata.id.as_str()[..12].into()),
            cycle: Some("first".into()),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(report.entries, [zero]);
    assert_eq!(report.total_seconds, 0);
    assert_eq!(
        repo.time_report(&TimeReportQuery {
            from: Some(input(1).worked_at),
            to: Some(input(1).worked_at),
            ..Default::default()
        })
        .unwrap_err()
        .code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn totals_group_multiple_issues_and_unassigned_cycles_without_overflow() {
    let (_dir, repo, issue) = fixture();
    logged(&repo, &issue, 60);
    let second: IssueRecord = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Other", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    logged(&repo, &second, 30);
    let report = repo.time_report(&TimeReportQuery::default()).unwrap();
    assert_eq!(report.total_seconds, 90);
    assert_eq!(report.by_issue[issue.metadata.id.as_str()], 60);
    assert_eq!(report.by_issue[second.metadata.id.as_str()], 30);
    assert_eq!(report.by_user["worker"], 90);
    assert_eq!(report.unassigned_cycle_seconds, 30);
    logged(&repo, &second, u64::MAX);
    assert_eq!(
        repo.time_report(&TimeReportQuery::default())
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
}

#[test]
fn duplicate_time_identity_across_issue_directories_is_never_hidden_by_filters() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    let second: IssueRecord = serde_json::from_value(
        repo.create_issue(&CreateIssue::new("Other", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut duplicate = first.entry.clone();
    duplicate.issue = second.metadata.id;
    write_entry(&repo, &duplicate);
    assert!(!repo.doctor().unwrap().valid);
    assert!(
        repo.time_report(&TimeReportQuery {
            issue: Some(issue.metadata.id.to_string()),
            ..Default::default()
        })
        .is_err()
    );
    assert!(repo.export_snapshot().is_err());
    assert!(
        repo.log_time(
            issue.metadata.id.as_str(),
            None,
            &input(1),
            &RequestId::new()
        )
        .is_err()
    );
}

#[test]
fn doctor_rejects_forks_cycles_orphans_and_wrong_repository_or_schema() {
    for case in [
        "fork",
        "cycle",
        "missing_predecessor",
        "orphan",
        "repository",
        "schema",
        "unknown",
        "path",
    ] {
        let (_dir, repo, issue) = fixture();
        let first = logged(&repo, &issue, 60);
        let mut bad = first.entry.clone();
        bad.id = RecordId::new("TIME").unwrap();
        match case {
            "fork" => {
                repo.amend_time(
                    issue.metadata.id.as_str(),
                    None,
                    &amendment(&first, 90),
                    &RequestId::new(),
                )
                .unwrap();
                bad.supersedes = Some(first.entry.id);
                bad.reason = Some("fork".into());
            }
            "cycle" => {
                bad.supersedes = Some(bad.id.clone());
                bad.reason = Some("cycle".into());
            }
            "missing_predecessor" => {
                bad.supersedes = Some(RecordId::new("TIME").unwrap());
                bad.reason = Some("missing".into());
            }
            "orphan" => {
                bad.issue = RecordId::new("WD").unwrap();
            }
            "repository" => {
                bad.repository = workdeck_pm::RepositoryId::new();
            }
            _ => {}
        }
        let path = write_entry(&repo, &bad);
        if case == "schema" {
            let text = fs::read_to_string(repo.root().join(&path))
                .unwrap()
                .replacen("schema: 1", "schema: 2", 1);
            fs::write(repo.root().join(&path), text).unwrap();
        }
        if case == "unknown" {
            let mut text = fs::read_to_string(repo.root().join(&path)).unwrap();
            text.push_str("secconds: 99\n");
            fs::write(repo.root().join(&path), text).unwrap();
        }
        if case == "path" {
            fs::rename(
                repo.root().join(&path),
                repo.root().join(&path).with_file_name("invalid.yml"),
            )
            .unwrap();
        }
        let report = repo.doctor().unwrap();
        assert!(!report.valid, "{case}");
        if case == "schema" {
            assert!(
                report
                    .errors
                    .iter()
                    .any(|error| error.code == ErrorCode::UnsupportedSchema)
            );
        }
        assert!(
            repo.time_report(&TimeReportQuery::default()).is_err(),
            "{case}"
        );
        assert!(repo.export_snapshot().is_err(), "{case}");
    }
}

#[test]
fn snapshot_replace_matching_cannot_rewrite_immutable_time_records() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    let bytes = fs::read(repo.root().join(&first.path)).unwrap();
    let mut direct = first.entry.clone();
    direct.seconds = 90;
    write_entry(&repo, &direct);
    let changed = repo.export_snapshot().unwrap();
    fs::write(repo.root().join(&first.path), &bytes).unwrap();
    let plan = repo
        .preview_snapshot_import(&changed, SnapshotImportMode::ReplaceMatching)
        .unwrap();
    assert!(!plan.allowed);
    assert!(plan.blockers.iter().any(|error| {
        error.code == ErrorCode::Conflict
            && error
                .path
                .as_ref()
                .is_some_and(|path| path.ends_with(&first.path.to_string_lossy().to_string()))
    }));
    assert!(
        repo.import_snapshot(
            &changed,
            SnapshotImportMode::ReplaceMatching,
            Some(&plan.fingerprint),
            &RequestId::new()
        )
        .is_err()
    );
    assert_eq!(fs::read(repo.root().join(&first.path)).unwrap(), bytes);
}

#[test]
fn interrupted_log_and_amendment_recover_once_without_rewriting_the_issue() {
    for amend in [false, true] {
        for point in [
            FaultPoint::BeforeJournal,
            FaultPoint::AfterJournal,
            FaultPoint::BeforeChange(0),
            FaultPoint::AfterChange(0),
            FaultPoint::BeforeReceipt,
            FaultPoint::AfterReceipt,
        ] {
            let (_dir, repo, issue) = fixture();
            let initial = logged(&repo, &issue, 60);
            let original = fs::read(repo.root().join(&issue.path)).unwrap();
            let request = RequestId::new();
            let correction = amendment(&initial, 90);
            let fault = |seen| {
                if seen == point {
                    Err(PmError::new(ErrorCode::Io, "simulated interruption"))
                } else {
                    Ok(())
                }
            };
            let result = if amend {
                repo.amend_time_with_faults(
                    issue.metadata.id.as_str(),
                    Some(&issue.source),
                    &correction,
                    &request,
                    fault,
                )
            } else {
                repo.log_time_with_faults(
                    issue.metadata.id.as_str(),
                    Some(&issue.source),
                    &input(30),
                    &request,
                    fault,
                )
            };
            assert!(result.is_err());
            if point != FaultPoint::BeforeJournal {
                assert_eq!(
                    repo.time_entries(issue.metadata.id.as_str())
                        .unwrap_err()
                        .code,
                    ErrorCode::RecoveryRequired
                );
                assert_eq!(repo.recover_operations().unwrap().len(), 1);
            }
            let receipt = if amend {
                repo.amend_time(
                    issue.metadata.id.as_str(),
                    Some(&issue.source),
                    &correction,
                    &request,
                )
            } else {
                repo.log_time(
                    issue.metadata.id.as_str(),
                    Some(&issue.source),
                    &input(30),
                    &request,
                )
            }
            .unwrap();
            assert_eq!(
                repo.time_entries(issue.metadata.id.as_str()).unwrap().len(),
                2
            );
            assert_eq!(
                repo.time_report(&TimeReportQuery::default())
                    .unwrap()
                    .total_seconds,
                90
            );
            assert_eq!(fs::read(repo.root().join(&issue.path)).unwrap(), original);
            let recorded: TimeEntryRecord = serde_json::from_value(receipt.result).unwrap();
            assert_eq!(
                recorded.content,
                ContentHash::of(&fs::read(repo.root().join(recorded.path)).unwrap())
            );
        }
    }
}

#[test]
fn direct_source_or_membership_races_before_journal_never_publish_the_new_entry() {
    for membership in [false, true] {
        let (_dir, repo, issue) = fixture();
        let first = logged(&repo, &issue, 60);
        let mut changed = first.entry.clone();
        changed.seconds = 61;
        if membership {
            changed.id = RecordId::new("TIME").unwrap();
        }
        let result = repo.log_time_with_faults(
            issue.metadata.id.as_str(),
            None,
            &input(30),
            &RequestId::new(),
            |point| {
                if point == FaultPoint::BeforeJournal {
                    write_entry(&repo, &changed);
                }
                Ok(())
            },
        );
        assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
        assert_eq!(
            repo.time_entries(issue.metadata.id.as_str()).unwrap().len(),
            if membership { 2 } else { 1 }
        );
        assert!(repo.pending_operations().unwrap().is_empty());
    }
}

#[test]
fn changed_durable_receipt_result_cannot_be_returned_as_original_time() {
    let (_dir, repo, issue) = fixture();
    let request = RequestId::new();
    let receipt = repo
        .log_time(issue.metadata.id.as_str(), None, &input(60), &request)
        .unwrap();
    let mut forged = receipt.clone();
    forged.result["entry"]["seconds"] = json!(99);
    fs::write(
        repo.root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&forged).unwrap(),
    )
    .unwrap();
    assert_eq!(
        repo.log_time(issue.metadata.id.as_str(), None, &input(60), &request)
            .unwrap_err()
            .code,
        ErrorCode::CorruptStore
    );
    assert!(repo.export_snapshot().is_err());
}

#[test]
fn oversized_and_symlinked_time_sources_are_rejected_without_following() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    fs::write(
        repo.root().join(&first.path),
        vec![b'a'; workdeck_pm::MAX_TIME_ENTRY_BYTES + 1],
    )
    .unwrap();
    assert!(repo.time_entries(issue.metadata.id.as_str()).is_err());
    assert!(!repo.doctor().unwrap().valid);
    #[cfg(unix)]
    {
        fs::remove_file(repo.root().join(&first.path)).unwrap();
        std::os::unix::fs::symlink(repo.root().join(&issue.path), repo.root().join(&first.path))
            .unwrap();
        assert_eq!(
            repo.time_entries(issue.metadata.id.as_str())
                .unwrap_err()
                .code,
            ErrorCode::UnsafePath
        );
    }
}

#[test]
fn missing_cross_repository_and_replaced_source_identity_cannot_log_time() {
    let (_dir, repo, issue) = fixture();
    let before = authority(&repo);
    for reference in [
        RecordId::new("WD").unwrap().to_string(),
        format!(
            "{}::{}",
            workdeck_pm::RepositoryId::new(),
            issue.metadata.id
        ),
    ] {
        assert_eq!(
            repo.log_time(&reference, None, &input(1), &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::NotFound
        );
        assert_eq!(authority(&repo), before);
    }
    let config_path = repo.root().join("config.yml");
    let original = fs::read(&config_path).unwrap();
    let mut config = repo.config().unwrap();
    config.repository = workdeck_pm::RepositoryId::new();
    let replacement = serde_yaml_ng::to_string(&config).unwrap();
    fs::write(&config_path, &replacement).unwrap();
    assert_eq!(
        repo.log_time(
            issue.metadata.id.as_str(),
            None,
            &input(1),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::StaleSource
    );
    assert_eq!(fs::read_to_string(&config_path).unwrap(), replacement);
    assert!(
        !repo
            .root()
            .join(format!("issues/{}/time", issue.metadata.id))
            .exists()
    );
    fs::write(&config_path, original).unwrap();
    assert_eq!(authority(&repo), before);
}

#[test]
fn retired_cycle_remains_historical_attribution_and_archived_issues_can_record_work() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    update(
        &repo,
        &issue,
        BTreeMap::from([("cycle".into(), json!(null))]),
    );
    let target =
        workdeck_pm::RetirementTarget::new(workdeck_pm::RetirementKind::Cycle, "first").unwrap();
    let preview = repo.retirement_preview(&target).unwrap();
    assert!(preview.allowed);
    repo.retire_record(
        &workdeck_pm::RetirementInput {
            target,
            expected: Some(preview.source),
            expected_preview: Some(preview.fingerprint),
        },
        &RequestId::new(),
    )
    .unwrap();
    let amended: TimeEntryRecord = serde_json::from_value(
        repo.amend_time(
            issue.metadata.id.as_str(),
            None,
            &amendment(&first, 90),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    assert_eq!(amended.entry.cycle.as_deref(), Some("first"));
    repo.mutate_issue(
        issue.metadata.id.as_str(),
        None,
        &IssueMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let archived = repo.show_issue(issue.metadata.id.as_str()).unwrap();
    let original = fs::read(repo.root().join(&issue.path)).unwrap();
    let unassigned = logged(&repo, &archived, 30);
    assert_eq!(unassigned.entry.cycle, None);
    let report = repo.time_report(&TimeReportQuery::default()).unwrap();
    assert_eq!(report.by_cycle["first"], 90);
    assert_eq!(report.unassigned_cycle_seconds, 30);
    assert_eq!(fs::read(repo.root().join(&issue.path)).unwrap(), original);
    assert!(repo.doctor().unwrap().valid);
    repo.export_snapshot().unwrap();
}

#[test]
fn time_staging_publishes_only_its_exact_entry_and_operation_receipt() {
    let (dir, repo, issue) = fixture();
    let git = |args: &[&str]| {
        let result = std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap()
    };
    git(&["init", "-q"]);
    fs::write(dir.path().join("unrelated.txt"), "original staged bytes\n").unwrap();
    git(&["add", "--", "unrelated.txt"]);
    fs::write(dir.path().join("unrelated.txt"), "later unstaged bytes\n").unwrap();
    let receipt = repo
        .log_time(
            issue.metadata.id.as_str(),
            None,
            &input(60),
            &RequestId::new(),
        )
        .unwrap();
    let record: TimeEntryRecord = serde_json::from_value(receipt.result.clone()).unwrap();
    repo.stage_operation(&receipt).unwrap();
    let paths = git(&["diff", "--cached", "--name-only"]);
    let mut actual = paths.lines().map(str::to_owned).collect::<Vec<_>>();
    actual.sort();
    let mut expected = vec![
        format!(".workdeck/{}", record.path.display()),
        format!(".workdeck/operations/{}.yml", receipt.operation_id),
        "unrelated.txt".into(),
    ];
    expected.sort();
    assert_eq!(actual, expected);
    assert_eq!(git(&["show", ":unrelated.txt"]), "original staged bytes\n");
    assert_eq!(
        fs::read_to_string(dir.path().join("unrelated.txt")).unwrap(),
        "later unstaged bytes\n"
    );
    assert_eq!(
        ContentHash::of(
            git(&["show", &format!(":.workdeck/{}", record.path.display())]).as_bytes()
        ),
        record.content
    );
    repo.stage_operation(&receipt).unwrap();
    let mut direct = record.entry.clone();
    direct.seconds = 61;
    write_entry(&repo, &direct);
    assert_eq!(
        repo.stage_operation(&receipt).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        ContentHash::of(
            git(&["show", &format!(":.workdeck/{}", record.path.display())]).as_bytes()
        ),
        record.content
    );
}

#[test]
fn changed_time_history_after_journal_blocks_recovery_until_explicit_source_repair() {
    let (_dir, repo, issue) = fixture();
    let first = logged(&repo, &issue, 60);
    let original = fs::read(repo.root().join(&first.path)).unwrap();
    let mut direct = first.entry.clone();
    direct.seconds = 61;
    let request = RequestId::new();
    let result = repo.log_time_with_faults(
        issue.metadata.id.as_str(),
        None,
        &input(30),
        &request,
        |point| {
            if point == FaultPoint::AfterJournal {
                write_entry(&repo, &direct);
            }
            Ok(())
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::RecoveryRequired);
    assert!(repo.recover_operations().is_err());
    let retained: TimeEntry =
        serde_yaml_ng::from_slice(&fs::read(repo.root().join(&first.path)).unwrap()).unwrap();
    assert_eq!(retained.seconds, 61);
    assert_eq!(
        fs::read_dir(repo.root().join(&first.path).parent().unwrap())
            .unwrap()
            .count(),
        1
    );
    fs::write(repo.root().join(&first.path), original).unwrap();
    repo.recover_operations().unwrap();
    repo.log_time(issue.metadata.id.as_str(), None, &input(30), &request)
        .unwrap();
    assert_eq!(
        repo.time_entries(issue.metadata.id.as_str()).unwrap().len(),
        2
    );
    assert_eq!(
        repo.time_report(&TimeReportQuery::default())
            .unwrap()
            .total_seconds,
        90
    );
}

#[test]
fn new_time_cannot_capture_a_retired_cycle_reintroduced_by_a_direct_edit() {
    let (_dir, repo, issue) = fixture();
    logged(&repo, &issue, 60);
    update(
        &repo,
        &issue,
        BTreeMap::from([("cycle".into(), json!(null))]),
    );
    let target =
        workdeck_pm::RetirementTarget::new(workdeck_pm::RetirementKind::Cycle, "first").unwrap();
    repo.retire_record(
        &workdeck_pm::RetirementInput {
            target,
            expected: None,
            expected_preview: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let mut edited = repo.show_issue(issue.metadata.id.as_str()).unwrap();
    edited.metadata.cycle = Some("first".into());
    let content = format!(
        "---\n{}---\n{}",
        serde_yaml_ng::to_string(&edited.metadata).unwrap(),
        edited.body
    );
    fs::write(repo.root().join(&edited.path), &content).unwrap();
    assert_eq!(
        repo.log_time(
            issue.metadata.id.as_str(),
            None,
            &input(30),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        fs::read_to_string(repo.root().join(&edited.path)).unwrap(),
        content
    );
    assert_eq!(
        fs::read_dir(
            repo.root()
                .join(format!("issues/{}/time", issue.metadata.id))
        )
        .unwrap()
        .count(),
        1
    );
}
