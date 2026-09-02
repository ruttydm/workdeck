use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;
use workdeck_core::{
    Changeset, ChangesetSource, ReviewFileChangeKind, ReviewSelection, ReviewSide, ReviewSnapshot,
};
use workdeck_diff::parse_patch;
use workdeck_examples::review_note_navigator_extension::{
    ReviewNoteChoice, build_review_note_choices, navigate_to_saved_review_note,
    required_capabilities, selected_review_note_choice,
};
use workdeck_extension_api::{
    Capability, ExtensionHostAction, ExtensionNotifyType, ExtensionReviewNoteResolution,
    ExtensionReviewSnapshot, ExtensionReviewSnapshotFile, ExtensionReviewSnapshotFileFlags,
    ExtensionReviewSnapshotFileStats, ExtensionReviewSnapshotLineAddress,
    ExtensionReviewSnapshotNote, ExtensionReviewSnapshotNoteAnchor, Registration,
};
use workdeck_extension_host::LoadedExtension;
use workdeck_review::{CommentAnchor, ReviewComment, ReviewNoteResolution};
use workdeck_tui::{ReviewApp, ReviewOptions, render};

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-review-note-navigator-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-review-note-navigator-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/review-note-navigator/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn changeset() -> Changeset {
    parse_patch(
        "diff --git a/src/alpha.ts b/src/alpha.ts\n--- a/src/alpha.ts\n+++ b/src/alpha.ts\n@@ -1 +1 @@\n-old\n+new\n",
        "note-navigator",
        "Note navigator",
        ChangesetSource::Patch {
            label: "note-navigator".into(),
        },
    )
    .unwrap()
}

fn core_snapshot() -> ReviewSnapshot {
    ReviewSnapshot {
        generation: 1,
        changeset: changeset(),
        selection: ReviewSelection::default(),
    }
}

fn file() -> ExtensionReviewSnapshotFile {
    ExtensionReviewSnapshotFile {
        file_key: "alpha-key".into(),
        runtime_id: "runtime-alpha".into(),
        path: "src/alpha.ts".into(),
        previous_path: None,
        change_kind: ReviewFileChangeKind::Change,
        stats: ExtensionReviewSnapshotFileStats {
            additions: 2,
            deletions: 1,
            truncated: false,
        },
        flags: ExtensionReviewSnapshotFileFlags {
            untracked: false,
            binary: false,
            too_large: false,
            partial: false,
        },
        content_identity: "sha256:alpha".into(),
        source_identity: None,
        source_attested: None,
    }
}

fn note(
    id: &str,
    file_key: &str,
    side: Option<ReviewSide>,
    line: Option<u32>,
    summary: &str,
    resolution: ExtensionReviewNoteResolution,
) -> ExtensionReviewSnapshotNote {
    ExtensionReviewSnapshotNote {
        id: id.into(),
        parent_id: None,
        source: if id == "note:stale" {
            workdeck_core::ReviewNoteSource::Agent
        } else {
            workdeck_core::ReviewNoteSource::User
        },
        original_source: None,
        file_key: file_key.into(),
        anchor: ExtensionReviewSnapshotNoteAnchor {
            old_range: None,
            new_range: None,
            preferred: side
                .zip(line)
                .map(|(side, line)| ExtensionReviewSnapshotLineAddress { side, line }),
            intersecting_hunk_indices: if resolution == ExtensionReviewNoteResolution::Orphaned {
                Vec::new()
            } else {
                vec![0]
            },
            owner_hunk_index: (resolution != ExtensionReviewNoteResolution::Orphaned).then_some(0),
        },
        summary: summary.into(),
        rationale: None,
        markup: None,
        title: None,
        author: None,
        created_at: None,
        updated_at: None,
        editable: id != "note:stale",
        tags: Vec::new(),
        confidence: None,
        resolution,
    }
}

