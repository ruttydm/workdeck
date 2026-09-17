use super::{indexed_workspace::IndexedWorkspace, projection_reader::*};
use std::{
    fs,
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use workdeck_pm::{
    ContentHash, ErrorCode, IssueQuery, PlanningSourceIdentity, PmError, RepositoryId,
    SnapshotKind, SourceRole, projection::*,
};

type Reply = mpsc::Sender<ProjectionReply>;
type Reads = mpsc::Receiver<(ProjectionRead, Reply)>;
fn harness(limit: usize) -> (IndexedWorkspace, Reads) {
    let (send, receive) = mpsc::channel();
    let workspace =
        IndexedWorkspace::with_reader(ProjectionQuery::default(), limit, move |request| {
            let (reply, response) = mpsc::channel();
            send.send((request, reply)).unwrap();
            Ok(response.recv_timeout(Duration::from_secs(3)).unwrap())
        })
        .unwrap();
    (workspace, receive)
}
fn identity() -> ProjectionViewId {
    ProjectionViewId {
        schema: 1,
        slot: ContentHash::of(b"checkout/source slot"),
        generation: ContentHash::of(b"generation 1"),
        source: PlanningSourceIdentity {
            repository: RepositoryId::new(),
            role: SourceRole::Local,
            ref_name: None,
            commit: None,
            tree: None,
            index_content: None,
            content: ContentHash::of(b"original source"),
        },
    }
}
fn status(view: &ProjectionViewId) -> ProjectionStatus {
    ProjectionStatus {
        state: ProjectionState::Current,
        view: Some(view.clone()),
        observation: None,
        publication_binding: None,
        diagnostics: Vec::new(),
    }
}
fn respond(reply: Reply, view: &ProjectionViewId, value: ProjectionValue) {
    reply
        .send(ProjectionReply {
            retained: None,
            status: status(view),
            result: Ok(value),
        })
        .unwrap();
}
fn row(view: &ProjectionViewId, index: usize) -> ProjectionRow {
    ProjectionRow {
        tree: None,
        token: ProjectionRowToken {
            view: view.clone(),
            key: ProjectionRecordKey {
                repository: view.source.repository.clone(),
                kind: SnapshotKind::Issue,
                id: format!("issue-{index}"),
            },
            path: format!("issues/issue-{index}/item.md").into(),
            content: ContentHash::of(index.to_string().as_bytes()),
        },
        title: format!("Row {index}"),
        archived: false,
        retired: false,
        status: Some("todo".into()),
        priority: None,
        assignee: None,
        decision: None,
        maturity: None,
        availability: None,
        lead: None,
        project: None,
        cycle: None,
        milestone: None,
        parent: None,
        group: None,
        created_at: None,
        updated_at: None,
    }
}
fn detail(row: ProjectionRow, text: &str) -> Box<ProjectionDetail> {
    Box::new(ProjectionDetail {
        row,
        document: Some(text.into()),
        document_bytes: text.len(),
        omitted_document_bytes: 0,
        relations: Vec::new(),
        total_relations: 0,
    })
}
fn next(workspace: &mut IndexedWorkspace, reads: &Reads) -> (ProjectionRead, Reply) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        workspace.poll();
        if let Ok(read) = reads.try_recv() {
            return read;
        }
        assert!(Instant::now() < deadline, "No read: {workspace:?}");
        thread::sleep(Duration::from_millis(1));
    }
}
fn until(workspace: &mut IndexedWorkspace, ready: impl Fn(&IndexedWorkspace) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        workspace.poll();
        if ready(workspace) {
            return;
        }
        assert!(Instant::now() < deadline, "No state: {workspace:?}");
        thread::sleep(Duration::from_millis(1));
    }
}
fn answer_page(workspace: &mut IndexedWorkspace, reads: &Reads) -> (usize, usize) {
    let (request, reply) = next(workspace, reads);
    let ProjectionRead::Page {
        handle,
        offset,
        limit,
    } = request
    else {
        panic!("Expected page, got {request:?}")
    };
    let count = limit.min(handle.total - offset);
    let page = ProjectionPage {
        rows: (offset..offset + count)
            .map(|index| row(&handle.view, index))
            .collect(),
        offset,
        next_offset: (offset + count < handle.total).then_some(offset + count),
        handle: handle.clone(),
    };
    respond(reply, &handle.view, ProjectionValue::Page(page));
    until(workspace, |workspace| !workspace.loading_page);
    (offset, limit)
}
fn answer_query(
    workspace: &mut IndexedWorkspace,
    reads: &Reads,
    total: usize,
    located: Option<usize>,
) {
    let (request, reply) = next(workspace, reads);
    let ProjectionRead::Query { view, .. } = request else {
        panic!("Expected query, got {request:?}")
    };
    let handle = ProjectionQueryHandle {
        view: view.clone(),
        query: ContentHash::of(b"query"),
        total,
    };
    respond(
        reply,
        &view,
        ProjectionValue::Query {
            handle,
            groups: Vec::new(),
            located,
        },
    );
}
fn loaded(limit: usize) -> (IndexedWorkspace, Reads, ProjectionViewId) {
    let (mut workspace, reads) = harness(limit);
    let view = identity();
    workspace.resize(5);
    workspace.refresh(false);
    let (request, reply) = next(&mut workspace, &reads);
    assert!(matches!(request, ProjectionRead::Refresh(_)));
    respond(reply, &view, ProjectionValue::Refreshed(view.clone()));
    answer_query(&mut workspace, &reads, 40_000, None);
    answer_page(&mut workspace, &reads);
    (workspace, reads, view)
}

