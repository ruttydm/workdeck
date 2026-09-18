#![cfg(unix)]
use std::fs;
use workdeck_pm::{registry::*, *};

fn issue(repository: &Repository, title: &str) -> IssueRecord {
    let mut input = CreateIssue::new(title, "Work contract");
    input
        .fields
        .insert("assignee".into(), serde_json::json!("agent-a"));
    serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}
fn register(store: &RegistryStore, alias: &str, path: &std::path::Path) {
    let request = RegistryRequest {
        expected: store.snapshot().unwrap().source,
        mutation: RegistryMutation::Register {
            checkout: inspect_checkout(alias, path, SourceSelector::WorkingTree).unwrap(),
        },
    };
    store.mutate(&request, &RequestId::new()).unwrap();
}
fn request(limit: usize) -> MyWorkRequest {
    MyWorkRequest {
        assignee: "agent-a".into(),
        limit,
        ..Default::default()
    }
}

#[test]
fn identical_issue_ids_in_registered_repositories_remain_qualified_and_page_consistently() {
    let owner = tempfile::tempdir().unwrap();
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let left_repo = Repository::init(left.path(), "WD").unwrap();
    let right_repo = Repository::init(right.path(), "WD").unwrap();
    let record = issue(&left_repo, "Shared-looking identity");
    let destination = right_repo.root().join(&record.path);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::copy(left_repo.root().join(&record.path), destination).unwrap();
    let store = RegistryStore::open(&owner_repo).unwrap();
    register(&store, "a", left.path());
    register(&store, "b", right.path());
    let first = store.my_work(&request(1)).unwrap();
    assert!(first.all_sources_available, "{first:?}");
    assert_eq!(first.known_total, 2);
    assert_eq!(first.rows.len(), 1);
    assert_eq!(first.rows[0].alias, "a");
    let second = store
        .my_work(&MyWorkRequest {
            cursor: first.next_cursor.clone(),
            ..request(1)
        })
        .unwrap();
    assert!(second.all_sources_available, "{second:?}");
    assert_eq!(second.rows[0].alias, "b");
    assert_eq!(
        first.rows[0].row.token.key.id,
        second.rows[0].row.token.key.id
    );
    assert_ne!(
        first.rows[0].row.token.key.repository,
        second.rows[0].row.token.key.repository
    );
    assert_ne!(
        first.rows[0].row.token.view.slot,
        second.rows[0].row.token.view.slot
    );
    assert!(second.next_cursor.is_none());
}

#[test]
fn source_changes_invalidate_a_cross_repository_cursor() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    let first_issue = issue(&target_repo, "First");
    issue(&target_repo, "Second");
    let store = RegistryStore::open(&owner_repo).unwrap();
    register(&store, "selected", target.path());
    let first = store.my_work(&request(1)).unwrap();
    assert!(first.next_cursor.is_some(), "{first:?}");
    let path = target_repo.root().join(first_issue.path);
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(b"\nEditor change\n");
    fs::write(path, bytes).unwrap();
    let result = store.my_work(&MyWorkRequest {
        cursor: first.next_cursor,
        ..request(1)
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::StaleSource);
}

#[test]
fn unavailable_sources_remain_explicit_and_never_become_an_empty_success() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    issue(&target_repo, "Missing");
    let store = RegistryStore::open(&owner_repo).unwrap();
    register(&store, "missing", target.path());
    fs::remove_dir_all(target_repo.root()).unwrap();
    let report = store.my_work(&request(20)).unwrap();
    assert!(!report.all_sources_available);
    assert_eq!(report.sources.len(), 1);
    assert!(report.sources[0].error.is_some());
    assert_eq!(report.sources[0].matches, None);
    assert!(report.rows.is_empty());
    assert!(!target_repo.root().exists());
}

