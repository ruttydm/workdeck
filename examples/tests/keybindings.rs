use std::fs;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, UserKeyBinding, UserKeyBindingEntry};
use workdeck_diff::parse_patch;
use workdeck_extension_api::ExtensionNotificationHub;
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

const WIDTH: u16 = 120;
const HEIGHT: u16 = 24;

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-keybindings.json"
    ))
    .unwrap()
}

fn changeset() -> Changeset {
    parse_patch(
        "diff --git a/alpha.txt b/alpha.txt\n--- a/alpha.txt\n+++ b/alpha.txt\n@@ -1 +1,2 @@\n one\n+two\n",
        "keybindings",
        "Keybindings",
        ChangesetSource::Patch {
            label: "keybindings".into(),
        },
    )
    .unwrap()
}

fn binding(command_id: &str, binding: UserKeyBinding) -> UserKeyBindingEntry {
    UserKeyBindingEntry::new(command_id, binding)
}

fn press(app: &mut ReviewApp, code: KeyCode, modifiers: KeyModifiers) {
    app.handle_key(KeyEvent::new(code, modifiers));
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

struct ExtensionFixture {
    _directory: TempDir,
    log_path: std::path::PathBuf,
    app: Option<ReviewApp>,
}

impl ExtensionFixture {
    fn launch(
        keybindings: Vec<UserKeyBindingEntry>,
        external_quit_signal: Option<Arc<AtomicBool>>,
    ) -> Self {
        let directory = TempDir::new().unwrap();
        let binary_directory = directory.path().join("bin");
        fs::create_dir_all(&binary_directory).unwrap();
        let binary_name = format!(
            "workdeck-example-keybindings-probe-extension{}",
            std::env::consts::EXE_SUFFIX
        );
        fs::copy(
            env!("CARGO_BIN_EXE_workdeck-example-keybindings-probe-extension"),
            binary_directory.join(binary_name),
        )
        .unwrap();
        let manifest = directory.path().join("workdeck-extension.toml");
        fs::write(
            &manifest,
            include_str!("../extensions/keybindings-probe/workdeck-extension.toml"),
        )
        .unwrap();
        let log_path = directory.path().join("events.log");
        let notifications = ExtensionNotificationHub::new();
        let extension = LoadedExtension::spawn_with_notifications_and_configuration(
            &manifest,
            "test",
            notifications.clone(),
            serde_json::json!({ "logPath": log_path }),
        )
        .unwrap();
        let mut app = ReviewApp::new_with_extensions(
            changeset(),
            ReviewOptions {
                keybindings,
                external_quit_signal,
                extension_notifications: Some(notifications),
                ..ReviewOptions::default()
            },
            vec![extension],
        );
        settle(&mut app);
        Self {
            _directory: directory,
            log_path,
            app: Some(app),
        }
    }

    fn app(&mut self) -> &mut ReviewApp {
        self.app.as_mut().unwrap()
    }

    fn lines(&self) -> Vec<String> {
        fs::read_to_string(&self.log_path)
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn clear_log(&self) {
        fs::write(&self.log_path, "").unwrap();
    }

    fn retire(&mut self) {
        drop(self.app.take());
    }
}

#[test]
fn frozen_app_host_keybindings_oracle_maps_both_pins_and_all_tests() {
    let oracle = oracle();
    assert_eq!(
        oracle["source"]["baseline"]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["source"]["stable"]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_eq!(oracle["source"]["baseline"]["bytes"], 12_761);
    assert_eq!(oracle["source"]["stable"]["bytes"], 12_761);
    assert_eq!(oracle["oracleRuns"]["baseline"]["fullFile"]["passed"], 10);
    assert_eq!(oracle["oracleRuns"]["baseline"]["fullFile"]["failed"], 1);
    assert_eq!(oracle["oracleRuns"]["stable"]["fullFile"]["passed"], 10);
    assert_eq!(oracle["oracleRuns"]["stable"]["fullFile"]["failed"], 1);
    assert_eq!(
        oracle["oracleRuns"]["baseline"]["isolatedThemeCase"]["passed"],
        1
    );
    assert_eq!(
        oracle["oracleRuns"]["stable"]["isolatedThemeCase"]["passed"],
        1
    );
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 11);
}

#[test]
fn config_table_rebinds_quit_and_releases_the_default_key() {
    let mut app = ReviewApp::new(
        changeset(),
        ReviewOptions {
            keybindings: vec![binding(
                "workdeck.app.quit",
                UserKeyBinding::Chord("ctrl+x".into()),
            )],
            ..ReviewOptions::default()
        },
    );
    press(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(!app.take_quit_requested());
    press(&mut app, KeyCode::Char('x'), KeyModifiers::CONTROL);
    assert!(app.take_quit_requested());
}

#[test]
fn user_claimed_chord_is_removed_from_the_previous_default_owner() {
    let mut app = ReviewApp::new(
        changeset(),
        ReviewOptions {
            keybindings: vec![binding(
                "workdeck.review.focusFilter",
                UserKeyBinding::Chords(vec!["q".into(), "/".into()]),
            )],
            ..ReviewOptions::default()
        },
    );
    press(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(!app.take_quit_requested());
}

#[test]
fn disabled_quit_binding_leaves_the_default_key_inert() {
    let mut app = ReviewApp::new(
        changeset(),
        ReviewOptions {
            keybindings: vec![binding("workdeck.app.quit", UserKeyBinding::Disabled)],
            ..ReviewOptions::default()
        },
    );
    press(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(!app.take_quit_requested());
}

#[test]
fn default_quit_binding_remains_active_without_configuration() {
    let mut app = ReviewApp::new(changeset(), ReviewOptions::default());
    press(&mut app, KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(app.take_quit_requested());
}

#[test]
fn legacy_sidebar_alias_remaps_and_emits_the_canonical_files_pane_command() {
    let mut fixture = ExtensionFixture::launch(
        vec![binding(
            "workdeck.view.toggleSidebar",
            UserKeyBinding::Chord("f6".into()),
        )],
        None,
    );
    let initial_sidebar = fixture.app().options().sidebar;
    press(fixture.app(), KeyCode::Char('s'), KeyModifiers::NONE);
    assert_eq!(fixture.app().options().sidebar, initial_sidebar);
    assert!(fixture.lines().is_empty());

    press(fixture.app(), KeyCode::F(6), KeyModifiers::NONE);
    assert_ne!(fixture.app().options().sidebar, initial_sidebar);
    assert_eq!(fixture.lines(), ["command:workdeck.view.toggleFilesPane"]);
}

#[test]
fn theme_selector_honors_configured_control_vertical_bindings() {
    let mut app = ReviewApp::new(
        changeset(),
        ReviewOptions {
            keybindings: vec![
                binding(
                    "workdeck.review.stepDown",
                    UserKeyBinding::Chords(vec!["down".into(), "j".into(), "ctrl+n".into()]),
                ),
                binding(
                    "workdeck.review.stepUp",
                    UserKeyBinding::Chords(vec!["up".into(), "k".into(), "ctrl+p".into()]),
                ),
            ],
            ..ReviewOptions::default()
        },
    );
    press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
    assert!(frame(&app).contains("Theme selector"));
    press(&mut app, KeyCode::Char('n'), KeyModifiers::CONTROL);
    let rendered = frame(&app);
    assert!(rendered.contains("›  github-dark-dimmed"), "{rendered}");
    press(&mut app, KeyCode::Char('p'), KeyModifiers::CONTROL);
    let rendered = frame(&app);
    assert!(rendered.contains("›  github-dark-default"), "{rendered}");
}

#[test]
fn keyboard_dispatch_emits_the_step_down_command_event() {
    let mut fixture = ExtensionFixture::launch(Vec::new(), None);
    press(fixture.app(), KeyCode::Char('j'), KeyModifiers::NONE);
    assert_eq!(fixture.lines(), ["command:workdeck.review.stepDown"]);
}

#[test]
fn programmatic_command_event_precedes_extension_command_exactly_once() {
    let mut fixture = ExtensionFixture::launch(Vec::new(), None);
    assert!(fixture.app().options().line_numbers);
    press(fixture.app(), KeyCode::Char('y'), KeyModifiers::NONE);
    assert!(
        !fixture.app().options().line_numbers,
        "frame: {}\nlog: {:?}",
        frame(fixture.app()),
        fixture.lines()
    );
    assert_eq!(
        fixture.lines(),
        [
            "command:workdeck.view.toggleLineNumbers",
            "command:coach.toggle-lines",
        ]
    );
}

#[test]
fn quit_command_event_precedes_the_single_shutdown_event() {
    let mut fixture = ExtensionFixture::launch(Vec::new(), None);
    press(fixture.app(), KeyCode::Char('q'), KeyModifiers::NONE);
    assert!(fixture.app().take_quit_requested());
    fixture.retire();
    assert_eq!(fixture.lines(), ["command:workdeck.app.quit", "shutdown"]);
}

#[test]
fn external_quit_retires_extensions_before_reporting_quit() {
    let signal = Arc::new(AtomicBool::new(false));
    let mut fixture = ExtensionFixture::launch(Vec::new(), Some(Arc::clone(&signal)));
    signal.store(true, Ordering::Release);
    assert!(fixture.app().process_external_quit_signal());
    assert!(fixture.app().take_quit_requested());
    assert_eq!(fixture.lines(), ["shutdown"]);
    fixture.retire();
    assert_eq!(fixture.lines(), ["shutdown"]);
}

#[test]
fn tab_leaving_the_focused_filter_emits_toggle_focus_area() {
    let mut fixture = ExtensionFixture::launch(Vec::new(), None);
    press(fixture.app(), KeyCode::Tab, KeyModifiers::NONE);
    fixture.clear_log();
    press(fixture.app(), KeyCode::Tab, KeyModifiers::NONE);
    assert_eq!(fixture.lines(), ["command:workdeck.app.toggleFocusArea"]);
}