fn mounted_document(path: &Path, metadata: &impl serde::Serialize, body: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    // JSON is a YAML 1.2 mapping, so the test fixture can use the existing
    // direct serde_json dependency without adding a second serializer seam.
    let metadata = serde_json::to_string_pretty(metadata).unwrap();
    fs::write(path, format!("---\n{metadata}\n---\n{body}")).unwrap();
}

fn mounted_feature_id(index: usize) -> workdeck_pm::FeatureId {
    format!("FEAT-{index:026}").parse().unwrap()
}

fn mounted_issue_id(index: usize) -> workdeck_pm::IssueId {
    format!("WD-{index:026}").parse().unwrap()
}

fn mounted_features(root: &Path, repository: &workdeck_pm::Repository, count: usize) {
    let timestamp: workdeck_pm::Timestamp = "2026-09-09T00:00:00Z".parse().unwrap();
    for index in 0..count {
        let id = mounted_feature_id(index);
        let metadata: workdeck_pm::FeatureMetadata = serde_json::from_value(serde_json::json!({
            "schema": 1,
            "repository": repository.identity(),
            "id": id,
            "revision": 1,
            "name": format!("Mounted capability {index:05} bucket {}", index % 10),
            "created_at": timestamp,
            "updated_at": timestamp,
            "parent": (index > 0).then(|| mounted_feature_id((index - 1) / 8)),
            "prerequisites": if index >= 16 && index % 16 == 0 {
                vec![mounted_feature_id(index - 8)]
            } else {
                Vec::new()
            }
        }))
        .unwrap();
        metadata.validate().unwrap();
        mounted_document(
            &root.join(format!("features/{id}.md")),
            &metadata,
            "# Mounted capability\nScale probe fixture.\n",
        );
    }
}

fn mounted_issues(root: &Path, repository: &workdeck_pm::Repository, count: usize) {
    let config = repository.config().unwrap();
    let timestamp: workdeck_pm::Timestamp = "2026-09-09T00:00:00Z".parse().unwrap();
    for index in 0..count {
        let id = mounted_issue_id(index);
        let mut metadata = workdeck_pm::IssueMetadata::new(
            &config,
            &format!("Mounted work item {index:05} bucket {}", index % 10),
            timestamp,
        )
        .unwrap();
        metadata.id = id.clone();
        metadata.assignee = Some(if index % 2 == 0 { "agent-a" } else { "agent-b" }.into());
        metadata.parent = (index > 0).then(|| mounted_issue_id((index - 1) / 8));
        if index >= 16 && index % 16 == 0 {
            metadata.prerequisites.push(mounted_issue_id(index - 8));
        }
        metadata.validate(&config).unwrap();
        mounted_document(
            &root.join(format!("issues/{id}/item.md")),
            &metadata,
            "# Mounted work\nScale probe fixture.\n",
        );
    }
}

fn mounted_rss_bytes() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        let peak = u64::try_from(unsafe { usage.assume_init() }.ru_maxrss).ok()?;
        #[cfg(target_os = "macos")]
        return Some(peak);
        #[cfg(not(target_os = "macos"))]
        return Some(peak.saturating_mul(1024));
    }
    #[cfg(not(unix))]
    None
}

