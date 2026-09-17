use super::*;
use tempfile::{TempDir, tempdir};
use workdeck_pm::{CreateIssue, IssueRecord, Repository, RequestId};

fn setup() -> (TempDir, Repository, WorkbenchController) {
    let directory = tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let controller = WorkbenchController::new(repository.clone());
    (directory, repository, controller)
}

fn create(repository: &Repository, title: &str) -> IssueRecord {
    let receipt = repository
        .create_issue(&CreateIssue::new(title, "Details"), &RequestId::new())
        .unwrap();
    serde_json::from_value(receipt.result).unwrap()
}

#[test]
fn refresh_loads_issues_and_preserves_stable_selection() {
    let (_directory, repository, mut controller) = setup();
    let first = create(&repository, "First issue");
    controller.refresh().unwrap();
    assert_eq!(controller.selected_id(), Some(&first.metadata.id));
    let second = create(&repository, "Second issue");
    controller.refresh().unwrap();
    assert_eq!(controller.visible_issues().len(), 2);
    assert_eq!(controller.selected_id(), Some(&first.metadata.id));
    assert!(controller.select(&second.metadata.id));
    create(&repository, "Third issue");
    controller.refresh().unwrap();
    assert_eq!(controller.selected_id(), Some(&second.metadata.id));
}

use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use serde_json::json;
use std::{collections::BTreeMap, fs};
use workdeck_pm::{ErrorCode, IssueMutation, PmError, Priority, SourceLink, UpdateIssue};

