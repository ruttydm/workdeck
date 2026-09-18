#![cfg(unix)]
use workdeck_pm::*;
#[path = "support/red_green_fixture.rs"]
mod support;

#[test]
fn retained_pair_replays_original_proof_and_requires_fresh_external_authority() {
    let (root, repo, pair) = support::fixture("broken", "fixed", false);
    let review = support::baseline_review(root.path(), &pair);
    let input = ImportCheckReportRequest {
        envelope: serde_json::to_string(&pair.green).unwrap(),
        policy: pair.producer_policy.clone(),
        expected_policy: pair.expected_producer_policy.clone(),
        expected_commit: pair.candidate.clone(),
        actor: "fixture".into(),
        red_green: Some(RetainedRedGreenProof {
            baseline: pair.baseline.clone(),
            check: pair.check.clone(),
            red: pair.red.clone(),
            red_artifact: pair.red_artifact.clone(),
            green_artifact: pair.green_artifact.clone(),
            review: Some(review.clone()),
        }),
    };
    let before = repo.operation_history().unwrap();
    let mut invalid = input.clone();
    invalid.red_green.as_mut().unwrap().red_artifact.push(' ');
    assert!(
        repo.import_check_report(&invalid, &RequestId::new())
            .is_err()
    );
    invalid = input.clone();
    invalid.red_green.as_mut().unwrap().baseline.contract = ContentHash::of(b"unaccepted contract");
    assert!(
        repo.import_check_report(&invalid, &RequestId::new())
            .is_err()
    );
    assert_eq!(repo.operation_history().unwrap(), before);
    let request = RequestId::new();
    let receipt = repo.import_check_report(&input, &request).unwrap();
    assert_eq!(repo.import_check_report(&input, &request).unwrap(), receipt);
    let record: ImportedCheckReportRecord = serde_json::from_value(receipt.result).unwrap();
    assert_eq!(record.record.input, input);
    let authority = RetainedRedGreenAuthority {
        baseline: pair.baseline.clone(),
        candidate: pair.candidate.clone(),
        policy: pair.producer_policy.clone(),
        expected_policy: pair.expected_producer_policy.clone(),
        review: Some(ReviewCoverageAuthority {
            baseline: review.accepted.clone(),
            policy: review.policy.clone(),
            expected_policy: review.expected_policy.clone(),
        }),
    };
    let admitted = repo
        .reauthenticate_imported_red_green(&record.record.id, &authority)
        .unwrap();
    assert_eq!(admitted.pair.check.id, "unit");
    assert_eq!(admitted.review.unwrap().reviewers, vec!["maintainer"]);
    let mut wrong = authority.clone();
    wrong.expected_policy = ContentHash::of(b"wrong current producer authority");
    assert!(
        repo.reauthenticate_imported_red_green(&record.record.id, &wrong)
            .is_err()
    );
    wrong = authority.clone();
    wrong.review = None;
    assert!(
        repo.reauthenticate_imported_red_green(&record.record.id, &wrong)
            .is_err()
    );
    wrong = authority.clone();
    wrong.baseline.contract = ContentHash::of(b"wrong accepted red contract");
    assert!(
        repo.reauthenticate_imported_red_green(&record.record.id, &wrong)
            .is_err()
    );
    let snapshot = repo.export_snapshot().unwrap();
    snapshot.validate().unwrap();
    let target = tempfile::tempdir().unwrap();
    let destination = target.path().join(".workdeck");
    let preview = preview_snapshot_restore(&destination, &snapshot).unwrap();
    assert!(preview.allowed);
    restore_snapshot(
        &destination,
        &snapshot,
        Some(&preview.fingerprint),
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        Repository::open_source(&destination)
            .unwrap()
            .imported_check_report(&record.record.id)
            .unwrap(),
        record
    );
    // Original proof survives without Git; reads do not renew its imported authority.
    std::fs::rename(root.path().join(".git"), root.path().join("git-offline")).unwrap();
    assert_eq!(
        repo.imported_check_report(&record.record.id).unwrap(),
        record
    );
    assert_eq!(
        repo.import_check_report(&input, &request)
            .unwrap()
            .request_id,
        request
    );
    assert!(
        repo.reauthenticate_imported_red_green(&record.record.id, &authority)
            .is_err()
    );
    // Altering retained artifact bytes must not produce a parseable valid history.
    let path = repo.root().join(&record.path);
    let mut bytes: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    bytes["input"]["red_green"]["green_artifact"] = serde_json::json!("<testsuite name=\"unit\"/>");
    std::fs::write(path, serde_json::to_vec(&bytes).unwrap()).unwrap();
    assert!(repo.imported_check_report(&record.record.id).is_err());
}

