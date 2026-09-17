use super::*;
use crate::{ReviewApp, ReviewOptions};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use workdeck_pm::{CreateIssue, Repository, RequestId};

const PATCH: &str = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+first changed\n@@ -20 +20 @@\n-old\n+second changed\ndiff --git a/b.rs b/b.rs\n--- a/b.rs\n+++ b/b.rs\n@@ -2 +2 @@\n-before\n+linked changed\n";

fn app(root: &std::path::Path, patch: &str) -> ReviewApp {
    let changeset = workdeck_diff::changeset_from_patch(
        patch,
        "Workbench",
        "Workbench",
        "test",
        workdeck_core::ChangesetSource::WorkingTree { staged: false },
        None,
    );
    let mut app = ReviewApp::new(
        changeset,
        ReviewOptions {
            workbench: Some(WorkbenchOptions::new(root)),
            repo: Some(root.into()),
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
fn ctrl(app: &mut ReviewApp, ch: char) {
    app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL));
    super::test_pump::settle_app(app);
}
fn type_text(app: &mut ReviewApp, text: &str) {
    for ch in text.chars() {
        key(app, KeyCode::Char(ch));
    }
}
fn effects_without_reload(app: &mut ReviewApp) {
    app.process_workbench_effect(&mut |_, _, _| Err("unexpected source reload".into()));
}

#[test]
fn mounted_forms_create_edit_comment_and_keep_drafts_across_review_switches() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut app = app(directory.path(), PATCH);
    let document = app.with_state(|state| state.changeset_snapshot());
    assert!(!app.workbench_issues_visible());
    key(&mut app, KeyCode::Tab);
    assert!(!app.workbench_issues_visible());
    assert_eq!(app.focus, crate::Focus::Filter);
    key(&mut app, KeyCode::F(3));
    assert!(
        !app.workbench_issues_visible(),
        "the native filter retains key ownership"
    );
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "Mounted issue");
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "Retained body 🐦");
    key(&mut app, KeyCode::F(2));
    assert!(std::sync::Arc::ptr_eq(
        &document,
        &app.with_state(|state| state.changeset_snapshot())
    ));
    key(&mut app, KeyCode::F(3));
    let retained_screen = screen(&app, 110);
    assert!(
        retained_screen.contains("Retained body 🐦"),
        "{retained_screen}"
    );
    ctrl(&mut app, 's');
    let issue = repository.list_issues().unwrap().remove(0);
    assert_eq!(issue.metadata.title, "Mounted issue");
    assert_eq!(issue.body, "Retained body 🐦");
    key(&mut app, KeyCode::Char('e'));
    ctrl(&mut app, 'u');
    type_text(&mut app, "Edited issue");
    ctrl(&mut app, 's');
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Edited issue"
    );
    key(&mut app, KeyCode::Char('c'));
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "A mounted comment");
    ctrl(&mut app, 's');
    assert_eq!(
        repository.comments(issue.metadata.id.as_str()).unwrap()[0].body,
        "A mounted comment"
    );
}

