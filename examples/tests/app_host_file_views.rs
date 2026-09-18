use std::fs;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource};
use workdeck_diff::parse_patch;
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

const WIDTH: u16 = 120;
const HEIGHT: u16 = 30;

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-file-views.json"
    ))
    .unwrap()
}

fn changeset() -> Changeset {
    parse_patch(
        "diff --git a/alpha.ts b/alpha.ts\n--- a/alpha.ts\n+++ b/alpha.ts\n@@ -1 +1 @@\n-export const alpha = 1;\n+export const alpha = 2;\ndiff --git a/beta.ts b/beta.ts\n--- a/beta.ts\n+++ b/beta.ts\n@@ -1 +1 @@\n-export const beta = 1;\n+export const beta = 2;\ndiff --git a/notes.md b/notes.md\n--- a/notes.md\n+++ b/notes.md\n@@ -1 +1 @@\n-old notes\n+new notes\n",
        "changeset:app-host-file-views",
        "AppHost file views",
        ChangesetSource::Patch {
            label: "app-host-file-views".into(),
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
    fn launch(review: Changeset, width: u16) -> Self {
        let directory = TempDir::new().unwrap();
        let binary_directory = directory.path().join("bin");
        fs::create_dir_all(&binary_directory).unwrap();
        let binary_name = format!(
            "workdeck-example-app-host-file-views-probe-extension{}",
            std::env::consts::EXE_SUFFIX
        );
        fs::copy(
            env!("CARGO_BIN_EXE_workdeck-example-app-host-file-views-probe-extension"),
            binary_directory.join(binary_name),
        )
        .unwrap();
        let manifest = directory.path().join("workdeck-extension.toml");
        fs::write(
            &manifest,
            include_str!("../extensions/app-host-file-views-probe/workdeck-extension.toml"),
        )
        .unwrap();
        let extension = LoadedExtension::spawn(&manifest, "test").unwrap();
        Self {
            _directory: directory,
            app: ReviewApp::new_with_extensions(review, ReviewOptions::default(), vec![extension]),
            terminal: Terminal::new(TestBackend::new(width, HEIGHT)).unwrap(),
        }
    }

    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(3);
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
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            let frame = self.draw();
            if predicate(&frame) {
                return frame;
            }
            assert!(
                Instant::now() < deadline,
                "file-view frame did not settle:\n{frame}"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn focus_filter(&mut self, text: &str) {
        self.press(KeyCode::Tab);
        self.type_text(text);
        self.press(KeyCode::Tab);
    }

    fn clear_filter(&mut self) {
        self.press(KeyCode::Tab);
        self.press(KeyCode::Esc);
        self.press(KeyCode::Tab);
    }
}

#[test]
fn frozen_app_host_file_views_oracle_maps_identical_pins_and_all_source_tests() {
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
    assert_eq!(oracle["source"]["baseline"]["bytes"], 22_004);
    assert_eq!(oracle["source"]["baseline"]["lines"], 581);
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 6);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 6);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 6);
}

#[test]
fn stateful_view_relayouts_only_for_valid_view_wide_and_file_scoped_refreshes() {
    let mut review = changeset();
    review.files.truncate(1);
    let mut fixture = Fixture::launch(review, WIDTH);

    fixture.press(KeyCode::F(8));
    fixture.wait_for(|frame| frame.contains("STATE COLLAPSED"));
    fixture.press(KeyCode::F(9));
    fixture.wait_for(|frame| frame.contains("STATE EXPANDED"));
    fixture.press(KeyCode::F(6));
    fixture.wait_for(|frame| frame.contains("STATE EXPANDED MARKED"));

    fixture.press(KeyCode::F(7));
    let warned = fixture.wait_for(|frame| frame.contains("targeted unknown file view"));
    assert!(warned.contains("STATE EXPANDED MARKED"));
    assert!(warned.contains("not-a-view"));

    fixture.press(KeyCode::F(4));
    for _ in 0..12 {
        let unchanged = fixture.draw();
        assert!(unchanged.contains("STATE EXPANDED MARKED"));
        assert!(!unchanged.contains("PENDING"));
        std::thread::sleep(Duration::from_millis(2));
    }

    fixture.press(KeyCode::F(9));
    fixture.wait_for(|frame| frame.contains("STATE COLLAPSED MARKED PENDING"));
}

#[test]
fn hidden_file_scoped_refresh_survives_filtering_and_reappears_marked() {
    let mut review = changeset();
    review.files.truncate(2);
    let file_ids = review
        .files
        .iter()
        .map(|file| file.runtime_id.clone())
        .collect::<Vec<_>>();
    let mut fixture = Fixture::launch(review, 220);

    fixture.press(KeyCode::F(8));
    fixture.press(KeyCode::Char('.'));
    fixture.press(KeyCode::F(8));
    fixture.press(KeyCode::Char(','));
    fixture.wait_for(|frame| frame.matches("STATE COLLAPSED").count() == 2);
    assert_eq!(
        fixture
            .app
            .selected_extension_file_view(&file_ids[0])
            .as_deref(),
        Some("app-host-file-views-probe:stateful")
    );
    assert_eq!(
        fixture
            .app
            .selected_extension_file_view(&file_ids[1])
            .as_deref(),
        Some("app-host-file-views-probe:stateful")
    );

    fixture.focus_filter("alpha");
    let hidden = fixture.wait_for(|frame| !frame.contains("beta.ts"));
    assert!(hidden.contains("alpha.ts"));
    fixture.press(KeyCode::F(3));
    fixture.clear_filter();
    let restored = fixture.wait_for(|frame| frame.matches("STATE COLLAPSED").count() == 2);
    assert!(restored.contains("STATE COLLAPSED MARKED"));
}

#[test]
fn bulk_view_menu_applies_to_hidden_matching_files_but_not_nonmatches() {
    let review = changeset();
    let file_ids = review
        .files
        .iter()
        .map(|file| file.runtime_id.clone())
        .collect::<Vec<_>>();
    let mut fixture = Fixture::launch(review, WIDTH);

    fixture.focus_filter("alpha");
    fixture.press(KeyCode::F(2));
    fixture.wait_for(|frame| frame.contains("PREVIEW alpha.ts"));
    assert_eq!(
        fixture
            .app
            .selected_extension_file_view(&file_ids[0])
            .as_deref(),
        Some("app-host-file-views-probe:preview")
    );
    assert_eq!(fixture.app.selected_extension_file_view(&file_ids[1]), None);

    fixture.press(KeyCode::F(10));
    fixture.press(KeyCode::Right);
    let menu =
        fixture.wait_for(|frame| frame.contains("Apply \"Bulk preview\" to all matching files"));
    let (row, line) = menu
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains("Apply \"Bulk preview\" to all matching files"))
        .expect("bulk menu row is visible");
    let column = line.find("Apply \"Bulk preview\"").unwrap();
    fixture.app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: u16::try_from(column).unwrap(),
        row: u16::try_from(row).unwrap(),
        modifiers: KeyModifiers::NONE,
    });

    fixture.clear_filter();
    let expanded = fixture
        .wait_for(|frame| frame.contains("PREVIEW alpha.ts") && frame.contains("PREVIEW beta.ts"));
    assert!(!expanded.contains("PREVIEW notes.md"));
    assert_eq!(
        fixture
            .app
            .selected_extension_file_view(&file_ids[1])
            .as_deref(),
        Some("app-host-file-views-probe:preview")
    );
    assert_eq!(fixture.app.selected_extension_file_view(&file_ids[2]), None);
}