fn create_fields(repository: &Repository, title: &str, fields: serde_json::Value) -> IssueRecord {
    let input = CreateIssue {
        title: title.into(),
        body: "Body visible in detail".into(),
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

#[test]
fn filters_search_ownership_labels_and_archived_rows_without_mutation() {
    let (_directory, repository, mut controller) = setup();
    let mut label = workdeck_pm::CreatePlanning::new("Bug");
    label.id = Some("bug".into());
    repository
        .create_planning(workdeck_pm::PlanningKind::Label, &label, &RequestId::new())
        .unwrap();
    let first = create_fields(
        &repository,
        "Unicode café parser",
        json!({"assignee":"agent","labels":["bug"],"priority":"high"}),
    );
    create_fields(&repository, "Other issue", json!({"archived":true}));
    controller.refresh().unwrap();
    assert_eq!(controller.visible_issues().len(), 1);
    let before = repository
        .issue_markdown(first.metadata.id.as_str())
        .unwrap();
    controller
        .set_filter(IssueFilter {
            query: "CAFÉ".into(),
            assignee: Some("agent".into()),
            label: Some("bug".into()),
            priority: Some(Priority::High),
            ..IssueFilter::default()
        })
        .unwrap();
    assert_eq!(controller.selected_id(), Some(&first.metadata.id));
    controller
        .set_filter(IssueFilter {
            query: "absent".into(),
            ..IssueFilter::default()
        })
        .unwrap();
    assert_eq!(controller.selected_id(), None);
    controller.move_selection(isize::MIN);
    controller
        .set_filter(IssueFilter {
            include_archived: true,
            ..IssueFilter::default()
        })
        .unwrap();
    assert_eq!(controller.visible_issues().len(), 2);
    controller.move_selection(isize::MAX);
    assert_eq!(controller.selected_index(), Some(1));
    assert_eq!(
        repository
            .issue_markdown(first.metadata.id.as_str())
            .unwrap(),
        before
    );
}

#[test]
fn create_edit_comment_status_and_assignment_use_shared_mutations() {
    let (_directory, repository, mut controller) = setup();
    let link = SourceLink {
        path: "src/lib.rs".into(),
        line: Some(12),
        end_line: Some(15),
    };
    let key = controller
        .begin_create_from_file(link.clone(), "New work".into(), "Initial body".into())
        .unwrap();
    controller.submit_draft(&key).unwrap();
    let created = controller.selected_issue().unwrap().clone();
    assert_eq!(created.metadata.files, [link]);
    assert!(controller.drafts().is_empty());
    let edit = controller.begin_edit().unwrap();
    let DraftInput::Edit(input) = &mut controller.draft_mut(&edit).unwrap().input else {
        panic!()
    };
    input.fields.insert("title".into(), json!("Edited title"));
    input.body = Some("Edited body".into());
    controller.submit_draft(&edit).unwrap();
    assert_eq!(
        controller.selected_issue().unwrap().metadata.title,
        "Edited title"
    );
    let source = controller.selected_issue().unwrap().source.clone();
    let comment = controller.begin_comment("maintainer".into()).unwrap();
    let DraftInput::Comment { body, .. } = &mut controller.draft_mut(&comment).unwrap().input
    else {
        panic!()
    };
    *body = "Please preserve the public API".into();
    let receipt = controller.submit_draft(&comment).unwrap();
    assert_eq!(controller.comments().len(), 1);
    assert_eq!(
        controller.comments()[0].body,
        "Please preserve the public API"
    );
    assert_eq!(controller.selected_issue().unwrap().source, source);
    assert_eq!(controller.last_receipt(), Some(&receipt));
    controller
        .perform(
            controller.selected_target().unwrap(),
            IssueAction::Status("in_progress".into()),
        )
        .unwrap();
    controller
        .perform(
            controller.selected_target().unwrap(),
            IssueAction::Assign(Some("agent".into())),
        )
        .unwrap();
    assert_eq!(
        controller
            .selected_issue()
            .unwrap()
            .metadata
            .assignee
            .as_deref(),
        Some("agent")
    );
    controller
        .perform(controller.selected_target().unwrap(), IssueAction::Complete)
        .unwrap();
    assert_eq!(controller.selected_issue().unwrap().metadata.status, "done");
    controller
        .perform(controller.selected_target().unwrap(), IssueAction::Reopen)
        .unwrap();
    controller
        .perform(
            controller.selected_target().unwrap(),
            IssueAction::Archive(true),
        )
        .unwrap();
    assert!(controller.visible_issues().len() == 0);
    assert!(
        repository
            .show_issue(created.metadata.id.as_str())
            .unwrap()
            .metadata
            .archived
    );
}

#[test]
fn refresh_keeps_edit_draft_and_original_source_after_stale_rejection() {
    let (_directory, repository, mut controller) = setup();
    let issue = create(&repository, "Original title");
    controller.refresh().unwrap();
    let key = controller.begin_edit().unwrap();
    let DraftInput::Edit(input) = &mut controller.draft_mut(&key).unwrap().input else {
        panic!()
    };
    input.body = Some("Unsaved user draft".into());
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &IssueMutation::Update {
                input: UpdateIssue {
                    fields: BTreeMap::new(),
                    body: Some("Concurrent authoritative body".into()),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    controller.refresh().unwrap();
    assert_eq!(
        controller.submit_draft(&key).unwrap_err().code,
        ErrorCode::StaleSource
    );
    assert_eq!(controller.error().unwrap().draft.as_ref(), Some(&key));
    controller.refresh().unwrap();
    assert_eq!(
        controller.error().unwrap().error.code,
        ErrorCode::StaleSource
    );
    assert_eq!(controller.drafts()[&key].source(), Some(&issue.source));
    let DraftInput::Edit(input) = &controller.drafts()[&key].input else {
        panic!()
    };
    assert_eq!(input.body.as_deref(), Some("Unsaved user draft"));
    assert_eq!(
        controller.selected_issue().unwrap().body,
        "Concurrent authoritative body"
    );
}

#[test]
fn edited_retry_cannot_duplicate_a_comment_after_uncertain_acknowledgement() {
    let (_directory, repository, mut controller) = setup();
    let issue = create(&repository, "Uncertain comment write");
    controller.refresh().unwrap();
    let key = controller.begin_comment("agent".into()).unwrap();
    let DraftInput::Comment { body, .. } = &mut controller.draft_mut(&key).unwrap().input else {
        panic!()
    };
    *body = "Original attempted comment".into();

    // Keep the attempted request in the controller, then model a successful
    // write whose acknowledgement never reached it.
    let config_path = repository.root().join("config.yml");
    let config = fs::read(&config_path).unwrap();
    fs::write(&config_path, "repository: [malformed\n").unwrap();
    assert!(controller.submit_draft(&key).is_err());
    fs::write(&config_path, config).unwrap();
    let request = controller.drafts()[&key].request_id().unwrap().clone();
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &IssueMutation::Comment {
                author: "agent".into(),
                body: "Original attempted comment".into(),
            },
            &request,
        )
        .unwrap();
    let DraftInput::Comment { body, .. } = &mut controller.draft_mut(&key).unwrap().input else {
        panic!()
    };
    *body = "Edited retained comment".into();

    assert_eq!(
        controller.submit_draft(&key).unwrap_err().code,
        ErrorCode::IdempotencyConflict
    );
    assert_eq!(controller.drafts()[&key].request_id(), Some(&request));
    let comments = repository.comments(issue.metadata.id.as_str()).unwrap();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].body, "Original attempted comment");
    assert!(matches!(
        &controller.drafts()[&key].input,
        DraftInput::Comment { body, .. } if body == "Edited retained comment"
    ));
}

#[test]
fn rejected_completion_keeps_pending_intent_and_does_not_rewrite_issue() {
    let (_directory, repository, mut controller) = setup();
    let issue = create_fields(
        &repository,
        "Acceptance required",
        json!({"acceptance":[{"id":"tested","description":"Test succeeds","checked":false}]}),
    );
    controller.refresh().unwrap();
    assert_eq!(
        controller
            .perform(controller.selected_target().unwrap(), IssueAction::Complete)
            .unwrap_err()
            .code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        controller.retry_pending_action().unwrap_err().code,
        ErrorCode::PolicyBlocked
    );
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
    assert_eq!(
        controller.error().unwrap().context,
        WorkbenchErrorContext::Mutation
    );
}

#[test]
fn malformed_refresh_keeps_visible_snapshot_and_all_drafts() {
    let (_directory, repository, mut controller) = setup();
    let issue = create(&repository, "Keep this snapshot");
    controller.refresh().unwrap();
    let draft = controller.begin_comment("person".into()).unwrap();
    fs::write(
        repository.root().join(&issue.path),
        "---\ntitle: [broken\n---\n",
    )
    .unwrap();
    assert_eq!(
        controller.refresh().unwrap_err().code,
        ErrorCode::InvalidSchema
    );
    assert_eq!(controller.selected_id(), Some(&issue.metadata.id));
    assert_eq!(
        controller.selected_issue().unwrap().metadata.title,
        "Keep this snapshot"
    );
    assert!(controller.drafts().contains_key(&draft));
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NativeReviewContext {
    source: String,
    file: String,
    hunk: usize,
    viewport: usize,
}

#[test]
fn link_navigation_preserves_planning_drafts_and_opaque_review_context() {
    let (_directory, repository, mut controller) = setup();
    let linked = create_fields(
        &repository,
        "Linked issue",
        json!({"files":[{"path":"src/lib.rs","line":12,"end_line":15}],"commits":["abcdef1"]}),
    );
    let other = create(&repository, "Other issue");
    controller.refresh().unwrap();
    controller.select(&linked.metadata.id);
    let draft = controller.begin_edit().unwrap();
    controller.view_mut().detail_scroll = 7;
    controller
        .set_filter(IssueFilter {
            query: "Linked".into(),
            ..IssueFilter::default()
        })
        .unwrap();
    let review = NativeReviewContext {
        source: "working-tree".into(),
        file: "native/file.rs".into(),
        hunk: 4,
        viewport: 27,
    };
    let request = controller.navigate_file(0, review.clone()).unwrap();
    assert_eq!(request.repository, *repository.identity());
    assert_eq!(request.repository_root, repository.root().parent().unwrap());
    assert_eq!(
        request.target,
        IssueReviewTarget::File(linked.metadata.files[0].clone())
    );
    assert_eq!(request.review, review);
    controller.set_filter(IssueFilter::default()).unwrap();
    controller.select(&other.metadata.id);
    assert!(controller.active_draft().is_none());
    controller.restore_context(&request.planning).unwrap();
    assert_eq!(controller.selected_id(), Some(&linked.metadata.id));
    assert_eq!(controller.view().detail_scroll, 7);
    assert_eq!(controller.filter().query, "Linked");
    assert_eq!(controller.active_draft().unwrap().0, &draft);
    assert_eq!(
        controller.navigate_commit(0, ()).unwrap().target,
        IssueReviewTarget::Commit("abcdef1".into())
    );
}

fn paint(
    controller: &mut WorkbenchController,
    area: Rect,
) -> (RenderedWorkbench, ratatui::buffer::Buffer) {
    let mut terminal =
        Terminal::new(TestBackend::new(area.right() + 3, area.bottom() + 3)).unwrap();
    let theme = crate::resolve_theme(None, None, &[]);
    let mut rendered = None;
    terminal
        .draw(|frame| {
            for cell in &mut frame.buffer_mut().content {
                cell.set_symbol("·");
            }
            rendered = Some(render_workbench(frame, area, controller, &theme));
        })
        .unwrap();
    (rendered.unwrap(), terminal.backend().buffer().clone())
}

fn buffer_text(buffer: &ratatui::buffer::Buffer, area: Rect) -> String {
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn narrow_and_wide_views_use_allocated_rectangles_and_keep_selection_visible() {
    let (_directory, repository, mut controller) = setup();
    let mut last = None;
    for index in 0..14 {
        last = Some(create(&repository, &format!("Visible title {index}")));
    }
    controller.refresh().unwrap();
    let last = last.unwrap();
    controller.select(&last.metadata.id);
    for width in [36, 110] {
        let area = Rect::new(4, 3, width, 18);
        let (rendered, buffer) = paint(&mut controller, area);
        assert!(rendered.visible_issue_ids.contains(&last.metadata.id));
        assert!(
            rendered.formatted_issue_rows
                <= usize::from(rendered.layout.list.unwrap().height.saturating_sub(2)).div_ceil(2),
            "formatted {} rows outside the visible viewport",
            rendered.formatted_issue_rows
        );
        assert_eq!(rendered.layout.detail.is_some(), width >= 96);
        let text = buffer_text(&buffer, area);
        assert!(text.contains("Visible title 13"), "{text}");
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                if !area.contains((x, y).into()) {
                    assert_eq!(
                        buffer[(x, y)].symbol(),
                        "·",
                        "outside allocation at {x},{y}"
                    );
                }
            }
        }
    }
    controller.view_mut().pane = WorkbenchPane::Detail;
    let (rendered, buffer) = paint(&mut controller, Rect::new(2, 2, 40, 14));
    assert!(rendered.layout.list.is_none());
    assert!(buffer_text(&buffer, rendered.layout.detail.unwrap()).contains("Details"));
}

#[test]
fn empty_error_and_draft_views_are_safe_at_tiny_sizes() {
    let (_directory, _repository, mut controller) = setup();
    controller.refresh().unwrap();
    let (_, buffer) = paint(&mut controller, Rect::new(3, 2, 35, 10));
    assert!(buffer_text(&buffer, buffer.area).contains("No issues yet"));
    controller.begin_create(CreateIssue::new(
        "Draft title",
        "Text \x1b[31mwith terminal controls",
    ));
    let (_, buffer) = paint(&mut controller, Rect::new(3, 2, 45, 12));
    let text = buffer_text(&buffer, buffer.area);
    assert!(text.contains("Draft title"));
    assert!(!text.contains('\x1b'));
    let mut unavailable = WorkbenchController::unavailable(PmError::new(
        ErrorCode::NotInitialized,
        "Run workdeck init explicitly",
    ));
    let (_, buffer) = paint(&mut unavailable, Rect::new(3, 2, 70, 10));
    assert!(buffer_text(&buffer, buffer.area).contains("Run workdeck init"));
    for width in 0..5 {
        for height in 0..5 {
            paint(&mut unavailable, Rect::new(2, 2, width, height));
        }
    }
}

#[test]
fn captured_action_target_cannot_cross_repository_boundaries() {
    let (_first_dir, first_repo, mut first) = setup();
    let issue = create(&first_repo, "Same file content in two repositories");
    first.refresh().unwrap();
    let target = first.selected_target().unwrap();
    let (_second_dir, second_repo, mut second) = setup();
    let copied_path = second_repo.root().join(&issue.path);
    fs::create_dir_all(copied_path.parent().unwrap()).unwrap();
    fs::copy(first_repo.root().join(&issue.path), &copied_path).unwrap();
    second.refresh().unwrap();
    assert_eq!(second.selected_issue().unwrap().source, target.source);
    assert_eq!(
        second
            .perform(target, IssueAction::Assign(Some("wrong target".into())))
            .unwrap_err()
            .code,
        ErrorCode::StaleSource
    );
    assert_eq!(
        second_repo
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .source,
        issue.source
    );
}

#[test]
fn rejected_query_keeps_last_good_membership_selection_and_draft() {
    let (_directory, repository, mut controller) = setup();
    let first = create(&repository, "Selected issue");
    controller.refresh().unwrap();
    let draft = controller.begin_edit().unwrap();
    let draft_source = controller.drafts()[&draft].source().cloned();
    let before = controller.return_context();
    assert!(
        controller
            .set_filter(IssueFilter {
                status: Some("not-a-workflow-state".into()),
                ..IssueFilter::default()
            })
            .is_err()
    );
    assert_eq!(controller.selected_id(), Some(&first.metadata.id));
    assert_eq!(controller.return_context(), before);
    assert_eq!(controller.drafts()[&draft].source().cloned(), draft_source);
    assert!(controller.error().is_some());
}

#[test]
fn shared_query_aliases_and_sorting_use_cached_source_and_preserve_return_context() {
    use workdeck_pm::{IssueSort, IssueSortField, SortDirection};
    let (_directory, repository, mut controller) = setup();
    let alpha = create_fields(&repository, "Alpha", json!({"status":"ready"}));
    let zebra = create_fields(&repository, "Zebra", json!({"status":"ready"}));
    create_fields(&repository, "Hidden", json!({"status":"inbox"}));
    controller.refresh().unwrap();
    let filter = IssueFilter {
        status: Some("ToDo".into()),
        sort: vec![IssueSort {
            field: IssueSortField::Title,
            direction: SortDirection::Descending,
        }],
        ..IssueFilter::default()
    };
    let queried = repository.query_issues(&filter.query()).unwrap();
    controller.set_filter(filter.clone()).unwrap();
    assert_eq!(
        controller
            .visible_issues()
            .map(|issue| &issue.metadata.id)
            .collect::<Vec<_>>(),
        queried
            .iter()
            .map(|issue| &issue.metadata.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        controller.visible_issues().next().unwrap().metadata.id,
        zebra.metadata.id
    );
    assert_eq!(controller.selected_id(), Some(&alpha.metadata.id));
    let context = controller.return_context();
    // Filtering never reopens authority or consults a different workflow.
    fs::write(repository.root().join("config.yml"), "invalid: [yaml").unwrap();
    controller
        .set_filter(IssueFilter {
            query: "Zeb".into(),
            ..filter
        })
        .unwrap();
    assert_eq!(controller.selected_id(), Some(&zebra.metadata.id));
    let last_good = controller.return_context();
    assert!(controller.refresh().is_err());
    assert_eq!(controller.return_context(), last_good);
    controller.restore_context(&context).unwrap();
    assert_eq!(controller.return_context(), context);
}
