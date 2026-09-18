use super::*;
use crate::{ReviewApp, ReviewOptions};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{buffer::Buffer, layout::Rect};
use workdeck_pm::{CreateIssue, Repository, RequestId};

fn app(root: &std::path::Path) -> ReviewApp {
    let mut app = ReviewApp::new(
        workdeck_diff::changeset_from_patch(
            "",
            "Context",
            "Context",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        ),
        ReviewOptions {
            repo: Some(root.into()),
            workbench: Some(WorkbenchOptions::new(root)),
            highlight: false,
            ..ReviewOptions::default()
        },
    );
    super::test_pump::settle_app(&mut app);
    app
}
fn key(app: &mut ReviewApp, code: KeyCode) {
    super::test_pump::settle_app(app);
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    super::test_pump::settle_app(app);
}
fn screen(app: &ReviewApp, width: u16) -> String {
    let area = Rect::new(3, 2, width, 30);
    let mut buffer = Buffer::empty(Rect::new(0, 0, width + 6, 34));
    assert!(app.render_workbench_body(area, &mut buffer));
    buffer.content.iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn issues_context_entry_opens_task_sections_in_the_mounted_review_app() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Inspect this task", "Accepted scope"),
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    let text = screen(&app, 110);
    assert!(text.contains("Task context"), "{text}");
    for section in ["1 Context", "2 Next", "3 Questions", "4 Handoffs"] {
        assert!(text.contains(section), "missing {section}: {text}");
    }
    assert!(text.contains("Inspect this task"), "{text}");
}

#[test]
fn mounted_checks_entry_is_available_without_spawning_or_initializing_a_source() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    key(&mut app, KeyCode::Char('5'));
    let text = screen(&app, 110);
    assert!(text.contains("5 Checks"), "{text}");
    assert!(text.contains("Check definitions"), "{text}");
    assert!(text.contains("workdeck init"), "{text}");
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn mounted_context_exposes_inert_document_reference_and_its_issue_citation() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue {
                fields: serde_json::from_value(serde_json::json!({
                    "documents": ["https://example.invalid/accepted-design"]
                }))
                .unwrap(),
                ..CreateIssue::new("Linked design task", "")
            },
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('i'));
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    let rows = shell.context.rows();
    let row = rows
        .iter()
        .find(|row| row.id == "document:https://example.invalid/accepted-design")
        .expect("linked document must appear in mounted context");
    assert!(row.body.contains("have not been fetched"));
    assert!(row.target.is_none());
    assert!(rows.iter().any(|row| {
        row.id
            .starts_with("document:https://example.invalid/accepted-design:citation:")
            && matches!(
                &row.target,
                Some(super::context_workspace::RowTarget::Citation(citation))
                    if matches!(citation.target, workdeck_pm::ContextTarget::Issue { .. })
            )
    }));
}

#[test]
fn context_question_form_survives_review_and_planning_tabs_and_keeps_numeric_input() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(&CreateIssue::new("Task", "Scope"), &RequestId::new())
        .unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::Char('i'));
    key(&mut app, KeyCode::Char('3'));
    key(&mut app, KeyCode::Char('n'));
    key(&mut app, KeyCode::Tab);
    for ch in "Question 1234 🐦".chars() {
        key(&mut app, KeyCode::Char(ch));
    }
    key(&mut app, KeyCode::F(2));
    key(&mut app, KeyCode::F(11));
    key(&mut app, KeyCode::F(3));
    let text = screen(&app, 90);
    assert!(text.contains("Question 1234 🐦"), "{text}");
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    let questions = repository
        .questions(&workdeck_pm::QuestionQuery::default())
        .unwrap();
    assert_eq!(questions.len(), 1);
    assert_eq!(questions[0].body, "Question 1234 🐦");
    assert!(screen(&app, 110).contains("Question 1234 🐦"));
    key(&mut app, KeyCode::Esc);
    assert!(
        !app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .context_visible
    );
}

#[test]
fn uninitialized_context_refresh_discovers_source_without_creating_legacy_state() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::Char('i'));
    assert!(!directory.path().join(".workdeck").exists());
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(&CreateIssue::new("Fresh task", ""), &RequestId::new())
        .unwrap();
    key(&mut app, KeyCode::Char('r'));
    let text = screen(&app, 110);
    assert!(text.contains("Fresh task"), "{text}");
    assert!(!directory.path().join(".agents").exists());
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert!(screen(&app, 110).contains("Task context · Fresh task"));
}

