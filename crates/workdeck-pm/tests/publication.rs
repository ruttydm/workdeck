#![cfg(unix)]
use std::{fs, path::Path, process::Command};
use workdeck_pm::sources::{ProposalPlan, ProposalRequest};
use workdeck_pm::*;
fn git(root: &Path, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    for (key, _) in std::env::vars_os() {
        if key.to_str().is_some_and(|key| key.starts_with("GIT_")) {
            command.env_remove(key);
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
fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Repository, IssueId) {
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
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let create = CreateIssue::new("Implement", "accepted requirement");
    let receipt = repository.create_issue(&create, &RequestId::new()).unwrap();
    let issue: IssueRecord = serde_json::from_value(receipt.result).unwrap();
    let mut config = repository.config().unwrap();
    config.sources = Some(SharedSources {
        remote: "origin".into(),
        accepted_ref: "refs/heads/main".parse().unwrap(),
        coordination_ref: "refs/heads/workdeck-coordination".parse().unwrap(),
        proposal_namespace: "refs/heads/workdeck-proposals".parse().unwrap(),
    });
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    fs::write(temp.path().join("code.txt"), "accepted code").unwrap();
    git(temp.path(), &["add", "."]);
    git(temp.path(), &["commit", "-m", "accepted"]);
    git(temp.path(), &["push", "origin", "main"]);
    (temp, remote, repository, issue.metadata.id)
}
fn acquire(repo: &Repository, issue: &IssueId, actor: &str) -> ClaimRequest {
    ClaimRequest::Acquire {
        input: Box::new(AcquireClaim {
            actor: actor.into(),
            contract: repo.claim_contract(issue).unwrap(),
            ttl_seconds: None,
            recovery: None,
        }),
    }
}
#[test]
fn coordination_publication_preserves_developer_state_and_replays_original_receipt() {
    let (temp, remote, repo, issue) = fixture();
    fs::write(temp.path().join("code.txt"), "staged").unwrap();
    git(temp.path(), &["add", "code.txt"]);
    fs::write(temp.path().join("code.txt"), "unstaged").unwrap();
    let index = fs::read(temp.path().join(".git/index")).unwrap();
    let head = git(temp.path(), &["rev-parse", "HEAD"]);
    let request = acquire(&repo, &issue, "alice");
    let id = RequestId::new();
    let outcome = repo.mutate_claim(&request, &id).unwrap();
    assert!(outcome.may_continue, "{outcome:?}");
    assert_eq!(
        outcome.publication.as_ref().unwrap().state,
        PublicationState::Confirmed
    );
    let original = outcome.receipt.unwrap();
    let replay = repo.mutate_claim(&request, &id).unwrap();
    assert_eq!(replay.receipt.as_ref(), Some(&original));
    assert!(replay.publication.unwrap().replayed);
    assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(fs::read(temp.path().join("code.txt")).unwrap(), b"unstaged");
    assert!(!repo.root().join("claims").exists());
    let files = git(
        remote.path(),
        &[
            "ls-tree",
            "-r",
            "--name-only",
            "refs/heads/workdeck-coordination",
        ],
    );
    let files = String::from_utf8(files).unwrap();
    assert!(files.contains(".workdeck/coordination.yml"));
    assert!(files.contains(".workdeck/claims/"));
    assert!(!files.contains("code.txt"));
    assert!(!files.contains("issues/"));
}
#[test]
fn a_second_clone_cannot_acquire_the_confirmed_claim_and_old_replay_cannot_restore_ownership() {
    let (temp, remote, repo, issue) = fixture();
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &[
            "clone",
            "--branch",
            "main",
            remote.path().to_str().unwrap(),
            ".",
        ],
    );
    let second = Repository::open_source(&clone.path().join(".workdeck")).unwrap();
    let request = acquire(&repo, &issue, "alice");
    let other = acquire(&second, &issue, "bob");
    let id = RequestId::new();
    let first = repo.mutate_claim(&request, &id).unwrap();
    assert!(first.may_continue);
    assert!(second.mutate_claim(&other, &RequestId::new()).is_err());
    let current = first.current.unwrap().claim;
    let release = ClaimRequest::Mutate {
        issue: issue.clone(),
        expected: current.precondition(),
        mutation: ClaimMutation::Release {
            actor: "alice".into(),
            reason: "handover".into(),
        },
    };
    let released = repo.mutate_claim(&release, &RequestId::new()).unwrap();
    assert!(!released.may_continue);
    let second_claim = second.mutate_claim(&other, &RequestId::new()).unwrap();
    assert!(second_claim.may_continue);
    let replay = repo.mutate_claim(&request, &id).unwrap();
    assert!(!replay.may_continue);
    assert!(!replay.requested_token_current);
    assert!(replay.publication.unwrap().replayed);
    assert_eq!(
        git(temp.path(), &["rev-parse", "HEAD"]),
        git(temp.path(), &["rev-parse", "refs/heads/main"])
    );
}

#[test]
fn reviewed_proposal_publishes_only_planning_changes_and_keeps_accepted_application_bytes() {
    let (temp, remote, repo, issue) = fixture();
    use std::os::unix::fs::PermissionsExt;
    let protected = remote.path().join("hooks/update");
    fs::write(&protected, b"#!/bin/sh\n[ \"$1\" != refs/heads/main ]\n").unwrap();
    fs::set_permissions(&protected, fs::Permissions::from_mode(0o755)).unwrap();
    let old = repo.show_issue(issue.as_str()).unwrap();
    repo.update_issue(
        issue.as_str(),
        &old.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([(
                "title".into(),
                serde_json::json!("Proposed plan"),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    fs::write(temp.path().join("code.txt"), "local staged code").unwrap();
    git(temp.path(), &["add", "code.txt"]);
    fs::write(temp.path().join("code.txt"), "local unstaged code").unwrap();
    let index = fs::read(temp.path().join(".git/index")).unwrap();
    let head = git(temp.path(), &["rev-parse", "HEAD"]);
    let plan = repo
        .preview_proposal(&sources::ProposalRequest {
            reference: "refs/heads/workdeck-proposals/plan-one".parse().unwrap(),
            title: "Review this planning change".into(),
        })
        .unwrap();
    assert!(plan.changed.iter().any(|c| c.path == old.path));
    plan.validate().unwrap();
    let saved = repo.save_proposal_plan(&plan).unwrap();
    assert!(saved.starts_with(repo.root().join(".local")));
    assert_eq!(repo.load_proposal_plan(&plan.fingerprint).unwrap(), plan);
    let id = RequestId::new();
    let published = repo.publish_proposal(&plan, &id).unwrap();
    assert_eq!(published.state, PublicationState::Confirmed);
    assert_eq!(
        git(
            remote.path(),
            &["show", "refs/heads/workdeck-proposals/plan-one:code.txt"]
        ),
        b"accepted code"
    );
    let proposed = git(
        remote.path(),
        &[
            "show",
            &format!(
                "refs/heads/workdeck-proposals/plan-one:.workdeck/{}",
                old.path.display()
            ),
        ],
    );
    assert!(
        String::from_utf8(proposed)
            .unwrap()
            .contains("Proposed plan")
    );
    let accepted = git(
        remote.path(),
        &[
            "show",
            &format!("refs/heads/main:.workdeck/{}", old.path.display()),
        ],
    );
    assert!(
        !String::from_utf8(accepted)
            .unwrap()
            .contains("Proposed plan")
    );
    assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
    assert_eq!(git(temp.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(temp.path().join("code.txt")).unwrap(),
        b"local unstaged code"
    );
    let now = repo.show_issue(issue.as_str()).unwrap();
    repo.update_issue(
        issue.as_str(),
        &now.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([(
                "title".into(),
                serde_json::json!("Later local work"),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let replay = repo.publish_proposal(&plan, &id).unwrap();
    assert_eq!(replay.candidate, published.candidate);
    assert!(replay.replayed);
    assert_eq!(
        repo.resume_proposal(&id).unwrap().candidate,
        published.candidate
    );
}

fn edit_title(repo: &Repository, issue: &IssueId, title: &str) {
    let old = repo.show_issue(issue.as_str()).unwrap();
    repo.update_issue(
        issue.as_str(),
        &old.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([("title".into(), serde_json::json!(title))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
}
fn proposal(repo: &Repository, issue: &IssueId) -> ProposalPlan {
    edit_title(repo, issue, "Reviewed proposal");
    repo.preview_proposal(&ProposalRequest {
        reference: "refs/heads/workdeck-proposals/recovery".parse().unwrap(),
        title: "Exact reviewed content".into(),
    })
    .unwrap()
}

#[test]
fn interrupted_proposal_resumes_the_exact_candidate_after_later_local_edits() {
    for point in [
        PublicationFaultPoint::AfterPrepared,
        PublicationFaultPoint::AfterPush,
        PublicationFaultPoint::AfterConfirmation,
    ] {
        let (temp, remote, repo, issue) = fixture();
        let plan = proposal(&repo, &issue);
        let request = RequestId::new();
        let error = repo
            .publish_proposal_with_faults(&plan, &request, |at| {
                if at == point {
                    Err(PmError::new(
                        ErrorCode::RecoveryRequired,
                        "interrupted fixture",
                    ))
                } else {
                    Ok(())
                }
            })
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::RecoveryRequired);
        let saved = repo.proposal_status(&request).unwrap();
        edit_title(&repo, &issue, "Later local work must remain local");
        let index = fs::read(temp.path().join(".git/index")).unwrap();
        let resumed = repo.resume_proposal(&request).unwrap();
        assert_eq!(
            resumed.state,
            PublicationState::Confirmed,
            "{point:?}: {resumed:?}"
        );
        assert_eq!(resumed.candidate, saved.candidate);
        assert_eq!(resumed.operation_id, saved.operation_id);
        assert!(resumed.replayed);
        let remote_doc = git(
            remote.path(),
            &[
                "show",
                &format!("{}:.workdeck/issues/{}/item.md", plan.reference, issue),
            ],
        );
        let remote_doc = String::from_utf8(remote_doc).unwrap();
        assert!(remote_doc.contains("Reviewed proposal"));
        assert!(!remote_doc.contains("Later local work"));
        assert_eq!(
            repo.show_issue(issue.as_str()).unwrap().metadata.title,
            "Later local work must remain local"
        );
        assert_eq!(fs::read(temp.path().join(".git/index")).unwrap(), index);
    }
}

#[test]
fn uncertain_proposal_with_unchanged_remote_waits_for_exact_candidate_reconciliation() {
    let (temp, remote, repo, issue) = fixture();
    let plan = proposal(&repo, &issue);
    let request = RequestId::new();
    repo.publish_proposal_with_faults(&plan, &request, |at| {
        if at == PublicationFaultPoint::BeforePush {
            Err(PmError::new(
                ErrorCode::RecoveryRequired,
                "spawn outcome unknown",
            ))
        } else {
            Ok(())
        }
    })
    .unwrap_err();
    let saved = repo.proposal_status(&request).unwrap();
    assert_eq!(saved.state, PublicationState::Uncertain);
    let unchanged = git(remote.path(), &["for-each-ref", plan.reference.as_str()]);
    assert!(unchanged.is_empty());
    let resumed = repo.resume_proposal(&request).unwrap();
    assert_eq!(resumed.state, PublicationState::Uncertain);
    assert_eq!(resumed.candidate, saved.candidate);
    assert_eq!(
        git(remote.path(), &["for-each-ref", plan.reference.as_str()]),
        unchanged
    );
    // A previously uncertain send may arrive later. A retry observes it; it never sends a replacement.
    git(
        temp.path(),
        &[
            "push",
            "origin",
            &format!("{}:{}", saved.candidate, plan.reference),
        ],
    );
    let confirmed = repo.resume_proposal(&request).unwrap();
    assert_eq!(confirmed.state, PublicationState::Confirmed);
    assert_eq!(confirmed.candidate, saved.candidate);
    assert_eq!(confirmed.operation_id, saved.operation_id);
}

#[test]
fn proposal_preview_binds_existing_changes_and_rejects_a_concurrent_remote_advance() {
    let (_temp, remote, repo, issue) = fixture();
    let first = proposal(&repo, &issue);
    let first_outcome = repo.publish_proposal(&first, &RequestId::new()).unwrap();
    edit_title(&repo, &issue, "Second reviewed proposal");
    let second = repo
        .preview_proposal(&ProposalRequest {
            reference: first.reference.clone(),
            title: "Update proposal".into(),
        })
        .unwrap();
    assert_eq!(
        second.expected_proposal.as_ref().unwrap().commit.as_ref(),
        Some(&first_outcome.candidate)
    );
    let issue_path = repo.show_issue(issue.as_str()).unwrap().path;
    let accepted_change = second
        .changed
        .iter()
        .find(|change| change.path == issue_path)
        .unwrap();
    let existing_change = second
        .proposal_changed
        .iter()
        .find(|change| change.path == issue_path)
        .unwrap();
    assert_ne!(accepted_change.before, existing_change.before);
    assert_eq!(accepted_change.after, existing_change.after);
    let clone = tempfile::tempdir().unwrap();
    git(
        clone.path(),
        &[
            "clone",
            "--branch",
            "workdeck-proposals/recovery",
            remote.path().to_str().unwrap(),
            ".",
        ],
    );
    git(clone.path(), &["config", "user.name", "Concurrent fixture"]);
    git(
        clone.path(),
        &["config", "user.email", "concurrent@example.invalid"],
    );
    let other = Repository::open_source(&clone.path().join(".workdeck")).unwrap();
    edit_title(&other, &issue, "Concurrent remote change");
    git(clone.path(), &["add", ".workdeck"]);
    git(
        clone.path(),
        &["commit", "-m", "Concurrent planning change"],
    );
    git(clone.path(), &["push", "origin", first.reference.as_str()]);
    let remote_before = git(remote.path(), &["rev-parse", first.reference.as_str()]);
    let request = RequestId::new();
    let error = repo.publish_proposal(&second, &request).unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(
        git(remote.path(), &["rev-parse", first.reference.as_str()]),
        remote_before
    );
    assert_eq!(
        repo.proposal_status(&request).unwrap().state,
        PublicationState::Rejected
    );
}

#[test]
fn reviewed_proposal_rejects_a_remote_url_change_even_when_accepted_contents_match() {
    let (temp, remote, repo, issue) = fixture();
    let plan = proposal(&repo, &issue);
    let replacement = tempfile::tempdir().unwrap();
    git(
        replacement.path(),
        &["clone", "--mirror", remote.path().to_str().unwrap(), "."],
    );
    git(
        temp.path(),
        &[
            "remote",
            "set-url",
            "origin",
            replacement.path().to_str().unwrap(),
        ],
    );
    let before = git(replacement.path(), &["for-each-ref"]);
    let error = repo.publish_proposal(&plan, &RequestId::new()).unwrap_err();
    assert_eq!(error.code, ErrorCode::StaleSource);
    assert_eq!(git(replacement.path(), &["for-each-ref"]), before);
}