#[test]
fn mounted_priority_labels_and_unassignment_use_shared_operations() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    for id in ["bug", "api-reviewed"] {
        let mut label = workdeck_pm::CreatePlanning::new(id);
        label.id = Some(id.into());
        repository
            .create_planning(workdeck_pm::PlanningKind::Label, &label, &RequestId::new())
            .unwrap();
    }
    let mut input = CreateIssue::new("Issue properties", "Preserved body");
    input
        .fields
        .insert("labels".into(), serde_json::json!(["bug"]));
    input
        .fields
        .insert("assignee".into(), serde_json::json!("agent"));
    repository.create_issue(&input, &RequestId::new()).unwrap();
    let original = repository.list_issues().unwrap().remove(0);
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('p'));
    assert!(screen(&app, 110).contains("Change priority"));
    ctrl(&mut app, 'u');
    type_text(&mut app, "invalid");
    ctrl(&mut app, 's');
    assert!(screen(&app, 110).contains("unknown priority"));
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        original.source
    );
    ctrl(&mut app, 'u');
    type_text(&mut app, "high");
    ctrl(&mut app, 's');
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .metadata
            .priority,
        workdeck_pm::Priority::High
    );

    key(&mut app, KeyCode::Char('l'));
    assert!(screen(&app, 110).contains("Edit labels"));
    ctrl(&mut app, 'u');
    type_text(&mut app, "bug\napi-reviewed");
    key(&mut app, KeyCode::F(2));
    key(&mut app, KeyCode::F(3));
    assert!(screen(&app, 110).contains("api-reviewed"));
    ctrl(&mut app, 's');
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .metadata
            .labels,
        ["bug", "api-reviewed"]
    );
    key(&mut app, KeyCode::Char('l'));
    ctrl(&mut app, 'u');
    ctrl(&mut app, 's');
    assert!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .metadata
            .labels
            .is_empty()
    );

    key(&mut app, KeyCode::Char('a'));
    ctrl(&mut app, 'u');
    ctrl(&mut app, 's');
    let current = repository
        .show_issue(original.metadata.id.as_str())
        .unwrap();
    assert_eq!(current.metadata.assignee, None);
    assert_eq!(current.body, "Preserved body");
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(shell.controller.selected_id(), Some(&original.metadata.id));
    let receipt = shell.controller.last_receipt().unwrap();
    let recorded: workdeck_pm::IssueRecord =
        serde_json::from_value(receipt.result.clone()).unwrap();
    assert_eq!(recorded.source, current.source);
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn mounted_label_form_keeps_input_and_source_when_external_edit_wins() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Concurrent properties", "Original"),
            &RequestId::new(),
        )
        .unwrap();
    let original = repository.list_issues().unwrap().remove(0);
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('l'));
    assert!(screen(&app, 110).contains("Edit labels"));
    type_text(&mut app, "retained-label");
    repository
        .mutate_issue(
            original.metadata.id.as_str(),
            Some(&original.source),
            &workdeck_pm::IssueMutation::Update {
                input: workdeck_pm::UpdateIssue {
                    fields: Default::default(),
                    body: Some("External body".into()),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let external = repository
        .show_issue(original.metadata.id.as_str())
        .unwrap();
    key(&mut app, KeyCode::F(2));
    key(&mut app, KeyCode::F(3));
    ctrl(&mut app, 's');
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(
        shell.controller.error().unwrap().error.code,
        workdeck_pm::ErrorCode::StaleSource
    );
    assert_eq!(
        shell.form.as_ref().unwrap().fields[0].value,
        "retained-label"
    );
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        external.source
    );
    assert!(external.metadata.labels.is_empty());
}

#[test]
fn issue_copy_uses_native_clipboard_and_keeps_transport_errors_visible() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Copy selected issue", "Body"),
            &RequestId::new(),
        )
        .unwrap();
    let original = repository.list_issues().unwrap().remove(0);
    let mut app = app(directory.path(), PATCH);
    app.set_clipboard_copy_supported(true);
    key(&mut app, KeyCode::F(3));
    effects_without_reload(&mut app);
    key(&mut app, KeyCode::Char('y'));
    effects_without_reload(&mut app);
    assert_eq!(
        app.take_clipboard_copy_request().as_deref(),
        Some(original.metadata.id.as_str())
    );
    app.report_clipboard_copy_failure("controlled terminal write failure");
    assert!(screen(&app, 110).contains("controlled terminal write failure"));
    app.set_clipboard_copy_supported(false);
    key(&mut app, KeyCode::Char('y'));
    effects_without_reload(&mut app);
    assert!(app.take_clipboard_copy_request().is_none());
    assert!(screen(&app, 110).contains("Clipboard copy unsupported"));
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        original.source
    );
}

#[test]
fn linking_current_review_file_keeps_source_context_and_rejects_stale_issue() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Link selected file", "Body"),
            &RequestId::new(),
        )
        .unwrap();
    let original = repository.list_issues().unwrap().remove(0);
    let mut app = app(directory.path(), PATCH);
    app.with_state(|state| state.select_hunk(0, 1)).unwrap();
    app.scroll = 7;
    let selection = app.with_state(|state| state.selection());
    let document = app.with_state(|state| state.changeset_snapshot());
    app.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::SHIFT));
    effects_without_reload(&mut app);
    assert!(screen(&app, 110).contains("Link current file"));
    assert!(screen(&app, 110).contains("Link selected file"));
    key(&mut app, KeyCode::Enter);
    let linked = repository
        .show_issue(original.metadata.id.as_str())
        .unwrap();
    assert_eq!(
        linked.metadata.files,
        [workdeck_pm::SourceLink {
            path: "a.rs".into(),
            line: selection.line,
            end_line: None
        }]
    );
    key(&mut app, KeyCode::F(2));
    assert_eq!(app.with_state(|state| state.selection()), selection);
    assert_eq!(app.scroll, 7);
    assert!(std::sync::Arc::ptr_eq(
        &document,
        &app.with_state(|state| state.changeset_snapshot())
    ));

    app.with_state(|state| state.select_hunk(1, 0)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::SHIFT));
    effects_without_reload(&mut app);
    repository
        .mutate_issue(
            original.metadata.id.as_str(),
            Some(&linked.source),
            &workdeck_pm::IssueMutation::Update {
                input: workdeck_pm::UpdateIssue {
                    fields: Default::default(),
                    body: Some("External edit".into()),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let external = repository
        .show_issue(original.metadata.id.as_str())
        .unwrap();
    ctrl(&mut app, 's');
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert_eq!(
        shell.controller.error().unwrap().error.code,
        workdeck_pm::ErrorCode::StaleSource
    );
    assert!(shell.form.is_some());
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        external.source
    );
    assert_eq!(external.metadata.files, linked.metadata.files);
    drop(shell);
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::F(2));
    app.options.repo = Some("/another-repository".into());
    app.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::SHIFT));
    effects_without_reload(&mut app);
    assert!(screen(&app, 110).contains("another repository"));
    assert_eq!(
        repository
            .show_issue(original.metadata.id.as_str())
            .unwrap()
            .source,
        external.source
    );
}

