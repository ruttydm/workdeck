use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use std::fs;
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, ReviewSelection, ReviewSnapshot};
use workdeck_diff::parse_patch;
use workdeck_extension_api::{
    ExtensionHostAction, ExtensionKeyEvent, KeyRoutingResult, Registration,
};
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

const QUALIFIED_MODE: &str = "example.vim-navigation:normal";

fn staged_extension() -> (TempDir, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-vim-navigation-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-vim-navigation-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/vim-navigation/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn changeset() -> Changeset {
    let additions = (1..=48)
        .map(|line| format!("+line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    parse_patch(
        &format!(
            "diff --git a/src/lib.rs b/src/lib.rs\n--- /dev/null\n+++ b/src/lib.rs\n@@ -0,0 +1,48 @@\n{additions}\n"
        ),
        "vim-example",
        "Vim example",
        ChangesetSource::Patch {
            label: "Vim example".into(),
        },
    )
    .unwrap()
}

fn snapshot() -> ReviewSnapshot {
    ReviewSnapshot {
        generation: 1,
        changeset: changeset(),
        selection: ReviewSelection::default(),
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
fn compiled_extension_exposes_mode_lifecycle_navigation_and_dialog_protocol() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    assert!(extension.handshake.registrations.iter().any(|registration| {
        matches!(registration, Registration::KeyboardMode(mode) if mode.id == "normal" && mode.title == "Vim navigation")
    }));

    let execution = extension
        .invoke_command_with_context("toggle", snapshot(), Vec::new(), None)
        .unwrap();
    assert_eq!(
        execution.actions,
        vec![ExtensionHostAction::EnterKeyboardMode {
            id: "normal".into()
        }]
    );
    let execution = extension.enter_keyboard_mode("normal", snapshot()).unwrap();
    assert!(matches!(
        execution.actions.as_slice(),
        [ExtensionHostAction::ExecuteReviewCommand { id, count: None }]
            if id == "workdeck.view.cursor-line-row"
    ));

    let digit = extension
        .route_keyboard_mode_key(
            "normal",
            ExtensionKeyEvent {
                name: "5".into(),
                sequence: "5".into(),
                ..ExtensionKeyEvent::default()
            },
            snapshot(),
        )
        .unwrap();
    assert_eq!(digit.result, KeyRoutingResult::Handled);
    assert!(digit.actions.is_empty());
    let down = extension
        .route_keyboard_mode_key(
            "normal",
            ExtensionKeyEvent {
                name: "j".into(),
                sequence: "j".into(),
                ..ExtensionKeyEvent::default()
            },
            snapshot(),
        )
        .unwrap();
    assert!(matches!(
        down.actions.as_slice(),
        [ExtensionHostAction::ExecuteReviewCommand { id, count: Some(5) }]
            if id == "workdeck.review.step-down"
    ));

    let dialog = extension
        .invoke_command_with_context(
            "command-line",
            snapshot(),
            Vec::new(),
            Some(QUALIFIED_MODE.into()),
        )
        .unwrap();
    assert!(matches!(
        dialog.actions.as_slice(),
        [ExtensionHostAction::OpenInputDialog { title, placeholder, .. }]
            if title == "Vim command (:)" && placeholder == "top or bottom"
    ));
    let submitted = extension
        .submit_input_dialog(
            "vim-command",
            Some(" : bottom ".into()),
            snapshot(),
            Some(QUALIFIED_MODE.into()),
        )
        .unwrap();
    assert!(matches!(
        submitted.actions.as_slice(),
        [ExtensionHostAction::ExecuteReviewCommand { id, count: None }]
            if id == "workdeck.review.jump-to-bottom"
    ));
}

#[test]
fn review_shell_routes_f6_counts_ex_dialog_escape_and_reload() {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut app =
        ReviewApp::new_with_extensions(changeset(), ReviewOptions::default(), vec![extension]);
    let mut terminal = Terminal::new(TestBackend::new(100, 18)).unwrap();

    press(&mut app, KeyCode::F(6));
    assert_eq!(
        app.active_keyboard_mode_title().as_deref(),
        Some("Vim navigation")
    );
    let initial = app.current_line_row();
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let active_frame = rendered_text(&terminal);
    assert!(active_frame.contains("Vim navigation"));
    assert!(active_frame.contains("ext example.vim-navigation"));

    press(&mut app, KeyCode::Char('5'));
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.current_line_row(), initial + 5);

    press(&mut app, KeyCode::Char(':'));
    assert!(app.has_extension_input_dialog());
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let dialog = rendered_text(&terminal);
    assert!(dialog.contains("Vim command (:)"));
    assert!(dialog.contains("top or bottom"));
    for character in "bottom".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    assert!(!app.has_extension_input_dialog());
    assert!(app.current_line_row() > 40);
    assert!(app.review_scroll() > 20);

    press(&mut app, KeyCode::Char(':'));
    for character in "middle".chars() {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Enter);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("warning:"));

    press(&mut app, KeyCode::Esc);
    assert!(app.active_keyboard_mode_title().is_none());

    press(&mut app, KeyCode::F(6));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 90,
        row: 17,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.active_keyboard_mode_title().is_none());

    press(&mut app, KeyCode::F(6));
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 90,
        row: 1,
        modifiers: KeyModifiers::NONE,
    });
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("Exit Vim navigation"));
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 90,
        row: 3,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.active_keyboard_mode_title().is_none());

    press(&mut app, KeyCode::F(6));
    assert!(app.active_keyboard_mode_title().is_some());
    press(&mut app, KeyCode::Char(':'));
    assert!(app.has_extension_input_dialog());
    app.reload(changeset());
    assert!(app.active_keyboard_mode_title().is_none());
    assert!(!app.has_extension_input_dialog());
}
