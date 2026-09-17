use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Barrier},
};
use workdeck_pm::transactions::{
    FaultPoint, FileChange, MutationReceipt, PreparedOperation, TransactionStore,
};
use workdeck_pm::*;

fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Capacity subject", "Original body.\n"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.mutate_local_claim(
        &ClaimRequest::Acquire {
            input: Box::new(AcquireClaim {
                actor: "worker".into(),
                contract: repo.local_claim_contract(&issue.metadata.id).unwrap(),
                ttl_seconds: None,
                recovery: None,
            }),
        },
        &RequestId::new(),
    )
    .unwrap();
    (temp, repo, issue)
}

fn history(repo: &Repository) -> Vec<(PathBuf, Vec<u8>)> {
    let mut files = fs::read_dir(repo.root().join("operations"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| a.0.cmp(&b.0));
    files
}

fn limited(repo: &Repository, count: usize, bytes: usize) -> TransactionStore {
    TransactionStore::open(repo.root())
        .unwrap()
        .with_operation_limits(count, bytes)
        .unwrap()
}

fn noop() -> PreparedOperation {
    PreparedOperation {
        changes: Vec::new(),
        result: json!({"note":"retained generic operation"}),
    }
}

fn edit(repo: &Repository, issue: &IssueRecord) -> PreparedOperation {
    let before = fs::read(repo.root().join(&issue.path)).unwrap();
    let mut after = before.clone();
    after.extend_from_slice(b"Additional authored paragraph.\n");
    PreparedOperation {
        changes: vec![FileChange {
            path: issue.path.clone(),
            expected: Some(ContentHash::of(&before)),
            content: Some(after),
        }],
        result: json!({"note":"generic document edit"}),
    }
}

fn assert_unchanged(repo: &Repository, issue: &IssueRecord, original: &[(PathBuf, Vec<u8>)]) {
    assert_eq!(repo.show_issue(issue.metadata.id.as_str()).unwrap(), *issue);
    assert_eq!(history(repo), original);
    assert!(repo.pending_operations().unwrap().is_empty());
    assert!(
        repo.local_claims_with_limits(&ClaimCatalogLimits {
            max_operations: original.len(),
            ..Default::default()
        })
        .unwrap()[0]
            .assessment
            .may_continue
    );
}

#[test]
fn unrelated_mutation_cannot_exceed_history_count_and_break_existing_claims() {
    let (_temp, repo, issue) = fixture();
    let before = history(&repo);
    let prepared = edit(&repo, &issue);
    let error = limited(&repo, before.len(), 64 * 1024 * 1024)
        .transact(&RequestId::new(), "fixture.edit", &json!({}), |_| {
            Ok(prepared)
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_unchanged(&repo, &issue, &before);
}

#[test]
fn exact_encoded_receipt_fit_is_allowed_and_one_byte_less_is_rejected() {
    let (_temp, repo, issue) = fixture();
    let first: RequestId = "capacity-bytes-1".parse().unwrap();
    let second: RequestId = "capacity-bytes-2".parse().unwrap();
    let store = TransactionStore::open(repo.root()).unwrap();
    let measured = store
        .transact(&first, "fixture.note", &json!({}), |_| Ok(noop()))
        .unwrap();
    let measured_bytes = fs::read(
        repo.root()
            .join(format!("operations/{}.yml", measured.operation_id)),
    )
    .unwrap()
    .len();
    let before = history(&repo);
    let current_bytes = before.iter().map(|(_, bytes)| bytes.len()).sum::<usize>();
    let error = limited(&repo, before.len() + 1, current_bytes + measured_bytes - 1)
        .transact(&second, "fixture.note", &json!({}), |_| Ok(noop()))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidInput);
    assert_unchanged(&repo, &issue, &before);
    let receipt = limited(&repo, before.len() + 1, current_bytes + measured_bytes)
        .transact(&second, "fixture.note", &json!({}), |_| Ok(noop()))
        .unwrap();
    let actual = fs::read(
        repo.root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
    )
    .unwrap();
    assert_eq!(actual.len(), measured_bytes);
    assert_eq!(
        history(&repo)
            .iter()
            .map(|(_, bytes)| bytes.len())
            .sum::<usize>(),
        current_bytes + measured_bytes
    );
    assert!(
        repo.local_claims_with_limits(&ClaimCatalogLimits {
            max_operations: before.len() + 1,
            max_operation_bytes: current_bytes + measured_bytes,
            ..Default::default()
        })
        .unwrap()[0]
            .assessment
            .may_continue
    );
}

#[test]
fn historical_retry_precedes_new_capacity_admission() {
    let (_temp, repo, _issue) = fixture();
    let request = RequestId::new();
    let original = TransactionStore::open(repo.root())
        .unwrap()
        .transact(&request, "fixture.note", &json!({}), |_| Ok(noop()))
        .unwrap();
    let before = history(&repo);
    let replay = limited(&repo, 1, 1)
        .transact(&request, "fixture.note", &json!({}), |_| {
            panic!("historical replay must not prepare another mutation")
        })
        .unwrap();
    assert_eq!(replay, original);
    assert_eq!(history(&repo), before);
}

#[test]
fn admitted_interruption_recovers_even_when_new_admission_limits_are_lower() {
    for point in [
        FaultPoint::AfterJournal,
        FaultPoint::AfterChange(0),
        FaultPoint::AfterReceipt,
    ] {
        let (_temp, repo, issue) = fixture();
        let before = history(&repo);
        let request = RequestId::new();
        let prepared = edit(&repo, &issue);
        let after = prepared.changes[0].content.clone().unwrap();
        let error = limited(&repo, before.len() + 1, 64 * 1024 * 1024)
            .transact_with_faults(
                &request,
                "fixture.edit",
                &json!({}),
                |_| Ok(prepared),
                |actual| {
                    if actual == point {
                        Err(PmError::new(ErrorCode::Io, "interrupted admitted write"))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired);
        let recovery = limited(&repo, 1, 1);
        let receipts = recovery.recover().unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(fs::read(repo.root().join(&issue.path)).unwrap(), after);
        assert_eq!(history(&repo).len(), before.len() + 1);
        assert!(recovery.pending_operations().unwrap().is_empty());
        assert_eq!(
            recovery
                .transact(&request, "fixture.edit", &json!({}), |_| panic!(
                    "recovered request is historical"
                ))
                .unwrap(),
            receipts[0]
        );
    }
}

#[test]
fn concurrent_generic_writers_cannot_both_consume_the_final_receipt_slot() {
    let (_temp, repo, issue) = fixture();
    let before = history(&repo);
    let store = limited(&repo, before.len() + 1, 64 * 1024 * 1024);
    let barrier = Arc::new(Barrier::new(2));
    let workers = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.transact(
                    &RequestId::new(),
                    "fixture.note",
                    &json!({}),
                    |_| Ok(noop()),
                )
            })
        })
        .collect::<Vec<_>>();
    let outcomes = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .find_map(|result| result.as_ref().err())
            .unwrap()
            .code,
        ErrorCode::InvalidInput
    );
    assert_eq!(history(&repo).len(), before.len() + 1);
    assert_eq!(repo.show_issue(issue.metadata.id.as_str()).unwrap(), issue);
    assert!(
        repo.local_claims_with_limits(&ClaimCatalogLimits {
            max_operations: before.len() + 1,
            ..Default::default()
        })
        .unwrap()[0]
            .assessment
            .may_continue
    );
}

#[test]
fn receipt_bytes_used_for_capacity_are_pinned_until_journal_admission() {
    let (_temp, repo, issue) = fixture();
    let before = history(&repo);
    let path = before[0].0.clone();
    let mut edited = before[0].1.clone();
    edited.extend_from_slice(b"\n");
    let parsed_before: MutationReceipt = serde_yaml_ng::from_slice(&before[0].1).unwrap();
    let parsed_after: MutationReceipt = serde_yaml_ng::from_slice(&edited).unwrap();
    assert_eq!(parsed_before, parsed_after);
    let prepared = edit(&repo, &issue);
    let error = TransactionStore::open(repo.root())
        .unwrap()
        .transact_with_faults(
            &RequestId::new(),
            "fixture.edit",
            &json!({}),
            |_| Ok(prepared),
            |point| {
                if point == FaultPoint::BeforeJournal {
                    fs::write(&path, &edited).unwrap();
                }
                Ok(())
            },
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(fs::read(path).unwrap(), edited);
    assert_eq!(repo.show_issue(issue.metadata.id.as_str()).unwrap(), issue);
    assert_eq!(history(&repo).len(), before.len());
    assert!(repo.pending_operations().unwrap().is_empty());
}