#[test]
fn mounted_stale_edit_retains_input_and_original_precondition() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let issue: workdeck_pm::IssueRecord = serde_json::from_value(
        repository
            .create_issue(&CreateIssue::new("Original", "Body"), &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut app = app(directory.path(), "");
    key(&mut app, KeyCode::Char('e'));
    ctrl(&mut app, 'u');
    type_text(&mut app, "Retained changed title");
    repository
        .mutate_issue(
            issue.metadata.id.as_str(),
            Some(&issue.source),
            &workdeck_pm::IssueMutation::Update {
                input: workdeck_pm::UpdateIssue {
                    fields: std::collections::BTreeMap::from([(
                        "title".into(),
                        serde_json::json!("Concurrent title"),
                    )]),
                    body: None,
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    ctrl(&mut app, 's');
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert!(shell.form.is_some());
    assert_eq!(
        shell.controller.error().unwrap().error.code,
        workdeck_pm::ErrorCode::StaleSource
    );
    assert_eq!(
        shell.controller.drafts()[&DraftKey::Edit(issue.metadata.id.clone())].source(),
        Some(&issue.source)
    );
    drop(shell);
    assert!(screen(&app, 110).contains("Retained changed title"));
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .title,
        "Concurrent title"
    );
}

#[test]
fn file_jump_and_return_preserve_native_document_hunk_scroll_and_issue_selection() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut input = CreateIssue::new("Linked issue", "Body");
    input.fields.insert(
        "files".into(),
        serde_json::json!([{"path":"b.rs","line":2}]),
    );
    let issue: workdeck_pm::IssueRecord = serde_json::from_value(
        repository
            .create_issue(&input, &RequestId::new())
            .unwrap()
            .result,
    )
    .unwrap();
    let mut app = app(directory.path(), PATCH);
    app.with_state(|state| state.select_hunk(0, 1)).unwrap();
    app.scroll = 7;
    let before = app.with_state(|state| state.selection());
    let source = app.with_state(|state| state.changeset_snapshot());
    key(&mut app, KeyCode::F(3));
    effects_without_reload(&mut app);
    key(&mut app, KeyCode::Char('f'));
    effects_without_reload(&mut app);
    assert!(!app.workbench_issues_visible());
    assert_eq!(
        app.with_state(|state| state.selected_file().unwrap().path.clone()),
        "b.rs"
    );
    assert!(screen(&app, 110).contains("linked changed"));
    key(&mut app, KeyCode::F(3));
    effects_without_reload(&mut app);
    assert_eq!(app.with_state(|state| state.selection()), before);
    assert_eq!(app.scroll, 7);
    assert!(std::sync::Arc::ptr_eq(
        &source,
        &app.with_state(|state| state.changeset_snapshot())
    ));
    assert_eq!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .controller
            .selected_id(),
        Some(&issue.metadata.id)
    );
}

#[test]
fn native_modal_owns_navigation_keys_and_wide_shell_keeps_review_canvas() {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path(), "WD").unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::Char('?'));
    assert!(app.show_help);
    key(&mut app, KeyCode::F(3));
    assert!(!app.workbench_issues_visible());
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::F(3));
    let wide = screen(&app, 180);
    assert!(wide.contains("No issues yet"), "{wide}");
    assert!(wide.contains("first changed"), "{wide}");
    let narrow = screen(&app, 90);
    assert!(narrow.contains("No issues yet"));
    assert!(!narrow.contains("first changed"));
}

#[test]
fn explicit_file_source_projection_has_context_rows_and_keeps_specialized_comparisons_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path(), "WD").unwrap();
    std::fs::write(directory.path().join("plain.rs"), "first\nsecond\n").unwrap();
    let original = workdeck_vcs::load_file_comparison(
        directory.path(),
        std::path::Path::new("plain.rs"),
        std::path::Path::new("plain.rs"),
    )
    .unwrap();
    let input = workdeck_core::CliInput::Files(workdeck_core::FileCommandInput {
        left: "plain.rs".into(),
        right: "plain.rs".into(),
        options: Default::default(),
    });
    let mut app = app(directory.path(), "");
    let mut unchanged = original.clone();
    app.prepare_workbench_source_view(&input, &mut unchanged)
        .unwrap();
    assert_eq!(unchanged, original);
    app.workbench.as_ref().unwrap().lock().unwrap().source_file = Some("plain.rs".into());
    app.prepare_workbench_source_view(&input, &mut unchanged)
        .unwrap();
    let file = &unchanged.files[0];
    assert_eq!(file.sources, original.files[0].sources);
    assert_eq!(file.stats, original.files[0].stats);
    assert_eq!(file.hunks[0].lines.len(), 2);
    assert!(
        file.hunks[0]
            .lines
            .iter()
            .all(|line| line.kind == workdeck_core::DiffLineKind::Context)
    );
    app.options.review_input = Some(input);
    app.reload(unchanged);
    assert!(screen(&app, 100).contains("No issues yet"));
    key(&mut app, KeyCode::F(2));
    assert!(screen(&app, 100).contains("second"));
}

