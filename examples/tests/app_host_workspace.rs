use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{
    Changeset, ChangesetSource, CliInput, CommonOptions, FileSourceSnapshots, SourceOrigin,
    SourceSnapshot, VcsDiffCommandInput, VcsShowCommandInput,
};
use workdeck_diff::parse_patch;
use workdeck_extension_api::ExtensionNotificationHub;
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/app-host-workspace.json"
    ))
    .unwrap()
}

fn working_tree_input() -> CliInput {
    CliInput::Vcs(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    })
}

fn show_input() -> CliInput {
    CliInput::Show(VcsShowCommandInput {
        reference: Some("HEAD".into()),
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    })
}

fn working_tree_changeset(path: &str, rewritten: bool) -> Changeset {
    let (patch, new) = if rewritten {
        (
            format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-one\n+rewritten\n"
            ),
            "rewritten\n",
        )
    } else {
        (
            format!(
                "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1,2 @@\n one\n+two\n"
            ),
            "one\ntwo\n",
        )
    };
    let mut changeset = parse_patch(
        &patch,
        "changeset:app-host-workspace",
        "AppHost workspace",
        ChangesetSource::WorkingTree { staged: false },
    )
    .unwrap();
    changeset.files[0].set_sources(FileSourceSnapshots {
        old: Some(SourceSnapshot::new(
            "one\n".into(),
            SourceOrigin::Revision {
                revision: "HEAD".into(),
            },
            true,
        )),
        new: Some(SourceSnapshot::new(
            new.into(),
            SourceOrigin::WorkingTree,
            true,
        )),
    });
    changeset
}

fn revision_changeset() -> Changeset {
    let mut changeset = parse_patch(
        "diff --git a/alpha.txt b/alpha.txt\nnew file mode 100644\n--- /dev/null\n+++ b/alpha.txt\n@@ -0,0 +1 @@\n+one\n",
        "changeset:app-host-workspace-show",
        "AppHost workspace show",
        ChangesetSource::Revision {
            from: None,
            to: "HEAD".into(),
        },
    )
    .unwrap();
    changeset.files[0].set_sources(FileSourceSnapshots {
        old: None,
        new: Some(SourceSnapshot::new(
            "one\n".into(),
            SourceOrigin::Revision {
                revision: "HEAD".into(),
            },
            true,
        )),
    });
    changeset
}

fn reviewed_symlink_changeset() -> Changeset {
    let mut changeset = parse_patch(
        "diff --git a/linked.txt b/linked.txt\nnew file mode 120000\n--- /dev/null\n+++ b/linked.txt\n@@ -0,0 +1 @@\n+outside/secret.txt\n",
        "changeset:app-host-workspace-link",
        "AppHost workspace symlink",
        ChangesetSource::WorkingTree { staged: false },
    )
    .unwrap();
    changeset.files[0].set_sources(FileSourceSnapshots {
        old: None,
        new: Some(SourceSnapshot::new(
            "outside/secret.txt".into(),
            SourceOrigin::WorkingTree,
            true,
        )),
    });
    changeset
}

struct Fixture {
    _extension_directory: TempDir,
    app: ReviewApp,
    terminal: Terminal<TestBackend>,
}

impl Fixture {
    fn launch(scenario: &str, repository: &Path, changeset: Changeset, input: CliInput) -> Self {
        let extension_directory = TempDir::new().unwrap();
        let binary_directory = extension_directory.path().join("bin");
        fs::create_dir_all(&binary_directory).unwrap();
        let binary_name = format!(
            "workdeck-example-app-host-workspace-probe-extension{}",
            std::env::consts::EXE_SUFFIX
        );
        fs::copy(
            env!("CARGO_BIN_EXE_workdeck-example-app-host-workspace-probe-extension"),
            binary_directory.join(binary_name),
        )
        .unwrap();
        let manifest = extension_directory.path().join("workdeck-extension.toml");
        fs::write(
            &manifest,
            include_str!("../extensions/app-host-workspace-probe/workdeck-extension.toml"),
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
            _extension_directory: extension_directory,
            app: ReviewApp::new_with_extensions(
                changeset,
                ReviewOptions {
                    repo: Some(repository.to_owned()),
                    command_cwd: Some(repository.to_owned()),
                    review_input: Some(input),
                    extension_notifications: Some(notifications),
                    sidebar: false,
                    highlight: false,
                    prompt_save_view_preferences: false,
                    ..ReviewOptions::default()
                },
                vec![extension],
            ),
            terminal: Terminal::new(TestBackend::new(140, 30)).unwrap(),
        }
    }

    fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(4);
        while self.app.has_pending_extension_commands() || self.app.has_pending_extension_events() {
            self.app.poll_extension_commands();
            assert!(
                Instant::now() < deadline,
                "workspace request did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn press(&mut self, code: KeyCode) {
        self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        self.settle();
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

    fn advance_notice(&mut self) {
        let now = Instant::now();
        self.app.tick_extension_notifications(now);
        self.app
            .tick_extension_notifications(now + Duration::from_secs(10));
    }
}

#[test]
fn frozen_app_host_workspace_oracle_maps_both_pins_and_all_source_tests() {
    let oracle = oracle();
    assert_eq!(
        oracle["source"]["baseline"]["commit"],
        "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
    );
    assert_eq!(
        oracle["source"]["stable"]["commit"],
        "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
    );
    assert_ne!(
        oracle["source"]["baseline"]["blob"],
        oracle["source"]["stable"]["blob"]
    );
    assert_eq!(oracle["source"]["baseline"]["bytes"], 32_073);
    assert_eq!(oracle["source"]["baseline"]["lines"], 904);
    assert_eq!(oracle["oracleRuns"]["baseline"]["passed"], 13);
    assert_eq!(oracle["oracleRuns"]["stable"]["passed"], 13);
    assert_eq!(oracle["testMappings"].as_array().unwrap().len(), 13);
}

#[test]
fn deferred_read_resolves_null_after_the_review_generation_reloads() {
    let repository = TempDir::new().unwrap();
    fs::write(repository.path().join("alpha.txt"), "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "deferred-read",
        repository.path(),
        working_tree_changeset("alpha.txt", false),
        working_tree_input(),
    );
    fixture
        .app
        .handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
    assert!(fixture.app.has_pending_extension_commands());
    fixture
        .app
        .reload(working_tree_changeset("alpha.txt", false));
    fixture.settle();
    assert_eq!(fixture.notice(), "app-host-workspace-probe: read null");
}

#[test]
fn working_tree_read_returns_both_sides_unknown_null_and_a_true_write_probe() {
    let repository = TempDir::new().unwrap();
    fs::write(repository.path().join("alpha.txt"), "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "read",
        repository.path(),
        working_tree_changeset("alpha.txt", false),
        working_tree_input(),
    );
    fixture.press(KeyCode::Char('y'));
    assert_eq!(
        fixture.notice(),
        "app-host-workspace-probe: reads new=\"one\\ntwo\\n\" old=\"one\\n\" unknown=null can=true"
    );
    assert!(!fixture.draw().contains("Write alpha.txt?"));
}

#[test]
fn revision_read_returns_the_commit_side_but_refuses_workspace_writes() {
    let repository = TempDir::new().unwrap();
    fs::write(repository.path().join("alpha.txt"), "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "read",
        repository.path(),
        revision_changeset(),
        show_input(),
    );
    fixture.press(KeyCode::Char('y'));
    assert_eq!(
        fixture.notice(),
        "app-host-workspace-probe: reads new=\"one\\n\" old=null unknown=null can=false"
    );
}

#[test]
fn read_transform_write_replaces_the_whole_document_with_consent() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "read-write",
        repository.path(),
        working_tree_changeset("alpha.txt", false),
        working_tree_input(),
    );
    fixture.press(KeyCode::Char('y'));
    assert!(fixture.draw().contains("Write alpha.txt?"));
    fixture.press(KeyCode::Enter);
    assert_eq!(fs::read_to_string(path).unwrap(), "ONE\nTWO\n");
    assert_eq!(
        fixture.notice(),
        "app-host-workspace-probe: result {\"kind\":\"written\"}"
    );
}

