use std::fs;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, SidebarVisibility};
use workdeck_diff::parse_patch;
use workdeck_extension_api::ExtensionNotificationHub;
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

const WIDTH: u16 = 120;
const HEIGHT: u16 = 24;

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-file-view-modes.json"
    ))
    .unwrap()
}

fn changeset(marker: &str, two_files: bool) -> Changeset {
    let mut patch = format!(
        "diff --git a/alpha.ts b/alpha.ts\n--- a/alpha.ts\n+++ b/alpha.ts\n@@ -1 +1 @@\n-export const alpha = 'old';\n+export const alpha = '{marker}';\n"
    );
    if two_files {
        patch.push_str(
            "diff --git a/beta.ts b/beta.ts\n--- a/beta.ts\n+++ b/beta.ts\n@@ -1 +1 @@\n-export const beta = 1;\n+export const beta = 2;\n",
        );
    }
    parse_patch(
        &patch,
        format!("changeset:file-view-mode:{marker}"),
        "AppHost file-view modes",
        ChangesetSource::Patch {
            label: "file-view-mode".into(),
        },
    )
    .unwrap()
}

struct Fixture {
    _directory: TempDir,
    app: ReviewApp,
    terminal: Terminal<TestBackend>,
    file_ids: Vec<String>,
}