#[test]
fn selected_review_note_is_saved_as_an_issue_with_body_and_source_location() {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path(), "WD").unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::Char('c'));
    assert!(app.note_composer.is_some());
    type_text(&mut app, "Explain this change before merging");
    ctrl(&mut app, 's');
    assert!(app.note_composer.is_none());
    key(&mut app, KeyCode::F(4));
    effects_without_reload(&mut app);
    let input = {
        let shell = app.workbench.as_ref().unwrap().lock().unwrap();
        let DraftInput::Create(input) = &shell.controller.drafts()[&DraftKey::Create].input else {
            panic!();
        };
        input.clone()
    };
    assert!(input.body.contains("Explain this change before merging"));
    assert_eq!(input.fields["files"][0]["path"], "a.rs");
    assert!(input.fields["files"][0]["line"].as_u64().is_some());
    // Submit the mounted form, then reopen the store so this checks durable
    // shared-operation output rather than only the in-memory draft.
    ctrl(&mut app, 's');
    let repository = Repository::discover(directory.path()).unwrap();
    let issues = repository.list_issues().unwrap();
    assert_eq!(issues.len(), 1);
    let issue = &issues[0];
    assert_eq!(issue.metadata.title, input.title);
    assert_eq!(issue.body, input.body);
    assert_eq!(
        serde_json::to_value(&issue.metadata.files).unwrap(),
        input.fields["files"]
    );
    assert!(
        repository
            .operation_history()
            .unwrap()
            .iter()
            .any(|receipt| {
                receipt.operation == "issue.create"
                    && receipt.changed.iter().any(|change| {
                        change
                            .path
                            .to_string_lossy()
                            .contains(issue.metadata.id.as_str())
                    })
            })
    );
    key(&mut app, KeyCode::F(2));
    let (note, _) = app.active_note_for_composer(false).unwrap();
    assert!(
        note.markup
            .or(note.rationale)
            .unwrap_or(note.summary)
            .contains("Explain this change before merging"),
        "saving an issue must preserve its source review note"
    );
}

#[test]
fn explicit_builtin_binding_precedes_workbench_default_navigation_key() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = app(directory.path(), PATCH);
    let mut options = fixture.options.clone();
    options.keybindings = vec![workdeck_core::UserKeyBindingEntry::new(
        "workdeck.view.toggleFilesPane",
        workdeck_core::UserKeyBinding::Chord("f3".into()),
    )];
    let mut app = ReviewApp::new(
        fixture.with_state(|state| state.changeset().clone()),
        options,
    );
    let sidebar = app.options.sidebar;
    key(&mut app, KeyCode::F(3));
    assert_eq!(app.options.sidebar, !sidebar);
    assert!(!app.workbench_issues_visible());
}

#[test]
fn multiple_links_offer_explicit_selection_instead_of_always_opening_first() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut input = CreateIssue::new("Multiple targets", "Body");
    input.fields.insert(
        "files".into(),
        serde_json::json!([{"path":"a.rs","line":1},{"path":"b.rs","line":2}]),
    );
    input
        .fields
        .insert("commits".into(), serde_json::json!(["abc1234", "def5678"]));
    repository.create_issue(&input, &RequestId::new()).unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(3));
    effects_without_reload(&mut app);
    key(&mut app, KeyCode::Char('f'));
    assert!(screen(&app, 90).contains("Choose file"));
    ctrl(&mut app, 'u');
    type_text(&mut app, "2");
    key(&mut app, KeyCode::Enter);
    effects_without_reload(&mut app);
    assert_eq!(
        app.with_state(|state| state.selected_file().unwrap().path.clone()),
        "b.rs"
    );
    key(&mut app, KeyCode::F(3));
    effects_without_reload(&mut app);
    key(&mut app, KeyCode::Char('g'));
    assert!(screen(&app, 90).contains("def5678"));
    ctrl(&mut app, 'u');
    type_text(&mut app, "99");
    key(&mut app, KeyCode::Enter);
    assert!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .form
            .is_some()
    );
    ctrl(&mut app, 'u');
    type_text(&mut app, "2");
    key(&mut app, KeyCode::Enter);
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert!(shell.form.is_none());
    // Commit loading remains host-owned; an explicit choice queues that intent.
    assert!(format!("{:?}", shell.effect).contains("def5678"));
}

fn screen(app: &ReviewApp, width: u16) -> String {
    screen_height(app, width, 30)
}

#[test]
fn graph_mouse_events_preserve_the_visible_native_review_pane() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(&CreateIssue::new("Graph subject", ""), &RequestId::new())
        .unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('b'));
    let before = screen(&app, 180);
    let event = MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 150,
        row: 10,
        modifiers: KeyModifiers::NONE,
    };
    assert!(
        !app.handle_workbench_mouse(&event),
        "the visible review pane retains its mouse routing"
    );
    assert_eq!(screen(&app, 180), before);
    assert!(app.handle_workbench_mouse(&MouseEvent {
        column: 10,
        ..event
    }));
    assert_ne!(
        screen(&app, 180),
        before,
        "scrolling the graph changes its details pane"
    );
}

fn screen_height(app: &ReviewApp, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| crate::render(frame.area(), frame.buffer_mut(), app))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>()
}

#[test]
fn clean_startup_mounts_issues_with_actionable_uninitialized_state_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let app = app(directory.path(), "");
    let text = screen(&app, 110);
    assert!(text.contains("F2 Review"), "{text}");
    assert!(text.contains("F3 Issues"), "{text}");
    assert!(text.contains("workdeck init"), "{text}");
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn expanded_filter_form_keeps_selected_archive_field_visible_and_applies_shared_scope() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(&CreateIssue::new("Active issue", ""), &RequestId::new())
        .unwrap();
    let mut archived = CreateIssue::new("Archived issue", "");
    archived
        .fields
        .insert("archived".into(), serde_json::json!(true));
    repository
        .create_issue(&archived, &RequestId::new())
        .unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('/'));
    for _ in 0..9 {
        key(&mut app, KeyCode::Tab);
    }
    ctrl(&mut app, 'u');
    type_text(&mut app, "archived");
    assert!(
        screen(&app, 80).contains("archived▏"),
        "selected archive value must remain visible on a short terminal"
    );
    ctrl(&mut app, 's');
    let rendered = screen(&app, 80);
    assert!(!rendered.contains("Filter issues"));
    assert!(rendered.contains("Archived issue"));
    assert!(!rendered.contains("Active issue"));
}

