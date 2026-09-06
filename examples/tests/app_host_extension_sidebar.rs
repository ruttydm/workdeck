use std::fs;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, UserKeyBinding, UserKeyBindingEntry};
use workdeck_diff::parse_patch;
use workdeck_extension_api::ExtensionNotificationHub;
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{CursorLineMode, ReviewApp, ReviewOptions, render};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-extension-sidebar.json"
    ))
    .unwrap()
}

fn changed_files(count: usize) -> Changeset {
    let mut patch = String::new();
    for index in 0..count {
        let path = if count == 2 {
            ["alpha.txt", "beta.txt"][index].to_owned()
        } else {
            format!("file-{index:02}.txt")
        };
        patch.push_str(&format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1,2 @@\n one\n+two\n"
        ));
    }
    parse_patch(
        &patch,
        "changeset:app-host-extension-sidebar",
        "AppHost extension sidebar",
        ChangesetSource::WorkingTree { staged: false },
    )
    .unwrap()
}

struct Fixture {
    _extension_directory: TempDir,
    app: ReviewApp,
    terminal: Terminal<TestBackend>,
}

impl Fixture {
    fn launch(
        scenario: &str,
        changeset: Changeset,
        width: u16,
        configure: impl FnOnce(&mut ReviewOptions),
    ) -> Self {
        let extension_directory = TempDir::new().unwrap();
        let binary_directory = extension_directory.path().join("bin");
        fs::create_dir_all(&binary_directory).unwrap();
        let binary_name = format!(
            "workdeck-example-app-host-extension-sidebar-probe-extension{}",
            std::env::consts::EXE_SUFFIX
        );
        fs::copy(
            env!("CARGO_BIN_EXE_workdeck-example-app-host-extension-sidebar-probe-extension"),
            binary_directory.join(binary_name),
        )
        .unwrap();
        let manifest = extension_directory.path().join("workdeck-extension.toml");
        fs::write(
            &manifest,
            include_str!("../extensions/app-host-extension-sidebar-probe/workdeck-extension.toml"),
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
        let mut options = ReviewOptions {
            extension_notifications: Some(notifications),
            highlight: false,
            prompt_save_view_preferences: false,
            ..ReviewOptions::default()
        };
        configure(&mut options);
        Self {
            _extension_directory: extension_directory,
            app: ReviewApp::new_with_extensions(changeset, options, vec![extension]),
            terminal: Terminal::new(TestBackend::new(width, 30)).unwrap(),
        }
    }

    fn standard(scenario: &str) -> Self {
        Self::launch(scenario, changed_files(2), 240, |_| {})
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
        self.app.poll_extension_commands();
    }

    fn settle_selection_events(&mut self) {
        self.app
            .tick_extension_notifications(Instant::now() + Duration::from_secs(1));
        self.settle();
    }

    fn press(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        self.settle();
    }

    fn press_modified(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        self.app.handle_key(KeyEvent::new(code, modifiers));
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

    fn notice(&self) -> String {
        self.app
            .active_extension_notification()
            .map(|notice| notice.message)
            .unwrap_or_default()
    }

    fn expire_notice(&mut self) {
        let now = Instant::now();
        self.app.tick_extension_notifications(now);
        self.app
            .tick_extension_notifications(now + Duration::from_secs(10));
    }

    fn selected_path(&self) -> String {
        let state = self.app.shared_state();
        let state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.selected_file().unwrap().path.clone()
    }
}

#[test]
fn frozen_app_host_extension_sidebar_oracle_maps_both_pins_and_all_source_tests() {
    let oracle = oracle();
    assert_eq!(
        oracle["source"]["baseline"]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["source"]["stable"]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 15);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 14);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 15);
    assert_eq!(
        oracle["baselineOnly"][0],
        "the files toggle closes a built-in fallback injected after availability failure"
    );
}

#[test]
fn startup_event_opens_a_pane_after_native_controls_are_mounted() {
    let mut fixture = Fixture::standard("startup");
    fixture.settle();
    let frame = fixture.draw();
    assert!(frame.contains("MOUNTED STARTUP SIDEBAR"));
    assert!(
        fixture.notice().is_empty(),
        "startup targeted an unmounted control: {:?}",
        fixture.notice()
    );
}

