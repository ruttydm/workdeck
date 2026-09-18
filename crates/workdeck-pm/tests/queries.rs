use serde_json::{Value, json};
use std::fs;
use tempfile::TempDir;
use workdeck_pm::{
    ArchiveFilter, Config, CreateIssue, CreatePlanning, ErrorCode, IssueQuery, IssueRecord,
    IssueSort, IssueSortField, PlanningKind, Repository, RequestId, SortDirection, TargetMatch,
};

fn setup() -> (TempDir, Repository) {
    let temp = TempDir::new().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}

fn create(repository: &Repository, title: &str, fields: Value) -> IssueRecord {
    let input = CreateIssue {
        title: title.into(),
        body: "Literal café [parser]".into(),
        fields: serde_json::from_value(fields).unwrap(),
    };
    serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap()
}

fn titles(repository: &Repository, query: &IssueQuery) -> Vec<String> {
    repository
        .query_issues(query)
        .unwrap()
        .into_iter()
        .map(|issue| issue.metadata.title)
        .collect()
}

fn reference(repository: &Repository, kind: PlanningKind, id: &str) {
    let mut input = CreatePlanning::new(id);
    input.id = Some(id.into());
    repository
        .create_planning(kind, &input, &RequestId::new())
        .unwrap();
}

