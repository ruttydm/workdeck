#![cfg(unix)]
use assert_cmd::prelude::*;
use std::{fs, process::Command};
use workdeck_pm::*;
#[path = "../../workdeck-pm/tests/support/red_green_fixture.rs"]
mod support;

#[test]
fn command_verifies_signed_artifacts_and_fails_when_a_required_case_is_replaced() {
    let (root, repo, request) = support::fixture_with_gate("broken", "fixed", false, true);
    for (name, value) in [
        ("red.json", serde_json::to_vec(&request.red).unwrap()),
        ("green.json", serde_json::to_vec(&request.green).unwrap()),
        (
            "policy.json",
            serde_json::to_vec(&request.producer_policy).unwrap(),
        ),
        ("red.xml", request.red_artifact.as_bytes().to_vec()),
        ("green.xml", request.green_artifact.as_bytes().to_vec()),
    ] {
        fs::write(root.path().join(name), value).unwrap();
    }
    let operations = repo.operation_history().unwrap();
    let args = vec![
        "ci".to_owned(),
        "red-green".into(),
        "--red-report-file".into(),
        "red.json".into(),
        "--green-report-file".into(),
        "green.json".into(),
        "--red-artifact-file".into(),
        "red.xml".into(),
        "--green-artifact-file".into(),
        "green.xml".into(),
        "--policy-file".into(),
        "policy.json".into(),
        "--expected-policy".into(),
        request.expected_producer_policy.to_string(),
        "--expected-base-commit".into(),
        request.baseline.commit.to_string(),
        "--expected-base-contract".into(),
        request.baseline.contract.to_string(),
        "--revision".into(),
        request.candidate.to_string(),
        "--check".into(),
        "unit".into(),
        "--json".into(),
    ];
    let run = || {
        Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root.path())
            .env("XDG_CONFIG_HOME", root.path().join("config"))
            .args(&args)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "stdout:{} stderr:{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["kind"], "ci.red_green");
    assert_eq!(result["result"]["check"]["id"], "unit");
    assert_eq!(result["result"]["cases"][0]["name"], "regression");
    let review = support::baseline_review(root.path(), &request);
    fs::write(
        root.path().join("review.json"),
        serde_json::to_vec(&review.envelope).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join("review-policy.json"),
        serde_json::to_vec(&review.policy).unwrap(),
    )
    .unwrap();
    let partial = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .args(&args)
        .args(["--baseline-review-file", "review.json"])
        .output()
        .unwrap();
    assert!(!partial.status.success());
    assert!(String::from_utf8_lossy(&partial.stderr).contains("--expected-review-policy"));
    let mut reviewed_args = args.clone();
    reviewed_args.extend([
        "--baseline-review-file".into(),
        "review.json".into(),
        "--review-policy-file".into(),
        "review-policy.json".into(),
        "--expected-review-policy".into(),
        review.expected_policy.to_string(),
        "--accepted-commit".into(),
        review.accepted.commit.to_string(),
        "--accepted-contract".into(),
        review.accepted.contract.to_string(),
    ]);
    let reviewed = || {
        Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root.path())
            .args(&reviewed_args)
            .output()
            .unwrap()
    };
    let output = reviewed();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["kind"], "ci.red_green_reviewed");
    assert_eq!(result["result"]["review"]["reviewers"][0], "maintainer");
    assert_eq!(result["result"]["pair"]["check"]["id"], "unit");
    fs::write(root.path().join("review.json"), b"{}").unwrap();
    let rejected = reviewed();
    assert!(!rejected.status.success());
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&rejected.stdout).unwrap()["ok"],
        false
    );
    let retained = RetainedRedGreenProof {
        baseline: request.baseline.clone(),
        check: request.check.clone(),
        red: request.red.clone(),
        red_artifact: request.red_artifact.clone(),
        green_artifact: request.green_artifact.clone(),
        review: Some(review.clone()),
    };
    fs::write(
        root.path().join("pair.json"),
        serde_json::to_vec(&retained).unwrap(),
    )
    .unwrap();
    let request_id = RequestId::new();
    let imported = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .args([
            "ci",
            "import-report",
            "--report-file",
            "green.json",
            "--policy-file",
            "policy.json",
            "--expected-policy",
            request.expected_producer_policy.as_str(),
            "--expected-commit",
            request.candidate.as_str(),
            "--actor",
            "fixture",
            "--request-id",
            request_id.as_str(),
            "--red-green-file",
            "pair.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let imported: serde_json::Value = serde_json::from_slice(&imported.stdout).unwrap();
    let id = imported["result"]["record"]["id"].as_str().unwrap();
    let authority = RetainedRedGreenAuthority {
        baseline: request.baseline.clone(),
        candidate: request.candidate.clone(),
        policy: request.producer_policy.clone(),
        expected_policy: request.expected_producer_policy.clone(),
        review: Some(ReviewCoverageAuthority {
            baseline: review.accepted,
            policy: review.policy,
            expected_policy: review.expected_policy,
        }),
    };
    fs::write(
        root.path().join("authority.json"),
        serde_json::to_vec(&authority).unwrap(),
    )
    .unwrap();
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .args([
            "ci",
            "reauthenticate-red-green",
            id,
            "--authority-file",
            "authority.json",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["result"]["pair"]["check"]["id"], "unit");
    assert_eq!(
        repo.operation_history().unwrap().len(),
        operations.len() + 1
    );
    let pair_proof: RetainedRedGreenAssessment =
        serde_json::from_value(result["result"].clone()).unwrap();
    let imported_record = repo.imported_check_report(&id.parse().unwrap()).unwrap();
    let original_report = repo
        .reauthenticate_imported_report(
            &id.parse().unwrap(),
            &authority.policy,
            &authority.expected_policy,
            &authority.candidate,
        )
        .unwrap();
    let issue = repo.list_issues().unwrap().remove(0);
    let criterion = repo
        .resolve_criterion(&CriterionOwner::Issue(issue.metadata.id), "works")
        .unwrap()
        .reference;
    let declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
        "criterion":criterion,"subject":{"repository":repo.identity(),"kind":"source","content":pair_proof.pair.candidate.content},
        "producer":pair_proof.pair.green_producer,"check":pair_proof.pair.check,
        "result":{"id":pair_proof.pair.green_run,"content":original_report.report.fingerprint},
        "observed_at":original_report.report.observed_at,"provenance":{"actor":"fixture","reason":"Criterion check link"},
        "links":[{"kind":"attestation","id":imported_record.record.id,"content":imported_record.content}]
    })).unwrap();
    let evidence: EvidenceRecord = serde_json::from_value(
        repo.declare_evidence(&declaration, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let evidence_args = [
        "evidence",
        "verify-red-green",
        evidence.reference.id.as_str(),
        "--expected-evidence-content",
        evidence.content.as_str(),
        "--authority-file",
        "authority.json",
        "--json",
    ];
    let output = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .args(evidence_args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["result"]["basis"], "authenticated_check_link");
    assert_eq!(result["result"]["criterion"]["reference"]["id"], "works");
    let gate = repo.gates().unwrap().remove(0);
    let mut gate_request = RedGreenGateRequest {
        gate: gate.definition.id,
        expected_gate: gate.source,
        authority,
        evidence: ["behavior", "also-required"]
            .into_iter()
            .map(|requirement| GateEvidenceSelection {
                requirement: requirement.into(),
                evidence: evidence.reference.id.clone(),
                expected_evidence: evidence.content.clone(),
            })
            .collect(),
    };
    let operations = repo.operation_history().unwrap();
    fs::write(
        root.path().join("gate-proof.json"),
        serde_json::to_vec(&gate_request).unwrap(),
    )
    .unwrap();
    let gate_run = || {
        Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root.path())
            .args(["gate", "verify-red-green", "gate-proof.json", "--json"])
            .output()
            .unwrap()
    };
    let output = gate_run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["result"]["basis"], "authenticated_requirements");
    assert_eq!(
        result["result"]["requirements"].as_array().unwrap().len(),
        2
    );
    gate_request.evidence.pop();
    fs::write(
        root.path().join("gate-proof.json"),
        serde_json::to_vec(&gate_request).unwrap(),
    )
    .unwrap();
    assert!(!gate_run().status.success());
    assert_eq!(repo.operation_history().unwrap(), operations);
    let issue = repo.list_issues().unwrap().remove(0);
    let completion = CompleteRedGreenIssue {
        issue: issue.metadata.id.clone(),
        expected_issue: issue.source,
        actor: "fixture".into(),
        authority: gate_request.authority.clone(),
        checks: vec![CompletionCheckSelection {
            check: "unit".into(),
            attestation: imported_record.record.id.clone(),
            expected_attestation: imported_record.content.clone(),
        }],
        gates: vec![CompletionGateSelection {
            gate: gate_request.gate.clone(),
            expected_gate: gate_request.expected_gate.clone(),
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
    fs::write(
        root.path().join("completion.json"),
        serde_json::to_vec(&completion).unwrap(),
    )
    .unwrap();
    let preview = Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root.path())
        .args([
            "issue",
            "done",
            issue.metadata.id.as_str(),
            "--verification-file",
            "completion.json",
            "--dry-run",
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    let preview: serde_json::Value = serde_json::from_slice(&preview.stdout).unwrap();
    assert_eq!(preview["result"]["allowed"], true);
    assert_eq!(repo.operation_history().unwrap(), operations);
    let completion_request = RequestId::new();
    let complete = || {
        Command::cargo_bin("workdeck")
            .unwrap()
            .current_dir(root.path())
            .args([
                "issue",
                "--request-id",
                completion_request.as_str(),
                "done",
                issue.metadata.id.as_str(),
                "--verification-file",
                "completion.json",
                "--json",
            ])
            .output()
            .unwrap()
    };
    let completed = complete();
    assert!(
        completed.status.success(),
        "{}",
        String::from_utf8_lossy(&completed.stderr)
    );
    let completed: serde_json::Value = serde_json::from_slice(&completed.stdout).unwrap();
    assert_eq!(completed["result"]["after"]["metadata"]["status"], "done");
    let replay = complete();
    assert!(replay.status.success());
    let replay: serde_json::Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(replay["result"], completed["result"]);
    let operations = repo.operation_history().unwrap();
    // Artifact data cannot be substituted even if the replacement would pass.
    fs::write(
        root.path().join("green.xml"),
        request.green_artifact.replace("regression", "replacement"),
    )
    .unwrap();
    let output = run();
    assert!(!output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["ok"], false);
    assert_eq!(repo.operation_history().unwrap(), operations);
    // Older check definitions have no implicit red/green requirement.
    let old: CheckDefinition = serde_json::from_value(serde_json::json!({"schema":1,"repository":repo.identity(),"id":"old","name":"Old","command":"unit","expectation":{"kind":"process","allowed_exit_codes":[0]}})).unwrap();
    assert!(old.red_green.is_none());
    assert!(
        serde_json::to_value(old)
            .unwrap()
            .get("red_green")
            .is_none()
    );
}