impl Fixture {
    fn launch(review: Changeset) -> Self {
        let directory = TempDir::new().unwrap();
        let binary_directory = directory.path().join("bin");
        fs::create_dir_all(&binary_directory).unwrap();
        let binary_name = format!(
            "workdeck-example-app-host-file-view-modes-probe-extension{}",
            std::env::consts::EXE_SUFFIX
        );
        fs::copy(
            env!("CARGO_BIN_EXE_workdeck-example-app-host-file-view-modes-probe-extension"),
            binary_directory.join(binary_name),
        )
        .unwrap();
        let manifest = directory.path().join("workdeck-extension.toml");
        fs::write(
            &manifest,
            include_str!("../extensions/app-host-file-view-modes-probe/workdeck-extension.toml"),
        )
        .unwrap();
        let notifications = ExtensionNotificationHub::new();
        let extension =
            LoadedExtension::spawn_with_notifications(&manifest, "test", notifications.clone())
                .unwrap();
        let file_ids = review
            .files
            .iter()
            .map(|file| file.runtime_id.clone())
            .collect();
        Self {
            _directory: directory,
            app: ReviewApp::new_with_extensions(
                review,
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
            terminal: Terminal::new(TestBackend::new(WIDTH, HEIGHT)).unwrap(),
            file_ids,
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

    fn press_unsettled(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
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
                "file-view mode frame did not settle:\n{frame}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

#[test]
fn frozen_app_host_file_view_modes_oracle_maps_identical_pins_and_all_source_tests() {
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
    assert_eq!(oracle["source"]["baseline"]["bytes"], 28_767);
    assert_eq!(oracle["source"]["baseline"]["lines"], 704);
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 9);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 9);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 9);
}

#[test]
fn live_mode_enters_from_raw_routes_handled_and_pass_keys_and_escapes_once() {
    let mut fixture = Fixture::launch(changeset("initial", false));
    let initial = fixture.draw();
    assert!(initial.contains("alpha.ts"));
    assert!(!initial.contains("ALPHA VIEW"));

    fixture.press(KeyCode::F(9));
    let entered = fixture
        .wait_for(|frame| frame.contains("ALPHA VIEW") && frame.contains("mode — Esc exits"));
    assert_eq!(
        fixture.app.active_keyboard_mode_title().as_deref(),
        Some("alpha")
    );
    assert_eq!(entered.matches("ENTER alpha alpha.ts").count(), 1);

    fixture.press(KeyCode::Char('j'));
    assert!(
        fixture
            .wait_for(|frame| frame.contains("CURSOR 1"))
            .contains("KEY alpha j")
    );
    fixture.press(KeyCode::Char('n'));
    let handled = fixture.wait_for(|frame| frame.contains("KEY alpha n"));
    assert!(!handled.contains("COMMAND N RAN"));

    fixture.press(KeyCode::Char('p'));
    let passed =
        fixture.wait_for(|frame| frame.contains("COMMAND P RAN") && frame.contains("KEY alpha p"));
    assert!(passed.contains("KEY alpha p"), "{passed}");

    fixture.press(KeyCode::Esc);
    assert!(fixture.app.active_keyboard_mode_title().is_none());
    let exited = fixture.wait_for(|frame| frame.contains("EXIT alpha alpha.ts"));
    assert!(!exited.contains("KEY alpha escape"));

    fixture.press(KeyCode::Char('n'));
    assert!(
        fixture
            .wait_for(|frame| frame.contains("COMMAND N RAN"))
            .contains("ALPHA VIEW")
    );
}

#[test]
fn mode_exits_on_its_answer_explicit_leave_deselection_and_file_navigation() {
    let mut fixture = Fixture::launch(changeset("initial", true));
    let first_file = fixture.file_ids[0].clone();

    fixture.press(KeyCode::F(9));
    fixture.press(KeyCode::Char('x'));
    assert!(fixture.app.active_keyboard_mode_title().is_none());
    assert!(
        fixture
            .wait_for(|frame| frame.contains("EXIT alpha alpha.ts"))
            .contains("KEY alpha x")
    );

    fixture.press(KeyCode::F(9));
    fixture.press(KeyCode::F(5));
    assert!(fixture.app.active_keyboard_mode_title().is_none());
    let explicitly_left =
        fixture.wait_for(|frame| frame.matches("EXIT alpha alpha.ts").count() == 2);
    assert_eq!(explicitly_left.matches("EXIT alpha alpha.ts").count(), 2);

    fixture.press(KeyCode::F(9));
    fixture.press(KeyCode::F(8));
    assert!(fixture.app.active_keyboard_mode_title().is_none());
    assert_eq!(fixture.app.selected_extension_file_view(&first_file), None);

    fixture.press(KeyCode::F(9));
    fixture.press(KeyCode::Char('.'));
    assert!(fixture.app.active_keyboard_mode_title().is_none());
}

#[test]
fn entering_refuses_unknown_plain_and_nonmatching_views_without_selecting_them() {
    let mut fixture = Fixture::launch(changeset("initial", false));
    let file_id = fixture.file_ids[0].clone();

    fixture.press(KeyCode::F(4));
    let picky = fixture.wait_for(|frame| frame.contains("does not match the selected file"));
    assert!(!picky.contains("PICKY VIEW"));
    assert!(fixture.app.active_keyboard_mode_title().is_none());
    assert_eq!(fixture.app.selected_extension_file_view(&file_id), None);

    fixture.press(KeyCode::F(6));
    assert!(
        fixture
            .wait_for(|frame| frame.contains("targeted unknown file view \"not-a-view\""))
            .contains("alpha.ts")
    );
    assert_eq!(fixture.app.selected_extension_file_view(&file_id), None);

    fixture.press(KeyCode::F(3));
    assert!(
        fixture
            .wait_for(|frame| frame.contains("file view \"plain\" has no interactive mode"))
            .contains("alpha.ts")
    );
    assert_eq!(fixture.app.selected_extension_file_view(&file_id), None);
}

#[test]
fn mode_handoffs_order_lifecycle_and_an_exiting_handler_cannot_retire_its_successor() {
    let mut fixture = Fixture::launch(changeset("initial", false));
    fixture.press(KeyCode::F(9));
    fixture.press(KeyCode::F(7));
    assert_eq!(
        fixture.app.active_keyboard_mode_title().as_deref(),
        Some("beta")
    );
    let handoff = fixture.wait_for(|frame| frame.contains("ENTER beta alpha.ts"));
    assert!(
        handoff.find("EXIT alpha alpha.ts").unwrap() < handoff.find("ENTER beta alpha.ts").unwrap()
    );
    fixture.press(KeyCode::Char('n'));
    let beta_key = fixture.wait_for(|frame| frame.contains("KEY beta n"));
    assert!(!beta_key.contains("COMMAND N RAN"));

    let mut replacement = Fixture::launch(changeset("initial", false));
    replacement.press(KeyCode::F(9));
    replacement.press(KeyCode::Char('r'));
    assert_eq!(
        replacement.app.active_keyboard_mode_title().as_deref(),
        Some("beta")
    );
    let frame = replacement.wait_for(|frame| frame.contains("ENTER beta alpha.ts"));
    assert!(frame.contains("KEY alpha r"));
    assert!(!frame.contains("EXIT beta alpha.ts"));
}

#[test]
fn deferred_pre_reload_command_enters_against_and_remains_owned_by_the_new_review() {
    let mut fixture = Fixture::launch(changeset("initial", false));
    fixture.press(KeyCode::F(2));
    assert!(fixture.draw().contains("Ready to edit?"));

    fixture.app.reload(changeset("reloaded", false));
    let entered = fixture.wait_for(|frame| {
        frame.contains("ALPHA VIEW")
            && frame.contains("FILE RELOADED true")
            && frame.contains("ENTER alpha alpha.ts RELOADED true")
    });
    assert!(entered.contains("DIALOG ANSWER false"));
    assert!(entered.contains("LATE RESULT true"));
    assert!(entered.contains("mode — Esc exits"));
    assert_eq!(
        fixture.app.active_keyboard_mode_title().as_deref(),
        Some("alpha")
    );
    for _ in 0..5 {
        assert!(fixture.draw().contains("mode — Esc exits"));
    }
}

#[test]
fn second_escape_in_one_flush_reaches_commands_after_the_first_key_exits_the_mode() {
    let mut fixture = Fixture::launch(changeset("initial", false));
    fixture.press(KeyCode::F(9));
    fixture.press_unsettled(KeyCode::Char('x'));
    fixture.press_unsettled(KeyCode::Esc);
    fixture.settle();

    let frame = fixture.wait_for(|frame| {
        frame.contains("ESCAPE COMMAND RAN") && frame.contains("EXIT alpha alpha.ts")
    });
    assert!(frame.contains("EXIT alpha alpha.ts"), "{frame}");
    assert!(!frame.contains("KEY alpha escape"));
    assert!(fixture.app.active_keyboard_mode_title().is_none());
}

#[test]
fn throwing_mode_key_warns_exits_and_leaves_the_selected_view_and_review_usable() {
    let mut fixture = Fixture::launch(changeset("initial", false));
    fixture.press(KeyCode::F(1));
    assert!(
        fixture
            .wait_for(|frame| frame.contains("BOOM VIEW"))
            .contains("mode — Esc exits")
    );

    fixture.press(KeyCode::Char('j'));
    assert!(fixture.app.active_keyboard_mode_title().is_none());
    assert_eq!(
        fixture
            .app
            .active_extension_notification()
            .map(|notice| notice.message),
        Some(
            "Extension app-host-file-view-modes-probe file view \"boom\" mode failed onKey • key handler exploded"
                .into()
        )
    );
    let failed = fixture.wait_for(|frame| frame.contains("BOOM EXIT alpha.ts"));
    assert!(failed.contains("BOOM VIEW"));

    fixture.press(KeyCode::Char('p'));
    assert!(
        fixture
            .wait_for(|frame| frame.contains("COMMAND P RAN"))
            .contains("BOOM VIEW")
    );
}
