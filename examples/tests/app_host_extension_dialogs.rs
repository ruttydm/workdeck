use std::fs;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, SidebarVisibility};
use workdeck_diff::parse_patch;
use workdeck_extension_api::ExtensionNotificationHub;
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-extension-dialogs.json"
    ))
    .unwrap()
}

fn changeset(marker: &str) -> Changeset {
    parse_patch(
        &format!(
            "diff --git a/alpha.txt b/alpha.txt\n--- a/alpha.txt\n+++ b/alpha.txt\n@@ -1 +1,2 @@\n one\n+{marker}\n"
        ),
        format!("changeset:extension-dialogs:{marker}"),
        "AppHost extension dialogs",
        ChangesetSource::Patch {
            label: "extension-dialogs".into(),
        },
    )
    .unwrap()
}

struct Fixture {
    _directory: TempDir,
    app: ReviewApp,
    terminal: Terminal<TestBackend>,
}

impl Fixture {
    fn launch(scenario: &str, width: u16, height: u16) -> Self {
        let directory = TempDir::new().unwrap();
        let binary_directory = directory.path().join("bin");
        fs::create_dir_all(&binary_directory).unwrap();
        let binary_name = format!(
            "workdeck-example-app-host-extension-dialogs-probe-extension{}",
            std::env::consts::EXE_SUFFIX
        );
        fs::copy(
            env!("CARGO_BIN_EXE_workdeck-example-app-host-extension-dialogs-probe-extension"),
            binary_directory.join(binary_name),
        )
        .unwrap();
        let manifest = directory.path().join("workdeck-extension.toml");
        fs::write(
            &manifest,
            include_str!("../extensions/app-host-extension-dialogs-probe/workdeck-extension.toml"),
        )
        .unwrap();
        let notifications = ExtensionNotificationHub::new();
        let extension = LoadedExtension::spawn_with_notifications_and_configuration(
            &manifest,
            "test",
            notifications.clone(),
            serde_json::json!({ "scenario": scenario }),
        )
        .unwrap();
        Self {
            _directory: directory,
            app: ReviewApp::new_with_extensions(
                changeset("initial"),
                ReviewOptions {
                    extension_notifications: Some(notifications),
                    sidebar_visibility: SidebarVisibility::Hidden,
                    sidebar: false,
                    highlight: false,
                    prompt_save_view_preferences: false,
                    ..ReviewOptions::default()
                },
                vec![extension],
            ),
            terminal: Terminal::new(TestBackend::new(width, height)).unwrap(),
        }
    }

    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while self.app.has_pending_extension_commands() || self.app.has_pending_extension_events() {
            self.app.poll_extension_commands();
            assert!(
                Instant::now() < deadline,
                "extension request did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        self.settle();
    }

    fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            self.press(KeyCode::Char(character));
        }
    }

    fn draw(&mut self) -> String {
        self.terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &self.app))
            .unwrap();
        let buffer = self.terminal.backend().buffer();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn wait_for(&mut self, predicate: impl Fn(&str) -> bool) -> String {
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            self.settle();
            let frame = self.draw();
            if predicate(&frame) {
                return frame;
            }
            assert!(
                Instant::now() < deadline,
                "extension dialog frame did not settle:\n{frame}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn click_text(&mut self, text: &str) {
        let frame = self.draw();
        let (row, line) = frame
            .lines()
            .enumerate()
            .find(|(_, line)| line.contains(text))
            .unwrap_or_else(|| panic!("{text:?} was not visible:\n{frame}"));
        let column = line.find(text).unwrap();
        for kind in [
            MouseEventKind::Down(MouseButton::Left),
            MouseEventKind::Up(MouseButton::Left),
        ] {
            self.app.handle_mouse_event(MouseEvent {
                kind,
                column: u16::try_from(column).unwrap(),
                row: u16::try_from(row).unwrap(),
                modifiers: KeyModifiers::NONE,
            });
        }
        self.settle();
    }

    fn notice(&self) -> Option<String> {
        self.app
            .active_extension_notification()
            .map(|notice| notice.message)
    }
}

#[test]
fn frozen_app_host_extension_dialogs_oracle_maps_identical_pins_and_all_source_tests() {
    let oracle = oracle();
    assert_eq!(
        oracle["source"]["baseline"]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["source"]["stable"]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_eq!(
        oracle["source"]["baseline"]["blob"],
        oracle["source"]["stable"]["blob"]
    );
    assert_eq!(oracle["source"]["baseline"]["bytes"], 19_344);
    assert_eq!(oracle["source"]["baseline"]["lines"], 579);
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 8);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 8);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 8);
}

