#![cfg(unix)]
use assert_cmd::prelude::*;
use std::{fs, process::Command};
use workdeck_pm::*;
#[allow(dead_code)]
#[path = "../../workdeck-pm/tests/support/red_green_fixture.rs"]
mod support;

#[test]
fn cli_verifies_green_only_gate_without_writing_history() {
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
    let request = VerifiedGateRequest {
        gate: gate.definition.id,
        expected_gate: gate.source,
        authority: CompletionAuthority {
            candidate: pair.candidate,
            policy: pair.producer_policy,
            expected_policy: pair.expected_producer_policy,
            red_green: None,
        },
        evidence: ["behavior", "also-required"]
            .into_iter()
            .map(|requirement| GateEvidenceSelection {
                requirement: requirement.into(),
                evidence: evidence.reference.id.clone(),
                expected_evidence: evidence.content.clone(),
            })
            .collect(),
    };
    fs::write(
        root.path().join("gate.json"),
        serde_json::to_vec(&request).unwrap(),
    )
    .unwrap();
    let history = repo.operation_history().unwrap();
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .args(["gate", "verify-green", "gate.json", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status={:?} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["kind"], "gate.verified");
    assert_eq!(value["result"]["basis"], "authenticated_green_requirements");
    assert_eq!(repo.operation_history().unwrap(), history);
}

#[test]
fn cli_completes_a_current_claim_with_green_only_proof() {
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
    let declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
        "criterion": gate.definition.requirements[0].criterion,
        "subject": {"repository":repo.identity(),"kind":"source","content":report.source.content},
        "producer": report.producer,
        "check": report.report.publication.result.result.checks[0].check,
        "result": {"id":report.report.publication.intent.intent.id,"content":report.report.fingerprint},
        "observed_at": report.report.observed_at,
        "provenance": {"actor":"fixture","reason":"claimed green-only result"},
        "links": [{"kind":"attestation","id":imported.record.id,"content":imported.content}]
    }))
    .unwrap();
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
        gates: vec![CompletionGateSelection {
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
        }],
    };
    let contract = repo.claim_contract(&issue.metadata.id).unwrap();
    repo.save_claim_contract(&contract).unwrap();
    let contract_hash = contract.fingerprint().unwrap().to_string();
    let claim: ClaimChange = serde_json::from_value(
        repo.mutate_local_claim(
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
        .unwrap()
        .result,
    )
    .unwrap();
    let claim = claim.after;
    fs::write(
        root.path().join("verification.json"),
        serde_json::to_vec(&verification).unwrap(),
    )
    .unwrap();
    let request = RequestId::new().to_string();
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .args([
            "claim",
            "complete",
            issue.metadata.id.as_str(),
            "--token",
            claim.metadata.token.as_str(),
            "--generation",
            &claim.metadata.generation.to_string(),
            "--expected-content",
            claim.source.content.as_str(),
            "--expected-issue-revision",
            &issue.metadata.revision.get().to_string(),
            "--expected-issue-content",
            issue.source.content.as_str(),
            "--contract",
            &contract_hash,
            "--expected-contract",
            &contract_hash,
            "--actor",
            "worker",
            "--request-id",
            &request,
            "--verification-file",
            "verification.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "status={:?} stdout={} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["kind"], "claimed_verified_completion_receipt");
    assert_eq!(
        value["result"]["result"]["verification"]["completion"]["basis"],
        "authenticated_checks"
    );
    assert_eq!(
        value["result"]["result"]["after"]["metadata"]["status"],
        "done"
    );
    assert!(repo.doctor().unwrap().valid);
}
