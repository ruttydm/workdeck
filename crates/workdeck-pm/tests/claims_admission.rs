use serde_json::json;
use std::collections::BTreeMap;
use workdeck_pm::*;

fn fixture() -> (tempfile::TempDir, Repository) {
    let temp = tempfile::tempdir().unwrap();
    let repo = Repository::init(temp.path(), "WD").unwrap();
    (temp, repo)
}

fn issue(repo: &Repository, fields: BTreeMap<String, serde_json::Value>) -> IssueRecord {
    let mut input = CreateIssue::new("Ready scoped work", "Implement the agreed behavior.\n");
    input.fields = fields;
    input.fields.insert("status".into(), json!("ready"));
    serde_json::from_value(repo.create_issue(&input, &RequestId::new()).unwrap().result).unwrap()
}

fn acquire(repo: &Repository, issue: &IssueId, actor: &str) -> ClaimRequest {
    ClaimRequest::Acquire {
        input: Box::new(AcquireClaim {
            actor: actor.into(),
            contract: repo.local_claim_contract(issue).unwrap(),
            ttl_seconds: None,
            recovery: None,
        }),
    }
}

fn claim(receipt: workdeck_pm::transactions::MutationReceipt) -> ClaimRecord {
    serde_json::from_value::<ClaimChange>(receipt.result)
        .unwrap()
        .after
}

fn registered_user(repo: &Repository, id: &str) {
    repo.mutate_user(
        id,
        None,
        &UserMutation::Create {
            user: UserDefinition::new(id),
        },
        &RequestId::new(),
    )
    .unwrap();
}

#[test]
fn registered_identity_policy_rejects_unknown_claim_actor_without_publication() {
    let (_temp, repo) = fixture();
    let issue = issue(&repo, BTreeMap::new());
    registered_user(&repo, "owner");
    repo.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    let before = repo.operation_history().unwrap();
    let error = repo
        .mutate_local_claim(
            &acquire(&repo, &issue.metadata.id, "unknown"),
            &RequestId::new(),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyBlocked);
    assert_eq!(repo.operation_history().unwrap(), before);
    assert!(repo.local_claims().unwrap().is_empty());
    let owned = claim(
        repo.mutate_local_claim(
            &acquire(&repo, &issue.metadata.id, "owner"),
            &RequestId::new(),
        )
        .unwrap(),
    );
    assert_eq!(owned.metadata.actor, "owner");
    assert!(repo.local_claims().unwrap()[0].assessment.may_continue);
}

#[test]
fn registered_identity_policy_rejects_archived_claim_actor_without_publication() {
    let (_temp, repo) = fixture();
    let issue = issue(&repo, BTreeMap::new());
    registered_user(&repo, "archived-owner");
    registered_user(&repo, "active-owner");
    repo.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())
        .unwrap();
    repo.mutate_user(
        "archived-owner",
        None,
        &UserMutation::Archive { archived: true },
        &RequestId::new(),
    )
    .unwrap();
    let before = repo.operation_history().unwrap();
    let error = repo
        .mutate_local_claim(
            &acquire(&repo, &issue.metadata.id, "archived-owner"),
            &RequestId::new(),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyBlocked);
    assert_eq!(repo.operation_history().unwrap(), before);
    assert!(repo.local_claims().unwrap().is_empty());
    let owned = claim(
        repo.mutate_local_claim(
            &acquire(&repo, &issue.metadata.id, "active-owner"),
            &RequestId::new(),
        )
        .unwrap(),
    );
    assert_eq!(owned.metadata.actor, "active-owner");
}