#[test]
fn scalar_filters_combine_with_literal_unicode_text_and_canonical_status() {
    let (_temp, repository) = setup();
    for (kind, id) in [
        (PlanningKind::Project, "Project_A"),
        (PlanningKind::Cycle, "Cycle_A"),
        (PlanningKind::Label, "bug"),
    ] {
        reference(&repository, kind, id);
    }
    create(
        &repository,
        "Matching",
        json!({"project":"Project_A","cycle":"Cycle_A","labels":["bug"],"assignee":"Agent","due_at":"2026-09-10","priority":"high"}),
    );
    create(
        &repository,
        "Other owner",
        json!({"project":"Project_A","cycle":"Cycle_A","labels":["bug"],"assignee":"Other"}),
    );
    let query = IssueQuery {
        query: " CAFÉ ".into(),
        status: Some("todo".into()),
        priority: Some(workdeck_pm::Priority::High),
        assignee: Some("Agent".into()),
        label: Some("bug".into()),
        project: Some("Project_A".into()),
        cycle: Some("Cycle_A".into()),
        due_at: Some("2026-09-10".into()),
        ..IssueQuery::default()
    };
    assert_eq!(titles(&repository, &query), ["Matching"]);
    assert!(
        titles(
            &repository,
            &IssueQuery {
                project: Some("project_a".into()),
                ..query.clone()
            }
        )
        .is_empty()
    );
    assert!(
        titles(
            &repository,
            &IssueQuery {
                query: ".*".into(),
                ..query.clone()
            }
        )
        .is_empty()
    );
    assert!(
        titles(
            &repository,
            &IssueQuery {
                milestone: Some("unknown".into()),
                ..query
            }
        )
        .is_empty()
    );
    repository
        .mutate_planning(
            PlanningKind::Project,
            "Project_A",
            None,
            &workdeck_pm::PlanningMutation::Archive { archived: true },
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(
        titles(
            &repository,
            &IssueQuery {
                project: Some("Project_A".into()),
                ..IssueQuery::default()
            }
        )
        .len(),
        2
    );
}

#[test]
fn archive_scope_is_explicit_and_list_compatibility_retains_archived_records() {
    let (_temp, repository) = setup();
    create(&repository, "Active", json!({}));
    let archived = create(&repository, "Archived", json!({}));
    repository
        .archive_issue(
            archived.metadata.id.as_str(),
            &archived.source,
            true,
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(titles(&repository, &IssueQuery::default()), ["Active"]);
    assert_eq!(
        titles(
            &repository,
            &IssueQuery {
                archive: ArchiveFilter::Archived,
                ..IssueQuery::default()
            }
        ),
        ["Archived"]
    );
    assert_eq!(
        repository.list_issues().unwrap(),
        repository.query_issues(&IssueQuery::all()).unwrap()
    );
    assert_eq!(
        titles(&repository, &IssueQuery::all()),
        ["Active", "Archived"]
    );
}

#[test]
fn query_snapshot_retains_records_and_workflow_after_source_changes_without_reading_again() {
    let (_temp, repository) = setup();
    let issue = create(&repository, "Captured", json!({}));
    let captured = repository.issue_query_snapshot().unwrap();
    assert_eq!(captured.repository(), repository.identity());
    let path = repository.root().join(&issue.path);
    fs::write(&path, "broken authoritative source").unwrap();
    fs::write(repository.root().join("config.yml"), "workflow: [broken\n").unwrap();
    let rows = captured
        .select_indices(&IssueQuery {
            status: Some("todo".into()),
            ..IssueQuery::default()
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(captured.issues()[rows[0]].metadata.title, "Captured");
    assert_eq!(captured.issues()[rows[0]].source, issue.source);
    assert!(repository.issue_query_snapshot().is_err());
}

#[test]
fn sorting_is_deterministic_with_unique_identity_ties_and_explicit_priority_order() {
    let (_temp, repository) = setup();
    let low = create(&repository, "Same", json!({"priority":"low"}));
    let urgent = create(&repository, "same", json!({"priority":"urgent"}));
    for issue in [&low, &urgent] {
        let mut metadata = serde_json::to_value(&issue.metadata).unwrap();
        metadata["created_at"] = json!("2026-01-02T00:00:00Z");
        metadata["updated_at"] = json!("2026-01-02T00:00:00Z");
        fs::write(
            repository.root().join(&issue.path),
            format!(
                "---\n{}---\n{}",
                serde_yaml_ng::to_string(&metadata).unwrap(),
                issue.body
            ),
        )
        .unwrap();
    }
    let query = IssueQuery {
        sort: vec![IssueSort {
            field: IssueSortField::Title,
            direction: SortDirection::Descending,
        }],
        ..IssueQuery::default()
    };
    let mut expected = [low.metadata.id.clone(), urgent.metadata.id.clone()];
    expected.sort();
    let ids = |query: &IssueQuery| {
        repository
            .query_issues(query)
            .unwrap()
            .into_iter()
            .map(|issue| issue.metadata.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&query), expected);
    assert_eq!(ids(&IssueQuery::default()), expected);
    assert_eq!(
        ids(&IssueQuery {
            sort: vec![IssueSort {
                field: IssueSortField::Priority,
                direction: SortDirection::Descending
            }],
            ..IssueQuery::default()
        }),
        [urgent.metadata.id, low.metadata.id]
    );
    assert_eq!(ids(&query), expected);
}

#[test]
fn invalid_queries_are_explicit_and_do_not_change_source_or_cached_results() {
    let (_temp, repository) = setup();
    let issue = create(&repository, "Valid", json!({}));
    let snapshot = repository.issue_query_snapshot().unwrap();
    let before = fs::read(repository.root().join(&issue.path)).unwrap();
    for query in [
        IssueQuery {
            status: Some("missing".into()),
            ..IssueQuery::default()
        },
        IssueQuery {
            project: Some("".into()),
            ..IssueQuery::default()
        },
        IssueQuery {
            query: "x".repeat(4097),
            ..IssueQuery::default()
        },
        IssueQuery {
            query: "bad\nquery".into(),
            ..IssueQuery::default()
        },
        IssueQuery {
            sort: vec![IssueSort::default(), IssueSort::default()],
            ..IssueQuery::default()
        },
    ] {
        assert_eq!(
            snapshot.select_indices(&query).unwrap_err().code,
            ErrorCode::InvalidInput
        );
    }
    assert_eq!(
        snapshot.select_indices(&IssueQuery::default()).unwrap(),
        [0]
    );
    assert_eq!(
        fs::read(repository.root().join(&issue.path)).unwrap(),
        before
    );
    assert!(serde_json::from_value::<IssueQuery>(json!({"projet":"typo"})).is_err());
    assert_eq!(
        serde_json::from_value::<IssueQuery>(json!({})).unwrap(),
        IssueQuery::default()
    );
}

#[test]
fn exact_custom_statuses_and_ambiguous_aliases_use_the_captured_workflow() {
    let (_temp, repository) = setup();
    let mut config: Config = repository.config().unwrap();
    let mut duplicate = config
        .workflow
        .states
        .iter()
        .find(|state| state.id == "in_progress")
        .unwrap()
        .clone();
    duplicate.id = "in-progress".into();
    config.workflow.states.push(duplicate);
    fs::write(
        repository.root().join("config.yml"),
        serde_yaml_ng::to_string(&config).unwrap(),
    )
    .unwrap();
    create(&repository, "Exact", json!({"status":"in_progress"}));
    let snapshot = repository.issue_query_snapshot().unwrap();
    assert_eq!(
        snapshot
            .select_indices(&IssueQuery {
                status: Some("in_progress".into()),
                ..IssueQuery::default()
            })
            .unwrap(),
        [0]
    );
    assert_eq!(
        snapshot
            .select_indices(&IssueQuery {
                status: Some("In Progress".into()),
                ..IssueQuery::default()
            })
            .unwrap_err()
            .code,
        ErrorCode::AmbiguousReference
    );
}

#[test]
fn retired_records_keep_their_proof_in_all_and_archived_queries() {
    let (_temp, repository) = setup();
    reference(&repository, PlanningKind::Target, "release");
    let record = create(&repository, "Retired", json!({"targets":["release"]}));
    repository
        .retire_issue(
            record.metadata.id.as_str(),
            Some(&record.source),
            None,
            &RequestId::new(),
        )
        .unwrap();
    assert!(
        repository
            .query_issues(&IssueQuery::default())
            .unwrap()
            .is_empty()
    );
    let retired = repository.query_issues(&IssueQuery::all()).unwrap();
    assert_eq!(retired.len(), 1);
    assert!(retired[0].retirement.is_some());
    assert_eq!(
        retired,
        repository
            .query_issues(&IssueQuery {
                targets: vec!["release".into()],
                ..IssueQuery::all()
            })
            .unwrap()
    );
    assert_eq!(
        retired,
        repository
            .query_issues(&IssueQuery {
                archive: ArchiveFilter::Archived,
                ..IssueQuery::default()
            })
            .unwrap()
    );
}

#[test]
fn target_filters_include_direct_project_and_milestone_membership_in_one_capture() {
    let (_temp, repository) = setup();
    for target in ["release_a", "release_b", "release_c"] {
        reference(&repository, PlanningKind::Target, target);
    }
    reference(&repository, PlanningKind::Project, "other_project");
    reference(&repository, PlanningKind::Cycle, "cycle");
    for (kind, id, fields) in [
        (
            PlanningKind::Project,
            "project",
            json!({"targets":["release_a"]}),
        ),
        (
            PlanningKind::Milestone,
            "milestone",
            json!({"project":"project","targets":["release_b"]}),
        ),
    ] {
        repository
            .create_planning(
                kind,
                &CreatePlanning {
                    id: Some(id.into()),
                    name: id.into(),
                    body: String::new(),
                    fields: serde_json::from_value(fields).unwrap(),
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    let first = create(
        &repository,
        "All memberships",
        json!({"project":"project","milestone":"milestone","cycle":"cycle","targets":["release_c"]}),
    );
    create(&repository, "Project only", json!({"project":"project"}));
    create(&repository, "Direct only", json!({"targets":["release_b"]}));
    let snapshot = repository.issue_query_snapshot().unwrap();
    let all = IssueQuery {
        targets: vec!["release_a".into(), "release_b".into()],
        ..IssueQuery::default()
    };
    assert_eq!(titles(&repository, &all), ["All memberships"]);
    assert_eq!(
        titles(
            &repository,
            &IssueQuery {
                target_match: TargetMatch::Any,
                ..all.clone()
            }
        ),
        ["All memberships", "Project only", "Direct only"]
    );
    assert_eq!(
        titles(
            &repository,
            &IssueQuery {
                project: Some("project".into()),
                cycle: Some("cycle".into()),
                milestone: Some("milestone".into()),
                targets: vec!["release_c".into()],
                ..IssueQuery::default()
            }
        ),
        ["All memberships"]
    );
    assert!(
        titles(
            &repository,
            &IssueQuery {
                targets: vec!["unknown".into()],
                ..IssueQuery::default()
            }
        )
        .is_empty()
    );
    repository
        .mutate_planning(
            PlanningKind::Project,
            "project",
            None,
            &workdeck_pm::PlanningMutation::Archive { archived: true },
            &RequestId::new(),
        )
        .unwrap();
    assert_eq!(titles(&repository, &all), ["All memberships"]);
    repository
        .update_issue(
            first.metadata.id.as_str(),
            &first.source,
            &workdeck_pm::UpdateIssue {
                fields: serde_json::from_value(json!({"project":"other_project","milestone":null}))
                    .unwrap(),
                body: None,
            },
            &RequestId::new(),
        )
        .unwrap();
    assert!(titles(&repository, &all).is_empty());
    assert_eq!(snapshot.select_indices(&all).unwrap().len(), 1);
    assert_eq!(
        titles(
            &repository,
            &IssueQuery {
                targets: vec!["release_c".into()],
                ..IssueQuery::default()
            }
        ),
        ["All memberships"]
    );
}

#[test]
fn unused_new_predicates_preserve_the_existing_serialized_saved_query_contract() {
    let legacy = json!({"query":"", "status":null, "priority":null, "assignee":null, "label":null, "project":null, "cycle":null, "milestone":null, "targets":[], "target_match":"all", "due_at":null, "archive":"active", "sort":[{"field":"created_at", "direction":"ascending"}]});
    let parsed: IssueQuery = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(serde_json::to_value(parsed).unwrap(), legacy);
}
