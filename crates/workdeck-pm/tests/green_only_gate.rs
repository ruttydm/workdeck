#![cfg(unix)]
use workdeck_pm::*;
#[allow(dead_code)]
#[path = "support/red_green_fixture.rs"]
mod support;

fn setup() -> (
    tempfile::TempDir,
    Repository,
    CompleteVerifiedIssue,
    VerifiedGateRequest,
    EvidenceRecord,
) {
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
    let report = repo
        .verify_imported_check(&VerifyImportedCheck {
            attestation: imported.record.id.clone(),
            expected_attestation: imported.content.clone(),
            check: "unit".into(),
            candidate: pair.candidate.clone(),
            policy: pair.producer_policy.clone(),
            expected_policy: pair.expected_producer_policy.clone(),
            red_green: None,
        })
        .unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let gate = repo.gates().unwrap().remove(0);
    let declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
        "criterion": gate.definition.requirements[0].criterion,
        "subject": {"repository":repo.identity(),"kind":"source","content":report.source.content},
        "producer": report.producer,
        "check": report.report.publication.result.result.checks[0].check,
        "result": {"id":report.report.publication.intent.intent.id,"content":report.report.fingerprint},
        "observed_at": report.report.observed_at,
        "provenance": {"actor":"fixture","reason":"Green-only gate result"},
        "links": [{"kind":"attestation","id":imported.record.id,"content":imported.content}]
    })).unwrap();
    let evidence: EvidenceRecord = serde_json::from_value(
        repo.declare_evidence(&declaration, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let authority = CompletionAuthority {
        candidate: pair.candidate,
        policy: pair.producer_policy,
        expected_policy: pair.expected_producer_policy,
        red_green: None,
    };
    let gate_input = VerifiedGateRequest {
        gate: gate.definition.id.clone(),
        expected_gate: gate.source.clone(),
        authority: authority.clone(),
        evidence: ["behavior", "also-required"]
            .into_iter()
            .map(|requirement| GateEvidenceSelection {
                requirement: requirement.into(),
                evidence: evidence.reference.id.clone(),
                expected_evidence: evidence.content.clone(),
            })
            .collect(),
    };
    let input = CompleteVerifiedIssue {
        issue: issue.metadata.id,
        expected_issue: issue.source,
        actor: "fixture".into(),
        authority,
        checks: vec![CompletionCheckSelection {
            check: "unit".into(),
            attestation: imported.record.id,
            expected_attestation: imported.content,
        }],
        gates: vec![CompletionGateSelection {
            gate: gate_input.gate.clone(),
            expected_gate: gate_input.expected_gate.clone(),
            evidence: gate_input.evidence.clone(),
        }],
    };
    (root, repo, input, gate_input, evidence)
}

#[test]
fn green_only_gate_verifies_read_only_and_completes_with_historical_proof() {
    let (root, repo, input, gate_input, evidence) = setup();
    let history = repo.operation_history().unwrap();
    let assessment = repo.verify_verified_gate(&gate_input).unwrap();
    assert_eq!(
        assessment.basis,
        GateVerificationBasis::AuthenticatedGreenRequirements
    );
    assert_eq!(assessment.requirements.len(), 2);
    assert_eq!(repo.operation_history().unwrap(), history);

    let report = repo.verified_completion_report(&input).unwrap();
    assert!(report.allowed);
    let request = RequestId::new();
    let receipt = repo.complete_verified_issue(&input, &request).unwrap();
    let proof: VerifiedIssueCompletion = serde_json::from_value(receipt.result.clone()).unwrap();
    assert!(matches!(
        proof.gates.first(),
        Some(CompletionGateAssessment::GreenOnly(_))
    ));
    assert_eq!(proof.evidence[0].reference.id, evidence.reference.id);
    assert_eq!(
        repo.complete_verified_issue(&input, &request).unwrap(),
        receipt
    );
    std::fs::rename(root.path().join(".git"), root.path().join("saved-git")).unwrap();
    assert_eq!(
        repo.complete_verified_issue(&input, &request).unwrap(),
        receipt
    );
}

#[test]
fn green_only_gate_rejects_missing_duplicate_and_changed_evidence() {
    let (_root, repo, _input, gate_input, evidence) = setup();
    let mut missing = gate_input.clone();
    missing.evidence.pop();
    assert_eq!(
        repo.verify_verified_gate(&missing).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
    let mut duplicate = gate_input.clone();
    duplicate.evidence.push(duplicate.evidence[0].clone());
    assert_eq!(
        repo.verify_verified_gate(&duplicate).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
    let path = repo.root().join(&evidence.path);
    let original = std::fs::read(&path).unwrap();
    let error = repo
        .verify_verified_gate_with_faults(&gate_input, || {
            std::fs::write(&path, format!("{}\n", evidence.document)).unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    std::fs::write(path, original).unwrap();
}