#[test]
fn explicit_alias_filters_do_not_discover_or_initialize_other_sources() {
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    issue(&target_repo, "Selected");
    let store = RegistryStore::open(&owner_repo).unwrap();
    register(&store, "selected", target.path());
    let result = store.my_work(&MyWorkRequest {
        aliases: vec!["unregistered".into()],
        ..request(20)
    });
    assert_eq!(result.unwrap_err().code, ErrorCode::NotFound);
    assert_eq!(
        fs::read_dir(target_repo.root().join(".index"))
            .unwrap()
            .count(),
        0
    );
    assert!(
        store
            .my_work(&MyWorkRequest {
                aliases: vec!["selected".into(), "selected".into()],
                ..request(20)
            })
            .is_err()
    );
}

#[test]
fn review_and_overdue_facets_keep_actor_time_and_pagination_explicit() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    register(&store, "local", directory.path());
    let mut config = repository.config().unwrap();
    let mut custom_review = config
        .workflow
        .states
        .iter()
        .find(|state| state.category == WorkflowCategory::Review)
        .unwrap()
        .clone();
    custom_review.id = "peer_review".into();
    config.workflow.states.push(custom_review);
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    for (title, fields) in [
        (
            "Review A",
            serde_json::json!({"reviewer":"agent-a","assignee":"other","status":"in_review"}),
        ),
        (
            "Review B",
            serde_json::json!({"reviewer":"agent-a","status":"peer_review"}),
        ),
        (
            "Future request",
            serde_json::json!({"reviewer":"agent-a","status":"ready"}),
        ),
        (
            "Overdue mine",
            serde_json::json!({"assignee":"agent-a","due_at":"2026-09-09"}),
        ),
        (
            "Overdue mine second",
            serde_json::json!({"assignee":"agent-a","due_at":"2026-09-09"}),
        ),
        (
            "Overdue other",
            serde_json::json!({"assignee":"other","due_at":"2026-09-09"}),
        ),
    ] {
        let mut input = CreateIssue::new(title, "Work contract");
        input.fields = serde_json::from_value(fields).unwrap();
        repository.create_issue(&input, &RequestId::new()).unwrap();
    }
    let review: MyWorkRequest = serde_json::from_value(
        serde_json::json!({"assignee":"agent-a","facet":"review_requested","limit":1}),
    )
    .unwrap();
    let first = store.my_work(&review).unwrap();
    assert!(first.all_sources_available);
    assert_eq!(first.known_total, 2);
    let next = store
        .my_work(&MyWorkRequest {
            cursor: first.next_cursor.clone(),
            ..review.clone()
        })
        .unwrap();
    assert_ne!(first.rows[0].row.token.key, next.rows[0].row.token.key);
    let overdue: MyWorkRequest = serde_json::from_value(
        serde_json::json!({"assignee":"agent-a","facet":"overdue","limit":1,"as_of":"2026-09-10T00:00:00Z"}),
    )
    .unwrap();
    let report = store.my_work(&overdue).unwrap();
    assert_eq!(report.known_total, 2);
    assert_eq!(report.rows[0].row.title, "Overdue mine");
    assert_eq!(
        store
            .my_work(&MyWorkRequest {
                cursor: first.next_cursor,
                ..overdue.clone()
            })
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        store
            .my_work(&MyWorkRequest {
                cursor: report.next_cursor.clone(),
                as_of: Some("2026-09-11T00:00:00Z".parse().unwrap()),
                ..overdue.clone()
            })
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    let second = store
        .my_work(&MyWorkRequest {
            cursor: report.next_cursor,
            ..overdue
        })
        .unwrap();
    assert_eq!(second.rows[0].row.title, "Overdue mine second");
    let missing_time: MyWorkRequest =
        serde_json::from_value(serde_json::json!({"assignee":"agent-a","facet":"overdue"}))
            .unwrap();
    assert_eq!(
        store.my_work(&missing_time).unwrap_err().code,
        ErrorCode::InvalidInput
    );
}

