use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, LineRange, ReviewSide};
use workdeck_diff::parse_patch;
use workdeck_examples::review_triage_extension::{
    ReviewTriageState, TriageDecision, TriageStatus, handle_event, hunk_key, invoke_command,
    reconcile_changeset, registrations, render_pane, required_capabilities, submit_confirm,
    submit_input, submit_select,
};
use workdeck_extension_api::{
    Capability, CommandInvocation, ConfirmDialogSubmission, ExtensionCommandAvailability,
    ExtensionEventContext, ExtensionHostAction, ExtensionNotifyType, InputDialogSubmission,
    PaneActionInvocation, PanePlacement, PaneRenderRequest, Registration, ReviewEvent,
    SelectDialogSubmission, ViewNode,
};
use workdeck_extension_host::LoadedExtension;
use workdeck_review::{CommentAnchor, ReviewComment, ReviewNoteResolution, ReviewState};
use workdeck_tui::{ReviewApp, ReviewOptions, render, to_extension_paint_theme};

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-review-triage-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-review-triage-extension"),
        binary_directory.join(binary_name),
    )
    .unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/review-triage/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn changeset() -> Changeset {
    parse_patch(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old one\n+new one\n@@ -10 +10 @@\n-old two\n+new two\ndiff --git a/src/other.rs b/src/other.rs\n--- a/src/other.rs\n+++ b/src/other.rs\n@@ -3 +3 @@\n-before\n+after\n",
        "triage",
        "Review triage",
        ChangesetSource::Patch {
            label: "triage".into(),
        },
    )
    .unwrap()
}

fn one_hunk_changeset() -> Changeset {
    parse_patch(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old one\n+new one\n",
        "triage",
        "Review triage",
        ChangesetSource::Patch {
            label: "triage".into(),
        },
    )
    .unwrap()
}

fn oracle_changeset() -> Changeset {
    parse_patch(
        "diff --git a/src/lib.ts b/src/lib.ts\n--- a/src/lib.ts\n+++ b/src/lib.ts\n@@ -1 +1 @@\n-old one\n+new one\n@@ -10 +10 @@\n-old two\n+new two\ndiff --git a/src/other.ts b/src/other.ts\n--- a/src/other.ts\n+++ b/src/other.ts\n@@ -3 +3 @@\n-before\n+after\n",
        "triage-oracle",
        "Review triage oracle",
        ChangesetSource::Patch {
            label: "triage-oracle".into(),
        },
    )
    .unwrap()
}

fn snapshot() -> workdeck_core::ReviewSnapshot {
    ReviewState::new(changeset()).snapshot()
}

fn command_invocation(command_id: &str) -> CommandInvocation {
    CommandInvocation {
        command_id: command_id.into(),
        snapshot: snapshot(),
        cwd: PathBuf::from("/repo"),
        review: None,
        open_panes: Vec::new(),
        active_keyboard_mode: None,
        workspace: None,
        commands: ExtensionCommandAvailability {
            enabled: vec!["workdeck.review.align-current-line-center".into()],
        },
    }
}

fn event(name: &str, payload: serde_json::Value) -> ReviewEvent {
    ReviewEvent {
        name: name.into(),
        snapshot: snapshot(),
        payload,
        review: None,
        context: ExtensionEventContext::default(),
    }
}

fn pane_request() -> PaneRenderRequest {
    PaneRenderRequest {
        pane_id: "triage".into(),
        snapshot: snapshot(),
        placement: PanePlacement::Right,
        width: 32,
        height: 20,
        theme: to_extension_paint_theme(&ReviewOptions::default().theme),
    }
}

fn flatten_text(node: &ViewNode, output: &mut String) {
    match node {
        ViewNode::Text { text, .. } => output.push_str(text),
        ViewNode::Row { children, .. } | ViewNode::Column { children, .. } => {
            for child in children {
                flatten_text(child, output);
                output.push('\n');
            }
        }
        ViewNode::List { items, .. } => {
            for item in items {
                flatten_text(item, output);
                output.push('\n');
            }
        }
        ViewNode::Action { child, .. } => flatten_text(child, output),
        ViewNode::Divider | ViewNode::Empty => {}
    }
}

fn text_rows(node: &ViewNode, output: &mut Vec<String>) {
    match node {
        ViewNode::Text { text, .. } => output.push(text.clone()),
        ViewNode::Row { children, .. } | ViewNode::Column { children, .. } => {
            for child in children {
                text_rows(child, output);
            }
        }
        ViewNode::List { items, .. } => {
            for item in items {
                text_rows(item, output);
            }
        }
        ViewNode::Action { child, .. } => text_rows(child, output),
        ViewNode::Divider | ViewNode::Empty => {}
    }
}

