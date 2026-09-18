#![cfg(unix)]
use workdeck_pm::transactions::FaultPoint;
use workdeck_pm::*;
#[path = "support/red_green_fixture.rs"]
mod support;

fn setup() -> (
    tempfile::TempDir,
    Repository,
    IssueRecord,
    CompleteRedGreenIssue,
) {
    setup_with_gate(false)
}
fn setup_with_gate(
    gate: bool,
) -> (
    tempfile::TempDir,
    Repository,
    IssueRecord,
    CompleteRedGreenIssue,
) {
    let (root, repo, pair) = support::fixture_with_gate("broken", "fixed", false, gate);
    let review = support::baseline_review(root.path(), &pair);
    let record: ImportedCheckReportRecord = serde_json::from_value(
        repo.import_check_report(
            &ImportCheckReportRequest {
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
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let mut input = CompleteRedGreenIssue {
        issue: issue.metadata.id.clone(),
        expected_issue: issue.source.clone(),
        actor: "fixture".into(),
        authority: RetainedRedGreenAuthority {
            baseline: pair.baseline,
            candidate: pair.candidate,
            policy: pair.producer_policy,
            expected_policy: pair.expected_producer_policy,
            review: Some(ReviewCoverageAuthority {
                baseline: review.accepted,
                policy: review.policy,
                expected_policy: review.expected_policy,
            }),
        },
        checks: vec![CompletionCheckSelection {
            check: "unit".into(),
            attestation: record.record.id,
            expected_attestation: record.content,
        }],
        gates: vec![],
    };
    if gate {
        let gate = repo.gates().unwrap().remove(0);
        let record = repo
            .imported_check_report(&input.checks[0].attestation)
            .unwrap();
        let report = repo
            .reauthenticate_imported_report(
                &record.record.id,
                &input.authority.policy,
                &input.authority.expected_policy,
                &input.authority.candidate,
            )
            .unwrap();
        let declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
            "criterion":gate.definition.requirements[0].criterion,
            "subject":{"repository":repo.identity(),"kind":"source","content":report.source.content},
            "producer":report.producer,"check":report.report.publication.result.result.checks[0].check,
            "result":{"id":report.report.publication.intent.intent.id,"content":report.report.fingerprint},
            "observed_at":report.report.observed_at,"provenance":{"actor":"fixture","reason":"Completion criterion link"},
            "links":[{"kind":"attestation","id":record.record.id,"content":record.content}]
        })).unwrap();
        let evidence: EvidenceRecord = serde_json::from_value(
            repo.declare_evidence(&declaration, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        input.gates.push(CompletionGateSelection {
            gate: gate.definition.id,
            expected_gate: gate.source,
            evidence: ["behavior", "also-required"]
                .into_iter()
                .map(|requirement| GateEvidenceSelection {
                    requirement: requirement.into(),
                    evidence: evidence.reference.id.clone(),
                    expected_evidence: evidence.content.clone(),
                })
                .collect(),
        });
    }
    (root, repo, issue, input)
}
#[test]
fn original_proof_completes_the_exact_current_issue_and_replays_without_requalification() {
    let (root, repo, issue, input) = setup();
    assert!(
        !repo
            .completion_report(issue.metadata.id.as_str())
            .unwrap()
            .allowed
    );
    let request = RequestId::new();
    let before = repo.operation_history().unwrap().len();
    let mut missing = input.clone();
    missing.checks.clear();
    assert!(
        repo.complete_red_green_issue(&missing, &RequestId::new())
            .is_err()
    );
    let mut wrong = input.clone();
    wrong.checks[0].expected_attestation = ContentHash::of(b"wrong original proof");
    assert_eq!(
        repo.complete_red_green_issue(&wrong, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let source = root.path().join("src/value");
    std::fs::write(&source, "dirty\n").unwrap();
    assert!(
        repo.complete_red_green_issue(&input, &RequestId::new())
            .is_err()
    );
    std::fs::write(&source, "fixed\n").unwrap();
    let error = repo
        .complete_red_green_issue_with_faults(
            &input,
            &RequestId::new(),
            || {
                std::fs::write(&source, "changed after preflight\n").unwrap();
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    std::fs::write(&source, "fixed\n").unwrap();
    let error = repo
        .complete_red_green_issue_with_faults(
            &input,
            &RequestId::new(),
            || Ok(()),
            |point| {
                if point == FaultPoint::BeforeJournal {
                    std::fs::write(&source, "changed before journal\n").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    std::fs::write(&source, "fixed\n").unwrap();
    assert_eq!(repo.operation_history().unwrap().len(), before);
    assert_ne!(repo.list_issues().unwrap()[0].metadata.status, "done");
    let receipt = repo.complete_red_green_issue(&input, &request).unwrap();
    let proof: RedGreenIssueCompletion = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(proof.after.metadata.status, "done");
    assert_eq!(proof.before.source, issue.source);
    assert_eq!(proof.completion.basis, "authenticated_checks");
    assert_eq!(repo.operation_history().unwrap().len(), before + 1);
    // Exact replay retrieves historical publication even after Git is unavailable.
    std::fs::rename(root.path().join(".git"), root.path().join("saved-git")).unwrap();
    assert_eq!(
        repo.complete_red_green_issue(&input, &request).unwrap(),
        receipt
    );
    assert_eq!(repo.operation_history().unwrap().len(), before + 1);
    let path = repo
        .root()
        .join(format!("operations/{}.yml", receipt.operation_id));
    let original = std::fs::read(&path).unwrap();
    for field in ["issue", "artifact"] {
        let mut forged = serde_json::to_value(&receipt).unwrap();
        if field == "issue" {
            forged["result"]["after"]["metadata"]["title"] = serde_json::json!("Forged title");
        } else {
            forged["result"]["attestations"][0]["record"]["input"]["red_green"]["green_artifact"] =
                serde_json::json!("Forged artifact");
        }
        std::fs::write(&path, serde_json::to_vec(&forged).unwrap()).unwrap();
        assert!(repo.operation_history().is_err(), "{field}");
        assert!(!repo.doctor().unwrap().valid, "{field}");
        assert!(repo.export_snapshot().is_err(), "{field}");
        std::fs::write(&path, &original).unwrap();
    }
    let mut conflict = input.clone();
    conflict.actor = "different-actor".into();
    assert_eq!(
        repo.complete_red_green_issue(&conflict, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
}

#[test]
fn interrupted_verified_completion_recovers_one_original_transition() {
    for boundary in [
        FaultPoint::AfterJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::BeforeReceipt,
        FaultPoint::AfterReceipt,
    ] {
        let (_root, repo, issue, input) = setup();
        let request = RequestId::new();
        let before = repo.operation_history().unwrap().len();
        let mut reached = false;
        let error = repo
            .complete_red_green_issue_with_faults(
                &input,
                &request,
                || Ok(()),
                |point| {
                    if point == boundary {
                        reached = true;
                        return Err(PmError::new(
                            ErrorCode::Io,
                            "interrupted verified completion",
                        ));
                    }
                    Ok(())
                },
            )
            .unwrap_err();
        assert!(reached, "{boundary:?}: {error:?}");
        assert_eq!(error.code, ErrorCode::RecoveryRequired);
        repo.recover_operations().unwrap();
        let original = repo.complete_red_green_issue(&input, &request).unwrap();
        assert_eq!(
            repo.complete_red_green_issue(&input, &request).unwrap(),
            original
        );
        assert_eq!(repo.operation_history().unwrap().len(), before + 1);
        assert_eq!(
            repo.show_issue(issue.metadata.id.as_str())
                .unwrap()
                .metadata
                .status,
            "done"
        );
        repo.export_snapshot().unwrap();
    }
}

#[test]
fn concurrent_verified_completion_retries_share_the_original_receipt() {
    let (_root, repo, _issue, input) = setup();
    let request = RequestId::new();
    let before = repo.operation_history().unwrap().len();
    let barrier = std::sync::Barrier::new(2);
    let (a, b) = std::thread::scope(|scope| {
        let a = scope.spawn(|| {
            barrier.wait();
            repo.complete_red_green_issue(&input, &request)
        });
        let b = scope.spawn(|| {
            barrier.wait();
            repo.complete_red_green_issue(&input, &request)
        });
        (a.join().unwrap(), b.join().unwrap())
    });
    assert!(a.is_ok() || b.is_ok(), "{a:?}; {b:?}");
    let receipt = repo.complete_red_green_issue(&input, &request).unwrap();
    for outcome in [a, b] {
        match outcome {
            Ok(original) => assert_eq!(original, receipt),
            Err(error) => assert_eq!(error.code, ErrorCode::Locked),
        }
    }
    assert_eq!(repo.operation_history().unwrap().len(), before + 1);
}

#[test]
fn replacing_git_directory_after_preflight_cannot_publish_completion() {
    fn copy_tree(from: &std::path::Path, to: &std::path::Path) {
        std::fs::create_dir(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                copy_tree(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).unwrap();
            }
        }
    }
    let (root, repo, issue, input) = setup();
    let git = root.path().join(".git");
    let replacement = root.path().join("replacement-git");
    let saved = root.path().join("original-git");
    copy_tree(&git, &replacement);
    let before = repo.operation_history().unwrap();
    let error = repo
        .complete_red_green_issue_with_faults(
            &input,
            &RequestId::new(),
            || {
                std::fs::rename(&git, &saved).unwrap();
                std::fs::rename(&replacement, &git).unwrap();
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(repo.operation_history().unwrap(), before);
    assert_eq!(repo.show_issue(issue.metadata.id.as_str()).unwrap(), issue);
}

#[test]
fn attached_gate_completion_requires_active_evidence_through_the_transaction() {
    let (_root, repo, issue, mut input) = setup_with_gate(true);
    let mut missing = input.clone();
    missing.gates.clear();
    assert!(
        repo.complete_red_green_issue(&missing, &RequestId::new())
            .is_err()
    );
    let evidence = repo.evidence(&input.gates[0].evidence[0].evidence).unwrap();
    let mut replacement = None;
    let error = repo
        .complete_red_green_issue_with_faults(
            &input,
            &RequestId::new(),
            || {
                let mut declaration = evidence.reference.declaration.clone();
                declaration.supersedes = Some(EvidenceSupersession {
                    id: evidence.reference.id.clone(),
                    content: evidence.content.clone(),
                    reason: "Corrected citation".into(),
                });
                replacement = Some(
                    serde_json::from_value::<EvidenceRecord>(
                        repo.declare_evidence(&declaration, &RequestId::new())
                            .unwrap()
                            .result,
                    )
                    .unwrap(),
                );
                Ok(())
            },
            |_| Ok(()),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(repo.show_issue(issue.metadata.id.as_str()).unwrap(), issue);
    let replacement = replacement.unwrap();
    for selected in &mut input.gates[0].evidence {
        selected.evidence = replacement.reference.id.clone();
        selected.expected_evidence = replacement.content.clone();
    }
    let receipt = repo
        .complete_red_green_issue(&input, &RequestId::new())
        .unwrap();
    let proof: RedGreenIssueCompletion = serde_json::from_value(receipt.result).unwrap();
    assert_eq!(proof.after.metadata.status, "done");
    assert_eq!(proof.gates.len(), 1);
    assert!(repo.doctor().unwrap().valid);
    repo.export_snapshot().unwrap();
}

#[test]
fn general_completion_preserves_mandatory_pair_and_attached_gate_authority() {
    let (_root, repo, _issue, legacy) = setup_with_gate(true);
    let input = CompleteVerifiedIssue::from(&legacy);
    let mut missing = input.clone();
    missing.authority.red_green = None;
    assert!(
        repo.complete_verified_issue(&missing, &RequestId::new())
            .is_err()
    );
    let request = RequestId::new();
    let receipt = repo.complete_verified_issue(&input, &request).unwrap();
    let result: VerifiedIssueCompletion = serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(result.gates.len(), 1);
    assert_eq!(result.after.metadata.status, "done");
    assert_eq!(
        repo.complete_verified_issue(&input, &request).unwrap(),
        receipt
    );
    assert!(repo.doctor().unwrap().valid);
    repo.export_snapshot().unwrap();
}
