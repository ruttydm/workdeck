use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource};
use workdeck_diff::parse_patch;
use workdeck_extension_api::ExtensionNotificationHub;
use workdeck_extension_host::{ExtensionRuntimeRegistry, LoadedExtension};
use workdeck_tui::{EXTENSION_TOAST_DURATION_MS, ReviewApp, ReviewOptions, render};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-keyboard-modes.json"
    ))
    .unwrap()
}

fn staged_extension() -> (TempDir, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-keyboard-probe-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-keyboard-probe-extension"),
        binary_directory.join(binary_name),
    )
    .unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/keyboard-probe/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn changeset() -> Changeset {
    parse_patch(
        "diff --git a/alpha.ts b/alpha.ts\n--- a/alpha.ts\n+++ b/alpha.ts\n@@ -1 +1 @@\n-old\n+new\n",
        "keyboard-mode",
        "Keyboard mode",
        ChangesetSource::Patch {
            label: "keyboard-mode".into(),
        },
    )
    .unwrap()
}

struct Harness {
    _directory: TempDir,
    app: ReviewApp,
    registry: Arc<ExtensionRuntimeRegistry>,
    notices: Vec<String>,
    toast_now: Instant,
}

impl Harness {
    fn new() -> Self {
        let (directory, manifest) = staged_extension();
        let notifications = ExtensionNotificationHub::new();
        let extension =
            LoadedExtension::spawn_with_notifications(&manifest, "test", notifications.clone())
                .unwrap();
        let registry = extension.registry();
        let app = ReviewApp::new_with_extensions(
            changeset(),
            ReviewOptions {
                extension_notifications: Some(notifications),
                ..ReviewOptions::default()
            },
            vec![extension],
        );
        Self {
            _directory: directory,
            app,
            registry,
            notices: Vec::new(),
            toast_now: Instant::now(),
        }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        self.settle();
    }

    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.app.has_pending_extension_commands() {
            self.app.poll_extension_commands();
            assert!(
                Instant::now() < deadline,
                "extension command did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn drain_notices(&mut self) {
        self.app.tick_extension_notifications(self.toast_now);
        while let Some(notification) = self.app.active_extension_notification() {
            self.notices.push(
                notification
                    .message
                    .strip_prefix("test.keyboard-probe: ")
                    .unwrap_or(&notification.message)
                    .to_owned(),
            );
            self.toast_now += Duration::from_millis(EXTENSION_TOAST_DURATION_MS + 1);
            self.app.tick_extension_notifications(self.toast_now);
        }
    }

    fn notice_count(&self, message: &str) -> usize {
        self.notices
            .iter()
            .filter(|notice| notice.as_str() == message)
            .count()
    }

    fn frame(&self) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &self.app))
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

    fn wait_for_frame(&mut self, predicate: impl Fn(&str) -> bool) -> String {
        for _ in 0..80 {
            let frame = self.frame();
            if predicate(&frame) {
                return frame;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        self.frame()
    }

    fn enter_both_modes(&mut self) {
        self.press(KeyCode::F(8));
        self.press(KeyCode::F(9));
        self.drain_notices();
        assert_eq!(
            self.app.active_keyboard_mode_title().as_deref(),
            Some("Probe normal")
        );
        let frame = self.wait_for_frame(|frame| frame.contains("FOCUSED VIEW"));
        assert!(
            frame.contains("FOCUSED VIEW"),
            "notices={:?}\n{frame}",
            self.notices
        );
        assert!(frame.contains("Probe normal"), "{frame}");
    }
}

#[test]
fn frozen_app_host_keyboard_modes_oracle_maps_identical_pins_and_all_tests() {
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
    assert_eq!(oracle["source"]["baseline"]["bytes"], 11003);
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 5);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 5);
    assert_eq!(oracle["oracleRuns"]["baseline"]["expectCalls"], 26);
    assert_eq!(oracle["oracleRuns"]["stable"]["expectCalls"], 26);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 5);
}