#[test]
fn confirmed_write_is_attributed_replaces_the_file_and_requests_review_reload() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "write",
        repository.path(),
        working_tree_changeset("alpha.txt", false),
        working_tree_input(),
    );
    fixture.press(KeyCode::Char('y'));
    let prompt = fixture.draw();
    assert!(prompt.contains("ext app-host-workspace-probe"));
    assert!(prompt.contains("Write alpha.txt?"));
    assert!(prompt.contains("replace this file's contents on disk"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "one\ntwo\n");

    fixture.press(KeyCode::Enter);
    assert_eq!(fs::read_to_string(&path).unwrap(), "rewritten\n");
    assert!(fixture.app.take_reload_requested());
    fixture.advance_notice();
    assert_eq!(
        fixture.notice(),
        "app-host-workspace-probe: result {\"kind\":\"written\"}"
    );
    fixture
        .app
        .reload(working_tree_changeset("alpha.txt", true));
    assert!(fixture.draw().contains("rewritten"));
}

#[test]
fn disappearing_target_after_consent_is_refused_without_recreation() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "write",
        repository.path(),
        working_tree_changeset("alpha.txt", false),
        working_tree_input(),
    );
    fixture.press(KeyCode::Char('y'));
    fs::remove_file(&path).unwrap();
    fixture.press(KeyCode::Enter);
    fixture.advance_notice();
    assert!(!path.exists());
    assert!(fixture.notice().contains("no longer in the working tree"));
}

#[cfg(unix)]
#[test]
fn symlink_swap_after_consent_is_refused_without_following_the_link() {
    use std::os::unix::fs::symlink;

    let repository = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let path = repository.path().join("alpha.txt");
    let secret = outside.path().join("secret.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    fs::write(&secret, "secret\n").unwrap();
    let mut fixture = Fixture::launch(
        "write",
        repository.path(),
        working_tree_changeset("alpha.txt", false),
        working_tree_input(),
    );
    fixture.press(KeyCode::Char('y'));
    fs::remove_file(&path).unwrap();
    symlink(&secret, &path).unwrap();
    fixture.press(KeyCode::Enter);
    fixture.advance_notice();
    assert!(fixture.notice().contains("is a symlink"));
    assert_eq!(fs::read_to_string(secret).unwrap(), "secret\n");
}

#[test]
fn declined_write_reports_cancelled_and_leaves_the_file_untouched() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "write",
        repository.path(),
        working_tree_changeset("alpha.txt", false),
        working_tree_input(),
    );
    fixture.press(KeyCode::Char('y'));
    fixture.press(KeyCode::Esc);
    fixture.advance_notice();
    assert_eq!(fs::read_to_string(path).unwrap(), "one\ntwo\n");
    assert!(fixture.notice().contains("\"kind\":\"cancelled\""));
    assert!(fixture.notice().contains("write to alpha.txt was declined"));
}

#[cfg(unix)]
#[test]
fn reviewed_symlink_is_refused_before_consent_without_following_the_link() {
    use std::os::unix::fs::symlink;

    let repository = TempDir::new().unwrap();
    let outside = TempDir::new().unwrap();
    let secret = outside.path().join("secret.txt");
    fs::write(&secret, "secret\n").unwrap();
    symlink(&secret, repository.path().join("linked.txt")).unwrap();
    let mut fixture = Fixture::launch(
        "write",
        repository.path(),
        reviewed_symlink_changeset(),
        working_tree_input(),
    );
    fixture.press(KeyCode::Char('y'));
    assert!(!fixture.app.has_extension_dialog());
    assert!(!fixture.draw().contains("Write linked.txt?"));
    assert_eq!(fixture.notice(), "app-host-workspace-probe: can true");
    fixture.advance_notice();
    assert!(fixture.notice().contains("is a symlink"));
    assert_eq!(fs::read_to_string(secret).unwrap(), "secret\n");
}

#[test]
fn revision_write_is_refused_before_consent_and_preserves_the_working_tree() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.txt");
    fs::write(&path, "one\ntwo\n").unwrap();
    let mut fixture = Fixture::launch(
        "write",
        repository.path(),
        revision_changeset(),
        show_input(),
    );
    fixture.press(KeyCode::Char('y'));
    assert!(!fixture.app.has_extension_dialog());
    assert!(!fixture.draw().contains("Write alpha.txt?"));
    assert_eq!(fixture.notice(), "app-host-workspace-probe: can false");
    fixture.advance_notice();
    assert!(fixture.notice().contains("working-tree only"));
    assert_eq!(fs::read_to_string(path).unwrap(), "one\ntwo\n");
}