#[test]
fn expanded_filter_form_scrolls_each_selected_field_into_a_short_viewport() {
    let directory = tempfile::tempdir().unwrap();
    Repository::init(directory.path(), "WD").unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('/'));
    for index in 0..10 {
        ctrl(&mut app, 'u');
        let value = format!("visible{index}");
        type_text(&mut app, &value);
        assert!(
            screen_height(&app, 80, 14).contains(&format!("{value}▏")),
            "selected filter field {index} is not visible"
        );
        key(&mut app, KeyCode::Tab);
    }
}

#[test]
fn projects_and_cycles_have_persistent_native_authoring_views() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(11));
    assert!(screen(&app, 110).contains("Projects"));
    key(&mut app, KeyCode::Char('n'));
    assert!(
        screen(&app, 110).contains("Create Projects"),
        "{}",
        screen(&app, 110)
    );
    type_text(&mut app, "Deliver parser");
    key(&mut app, KeyCode::Tab);
    type_text(&mut app, "Project body preserved");
    ctrl(&mut app, 's');
    let projects = repository
        .list_planning(workdeck_pm::PlanningKind::Project)
        .unwrap();
    assert_eq!(projects.len(), 1, "{}", screen(&app, 110));
    assert_eq!(projects[0].body, "Project body preserved");
    key(&mut app, KeyCode::F(12));
    assert!(screen(&app, 110).contains("Cycles"));
    key(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "September cycle");
    ctrl(&mut app, 's');
    assert_eq!(
        repository
            .list_planning(workdeck_pm::PlanningKind::Cycle)
            .unwrap()
            .len(),
        1
    );
    key(&mut app, KeyCode::F(2));
    key(&mut app, KeyCode::F(11));
    assert!(screen(&app, 110).contains("Deliver parser"));
}

#[test]
fn issue_custom_field_form_preserves_invalid_json_and_meets_active_policy() {
    use std::collections::{BTreeMap, BTreeSet};
    use workdeck_pm::{CustomFieldDefinition, CustomFieldType, CustomScope, SchemaChange};
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    let change = SchemaChange {
        fields: BTreeMap::from([(
            "risk".into(),
            CustomFieldDefinition {
                field_type: CustomFieldType::Text,
                scopes: BTreeSet::from([CustomScope::Issue]),
                required: true,
                archived: false,
                options: vec![],
                custom: BTreeMap::new(),
                extra: BTreeMap::new(),
            },
        )]),
        ..Default::default()
    };
    repository
        .apply_schema_change(&change, None, &RequestId::new())
        .unwrap();
    let mut app = app(directory.path(), PATCH);
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "Policy issue");
    ctrl(&mut app, 's');
    assert!(repository.list_issues().unwrap().is_empty());
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'u');
    type_text(&mut app, "{unfinished");
    ctrl(&mut app, 's');
    assert!(repository.list_issues().unwrap().is_empty());
    key(&mut app, KeyCode::Esc);
    key(&mut app, KeyCode::Char('n'));
    assert!(screen(&app, 110).contains("{unfinished"));
    key(&mut app, KeyCode::Tab);
    key(&mut app, KeyCode::Tab);
    ctrl(&mut app, 'u');
    type_text(&mut app, "{\"risk\":\"reviewed\",\"unknown\":42}");
    ctrl(&mut app, 's');
    let issue = repository.list_issues().unwrap().remove(0);
    assert_eq!(issue.metadata.custom["risk"], serde_json::json!("reviewed"));
    key(&mut app, KeyCode::Char('e'));
    ctrl(&mut app, 'u');
    type_text(&mut app, "Renamed");
    ctrl(&mut app, 's');
    assert_eq!(
        repository
            .show_issue(issue.metadata.id.as_str())
            .unwrap()
            .metadata
            .custom["unknown"],
        serde_json::json!(42)
    );
}

#[test]
fn indexed_mouse_scroll_targets_the_visible_source_without_moving_issue_selection() {
    use crossterm::event::{MouseEvent, MouseEventKind};
    for width in [72, 132] {
        let directory = tempfile::tempdir().unwrap();
        let repository = Repository::init(directory.path(), "WD").unwrap();
        let body = (0..100)
            .map(|line| format!("Source line {line}\n"))
            .collect::<String>();
        for title in ["First", "Second"] {
            repository
                .create_issue(&CreateIssue::new(title, &body), &RequestId::new())
                .unwrap();
        }
        let mut app = app(directory.path(), PATCH);
        key(&mut app, KeyCode::F(3));
        key(&mut app, KeyCode::Enter);
        screen(&app, width);
        let (list, selected, opened) = {
            let shell = app.workbench.as_ref().unwrap().lock().unwrap();
            (
                shell.index_rendered.as_ref().unwrap().list,
                shell.controller.selected_id().cloned(),
                shell.index.as_ref().unwrap().opened.clone(),
            )
        };
        let (column, row) = if width >= 100 {
            (list.right() + 2, list.y + 2)
        } else {
            (list.x + 2, list.bottom() + 2)
        };
        assert!(app.handle_workbench_mouse(&MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column,
            row,
            modifiers: KeyModifiers::NONE
        }));
        let shell = app.workbench.as_ref().unwrap().lock().unwrap();
        assert!(
            shell.index.as_ref().unwrap().detail_scroll > 0,
            "wheel over source must scroll the source at width {width}"
        );
        assert_eq!(shell.controller.selected_id(), selected.as_ref());
        assert_eq!(shell.index.as_ref().unwrap().opened, opened);
    }
}

