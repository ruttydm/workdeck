use super::*;
use crate::*;
use rusqlite::Connection;
use std::{cell::RefCell, collections::BTreeMap, path::Path};
#[path = "family_tests.rs"]
mod families;

fn fixture() -> (tempfile::TempDir, Repository) {
    let temp = tempfile::tempdir().unwrap();
    let repository = Repository::init(temp.path(), "WD").unwrap();
    (temp, repository)
}
fn issue(repo: &Repository, title: &str, fields: serde_json::Value) -> IssueRecord {
    serde_json::from_value(
        repo.create_issue(
            &CreateIssue {
                title: title.into(),
                body: "Literal café [parser]".into(),
                fields: serde_json::from_value(fields).unwrap(),
            },
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap()
}
fn view(root: &Path) -> Result<ProjectionReadView> {
    let limits = ProjectionLimits::default();
    let source = crate::sources::capture(root, &SourceSelector::WorkingTree, &limits.source)?;
    let mut connection = Connection::open_in_memory().unwrap();
    let tx = connection.transaction().unwrap();
    schema::initialize(&tx)?;
    let build = projector::project(&tx, &source.snapshot, None, &limits)?;
    tx.commit().unwrap();
    let id = ProjectionViewId {
        schema: PROJECTION_SCHEMA_VERSION,
        slot: ContentHash::of(root.as_os_str().as_encoded_bytes()),
        source: source.snapshot.identity().clone(),
        generation: build.manifest.fingerprint.clone(),
    };
    Ok(ProjectionReadView {
        connection,
        id,
        observation: source.observation.clone(),
        binding: source.publication_binding().cloned(),
        limits,
        counts: build.counts,
        query_cache: RefCell::default(),
    })
}
fn titles(view: &ProjectionReadView, query: &IssueQuery) -> Vec<String> {
    let handle = view
        .query(&ProjectionQuery::Issues {
            query: query.clone(),
            group_by: None,
        })
        .unwrap();
    view.page(&handle, 0, 100)
        .unwrap()
        .rows
        .into_iter()
        .map(|row| row.title)
        .collect()
}

#[test]
fn indexed_issue_predicates_match_native_unicode_alias_archive_and_order() {
    let (temp, repo) = fixture();
    issue(
        &repo,
        "Alpha",
        serde_json::json!({"priority":"high","assignee":"Alice"}),
    );
    issue(
        &repo,
        "alpha",
        serde_json::json!({"priority":"low","assignee":"Bob"}),
    );
    let archived = issue(&repo, "Archived", serde_json::json!({}));
    repo.archive_issue(
        archived.metadata.id.as_str(),
        &archived.source,
        true,
        &RequestId::new(),
    )
    .unwrap();
    let projected = view(temp.path()).unwrap();
    for query in [
        IssueQuery::default(),
        IssueQuery::all(),
        IssueQuery {
            query: " CAFÉ ".into(),
            ..IssueQuery::default()
        },
        IssueQuery {
            query: ".*".into(),
            ..IssueQuery::default()
        },
        IssueQuery {
            status: Some("todo".into()),
            assignee: Some("Alice".into()),
            ..IssueQuery::default()
        },
        IssueQuery {
            sort: vec![IssueSort {
                field: IssueSortField::Title,
                direction: SortDirection::Descending,
            }],
            ..IssueQuery::all()
        },
        IssueQuery {
            sort: vec![IssueSort {
                field: IssueSortField::Priority,
                direction: SortDirection::Descending,
            }],
            ..IssueQuery::all()
        },
    ] {
        let native = repo
            .query_issues(&query)
            .unwrap()
            .into_iter()
            .map(|record| record.metadata.title)
            .collect::<Vec<_>>();
        assert_eq!(titles(&projected, &query), native, "{query:?}");
    }
}

#[test]
fn query_pages_locate_and_details_share_one_generation() {
    let (temp, repo) = fixture();
    let first = issue(&repo, "First", serde_json::json!({}));
    issue(&repo, "Second", serde_json::json!({}));
    let old = view(temp.path()).unwrap();
    let handle = old.query(&ProjectionQuery::default()).unwrap();
    let page = old.page(&handle, 0, 1).unwrap();
    assert_eq!(page.next_offset, Some(1));
    assert_eq!(
        old.locate(&handle, &page.rows[0].token.key).unwrap(),
        Some(0)
    );
    assert!(
        old.detail(&page.rows[0].token)
            .unwrap()
            .document
            .unwrap()
            .contains("Literal café")
    );
    repo.update_issue(
        first.metadata.id.as_str(),
        &first.source,
        &UpdateIssue {
            fields: BTreeMap::from([("title".into(), serde_json::json!("Changed"))]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    let next = view(temp.path()).unwrap();
    assert_eq!(
        next.page(&handle, 0, 1).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        next.detail(&page.rows[0].token).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(old.page(&handle, 0, 1).unwrap().rows[0].title, "First");
    assert!(
        old.page(&handle, 0, old.limits().max_page_rows + 1)
            .is_err()
    );
}

#[test]
fn inherited_target_membership_is_indexed_without_case_folding_exact_ids() {
    let (temp, repo) = fixture();
    let mut target = CreatePlanning::new("Target");
    target.id = Some("Target_A".into());
    repo.create_planning(PlanningKind::Target, &target, &RequestId::new())
        .unwrap();
    let mut project = CreatePlanning::new("Project");
    project.id = Some("Project_A".into());
    project.fields = BTreeMap::from([("targets".into(), serde_json::json!(["Target_A"]))]);
    repo.create_planning(PlanningKind::Project, &project, &RequestId::new())
        .unwrap();
    issue(&repo, "Member", serde_json::json!({"project":"Project_A"}));
    let projected = view(temp.path()).unwrap();
    let query = IssueQuery {
        targets: vec!["Target_A".into()],
        ..IssueQuery::default()
    };
    assert_eq!(titles(&projected, &query), vec!["Member"]);
    assert!(
        titles(
            &projected,
            &IssueQuery {
                targets: vec!["target_a".into()],
                ..query
            }
        )
        .is_empty()
    );
}

#[test]
fn malformed_source_and_forged_durable_receipt_cannot_be_projected() {
    let (temp, repo) = fixture();
    let record = issue(&repo, "Subject", serde_json::json!({}));
    view(temp.path()).unwrap();
    let operation = std::fs::read_dir(repo.root().join("operations"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let bytes = std::fs::read(&operation).unwrap();
    let mut receipt: serde_json::Value = serde_yaml_ng::from_slice(&bytes).unwrap();
    receipt["result"]["source"]["content"] = serde_json::json!(ContentHash::of(b"forged"));
    std::fs::write(&operation, serde_yaml_ng::to_string(&receipt).unwrap()).unwrap();
    assert!(view(temp.path()).is_err());
    std::fs::write(&operation, bytes).unwrap();
    std::fs::write(repo.root().join(record.path), "invalid item").unwrap();
    assert!(view(temp.path()).is_err());
}

#[test]
fn a_bulk_issue_query_does_not_parse_every_receipt_for_every_issue() {
    let (_temp, repo) = fixture();
    let original = issue(&repo, "Seed", serde_json::json!({}));
    for _ in 0..200 {
        let mut metadata = original.metadata.clone();
        metadata.id = IssueId::new("WD").unwrap();
        let path = repo
            .root()
            .join("issues")
            .join(metadata.id.as_str())
            .join("item.md");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            format!(
                "---\n{}---\nBody\n",
                serde_yaml_ng::to_string(&metadata).unwrap()
            ),
        )
        .unwrap();
    }
    crate::retirement::take_retirement_parse_count();
    assert_eq!(repo.query_issues(&IssueQuery::all()).unwrap().len(), 201);
    let parses = crate::retirement::take_retirement_parse_count();
    let receipts = std::fs::read_dir(repo.root().join("operations"))
        .unwrap()
        .count();
    assert!(
        parses <= receipts + 1,
        "one captured query reparsed {receipts} receipts {parses} times"
    );
}

#[test]
fn cold_projection_sql_work_grows_below_quadratic_for_doubled_sources() {
    fn steps(count: usize) -> usize {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        let (temporary, repository) = fixture();
        std::fs::create_dir_all(repository.root().join("features")).unwrap();
        for index in 0..count {
            let id: FeatureId = format!("FEAT-{index:026}").parse().unwrap();
            let metadata: FeatureMetadata = serde_json::from_value(serde_json::json!({
                "schema":1,"repository":repository.identity(),"id":id,"revision":1,
                "name":format!("Feature {index}"),"created_at":"2026-09-09T00:00:00Z",
                "updated_at":"2026-09-09T00:00:00Z"
            }))
            .unwrap();
            std::fs::write(
                repository.root().join(format!("features/{id}.md")),
                format!(
                    "---\n{}---\nSearchable body\n",
                    serde_yaml_ng::to_string(&metadata).unwrap()
                ),
            )
            .unwrap();
        }
        let limits = ProjectionLimits::default();
        let source = crate::sources::capture(
            temporary.path(),
            &SourceSelector::WorkingTree,
            &limits.source,
        )
        .unwrap();
        let mut connection = Connection::open_in_memory().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        connection
            .progress_handler(
                100,
                Some(move || {
                    counter.fetch_add(1, Ordering::Relaxed);
                    false
                }),
            )
            .unwrap();
        let tx = connection.transaction().unwrap();
        schema::initialize(&tx).unwrap();
        projector::project(&tx, &source.snapshot, None, &limits).unwrap();
        tx.commit().unwrap();
        calls.load(Ordering::Relaxed)
    }
    let small = steps(200);
    let large = steps(400);
    assert!(
        large < small * 3,
        "doubling source size used {small} -> {large} SQL progress intervals; expected sub-quadratic construction"
    );
}

#[test]
fn board_windows_are_bounded_and_keep_columns_in_one_query_generation() {
    let (temp, repo) = fixture();
    for index in 0..6 {
        issue(
            &repo,
            &format!("Alice {index}"),
            serde_json::json!({"assignee":"Alice"}),
        );
        issue(
            &repo,
            &format!("Bob {index}"),
            serde_json::json!({"assignee":"Bob"}),
        );
    }
    let old = view(temp.path()).unwrap();
    let handle = old
        .query(&ProjectionQuery::Issues {
            query: IssueQuery::default(),
            group_by: Some(IssueGroupBy::Assignee),
        })
        .unwrap();
    let request = ProjectionBoardRequest {
        first_group: 0,
        columns: 2,
        rows: 2,
        selected: Some(5),
    };
    let board = old.board(&handle, &request).unwrap();
    assert_eq!(board.len(), 2);
    assert_eq!(board[0].group.value.as_deref(), Some("Alice"));
    assert_eq!(board[0].group.count, 6);
    assert_eq!(board[0].page.offset, 4);
    assert_eq!(board[1].page.offset, 6);
    for column in &board {
        assert_eq!(column.page.handle, handle);
        assert_eq!(column.page.rows.len(), 2);
        assert!(
            column
                .page
                .rows
                .iter()
                .all(|row| row.group == column.group.value)
        );
    }
    assert!(
        old.board(
            &handle,
            &ProjectionBoardRequest {
                rows: old.limits().max_page_rows,
                ..request.clone()
            }
        )
        .is_err()
    );
    assert!(
        old.board(
            &handle,
            &ProjectionBoardRequest {
                columns: 0,
                ..request.clone()
            }
        )
        .is_err()
    );
    assert!(
        old.board(
            &handle,
            &ProjectionBoardRequest {
                first_group: 3,
                ..request.clone()
            }
        )
        .is_err()
    );
    assert!(
        old.board(
            &handle,
            &ProjectionBoardRequest {
                selected: Some(handle.total),
                ..request.clone()
            }
        )
        .is_err()
    );
    issue(&repo, "New", serde_json::json!({"assignee":"Aaron"}));
    let next = view(temp.path()).unwrap();
    assert_eq!(
        next.board(&handle, &request).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(old.board(&handle, &request).unwrap(), board);
}

#[test]
fn feature_tree_orders_parents_before_children_and_collapses_with_pinned_pages() {
    let (temp, repo) = fixture();
    let create = |name: &str, parent: Option<&FeatureId>| -> FeatureRecord {
        let mut input = CreateFeature::new(name);
        if let Some(parent) = parent {
            input
                .fields
                .insert("parent".into(), serde_json::json!(parent));
        }
        serde_json::from_value::<FeatureOutcome>(
            repo.create_feature(&input, &RequestId::new())
                .unwrap()
                .result,
        )
        .unwrap()
        .record
    };
    let root = create("Z root", None);
    let child = create("B child", Some(&root.metadata.id));
    let leaf = create("A leaf", Some(&child.metadata.id));
    let sibling = create("C sibling", Some(&root.metadata.id));
    let old = view(temp.path()).unwrap();
    let tree = ProjectionFeatureQuery {
        tree: true,
        ..Default::default()
    };
    let handle = old
        .query(&ProjectionQuery::Features {
            query: tree.clone(),
        })
        .unwrap();
    let rows = old.page(&handle, 0, 10).unwrap().rows;
    assert_eq!(
        rows.iter()
            .map(|row| row.token.key.id.as_str())
            .collect::<Vec<_>>(),
        vec![
            root.metadata.id.as_str(),
            child.metadata.id.as_str(),
            leaf.metadata.id.as_str(),
            sibling.metadata.id.as_str()
        ]
    );
    assert_eq!(
        rows.iter()
            .map(|row| row.tree.as_ref().unwrap().depth)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 1]
    );
    assert_eq!(rows[0].tree.as_ref().unwrap().children, 2);
    let collapsed = old
        .query(&ProjectionQuery::Features {
            query: ProjectionFeatureQuery {
                collapsed: vec![child.metadata.id.clone()],
                ..tree.clone()
            },
        })
        .unwrap();
    assert_eq!(collapsed.total, 3);
    assert!(
        old.locate(&collapsed, &rows[2].token.key)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        old.page(&handle, 2, 1).unwrap().rows[0].token,
        rows[2].token
    );
    let filtered = old
        .query(&ProjectionQuery::Features {
            query: ProjectionFeatureQuery {
                query: "leaf".into(),
                ..tree
            },
        })
        .unwrap();
    let filtered_row = old.page(&filtered, 0, 1).unwrap().rows.remove(0);
    assert_eq!(filtered_row.tree.as_ref().unwrap().depth, 0);
    assert!(filtered_row.tree.as_ref().unwrap().parent_outside_view);
    assert_eq!(
        filtered_row.parent.as_ref().unwrap().id,
        child.metadata.id.as_str()
    );
}

#[test]
fn reviewer_category_and_due_filters_match_native_exact_time_semantics() {
    let (temp, repo) = fixture();
    issue(
        &repo,
        "Review for me",
        serde_json::json!({"reviewer":"Ada","status":"in_review","assignee":"Bob"}),
    );
    issue(
        &repo,
        "Not requested yet",
        serde_json::json!({"reviewer":"Ada","status":"ready"}),
    );
    issue(
        &repo,
        "Other reviewer",
        serde_json::json!({"reviewer":"Bob","status":"in_review"}),
    );
    issue(
        &repo,
        "Yesterday",
        serde_json::json!({"due_at":"2026-09-09"}),
    );
    issue(&repo, "Today", serde_json::json!({"due_at":"2026-09-10"}));
    issue(
        &repo,
        "Offset overdue",
        serde_json::json!({"due_at":"2026-09-10T02:00:00+03:00"}),
    );
    issue(
        &repo,
        "Exact boundary",
        serde_json::json!({"due_at":"2026-09-10T00:00:00Z"}),
    );
    issue(
        &repo,
        "Nanosecond before",
        serde_json::json!({"due_at":"2026-09-09T23:59:59.999999999Z"}),
    );
    let projected = view(temp.path()).unwrap();
    let native = repo.issue_query_snapshot().unwrap();
    for (json, expected) in [
        (
            serde_json::json!({"reviewer":"Ada", "workflow_categories":["review"]}),
            vec!["Review for me"],
        ),
        (
            serde_json::json!({"due_before":"2026-09-10T00:00:00Z"}),
            vec!["Yesterday", "Offset overdue", "Nanosecond before"],
        ),
    ] {
        let query: IssueQuery = serde_json::from_value(json).unwrap();
        let mut actual = titles(&projected, &query);
        let mut native_titles = native
            .select_indices(&query)
            .unwrap()
            .iter()
            .map(|&i| native.issues()[i].metadata.title.clone())
            .collect::<Vec<_>>();
        let mut expected = expected.into_iter().map(str::to_owned).collect::<Vec<_>>();
        actual.sort();
        native_titles.sort();
        expected.sort();
        assert_eq!(actual, expected);
        assert_eq!(native_titles, expected);
    }
}

#[test]
fn explicit_issue_id_sets_match_native_queries_and_empty_means_no_rows() {
    let (temp, repo) = fixture();
    let selected = issue(&repo, "Selected", serde_json::json!({}));
    issue(&repo, "Excluded", serde_json::json!({}));
    let projected = view(temp.path()).unwrap();
    let native = repo.issue_query_snapshot().unwrap();
    for (ids, expected) in [
        (vec![selected.metadata.id.clone()], vec!["Selected"]),
        (vec![], vec![]),
        (vec![IssueId::new("WD").unwrap()], vec![]),
    ] {
        let query = IssueQuery {
            ids: Some(ids),
            ..Default::default()
        };
        assert_eq!(titles(&projected, &query), expected);
        let actual = native
            .select_indices(&query)
            .unwrap()
            .iter()
            .map(|&index| native.issues()[index].metadata.title.as_str())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }
    let duplicate = IssueQuery {
        ids: Some(vec![selected.metadata.id.clone(), selected.metadata.id]),
        ..Default::default()
    };
    assert!(duplicate.validate().is_err());
}
