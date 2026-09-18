use workdeck_pm::*;
#[test]
fn historical_receipt_survives_current_issue_becoming_unreadable_without_authorizing_work() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new("Outcome", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let input = ClaimRequest::Acquire {
        input: Box::new(AcquireClaim {
            actor: "agent".into(),
            contract: repository.claim_contract(&issue.metadata.id).unwrap(),
            ttl_seconds: None,
            recovery: None,
        }),
    };
    let request = RequestId::new();
    let first = repository.mutate_claim(&input, &request).unwrap();
    assert!(first.may_continue);
    std::fs::write(repository.root().join(issue.path), "---\ninvalid: [\n---\n").unwrap();
    let replay = repository.mutate_claim(&input, &request).unwrap();
    assert_eq!(replay.receipt, first.receipt);
    assert!(replay.current.is_none());
    assert!(!replay.may_continue);
    assert!(!replay.requested_token_current);
    assert!(
        replay
            .reason_codes
            .iter()
            .any(|reason| reason.starts_with("current_claim_unavailable"))
    );
}