fn review_snapshot() -> ExtensionReviewSnapshot {
    ExtensionReviewSnapshot {
        generation: "generation:test:1".into(),
        state_revision: 3,
        files: vec![file()],
        notes: vec![
            note(
                "note:active",
                "alpha-key",
                Some(ReviewSide::New),
                Some(12),
                "Check   the\nreturn value.",
                ExtensionReviewNoteResolution::Active,
            ),
            note(
                "note:stale",
                "alpha-key",
                Some(ReviewSide::Old),
                Some(8),
                "Recheck after the refactor.",
                ExtensionReviewNoteResolution::Stale,
            ),
            note(
                "note:orphaned",
                "retired-key",
                None,
                None,
                "Keep the deleted fallback in mind.",
                ExtensionReviewNoteResolution::Orphaned,
            ),
        ],
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
}

#[test]
fn translated_choice_labels_preserve_order_status_location_and_duplicate_identity() {
    let snapshot = review_snapshot();
    let choices = build_review_note_choices(&snapshot);
    assert_eq!(
        choices,
        [
            ReviewNoteChoice {
                label: "1. [active] src/alpha.ts:12 (new) — Check the return value.".into(),
                note_id: "note:active".into(),
            },
            ReviewNoteChoice {
                label: "2. [stale] src/alpha.ts:8 (old) — Recheck after the refactor.".into(),
                note_id: "note:stale".into(),
            },
            ReviewNoteChoice {
                label:
                    "3. [orphaned] retired file retired-key — Keep the deleted fallback in mind."
                        .into(),
                note_id: "note:orphaned".into(),
            },
        ]
    );

    let mut duplicated = snapshot.clone();
    duplicated.notes = vec![snapshot.notes[0].clone(), snapshot.notes[0].clone()];
    duplicated.notes[1].id = "note:second".into();
    let duplicate_choices = build_review_note_choices(&duplicated);
    assert_ne!(duplicate_choices[0].label, duplicate_choices[1].label);
    assert_eq!(
        duplicate_choices
            .iter()
            .map(|choice| choice.note_id.as_str())
            .collect::<Vec<_>>(),
        ["note:active", "note:second"]
    );
}

#[test]
fn translated_choice_resolution_survives_host_sanitization() {
    let mut snapshot = review_snapshot();
    snapshot.files[0].path = "evil\u{1b}[31m.ts".into();
    let choices = build_review_note_choices(&snapshot);
    assert!(choices[0].label.contains("\u{1b}[31m"));
    assert_eq!(
        selected_review_note_choice(
            &choices,
            "1. [active] evil.ts:12 (new) — Check the return value."
        )
        .map(|choice| choice.note_id.as_str()),
        Some("note:active")
    );
    assert!(selected_review_note_choice(&choices, "not an option").is_none());
}

#[test]
fn translated_navigation_keeps_owner_hunk_before_exact_line_fallback() {
    let snapshot = review_snapshot();
    let mut gap_note = snapshot.notes[0].clone();
    gap_note.anchor.intersecting_hunk_indices.clear();
    assert_eq!(
        navigate_to_saved_review_note(&snapshot.files[0], &gap_note),
        [
            ExtensionHostAction::SelectReviewHunk {
                file_id: "runtime-alpha".into(),
                hunk_index: 0,
            },
            ExtensionHostAction::RevealReviewLine {
                file_id: "runtime-alpha".into(),
                side: ReviewSide::New,
                line: 12,
            },
        ]
    );
    gap_note.anchor.owner_hunk_index = None;
    gap_note.anchor.preferred = None;
    assert_eq!(
        navigate_to_saved_review_note(&snapshot.files[0], &gap_note),
        [ExtensionHostAction::SelectReviewFile {
            file_id: "runtime-alpha".into(),
        }]
    );
}

#[test]
fn compiled_extension_re_resolves_note_identity_and_reports_retired_states() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    assert!(extension.handshake.registrations.iter().any(|registration| {
        matches!(registration, Registration::Command(command) if command.id == "navigate" && command.default_keys == ["f8"])
    }));
    let empty = ExtensionReviewSnapshot {
        generation: "generation:test:empty".into(),
        state_revision: 0,
        files: Vec::new(),
        notes: Vec::new(),
    };
    let no_notes = extension
        .invoke_command_with_review_context(
            "navigate",
            core_snapshot(),
            Vec::new(),
            None,
            PathBuf::new(),
            Some(empty),
        )
        .unwrap();
    assert!(matches!(
        no_notes.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Info }]
            if message == "This review has no saved notes"
    ));
    let captured = review_snapshot();
    let dialog = extension
        .invoke_command_with_review_context(
            "navigate",
            core_snapshot(),
            Vec::new(),
            None,
            PathBuf::new(),
            Some(captured.clone()),
        )
        .unwrap();
    let ExtensionHostAction::OpenSelectDialog { options, .. } = &dialog.actions[0] else {
        panic!("expected select dialog");
    };
    assert_eq!(
        options,
        &build_review_note_choices(&captured)
            .iter()
            .map(|choice| choice.label.clone())
            .collect::<Vec<_>>()
    );

    let mut current = captured.clone();
    current.state_revision += 1;
    current.notes[0].anchor.preferred = Some(ExtensionReviewSnapshotLineAddress {
        side: ReviewSide::New,
        line: 20,
    });
    let navigated = extension
        .submit_select_dialog_with_context(
            "saved-review-note",
            Some(options[0].clone()),
            core_snapshot(),
            None,
            PathBuf::new(),
            Some(current),
        )
        .unwrap();
    assert!(matches!(
        navigated.actions.as_slice(),
        [
            ExtensionHostAction::SelectReviewHunk { hunk_index: 0, .. },
            ExtensionHostAction::RevealReviewLine { line: 20, .. }
        ]
    ));

    extension
        .invoke_command_with_review_context(
            "navigate",
            core_snapshot(),
            Vec::new(),
            None,
            PathBuf::new(),
            Some(captured.clone()),
        )
        .unwrap();
    let orphaned = extension
        .submit_select_dialog_with_context(
            "saved-review-note",
            Some(build_review_note_choices(&captured)[2].label.clone()),
            core_snapshot(),
            None,
            PathBuf::new(),
            Some(captured.clone()),
        )
        .unwrap();
    assert!(matches!(
        orphaned.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning }]
            if message.contains("orphaned")
    ));

    extension
        .invoke_command_with_review_context(
            "navigate",
            core_snapshot(),
            Vec::new(),
            None,
            PathBuf::new(),
            Some(captured.clone()),
        )
        .unwrap();
    let mut removed = captured.clone();
    removed.notes.clear();
    let missing = extension
        .submit_select_dialog_with_context(
            "saved-review-note",
            Some(build_review_note_choices(&captured)[0].label.clone()),
            core_snapshot(),
            None,
            PathBuf::new(),
            Some(removed),
        )
        .unwrap();
    assert!(matches!(
        missing.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning }]
            if message.contains("no longer exists")
    ));

    extension
        .invoke_command_with_review_context(
            "navigate",
            core_snapshot(),
            Vec::new(),
            None,
            PathBuf::new(),
            Some(captured.clone()),
        )
        .unwrap();
    let mut retired_file = captured.clone();
    retired_file.files.clear();
    let missing_file = extension
        .submit_select_dialog_with_context(
            "saved-review-note",
            Some(build_review_note_choices(&captured)[0].label.clone()),
            core_snapshot(),
            None,
            PathBuf::new(),
            Some(retired_file),
        )
        .unwrap();
    assert!(matches!(
        missing_file.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning }]
            if message.contains("file is no longer in the review")
    ));

    extension
        .invoke_command_with_review_context(
            "navigate",
            core_snapshot(),
            Vec::new(),
            None,
            PathBuf::new(),
            Some(captured.clone()),
        )
        .unwrap();
    let retired_review = extension
        .submit_select_dialog_with_context(
            "saved-review-note",
            Some(build_review_note_choices(&captured)[0].label.clone()),
            core_snapshot(),
            None,
            PathBuf::new(),
            None,
        )
        .unwrap();
    assert!(matches!(
        retired_review.actions.as_slice(),
        [ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning }]
            if message.contains("review changed")
    ));
}

