use assert_cmd::prelude::*;
use serde_json::Value;
use std::{
    path::Path,
    process::{Command, Output},
};
use workdeck_pm::{CreateIssue, IssueRecord, Repository, RequestId};
fn run(root: &Path, args: &[&str]) -> Output {
    Command::cargo_bin("workdeck")
        .unwrap()
        .current_dir(root)
        .env("XDG_CONFIG_HOME", root.join("isolated-config"))
        .args(args)
        .output()
        .unwrap()
}
fn value(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {:?}", output))
}
#[test]
fn saved_contract_acquire_replay_and_release_keep_receipt_separate_from_current_ownership() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new("Pick up work", ""), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let contract = run(
        temp.path(),
        &["claim", "contract", issue.metadata.id.as_str(), "--json"],
    );
    assert!(
        contract.status.success(),
        "claim contract failed: {:?}",
        contract
    );
    let data = value(&contract);
    let fingerprint = data["result"]["fingerprint"].as_str().unwrap();
    let request = RequestId::new().to_string();
    let args = [
        "claim",
        "acquire",
        "--contract",
        fingerprint,
        "--expected-contract",
        fingerprint,
        "--actor",
        "agent",
        "--request-id",
        &request,
        "--json",
        "--no-input",
    ];
    let acquired = run(temp.path(), &args);
    assert!(acquired.status.success(), "{:?}", value(&acquired));
    let first = value(&acquired);
    assert_eq!(first["result"]["may_continue"], true);
    assert_eq!(
        first["result"]["current"]["assessment"]["guarantee"],
        "local_source_only"
    );
    let replay = value(&run(temp.path(), &args));
    assert_eq!(
        first["result"]["receipt"]["operation_id"],
        replay["result"]["receipt"]["operation_id"]
    );
    let claim = &first["result"]["current"]["claim"];
    let token = claim["metadata"]["token"].as_str().unwrap();
    let generation = claim["metadata"]["generation"].to_string();
    let content = claim["source"]["content"].as_str().unwrap();
    let released = run(
        temp.path(),
        &[
            "claim",
            "release",
            issue.metadata.id.as_str(),
            "--token",
            token,
            "--generation",
            &generation,
            "--expected-content",
            content,
            "--actor",
            "agent",
            "--reason",
            "handoff",
            "--request-id",
            &RequestId::new().to_string(),
            "--json",
        ],
    );
    assert!(released.status.success(), "{:?}", value(&released));
    let old = run(temp.path(), &args);
    assert!(
        !old.status.success(),
        "historical acquire must not authorize continued work"
    );
    let old = value(&old);
    assert_eq!(
        old["result"]["receipt"]["operation_id"],
        first["result"]["receipt"]["operation_id"]
    );
    assert_eq!(old["result"]["requested_token_current"], false);
    assert_eq!(old["result"]["may_continue"], false);
    assert!(repository.doctor().unwrap().valid);
}