fn rendered_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn find_text(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
    let symbols = needle
        .chars()
        .map(|character| character.to_string())
        .collect::<Vec<_>>();
    for y in buffer.area.y..buffer.area.bottom() {
        let end = buffer.area.right().saturating_sub(symbols.len() as u16);
        for x in buffer.area.x..=end {
            if symbols.iter().enumerate().all(|(offset, symbol)| {
                buffer
                    .cell((x + offset as u16, y))
                    .is_some_and(|cell| cell.symbol() == symbol)
            }) {
                return Some((x, y));
            }
        }
    }
    None
}

fn press(app: &mut ReviewApp, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    settle_extension_commands(app);
}

fn settle_extension_commands(app: &mut ReviewApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.has_pending_extension_commands() {
        app.poll_extension_commands();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn registration_and_manifest_capabilities_cover_the_complete_public_surface() {
    let registrations = registrations();
    assert!(matches!(
        &registrations[0],
        Registration::Pane(pane)
            if pane.id == "triage" && pane.placement == PanePlacement::Right && !pane.default_open
    ));
    let commands = registrations
        .iter()
        .filter_map(|registration| match registration {
            Registration::Command(command) => {
                Some((command.id.as_str(), command.default_keys.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(commands.len(), 5);
    assert!(commands.contains(&("toggle", vec!["y".into()])));
    assert!(commands.contains(&("mark", vec!["x".into()])));
    let Registration::EventSubscription { names } = &registrations[6] else {
        panic!("event subscription is registered last");
    };
    assert_eq!(names.len(), 8);
    assert!(names.contains(&"review-triage:open".into()));
    assert_eq!(
        required_capabilities(),
        [
            Capability::Commands,
            Capability::Panes,
            Capability::Events,
            Capability::Dialogs,
            Capability::Notifications,
            Capability::ReviewNavigation,
        ]
    );
}

#[test]
fn lifecycle_state_reconciles_only_hunks_that_still_exist() {
    let source = changeset();
    let first = &source.files[0].runtime_id;
    let second_hunk = hunk_key(first, 1);
    let mut state = ReviewTriageState::default();
    state.decisions.insert(
        second_hunk.clone(),
        TriageDecision {
            status: TriageStatus::Blocked,
            rationale: Some("race".into()),
        },
    );
    state.viewed.insert(second_hunk.clone());
    state.note_counts.insert(second_hunk, 2);
    state.current = Some((first.clone(), 1));
    state.reload_pending = true;

    reconcile_changeset(&mut state, &one_hunk_changeset());
    assert!(state.decisions.is_empty());
    assert!(state.viewed.is_empty());
    assert!(state.note_counts.is_empty());
    assert_eq!(state.current, None);
    assert!(!state.reload_pending);
}

#[test]
fn command_dialog_flow_records_trimmed_decision_and_emits_the_public_event() {
    let mut state = ReviewTriageState::default();
    let mark = invoke_command(&command_invocation("mark"), &mut state).unwrap();
    assert!(matches!(
        &mark.actions[..],
        [ExtensionHostAction::OpenSelectDialog { title, options, .. }]
            if title == "Triage src/lib.rs, hunk 1"
                && options == &["approved", "investigate", "blocked"]
    ));
    let selected = submit_select(
        &SelectDialogSubmission {
            action_id: "triage-status".into(),
            value: Some("blocked".into()),
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            active_keyboard_mode: None,
            commands: Default::default(),
        },
        &mut state,
    );
    assert!(matches!(
        &selected.actions[..],
        [ExtensionHostAction::OpenInputDialog { title, placeholder, initial: None, .. }]
            if title == "blocked: optional rationale" && placeholder == "Why should a reviewer care?"
    ));
    let completed = submit_input(
        &InputDialogSubmission {
            action_id: "triage-rationale".into(),
            value: Some("  data race  ".into()),
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            active_keyboard_mode: None,
            commands: Default::default(),
        },
        &mut state,
    );
    let first = &snapshot().changeset.files[0].runtime_id;
    assert_eq!(
        state.decisions.get(&hunk_key(first, 0)),
        Some(&TriageDecision {
            status: TriageStatus::Blocked,
            rationale: Some("data race".into()),
        })
    );
    assert!(matches!(
        &completed.actions[..],
        [
            ExtensionHostAction::RefreshPane { id },
            ExtensionHostAction::EmitEvent { name, payload },
            ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Info }
        ] if id == "triage"
            && name == "review-triage:decision"
            && payload["status"] == "blocked"
            && message == "Marked hunk 1 blocked"
    ));
}

#[test]
fn focus_clear_and_cancellation_preserve_session_only_state() {
    let mut state = ReviewTriageState::default();
    state.focus = "security".into();
    let focus = invoke_command(&command_invocation("focus"), &mut state).unwrap();
    assert!(matches!(
        &focus.actions[..],
        [ExtensionHostAction::OpenInputDialog { initial: Some(initial), .. }] if initial == "security"
    ));
    let center = invoke_command(&command_invocation("center"), &mut state).unwrap();
    assert!(matches!(
        &center.actions[..],
        [ExtensionHostAction::ExecuteReviewCommand { id, count: None }]
            if id == "workdeck.review.align-current-line-center"
    ));
    let focused = submit_input(
        &InputDialogSubmission {
            action_id: "triage-focus".into(),
            value: Some("  concurrency  ".into()),
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            active_keyboard_mode: None,
            commands: Default::default(),
        },
        &mut state,
    );
    assert_eq!(state.focus, "concurrency");
    assert!(matches!(
        &focused.actions[..],
        [
            ExtensionHostAction::RefreshPane { .. },
            ExtensionHostAction::OpenPane { .. }
        ]
    ));

    let file_id = snapshot().changeset.files[0].runtime_id.clone();
    state.decisions.insert(
        hunk_key(&file_id, 0),
        TriageDecision {
            status: TriageStatus::Approved,
            rationale: None,
        },
    );
    let clear = invoke_command(&command_invocation("clear"), &mut state).unwrap();
    assert!(matches!(
        &clear.actions[..],
        [ExtensionHostAction::OpenConfirmDialog { confirm_label, .. }] if confirm_label == "clear"
    ));
    let cancelled = submit_confirm(
        &ConfirmDialogSubmission {
            action_id: "triage-clear".into(),
            confirmed: false,
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            active_keyboard_mode: None,
            commands: Default::default(),
        },
        &mut state,
    );
    assert!(cancelled.actions.is_empty());
    assert_eq!(state.decisions.len(), 1);
    let confirmed = submit_confirm(
        &ConfirmDialogSubmission {
            action_id: "triage-clear".into(),
            confirmed: true,
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            active_keyboard_mode: None,
            commands: Default::default(),
        },
        &mut state,
    );
    assert!(state.decisions.is_empty());
    assert_eq!(confirmed.actions.len(), 2);
}

#[test]
fn disabled_current_line_marker_preserves_the_hunk_warning_fallback() {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut app = ReviewApp::new_with_extensions(
        changeset(),
        ReviewOptions {
            cursor_line: workdeck_tui::CursorLineMode::Off,
            ..ReviewOptions::default()
        },
        vec![extension],
    );
    let mut terminal = Terminal::new(TestBackend::new(160, 24)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT));
    settle_extension_commands(&mut app);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let rendered = rendered_text(&terminal);
    assert!(
        rendered.contains("Enable the current-line marker before centering it"),
        "{rendered}"
    );
}

#[test]
fn lifecycle_events_track_visits_notes_filters_pending_reload_and_external_open() {
    let mut state = ReviewTriageState::default();
    let file_id = snapshot().changeset.files[0].runtime_id.clone();
    handle_event(
        &event(
            "selection_changed",
            json!({ "fileId": file_id, "hunkIndex": 1 }),
        ),
        &mut state,
    );
    assert_eq!(state.current, Some((file_id.clone(), 1)));
    handle_event(
        &event("hunk_viewed", json!({ "fileId": file_id, "hunkIndex": 1 })),
        &mut state,
    );
    handle_event(
        &event("note_created", json!({ "fileId": file_id, "hunkIndex": 1 })),
        &mut state,
    );
    assert!(state.viewed.contains(&hunk_key(&file_id, 1)));
    assert_eq!(state.note_counts[&hunk_key(&file_id, 1)], 1);
    handle_event(
        &event("filter_changed", json!({ "filter": "*.rs" })),
        &mut state,
    );
    handle_event(&event("watch_reload_pending", json!({})), &mut state);
    assert_eq!(state.filter, "*.rs");
    assert!(state.reload_pending);
    let opened = handle_event(&event("review-triage:open", json!({})), &mut state);
    assert!(matches!(
        &opened.actions[..],
        [ExtensionHostAction::OpenPane { id }] if id == "triage"
    ));
}

#[test]
fn pane_rows_preserve_markers_counts_styles_and_click_navigation() {
    let mut state = ReviewTriageState::default();
    let request = pane_request();
    let file_id = request.snapshot.changeset.files[0].runtime_id.clone();
    state.viewed.insert(hunk_key(&file_id, 0));
    state.note_counts.insert(hunk_key(&file_id, 0), 2);
    state.decisions.insert(
        hunk_key(&file_id, 1),
        TriageDecision {
            status: TriageStatus::Investigate,
            rationale: Some("check ordering".into()),
        },
    );
    state.focus = "concurrency".into();
    state.filter = "src/**".into();
    state.reload_pending = true;
    let rendered = render_pane(&request, &mut state).unwrap();
    let mut text = String::new();
    flatten_text(&rendered.content, &mut text);
    assert!(text.contains("0/3 reviewed · !1 · ×0"));
    assert!(text.contains("Reload pending…"));
    assert!(text.contains("Focus: concurrency"));
    assert!(text.contains("Filter: src/**"));
    assert!(text.contains("· hunk 1 [2 notes]"));
    assert!(text.contains("! hunk 2 — check ordering"));

    let file = workdeck_examples::review_triage_extension::invoke_pane_action(
        &PaneActionInvocation {
            pane_id: "triage".into(),
            action_id: "file:1".into(),
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            open_panes: Vec::new(),
        },
        &state,
    );
    assert!(matches!(
        &file.actions[..],
        [ExtensionHostAction::SelectReviewFile { file_id: selected }] if selected == &request.snapshot.changeset.files[1].runtime_id
    ));
    let hunk = workdeck_examples::review_triage_extension::invoke_pane_action(
        &PaneActionInvocation {
            pane_id: "triage".into(),
            action_id: "hunk:0:1".into(),
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            open_panes: Vec::new(),
        },
        &state,
    );
    assert!(matches!(
        &hunk.actions[..],
        [ExtensionHostAction::SelectReviewHunk { hunk_index: 1, .. }]
    ));
}

#[test]
fn rust_projection_matches_the_frozen_baseline_and_stable_oracle() {
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("../../port/hunk/oracles/review-triage.json")).unwrap();
    assert_eq!(
        oracle["sources"][0]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(oracle["sources"][0]["viewedEvent"], "hunk_viewed");
    assert_eq!(
        oracle["sources"][1]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_eq!(oracle["sources"][1]["viewedEvent"], "file_viewed");

    let mut request = pane_request();
    request.snapshot = ReviewState::new(oracle_changeset()).snapshot();
    request.snapshot.selection.hunk_index = Some(1);
    let file_id = request.snapshot.changeset.files[0].runtime_id.clone();
    let mut state = ReviewTriageState::default();
    state.viewed.insert(hunk_key(&file_id, 0));
    state.note_counts.insert(hunk_key(&file_id, 0), 1);
    state.decisions.insert(
        hunk_key(&file_id, 1),
        TriageDecision {
            status: TriageStatus::Blocked,
            rationale: Some("race window".into()),
        },
    );
    state.filter = "src/**".into();
    state.reload_pending = true;
    let rendered = render_pane(&request, &mut state).unwrap();
    let mut rows = Vec::new();
    text_rows(&rendered.content, &mut rows);
    let expected = oracle["beforeClear"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["content"].as_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert_eq!(rows, expected);
}

#[test]
fn subprocess_protocol_preserves_state_across_every_callback_boundary() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    assert!(extension.subscribes_to_event("changeset_loaded"));
    assert!(extension.subscribes_to_event("review-triage:open"));
    let loaded = extension
        .deliver_event(event("changeset_loaded", json!({})))
        .unwrap();
    assert!(matches!(
        &loaded.actions[..],
        [ExtensionHostAction::RefreshPane { .. }]
    ));
    extension
        .deliver_event(event(
            "hunk_viewed",
            json!({ "fileId": snapshot().changeset.files[0].runtime_id, "hunkIndex": 0 }),
        ))
        .unwrap();
    let view = extension.render_pane(pane_request()).unwrap();
    let mut text = String::new();
    flatten_text(&view.content, &mut text);
    assert!(text.contains("· hunk 1"));

    let action = extension
        .invoke_pane_action(PaneActionInvocation {
            pane_id: "triage".into(),
            action_id: "hunk:0:1".into(),
            snapshot: snapshot(),
            cwd: PathBuf::new(),
            review: None,
            open_panes: Vec::new(),
        })
        .unwrap();
    assert!(matches!(
        &action.actions[..],
        [ExtensionHostAction::SelectReviewHunk { hunk_index: 1, .. }]
    ));
    let opened = extension
        .deliver_event(event("review-triage:open", json!({})))
        .unwrap();
    assert!(matches!(
        &opened.actions[..],
        [ExtensionHostAction::OpenPane { .. }]
    ));
    let mut close_event = event("review-triage:open", json!({}));
    close_event.context = ExtensionEventContext::new(PathBuf::from("/repo"), vec!["triage".into()]);
    let closed = extension.deliver_event(close_event).unwrap();
    assert!(matches!(
        &closed.actions[..],
        [ExtensionHostAction::ClosePane { .. }]
    ));
}

#[test]
fn ratatui_routes_clicks_dialogs_lifecycle_and_note_events_end_to_end() {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut app =
        ReviewApp::new_with_extensions(changeset(), ReviewOptions::default(), vec![extension]);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();

    press(&mut app, KeyCode::Char('y'));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let initial = rendered_text(&terminal);
    assert!(initial.contains("Review triage (session only)"));
    assert!(initial.contains("· hunk 1"));
    let (_, second_hunk_row) = find_text(terminal.backend().buffer(), "hunk 2").unwrap();
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 70,
        row: second_hunk_row,
        modifiers: KeyModifiers::NONE,
    });
    assert_eq!(
        app.shared_state().lock().unwrap().selection().hunk_index,
        Some(1)
    );

    press(&mut app, KeyCode::Char('x'));
    assert!(app.has_extension_select_dialog());
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    assert!(app.has_extension_input_dialog());
    for character in "ordering 🧭".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("! hunk 2 — ordering 🧭"));

    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT));
    settle_extension_commands(&mut app);
    for _ in 0..4 {
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::Enter);
    assert!(app.has_extension_confirm_dialog());
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("Clear review triage?"));
    press(&mut app, KeyCode::Char('n'));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("ordering 🧭"));

    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::ALT));
    settle_extension_commands(&mut app);
    for _ in 0..4 {
        press(&mut app, KeyCode::Down);
    }
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('y'));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(!rendered_text(&terminal).contains("ordering 🧭"));

    let state = app.shared_state();
    let file_key = state.lock().unwrap().changeset().files[0].key.clone();
    state
        .lock()
        .unwrap()
        .add_comment(ReviewComment {
            id: "triage-note".into(),
            parent_id: None,
            source: "user".into(),
            author: None,
            created_at: None,
            file_path: Some("src/lib.rs".into()),
            hunk_index: Some(1),
            side: Some(ReviewSide::New),
            line: Some(10),
            summary: "Inspect ordering".into(),
            rationale: None,
            markup: None,
            title: None,
            tags: Vec::new(),
            confidence: None,
            updated_at: None,
            resolution: ReviewNoteResolution::Active,
            anchor: CommentAnchor {
                file_key,
                old_range: Some(LineRange { start: 10, end: 10 }),
                new_range: Some(LineRange { start: 10, end: 10 }),
                preferred_side: Some(ReviewSide::New),
                preferred_line: Some(10),
                intersecting_hunk_indices: vec![1],
                owner_hunk_index: Some(1),
            },
            editable: true,
        })
        .unwrap();
    app.tick_extension_notifications(std::time::Instant::now());
    app.notify_watch_reload_pending();
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let updated = rendered_text(&terminal);
    assert!(updated.contains("[1 note]"));
    assert!(updated.contains("Reload pending…"));

    press(&mut app, KeyCode::Char('x'));
    assert!(app.has_extension_select_dialog());
    app.reload(one_hunk_changeset());
    assert!(!app.has_extension_dialog());
    app.shared_state()
        .lock()
        .unwrap()
        .select_hunk(0, 0)
        .unwrap();
    press(&mut app, KeyCode::Char('x'));
    assert!(app.has_extension_select_dialog());
    press(&mut app, KeyCode::Esc);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let reloaded = rendered_text(&terminal);
    assert!(!reloaded.contains("ordering 🧭"));
    assert!(!reloaded.contains("Reload pending…"));
}