fn mounted_wait(index: &mut IndexedWorkspace, timeout: Duration) -> Duration {
    let start = Instant::now();
    let deadline = start + timeout;
    loop {
        index.poll();
        if index.board_ready() && index.page.is_some() && !index.loading_page {
            return start.elapsed();
        }
        assert!(
            Instant::now() < deadline,
            "mounted workbench did not become ready: {index:?}"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn mounted_board_wait(index: &mut IndexedWorkspace, timeout: Duration) -> Duration {
    let start = Instant::now();
    let deadline = start + timeout;
    loop {
        index.poll();
        if index.board_ready()
            && index.page.is_some()
            && !index.loading_page
            && !index.board_columns.is_empty()
            && index
                .board_columns
                .iter()
                .all(|column| !column.page.rows.is_empty())
        {
            return start.elapsed();
        }
        assert!(
            Instant::now() < deadline,
            "mounted board did not become ready: {index:?}"
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn mounted_probe(
    root: &Path,
    query: ProjectionQuery,
    filter: impl Fn(usize) -> ProjectionQuery,
    total: usize,
) -> serde_json::Value {
    let limits = ProjectionLimits::default();
    let max_page_rows = limits.max_page_rows;
    let mut index = IndexedWorkspace::new(
        root.to_path_buf(),
        workdeck_pm::SourceSelector::WorkingTree,
        query,
        limits,
    )
    .unwrap();
    index.resize(20);
    let cold = Instant::now();
    index.open();
    let open_elapsed = mounted_wait(&mut index, Duration::from_secs(120));
    assert_eq!(index.handle.as_ref().unwrap().total, total);
    assert_eq!(index.visible_rows().count(), 20);

    let mut warm_samples = Vec::new();
    for sample in 0..20 {
        index.set_query(filter(sample));
        warm_samples.push(mounted_wait(&mut index, Duration::from_secs(30)).as_secs_f64() * 1000.0);
    }
    let mut ordered = warm_samples.clone();
    ordered.sort_by(f64::total_cmp);
    let p50 = ordered[ordered.len() / 2];
    let p95 = ordered[(ordered.len() * 95).div_ceil(100).saturating_sub(1)];

    let alternate = if total == 40_000 {
        let mut samples = Vec::with_capacity(10);
        for sample in 0..10 {
            index.set_query(ProjectionQuery::Features {
                query: ProjectionFeatureQuery {
                    tree: true,
                    // Use distinct leaf collapses so every query still
                    // traverses the complete 40,000-node forest.
                    collapsed: vec![mounted_feature_id(39_990 + sample)],
                    ..Default::default()
                },
            });
            samples.push(mounted_wait(&mut index, Duration::from_secs(30)).as_secs_f64() * 1000.0);
            let page_rows = index.page.as_ref().map_or(0, |page| page.rows.len());
            assert!(
                page_rows <= max_page_rows,
                "feature tree materialized {page_rows} rows; bound is {max_page_rows}"
            );
        }
        let mut ordered = samples.clone();
        ordered.sort_by(f64::total_cmp);
        let end = Instant::now();
        index.end();
        let end_navigation = mounted_wait(&mut index, Duration::from_secs(30));
        serde_json::json!({
            "kind": "feature_tree",
            "samples": samples.len(),
            "warm_ms": {
                "p50": ordered[ordered.len() / 2],
                "p95": ordered[(ordered.len() * 95).div_ceil(100).saturating_sub(1)]
            },
            "end_navigation_ms": end_navigation.as_secs_f64() * 1000.0,
            "measurement_elapsed_ms": end.elapsed().as_secs_f64() * 1000.0
        })
    } else {
        index.board = true;
        let mut samples = Vec::with_capacity(6);
        for group_by in [
            IssueGroupBy::Status,
            IssueGroupBy::Priority,
            IssueGroupBy::Assignee,
            IssueGroupBy::Project,
            IssueGroupBy::Cycle,
            IssueGroupBy::Milestone,
        ] {
            index.set_query(ProjectionQuery::Issues {
                query: IssueQuery::default(),
                group_by: Some(group_by),
            });
            index.resize_board(3, 6);
            samples.push(
                mounted_board_wait(&mut index, Duration::from_secs(30)).as_secs_f64() * 1000.0,
            );
            assert!(
                index.board_columns.len() <= 3
                    && index
                        .board_columns
                        .iter()
                        .all(|column| column.page.rows.len() <= 6),
                "issue board materialized an unbounded window: {:?}",
                index.board_columns
            );
        }
        let mut ordered = samples.clone();
        ordered.sort_by(f64::total_cmp);
        let end = Instant::now();
        index.end();
        let end_navigation = mounted_board_wait(&mut index, Duration::from_secs(30));
        serde_json::json!({
            "kind": "issue_board",
            "samples": samples.len(),
            "warm_ms": {
                "p50": ordered[ordered.len() / 2],
                "p95": ordered[(ordered.len() * 95).div_ceil(100).saturating_sub(1)]
            },
            "end_navigation_ms": end_navigation.as_secs_f64() * 1000.0,
            "measurement_elapsed_ms": end.elapsed().as_secs_f64() * 1000.0
        })
    };
    let selected = index.selected_row().unwrap();
    assert_eq!(index.viewport().selected(), Some((total - 1) as u64));
    assert_eq!(
        selected.token.key.kind,
        if total == 40_000 {
            SnapshotKind::Feature
        } else {
            SnapshotKind::Issue
        }
    );
    assert_eq!(
        selected.token.view,
        index.handle.as_ref().unwrap().view,
        "end navigation selected a row from another projection"
    );
    index.begin_shutdown();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !index.finish_shutdown() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
    serde_json::json!({
        "records": total,
        "open_ms": open_elapsed.as_secs_f64() * 1000.0,
        "warm_filter_page_ms": { "samples": warm_samples.len(), "p50": p50, "p95": p95 },
        "end_navigation_ms": alternate["end_navigation_ms"],
        "alternate": alternate,
        "peak_rss_bytes": mounted_rss_bytes(),
        "probe_elapsed_ms": cold.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Drives the mounted TUI reader over both required full-size datasets, including
/// feature-tree and issue-board projections. This is intentionally opt-in because it
/// writes tens of thousands of temporary files;
/// run with `cargo test --release -p workdeck-tui mounted_full_size_workbench_probe
/// --lib -- --ignored --nocapture` when qualifying a host.
#[test]
#[ignore = "full-size mounted probe is an explicit qualification workload"]
fn mounted_full_size_workbench_probe() {
    let features = tempfile::tempdir().unwrap();
    let feature_repository = workdeck_pm::Repository::init(features.path(), "WD").unwrap();
    mounted_features(feature_repository.root(), &feature_repository, 40_000);
    let feature_result = mounted_probe(
        features.path(),
        ProjectionQuery::Features {
            query: ProjectionFeatureQuery::default(),
        },
        |sample| ProjectionQuery::Features {
            query: ProjectionFeatureQuery {
                query: format!("bucket {}", sample % 10),
                ..Default::default()
            },
        },
        40_000,
    );

    let issues = tempfile::tempdir().unwrap();
    let issue_repository = workdeck_pm::Repository::init(issues.path(), "WD").unwrap();
    mounted_issues(issue_repository.root(), &issue_repository, 10_000);
    let issue_result = mounted_probe(
        issues.path(),
        ProjectionQuery::default(),
        |sample| ProjectionQuery::Issues {
            query: workdeck_pm::IssueQuery {
                query: format!("bucket {}", sample % 10),
                ..Default::default()
            },
            group_by: None,
        },
        10_000,
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "probe": "mounted_full_size_workbench",
            "environment": {
                "os": std::env::consts::OS,
                "arch": std::env::consts::ARCH,
                "parallelism": thread::available_parallelism().map(|n| n.get()).ok(),
            },
            "features": feature_result,
            "issues": issue_result,
        }))
        .unwrap()
    );
}

#[test]
fn indexed_end_home_and_locate_request_only_the_destination_window() {
    let (mut workspace, reads, view) = loaded(500);
    assert_eq!(workspace.visible_rows().count(), 5);
    workspace.end();
    let (offset, limit) = answer_page(&mut workspace, &reads);
    assert!(offset > 39_980);
    assert!(limit <= 21);
    assert_eq!(
        workspace.selected_row().unwrap().token.key.id,
        "issue-39999"
    );
    workspace.home();
    assert_eq!(answer_page(&mut workspace, &reads).0, 0);
    let target = row(&view, 12_345).token.key;
    workspace.locate(target.clone());
    let (request, reply) = next(&mut workspace, &reads);
    let ProjectionRead::Locate { handle, key } = request else {
        panic!("Expected locate")
    };
    assert_eq!(key, target);
    respond(
        reply,
        &view,
        ProjectionValue::Located {
            handle,
            key,
            ordinal: Some(12_345),
        },
    );
    answer_page(&mut workspace, &reads);
    assert_eq!(workspace.selected_row().unwrap().token.key, target);
    assert_eq!(workspace.visible_rows().count(), 5);
    workspace.move_page(false);
    workspace.move_by(1);
    assert!(workspace.viewport().selected().unwrap() < 12_345);
}

#[test]
fn opening_and_refresh_keep_the_exact_original_citation_and_show_stale_errors() {
    let (mut workspace, reads, old) = loaded(500);
    let selected = workspace.selected_row().unwrap().clone();
    workspace.open_selected();
    let (request, reply) = next(&mut workspace, &reads);
    assert!(matches!(&request, ProjectionRead::Detail(token) if token == &selected.token));
    workspace.refresh(false);
    respond(
        reply,
        &old,
        ProjectionValue::Detail(detail(selected.clone(), "Original inspected document")),
    );
    let (_, refresh) = next(&mut workspace, &reads);
    let mut new = old.clone();
    new.generation = ContentHash::of(b"generation 2");
    new.source.content = ContentHash::of(b"changed source");
    respond(refresh, &new, ProjectionValue::Refreshed(new.clone()));
    answer_query(&mut workspace, &reads, 40_000, Some(0));
    answer_page(&mut workspace, &reads);
    assert_eq!(workspace.handle.as_ref().unwrap().view, new);
    assert_eq!(workspace.opened.as_ref().unwrap().row.token, selected.token);
    assert_eq!(
        workspace.opened.as_ref().unwrap().document.as_deref(),
        Some("Original inspected document")
    );
    assert!(!workspace.stale());
    workspace.refresh(false);
    let (_, reply) = next(&mut workspace, &reads);
    let mut failed = status(&new);
    failed.state = ProjectionState::Error;
    reply
        .send(ProjectionReply {
            retained: None,
            status: failed,
            result: Err(PmError::new(
                ErrorCode::InvalidSchema,
                "Malformed direct editor change",
            )),
        })
        .unwrap();
    until(&mut workspace, |workspace| !workspace.refreshing);
    assert!(workspace.stale());
    assert!(
        workspace
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("Malformed")
    );
    assert_eq!(workspace.visible_rows().count(), 5);
    assert_eq!(workspace.opened.as_ref().unwrap().row.token, selected.token);
}

#[test]
fn a_different_detail_or_source_slot_never_replaces_the_opened_document() {
    let (mut workspace, reads, view) = loaded(500);
    workspace.open_selected();
    let (_, reply) = next(&mut workspace, &reads);
    let first = workspace.selected_row().unwrap().clone();
    respond(
        reply,
        &view,
        ProjectionValue::Detail(detail(first.clone(), "Retained")),
    );
    until(&mut workspace, |workspace| workspace.opening.is_none());
    workspace.move_by(1);
    workspace.open_selected();
    let (_, reply) = next(&mut workspace, &reads);
    respond(
        reply,
        &view,
        ProjectionValue::Detail(detail(first.clone(), "Wrong row")),
    );
    until(&mut workspace, |workspace| workspace.error.is_some());
    assert_eq!(
        workspace.error.as_ref().unwrap().code,
        ErrorCode::StaleSource
    );
    assert!(workspace.opening.is_none());
    assert_eq!(
        workspace.opened.as_ref().unwrap().document.as_deref(),
        Some("Retained")
    );
    workspace.refresh(false);
    let (_, reply) = next(&mut workspace, &reads);
    let mut replaced = view.clone();
    replaced.slot = ContentHash::of(b"another checkout");
    respond(
        reply,
        &replaced,
        ProjectionValue::Refreshed(replaced.clone()),
    );
    until(&mut workspace, |workspace| !workspace.refreshing);
    assert_eq!(workspace.handle.as_ref().unwrap().view, view);
    assert!(workspace.stale());
    assert!(
        workspace
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("source slot")
    );
    assert_eq!(
        workspace.opened.as_ref().unwrap().document.as_deref(),
        Some("Retained")
    );
}

#[test]
fn returning_to_cached_rows_discards_an_in_flight_end_page_and_failed_pages_can_retry() {
    let (mut workspace, reads, view) = loaded(500);
    workspace.end();
    let (request, reply) = next(&mut workspace, &reads);
    let ProjectionRead::Page {
        handle,
        offset,
        limit,
    } = request
    else {
        panic!("Expected page")
    };
    workspace.home();
    let count = limit.min(handle.total - offset);
    respond(
        reply,
        &view,
        ProjectionValue::Page(ProjectionPage {
            rows: (offset..offset + count)
                .map(|index| row(&view, index))
                .collect(),
            handle,
            offset,
            next_offset: None,
        }),
    );
    until(&mut workspace, |workspace| !workspace.loading_page);
    assert_eq!(workspace.selected_row().unwrap().token.key.id, "issue-0");
    workspace.end();
    let (_, reply) = next(&mut workspace, &reads);
    reply
        .send(ProjectionReply {
            retained: None,
            status: status(&view),
            result: Err(PmError::new(
                ErrorCode::StaleSource,
                "Generation temporarily unavailable",
            )),
        })
        .unwrap();
    until(&mut workspace, |workspace| !workspace.loading_page);
    workspace.end();
    answer_page(&mut workspace, &reads);
    assert_eq!(
        workspace.selected_row().unwrap().token.key.id,
        "issue-39999"
    );
}

#[test]
fn invalid_query_retains_membership_but_never_labels_the_old_predicate_current() {
    let (mut workspace, reads, view) = loaded(500);
    let old = workspace.handle.clone();
    workspace.set_query(ProjectionQuery::Issues {
        query: workdeck_pm::IssueQuery {
            status: Some("unknown".into()),
            ..Default::default()
        },
        group_by: None,
    });
    assert!(workspace.stale());
    let (_, reply) = next(&mut workspace, &reads);
    reply
        .send(ProjectionReply {
            retained: None,
            status: status(&view),
            result: Err(PmError::new(ErrorCode::InvalidInput, "Unknown status")),
        })
        .unwrap();
    until(&mut workspace, |workspace| !workspace.querying);
    assert!(workspace.stale());
    assert_eq!(workspace.handle, old);
    assert_eq!(workspace.visible_rows().count(), 5);
}

#[test]
fn a_one_row_response_limit_keeps_navigation_bounded_and_shutdown_owned() {
    let (mut workspace, reads, _) = loaded(1);
    assert_eq!(workspace.visible_rows().count(), 1);
    workspace.end();
    assert_eq!(answer_page(&mut workspace, &reads), (39_999, 1));
    workspace.begin_shutdown();
    until(&mut workspace, |workspace| {
        !workspace.refreshing && !workspace.loading_page
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while !workspace.finish_shutdown() {
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn cold_start_can_query_a_retained_cache_without_calling_its_failed_refresh_current() {
    let (mut workspace, reads) = harness(500);
    let view = identity();
    let mut retained_status = status(&view);
    retained_status.state = ProjectionState::Stale;
    retained_status.diagnostics.push(PmError::new(
        ErrorCode::InvalidSchema,
        "Current config is malformed",
    ));
    workspace.resize(5);
    workspace.refresh(false);
    let (_, reply) = next(&mut workspace, &reads);
    reply
        .send(ProjectionReply {
            retained: Some(view.clone()),
            status: retained_status.clone(),
            result: Err(retained_status.diagnostics[0].clone()),
        })
        .unwrap();
    let (request, reply) = next(&mut workspace, &reads);
    assert!(matches!(request, ProjectionRead::Query { view: captured, .. } if captured == view));
    let handle = ProjectionQueryHandle {
        view: view.clone(),
        query: ContentHash::of(b"cached query"),
        total: 10_000,
    };
    reply
        .send(ProjectionReply {
            retained: Some(view.clone()),
            status: retained_status.clone(),
            result: Ok(ProjectionValue::Query {
                handle: handle.clone(),
                groups: Vec::new(),
                located: None,
            }),
        })
        .unwrap();
    let (request, reply) = next(&mut workspace, &reads);
    let ProjectionRead::Page { offset, limit, .. } = request else {
        panic!("Expected cached page")
    };
    reply
        .send(ProjectionReply {
            retained: Some(view.clone()),
            status: retained_status,
            result: Ok(ProjectionValue::Page(ProjectionPage {
                handle,
                offset,
                rows: (offset..offset + limit)
                    .map(|index| row(&view, index))
                    .collect(),
                next_offset: Some(offset + limit),
            })),
        })
        .unwrap();
    until(&mut workspace, |workspace| !workspace.loading_page);
    assert!(workspace.stale());
    assert_eq!(workspace.visible_rows().count(), 5);
    assert!(
        workspace.status.as_ref().unwrap().diagnostics[0]
            .message
            .contains("malformed")
    );
}

#[test]
fn virtual_rendering_keeps_end_visible_at_narrow_and_wide_sizes_with_exact_mouse_tokens() {
    use ratatui::{buffer::Buffer, layout::Rect};
    let (mut workspace, reads, _) = loaded(500);
    workspace.end();
    answer_page(&mut workspace, &reads);
    let selected = workspace.selected_row().unwrap().token.clone();
    let theme = crate::resolve_theme(None, None, &[]);
    for width in [72, 132] {
        let area = Rect::new(4, 3, width, 18);
        let mut buffer = Buffer::empty(Rect::new(0, 0, width + 8, 25));
        for cell in &mut buffer.content {
            cell.set_symbol("·");
        }
        let painted = super::indexed_view::render(&mut workspace, area, &mut buffer, &theme);
        assert!(painted.rows.iter().any(|(_, token)| token == &selected));
        assert!(painted.rows.len() <= usize::from(painted.list.height.saturating_sub(2) / 2));
        assert_eq!(painted.rows.len(), if width == 72 { 2 } else { 6 });
        for (hit, token) in &painted.rows {
            assert_eq!(hit.intersection(painted.list), *hit);
            assert_eq!(token.view, selected.view);
        }
        let text = (area.y..area.bottom())
            .map(|y| {
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Row 39999"), "{text}");
        assert!(text.contains("observation unavailable"), "{text}");
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if !area.contains((x, y).into()) {
                    assert_eq!(buffer[(x, y)].symbol(), "·");
                }
            }
        }
    }
    assert!(workspace.select_token(&selected));
    let mut stale_token = selected.clone();
    stale_token.content = ContentHash::of(b"previously rendered bytes");
    assert!(!workspace.select_token(&stale_token));
    assert_eq!(workspace.selected_row().unwrap().token, selected);
}

#[test]
fn indexed_navigation_leaves_interrupt_quit_and_global_tab_keys_to_the_host() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let (mut workspace, reads, _) = loaded(500);
    for event in [
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
        KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE),
    ] {
        assert!(!workspace.key(event));
    }
    assert!(workspace.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)));
    assert_eq!(workspace.viewport().selected(), Some(5));
    assert!(workspace.key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE)));
    assert_eq!(workspace.viewport().selected(), Some(0));
    assert!(workspace.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
    answer_page(&mut workspace, &reads);
    assert_eq!(workspace.viewport().selected(), Some(39_999));
}

#[test]
fn board_rejects_an_unrequested_column_window_without_replacing_inspected_cards() {
    let (mut workspace, reads, view) = loaded(500);
    workspace.board = true;
    workspace.groups = vec![ProjectionGroup {
        value: None,
        count: 40_000,
    }];
    workspace.resize_board(1, 2);
    let (input, reply) = next(&mut workspace, &reads);
    let ProjectionRead::Board { handle, request } = input else {
        panic!("board request expected")
    };
    let valid = ProjectionBoardColumn {
        group: workspace.groups[0].clone(),
        page: ProjectionPage {
            handle: handle.clone(),
            offset: 0,
            rows: vec![row(&view, 0), row(&view, 1)],
            next_offset: Some(2),
        },
    };
    respond(
        reply,
        &view,
        ProjectionValue::Board {
            handle,
            request,
            columns: vec![valid.clone()],
        },
    );
    until(&mut workspace, |workspace| workspace.is_idle());
    assert_eq!(workspace.board_columns, vec![valid.clone()]);
    workspace.resize_board(1, 3);
    let (input, reply) = next(&mut workspace, &reads);
    let ProjectionRead::Board { handle, request } = input else {
        panic!("board request expected")
    };
    let mut wrong = valid.clone();
    wrong.page.offset = 39_997;
    wrong.page.rows = (39_997..40_000).map(|index| row(&view, index)).collect();
    respond(
        reply,
        &view,
        ProjectionValue::Board {
            handle,
            request,
            columns: vec![wrong],
        },
    );
    until(&mut workspace, |workspace| workspace.is_idle());
    assert_eq!(
        workspace.error.as_ref().map(|error| error.code),
        Some(ErrorCode::StaleSource)
    );
    assert_eq!(workspace.board_columns, vec![valid]);
}