#[test]
fn mounted_activity_opens_exact_events_and_preserves_issue_drafts() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Timeline task", "source body"),
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(directory.path(), "");
    key(&mut app, KeyCode::Char('e'));
    ctrl(&mut app, 'u');
    type_text(&mut app, "retained timeline draft");
    app.handle_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::SHIFT));
    super::test_pump::settle_app(&mut app);
    assert_eq!(
        format!("{:?}", app.workbench.as_ref().unwrap().lock().unwrap().tab),
        "Activity",
        "Shift-F12 must open the activity timeline"
    );
    for width in [72, 160] {
        assert!(screen(&app, width).contains("Activity"));
    }
    key(&mut app, KeyCode::Enter);
    assert!(
        screen(&app, 160).contains("issue.create"),
        "event opens its exact operation source"
    );
    key(&mut app, KeyCode::F(3));
    assert!(screen(&app, 160).contains("retained timeline draft"));
    assert_eq!(
        repository.list_issues().unwrap()[0].metadata.title,
        "Timeline task"
    );
}

#[test]
fn activity_refresh_retains_opened_generation_and_malformed_source_is_visible() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    repository
        .create_issue(
            &CreateIssue::new("Original activity", ""),
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(directory.path(), "");
    app.handle_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::SHIFT));
    super::test_pump::settle_app(&mut app);
    key(&mut app, KeyCode::Enter);
    let opened = app
        .workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .activity
        .as_ref()
        .unwrap()
        .opened
        .clone()
        .unwrap();
    repository
        .create_issue(&CreateIssue::new("Later activity", ""), &RequestId::new())
        .unwrap();
    key(&mut app, KeyCode::Char('r'));
    {
        let shell = app.workbench.as_ref().unwrap().lock().unwrap();
        let activity = shell.activity.as_ref().unwrap();
        assert_ne!(
            activity.handle.as_ref().unwrap().view,
            opened.row.token.view
        );
        assert_eq!(
            activity.opened.as_ref().unwrap().row.token,
            opened.row.token
        );
        assert_eq!(activity.opened.as_ref().unwrap().document, opened.document);
    }
    let issue = repository.list_issues().unwrap().remove(0);
    std::fs::write(repository.root().join(issue.path), "broken native issue").unwrap();
    key(&mut app, KeyCode::Char('r'));
    assert!(screen(&app, 160).contains("stale retained view"));
    key(&mut app, KeyCode::F(2));
    app.handle_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::SHIFT));
    super::test_pump::settle_app(&mut app);
    assert_eq!(
        app.workbench
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .activity
            .as_ref()
            .unwrap()
            .opened
            .as_ref()
            .unwrap()
            .row
            .token,
        opened.row.token
    );
}

#[test]
fn unavailable_activity_does_not_initialize_planning_or_git() {
    let directory = tempfile::tempdir().unwrap();
    let mut app = app(directory.path(), "");
    app.handle_key(KeyEvent::new(KeyCode::F(12), KeyModifiers::SHIFT));
    super::test_pump::settle_app(&mut app);
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    let activity = shell.activity.as_ref().unwrap();
    assert!(activity.handle.is_none());
    assert!(activity.error.is_some());
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".git").exists());
}

