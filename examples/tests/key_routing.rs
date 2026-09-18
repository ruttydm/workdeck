use std::fs;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource};
use workdeck_diff::parse_patch;
use workdeck_extension_api::ExtensionNotificationHub;
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

const WIDTH: u16 = 120;
const HEIGHT: u16 = 24;

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-key-routing.json"
    ))
    .unwrap()
}

fn scrollable_changeset() -> Changeset {
    let mut patch = String::from(
        "diff --git a/big.ts b/big.ts\n--- a/big.ts\n+++ b/big.ts\n@@ -1,80 +1,80 @@\n",
    );
    for index in 1..=80 {
        patch.push_str(&format!("-line {index:02} old value\n"));
    }
    for index in 1..=80 {
        patch.push_str(&format!("+line {index:02} new value\n"));
    }
    parse_patch(
        &patch,
        "key-routing",
        "Key routing",
        ChangesetSource::Patch {
            label: "key-routing".into(),
        },
    )
    .unwrap()
}

fn staged_extension() -> (TempDir, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-key-routing-probe-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-key-routing-probe-extension"),
        binary_directory.join(binary_name),
    )
    .unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/key-routing-probe/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn frame(app: &ReviewApp) -> String {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap();
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), app))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn press(app: &mut ReviewApp, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
    settle(app);
}

fn settle(app: &mut ReviewApp) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while app.has_pending_extension_commands() || app.has_pending_extension_events() {
        app.poll_extension_commands();
        assert!(
            Instant::now() < deadline,
            "extension request did not settle"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait_for_frame(app: &mut ReviewApp, predicate: impl Fn(&str) -> bool) -> String {
    for _ in 0..80 {
        let rendered = frame(app);
        if predicate(&rendered) {
            return rendered;
        }
        settle(app);
        std::thread::sleep(Duration::from_millis(10));
    }
    frame(app)
}

fn wait_for_pane_input(app: &mut ReviewApp) -> String {
    for _ in 0..80 {
        let rendered = frame(app);
        if app.extension_pane_input_cursor_position().is_some() {
            return rendered;
        }
        settle(app);
        std::thread::sleep(Duration::from_millis(10));
    }
    frame(app)
}

fn focus_scrolled_review(app: &mut ReviewApp) {
    let _ = frame(app);
    // Split layout can visit both sides of a row before moving vertically.
    // Establish the scrolled precondition, rather than assuming a fixed number
    // of cursor movements crosses the viewport in every presentation.
    for _ in 0..160 {
        press(app, KeyCode::Char('j'));
        let _ = frame(app);
        if app.review_scroll() > 0 {
            break;
        }
    }
    assert!(app.review_scroll() > 0);
}

fn extension_app(surface: &str) -> (TempDir, ReviewApp) {
    let (directory, manifest) = staged_extension();
    let notifications = ExtensionNotificationHub::new();
    let extension = LoadedExtension::spawn_with_notifications_and_configuration(
        &manifest,
        "test",
        notifications.clone(),
        serde_json::json!({ "surface": surface }),
    )
    .unwrap();
    let app = ReviewApp::new_with_extensions(
        scrollable_changeset(),
        ReviewOptions {
            extension_notifications: Some(notifications.clone()),
            ..ReviewOptions::default()
        },
        vec![extension],
    );
    (directory, app)
}

#[test]
fn frozen_app_host_key_routing_oracle_maps_both_pins_and_all_tests() {
    let oracle = oracle();
    assert_eq!(
        oracle["source"]["baseline"]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["source"]["stable"]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_eq!(oracle["source"]["baseline"]["bytes"], 11_520);
    assert_eq!(oracle["source"]["stable"]["bytes"], 9_387);
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 4);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 3);
    assert_eq!(oracle["oracleRuns"]["baseline"]["expectCalls"], 13);
    assert_eq!(oracle["oracleRuns"]["stable"]["expectCalls"], 9);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 4);
}

