use workdeck_pm::transactions::{FaultPoint, MutationReceipt};
use workdeck_pm::*;

fn fixture() -> (
    tempfile::TempDir,
    Repository,
    IssueRecord,
    ClaimRecord,
    CompleteClaimedIssue,
) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Owned implementation", "Finish the accepted behavior.\n"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let contract = repo.local_claim_contract(&issue.metadata.id).unwrap();
    let request = ClaimRequest::Acquire {
        input: Box::new(AcquireClaim {
            actor: "worker".into(),
            contract: contract.clone(),
            ttl_seconds: None,
            recovery: None,
        }),
    };
    let claim: ClaimChange = serde_json::from_value(
        repo.mutate_local_claim(&request, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let claim = claim.after;
    let input = CompleteClaimedIssue {
        expected_binding: None,
        issue: issue.metadata.id.clone(),
        actor: "worker".into(),
        expected_claim: claim.precondition(),
        expected_issue: issue.source.clone(),
        contract,
    };
    (temp, repo, issue, claim, input)
}

fn proof(receipt: &MutationReceipt) -> ClaimedCompletionProof {
    serde_json::from_value(receipt.result.clone()).unwrap()
}

fn assert_unchanged(repo: &Repository, issue: &IssueRecord, claim: &ClaimRecord, count: usize) {
    assert_eq!(repo.show_issue(issue.metadata.id.as_str()).unwrap(), *issue);
    assert_eq!(repo.local_claims().unwrap()[0].claim, *claim);
    assert_eq!(repo.operation_history().unwrap().len(), count);
}

#[test]
fn completion_admits_exact_live_claim_and_keeps_release_separate() {
    let (_temp, repo, issue, claim, input) = fixture();
    let claim_bytes = std::fs::read(repo.root().join(&claim.path)).unwrap();
    let receipt = repo
        .complete_claimed_issue(&input, &RequestId::new())
        .unwrap();
    let proof = proof(&receipt);
    assert_eq!(receipt.operation, "issue.complete_claimed");
    assert_eq!(proof.before, issue);
    assert_eq!(proof.claim, claim);
    assert_eq!(proof.after.metadata.status, "done");
    assert_eq!(
        proof.after.metadata.revision,
        issue.metadata.revision.next().unwrap()
    );
    assert!(proof.after.metadata.manual_acceptance.is_none());
    assert_eq!(receipt.changed.len(), 1);
    assert_eq!(receipt.changed[0].path, issue.path);
    assert_eq!(
        std::fs::read(repo.root().join(&claim.path)).unwrap(),
        claim_bytes
    );
    assert_eq!(
        repo.local_claims().unwrap()[0].claim.metadata.state,
        ClaimState::Active
    );
    assert!(repo.doctor().unwrap().valid);
    repo.export_snapshot().unwrap().validate().unwrap();
}

#[test]
fn stale_generation_wrong_token_and_foreign_actor_cannot_complete() {
    let (_temp, repo, issue, claim, mut input) = fixture();
    let renewed: ClaimChange = serde_json::from_value(
        repo.mutate_local_claim(
            &ClaimRequest::Mutate {
                issue: issue.metadata.id.clone(),
                expected: claim.precondition(),
                mutation: ClaimMutation::Renew {
                    actor: "worker".into(),
                    ttl_seconds: None,
                },
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let current = renewed.after;
    let count = repo.operation_history().unwrap().len();
    assert_eq!(
        repo.complete_claimed_issue(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::ClaimLost
    );
    input.expected_claim = current.precondition();
    input.expected_claim.token = ClaimToken::new();
    assert_eq!(
        repo.complete_claimed_issue(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::ClaimLost
    );
    input.expected_claim = current.precondition();
    input.actor = "different-worker".into();
    assert_eq!(
        repo.complete_claimed_issue(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::ClaimLost
    );
    assert_unchanged(&repo, &issue, &current, count);
}

#[test]
fn fresh_issue_token_does_not_silently_revalidate_old_claim_requirements() {
    let (_temp, repo, issue, claim, mut input) = fixture();
    let changed: IssueRecord = serde_json::from_value(
        repo.update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: Default::default(),
                body: Some("A different accepted requirement.\n".into()),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    input.expected_issue = changed.source.clone();
    input.contract = repo.local_claim_contract(&issue.metadata.id).unwrap();
    let count = repo.operation_history().unwrap().len();
    assert_eq!(
        repo.complete_claimed_issue(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_unchanged(&repo, &changed, &claim, count);
}

#[test]
fn expired_or_expiring_before_the_journal_claim_cannot_complete() {
    let (_temp, repo, issue, claim, input) = fixture();
    let count = repo.operation_history().unwrap().len();
    let expired = claim.metadata.expires_at + chrono::Duration::seconds(31);
    assert_eq!(
        repo.complete_claimed_issue_with_faults(&input, &RequestId::new(), || expired, |_| Ok(()))
            .unwrap_err()
            .code,
        ErrorCode::ClaimLost
    );
    let mut readings = [claim.metadata.updated_at, expired].into_iter();
    assert_eq!(
        repo.complete_claimed_issue_with_faults(
            &input,
            &RequestId::new(),
            || readings.next().unwrap_or(expired),
            |_| Ok(())
        )
        .unwrap_err()
        .code,
        ErrorCode::ClaimLost
    );
    assert_unchanged(&repo, &issue, &claim, count);
    assert!(repo.pending_operations().unwrap().is_empty());
}

#[test]
fn blocking_question_and_required_completion_policy_remain_enforced() {
    let (_temp, repo, issue, claim, input) = fixture();
    repo.create_question(
        &CreateQuestion {
            actor: "reviewer".into(),
            body: "Resolve the current design before proceeding.".into(),
            subjects: vec![QuestionSubject {
                subject: SubjectRef::Issue(issue.metadata.id.clone()),
                source: issue.source.clone(),
            }],
            requirements: vec![],
            blocks_work: true,
            custom: Default::default(),
            extra: Default::default(),
        },
        &RequestId::new(),
    )
    .unwrap();
    let count = repo.operation_history().unwrap().len();
    assert_eq!(
        repo.complete_claimed_issue(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_unchanged(&repo, &issue, &claim, count);

    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    let mut create = CreateIssue::new("Incomplete criterion", "The description exists.\n");
    create.fields.insert(
        "acceptance".into(),
        serde_json::json!([{"id":"behavior","description":"Required behavior","checked":false}]),
    );
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(&create, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let contract = repo.local_claim_contract(&issue.metadata.id).unwrap();
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
    let input = CompleteClaimedIssue {
        expected_binding: None,
        issue: issue.metadata.id.clone(),
        actor: "worker".into(),
        expected_claim: claim.after.precondition(),
        expected_issue: issue.source.clone(),
        contract,
    };
    let count = repo.operation_history().unwrap().len();
    assert_eq!(
        repo.complete_claimed_issue(&input, &RequestId::new())
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_unchanged(&repo, &issue, &claim.after, count);
}

#[test]
fn original_completion_replays_after_release_reopen_and_later_issue_edits() {
    let (_temp, repo, issue, claim, input) = fixture();
    let request = RequestId::new();
    let receipt = repo.complete_claimed_issue(&input, &request).unwrap();
    repo.mutate_local_claim(
        &ClaimRequest::Mutate {
            issue: issue.metadata.id.clone(),
            expected: claim.precondition(),
            mutation: ClaimMutation::Release {
                actor: "worker".into(),
                reason: "Explicit later release".into(),
            },
        },
        &RequestId::new(),
    )
    .unwrap();
    let reopened: IssueRecord = serde_json::from_value(
        repo.mutate_issue(
            issue.metadata.id.as_str(),
            Some(&proof(&receipt).after.source),
            &IssueMutation::Reopen,
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let changed = repo
        .update_issue(
            issue.metadata.id.as_str(),
            &reopened.source,
            &UpdateIssue {
                fields: Default::default(),
                body: Some("Later scoped work.\n".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(
        repo.complete_claimed_issue(&input, &request).unwrap(),
        receipt
    );
    assert_eq!(
        serde_json::to_value(repo.show_issue(issue.metadata.id.as_str()).unwrap()).unwrap(),
        changed.result
    );
}

#[test]
fn direct_editor_change_before_journal_is_detected_under_the_same_guard() {
    let (_temp, repo, issue, claim, input) = fixture();
    let path = repo.root().join(&issue.path);
    let original = std::fs::read_to_string(&path).unwrap();
    let edited = format!("{original}\nConcurrent authored note.\n");
    let count = repo.operation_history().unwrap().len();
    let error = repo
        .complete_claimed_issue_with_faults(&input, &RequestId::new(), chrono::Utc::now, |point| {
            if point == FaultPoint::BeforeJournal {
                std::fs::write(&path, &edited).unwrap();
            }
            Ok(())
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(std::fs::read_to_string(path).unwrap(), edited);
    assert_eq!(repo.operation_history().unwrap().len(), count);
    assert_eq!(repo.local_claims().unwrap()[0].claim, claim);
}

#[test]
fn interrupted_completion_recovers_once_and_never_releases_claim_implicitly() {
    let (_temp, repo, _issue, claim, input) = fixture();
    let request = RequestId::new();
    let error = repo
        .complete_claimed_issue_with_faults(&input, &request, chrono::Utc::now, |point| {
            if point == FaultPoint::AfterChange(0) {
                Err(PmError::new(ErrorCode::Io, "lost acknowledgement"))
            } else {
                Ok(())
            }
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::RecoveryRequired);
    repo.recover_operations().unwrap();
    let receipt = repo.complete_claimed_issue(&input, &request).unwrap();
    assert_eq!(proof(&receipt).after.metadata.status, "done");
    assert_eq!(repo.local_claims().unwrap()[0].claim, claim);
    assert_eq!(
        repo.complete_claimed_issue(&input, &request).unwrap(),
        receipt
    );
}

#[test]
fn completed_receipt_survives_separate_release_failure_and_retry() {
    let (_temp, repo, _issue, claim, input) = fixture();
    let completion = RequestId::new();
    let release = RequestId::new();
    let partial = repo
        .complete_claimed_issue_and_release_with_faults(
            &input,
            &completion,
            &release,
            "Completed owned work",
            |_| {
                Err(PmError::new(
                    ErrorCode::Io,
                    "release unavailable after completion",
                ))
            },
        )
        .unwrap();
    assert_eq!(proof(&partial.completion).after.metadata.status, "done");
    assert!(!partial.release_recorded);
    assert!(partial.release.is_none());
    assert_eq!(partial.release_error.unwrap().code, ErrorCode::Io);
    assert_eq!(repo.local_claims().unwrap()[0].claim, claim);
    let completed = repo
        .complete_claimed_issue_and_release(&input, &completion, &release, "Completed owned work")
        .unwrap();
    assert_eq!(completed.completion, partial.completion);
    assert!(completed.release_recorded);
    let released: ClaimChange =
        serde_json::from_value(completed.release.as_ref().unwrap().result.clone()).unwrap();
    assert_eq!(released.after.metadata.state, ClaimState::Released);
    let replay = repo
        .complete_claimed_issue_and_release(&input, &completion, &release, "Completed owned work")
        .unwrap();
    assert_eq!(replay.completion, completed.completion);
    assert_eq!(replay.release, completed.release);
}

#[test]
fn forged_historical_completed_result_is_rejected_on_retry() {
    let (_temp, repo, _issue, _claim, input) = fixture();
    let request = RequestId::new();
    let mut receipt = repo.complete_claimed_issue(&input, &request).unwrap();
    receipt.result["after"]["metadata"]["title"] = serde_json::json!("Forged accepted output");
    std::fs::write(
        repo.root()
            .join(format!("operations/{}.yml", receipt.operation_id)),
        serde_yaml_ng::to_string(&receipt).unwrap(),
    )
    .unwrap();
    assert_eq!(
        repo.complete_claimed_issue(&input, &request)
            .unwrap_err()
            .code,
        ErrorCode::InvalidSchema
    );
    assert!(!repo.doctor().unwrap().valid);
}

#[test]
fn invalid_composite_release_input_is_rejected_before_completion() {
    let (_temp, repo, issue, claim, input) = fixture();
    let count = repo.operation_history().unwrap().len();
    let request = RequestId::new();
    assert_eq!(
        repo.complete_claimed_issue_and_release(&input, &request, &request, "Explicit release")
            .unwrap_err()
            .code,
        ErrorCode::InvalidInput
    );
    assert!(
        repo.complete_claimed_issue_and_release(&input, &RequestId::new(), &RequestId::new(), " ")
            .is_err()
    );
    assert_unchanged(&repo, &issue, &claim, count);
}

#[test]
fn recorded_release_replay_does_not_describe_a_later_claims_current_ownership() {
    let (_temp, repo, issue, _claim, input) = fixture();
    let completion = RequestId::new();
    let release = RequestId::new();
    let original = repo
        .complete_claimed_issue_and_release(&input, &completion, &release, "Completed owned work")
        .unwrap();
    repo.mutate_issue(
        issue.metadata.id.as_str(),
        Some(&proof(&original.completion).after.source),
        &IssueMutation::Reopen,
        &RequestId::new(),
    )
    .unwrap();
    let next: ClaimChange = serde_json::from_value(
        repo.mutate_local_claim(
            &ClaimRequest::Acquire {
                input: Box::new(AcquireClaim {
                    actor: "later-owner".into(),
                    contract: repo.local_claim_contract(&issue.metadata.id).unwrap(),
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
    let replay = repo
        .complete_claimed_issue_and_release(&input, &completion, &release, "Completed owned work")
        .unwrap();
    assert!(replay.release_recorded);
    assert_eq!(replay.release, original.release);
    assert_eq!(repo.local_claims().unwrap()[0].claim, next.after);
    assert_eq!(next.after.metadata.state, ClaimState::Active);
}

#[cfg(unix)]
mod shared {
    use super::*;
    use std::{fs, path::Path, process::Command};

    fn git(root: &Path, args: &[&str]) -> Vec<u8> {
        let mut command = Command::new("git");
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(|name| name.starts_with("GIT_")) {
                command.env_remove(name);
            }
        }
        let output = command
            .current_dir(root)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        output.stdout
    }

    fn fixture() -> (
        tempfile::TempDir,
        tempfile::TempDir,
        Repository,
        CompleteClaimedIssue,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let remote = tempfile::tempdir().unwrap();
        git(temp.path(), &["init", "-b", "main"]);
        git(remote.path(), &["init", "--bare"]);
        git(temp.path(), &["config", "user.name", "Fixture"]);
        git(
            temp.path(),
            &["config", "user.email", "fixture@example.invalid"],
        );
        git(
            temp.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        );
        let repo = Repository::init(temp.path(), "WD").unwrap();
        let issue: IssueRecord = serde_json::from_value(
            repo.create_issue(
                &CreateIssue::new("Shared implementation", "Accepted work contract.\n"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
        )
        .unwrap();
        let mut config = repo.config().unwrap();
        config.sources = Some(SharedSources {
            remote: "origin".into(),
            accepted_ref: "refs/heads/main".parse().unwrap(),
            coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
            proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
        });
        fs::write(
            repo.root().join("config.yml"),
            serde_yaml_ng::to_string(&config).unwrap(),
        )
        .unwrap();
        fs::write(temp.path().join("code.txt"), "accepted code").unwrap();
        git(temp.path(), &["add", "."]);
        git(temp.path(), &["commit", "-m", "accepted"]);
        git(temp.path(), &["push", "origin", "main"]);
        let contract = repo.claim_contract(&issue.metadata.id).unwrap();
        let claimed = repo
            .mutate_claim(
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
            .unwrap();
        assert_eq!(
            claimed.publication.unwrap().state,
            PublicationState::Confirmed
        );
        let input = CompleteClaimedIssue {
            expected_binding: None,
            issue: issue.metadata.id,
            actor: "worker".into(),
            expected_claim: claimed.current.unwrap().claim.precondition(),
            expected_issue: issue.source,
            contract,
        };
        (temp, remote, repo, input)
    }

    #[test]
    fn shared_completion_is_local_until_separate_proposal_and_retains_observed_authority() {
        let (temp, remote, repo, input) = fixture();
        fs::write(temp.path().join("code.txt"), "staged code").unwrap();
        git(temp.path(), &["add", "code.txt"]);
        fs::write(temp.path().join("code.txt"), "unstaged code").unwrap();
        let index = fs::read(temp.path().join(".git/index")).unwrap();
        let head = git(temp.path(), &["rev-parse", "HEAD"]);
        let accepted = git(remote.path(), &["rev-parse", "refs/heads/main"]);
        let coordination = git(
            remote.path(),
            &["rev-parse", "refs/heads/workdeck-coordination"],
        );
        let receipt = repo
            .complete_claimed_issue(&input, &RequestId::new())
            .unwrap();
        let proof = proof(&receipt);
        assert_eq!(proof.after.metadata.status, "done");
        let confirmed = proof.confirmation.unwrap();
        assert_eq!(confirmed.accepted.identity.role, SourceRole::Accepted);
        assert_eq!(
            confirmed.coordination.identity.role,
            SourceRole::Coordination
        );
        assert_eq!(proof.claim.metadata.token, input.expected_claim.token);
        assert_eq!(
            git(remote.path(), &["rev-parse", "refs/heads/main"]),
            accepted
        );
        assert_eq!(
            git(
                remote.path(),
                &["rev-parse", "refs/heads/workdeck-coordination"]
            ),
            coordination
        );
        assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head);
        assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
        assert_eq!(
            fs::read(temp.path().join("code.txt")).unwrap(),
            b"unstaged code"
        );
        assert!(!repo.root().join("claims").exists());
        assert!(repo.doctor().unwrap().valid);
        repo.export_snapshot().unwrap().validate().unwrap();
    }

    #[test]
    fn shared_completion_rejects_a_newer_confirmed_claim_generation() {
        let (_temp, _remote, repo, input) = fixture();
        let before = repo.show_issue(input.issue.as_str()).unwrap();
        let renewed = repo
            .mutate_claim(
                &ClaimRequest::Mutate {
                    issue: input.issue.clone(),
                    expected: input.expected_claim.clone(),
                    mutation: ClaimMutation::Renew {
                        actor: input.actor.clone(),
                        ttl_seconds: None,
                    },
                },
                &RequestId::new(),
            )
            .unwrap();
        assert_eq!(
            renewed.publication.unwrap().state,
            PublicationState::Confirmed
        );
        assert_eq!(
            repo.complete_claimed_issue(&input, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::ClaimLost
        );
        assert_eq!(repo.show_issue(input.issue.as_str()).unwrap(), before);
    }

    #[test]
    fn shared_completion_rejects_changed_accepted_requirements_while_local_issue_is_unchanged() {
        let (_temp, remote, repo, input) = fixture();
        let other = tempfile::tempdir().unwrap();
        git(
            other.path(),
            &[
                "clone",
                "--branch",
                "main",
                remote.path().to_str().unwrap(),
                ".",
            ],
        );
        git(other.path(), &["config", "user.name", "Reviewer"]);
        git(
            other.path(),
            &["config", "user.email", "reviewer@example.invalid"],
        );
        let second = Repository::open_source(&other.path().join(".workdeck")).unwrap();
        let issue = second.show_issue(input.issue.as_str()).unwrap();
        second
            .update_issue(
                input.issue.as_str(),
                &issue.source,
                &UpdateIssue {
                    fields: Default::default(),
                    body: Some("New accepted requirement.\n".into()),
                },
                &RequestId::new(),
            )
            .unwrap();
        git(other.path(), &["add", ".workdeck"]);
        git(other.path(), &["commit", "-m", "new accepted scope"]);
        git(other.path(), &["push", "origin", "main"]);
        assert_eq!(
            repo.complete_claimed_issue(&input, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
        assert_eq!(
            repo.show_issue(input.issue.as_str()).unwrap().source,
            input.expected_issue
        );
    }

    #[test]
    fn shared_completion_replay_is_historical_even_when_remote_is_now_unavailable() {
        let (_temp, remote, repo, input) = fixture();
        let request = RequestId::new();
        let receipt = repo.complete_claimed_issue(&input, &request).unwrap();
        let unavailable = remote.path().join("unavailable.git");
        git(
            repo.root().parent().unwrap(),
            &["remote", "set-url", "origin", unavailable.to_str().unwrap()],
        );
        assert_eq!(
            repo.complete_claimed_issue(&input, &request).unwrap(),
            receipt
        );
    }

    #[test]
    fn local_policy_edits_cannot_replace_confirmed_accepted_policy() {
        let (_temp, _remote, repo, input) = fixture();
        let before = repo.show_issue(input.issue.as_str()).unwrap();
        let count = repo.operation_history().unwrap().len();
        let mut config = repo.config().unwrap();
        assert!(config.acceptance.require_all_criteria);
        config.acceptance.require_all_criteria = false;
        fs::write(
            repo.root().join("config.yml"),
            serde_yaml_ng::to_string(&config).unwrap(),
        )
        .unwrap();
        assert_eq!(
            repo.complete_claimed_issue(&input, &RequestId::new())
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
        assert_eq!(repo.show_issue(input.issue.as_str()).unwrap(), before);
        assert_eq!(repo.operation_history().unwrap().len(), count);
        assert!(repo.pending_operations().unwrap().is_empty());
    }

    #[test]
    fn shared_local_git_binding_is_revalidated_before_issue_publication() {
        let (temp, _remote, repo, input) = fixture();
        let before = repo.show_issue(input.issue.as_str()).unwrap();
        let count = repo.operation_history().unwrap().len();
        let git_config = temp.path().join(".git/config");
        let original = fs::read_to_string(&git_config).unwrap();
        let edited = format!("{original}\n# concurrent configuration edit\n");
        let error = repo
            .complete_claimed_issue_with_faults(
                &input,
                &RequestId::new(),
                chrono::Utc::now,
                |point| {
                    if point == FaultPoint::BeforeJournal {
                        fs::write(&git_config, &edited).unwrap();
                    }
                    Ok(())
                },
            )
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource);
        assert_eq!(fs::read_to_string(git_config).unwrap(), edited);
        assert_eq!(repo.show_issue(input.issue.as_str()).unwrap(), before);
        assert_eq!(repo.operation_history().unwrap().len(), count);
        assert!(repo.pending_operations().unwrap().is_empty());
    }

    #[test]
    fn forged_shared_confirmation_is_rejected_by_retry_and_doctor() {
        let (_temp, _remote, repo, input) = fixture();
        let request = RequestId::new();
        let receipt = repo.complete_claimed_issue(&input, &request).unwrap();
        let path = repo
            .root()
            .join(format!("operations/{}.yml", receipt.operation_id));
        for mutate in [
            |value: &mut serde_json::Value| {
                value["confirmation"]["coordination"]["freshness"] = serde_json::json!("cached");
            },
            |value: &mut serde_json::Value| {
                value["confirmation"]["accepted"]["remote_observation"]["remote"] =
                    serde_json::json!("other-remote");
            },
            |value: &mut serde_json::Value| {
                value["confirmation"]["contract"]["requirements"] =
                    serde_json::to_value(ContentHash::of(b"invented requirements")).unwrap();
            },
        ] {
            let mut forged = receipt.clone();
            mutate(&mut forged.result);
            fs::write(&path, serde_yaml_ng::to_string(&forged).unwrap()).unwrap();
            assert_eq!(
                repo.complete_claimed_issue(&input, &request)
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidSchema
            );
            assert!(!repo.doctor().unwrap().valid);
        }
        fs::write(path, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
        assert_eq!(
            repo.complete_claimed_issue(&input, &request).unwrap(),
            receipt
        );
    }
    #[test]
    fn shared_release_lost_response_retains_completion_and_reconciles_without_duplicate_release() {
        let (_temp, _remote, repo, input) = fixture();
        let completion = RequestId::new();
        let release = RequestId::new();
        let partial = repo
            .complete_claimed_issue_and_release_with_publication_faults(
                &input,
                &completion,
                &release,
                "Completed owned work",
                |point| {
                    if point == PublicationFaultPoint::AfterPush {
                        Err(PmError::new(
                            ErrorCode::Io,
                            "lost release push acknowledgement",
                        ))
                    } else {
                        Ok(())
                    }
                },
            )
            .unwrap();
        assert_eq!(proof(&partial.completion).after.metadata.status, "done");
        assert!(!partial.release_recorded);
        assert!(partial.release_error.is_some());
        let complete = repo
            .complete_claimed_issue_and_release(
                &input,
                &completion,
                &release,
                "Completed owned work",
            )
            .unwrap();
        assert_eq!(complete.completion, partial.completion);
        assert!(complete.release_recorded);
        let publication = complete
            .release_publication
            .as_ref()
            .unwrap()
            .publication
            .as_ref()
            .unwrap();
        assert_eq!(publication.state, PublicationState::Confirmed);
        assert!(publication.replayed);
        assert_eq!(publication.attempts.len(), 1);
        let released: ClaimChange =
            serde_json::from_value(complete.release.as_ref().unwrap().result.clone()).unwrap();
        assert_eq!(released.after.metadata.state, ClaimState::Released);
        assert_eq!(
            released.after.metadata.generation,
            input.expected_claim.generation + 1
        );
        assert_eq!(
            repo.complete_claimed_issue_and_release(
                &input,
                &completion,
                &release,
                "Completed owned work"
            )
            .unwrap()
            .release,
            complete.release
        );
    }

    #[test]
    fn release_cannot_rebind_to_a_mirror_after_completion_was_recorded() {
        let (temp, remote, repo, input) = fixture();
        let mirror = tempfile::tempdir().unwrap();
        git(
            mirror.path(),
            &["clone", "--mirror", remote.path().to_str().unwrap(), "."],
        );
        let original_coordination = git(
            remote.path(),
            &["rev-parse", "refs/heads/workdeck-coordination"],
        );
        assert_eq!(
            git(
                mirror.path(),
                &["rev-parse", "refs/heads/workdeck-coordination"]
            ),
            original_coordination
        );
        let completion_request = RequestId::new();
        let release_request = RequestId::new();
        let outcome = repo
            .complete_claimed_issue_and_release_with_faults(
                &input,
                &completion_request,
                &release_request,
                "Completed owned work",
                |_| {
                    git(
                        temp.path(),
                        &[
                            "remote",
                            "set-url",
                            "origin",
                            mirror.path().to_str().unwrap(),
                        ],
                    );
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(proof(&outcome.completion).after.metadata.status, "done");
        assert_eq!(
            outcome.release_error.as_ref().map(|error| error.code),
            Some(ErrorCode::StaleSource)
        );
        assert!(!outcome.release_recorded);
        assert!(outcome.release.is_none());
        assert_eq!(
            git(
                remote.path(),
                &["rev-parse", "refs/heads/workdeck-coordination"]
            ),
            original_coordination
        );
        assert_eq!(
            git(
                mirror.path(),
                &["rev-parse", "refs/heads/workdeck-coordination"]
            ),
            original_coordination
        );
        assert_eq!(
            repo.complete_claimed_issue(&input, &completion_request)
                .unwrap(),
            outcome.completion
        );
        // Restoring the original explicit binding permits the still-unpublished
        // release to finish under the same retained request.
        git(
            temp.path(),
            &[
                "remote",
                "set-url",
                "origin",
                remote.path().to_str().unwrap(),
            ],
        );
        let retry = repo
            .complete_claimed_issue_and_release(
                &input,
                &completion_request,
                &release_request,
                "Completed owned work",
            )
            .unwrap();
        assert_eq!(retry.completion, outcome.completion);
        assert!(retry.release_recorded);
        assert_eq!(
            git(
                mirror.path(),
                &["rev-parse", "refs/heads/workdeck-coordination"]
            ),
            original_coordination
        );
    }

    #[test]
    fn release_cannot_rebind_to_a_replaced_git_directory_with_identical_contents() {
        fn copy_tree(from: &Path, to: &Path) {
            fs::create_dir(to).unwrap();
            for entry in fs::read_dir(from).unwrap() {
                let entry = entry.unwrap();
                let kind = entry.file_type().unwrap();
                assert!(!kind.is_symlink());
                if kind.is_dir() {
                    copy_tree(&entry.path(), &to.join(entry.file_name()));
                } else {
                    fs::copy(entry.path(), to.join(entry.file_name())).unwrap();
                }
            }
        }
        let (temp, remote, repo, input) = fixture();
        let original_coordination = git(
            remote.path(),
            &["rev-parse", "refs/heads/workdeck-coordination"],
        );
        let original_git = temp.path().join(".git.original");
        let selected_git = temp.path().join(".git");
        let completion_request = RequestId::new();
        let release_request = RequestId::new();
        let outcome = repo
            .complete_claimed_issue_and_release_with_faults(
                &input,
                &completion_request,
                &release_request,
                "Completed owned work",
                |_| {
                    fs::rename(&selected_git, &original_git).unwrap();
                    copy_tree(&original_git, &selected_git);
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(
            outcome.release_error.as_ref().map(|error| error.code),
            Some(ErrorCode::StaleSource)
        );
        assert_eq!(proof(&outcome.completion).after.metadata.status, "done");
        assert!(!outcome.release_recorded);
        assert_eq!(
            git(
                remote.path(),
                &["rev-parse", "refs/heads/workdeck-coordination"]
            ),
            original_coordination
        );
        assert_eq!(
            repo.complete_claimed_issue(&input, &completion_request)
                .unwrap(),
            outcome.completion
        );
        fs::remove_dir_all(&selected_git).unwrap();
        fs::rename(&original_git, &selected_git).unwrap();
        let retry = repo
            .complete_claimed_issue_and_release(
                &input,
                &completion_request,
                &release_request,
                "Completed owned work",
            )
            .unwrap();
        assert!(retry.release_recorded);
        assert_eq!(retry.completion, outcome.completion);
    }

    #[test]
    fn inspected_binding_is_required_before_initial_shared_completion() {
        let (temp, remote, repo, mut input) = fixture();
        input.expected_binding = Some(repo.source_status().unwrap().binding.unwrap());
        let before = repo.show_issue(input.issue.as_str()).unwrap();
        let count = repo.operation_history().unwrap().len();
        let mirror = tempfile::tempdir().unwrap();
        git(
            mirror.path(),
            &["clone", "--mirror", remote.path().to_str().unwrap(), "."],
        );
        git(
            temp.path(),
            &[
                "remote",
                "set-url",
                "origin",
                mirror.path().to_str().unwrap(),
            ],
        );
        let request = RequestId::new();
        let error = repo.complete_claimed_issue(&input, &request).unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleSource);
        assert_eq!(repo.show_issue(input.issue.as_str()).unwrap(), before);
        assert_eq!(repo.operation_history().unwrap().len(), count);
        assert!(repo.pending_operations().unwrap().is_empty());
        git(
            temp.path(),
            &[
                "remote",
                "set-url",
                "origin",
                remote.path().to_str().unwrap(),
            ],
        );
        let original_config = fs::read(repo.root().join("config.yml")).unwrap();
        let mut local = repo.config().unwrap();
        local.sources = None;
        fs::write(
            repo.root().join("config.yml"),
            serde_yaml_ng::to_string(&local).unwrap(),
        )
        .unwrap();
        assert_eq!(
            repo.complete_claimed_issue(&input, &request)
                .unwrap_err()
                .code,
            ErrorCode::StaleSource
        );
        assert_eq!(repo.show_issue(input.issue.as_str()).unwrap(), before);
        fs::write(repo.root().join("config.yml"), original_config).unwrap();
        let receipt = repo.complete_claimed_issue(&input, &request).unwrap();
        assert_eq!(
            proof(&receipt).confirmation.unwrap().binding,
            input.expected_binding.unwrap()
        );
    }
}
