#![cfg(unix)]
use workdeck_pm::{registry::*, *};

#[test]
fn claimed_work_uses_claim_actor_and_keeps_expired_claims_visible_for_recovery() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut claims = Vec::new();
    for (title, actor) in [
        ("Claimed A", "agent-a"),
        ("Claimed B", "agent-a"),
        ("Other actor", "other"),
    ] {
        let issue: IssueRecord = serde_json::from_value(
            repository
                .create_issue(&CreateIssue::new(title, "Work contract"), &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap();
        let receipt = repository
            .mutate_local_claim(
                &ClaimRequest::Acquire {
                    input: Box::new(AcquireClaim {
                        actor: actor.into(),
                        contract: repository.local_claim_contract(&issue.metadata.id).unwrap(),
                        ttl_seconds: None,
                        recovery: None,
                    }),
                },
                &RequestId::new(),
            )
            .unwrap();
        claims.push(
            serde_json::from_value::<ClaimChange>(receipt.result)
                .unwrap()
                .after,
        );
    }
    let store = RegistryStore::open(&repository).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(
                        "self",
                        directory.path(),
                        SourceSelector::WorkingTree,
                    )
                    .unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let input: MyWorkRequest = serde_json::from_value(serde_json::json!({"assignee":"agent-a", "facet":"claimed", "as_of":chrono::Utc::now(), "limit":1})).unwrap();
    let report = store.my_work(&input).unwrap();
    assert!(report.all_sources_available, "{report:?}");
    assert_eq!(report.known_total, 2);
    assert!(report.rows[0].row.assignee.is_none());
    assert_eq!(
        report.rows[0].evidence.selected_requirements_match,
        Some(true)
    );
    let encoded = serde_json::to_value(&report).unwrap();
    assert_eq!(
        encoded["rows"][0]["evidence"]["claim"]["assessment"]["guarantee"],
        "local_source_only"
    );
    let page = store
        .my_work(&MyWorkRequest {
            cursor: report.next_cursor.clone(),
            ..input.clone()
        })
        .unwrap();
    assert_ne!(report.rows[0].row.token.key, page.rows[0].row.token.key);
    let expired = store
        .my_work(&MyWorkRequest {
            as_of: Some(claims[1].metadata.expires_at + chrono::Duration::seconds(1)),
            ..input.clone()
        })
        .unwrap();
    assert_eq!(expired.known_total, 2);
    assert_eq!(
        serde_json::to_value(&expired).unwrap()["rows"][0]["evidence"]["claim"]["assessment"]["disposition"],
        "clock_uncertain"
    );
    let outside_skew = store
        .my_work(&MyWorkRequest {
            as_of: Some(
                claims[1].metadata.expires_at
                    + chrono::Duration::seconds(
                        ClaimPolicy::default().max_clock_skew_seconds as i64 + 1,
                    ),
            ),
            ..input.clone()
        })
        .unwrap();
    assert_eq!(outside_skew.known_total, 2);
    assert_eq!(
        serde_json::to_value(&outside_skew).unwrap()["rows"][0]["evidence"]["claim"]["assessment"]
            ["disposition"],
        "expired"
    );
    let raced = store
        .my_work_with_faults(&input, |point| {
            if point == MyWorkFaultPoint::BeforeEvidenceRevalidation {
                repository.mutate_local_claim(
                    &ClaimRequest::Mutate {
                        issue: claims[0].metadata.issue.clone(),
                        expected: claims[0].precondition(),
                        mutation: ClaimMutation::Release {
                            actor: "agent-a".into(),
                            reason: "Handing off this work".into(),
                        },
                    },
                    &RequestId::new(),
                )?;
            }
            Ok(())
        })
        .unwrap();
    assert!(!raced.all_sources_available);
    assert_eq!(raced.known_total, 0);
    assert_eq!(
        raced.sources[0].error.as_ref().unwrap().code,
        ErrorCode::StaleSource
    );
    let after_release = store.my_work(&input).unwrap();
    assert!(after_release.all_sources_available);
    assert_eq!(after_release.known_total, 1);
    assert_eq!(
        after_release.rows[0]
            .evidence
            .claim
            .as_ref()
            .unwrap()
            .claim
            .metadata
            .token,
        claims[1].metadata.token
    );
    let missing: MyWorkRequest =
        serde_json::from_value(serde_json::json!({"assignee":"agent-a", "facet":"claimed"}))
            .unwrap();
    assert_eq!(
        store.my_work(&missing).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

fn git(root: &std::path::Path, args: &[&str]) -> Vec<u8> {
    let mut command = std::process::Command::new("git");
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

#[test]
fn shared_claims_keep_coordination_and_proposed_requirements_separate_and_reject_races() {
    use std::fs;
    let directory = tempfile::tempdir().unwrap();
    let remote = tempfile::tempdir().unwrap();
    git(directory.path(), &["init", "--quiet", "-b", "main"]);
    git(remote.path(), &["init", "--quiet", "--bare"]);
    git(directory.path(), &["config", "user.name", "Fixture"]);
    git(
        directory.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    git(
        directory.path(),
        &["remote", "add", "origin", remote.path().to_str().unwrap()],
    );
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut issues = Vec::new();
    for title in ["Accepted A", "Accepted B"] {
        issues.push(
            serde_json::from_value::<IssueRecord>(
                repository
                    .create_issue(
                        &CreateIssue::new(title, "Accepted contract"),
                        &RequestId::new(),
                    )
                    .unwrap()
                    .result,
            )
            .unwrap(),
        );
    }
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
    git(directory.path(), &["add", ".workdeck"]);
    git(
        directory.path(),
        &["commit", "--quiet", "-m", "Accepted planning"],
    );
    git(directory.path(), &["push", "--quiet", "origin", "main"]);
    let mut claims = Vec::new();
    for issue in &issues {
        let outcome = repository
            .mutate_claim(
                &ClaimRequest::Acquire {
                    input: Box::new(AcquireClaim {
                        actor: "Ada".into(),
                        contract: repository.claim_contract(&issue.metadata.id).unwrap(),
                        ttl_seconds: None,
                        recovery: None,
                    }),
                },
                &RequestId::new(),
            )
            .unwrap();
        assert_eq!(
            outcome.publication.as_ref().unwrap().state,
            PublicationState::Confirmed
        );
        claims.push(outcome.current.unwrap().claim);
    }
    repository
        .update_issue(
            issues[0].metadata.id.as_str(),
            &issues[0].source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "title".into(),
                    serde_json::json!("Proposed A"),
                )]),
                body: Some("Proposed different requirements".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    fs::write(directory.path().join("code.txt"), "staged\n").unwrap();
    git(directory.path(), &["add", "code.txt"]);
    fs::write(directory.path().join("code.txt"), "unstaged\n").unwrap();
    let index = fs::read(directory.path().join(".git/index")).unwrap();
    let head = git(directory.path(), &["rev-parse", "HEAD"]);
    let bytes = fs::read(repository.root().join(&issues[0].path)).unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(
                        "proposal",
                        directory.path(),
                        SourceSelector::WorkingTree,
                    )
                    .unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let input: MyWorkRequest = serde_json::from_value(serde_json::json!({"assignee":"Ada", "facet":"claimed", "as_of":chrono::Utc::now(), "limit":1})).unwrap();
    let first = store.my_work(&input).unwrap();
    assert!(first.all_sources_available, "{first:?}");
    assert_eq!(first.known_total, 2);
    assert_eq!(first.rows[0].row.title, "Proposed A");
    let claim = first.rows[0].evidence.claim.as_ref().unwrap();
    assert_eq!(claim.assessment.guarantee, ClaimGuarantee::Unconfirmed);
    assert!(!claim.assessment.may_continue);
    assert_eq!(
        claim.claim.metadata.contract.accepted_source.role,
        SourceRole::Accepted
    );
    assert_eq!(
        first.rows[0].row.token.view.source.role,
        SourceRole::Proposal
    );
    assert_eq!(
        first.rows[0].evidence.selected_requirements_match,
        Some(false)
    );
    assert_eq!(first.sources[0].evidence_sources.len(), 3);
    let raced = store
        .my_work_with_faults(&input, |point| {
            if point == MyWorkFaultPoint::BeforeEvidenceRevalidation {
                repository.mutate_claim(
                    &ClaimRequest::Mutate {
                        issue: claims[1].metadata.issue.clone(),
                        expected: claims[1].precondition(),
                        mutation: ClaimMutation::Renew {
                            actor: "Ada".into(),
                            ttl_seconds: Some(3600),
                        },
                    },
                    &RequestId::new(),
                )?;
            }
            Ok(())
        })
        .unwrap();
    assert!(!raced.all_sources_available);
    assert_eq!(
        raced.sources[0].error.as_ref().unwrap().code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        store
            .my_work(&MyWorkRequest {
                cursor: first.next_cursor,
                ..input.clone()
            })
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        fs::read(directory.path().join(".git/index")).unwrap(),
        index
    );
    assert_eq!(git(directory.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(
        fs::read(repository.root().join(&issues[0].path)).unwrap(),
        bytes
    );
    assert_eq!(
        fs::read(directory.path().join("code.txt")).unwrap(),
        b"unstaged\n"
    );
    config.sources.as_mut().unwrap().coordination_ref =
        "refs/heads/missing-coordination".parse().unwrap();
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    let missing = store.my_work(&input).unwrap();
    assert!(!missing.all_sources_available);
    assert_eq!(missing.known_total, 0);
    assert!(missing.sources[0].error.is_some());
}
