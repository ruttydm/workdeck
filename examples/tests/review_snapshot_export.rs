use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, LineRange, ReviewSide};
use workdeck_diff::parse_patch;
use workdeck_examples::review_snapshot_export_extension::{
    required_capabilities, resolve_snapshot_export_path, snapshot_position_matches,
};
use workdeck_extension_api::{
    Capability, ExtensionHostAction, ExtensionNotifyType, ExtensionReviewSnapshot, Registration,
};
use workdeck_extension_host::LoadedExtension;
use workdeck_review::{
    CommentAnchor, ReviewComment, ReviewNoteResolution, ReviewState,
    build_extension_review_snapshot,
};
use workdeck_tui::{ReviewApp, ReviewOptions, render};

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-review-snapshot-export-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-review-snapshot-export-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/review-snapshot-export/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn changeset() -> Changeset {
    parse_patch(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "snapshot-export",
        "Snapshot export",
        ChangesetSource::Patch {
            label: "snapshot-export".into(),
        },
    )
    .unwrap()
}

fn review_state_with_note() -> ReviewState {
    let mut state = ReviewState::new(changeset());
    let file_key = state.changeset().files[0].key.clone();
    state
        .add_comment(ReviewComment {
            id: "note-1".into(),
            parent_id: Some("thread-1".into()),
            source: "mcp".into(),
            author: Some("Review Bot".into()),
            created_at: Some("2026-09-02T08:00:00Z".into()),
            file_path: Some("src/lib.rs".into()),
            hunk_index: Some(0),
            side: Some(ReviewSide::New),
            line: Some(1),
            summary: "Keep the semantic address".into(),
            rationale: Some("Renderer rows are ephemeral".into()),
            markup: Some("<note>semantic</note>".into()),
            title: Some("Addressing".into()),
            tags: vec!["architecture".into()],
            confidence: Some(workdeck_core::AgentAnnotationConfidence::High),
            updated_at: Some("2026-09-02T08:01:00Z".into()),
            resolution: ReviewNoteResolution::Active,
            anchor: CommentAnchor {
                file_key,
                old_range: Some(LineRange { start: 1, end: 1 }),
                new_range: Some(LineRange { start: 1, end: 1 }),
                preferred_side: Some(ReviewSide::New),
                preferred_line: Some(1),
                intersecting_hunk_indices: vec![0],
                owner_hunk_index: Some(0),
            },
            editable: false,
        })
        .unwrap();
    state
}