#[test]
fn command_key_opens_an_additive_native_pane_and_its_action_publishes_selection() {
    let mut fixture = Fixture::standard("extra");
    let initial = fixture.draw();
    assert!(initial.contains("M alpha.txt"));
    assert!(!initial.contains("EXTSIDEBAR"));

    fixture.press(KeyCode::Char('y'));
    fixture.settle_selection_events();
    let opened = fixture.draw();
    assert!(opened.contains("EXTSIDEBAR files=2"));
    assert!(opened.contains("M alpha.txt"));
    assert_eq!(fixture.selected_path(), "beta.txt");
    assert!(
        fixture.notice().contains("selection-event"),
        "notice={:?}",
        fixture.notice()
    );

    fixture.press(KeyCode::Char('y'));
    assert!(!fixture.draw().contains("EXTSIDEBAR"));
}

#[test]
fn pane_receives_effective_remapped_keys_and_excludes_a_conflicting_extension_chord() {
    let mut fixture = Fixture::launch("keys", changed_files(2), 240, |options| {
        options.keybindings = vec![UserKeyBindingEntry::new(
            "workdeck.review.nextFile",
            UserKeyBinding::Chord("ctrl+n".into()),
        )];
    });
    let frame = fixture.draw();
    assert!(frame.contains("EXTKEYS ctrl+n matched=true"));
    assert!(frame.contains("BLOCKED none"));
}

#[test]
fn command_selection_is_coherent_current_and_frozen_at_invocation_time() {
    let mut fixture = Fixture::standard("selection");
    fixture.draw();
    fixture.press(KeyCode::Char('y'));
    assert!(
        fixture
            .notice()
            .contains("selection alpha.txt#0 line=new:1")
    );
    fixture.expire_notice();

    fixture
        .app
        .handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    fixture
        .app
        .handle_key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    fixture.settle();
    assert!(fixture.notice().ends_with("delayed new:1"));
    fixture.expire_notice();

    fixture.press(KeyCode::Char('y'));
    assert!(
        fixture
            .notice()
            .contains("selection alpha.txt#0 line=new:2")
    );
    fixture.expire_notice();
    fixture.press(KeyCode::Char('j'));
    fixture.press(KeyCode::Char('y'));
    assert!(fixture.notice().contains("selection beta.txt#0 line=new:1"));
}

