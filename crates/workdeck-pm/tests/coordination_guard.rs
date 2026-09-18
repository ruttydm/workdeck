use workdeck_pm::{
    transactions::{FileChange, PreparedOperation, TransactionStore},
    *,
};
#[test]
fn ordinary_transaction_cannot_convert_planning_authority_into_coordination_source() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let store = TransactionStore::open(repository.root()).unwrap();
    let marker = serde_json::to_vec(&CoordinationMarker {
        schema: SchemaVersion::CURRENT,
        repository: repository.identity().clone(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
    })
    .unwrap();
    let result = store.transact(
        &RequestId::new(),
        "fixture.marker",
        &serde_json::json!({}),
        |_| {
            Ok(PreparedOperation {
                changes: vec![FileChange {
                    path: "coordination.yml".into(),
                    expected: None,
                    content: Some(marker),
                }],
                result: serde_json::json!({}),
            })
        },
    );
    assert_eq!(result.unwrap_err().code, ErrorCode::UnsafePath);
    assert!(!repository.root().join("coordination.yml").exists());
    assert!(repository.doctor().unwrap().valid);
    assert!(repository.operation_history().unwrap().is_empty());
}

#[test]
fn recovery_rejects_a_claim_journal_with_forged_semantic_receipt_before_publishing_files() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Recover owned work", ""),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let input = ClaimRequest::Acquire {
        input: Box::new(AcquireClaim {
            actor: "agent".into(),
            contract: repository.local_claim_contract(&issue.metadata.id).unwrap(),
            ttl_seconds: None,
            recovery: None,
        }),
    };
    let error = repository
        .mutate_local_claim_with_faults(&input, &RequestId::new(), chrono::Utc::now, |point| {
            if matches!(point, transactions::FaultPoint::AfterJournal) {
                Err(PmError::new(ErrorCode::Io, "fixture interruption"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    let journal = std::fs::read_dir(repository.root().join(".tmp/journals"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut value: serde_json::Value =
        serde_yaml_ng::from_slice(&std::fs::read(&journal).unwrap()).unwrap();
    value["receipt"]["result"]["after"]["metadata"]["actor"] = serde_json::json!("forged");
    std::fs::write(&journal, serde_yaml_ng::to_string(&value).unwrap()).unwrap();
    assert!(
        repository.recover_operations().is_err(),
        "semantic claim authority must be checked before recovery writes"
    );
    assert!(
        !repository
            .root()
            .join(format!("claims/{}.yml", issue.metadata.id))
            .exists()
    );
    assert!(
        journal.exists(),
        "preserve rejected recovery material for explicit reconciliation"
    );
}