fn empty_snapshot(generation: &str, state_revision: u64) -> ExtensionReviewSnapshot {
    ExtensionReviewSnapshot {
        generation: generation.into(),
        state_revision,
        files: Vec::new(),
        notes: Vec::new(),
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
fn translated_helpers_resolve_from_cwd_and_match_only_the_exact_position() {
    let temporary = TempDir::new().unwrap();
    let cwd = temporary.path().join("repo");
    let expected = cwd.join("out/review.json");
    assert_eq!(
        resolve_snapshot_export_path(&cwd, " out/./nested/../review.json "),
        expected
    );
    let absolute = temporary.path().join("absolute.json");
    assert_eq!(
        resolve_snapshot_export_path(&cwd, absolute.to_str().unwrap()),
        absolute
    );

    let captured = empty_snapshot("generation:test:1", 2);
    assert!(snapshot_position_matches(
        &captured,
        Some(&empty_snapshot("generation:test:1", 2))
    ));
    assert!(!snapshot_position_matches(
        &captured,
        Some(&empty_snapshot("generation:test:2", 2))
    ));
    assert!(!snapshot_position_matches(
        &captured,
        Some(&empty_snapshot("generation:test:1", 3))
    ));
    assert!(!snapshot_position_matches(&captured, None));
}

#[test]
fn authoritative_projection_preserves_files_notes_and_json_contract() {
    let mut state = review_state_with_note();
    let mut stale = state.comments()[0].clone();
    stale.id = "note-2".into();
    stale.parent_id = None;
    stale.resolution = ReviewNoteResolution::Stale;
    state.add_comment(stale).unwrap();
    let mut orphaned = state.comments()[0].clone();
    orphaned.id = "note-3".into();
    orphaned.parent_id = None;
    orphaned.anchor.file_key = "retired-file-key".into();
    orphaned.resolution = ReviewNoteResolution::Orphaned;
    state.add_comment(orphaned).unwrap();
    let snapshot = build_extension_review_snapshot(&state);
    assert_eq!(snapshot.state_revision, 3);
    assert_eq!(snapshot.files.len(), 1);
    assert_eq!(snapshot.notes.len(), 3);

    let encoded = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(encoded["files"][0]["path"], "src/lib.rs");
    assert_eq!(encoded["files"][0]["changeKind"], "change");
    assert_eq!(encoded["files"][0]["flags"]["tooLarge"], false);
    assert_eq!(encoded["notes"][0]["source"], "agent");
    assert_eq!(encoded["notes"][0]["originalSource"], "mcp");
    assert_eq!(encoded["notes"][0]["anchor"]["preferred"]["side"], "new");
    assert_eq!(encoded["notes"][0]["anchor"]["preferred"]["line"], 1);
    assert_eq!(encoded["notes"][0]["title"], "Addressing");
    assert_eq!(encoded["notes"][0]["updatedAt"], "2026-09-02T08:01:00Z");
    assert_eq!(encoded["notes"][0]["resolution"], "active");
    assert_eq!(encoded["notes"][1]["resolution"], "stale");
    assert_eq!(encoded["notes"][2]["resolution"], "orphaned");
    assert!(
        serde_json::to_value(&state.comments()[0])
            .unwrap()
            .get("resolution")
            .is_none(),
        "active remains wire-compatible with pre-resolution comments"
    );
}

#[test]
fn compiled_extension_refuses_unavailable_stale_and_existing_exports() {
    let (_extension_directory, manifest) = staged_extension();
    let output_directory = TempDir::new().unwrap();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    assert!(extension.handshake.registrations.iter().any(|registration| {
        matches!(registration, Registration::Command(command) if command.id == "export" && command.default_keys == ["f9"])
    }));

    let state = review_state_with_note();
    let core_snapshot = state.snapshot();
    let captured = build_extension_review_snapshot(&state);
    let unavailable = extension
        .invoke_command_with_review_context(
            "export",
            core_snapshot.clone(),
            Vec::new(),
            None,
            output_directory.path().to_owned(),
            None,
        )
        .unwrap();
    assert!(matches!(
        unavailable.actions.as_slice(),
        [ExtensionHostAction::Notify {
            notification_type: ExtensionNotifyType::Warning,
            ..
        }]
    ));

    let dialog = extension
        .invoke_command_with_review_context(
            "export",
            core_snapshot.clone(),
            Vec::new(),
            None,
            output_directory.path().to_owned(),
            Some(captured.clone()),
        )
        .unwrap();
    assert!(matches!(
        dialog.actions.as_slice(),
        [ExtensionHostAction::OpenInputDialog { title, placeholder, .. }]
            if title == "Export review snapshot" && placeholder == "workdeck-review-snapshot.json"
    ));
    let mut advanced = captured.clone();
    advanced.state_revision += 1;
    let stale = extension
        .submit_input_dialog_with_context(
            "snapshot-export-path",
            Some("review.json".into()),
            core_snapshot.clone(),
            None,
            output_directory.path().to_owned(),
            Some(advanced),
        )
        .unwrap();
    assert!(matches!(
        stale.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning }]
            if message.contains("review changed")
    ));
    assert!(!output_directory.path().join("review.json").exists());

    extension
        .invoke_command_with_review_context(
            "export",
            core_snapshot.clone(),
            Vec::new(),
            None,
            output_directory.path().to_owned(),
            Some(captured.clone()),
        )
        .unwrap();
    let written = extension
        .submit_input_dialog_with_context(
            "snapshot-export-path",
            Some("review.json".into()),
            core_snapshot.clone(),
            None,
            output_directory.path().to_owned(),
            Some(captured.clone()),
        )
        .unwrap();
    assert!(matches!(
        written.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Info }]
            if message.contains("Exported 1 saved note")
    ));
    let output_path = output_directory.path().join("review.json");
    let contents = fs::read_to_string(&output_path).unwrap();
    assert!(contents.ends_with('\n'));
    assert_eq!(
        serde_json::from_str::<ExtensionReviewSnapshot>(&contents).unwrap(),
        captured
    );

    extension
        .invoke_command_with_review_context(
            "export",
            core_snapshot.clone(),
            Vec::new(),
            None,
            output_directory.path().to_owned(),
            Some(captured.clone()),
        )
        .unwrap();
    let existing = extension
        .submit_input_dialog_with_context(
            "snapshot-export-path",
            Some("review.json".into()),
            core_snapshot,
            None,
            output_directory.path().to_owned(),
            Some(captured),
        )
        .unwrap();
    assert!(matches!(
        existing.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning }]
            if message.contains("Refusing to overwrite")
    ));
    assert_eq!(fs::read_to_string(output_path).unwrap(), contents);
}

#[test]
fn review_shell_exports_the_visible_authoritative_snapshot_via_f9() {
    let (_extension_directory, manifest) = staged_extension();
    let output_directory = TempDir::new().unwrap();
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let options = ReviewOptions {
        command_cwd: Some(output_directory.path().to_owned()),
        ..ReviewOptions::default()
    };
    let mut app = ReviewApp::new_with_extensions(changeset(), options, vec![extension]);
    let source_state = review_state_with_note();
    let note = source_state.comments()[0].clone();
    app.shared_state()
        .lock()
        .unwrap()
        .add_comment(note)
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();

    press(&mut app, KeyCode::F(9));
    assert!(app.has_extension_input_dialog());
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let dialog_frame = rendered_text(&terminal);
    assert!(dialog_frame.contains("Export review snapshot"));
    assert!(dialog_frame.contains("workdeck-review-snapshot.json"));
    assert!(!output_directory.path().join("tui-review.json").exists());

    for character in "tui-review.json".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    assert!(!app.has_extension_input_dialog());
    let output_path = output_directory.path().join("tui-review.json");
    let exported: ExtensionReviewSnapshot =
        serde_json::from_str(&fs::read_to_string(output_path).unwrap()).unwrap();
    assert_eq!(exported.notes.len(), 1);
    assert_eq!(exported.notes[0].summary, "Keep the semantic address");
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("Exported 1 saved note"));
}

#[test]
fn review_shell_refuses_when_shared_state_advances_behind_the_dialog() {
    let (_extension_directory, manifest) = staged_extension();
    let output_directory = TempDir::new().unwrap();
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let options = ReviewOptions {
        command_cwd: Some(output_directory.path().to_owned()),
        ..ReviewOptions::default()
    };
    let mut app = ReviewApp::new_with_extensions(changeset(), options, vec![extension]);

    press(&mut app, KeyCode::F(9));
    assert!(app.has_extension_input_dialog());
    let note = review_state_with_note().comments()[0].clone();
    app.shared_state()
        .lock()
        .unwrap()
        .add_comment(note)
        .unwrap();
    for character in "stale.json".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);

    assert!(!output_directory.path().join("stale.json").exists());
    let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("review changed while exporting"));
}

#[test]
fn declared_capabilities_match_the_manifest() {
    assert_eq!(
        required_capabilities(),
        [
            Capability::Commands,
            Capability::Dialogs,
            Capability::Notifications,
        ]
    );
}
