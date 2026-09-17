#![cfg(unix)]
use workdeck_pm::*;
#[path = "support/red_green_fixture.rs"]
mod support;

#[test]
fn committed_gate_requires_every_exact_original_proof_and_rejects_post_verification_edits() {
    let (root, repo, pair) = support::fixture_with_gate("broken", "fixed", false, true);
    let review = support::baseline_review(root.path(), &pair);
    let imported: ImportedCheckReportRecord = serde_json::from_value(
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
    let authority = RetainedRedGreenAuthority {
        baseline: pair.baseline,
        candidate: pair.candidate,
        policy: pair.producer_policy,
        expected_policy: pair.expected_producer_policy,
        review: Some(ReviewCoverageAuthority {
            baseline: review.accepted,
            policy: review.policy,
            expected_policy: review.expected_policy,
        }),
    };
    let proof = repo
        .reauthenticate_imported_red_green(&imported.record.id, &authority)
        .unwrap();
    let report = repo
        .reauthenticate_imported_report(
            &imported.record.id,
            &authority.policy,
            &authority.expected_policy,
            &authority.candidate,
        )
        .unwrap();
    let gate = repo.gates().unwrap().remove(0);
    let declaration: DeclareEvidence = serde_json::from_value(serde_json::json!({
        "criterion":gate.definition.requirements[0].criterion,
        "subject":{"repository":repo.identity(),"kind":"source","content":proof.pair.candidate.content},
        "producer":proof.pair.green_producer,"check":proof.pair.check,
        "result":{"id":proof.pair.green_run,"content":report.report.fingerprint},
        "observed_at":report.report.observed_at,"provenance":{"actor":"fixture","reason":"Exact committed gate result"},
        "links":[{"kind":"attestation","id":imported.record.id,"content":imported.content}]
    })).unwrap();
    let evidence: EvidenceRecord = serde_json::from_value(
        repo.declare_evidence(&declaration, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let input = RedGreenGateRequest {
        gate: gate.definition.id.clone(),
        expected_gate: gate.source.clone(),
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
    assert_eq!(gate.definition.requirements[0].check, proof.pair.check);
    assert_eq!(
        gate.definition.requirements[0].producer,
        proof.pair.green_producer
    );
    let history = repo.operation_history().unwrap();
    let verified = repo.verify_red_green_gate(&input).unwrap();
    assert_eq!(
        verified.basis,
        GateVerificationBasis::AuthenticatedRequirements
    );
    assert_eq!(verified.requirements.len(), 2);
    assert_eq!(verified.gate.source, gate.source);
    assert_eq!(repo.operation_history().unwrap(), history);
    let mut missing = input.clone();
    missing.evidence.pop();
    assert!(repo.verify_red_green_gate(&missing).is_err());
    let mut duplicate = input.clone();
    duplicate.evidence.extend(input.evidence.clone());
    assert!(repo.verify_red_green_gate(&duplicate).is_err());
    let mut wrong = input.clone();
    wrong.evidence[0].requirement = "invented".into();
    assert!(repo.verify_red_green_gate(&wrong).is_err());
    wrong = input.clone();
    wrong.expected_gate.content = ContentHash::of(b"wrong gate");
    assert_eq!(
        repo.verify_red_green_gate(&wrong).unwrap_err().code,
        ErrorCode::StaleSource
    );
    let path = repo.root().join(&gate.path);
    let error = repo
        .verify_red_green_gate_with_faults(&input, || {
            std::fs::write(&path, format!("{}\n", gate.document)).unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    std::fs::write(&path, &gate.document).unwrap();
    let evidence_path = repo.root().join(&evidence.path);
    let error = repo
        .verify_red_green_gate_with_faults(&input, || {
            std::fs::write(&evidence_path, format!("{}\n", evidence.document)).unwrap();
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    std::fs::write(&evidence_path, &evidence.document).unwrap();
    let mut inconsistent = input.clone();
    inconsistent.evidence[1].expected_evidence = ContentHash::of(b"conflicting selection pin");
    assert_eq!(
        repo.verify_red_green_gate(&inconsistent).unwrap_err().code,
        ErrorCode::StaleSource
    );
    // A new or weakened gate cannot confer authority on this previously checked commit.
    let changed = repo
        .mutate_gate(
            &input.gate,
            &gate.source,
            &GateMutation::Update {
                fields: std::collections::BTreeMap::from([(
                    "name".into(),
                    serde_json::json!("Changed gate"),
                )]),
            },
            &RequestId::new(),
        )
        .unwrap();
    let changed: GateMutationResult = serde_json::from_value(changed.result).unwrap();
    wrong = input.clone();
    wrong.expected_gate = changed.gate.source;
    assert!(repo.verify_red_green_gate(&wrong).is_err());
}
