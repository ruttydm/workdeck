#![cfg(unix)]
use workdeck_pm::*;
#[path = "support/red_green_fixture.rs"]
mod support;
fn import(repo: &Repository, pair: &RedGreenRequest) -> VerifyImportedCheck {
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
    VerifyImportedCheck {
        attestation: record.record.id,
        expected_attestation: record.content,
        check: "unit".into(),
        candidate: pair.candidate.clone(),
        policy: pair.producer_policy.clone(),
        expected_policy: pair.expected_producer_policy.clone(),
        red_green: None,
    }
}
#[test]
fn green_only_success_is_source_bound_and_read_only() {
    let (root, repo, pair) = support::green_only_fixture("fixed");
    let input = import(&repo, &pair);
    let before = repo.operation_history().unwrap();
    let verified = repo.verify_imported_check(&input).unwrap();
    assert_eq!(verified.source.commit, pair.candidate);
    assert_eq!(repo.operation_history().unwrap(), before);
    let mut wrong = input.clone();
    wrong.expected_attestation = ContentHash::of(b"wrong");
    assert_eq!(
        repo.verify_imported_check(&wrong).unwrap_err().code,
        ErrorCode::StaleSource
    );
    let mut wrong = input.clone();
    wrong.expected_policy = ContentHash::of(b"wrong");
    assert!(repo.verify_imported_check(&wrong).is_err());
    std::fs::write(root.path().join("src/value"), "dirty\n").unwrap();
    assert!(repo.verify_imported_check(&input).is_err());
}
#[test]
fn authentic_failure_is_not_success() {
    let (_root, repo, pair) = support::green_only_fixture("broken");
    let input = import(&repo, &pair);
    assert_eq!(
        repo.verify_imported_check(&input).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
}
#[test]
fn mandatory_red_green_cannot_be_replaced_by_signed_green_only() {
    let (_root, repo, pair) = support::fixture("broken", "fixed", false);
    let input = import(&repo, &pair);
    assert_eq!(
        repo.verify_imported_check(&input).unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
}

#[test]
fn required_red_green_uses_original_proof_and_current_authority() {
    let (root, repo, pair) = support::fixture("broken", "fixed", false);
    let review = support::baseline_review(root.path(), &pair);
    let mut input = import(&repo, &pair);
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
    input.attestation = record.record.id;
    input.expected_attestation = record.content;
    input.red_green = Some(RetainedRedGreenAuthority {
        baseline: pair.baseline,
        candidate: pair.candidate,
        policy: pair.producer_policy,
        expected_policy: pair.expected_producer_policy,
        review: Some(ReviewCoverageAuthority {
            baseline: review.accepted,
            policy: review.policy,
            expected_policy: review.expected_policy,
        }),
    });
    repo.verify_imported_check(&input).unwrap();
    input.red_green.as_mut().unwrap().review = None;
    assert!(repo.verify_imported_check(&input).is_err());
}