#[test]
fn claim_pages_reject_changed_membership_and_saved_contract_tampering() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    for title in ["First", "Second"] {
        let issue: IssueRecord = serde_json::from_value(
            repository
                .create_issue(&CreateIssue::new(title, ""), &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        let contract = repository.local_claim_contract(&issue.metadata.id).unwrap();
        repository
            .mutate_local_claim(
                &workdeck_pm::ClaimRequest::Acquire {
                    input: Box::new(workdeck_pm::AcquireClaim {
                        actor: "agent".into(),
                        contract,
                        ttl_seconds: None,
                        recovery: None,
                    }),
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    let first = run(temp.path(), &["claim", "list", "--limit", "1", "--json"]);
    assert!(first.status.success(), "{first:?}");
    let first = value(&first);
    assert_eq!(first["result"]["records"].as_array().unwrap().len(), 1);
    let cursor = first["result"]["next_cursor"].as_str().unwrap();
    let next = run(
        temp.path(),
        &[
            "claim", "list", "--limit", "1", "--cursor", cursor, "--json",
        ],
    );
    assert!(next.status.success(), "{next:?}");
    let records = repository.local_claims().unwrap();
    let current = &records[0].claim;
    repository
        .mutate_local_claim(
            &workdeck_pm::ClaimRequest::Mutate {
                issue: current.metadata.issue.clone(),
                expected: current.precondition(),
                mutation: workdeck_pm::ClaimMutation::Release {
                    actor: "agent".into(),
                    reason: "handoff".into(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let stale = run(
        temp.path(),
        &[
            "claim", "list", "--limit", "1", "--cursor", cursor, "--json",
        ],
    );
    assert_eq!(stale.status.code(), Some(4), "{stale:?}");
    let contract = repository
        .local_claim_contract(&current.metadata.issue)
        .unwrap();
    let saved = repository.save_claim_contract(&contract).unwrap();
    let hash = contract.fingerprint().unwrap().to_string();
    let mut modified = serde_json::to_value(&contract).unwrap();
    modified["requirements"] = serde_json::json!(workdeck_pm::ContentHash::of(b"tampered"));
    std::fs::write(saved, serde_json::to_vec(&modified).unwrap()).unwrap();
    let tampered = run(
        temp.path(),
        &[
            "claim",
            "acquire",
            "--contract",
            &hash,
            "--expected-contract",
            &hash,
            "--actor",
            "agent",
            "--request-id",
            &RequestId::new().to_string(),
            "--json",
        ],
    );
    assert_eq!(tampered.status.code(), Some(4), "{tampered:?}");
    assert_eq!(
        repository.local_claims().unwrap()[0].claim.metadata.state,
        workdeck_pm::ClaimState::Released
    );
}

#[test]
fn claimed_completion_and_explicit_release_have_separate_receipts_and_retry_inputs() {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Complete owned work", ""),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let contract = repository.claim_contract(&issue.metadata.id).unwrap();
    repository.save_claim_contract(&contract).unwrap();
    let hash = contract.fingerprint().unwrap().to_string();
    let claim: workdeck_pm::ClaimChange = serde_json::from_value(
        repository
            .mutate_local_claim(
                &workdeck_pm::ClaimRequest::Acquire {
                    input: Box::new(workdeck_pm::AcquireClaim {
                        actor: "agent".into(),
                        contract,
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
    let claim = claim.after;
    let generation = claim.metadata.generation.to_string();
    let content = claim.source.content.to_string();
    let revision = issue.metadata.revision.get().to_string();
    let issue_content = issue.source.content.to_string();
    let completion = RequestId::new().to_string();
    let release = RequestId::new().to_string();
    let args = [
        "claim",
        "complete",
        issue.metadata.id.as_str(),
        "--token",
        claim.metadata.token.as_str(),
        "--generation",
        &generation,
        "--expected-content",
        &content,
        "--expected-issue-revision",
        &revision,
        "--expected-issue-content",
        &issue_content,
        "--contract",
        &hash,
        "--expected-contract",
        &hash,
        "--actor",
        "agent",
        "--request-id",
        &completion,
        "--release-request-id",
        &release,
        "--release-reason",
        "completed",
        "--json",
    ];
    let output = run(temp.path(), &args);
    assert!(output.status.success(), "{output:?}");
    let data = value(&output);
    assert_eq!(data["result"]["release_recorded"], true);
    assert_eq!(data["result"]["completion"]["request_id"], completion);
    assert_eq!(data["result"]["release"]["request_id"], release);
    assert_ne!(
        data["result"]["completion"]["operation_id"],
        data["result"]["release"]["operation_id"]
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .status,
        "done"
    );
    assert_eq!(
        repository.local_claims().unwrap()[0].claim.metadata.state,
        workdeck_pm::ClaimState::Released
    );
    let again = run(temp.path(), &args);
    assert!(again.status.success(), "{again:?}");
    assert_eq!(value(&again)["result"], data["result"]);
    assert!(repository.doctor().unwrap().valid);
}
