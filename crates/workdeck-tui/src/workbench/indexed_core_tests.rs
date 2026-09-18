use super::indexed_workspace::IndexedWorkspace;
use std::{
    thread,
    time::{Duration, Instant},
};
use workdeck_pm::{
    CreateFeature, CreateIssue, IssueRecord, Repository, RequestId, SourceSelector, UpdateIssue,
    projection::*,
};

fn until(workspace: &mut IndexedWorkspace, ready: impl Fn(&IndexedWorkspace) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        workspace.poll();
        if ready(workspace) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Real projection did not settle: {workspace:?}"
        );
        thread::sleep(Duration::from_millis(1));
    }
}
fn open(root: &std::path::Path) -> IndexedWorkspace {
    let mut workspace = IndexedWorkspace::new(
        root.into(),
        SourceSelector::WorkingTree,
        ProjectionQuery::default(),
        ProjectionLimits::default(),
    )
    .unwrap();
    workspace.resize(6);
    workspace.refresh(false);
    workspace
}
fn rows_ready(workspace: &IndexedWorkspace) -> bool {
    !workspace.refreshing
        && !workspace.querying
        && !workspace.loading_page
        && workspace.page.is_some()
}

#[test]
fn real_issue_and_feature_queries_keep_opened_source_through_edits_and_query_switches() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repo.create_issue(
            &CreateIssue::new("Indexed issue", "Original body"),
            &RequestId::new(),
        )
        .unwrap()
        .result,
    )
    .unwrap();
    repo.create_feature(
        &CreateFeature::new("Independent feature states"),
        &RequestId::new(),
    )
    .unwrap();
    let feature = repo.list_features().unwrap().remove(0);
    let mut workspace = open(directory.path());
    until(&mut workspace, rows_ready);
    assert!(workspace.error.is_none(), "{:?}", workspace.error);
    assert_eq!(
        workspace.selected_row().unwrap().token.content,
        issue.source.content
    );
    workspace.open_selected();
    until(&mut workspace, |workspace| workspace.opening.is_none());
    let original = workspace.opened.clone().unwrap();
    assert_eq!(
        original.document.as_deref(),
        Some(
            repo.issue_markdown(issue.metadata.id.as_str())
                .unwrap()
                .as_str()
        )
    );
    repo.update_issue(
        issue.metadata.id.as_str(),
        &issue.source,
        &UpdateIssue {
            fields: std::collections::BTreeMap::from([(
                "title".into(),
                serde_json::json!("Changed externally"),
            )]),
            body: None,
        },
        &RequestId::new(),
    )
    .unwrap();
    workspace.refresh(false);
    until(&mut workspace, rows_ready);
    assert_eq!(
        workspace.selected_row().unwrap().title,
        "Changed externally"
    );
    assert_ne!(
        workspace.selected_row().unwrap().token.content,
        original.row.token.content
    );
    assert_eq!(workspace.opened.as_ref(), Some(&original));
    workspace.set_query(ProjectionQuery::Features {
        query: ProjectionFeatureQuery::default(),
    });
    until(&mut workspace, rows_ready);
    let row = workspace.selected_row().unwrap();
    assert_eq!(row.token.key.id, feature.metadata.id.to_string());
    assert_eq!(row.decision, Some(feature.metadata.decision));
    assert_eq!(row.maturity, Some(feature.metadata.maturity));
    assert_eq!(row.availability, Some(feature.metadata.availability));
    assert_eq!(workspace.opened.as_ref(), Some(&original));
    workspace.open_selected();
    until(&mut workspace, |workspace| workspace.opening.is_none());
    assert_eq!(
        workspace.opened.as_ref().unwrap().row.token.key.id,
        feature.metadata.id.to_string()
    );
}

#[test]
fn real_cold_start_keeps_a_bound_cached_view_when_current_configuration_is_malformed() {
    let directory = tempfile::tempdir().unwrap();
    let repo = Repository::init(directory.path(), "WD").unwrap();
    repo.create_issue(
        &CreateIssue::new("Retained cached issue", "Body"),
        &RequestId::new(),
    )
    .unwrap();
    let mut workspace = open(directory.path());
    until(&mut workspace, rows_ready);
    let original = workspace.handle.as_ref().unwrap().view.clone();
    drop(workspace);
    std::fs::write(repo.root().join("config.yml"), "schema: [malformed\n").unwrap();
    let mut workspace = open(directory.path());
    until(&mut workspace, rows_ready);
    assert_eq!(workspace.handle.as_ref().unwrap().view, original);
    assert_eq!(
        workspace.selected_row().unwrap().title,
        "Retained cached issue"
    );
    assert!(workspace.stale());
    assert!(!workspace.status.as_ref().unwrap().diagnostics.is_empty());
    assert_ne!(
        workspace.status.as_ref().unwrap().state,
        ProjectionState::Current
    );
}