#[test]
fn routes_handled_and_passed_keys_after_modal_and_filter_owners() {
    let mut harness = Harness::new();
    harness.press(KeyCode::F(8));

    harness.press(KeyCode::Char('j'));
    harness.press(KeyCode::Char('p'));
    harness.press(KeyCode::F(10));
    assert!(harness.frame().contains("Reload"));
    harness.press(KeyCode::F(10));
    assert!(!harness.frame().contains("Reload"));
    assert!(harness.frame().contains("Probe normal"));

    // `/` belongs to content search; Tab reaches the filter.
    harness.press(KeyCode::Tab);
    harness.drain_notices();
    let filter_frame = harness.frame();
    assert!(
        filter_frame.contains("filter:"),
        "notices={:?}\n{filter_frame}",
        harness.notices
    );
    let before = harness.notice_count("SESSION KEY j");
    harness.press(KeyCode::Char('j'));
    harness.drain_notices();
    assert_eq!(harness.notice_count("SESSION KEY j"), before);
    harness.press(KeyCode::Tab);
    harness.press(KeyCode::Char('j'));
    harness.drain_notices();

    assert!(
        harness
            .notices
            .iter()
            .any(|notice| notice == "SESSION KEY p")
    );
    assert!(harness.notices.iter().any(|notice| notice == "COMMAND P"));
    assert_eq!(harness.notice_count("SESSION KEY j"), before + 1);
    assert_eq!(harness.notice_count("SESSION KEY f10"), 0);
}

#[test]
fn open_menu_accelerators_precede_both_extension_mode_layers() {
    let mut harness = Harness::new();
    harness.enter_both_modes();
    harness.drain_notices();

    harness.press(KeyCode::F(10));
    assert!(harness.frame().contains("Reload"));
    harness.press(KeyCode::Char('?'));
    harness.drain_notices();
    let frame = harness.frame();

    assert!(frame.contains("Controls help"));
    assert!(!frame.contains("Reload"));
    assert_eq!(harness.notice_count("FILE KEY ?"), 0);
    assert_eq!(harness.notice_count("SESSION KEY ?"), 0);
}

#[test]
fn focused_file_mode_owns_first_escape_and_session_mode_owns_second() {
    let mut harness = Harness::new();
    harness.enter_both_modes();
    harness.drain_notices();

    harness.press(KeyCode::Esc);
    harness.drain_notices();
    assert_eq!(harness.notice_count("FILE EXIT"), 1);
    assert_eq!(harness.notice_count("SESSION EXIT"), 0);
    assert_eq!(
        harness.app.active_keyboard_mode_title().as_deref(),
        Some("Probe normal")
    );

    harness.press(KeyCode::Esc);
    harness.drain_notices();
    assert_eq!(
        harness.notice_count("SESSION EXIT"),
        1,
        "notices={:?}",
        harness.notices
    );
    assert_eq!(harness.notice_count("SESSION REENTER false"), 1);
    assert_eq!(harness.notice_count("FILE KEY escape"), 0);
    assert_eq!(harness.notice_count("SESSION KEY escape"), 0);
    assert!(harness.app.active_keyboard_mode_title().is_none());
}

#[test]
fn two_escapes_in_one_input_flush_update_ownership_eagerly() {
    let mut harness = Harness::new();
    harness.enter_both_modes();
    harness.drain_notices();

    harness
        .app
        .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    harness
        .app
        .handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    harness.drain_notices();

    assert_eq!(harness.notice_count("FILE EXIT"), 1);
    assert_eq!(
        harness.notice_count("SESSION EXIT"),
        1,
        "notices={:?}",
        harness.notices
    );
    assert!(harness.app.active_keyboard_mode_title().is_none());
}

#[test]
fn closed_registry_retires_mode_before_delivering_another_key() {
    let mut harness = Harness::new();
    harness.press(KeyCode::F(8));
    harness.drain_notices();

    harness
        .registry
        .set_phase(workdeck_extension_host::ExtensionEventBusPhase::Closed);
    harness.press(KeyCode::Char('j'));
    harness.drain_notices();

    assert_eq!(harness.notice_count("SESSION KEY j"), 0);
    assert_eq!(
        harness.notice_count("SESSION EXIT"),
        1,
        "notices={:?}",
        harness.notices
    );
    assert!(harness.app.active_keyboard_mode_title().is_none());
}