#[test]
fn blocked_work_explains_canceled_prerequisites_questions_and_waivers() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut config = repository.config().unwrap();
    config.acceptance.allow_prerequisite_waivers = true;
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    let prerequisite = issue(&repository, "Prerequisite");
    let mut input = CreateIssue::new("Blocked assignment", "Contract");
    input.fields = serde_json::from_value(
        serde_json::json!({"assignee":"agent-a", "prerequisites":[prerequisite.metadata.id]}),
    )
    .unwrap();
    let dependent: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    repository
        .update_issue(
            prerequisite.metadata.id.as_str(),
            &prerequisite.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::from([(
                    "status".into(),
                    serde_json::json!("canceled"),
                )]),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    let questioned = issue(&repository, "Needs an answer");
    repository
        .create_question(
            &CreateQuestion {
                actor: "owner".into(),
                body: "Which behavior?".into(),
                subjects: vec![QuestionSubject {
                    subject: SubjectRef::Issue(questioned.metadata.id.clone()),
                    source: questioned.source.clone(),
                }],
                requirements: vec![],
                blocks_work: true,
                custom: Default::default(),
                extra: Default::default(),
            },
            &RequestId::new(),
        )
        .unwrap();
    let store = RegistryStore::open(&repository).unwrap();
    register(&store, "local", directory.path());
    let input: MyWorkRequest =
        serde_json::from_value(serde_json::json!({"assignee":"agent-a", "facet":"blocked"}))
            .unwrap();
    let first = store.my_work(&input).unwrap();
    assert!(first.all_sources_available, "{first:?}");
    assert_eq!(first.known_total, 2);
    let text = serde_json::to_string(&first).unwrap();
    assert!(text.contains("canceled_requirement"), "{text}");
    assert!(text.contains("blocks_implementation\":true"), "{text}");
    for point in [
        MyWorkFaultPoint::AfterProjection,
        MyWorkFaultPoint::BeforeEvidenceRevalidation,
    ] {
        let changed = store
            .my_work_with_faults(&input, |at| {
                if at == point {
                    let path = repository.root().join(&dependent.path);
                    let mut bytes = fs::read(&path).unwrap();
                    bytes.extend_from_slice(b"\nEdited while observing blockers\n");
                    fs::write(path, bytes).unwrap();
                }
                Ok(())
            })
            .unwrap();
        assert!(!changed.all_sources_available);
        assert_eq!(changed.known_total, 0);
        assert_eq!(
            changed.sources[0].error.as_ref().unwrap().code,
            ErrorCode::StaleSource
        );
    }
    repository
        .mutate_issue_graph(
            dependent.metadata.id.as_str(),
            None,
            None,
            &IssueGraphMutation::WaivePrerequisite {
                prerequisite: prerequisite.metadata.id.to_string(),
                actor: "owner".into(),
                reason: "Reviewed alternate implementation".into(),
            },
            &RequestId::new(),
        )
        .unwrap();
    let next = store.my_work(&input).unwrap();
    assert_eq!(next.known_total, 1);
    assert_eq!(next.rows[0].row.title, "Needs an answer");
    let question = repository
        .questions(&QuestionQuery::default())
        .unwrap()
        .remove(0);
    repository
        .mutate_question(
            &question.metadata.id,
            &question.source,
            &QuestionMutation::Answer {
                actor: "owner".into(),
                body: "Use the reviewed behavior".into(),
                decision_refs: vec![],
            },
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(store.my_work(&input).unwrap().known_total, 0);
    repository
        .update_issue(
            questioned.metadata.id.as_str(),
            &questioned.source,
            &UpdateIssue {
                fields: Default::default(),
                body: Some("Changed requirements after the answer".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    let stale_answer = store.my_work(&input).unwrap();
    assert_eq!(stale_answer.known_total, 1);
    assert!(
        serde_json::to_string(&stale_answer)
            .unwrap()
            .contains("subject_source_changed")
    );
    let current = repository
        .show_issue(prerequisite.metadata.id.as_str())
        .unwrap();
    repository
        .update_issue(
            current.metadata.id.as_str(),
            &current.source,
            &UpdateIssue {
                fields: Default::default(),
                body: Some("Changed prerequisite after waiver".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    let stale_waiver = store.my_work(&input).unwrap();
    assert_eq!(stale_waiver.known_total, 2);
    assert!(
        serde_json::to_string(&stale_waiver)
            .unwrap()
            .contains("waiver is stale")
    );
}