#[test]
fn indexed_authoring_keeps_one_native_selection_and_never_rebases_a_failed_draft() {
    use super::{DraftInput, WorkbenchController};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let first: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("First", "Original body"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Second", "Second body"),
            &RequestId::new(),
        )
        .unwrap();
    let mut store = ProjectionStore::open(
        directory.path(),
        SourceSelector::WorkingTree,
        ProjectionLimits::default(),
    )
    .unwrap();
    let ProjectionRefresh::Published(view) =
        store.refresh(&ProjectionRefreshRequest::default()).unwrap()
    else {
        panic!("new projection expected")
    };
    let query = view.query(&ProjectionQuery::default()).unwrap();
    let rows = view.page(&query, 0, 10).unwrap().rows;
    let first_row = rows
        .iter()
        .find(|row| row.token.key.id == first.metadata.id.as_str())
        .unwrap();
    let second_row = rows
        .iter()
        .find(|row| row.token.key.id != first.metadata.id.as_str())
        .unwrap();
    let mut controller = WorkbenchController::new_indexed(repository.clone());
    controller.refresh().unwrap();
    assert!(controller.issues().is_empty());
    controller.select_projection(&first_row.token).unwrap();
    let draft = controller.begin_edit().unwrap();
    if let DraftInput::Edit(input) = &mut controller.draft_mut(&draft).unwrap().input {
        input.body = Some("Retained user draft".into());
    } else {
        panic!("edit draft expected")
    }
    repository
        .update_issue(
            first.metadata.id.as_str(),
            &first.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::new(),
                body: Some("External change".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    assert!(controller.submit_draft(&draft).is_err());
    let request = controller.drafts()[&draft].request_id().unwrap().clone();
    let source = controller.drafts()[&draft].source().unwrap().clone();
    controller.select_projection(&second_row.token).unwrap();
    controller.refresh().unwrap();
    assert_eq!(controller.issues().len(), 1);
    assert_eq!(
        controller.selected_issue().unwrap().metadata.title,
        "Second"
    );
    assert!(controller.select_projection(&first_row.token).is_err());
    assert_eq!(
        controller.selected_issue().unwrap().metadata.title,
        "Second"
    );
    let retained = &controller.drafts()[&draft];
    assert_eq!(retained.source(), Some(&source));
    assert_eq!(retained.request_id(), Some(&request));
    assert!(
        matches!(&retained.input, DraftInput::Edit(input) if input.body.as_deref() == Some("Retained user draft"))
    );
    assert_eq!(
        repository
            .show_issue(first.metadata.id.as_str())
            .unwrap()
            .body,
        "External change"
    );
}

#[test]
fn normal_shell_does_not_materialize_the_whole_issue_collection_for_navigation() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    for title in ["One", "Two", "Three"] {
        repository
            .create_issue(&CreateIssue::new(title, "Body"), &RequestId::new())
            .unwrap();
    }
    let shell = super::WorkbenchShell::open(super::WorkbenchOptions::new(directory.path()), true);
    assert_eq!(shell.tab, super::WorkbenchTab::Issues);
    assert!(
        shell.controller.issues().len() <= 1,
        "normal startup retained {} full native issue records; collection browsing belongs to the bounded index",
        shell.controller.issues().len()
    );
}

fn shell_until(shell: &mut super::WorkbenchShell, ready: impl Fn(&super::WorkbenchShell) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        shell.poll_index(false);
        if ready(shell) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "mounted index did not settle: {:?}",
            shell.notice
        );
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn mounted_index_navigates_opens_and_retains_selection_across_review_tabs() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    for title in ["First", "Second", "Third"] {
        repository
            .create_issue(
                &CreateIssue::new(title, format!("Body for {title}")),
                &RequestId::new(),
            )
            .unwrap();
    }
    let mut shell =
        super::WorkbenchShell::open(super::WorkbenchOptions::new(directory.path()), true);
    shell_until(&mut shell, |shell| {
        shell.index.as_ref().is_some_and(rows_ready)
    });
    assert_eq!(
        shell.index.as_ref().unwrap().handle.as_ref().unwrap().total,
        3
    );
    let theme = crate::resolve_theme(None, None, &[]);
    for width in [72, 132] {
        let area = ratatui::layout::Rect::new(0, 0, width, 22);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        shell.render_planning(area, &mut buffer, &theme);
        let rendered = shell.index_rendered.as_ref().unwrap();
        assert!(!rendered.rows.is_empty());
        assert!(
            rendered
                .rows
                .iter()
                .all(|(rect, _)| rect.bottom() <= rendered.list.bottom())
        );
    }
    assert!(shell.key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE)));
    shell_until(&mut shell, |shell| {
        shell
            .controller
            .selected_issue()
            .is_some_and(|issue| issue.metadata.title == "Third")
    });
    let selected = shell.controller.selected_id().unwrap().clone();
    assert!(shell.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)));
    shell_until(&mut shell, |shell| {
        shell.index.as_ref().unwrap().opened.is_some()
    });
    let opened = shell.index.as_ref().unwrap().opened.clone().unwrap();
    assert!(
        opened
            .document
            .as_deref()
            .unwrap()
            .contains("Body for Third")
    );
    shell.key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    assert_eq!(shell.tab, super::WorkbenchTab::Review);
    shell.key(KeyEvent::new(KeyCode::F(3), KeyModifiers::NONE));
    shell_until(&mut shell, |shell| {
        shell.index.as_ref().is_some_and(rows_ready)
    });
    assert_eq!(shell.controller.selected_id(), Some(&selected));
    assert_eq!(shell.index.as_ref().unwrap().opened, Some(opened));
    assert_eq!(shell.controller.issues().len(), 1);
}