#[test]
fn menu_arrows_move_selection_without_scrolling_the_review_behind_it() {
    let mut app = ReviewApp::new(scrollable_changeset(), ReviewOptions::default());
    focus_scrolled_review(&mut app);
    let before = app.review_scroll();

    press(&mut app, KeyCode::F(10));
    assert!(frame(&app).contains("Reload"));
    for _ in 0..3 {
        press(&mut app, KeyCode::Down);
        let rendered = frame(&app);
        assert!(rendered.contains("Reload"), "{rendered}");
    }

    assert_eq!(app.review_scroll(), before);
}

#[test]
fn theme_selector_owns_review_vertical_keys_without_scrolling_the_review() {
    let mut app = ReviewApp::new(scrollable_changeset(), ReviewOptions::default());
    focus_scrolled_review(&mut app);
    let before = app.review_scroll();

    press(&mut app, KeyCode::Char('t'));
    assert!(frame(&app).contains("Theme selector"));
    press(&mut app, KeyCode::Char('j'));
    let rendered = frame(&app);

    assert_eq!(app.review_scroll(), before);
    assert!(rendered.contains("Theme selector"), "{rendered}");
    assert!(rendered.contains("github-dark-dimmed"), "{rendered}");
}

#[test]
fn active_file_view_mode_consumes_handled_keys_and_passes_review_scrolling() {
    let (_directory, mut app) = extension_app("file-view");
    let _ = frame(&app);
    press(&mut app, KeyCode::F(8));
    let rendered = wait_for_frame(&mut app, |frame| frame.contains("TALL ROW 1"));
    assert!(rendered.contains("TALL ROW 1"), "{rendered}");

    focus_scrolled_review(&mut app);
    press(&mut app, KeyCode::F(9));
    assert_eq!(app.active_keyboard_mode_title().as_deref(), Some("tall"));
    let before_scroll = app.review_scroll();
    let before_line = app.current_line_row();

    press(&mut app, KeyCode::Char('n'));
    assert_eq!(app.review_scroll(), before_scroll);
    assert_eq!(app.current_line_row(), before_line);

    press(&mut app, KeyCode::Char('j'));
    assert!(app.current_line_row() > before_line);
    assert!(app.review_scroll() > before_scroll);
}

#[test]
fn focused_extension_pane_input_receives_typing_before_all_commands() {
    let (_directory, mut app) = extension_app("pane");
    let rendered = wait_for_pane_input(&mut app);
    assert!(
        app.extension_pane_input_cursor_position().is_some(),
        "{rendered}"
    );

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('?'));
    let rendered = frame(&app);
    app.tick_extension_notifications(Instant::now());

    assert!(rendered.contains("j?"), "{rendered}");
    assert!(!rendered.contains("COMMAND FIRED"), "{rendered}");
    assert!(!rendered.contains("Controls help"), "{rendered}");
    assert!(app.active_extension_notification().is_none());
}

#[test]
fn focused_extension_pane_input_edits_unicode_at_the_host_cursor() {
    let (_directory, mut app) = extension_app("pane");
    let rendered = wait_for_pane_input(&mut app);
    assert!(
        app.extension_pane_input_cursor_position().is_some(),
        "{rendered}"
    );

    for character in ['a', '界', 'b'] {
        press(&mut app, KeyCode::Char(character));
    }
    press(&mut app, KeyCode::Left);
    press(&mut app, KeyCode::Backspace);
    assert!(frame(&app).contains("ab"));
    press(&mut app, KeyCode::Delete);
    press(&mut app, KeyCode::Char('z'));
    press(&mut app, KeyCode::Home);
    press(&mut app, KeyCode::Char('x'));

    let rendered = frame(&app);
    assert!(rendered.contains("xaz"), "{rendered}");
}

#[test]
fn focused_extension_pane_input_swallows_modified_shortcuts_and_job_control() {
    let (_directory, mut app) = extension_app("pane");
    let rendered = wait_for_pane_input(&mut app);
    assert!(
        app.extension_pane_input_cursor_position().is_some(),
        "{rendered}"
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    settle(&mut app);

    assert!(!app.take_quit_requested());
    assert!(app.extension_pane_input_cursor_position().is_some());
}
