use workdeck_pm::*;

#[test]
fn malformed_claim_authority_is_not_invisible_to_doctor() {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    std::fs::create_dir(repo.root().join("claims")).unwrap();
    std::fs::write(repo.root().join("claims/WD-1.yml"), "not: a valid claim\n").unwrap();
    assert!(!repo.doctor().unwrap().valid);
}

use chrono::Duration;
use std::sync::{Arc, Barrier};
use workdeck_pm::transactions::{FaultPoint, MutationReceipt};

fn fixture() -> (tempfile::TempDir, Repository, IssueRecord) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let issue = serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Scoped work", "Implement the behavior.\n"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    (temp, repo, issue)
}
fn acquire(repo: &Repository, issue: &IssueRecord, actor: &str) -> ClaimRequest {
    ClaimRequest::Acquire {
        input: Box::new(AcquireClaim {
            actor: actor.into(),
            contract: repo.local_claim_contract(&issue.metadata.id).unwrap(),
            ttl_seconds: None,
            recovery: None,
        }),
    }
}
fn after(receipt: MutationReceipt) -> ClaimRecord {
    serde_json::from_value::<ClaimChange>(receipt.result)
        .unwrap()
        .after
}
fn mutate(record: &ClaimRecord, mutation: ClaimMutation) -> ClaimRequest {
    ClaimRequest::Mutate {
        issue: record.metadata.issue.clone(),
        expected: record.precondition(),
        mutation,
    }
}

#[test]
fn local_claim_preserves_issue_bytes_and_replays_original_generation() {
    let (_temp, repo, issue) = fixture();
    let bytes = std::fs::read(repo.root().join(&issue.path)).unwrap();
    let input = acquire(&repo, &issue, "agent-one");
    let request = RequestId::new();
    let first = repo.mutate_local_claim(&input, &request).unwrap();
    assert_eq!(repo.mutate_local_claim(&input, &request).unwrap(), first);
    let claim = after(first.clone());
    assert_eq!(claim.metadata.generation, 1);
    let renewed = after(
        repo.mutate_local_claim(
            &mutate(
                &claim,
                ClaimMutation::Renew {
                    actor: "agent-one".into(),
                    ttl_seconds: None,
                },
            ),
            &RequestId::new(),
        )
        .unwrap(),
    );
    assert_eq!(renewed.metadata.token, claim.metadata.token);
    assert_eq!(renewed.metadata.generation, 2);
    assert_eq!(repo.mutate_local_claim(&input, &request).unwrap(), first);
    assert_eq!(std::fs::read(repo.root().join(&issue.path)).unwrap(), bytes);
    let status = repo.local_claims().unwrap();
    assert_eq!(status.len(), 1);
    assert_eq!(
        status[0].assessment.guarantee,
        ClaimGuarantee::LocalSourceOnly
    );
    assert!(status[0].assessment.may_continue);
    assert!(repo.doctor().unwrap().valid);
}