#[test]
fn command_review_snapshot_contains_saved_note_stable_files_and_anchor() {
    let mut fixture = Fixture::standard("review-snapshot");
    fixture.draw();
    fixture.press(KeyCode::Char('c'));
    fixture.type_text("Export this saved note.");
    fixture.press_modified(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert!(fixture.draw().contains("Your note"));

    fixture.press(KeyCode::Char('y'));
    let notice = fixture.notice();
    assert!(notice.contains("generation=generation:workdeck-tui:"));
    assert!(!notice.contains("revision=0 "));
    assert!(notice.contains("paths=alpha.txt|beta.txt"));
    assert!(notice.contains("identities-distinct=true"));
    assert!(notice.contains("notes=1"));
    assert!(notice.contains("summaries=Export this saved note."));
    assert!(notice.contains("detail=user:true:export this saved note.:new:1"));
}

#[test]
fn disabled_current_line_marker_reaches_commands_as_null() {
    let mut fixture = Fixture::launch("line-off", changed_files(2), 240, |options| {
        options.cursor_line = CursorLineMode::Off;
    });
    fixture.draw();
    fixture.press(KeyCode::Char('y'));
    assert!(fixture.notice().ends_with("line-null true"));
}

#[test]
fn files_and_additive_extension_panes_toggle_independently() {
    let mut fixture = Fixture::standard("independent");
    assert!(fixture.draw().contains("M alpha.txt"));
    fixture.press(KeyCode::Char('s'));
    assert!(!fixture.draw().contains("M alpha.txt"));
    fixture.press(KeyCode::Char('y'));
    let extension_only = fixture.draw();
    assert!(extension_only.contains("EXTSIDEBAR"));
    assert!(!extension_only.contains("M alpha.txt"));
    fixture.press(KeyCode::Char('s'));
    let both = fixture.draw();
    assert!(both.contains("EXTSIDEBAR"));
    assert!(both.contains("M alpha.txt"));
    fixture.press(KeyCode::Char('s'));
    let extension_only_again = fixture.draw();
    assert!(extension_only_again.contains("EXTSIDEBAR"));
    assert!(!extension_only_again.contains("M alpha.txt"));
}

#[test]
fn extensions_menu_lists_and_invokes_an_unbound_native_command() {
    let mut fixture = Fixture::standard("menu");
    assert!(fixture.draw().contains("Extensions"));
    fixture.press(KeyCode::F(10));
    assert!(fixture.draw().contains("Toggle files/filter focus"));
    for _ in 0..4 {
        fixture.press(KeyCode::Right);
    }
    assert!(fixture.draw().contains("Open the probe pane"));
    fixture.press(KeyCode::Enter);
    assert!(fixture.draw().contains("EXTSIDEBAR"));
}

#[test]
fn files_replacement_stands_in_for_the_built_in_navigation() {
    let mut fixture = Fixture::standard("replacement");
    let frame = fixture.draw();
    assert!(
        frame.contains("REPLACEMENT SIDEBAR"),
        "notice={:?}\n{frame}",
        fixture.notice()
    );
    assert!(!frame.contains("M alpha.txt"));
}

#[test]
fn first_registered_replacement_owns_the_named_files_slot_across_toggles() {
    let mut fixture = Fixture::standard("slot-owner");
    let initial = fixture.draw();
    assert!(initial.contains("FIRST FILES PANE"));
    assert!(!initial.contains("SECOND FILES PANE"));
    fixture.press(KeyCode::Char('s'));
    let closed = fixture.draw();
    assert!(!closed.contains("FIRST FILES PANE"));
    assert!(!closed.contains("SECOND FILES PANE"));
    fixture.press(KeyCode::Char('s'));
    let reopened = fixture.draw();
    assert!(reopened.contains("FIRST FILES PANE"), "{reopened}");
    assert!(!reopened.contains("SECOND FILES PANE"));
}

#[test]
fn view_menu_checkbox_tracks_the_files_slot_not_an_independent_pane() {
    let mut fixture = Fixture::standard("menu-slot");
    let initial = fixture.draw();
    assert!(initial.contains("FILES SLOT OWNER"));
    assert!(initial.contains("INDEPENDENT PANE"));
    fixture.press(KeyCode::Char('s'));
    let closed = fixture.draw();
    assert!(!closed.contains("FILES SLOT OWNER"));
    assert!(closed.contains("INDEPENDENT PANE"));
    fixture.press(KeyCode::F(10));
    fixture.press(KeyCode::Right);
    let view_menu = fixture.draw();
    assert!(view_menu.contains("[ ] Files pane"), "{view_menu}");
    assert!(!view_menu.contains("[x] Files pane"));
}

#[test]
fn files_command_toggles_a_bottom_edge_replacement_at_medium_width() {
    let mut fixture = Fixture::launch("bottom-slot", changed_files(2), 180, |_| {});
    assert!(fixture.draw().contains("BOTTOM FILES SLOT"));
    fixture.press(KeyCode::Char('s'));
    assert!(!fixture.draw().contains("BOTTOM FILES SLOT"));
    fixture.press(KeyCode::Char('s'));
    assert!(fixture.draw().contains("BOTTOM FILES SLOT"));
}

#[test]
fn crashing_replacement_is_quarantined_and_files_fallback_remains_toggleable() {
    let mut fixture = Fixture::standard("crash");
    fixture.draw();
    let frame = fixture.draw();
    assert!(fixture.notice().contains("failed rendering"));
    assert!(fixture.notice().contains("sidebar exploded"));
    assert!(frame.contains("M alpha.txt"));
    fixture.press(KeyCode::Char('s'));
    assert!(!fixture.draw().contains("M alpha.txt"));
    fixture.press(KeyCode::Char('s'));
    assert!(fixture.draw().contains("M alpha.txt"));
}

#[test]
fn availability_failure_injects_a_files_fallback_that_the_files_command_closes() {
    let mut fixture = Fixture::standard("availability-failure");
    let frame = fixture.draw();
    assert!(fixture.notice().contains("availability failed"));
    assert!(fixture.notice().contains("availability exploded"));
    assert!(frame.contains("M alpha.txt"));
    assert!(!frame.contains("BROKEN FILES"));
    fixture.press(KeyCode::Char('s'));
    assert!(!fixture.draw().contains("M alpha.txt"));
    fixture.press(KeyCode::Char('s'));
    assert!(fixture.draw().contains("M alpha.txt"));
}

#[test]
fn pane_availability_reacts_to_the_filtered_visible_file_projection() {
    let mut fixture = Fixture::standard("filtered-availability");
    assert!(fixture.draw().contains("TWO FILE PANE"));
    fixture.press(KeyCode::Char('/'));
    fixture.type_text("alpha");
    assert!(!fixture.draw().contains("TWO FILE PANE"));
}

#[test]
fn selected_declarative_list_row_scrolls_into_the_native_pane_viewport() {
    let mut fixture = Fixture::launch("scroll", changed_files(30), 240, |_| {});
    let initial = fixture.draw();
    assert!(initial.contains("ref:file-00.txt"));
    assert!(!initial.contains("ref:file-29.txt"));
    for _ in 0..29 {
        fixture.press(KeyCode::Char('.'));
        fixture.draw();
    }
    assert!(fixture.draw().contains("ref:file-29.txt"));
}