fn tui_comment(file_key: String, resolution: ReviewNoteResolution) -> ReviewComment {
    ReviewComment {
        id: if resolution == ReviewNoteResolution::Orphaned {
            "orphaned-note".into()
        } else {
            "active-note".into()
        },
        parent_id: None,
        source: "user".into(),
        author: Some("Reviewer".into()),
        created_at: None,
        file_path: Some("src/alpha.ts".into()),
        hunk_index: Some(0),
        side: Some(ReviewSide::New),
        line: Some(1),
        summary: "Check the return value".into(),
        rationale: None,
        markup: None,
        title: None,
        tags: Vec::new(),
        confidence: None,
        updated_at: None,
        resolution,
        anchor: CommentAnchor {
            file_key,
            old_range: None,
            new_range: None,
            preferred_side: Some(ReviewSide::New),
            preferred_line: Some(1),
            intersecting_hunk_indices: vec![0],
            owner_hunk_index: Some(0),
        },
        editable: true,
    }
}

#[test]
fn review_shell_renders_selects_navigates_warns_and_cancels_on_reload() {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut app =
        ReviewApp::new_with_extensions(changeset(), ReviewOptions::default(), vec![extension]);
    let file_key = app.shared_state().lock().unwrap().changeset().files[0]
        .key
        .clone();
    app.shared_state()
        .lock()
        .unwrap()
        .add_comment(tui_comment(file_key, ReviewNoteResolution::Active))
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();

    press(&mut app, KeyCode::F(8));
    assert!(app.has_extension_select_dialog());
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let dialog = rendered_text(&terminal);
    assert!(dialog.contains("Navigate saved review note"));
    assert!(dialog.contains("1. [active] src/alpha.ts:1 (new)"));
    press(&mut app, KeyCode::Enter);
    assert!(!app.has_extension_select_dialog());
    assert_eq!(app.shared_state().lock().unwrap().selection().line, Some(1));

    let orphaned = tui_comment("retired-file".into(), ReviewNoteResolution::Orphaned);
    app.shared_state()
        .lock()
        .unwrap()
        .add_comment(orphaned)
        .unwrap();
    press(&mut app, KeyCode::F(8));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Enter);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let warning = rendered_text(&terminal);
    assert!(warning.contains("warning:"));
    assert!(warning.contains("orphaned"));

    press(&mut app, KeyCode::F(8));
    assert!(app.has_extension_select_dialog());
    app.reload(changeset());
    assert!(!app.has_extension_dialog());

    drop(app);
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut unsafe_path = changeset();
    unsafe_path.files[0].path = "evil\u{1b}[31m.ts".into();
    unsafe_path.refresh_review_identities();
    let mut app =
        ReviewApp::new_with_extensions(unsafe_path, ReviewOptions::default(), vec![extension]);
    let file_key = app.shared_state().lock().unwrap().changeset().files[0]
        .key
        .clone();
    app.shared_state()
        .lock()
        .unwrap()
        .add_comment(tui_comment(file_key, ReviewNoteResolution::Active))
        .unwrap();
    press(&mut app, KeyCode::F(8));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let sanitized = rendered_text(&terminal);
    assert!(!sanitized.contains('\u{1b}'));
    assert!(sanitized.contains("evil.ts:1 (new)"));
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.shared_state().lock().unwrap().selection().line, Some(1));
}

#[test]
fn declared_capabilities_match_the_native_manifest() {
    assert_eq!(
        required_capabilities(),
        [
            Capability::Commands,
            Capability::Dialogs,
            Capability::Notifications,
            Capability::ReviewNavigation,
        ]
    );
}
