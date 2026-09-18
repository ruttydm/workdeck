#![cfg(unix)]
use workdeck_pm::*;
#[path = "support/red_green_fixture.rs"]
mod support;
use support::fixture;
#[test]
fn signed_real_assertion_failure_and_fix_qualify_only_the_accepted_check_pair() {
    let (root, repo, request) = fixture("broken", "fixed", false);
    let operations = repo.operation_history().unwrap();
    let assessment = verify_red_green(root.path(), &request).unwrap();
    assert_eq!(assessment.check.id, "unit");
    assert_eq!(assessment.cases.len(), 1);
    assert!(assessment.changed_inputs.contains(&"src/value".into()));
    assert_eq!(repo.operation_history().unwrap(), operations);
    let mut tampered = request.clone();
    tampered.red_artifact.push(' ');
    assert!(verify_red_green(root.path(), &tampered).is_err());
    tampered = request.clone();
    tampered.expected_producer_policy = ContentHash::of(b"wrong policy");
    assert!(verify_red_green(root.path(), &tampered).is_err());
    tampered = request.clone();
    tampered.red = request.green.clone();
    assert!(verify_red_green(root.path(), &tampered).is_err());
}
#[test]
fn infrastructure_errors_skips_removed_cases_and_changed_evaluators_do_not_qualify() {
    for (red, green, changed) in [
        ("infra", "fixed", false),
        ("crash", "fixed", false),
        ("unrelated", "fixed", false),
        ("broken", "skip", false),
        ("broken", "renamed", false),
        ("broken", "fixed", true),
    ] {
        let (root, _, request) = fixture(red, green, changed);
        assert!(
            verify_red_green(root.path(), &request).is_err(),
            "{red}/{green}/{changed}"
        );
        if changed {
            let review = support::baseline_review(root.path(), &request);
            assert!(verify_reviewed_red_green(root.path(), &request, &review).is_err());
        }
    }
}

#[test]
fn nondeterministic_outcomes_without_changed_selected_inputs_are_not_a_fix() {
    let (root, _, request) = fixture("flaky", "flaky", false);
    let error = verify_red_green(root.path(), &request).unwrap_err();
    assert!(error.message.contains("inputs did not change"), "{error:?}");
}
#[test]
fn additional_green_cases_are_allowed_without_losing_the_accepted_denominator() {
    let (root, _, request) = fixture("broken", "extended", false);
    let result = verify_red_green(root.path(), &request).unwrap();
    assert_eq!(result.cases.len(), 1);
    // Verification consumes immutable sources, not whatever is currently checked out.
    std::fs::write(root.path().join("src/value"), "uncommitted change").unwrap();
    assert_eq!(
        result.fingerprint,
        verify_red_green(root.path(), &request).unwrap().fingerprint
    );
}

#[test]
fn reviewed_test_baseline_connects_prior_acceptance_to_real_red_green_evidence() {
    let (root, repo, pair) = fixture("broken", "fixed", false);
    let review = support::baseline_review(root.path(), &pair);
    let before = repo.operation_history().unwrap();
    let result = verify_reviewed_red_green(root.path(), &pair, &review).unwrap();
    assert_eq!(result.pair.baseline, pair.baseline);
    assert_eq!(result.review.approval.baseline, review.accepted);
    assert_eq!(result.review.approval.head.commit, pair.baseline.commit);
    assert_eq!(result.review.reviewers, vec!["maintainer"]);
    assert_eq!(repo.operation_history().unwrap(), before);
    let mut wrong_red = pair.clone();
    wrong_red.baseline.contract = ContentHash::of(b"unreviewed red contract");
    assert!(verify_reviewed_red_green(root.path(), &wrong_red, &review).is_err());
    let mut bad = review.clone();
    bad.expected_policy = ContentHash::of(b"wrong independent policy");
    assert!(verify_reviewed_red_green(root.path(), &pair, &bad).is_err());
    bad = review.clone();
    bad.accepted = pair.baseline.clone();
    assert!(verify_reviewed_red_green(root.path(), &pair, &bad).is_err());
    bad = review.clone();
    bad.envelope.signatures.clear();
    assert!(verify_reviewed_red_green(root.path(), &pair, &bad).is_err());
    bad = review;
    bad.policy.required_reviewers[0].expires_at = chrono::Utc::now() - chrono::Duration::seconds(1);
    bad.expected_policy = bad.policy.fingerprint().unwrap();
    assert!(verify_reviewed_red_green(root.path(), &pair, &bad).is_err());
}