fn planning(
    repo: &Repository,
    kind: PlanningKind,
    id: &str,
    fields: BTreeMap<String, serde_json::Value>,
) -> PlanningRecord {
    serde_json::from_value(
        repo.create_planning(
            kind,
            &CreatePlanning {
                id: Some(id.into()),
                name: id.into(),
                body: String::new(),
                fields,
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}

fn feature_subject(repo: &Repository) -> (IssueRecord, QuestionSubject) {
    let feature: FeatureOutcome = serde_json::from_value(
        repo.create_feature(&CreateFeature::new("Linked capability"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let record = feature.record;
    let issue = issue(
        repo,
        BTreeMap::from([("features".into(), json!([record.metadata.id]))]),
    );
    (
        issue,
        QuestionSubject {
            subject: SubjectRef::Feature(record.metadata.id),
            source: record.source,
        },
    )
}

fn milestone_subject(repo: &Repository) -> (IssueRecord, QuestionSubject) {
    planning(repo, PlanningKind::Project, "delivery", BTreeMap::new());
    let milestone = planning(
        repo,
        PlanningKind::Milestone,
        "checkpoint",
        BTreeMap::from([("project".into(), json!("delivery"))]),
    );
    let issue = issue(
        repo,
        BTreeMap::from([
            ("project".into(), json!("delivery")),
            ("milestone".into(), json!(milestone.metadata.id)),
        ]),
    );
    (
        issue,
        QuestionSubject {
            subject: SubjectRef::Milestone(milestone.metadata.id),
            source: milestone.source,
        },
    )
}

fn gate_subject(repo: &Repository) -> (IssueRecord, QuestionSubject) {
    let criterion_owner = issue(
        repo,
        BTreeMap::from([(
            "acceptance".into(),
            json!([
                {"id":"behavior", "description":"The expected behavior is demonstrated", "checked":false}
            ]),
        )]),
    );
    let criterion = repo
        .resolve_criterion(
            &CriterionOwner::Issue(criterion_owner.metadata.id),
            "behavior",
        )
        .unwrap()
        .reference;
    let gate: GateMutationResult = serde_json::from_value(
        repo.create_gate(
            &CreateGate {
                name: "Local contract".into(),
                description: String::new(),
                requirements: vec![GateRequirement {
                    id: "behavior".into(),
                    criterion,
                    producer: ProducerRef {
                        id: "runner".into(),
                        definition: ContentHash::of(b"declared runner"),
                    },
                    check: CheckRef {
                        id: "behavior".into(),
                        definition: ContentHash::of(b"declared check"),
                    },
                    max_age_seconds: None,
                }],
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let record = gate.gate;
    let issue = issue(
        repo,
        BTreeMap::from([("gates".into(), json!([record.definition.id]))]),
    );
    (
        issue,
        QuestionSubject {
            subject: SubjectRef::Gate(record.definition.id),
            source: record.source,
        },
    )
}

fn linked_question_blocks_claim(build: fn(&Repository) -> (IssueRecord, QuestionSubject)) {
    let (_temp, repo) = fixture();
    let (issue, subject) = build(&repo);
    let issue_bytes = std::fs::read(repo.root().join(&issue.path)).unwrap();
    // The association itself permits ownership before the explicit blocking question.
    let owned = claim(
        repo.mutate_local_claim(
            &acquire(&repo, &issue.metadata.id, "worker"),
            &RequestId::new(),
        )
        .unwrap(),
    );
    let released = claim(
        repo.mutate_local_claim(
            &ClaimRequest::Mutate {
                issue: issue.metadata.id.clone(),
                expected: owned.precondition(),
                mutation: ClaimMutation::Release {
                    actor: "worker".into(),
                    reason: "Ready for the next explicit pickup".into(),
                },
            },
            &RequestId::new(),
        )
        .unwrap(),
    );
    let question: QuestionMutationResult = serde_json::from_value(
        repo.create_question(
            &CreateQuestion {
                actor: "reviewer".into(),
                body: "Resolve this linked contract decision before implementation.".into(),
                subjects: vec![subject],
                requirements: vec![],
                blocks_work: true,
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    let selection = repo.next_issue(&NextIssueRequest::default()).unwrap();
    let candidate = selection
        .candidates
        .iter()
        .find(|candidate| candidate.issue == issue.metadata.id)
        .unwrap();
    assert!(!candidate.eligible);
    assert!(
        candidate
            .reason_codes
            .iter()
            .any(|code| code == "open_question")
    );
    let before = repo.operation_history().unwrap();
    let error = repo
        .mutate_local_claim(
            &acquire(&repo, &issue.metadata.id, "worker"),
            &RequestId::new(),
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyBlocked);
    assert_eq!(repo.operation_history().unwrap(), before);
    assert_eq!(repo.local_claims().unwrap()[0].claim, released);
    assert_eq!(
        std::fs::read(repo.root().join(&issue.path)).unwrap(),
        issue_bytes
    );

    repo.mutate_question(
        &question.question.metadata.id,
        &question.question.source,
        &QuestionMutation::Answer {
            actor: "reviewer".into(),
            body: "Proceed with the declared behavior.".into(),
            decision_refs: vec![],
        },
        &RequestId::new(),
    )
    .unwrap();
    let resumed = claim(
        repo.mutate_local_claim(
            &acquire(&repo, &issue.metadata.id, "worker"),
            &RequestId::new(),
        )
        .unwrap(),
    );
    assert_eq!(
        resumed.metadata.generation,
        released.metadata.generation + 1
    );
}

#[test]
fn linked_feature_blocking_question_excludes_claim_acquisition() {
    linked_question_blocks_claim(feature_subject);
}

#[test]
fn linked_gate_blocking_question_excludes_claim_acquisition() {
    linked_question_blocks_claim(gate_subject);
}

#[test]
fn linked_milestone_blocking_question_excludes_claim_acquisition() {
    linked_question_blocks_claim(milestone_subject);
}