#[test]
fn mounted_cycle_carryover_reviews_exclusions_and_applies_without_closing_work() {
    let directory = tempfile::tempdir().unwrap();
    let repository = Repository::init(directory.path(), "WD").unwrap();
    for (id, name) in [("current", "A Current"), ("next", "B Next")] {
        repository
            .create_planning(
                workdeck_pm::PlanningKind::Cycle,
                &workdeck_pm::CreatePlanning {
                    id: Some(id.into()),
                    ..workdeck_pm::CreatePlanning::new(name)
                },
                &RequestId::new(),
            )
            .unwrap();
    }
    let mut records = Vec::new();
    for title in ["Carry this task", "Canceled stays"] {
        records.push(
            serde_json::from_value::<workdeck_pm::IssueRecord>(
                repository
                    .create_issue(
                        &CreateIssue {
                            title: title.into(),
                            body: "".into(),
                            fields: std::collections::BTreeMap::from([(
                                "cycle".into(),
                                serde_json::json!("current"),
                            )]),
                        },
                        &RequestId::new(),
                    )
                    .unwrap()
                    .result,
            )
            .unwrap(),
        );
    }
    repository
        .mutate_issue(
            records[1].metadata.id.as_str(),
            Some(&records[1].source),
            &workdeck_pm::IssueMutation::Cancel,
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(directory.path(), "");
    key(&mut app, KeyCode::F(12));
    key(&mut app, KeyCode::Char('u'));
    assert!(
        screen(&app, 160).contains("Cycle carryover"),
        "u must open the selected cycle's carryover form"
    );
    type_text(&mut app, "next");
    ctrl(&mut app, 's');
    for width in [72, 160] {
        let text = screen(&app, width);
        assert!(text.contains("Review cycle carryover"));
        assert!(text.contains("Carry this task"));
        assert!(text.contains("canceled"));
    }
    assert_eq!(
        repository
            .show_issue(records[0].metadata.id.as_str())
            .unwrap()
            .source,
        records[0].source
    );
    key(&mut app, KeyCode::F(2));
    key(&mut app, KeyCode::F(12));
    assert!(screen(&app, 160).contains("Review cycle carryover"));
    key(&mut app, KeyCode::Char('x'));
    assert!(screen(&app, 160).contains("Carryover saved"));
    let moved = repository
        .show_issue(records[0].metadata.id.as_str())
        .unwrap();
    assert_eq!(moved.metadata.cycle.as_deref(), Some("next"));
    assert_eq!(moved.metadata.status, records[0].metadata.status);
    assert_eq!(
        repository
            .show_issue(records[1].metadata.id.as_str())
            .unwrap()
            .metadata
            .cycle
            .as_deref(),
        Some("current")
    );
    key(&mut app, KeyCode::Char('x'));
    assert_eq!(
        repository
            .show_issue(records[0].metadata.id.as_str())
            .unwrap()
            .source,
        moved.source
    );
}

#[test]
fn mounted_my_work_reads_explicit_sources_and_retains_the_original_issue_draft() {
    use workdeck_pm::{SourceSelector, registry::*};
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_repo = Repository::init(owner.path(), "WD").unwrap();
    let target_repo = Repository::init(target.path(), "WD").unwrap();
    let mut input = CreateIssue::new("Registered assignment", "Exact registered source body");
    input
        .fields
        .insert("assignee".into(), serde_json::json!("local"));
    target_repo.create_issue(&input, &RequestId::new()).unwrap();
    let store = RegistryStore::open(&owner_repo).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: inspect_checkout(
                        "secondary",
                        target.path(),
                        SourceSelector::WorkingTree,
                    )
                    .unwrap(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(owner.path(), "");
    key(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "Original owner draft");
    app.handle_key(KeyEvent::new(KeyCode::F(9), KeyModifiers::SHIFT));
    super::test_pump::settle_app(&mut app);
    for width in [72, 160] {
        let text = screen(&app, width);
        assert!(text.contains("My work"), "{text}");
        assert!(text.contains("secondary"), "{text}");
        assert!(text.contains("Registered assignment"), "{text}");
    }
    key(&mut app, KeyCode::Enter);
    let text = screen(&app, 160);
    assert!(text.contains("Exact registered source body"), "{text}");
    key(&mut app, KeyCode::F(2));
    app.handle_key(KeyEvent::new(KeyCode::F(9), KeyModifiers::SHIFT));
    super::test_pump::settle_app(&mut app);
    assert!(screen(&app, 160).contains("Exact registered source body"));
    key(&mut app, KeyCode::F(3));
    assert!(screen(&app, 160).contains("Original owner draft"));
    ctrl(&mut app, 's');
    assert_eq!(
        owner_repo.list_issues().unwrap()[0].metadata.title,
        "Original owner draft"
    );
    assert_eq!(target_repo.list_issues().unwrap().len(), 1);
    assert!(!target_repo.root().join(".local/repositories").exists());
}

#[test]
fn mounted_checkout_switch_retains_each_draft_and_commits_only_in_its_source() {
    use std::{path::Path, sync::Arc};
    use workdeck_pm::{SourceSelector, registry::*};
    #[derive(Debug)]
    struct Panels(std::path::PathBuf);
    impl RepositoryPanelProvider for Panels {
        fn source(&self) -> RepositoryPanelSource {
            RepositoryPanelSource {
                root: self.0.clone(),
                identity: self.0.display().to_string(),
            }
        }
        fn load(&self, _: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
            Err(PanelError::new("no fixture collection"))
        }
        fn preview(&self, _: &PanelTarget) -> Result<PanelPreview, PanelError> {
            Err(PanelError::new("no fixture preview"))
        }
    }
    let owner = tempfile::tempdir().unwrap();
    let target = tempfile::tempdir().unwrap();
    let owner_root = owner.path().canonicalize().unwrap();
    let target_root = target.path().canonicalize().unwrap();
    let owner_repo = Repository::init(&owner_root, "WD").unwrap();
    let target_repo = Repository::init(&target_root, "WD").unwrap();
    let store = RegistryStore::open(&owner_repo).unwrap();
    let mapping = inspect_checkout("secondary", &target_root, SourceSelector::WorkingTree).unwrap();
    store
        .mutate(
            &RegistryRequest {
                expected: store.snapshot().unwrap().source,
                mutation: RegistryMutation::Register {
                    checkout: mapping.clone(),
                },
            },
            &RequestId::new(),
        )
        .unwrap();
    let mut app = app(&owner_root, PATCH);
    app.with_state(|state| state.select_file(1).unwrap());
    app.filter = "b.rs".into();
    app.scroll = 7;
    let nested_cwd = owner_root.join("nested");
    std::fs::create_dir(&nested_cwd).unwrap();
    app.options.command_cwd = Some(nested_cwd.clone());
    app.options.review_input = Some(workdeck_core::CliInput::Vcs(
        workdeck_core::VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: Default::default(),
        },
    ));
    app.options.repository_panels = Some(Arc::new(Panels(owner_root.clone())));
    let mut reload = |app: &mut ReviewApp, input: &workdeck_core::CliInput, root: &Path| {
        let source_root = if root.starts_with(&target_root) {
            &target_root
        } else {
            &owner_root
        };
        let changeset = workdeck_diff::changeset_from_patch(
            if source_root == &owner_root {
                PATCH
            } else {
                ""
            },
            "checkout",
            "checkout",
            "test",
            workdeck_core::ChangesetSource::WorkingTree { staged: false },
            None,
        );
        let host = crate::DynamicReviewHostOptions {
            command_cwd: root.to_owned(),
            repo_root: Some(source_root.to_owned()),
            repository_panels: Some(Some(Arc::new(Panels(source_root.to_owned())))),
            registry_navigation: (source_root == &target_root)
                .then(|| store.prepare_navigation(&mapping).unwrap()),
            ..Default::default()
        };
        app.session_commit_dynamic_reload(
            crate::DynamicReviewLoad {
                input: input.clone(),
                changeset,
                host_options: host,
                replacement_extensions: None,
                replacement_vcs_catalog: None,
            },
            &Default::default(),
        )
        .map(|_| ())
    };
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "Owner draft survives");
    app.handle_key(KeyEvent::new(KeyCode::F(9), KeyModifiers::SHIFT));
    super::test_pump::settle_app(&mut app);
    key(&mut app, KeyCode::Char('s'));
    key(&mut app, KeyCode::Char('o'));
    app.process_workbench_effect(&mut |_, _, _| Err("fixture load failed".into()));
    assert_eq!(app.options.repo.as_ref(), Some(&owner_root));
    assert!(app.workbench_checkouts.retained.is_empty());
    assert!(screen(&app, 160).contains("fixture load failed"));
    key(&mut app, KeyCode::Char('o'));
    app.process_workbench_effect(&mut reload);
    super::test_pump::settle_app(&mut app);
    assert_eq!(
        app.options.repo.as_ref(),
        Some(&target_root),
        "open registered checkout did not switch the mounted root"
    );
    assert!(screen(&app, 62).contains("Checkout: secondary"));
    assert!(app.filter.is_empty());
    let active_signal = app
        .workbench
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .context
        .checks
        .signal
        .clone();
    let retained_signal = app.workbench_checkouts.retained[0]
        .shell
        .lock()
        .unwrap()
        .context
        .checks
        .signal
        .clone();
    assert!(Arc::ptr_eq(&active_signal, &retained_signal));
    app.filter = "target-review".into();
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('n'));
    type_text(&mut app, "Target draft survives");
    app.handle_key(KeyEvent::new(KeyCode::F(9), KeyModifiers::SHIFT));
    key(&mut app, KeyCode::Char('b'));
    app.process_workbench_effect(&mut reload);
    key(&mut app, KeyCode::F(3));
    assert!(screen(&app, 160).contains("Owner draft survives"));
    assert!(screen(&app, 62).contains("Checkout: launch"));
    assert_eq!(app.filter, "b.rs");
    assert_eq!(app.options.command_cwd.as_ref(), Some(&nested_cwd));
    assert_eq!(
        app.with_state(|state| state.selected_file().unwrap().path.clone()),
        "b.rs"
    );
    assert_eq!(app.scroll, 7);
    ctrl(&mut app, 's');
    assert_eq!(
        owner_repo.list_issues().unwrap()[0].metadata.title,
        "Owner draft survives"
    );
    assert!(target_repo.list_issues().unwrap().is_empty());
    app.handle_key(KeyEvent::new(KeyCode::F(9), KeyModifiers::SHIFT));
    key(&mut app, KeyCode::Char('b'));
    app.process_workbench_effect(&mut |_, _, _| Err("retained target unavailable".into()));
    assert_eq!(app.options.repo.as_ref(), Some(&owner_root));
    assert_eq!(app.workbench_checkouts.retained.len(), 1);
    key(&mut app, KeyCode::Char('b'));
    app.process_workbench_effect(&mut reload);
    assert_eq!(app.filter, "target-review");
    key(&mut app, KeyCode::F(3));
    assert!(screen(&app, 160).contains("Target draft survives"));
    ctrl(&mut app, 's');
    assert_eq!(
        target_repo.list_issues().unwrap()[0].metadata.title,
        "Target draft survives"
    );
    assert_eq!(owner_repo.list_issues().unwrap().len(), 1);
    assert!(!target_repo.root().join(".local/repositories").exists());
    let saved = owner_repo.root().with_file_name("saved-owner-planning");
    std::fs::rename(owner_repo.root(), &saved).unwrap();
    std::fs::create_dir(owner_repo.root()).unwrap();
    std::fs::copy(
        saved.join("config.yml"),
        owner_repo.root().join("config.yml"),
    )
    .unwrap();
    app.handle_key(KeyEvent::new(KeyCode::F(9), KeyModifiers::SHIFT));
    key(&mut app, KeyCode::Char('b'));
    app.process_workbench_effect(&mut |_, _, _| panic!("copied owner replacement reached loader"));
    assert_eq!(app.options.repo.as_ref(), Some(&target_root));
    assert_eq!(app.workbench_checkouts.retained.len(), 1);
    std::fs::remove_dir_all(owner_repo.root()).unwrap();
    std::fs::rename(saved, owner_repo.root()).unwrap();
    app.shutdown_foreground_run().unwrap();
    assert!(app.workbench_checkouts.retained.is_empty());
}