#[test]
fn stale_token_generation_foreign_actor_and_reused_request_cannot_mutate_claim() {
    let (_temp, repo, issue) = fixture();
    let request = RequestId::new();
    let input = acquire(&repo, &issue, "one");
    let claim = after(repo.mutate_local_claim(&input, &request).unwrap());
    let mut other_input = input.clone();
    if let ClaimRequest::Acquire { input } = &mut other_input {
        input.actor = "two".into();
    }
    assert_eq!(
        repo.mutate_local_claim(&other_input, &request)
            .unwrap_err()
            .code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(
        repo.mutate_local_claim(
            &mutate(
                &claim,
                ClaimMutation::Release {
                    actor: "two".into(),
                    reason: "finish".into()
                }
            ),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::ClaimLost
    );
    let renewed = after(
        repo.mutate_local_claim(
            &mutate(
                &claim,
                ClaimMutation::Renew {
                    actor: "one".into(),
                    ttl_seconds: None,
                },
            ),
            &RequestId::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        repo.mutate_local_claim(
            &mutate(
                &claim,
                ClaimMutation::Release {
                    actor: "one".into(),
                    reason: "finish".into()
                }
            ),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::ClaimLost
    );
    let mut wrong_token = renewed.precondition();
    wrong_token.token = ClaimToken::new();
    let request = ClaimRequest::Mutate {
        issue: issue.metadata.id,
        expected: wrong_token,
        mutation: ClaimMutation::Renew {
            actor: "one".into(),
            ttl_seconds: None,
        },
    };
    assert_eq!(
        repo.mutate_local_claim(&request, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::ClaimLost
    );
    assert_eq!(repo.local_claims().unwrap()[0].claim, renewed);
}

#[test]
fn two_local_writers_confirm_only_one_owner() {
    let (_temp, repo, issue) = fixture();
    let barrier = Arc::new(Barrier::new(2));
    let inputs = [acquire(&repo, &issue, "one"), acquire(&repo, &issue, "two")];
    let handles = inputs
        .into_iter()
        .map(|input| {
            let repo = repo.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                repo.mutate_local_claim(&input, &RequestId::new())
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| r.as_ref().is_err_and(|e| e.code == ErrorCode::ClaimLost))
            .count(),
        1
    );
    assert_eq!(repo.local_claims().unwrap().len(), 1);
}

#[test]
fn changed_accepted_contract_requires_explicit_revalidation() {
    let (_temp, repo, issue) = fixture();
    let claim = after(
        repo.mutate_local_claim(&acquire(&repo, &issue, "one"), &RequestId::new())
            .unwrap(),
    );
    let mut input = UpdateIssue {
        fields: Default::default(),
        body: Some("Different scope.\n".into()),
    };
    input
        .fields
        .insert("title".into(), serde_json::json!("New accepted work"));
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &input,
        &RequestId::new(),
    )
    .unwrap();
    assert_eq!(
        repo.local_claims().unwrap()[0].assessment.disposition,
        ClaimDisposition::NeedsRevalidation
    );
    assert!(
        repo.mutate_local_claim(
            &mutate(
                &claim,
                ClaimMutation::Renew {
                    actor: "one".into(),
                    ttl_seconds: None
                }
            ),
            &RequestId::new()
        )
        .is_err()
    );
    let contract = repo.local_claim_contract(&issue.metadata.id).unwrap();
    let next = after(
        repo.mutate_local_claim(
            &mutate(
                &claim,
                ClaimMutation::Revalidate {
                    actor: "one".into(),
                    contract: Box::new(contract.clone()),
                    ttl_seconds: None,
                },
            ),
            &RequestId::new(),
        )
        .unwrap(),
    );
    assert_eq!(next.metadata.token, claim.metadata.token);
    assert_eq!(next.metadata.contract, contract);
    assert!(repo.local_claims().unwrap()[0].assessment.may_continue);
}

#[test]
fn expiry_requires_exact_explicit_recovery_beyond_clock_skew_and_revokes_old_token() {
    let (_temp, repo, issue) = fixture();
    let now = chrono::Utc::now();
    let mut input = acquire(&repo, &issue, "one");
    if let ClaimRequest::Acquire { input } = &mut input {
        input.ttl_seconds = Some(61);
    }
    let claim = after(
        repo.mutate_local_claim_with_faults(&input, &RequestId::new(), || now, |_| Ok(()))
            .unwrap(),
    );
    let late = now + Duration::seconds(92);
    assert_eq!(
        repo.mutate_local_claim_with_faults(
            &acquire(&repo, &issue, "two"),
            &RequestId::new(),
            || late,
            |_| Ok(())
        )
        .unwrap_err()
        .code,
        ErrorCode::ClaimLost
    );
    let mut recover = acquire(&repo, &issue, "two");
    if let ClaimRequest::Acquire { input } = &mut recover {
        input.recovery = Some(ClaimRecovery {
            expected: claim.precondition(),
            reason: "Inspected abandoned work; process state remains external".into(),
        });
    }
    assert_eq!(
        repo.mutate_local_claim_with_faults(
            &recover,
            &RequestId::new(),
            || now + Duration::seconds(90),
            |_| Ok(())
        )
        .unwrap_err()
        .code,
        ErrorCode::ClaimLost
    );
    let next = after(
        repo.mutate_local_claim_with_faults(&recover, &RequestId::new(), || late, |_| Ok(()))
            .unwrap(),
    );
    assert_ne!(next.metadata.token, claim.metadata.token);
    assert_eq!(next.metadata.generation, 2);
    assert_eq!(
        repo.mutate_local_claim(
            &mutate(
                &claim,
                ClaimMutation::Release {
                    actor: "one".into(),
                    reason: "late old release".into()
                }
            ),
            &RequestId::new()
        )
        .unwrap_err()
        .code,
        ErrorCode::ClaimLost
    );
}

#[test]
fn interrupted_claim_publication_recovers_without_duplicate_ownership() {
    let (_temp, repo, issue) = fixture();
    let input = acquire(&repo, &issue, "one");
    let request = RequestId::new();
    let error = repo
        .mutate_local_claim_with_faults(&input, &request, chrono::Utc::now, |point| {
            if point == FaultPoint::AfterChange(0) {
                Err(PmError::new(ErrorCode::Io, "simulated interruption"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    assert_eq!(
        repo.local_claims().unwrap_err().code,
        ErrorCode::RecoveryRequired
    );
    repo.recover_operations().unwrap();
    let receipt = repo.mutate_local_claim(&input, &request).unwrap();
    assert_eq!(after(receipt).metadata.generation, 1);
    assert_eq!(repo.local_claims().unwrap().len(), 1);
}

#[test]
fn edited_or_removed_claim_records_and_forged_receipts_cannot_be_usable() {
    let (_temp, repo, issue) = fixture();
    let receipt = repo
        .mutate_local_claim(&acquire(&repo, &issue, "one"), &RequestId::new())
        .unwrap();
    let claim = after(receipt.clone());
    let path = repo.root().join(&claim.path);
    std::fs::write(
        &path,
        claim.document.replace("actor: one", "actor: attacker"),
    )
    .unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.local_claims().is_err());
    std::fs::write(&path, &claim.document).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert!(!repo.doctor().unwrap().valid);
    std::fs::write(&path, &claim.document).unwrap();
    std::fs::remove_file(
        repo.root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
    )
    .unwrap();
    assert!(!repo.doctor().unwrap().valid);
    assert!(repo.local_claims().is_err());
}

#[test]
fn explicit_release_cancel_and_supersession_preserve_historical_generations() {
    for operation in ["release", "cancel", "supersede"] {
        let (_temp, repo, issue) = fixture();
        let claim = after(
            repo.mutate_local_claim(&acquire(&repo, &issue, "one"), &RequestId::new())
                .unwrap(),
        );
        let mutation = match operation {
            "release" => ClaimMutation::Release {
                actor: "one".into(),
                reason: "finished".into(),
            },
            "cancel" => ClaimMutation::Cancel {
                actor: "one".into(),
                reason: "abandoned".into(),
            },
            _ => ClaimMutation::Supersede {
                actor: "one".into(),
                reason: "replaced".into(),
            },
        };
        let terminal = after(
            repo.mutate_local_claim(&mutate(&claim, mutation), &RequestId::new())
                .unwrap(),
        );
        assert_ne!(terminal.metadata.state, ClaimState::Active);
        let next = after(
            repo.mutate_local_claim(&acquire(&repo, &issue, "two"), &RequestId::new())
                .unwrap(),
        );
        assert_eq!(next.metadata.generation, 3);
        assert_ne!(next.metadata.token, claim.metadata.token);
        assert!(repo.doctor().unwrap().valid);
    }
}

#[test]
fn same_request_replay_rejects_a_forged_original_claim_result() {
    let (_temp, repo, issue) = fixture();
    let input = acquire(&repo, &issue, "one");
    let request = RequestId::new();
    let receipt = repo.mutate_local_claim(&input, &request).unwrap();
    let mut forged = receipt.clone();
    forged.result["after"]["metadata"]["actor"] = serde_json::json!("attacker");
    std::fs::write(
        repo.root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&forged).unwrap(),
    )
    .unwrap();
    assert!(
        repo.mutate_local_claim(&input, &request).is_err(),
        "historical replay must validate claim authority, not return a forged result"
    );
}

#[test]
fn new_blocking_question_revokes_may_continue_without_rewriting_claim() {
    let (_temp, repo, issue) = fixture();
    let claim = after(
        repo.mutate_local_claim(&acquire(&repo, &issue, "one"), &RequestId::new())
            .unwrap(),
    );
    repo.create_question(
        &CreateQuestion {
            actor: "reviewer".into(),
            body: "Clarify the accepted scope.\n".into(),
            subjects: vec![QuestionSubject {
                subject: SubjectRef::Issue(issue.metadata.id.clone()),
                source: issue.source,
            }],
            requirements: vec![],
            blocks_work: true,
            custom: Default::default(),
            extra: Default::default(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let status = repo.local_claims().unwrap();
    assert_eq!(status[0].claim, claim);
    assert!(
        !status[0].assessment.may_continue,
        "current blocking questions must not leave a claim usable"
    );
}
