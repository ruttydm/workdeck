#![cfg(unix)]

use std::sync::Mutex;

use workdeck_pm::transactions::FaultPoint;
use workdeck_pm::*;

// Both fixtures perform many bounded Git reads and writes. Keep the two
// end-to-end cases from consuming the same short source budget concurrently;
// each case still exercises its own internal recovery/concurrency behavior.
static FIXTURE_LOCK: Mutex<()> = Mutex::new(());

#[allow(dead_code)]
#[path = "support/red_green_fixture.rs"]
mod support;

fn setup() -> (tempfile::TempDir, Repository, CompleteClaimedVerifiedIssue) {
    let (root, repo, pair) = support::green_only_fixture_with_gate("fixed");
    let imported: ImportedCheckReportRecord = serde_json::from_value(
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
    let gate = repo.gates().unwrap().remove(0);
    let authority = CompletionAuthority {
        candidate: pair.candidate.clone(),
        policy: pair.producer_policy.clone(),
        expected_policy: pair.expected_producer_policy.clone(),
        red_green: None,
    };
    let report = repo
        .verify_imported_check(&VerifyImportedCheck {
            attestation: imported.record.id.clone(),
            expected_attestation: imported.content.clone(),
            check: "unit".into(),
            candidate: authority.candidate.clone(),
            policy: authority.policy.clone(),
            expected_policy: authority.expected_policy.clone(),
            red_green: None,
        })
        .unwrap();
    let evidence_declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
        "criterion": gate.definition.requirements[0].criterion,
        "subject": {"repository": repo.identity(), "kind": "source", "content": report.source.content},
        "producer": report.producer,
        "check": report.report.publication.result.result.checks[0].check,
        "result": {"id": report.report.publication.intent.intent.id, "content": report.report.fingerprint},
        "observed_at": report.report.observed_at,
        "provenance": {"actor": "fixture", "reason": "claimed green-only result"},
        "links": [{"kind": "attestation", "id": imported.record.id, "content": imported.content}]
    }))
    .unwrap();
    let evidence: EvidenceRecord = serde_json::from_value(
        repo.declare_evidence(&evidence_declaration, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let gates = vec![CompletionGateSelection {
        gate: gate.definition.id.clone(),
        expected_gate: gate.source.clone(),
        evidence: ["behavior", "also-required"]
            .into_iter()
            .map(|requirement| GateEvidenceSelection {
                requirement: requirement.into(),
                evidence: evidence.reference.id.clone(),
                expected_evidence: evidence.content.clone(),
            })
            .collect(),
    }];
    let verification = CompleteVerifiedIssue {
        issue: issue.metadata.id.clone(),
        expected_issue: issue.source.clone(),
        actor: "worker".into(),
        authority,
        checks: vec![CompletionCheckSelection {
            check: "unit".into(),
            attestation: imported.record.id,
            expected_attestation: imported.content,
        }],
        gates,
    };
    let contract = repo.claim_contract(&issue.metadata.id).unwrap();
    let claim_receipt = repo
        .mutate_claim(
            &ClaimRequest::Acquire {
                input: Box::new(AcquireClaim {
                    actor: "worker".into(),
                    contract: contract.clone(),
                    ttl_seconds: None,
                    recovery: None,
                }),
            },
            &RequestId::new(),
        )
        .unwrap();
    let claim: ClaimChange = serde_json::from_value(claim_receipt.receipt.unwrap().result).unwrap();
    let claim = claim.after;
    let input = CompleteClaimedIssue {
        issue: issue.metadata.id,
        actor: "worker".into(),
        expected_claim: claim.precondition(),
        expected_issue: issue.source,
        contract,
        expected_binding: None,
    };
    (
        root,
        repo,
        CompleteClaimedVerifiedIssue {
            claim: input,
            verification,
        },
    )
}

#[test]
fn claimed_verified_completion_retains_ownership_and_ci_proof_for_replay() {
    let _fixture_guard = FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (root, repo, input) = setup();
    let request = RequestId::new();
    let receipt = repo
        .complete_claimed_verified_issue(&input, &request)
        .unwrap();
    let proof: ClaimedCompletionProof = serde_json::from_value(receipt.result.clone()).unwrap();
    let verification = proof.verification.as_ref().unwrap();
    assert_eq!(proof.claim.metadata.actor, "worker");
    assert_eq!(verification.completion.basis, "authenticated_checks");
    assert!(matches!(
        verification.gates.first(),
        Some(CompletionGateAssessment::GreenOnly(_))
    ));
    let history = repo.operation_history().unwrap().len();
    std::fs::rename(root.path().join(".git"), root.path().join("saved-git")).unwrap();
    assert_eq!(
        repo.complete_claimed_verified_issue(&input, &request)
            .unwrap(),
        receipt
    );
    assert_eq!(repo.operation_history().unwrap().len(), history);
    assert!(repo.doctor().unwrap().valid);
}

#[test]
fn claimed_verified_completion_rejects_claim_or_source_races_before_journal() {
    let _fixture_guard = FIXTURE_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let (root, repo, input) = setup();
    let history = repo.operation_history().unwrap().len();
    let mut wrong = input.clone();
    wrong.verification.actor = "other".into();
    assert_eq!(
        repo.complete_claimed_verified_issue(&wrong, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(repo.operation_history().unwrap().len(), history);
    let path = root.path().join("src/value");
    let error = repo
        .complete_claimed_verified_issue_with_faults(
            &input,
            &RequestId::new(),
            chrono::Utc::now,
            |point| {
                if point == FaultPoint::BeforeJournal {
                    std::fs::write(&path, "changed\n").unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(repo.operation_history().unwrap().len(), history);
    std::fs::write(path, "fixed\n").unwrap();
    assert_ne!(repo.list_issues().unwrap()[0].metadata.status, "done");
}