#[test]
fn mounted_stale_row_cannot_open_an_edit_against_new_source_content() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new("Before", "Original"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut shell =
        super::WorkbenchShell::open(super::WorkbenchOptions::new(directory.path()), true);
    shell_until(&mut shell, |shell| {
        shell.controller.selected_issue().is_some()
    });
    repository
        .update_issue(
            issue.metadata.id.as_str(),
            &issue.source,
            &UpdateIssue {
                fields: std::collections::BTreeMap::new(),
                body: Some("External edit".into()),
            },
            &RequestId::new(),
        )
        .unwrap();
    shell.key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
    assert!(shell.form.is_none());
    assert!(shell.controller.drafts().is_empty());
    assert!(shell.index.as_ref().unwrap().stale());
    assert!(
        shell
            .notice
            .as_deref()
            .unwrap()
            .contains("indexed citation differs")
    );
}

#[test]
fn mounted_excerpt_scrolling_preserves_list_selection_and_opened_source() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let body = (0..200)
        .map(|line| format!("Evidence line {line}\n"))
        .collect::<String>();
    repository
        .create_issue(&CreateIssue::new("Long source", body), &RequestId::new())
        .unwrap();
    let mut shell =
        super::WorkbenchShell::open(super::WorkbenchOptions::new(directory.path()), true);
    super::test_pump::settle_shell(&mut shell);
    shell.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    super::test_pump::settle_shell(&mut shell);
    let opened = shell.index.as_ref().unwrap().opened.clone().unwrap();
    let selected = shell.controller.selected_id().cloned();
    assert!(
        shell.key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::SHIFT)),
        "opened indexed excerpts need an independent scroll control"
    );
    assert!(shell.index.as_ref().unwrap().detail_scroll > 0);
    assert_eq!(shell.controller.selected_id(), selected.as_ref());
    assert_eq!(shell.index.as_ref().unwrap().opened, Some(opened));
    assert!(shell.key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::SHIFT)));
    assert_eq!(shell.index.as_ref().unwrap().detail_scroll, 0);
}

#[test]
fn batched_initial_context_keys_wait_for_the_native_issue_selection() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Batched context", "Accepted scope"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let mut shell =
        super::WorkbenchShell::open(super::WorkbenchOptions::new(directory.path()), true);
    for code in [KeyCode::F(3), KeyCode::Char('i'), KeyCode::Char('7')] {
        shell.key(KeyEvent::new(code, KeyModifiers::NONE));
    }
    shell_until(&mut shell, |shell| {
        shell.context_visible && shell.context.claims.current.as_ref() == Some(&issue.metadata.id)
    });
    assert!(shell.index_input.is_none());
    assert!(
        repository.claims().unwrap().is_empty(),
        "opening queued context must not acquire a claim"
    );
}