fn source_app() -> (tempfile::TempDir, Repository, ReviewApp) {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    std::fs::write(
        directory.path().join("source.rs"),
        "first\ninspected source\n",
    )
    .unwrap();
    let mut input = CreateIssue::new("Source task", "Inspect linked source");
    input.fields.insert(
        "files".into(),
        serde_json::json!([{"path":"source.rs","line":2}]),
    );
    repository.create_issue(&input, &RequestId::new()).unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::Char('i'));
    {
        let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
        let row = shell.context.rows().into_iter().find(|row| matches!(&row.target, Some(super::context_workspace::RowTarget::Citation(citation)) if matches!(&citation.target, workdeck_pm::ContextTarget::WorktreeSource { link } if link.path == "source.rs"))).unwrap();
        shell.context.state_mut().selected[0] = Some(row.id);
    }
    (directory, repository, app)
}

#[test]
fn context_citation_rejects_changed_source_before_loading_review() {
    let (directory, _repository, mut app) = source_app();
    std::fs::write(
        directory.path().join("source.rs"),
        "changed after inspection\n",
    )
    .unwrap();
    key(&mut app, KeyCode::Enter);
    let mut called = false;
    app.process_workbench_effect(&mut |_, _, _| {
        called = true;
        Ok(())
    });
    assert!(!called, "stale citation must fail before source reload");
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(shell.tab, WorkbenchTab::Issues);
    assert!(shell.context_visible);
    assert!(
        shell.notice.as_ref().unwrap().contains("changed"),
        "{:?}",
        shell.notice
    );
    drop(shell);
    assert!(
        screen(&app, 110).contains("changed"),
        "Navigation errors must be visible in the active context workspace"
    );
}

#[test]
fn context_citation_checks_callback_bytes_before_publication_and_restores_context() {
    let (_directory, _repository, mut app) = source_app();
    key(&mut app, KeyCode::Enter);
    let mut rejected = false;
    app.process_workbench_effect(&mut |app, input, root| {
        let workdeck_core::CliInput::Files(files) = input else {
            panic!("file navigation");
        };
        let changed = b"different bytes after revalidation\n";
        let mut snapshot = workdeck_vcs::load_file_comparison_from_bytes(
            root,
            std::path::Path::new(&files.left),
            std::path::Path::new(&files.right),
            changed,
            changed,
        )
        .unwrap();
        let error = app
            .prepare_workbench_source_view(input, &mut snapshot)
            .unwrap_err();
        rejected = true;
        Err(error)
    });
    assert!(rejected);
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Issues
    );
    key(&mut app, KeyCode::Enter);
    app.process_workbench_effect(&mut |app, input, root| {
        let workdeck_core::CliInput::Files(files) = input else {
            panic!("file navigation");
        };
        let original = b"first\ninspected source\n";
        let mut snapshot = workdeck_vcs::load_file_comparison_from_bytes(
            root,
            std::path::Path::new(&files.left),
            std::path::Path::new(&files.right),
            original,
            original,
        )
        .unwrap();
        app.prepare_workbench_source_view(input, &mut snapshot)?;
        app.options.review_input = Some(input.clone());
        app.reload(snapshot);
        Ok(())
    });
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Review
    );
    assert_eq!(
        app.with_state(|state| state.selected_file().unwrap().path.clone()),
        "source.rs"
    );
    key(&mut app, KeyCode::F(3));
    app.process_workbench_effect(&mut |_, _, _| {
        Err("Unexpected reload of unconfigured original source".into())
    });
    let text = screen(&app, 110);
    assert!(text.contains("Task context · Source task"), "{text}");
    assert!(text.contains("source.rs"));
}

#[test]
fn context_budget_omissions_are_visible_at_narrow_and_wide_allocated_sizes() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Budgeted task", "Long body. ".repeat(20000)),
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(directory.path());
    key(&mut app, KeyCode::Char('i'));
    let packet = app
        .workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .context
        .state()
        .unwrap()
        .packet
        .clone()
        .unwrap();
    assert!(packet.budget.omitted_entries > 0);
    for width in [72, 180] {
        let text = screen(&app, width);
        assert!(text.contains("omitted"), "{text}");
        assert!(text.contains("unknown"), "{text}");
    }
}