#[test]
fn confirm_dialog_is_attributed_modal_owns_quit_and_enter_resolves_true() {
    let mut fixture = Fixture::launch("confirm", 140, 30);
    assert!(fixture.draw().contains("alpha.txt"));
    fixture.press(KeyCode::Char('y'));
    let dialog = fixture.wait_for(|frame| frame.contains("Reformat the file?"));
    assert!(dialog.contains("This rewrites it in place."));
    assert!(dialog.contains("reformat"));
    assert!(dialog.contains("ext app-host-extension-dialogs-probe"));

    fixture.press(KeyCode::Char('q'));
    assert!(!fixture.app.take_quit_requested());
    assert!(fixture.draw().contains("Reformat the file?"));
    fixture.press(KeyCode::Enter);
    assert_eq!(
        fixture.notice().as_deref(),
        Some("app-host-extension-dialogs-probe: answer true")
    );
    assert!(!fixture.app.has_extension_dialog());
}

#[test]
fn short_confirm_keeps_overflow_and_both_footer_actions_visible() {
    let mut fixture = Fixture::launch("short-confirm", 50, 12);
    fixture.press(KeyCode::Char('y'));
    let dialog = fixture.wait_for(|frame| frame.contains("Short terminal"));
    assert!(dialog.contains('…'), "{dialog}");
    assert!(dialog.contains("enter/y"), "{dialog}");
    assert!(dialog.contains("esc/n"), "{dialog}");
}

#[test]
fn escape_resolves_confirm_false_and_returns_keyboard_to_review() {
    let mut fixture = Fixture::launch("escape-confirm", 140, 30);
    fixture.press(KeyCode::Char('y'));
    assert!(fixture.draw().contains("Discard the draft?"));
    fixture.press(KeyCode::Esc);
    assert_eq!(
        fixture.notice().as_deref(),
        Some("app-host-extension-dialogs-probe: answer false")
    );
    assert!(!fixture.app.has_extension_dialog());
}

#[test]
fn select_dialog_resolves_keyboard_highlight_and_exact_clicked_row() {
    let mut keyboard = Fixture::launch("select", 140, 30);
    keyboard.press(KeyCode::Char('y'));
    let dialog = keyboard.wait_for(|frame| frame.contains("production"));
    assert!(dialog.contains("Where to?"));
    keyboard.press(KeyCode::Down);
    keyboard.press(KeyCode::Enter);
    assert_eq!(
        keyboard.notice().as_deref(),
        Some("app-host-extension-dialogs-probe: answer production")
    );

    let mut mouse = Fixture::launch("select", 140, 30);
    mouse.press(KeyCode::Char('y'));
    mouse.wait_for(|frame| frame.contains("production"));
    mouse.click_text("production");
    assert_eq!(
        mouse.notice().as_deref(),
        Some("app-host-extension-dialogs-probe: answer production")
    );
}

#[test]
fn input_dialog_owns_typing_and_mouse_submit_returns_exact_text() {
    let mut fixture = Fixture::launch("input", 140, 30);
    fixture.press(KeyCode::Char('y'));
    let opened = fixture.wait_for(|frame| frame.contains("Branch name?"));
    assert!(opened.contains("feature/..."));
    fixture.type_text("quick-fix");
    assert!(!fixture.app.take_quit_requested());
    assert!(fixture.draw().contains("quick-fix"));
    fixture.click_text("submit");
    assert_eq!(
        fixture.notice().as_deref(),
        Some("app-host-extension-dialogs-probe: answer quick-fix")
    );
    assert!(!fixture.app.has_extension_dialog());
}

#[test]
fn soft_reload_cancels_the_old_dialog_and_submits_false() {
    let mut fixture = Fixture::launch("reload-cancel", 140, 30);
    fixture.press(KeyCode::Char('y'));
    assert!(fixture.draw().contains("Still relevant?"));
    fixture.app.reload(changeset("reloaded"));
    fixture.settle();
    let replacement = fixture.draw();
    assert!(replacement.contains("reloaded"));
    assert!(!replacement.contains("Still relevant?"));
    assert_eq!(
        fixture.notice().as_deref(),
        Some("app-host-extension-dialogs-probe: answer false")
    );
}

#[test]
fn replacement_reload_event_dialog_survives_old_generation_retirement() {
    let mut fixture = Fixture::launch("reload-lifecycle", 140, 30);
    assert!(!fixture.app.has_extension_dialog());
    fixture.app.reload(changeset("reloaded"));
    let dialog = fixture.wait_for(|frame| frame.contains("Review reloaded"));
    assert!(dialog.contains("alpha.txt"));
    assert!(fixture.app.has_extension_confirm_dialog());
    assert!(fixture.notice().is_none());

    fixture.press(KeyCode::Enter);
    assert_eq!(
        fixture.notice().as_deref(),
        Some("app-host-extension-dialogs-probe: answer true")
    );
    assert!(!fixture.app.has_extension_dialog());
}