#[test]
fn an_action_immediately_after_save_waits_for_the_same_issue_new_source() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue: IssueRecord = serde_json::from_value(
        repository
            .create_issue(
                &CreateIssue::new("Sequential actions", "Body"),
                &RequestId::new(),
            )
            .unwrap()
            .result,
    )
    .unwrap();
    let mut shell =
        super::WorkbenchShell::open(super::WorkbenchOptions::new(directory.path()), true);
    super::test_pump::settle_shell(&mut shell);
    shell.key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
    shell.form.as_mut().unwrap().fields[0].value = "high".into();
    shell.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    shell.key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
    shell_until(&mut shell, |shell| {
        shell
            .form
            .as_ref()
            .is_some_and(|form| form.title == "Edit labels")
    });
    let current = repository.show_issue(issue.metadata.id.as_str()).unwrap();
    assert_eq!(current.metadata.priority, workdeck_pm::Priority::High);
    let super::input::FormKind::Labels(target) = &shell.form.as_ref().unwrap().kind else {
        panic!("label form expected")
    };
    assert_eq!(target.issue, issue.metadata.id);
    assert_eq!(target.source, current.source);
    assert!(shell.index_input.is_none());
}

#[test]
fn shared_working_tree_selection_reopens_native_record_but_proposal_ref_does_not() {
    let (directory, repository, issue) = super::source_tests::shared_fixture();
    let mut workspace = open(directory.path());
    until(&mut workspace, rows_ready);
    let row = workspace.selected_row().unwrap();
    assert_eq!(
        row.token.view.source.role,
        workdeck_pm::SourceRole::Proposal
    );
    let native = repository.issue_from_projection(&row.token).unwrap();
    assert_eq!(native.metadata.id, issue.metadata.id);
    assert_eq!(native.metadata.title, "Uncommitted title");
    let mut store = ProjectionStore::open(
        directory.path(),
        SourceSelector::Proposal {
            reference: "refs/heads/workdeck-proposals/demo".parse().unwrap(),
        },
        ProjectionLimits::default(),
    )
    .unwrap();
    let ProjectionRefresh::Published(view) =
        store.refresh(&ProjectionRefreshRequest::default()).unwrap()
    else {
        panic!("new projection expected")
    };
    let query = view.query(&ProjectionQuery::default()).unwrap();
    let token = view.page(&query, 0, 1).unwrap().rows.remove(0).token;
    assert!(
        repository.issue_from_projection(&token).is_err(),
        "ref-backed proposal must not acquire working-tree authoring authority"
    );
}

#[test]
fn real_board_navigation_preserves_exact_selected_issue_and_opened_source() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let temporary = tempfile::tempdir().unwrap();
    let repo = Repository::init(temporary.path(), "WD").unwrap();
    for assignee in ["Alice", "Bob", "Carol"] {
        for number in 0..3 {
            repo.create_issue(
                &CreateIssue {
                    title: format!("{assignee} {number}"),
                    body: "Board source".into(),
                    fields: std::collections::BTreeMap::from([(
                        "assignee".into(),
                        serde_json::json!(assignee),
                    )]),
                },
                &RequestId::new(),
            )
            .unwrap();
        }
    }
    let mut workspace = open(temporary.path());
    until(&mut workspace, rows_ready);
    let selected = workspace.selected_row().unwrap().token.key.clone();
    assert!(
        workspace.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)),
        "w must open the board"
    );
    until(&mut workspace, |workspace| {
        rows_ready(workspace) && workspace.is_idle()
    });
    assert_eq!(workspace.selected_row().unwrap().token.key, selected);
    // Status -> priority -> assignee.
    for _ in 0..2 {
        assert!(workspace.key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE)));
        until(&mut workspace, |workspace| {
            rows_ready(workspace) && workspace.is_idle()
        });
    }
    assert!(workspace.key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE)));
    until(&mut workspace, |workspace| {
        rows_ready(workspace) && workspace.is_idle()
    });
    assert_eq!(
        workspace.selected_row().unwrap().assignee.as_deref(),
        Some("Bob")
    );
    workspace.open_selected();
    until(&mut workspace, |workspace| workspace.opening.is_none());
    let opened = workspace.opened.clone();
    assert!(workspace.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::NONE)));
    until(&mut workspace, |workspace| {
        rows_ready(workspace) && workspace.is_idle()
    });
    assert_eq!(
        workspace.selected_row().unwrap().assignee.as_deref(),
        Some("Bob")
    );
    assert_eq!(workspace.opened, opened);
}