#[test]
fn interrupted_pair_imports_recover_once_and_concurrent_retries_share_one_record() {
    use workdeck_pm::transactions::FaultPoint;
    let (root, repo, pair) = support::fixture("broken", "fixed", false);
    let review = support::baseline_review(root.path(), &pair);
    let input = ImportCheckReportRequest {
        envelope: serde_json::to_string(&pair.green).unwrap(),
        policy: pair.producer_policy,
        expected_policy: pair.expected_producer_policy,
        expected_commit: pair.candidate,
        actor: "fixture".into(),
        red_green: Some(RetainedRedGreenProof {
            baseline: pair.baseline,
            check: pair.check,
            red: pair.red,
            red_artifact: pair.red_artifact,
            green_artifact: pair.green_artifact,
            review: Some(review),
        }),
    };
    for point in [
        FaultPoint::BeforeJournal,
        FaultPoint::AfterJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::BeforeReceipt,
        FaultPoint::AfterReceipt,
    ] {
        let request = RequestId::new();
        let mut injected = false;
        let error = repo
            .import_check_report_with_faults(&input, &request, |actual| {
                if actual == point {
                    injected = true;
                    Err(PmError::new(ErrorCode::Io, "interrupted pair import"))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert!(injected, "requested fault was never reached: {point:?}");
        assert_eq!(
            error.code,
            if point == FaultPoint::BeforeJournal {
                ErrorCode::Io
            } else {
                ErrorCode::RecoveryRequired
            }
        );
        assert!(error.message.contains("interrupted pair import"));
        repo.recover_operations().unwrap();
        let receipt = repo.import_check_report(&input, &request).unwrap();
        assert_eq!(repo.import_check_report(&input, &request).unwrap(), receipt);
        assert_eq!(
            repo.imported_check_reports()
                .unwrap()
                .iter()
                .filter(|r| r.record.request_id == request)
                .count(),
            1
        );
    }
    let request = RequestId::new();
    let barrier = std::sync::Barrier::new(2);
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            repo.import_check_report(&input, &request)
        });
        let b = scope.spawn(|| {
            barrier.wait();
            repo.import_check_report(&input, &request)
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    // The bounded writer lock can return Locked while the other caller verifies
    // Git. Only that explicit retryable outcome is accepted before replay.
    assert!(a.is_ok() || b.is_ok());
    let receipt = repo.import_check_report(&input, &request).unwrap();
    for outcome in [a, b] {
        match outcome {
            Ok(original) => assert_eq!(original, receipt),
            Err(error) => assert_eq!(error.code, ErrorCode::Locked),
        }
    }
    assert_eq!(repo.import_check_report(&input, &request).unwrap(), receipt);
    assert_eq!(
        repo.imported_check_reports()
            .unwrap()
            .iter()
            .filter(|r| r.record.request_id == request)
            .count(),
        1
    );
    assert_eq!(
        repo.operation_history()
            .unwrap()
            .iter()
            .filter(|r| r.request_id == request)
            .count(),
        1
    );
    assert!(repo.doctor().unwrap().valid);
}
