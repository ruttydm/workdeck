#![cfg(unix)]
use workdeck_pm::transactions::FaultPoint;
use workdeck_pm::*;
#[allow(dead_code)]
#[path = "support/red_green_fixture.rs"]
mod support;
fn setup(mode: &str) -> (tempfile::TempDir, Repository, CompleteVerifiedIssue) {
    let (root, repo, pair) = support::green_only_fixture(mode);
    let record: ImportedCheckReportRecord = serde_json::from_value(
        repo.import_check_report(
            &ImportCheckReportRequest {
                envelope: serde_json::to_string(&pair.green).unwrap(),
                policy: pair.producer_policy.clone(),
                expected_policy: pair.expected_producer_policy.clone(),
                expected_commit: pair.candidate.clone(),
                actor: "fixture".into(),
                red_green: None,
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let input = CompleteVerifiedIssue {
        issue: issue.metadata.id,
        expected_issue: issue.source,
        actor: "fixture".into(),
        authority: CompletionAuthority {
            candidate: pair.candidate,
            policy: pair.producer_policy,
            expected_policy: pair.expected_producer_policy,
            red_green: None,
        },
        checks: vec![CompletionCheckSelection {
            check: "unit".into(),
            attestation: record.record.id,
            expected_attestation: record.content,
        }],
        gates: vec![],
    };
    (root, repo, input)
}
#[test]
fn green_only_completion_retains_original_proof_and_replays_without_git() {
    let (root, repo, input) = setup("fixed");
    let before = repo.operation_history().unwrap().len();
    assert!(repo.verified_completion_report(&input).unwrap().allowed);
    assert_eq!(repo.operation_history().unwrap().len(), before);
    assert!(
        !repo
            .completion_report(input.issue.as_str())
            .unwrap()
            .allowed
    );
    for duplicate in [false, true] {
        let mut incomplete = input.clone();
        if duplicate {
            incomplete.checks.push(incomplete.checks[0].clone());
        } else {
            incomplete.checks.clear();
        }
        assert!(
            repo.complete_verified_issue(&incomplete, &RequestId::new())
                .is_err()
        );
    }
    let request = RequestId::new();
    let receipt = repo.complete_verified_issue(&input, &request).unwrap();
    let result: VerifiedIssueCompletion = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(result.after.metadata.status, "done");
    assert_eq!(result.completion.basis, "authenticated_checks");
    assert!(result.attestations[0].record.input.red_green.is_none());
    std::fs::rename(root.path().join(".git"), root.path().join("saved-git")).unwrap();
    assert_eq!(
        repo.complete_verified_issue(&input, &request).unwrap(),
        receipt
    );
    assert_eq!(repo.operation_history().unwrap().len(), before + 1);
    let path = repo
        .root()
        .join(format!("operations/{}.yml", receipt.operation_id));
    let bytes = std::fs::read(&path).unwrap();
    for field in ["title", "signature"] {
        let mut forged = serde_json::to_value(&receipt).unwrap();
        if field == "title" {
            forged["result"]["after"]["metadata"]["title"] = serde_json::json!("Forged");
        } else {
            forged["result"]["attestations"][0]["record"]["input"]["envelope"] =
                serde_json::json!("{}");
        }
        std::fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(repo.operation_history().is_err());
        assert!(!repo.doctor().unwrap().valid);
        assert!(repo.export_snapshot().is_err());
        std::fs::write(&path, &bytes).unwrap();
    }
    let mut conflict = input.clone();
    conflict.actor = "someone-else".into();
    assert_eq!(
        repo.complete_verified_issue(&conflict, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
}
#[test]
fn failed_signed_check_cannot_complete_issue() {
    let (_root, repo, input) = setup("broken");
    assert_eq!(
        repo.complete_verified_issue(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_ne!(repo.list_issues().unwrap()[0].metadata.status, "done");
}

#[test]
fn source_changes_cannot_publish_and_interruption_recovers_original_proof() {
    let (root, repo, input) = setup("fixed");
    let before = repo.operation_history().unwrap().len();
    for preflight in [true, false] {
        let error = repo
            .complete_verified_issue_with_faults(
                &input,
                &RequestId::new(),
                || {
                    if preflight {
                        std::fs::write(root.path().join("src/value"), "changed\n").unwrap();
                    }
                    Ok(())
                },
                |point| {
                    if !preflight && point == FaultPoint::BeforeJournal {
                        std::fs::write(root.path().join("src/value"), "changed\n").unwrap();
                    }
                    Ok(())
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource);
        std::fs::write(root.path().join("src/value"), "fixed\n").unwrap();
        assert_eq!(repo.operation_history().unwrap().len(), before);
        assert_ne!(repo.list_issues().unwrap()[0].metadata.status, "done");
    }
    let request = RequestId::new();
    let mut reached = false;
    let error = repo
        .complete_verified_issue_with_faults(
            &input,
            &request,
            || Ok(()),
            |point| {
                if point == FaultPoint::AfterJournal {
                    reached = true;
                    return Err(PmError::new(ErrorCode::Io, "interrupted"));
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert!(reached);
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    repo.recover_operations().unwrap();
    let receipt = repo.complete_verified_issue(&input, &request).unwrap();
    assert_eq!(
        repo.complete_verified_issue(&input, &request).unwrap(),
        receipt
    );
    assert_eq!(repo.operation_history().unwrap().len(), before + 1);
    assert!(repo.doctor().unwrap().valid);
    repo.export_snapshot().unwrap();
}
