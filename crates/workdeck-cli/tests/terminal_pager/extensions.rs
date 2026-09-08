//! Hunk MIT: all seventeen contracts in test/pty/extensions-integration.test.ts.
use super::*;
use std::path::Path;

fn launch(root: &Path, name: &str, rows: u16) -> (tempfile::TempDir, Session) {
    let extension_root = tempfile::tempdir().unwrap();
    let extension = super::file_views::example(extension_root.path(), name);
    let session = Session::launch_in(
        "",
        &[
            "diff",
            "--mode",
            "stack",
            "--extension",
            extension.to_str().unwrap(),
        ],
        false,
        140,
        rows,
        None,
        Some(root),
    );
    (extension_root, session)
}

#[test]
fn bundled_review_snapshot_exports_the_exact_saved_user_note() {
    let root = super::layout::two_files(false);
    let output_path = root.path().join("review-snapshot.json");
    let (_extension_root, mut session) = launch(root.path(), "review-snapshot-export", 30);
    session.wait(|text| text.contains("alpha.ts"));
    session.write(b"c");
    session.wait(|text| text.contains("Draft note"));
    session.write(b"Publish this exact note.");
    session.write(b"\x13");
    session.wait(|text| text.contains("Your note"));
    session.write(b"\x1b[20~");
    session.wait(|text| {
        text.contains("Export review snapshot") && text.contains("workdeck-review-snapshot.json")
    });
    session.write(output_path.to_str().unwrap().as_bytes());
    session.write(b"\r");
    session.wait(|text| text.contains("Exported 1 saved note"));
    let snapshot: serde_json::Value =
        serde_json::from_slice(&fs::read(output_path).unwrap()).unwrap();
    assert!(
        snapshot["generation"]
            .as_str()
            .unwrap()
            .starts_with("generation:")
    );
    assert!(snapshot["stateRevision"].as_u64().unwrap() > 0);
    let file = snapshot["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|file| file["path"] == "alpha.ts")
        .unwrap();
    let notes = snapshot["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0]["source"], "user");
    assert_eq!(notes[0]["fileKey"], file["fileKey"]);
    assert_eq!(notes[0]["summary"], "Publish this exact note.");
    drop(session);
}

#[test]
fn bundled_triage_registers_all_five_menu_commands() {
    let root = super::layout::two_files(false);
    let (_extension_root, mut session) = launch(root.path(), "review-triage", 30);
    let before = session.wait(|text| text.contains("alpha.ts") && text.contains("Extensions"));
    assert!(!before.contains("Review triage (session only)"));
    session.click_label("Extensions");
    let menu = session.wait(|text| text.contains("Clear triage decisions"));
    assert!(
        menu.lines()
            .any(|line| line.contains("Toggle review triage") && line.contains('y'))
    );
    assert!(
        menu.lines()
            .any(|line| line.contains("Mark selected hunk…") && line.contains('x'))
    );
    assert!(menu.contains("Center current review line"));
    assert!(menu.contains("Set review focus…"));
    drop(session);
}

#[test]
fn bundled_note_navigator_inventories_and_reveals_the_exact_saved_note() {
    let numbered = |count, offset| {
        (1..=count)
            .map(|line| format!("export const line{line:02} = {};\n", line + offset))
            .collect::<String>()
    };
    let before = numbered(30, 0);
    let after = numbered(30, 100);
    let root = super::layout::repository(&[
        ("first.ts", &before, &after),
        (
            "second.ts",
            "export const shortLine1 = 1;\nexport const shortLine2 = 2;\nexport const shortLine3 = 3;\n",
            "export const shortLine1 = 10;\nexport const shortLine2 = 20;\nexport const shortLine3 = 30;\n",
        ),
    ]);
    let (_extension_root, mut session) = launch(root.path(), "review-note-navigator", 22);
    session.wait(|text| text.contains("first.ts"));
    session.write(b"\x1b[19~");
    session.wait(|text| text.contains("This review has no saved notes"));
    session.write(b"c");
    session.wait(|text| text.contains("Draft note"));
    session.write(b"Navigate to this exact note.");
    session.write(b"\x13");
    session.wait(|text| text.contains("Your note"));
    session.write(b"]");
    session
        .wait(|text| text.contains("second.ts") && !text.contains("Navigate to this exact note."));
    session.write(b"\x1b[19~");
    let picker = session.wait(|text| {
        text.contains("Navigate saved review note")
            && text.contains("[active]")
            && text.contains("Navigate to this exact note.")
    });
    assert!(picker.contains("first.ts"));
    assert!(picker.contains("(old)") || picker.contains("(new)"));
    session.write(b"\r");
    let revealed = session.wait(|text| {
        !text.contains("Navigate saved review note")
            && text.contains("first.ts")
            && text.contains("Your note")
    });
    assert!(revealed.contains("Navigate to this exact note."));
    drop(session);
}

#[test]
fn bundled_vim_routes_counts_alignment_dialogs_and_control_chords() {
    let numbered = |start, offset| {
        (start..start + 16)
            .map(|line| format!("export const line{line:02} = {};\n", line + offset))
            .collect::<String>()
    };
    let first = (numbered(1, 0), numbered(1, 100));
    let second = (numbered(17, 0), numbered(17, 100));
    let root = super::layout::repository(&[
        ("first.ts", &first.0, &first.1),
        ("second.ts", &second.0, &second.1),
    ]);
    let (_extension_root, mut session) = launch(root.path(), "vim-navigation", 24);
    let active = |text: &str| {
        text.lines()
            .any(|line| line.contains("Vim navigation") && line.contains("Esc exits"))
    };
    let row = |text: &str, needle: &str| {
        text.lines()
            .position(|line| line.contains(needle))
            .expect(needle)
    };
    session.wait(|text| text.contains("first.ts") && text.contains("Extensions"));
    session.write(b"\x1b[17~");
    session.wait(active);
    session.click_label("Extensions");
    session.wait(|text| text.contains("Exit Vim navigation"));
    session.click_label("Exit Vim navigation");
    session.wait(|text| !active(text));
    session.write(b"\x1b[17~");
    session.wait(active);
    session.write(b"c");
    let initial = session.wait(|text| text.contains("Draft note"));
    let initial_row = row(&initial, "Draft note");
    session.write(b"\x1b");
    session.wait(|text| !text.contains("Draft note"));
    session.write(b"10jc");
    let counted = session.wait(|text| text.contains("Draft note"));
    assert!(row(&counted, "Draft note") > initial_row);
    session.write(b"\x1b");
    session.wait(|text| !text.contains("Draft note"));
    session.write(b"zt");
    session.wait_for(Duration::from_millis(200), |_| false);
    let aligned = session.parser.terminal().plain_string();
    let top_row = row(&aligned, "export const line11 = 11;");
    session.write(b"zz");
    session.wait(|text| {
        text.contains("export const line11 = 11;")
            && row(text, "export const line11 = 11;") > top_row
    });
    session.write(b":");
    session.wait(|text| text.contains("Vim command (:)"));
    session.write(b"j-owned");
    session.wait(|text| text.contains("j-owned"));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("Vim command (:)") && active(text));
    for (command, needle, absent) in [
        ("bottom", "second.ts", "first.ts"),
        ("top", "export const line01 = 1;", "second.ts"),
    ] {
        session.write(b":");
        session.wait(|text| text.contains("Vim command (:)"));
        session.write(command.as_bytes());
        session.write(b"\r");
        session.wait(|text| {
            text.contains(needle)
                && !text.contains(absent)
                && !text.contains("Vim command (:)")
                && active(text)
        });
    }
    session.write(b"\x04");
    session.wait(|text| text.contains("first.ts") && !text.contains("export const line01 = 1;"));
    session.write(b"\x15");
    session.wait(|text| text.contains("export const line01 = 1;"));
    session.write(b"G");
    session.wait(|text| text.contains("second.ts") && !text.contains("first.ts"));
    session.write(b"gg");
    session.wait(|text| text.contains("first.ts") && !text.contains("second.ts"));
    session.wait(active);
    session.write(b"\x1b");
    session.wait(|text| !active(text));
    drop(session);
}

fn pane_fixture(kind: &str, cols: u16) -> (tempfile::TempDir, tempfile::TempDir, Session) {
    let root = super::layout::two_files(false);
    let extension_root = tempfile::tempdir().unwrap();
    let extension = super::file_views::example(extension_root.path(), "pty-extension-probe");
    fs::write(extension.join("fixture-kind"), kind).unwrap();
    let session = Session::launch_in(
        "",
        &[
            "diff",
            "--mode",
            "stack",
            "--extension",
            extension.to_str().unwrap(),
        ],
        false,
        cols,
        24,
        None,
        Some(root.path()),
    );
    (root, extension_root, session)
}

#[test]
fn extension_menu_dispatches_the_sidebar_command_by_mouse() {
    let (_root, _extension, mut session) = pane_fixture("sidebar", 240);
    let before = session.wait(|text| text.contains("alpha.ts") && text.contains("Extensions"));
    assert!(!before.contains("EXTSIDEBAR"));
    session.click_label("Extensions");
    session.wait(|text| {
        text.lines()
            .any(|line| line.contains("Toggle fixture") && line.contains('y'))
    });
    session.click_label("Toggle fixture");
    let opened = session.wait(|text| text.contains("EXTSIDEBAR 2 FILES"));
    assert!(opened.contains("alpha.ts"));
    drop(session);
}

#[test]
fn registered_key_toggles_a_sidebar_beside_the_builtin_files_pane() {
    let (_root, _extension, mut session) = pane_fixture("sidebar", 240);
    let before = session.wait(|text| text.contains("alpha.ts"));
    assert!(!before.contains("EXTSIDEBAR"));
    session.write(b"y");
    let opened = session.wait(|text| text.contains("EXTSIDEBAR 2 FILES"));
    assert!(opened.contains("alpha.ts"));
    session.write(b"y");
    session.wait(|text| !text.contains("EXTSIDEBAR"));
    drop(session);
}

#[test]
fn files_key_and_view_menu_follow_the_named_replacement_slot() {
    let (_root, _extension, mut session) = pane_fixture("slots", 240);
    session.wait(|text| text.contains("FILES SLOT RIGHT") && text.contains("AUX PANE LEFT"));
    session.write(b"s");
    let closed =
        session.wait(|text| !text.contains("FILES SLOT RIGHT") && text.contains("AUX PANE LEFT"));
    assert!(closed.contains("alpha.ts"));
    session.click_label("View");
    session.wait(|text| text.contains("[ ] Files pane"));
    session.click_label("Files pane");
    let reopened =
        session.wait(|text| text.contains("FILES SLOT RIGHT") && text.contains("AUX PANE LEFT"));
    assert!(reopened.contains("alpha.ts"));
    drop(session);
}

#[test]
fn edge_panes_resize_through_the_horizontal_divider_hit_slop() {
    let (_root, _extension, mut session) = pane_fixture("edges", 140);
    session.wait(|text| text.contains("alpha.ts"));
    session.write(b"y");
    session.wait(|text| {
        text.contains("PANE TOP 138x2")
            && text.contains("PANE BOTTOM 138x2")
            && text.contains("alpha.ts")
    });
    session.write(b"\x1b[<0;71;5M\x1b[<32;71;7M\x1b[<0;71;7m");
    session.wait(|text| text.contains("PANE TOP 138x4"));
    session.write(b"y");
    session.wait(|text| !text.contains("PANE TOP") && !text.contains("PANE BOTTOM"));
    drop(session);
}

#[test]
fn short_terminal_keeps_confirmation_actions_mouse_accessible() {
    let root = super::layout::two_files(false);
    let extension_root = tempfile::tempdir().unwrap();
    let extension = super::file_views::example(extension_root.path(), "pty-extension-probe");
    fs::write(extension.join("fixture-kind"), "dialog").unwrap();
    let mut session = Session::launch_in(
        "",
        &[
            "diff",
            "--mode",
            "stack",
            "--extension",
            extension.to_str().unwrap(),
        ],
        false,
        50,
        12,
        None,
        Some(root.path()),
    );
    session.wait(|text| text.contains("alpha.ts"));
    session.write(b"y");
    let prompt = session
        .wait(|text| text.contains("Reformat the changeset?") && text.contains("enter/y reformat"));
    assert!(prompt.contains('…'));
    assert!(prompt.contains("ext fixture"));
    session.click_label("enter/y reformat");
    let answered = session.wait(|text| text.contains("DIALOG ANSWERED YES"));
    assert!(!answered.contains("Reformat the changeset?"));
    drop(session);
}

#[test]
fn startup_notification_renders_and_retires_without_changing_the_review() {
    let (_root, _extension, mut session) = pane_fixture("notify", 140);
    let toast = session.wait(|text| text.contains("hello from the fixture extension"));
    assert!(
        toast.contains("ext hello from the fixture extension"),
        "{toast}"
    );
    let cleared = session.wait(|text| !text.contains("hello from the fixture extension"));
    assert!(cleared.contains("alpha.ts"));
    drop(session);
}

fn trusted_fixture(kind: &str) -> (tempfile::TempDir, tempfile::TempDir) {
    let root = super::harness::repository(
        &[
            (
                "alpha.ts",
                "export const alpha = 1;\n",
                "export const alphaValue = 2;\n",
            ),
            (
                "beta.ts",
                "export const beta = 1;\n",
                "export const betaValue = 2;\n",
            ),
        ],
        |root| {
            let extension = super::file_views::example(
                &root.join(".agents/workdeck/extensions"),
                "pty-extension-probe",
            );
            fs::write(extension.join("fixture-kind"), kind).unwrap();
            fs::write(root.join(".git/info/exclude"), ".workdeck-shutdown.log\n").unwrap();
        },
    );
    (root, tempfile::tempdir().unwrap())
}

fn trust_launch(root: &Path, config: &Path) -> Session {
    Session::launch_in_config(
        "",
        &["diff", "--mode", "stack"],
        false,
        140,
        24,
        None,
        Some(root),
        Some(config),
    )
}

fn trust_decision(root: &Path, config: &Path) -> Option<String> {
    let Ok(bytes) = fs::read(config.join("workdeck/state.json")) else {
        return None;
    };
    let state: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    state["extensionTrust"][root.canonicalize().unwrap().to_str().unwrap()]
        .as_str()
        .map(str::to_owned)
}

#[test]
fn repository_trust_runs_the_transform_only_after_confirmation() {
    let (root, config) = trusted_fixture("transform");
    let mut session = trust_launch(root.path(), config.path());
    let prompt = session.wait(|text| {
        text.contains("Run this repository's extensions?")
            && text.contains("Extensions run with your user permissions.")
    });
    assert!(prompt.contains(".agents/workdeck/extensions"), "{prompt}");
    assert!(prompt.contains("Extensions run with your user permissions."));
    assert!(prompt.contains("beta.ts"));
    session.write(b"t");
    let transformed = session.wait(|text| {
        text.contains("REPO EXTENSION ACTIVE")
            && !text.contains("Run this repository's extensions?")
            && !text.contains("beta.ts")
    });
    assert!(transformed.contains("alpha.ts"));
    assert_eq!(
        trust_decision(root.path(), config.path()).as_deref(),
        Some("trusted")
    );
    drop(session);
}

#[test]
fn escaping_repository_trust_does_not_persist_a_decision() {
    let (root, config) = trusted_fixture("transform");
    let mut session = trust_launch(root.path(), config.path());
    session.wait(|text| text.contains("Run this repository's extensions?"));
    session.write(b"\x1b");
    let dismissed = session.wait(|text| !text.contains("Run this repository's extensions?"));
    assert!(dismissed.contains("beta.ts"));
    assert!(!dismissed.contains("REPO EXTENSION ACTIVE"));
    assert_eq!(trust_decision(root.path(), config.path()), None);
    drop(session);
}

#[test]
fn denying_repository_trust_persists_across_a_fresh_launch() {
    let (root, config) = trusted_fixture("transform");
    let mut session = trust_launch(root.path(), config.path());
    session.wait(|text| text.contains("Run this repository's extensions?"));
    session.write(b"n");
    let denied = session.wait(|text| !text.contains("Run this repository's extensions?"));
    assert!(denied.contains("beta.ts"));
    assert!(!denied.contains("REPO EXTENSION ACTIVE"));
    assert_eq!(
        trust_decision(root.path(), config.path()).as_deref(),
        Some("denied")
    );
    drop(session);
    let mut session = trust_launch(root.path(), config.path());
    let review = session.wait(|text| text.contains("alpha.ts") && text.contains("beta.ts"));
    assert!(!review.contains("Run this repository's extensions?"));
    assert!(!review.contains("REPO EXTENSION ACTIVE"));
    drop(session);
}

#[test]
fn control_c_delivers_native_shutdown_before_terminal_exit() {
    let (root, config) = trusted_fixture("shutdown");
    let mut session = trust_launch(root.path(), config.path());
    session.wait(|text| text.contains("Run this repository's extensions?"));
    session.write(b"t");
    session.wait(|text| text.contains("INTERRUPT FIXTURE READY"));
    session.write(b"\x03");
    let deadline = Instant::now() + Duration::from_secs(5);
    let log = root.path().join(".workdeck-shutdown.log");
    while !log.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(fs::read_to_string(log).unwrap(), "shutdown\n");
    drop(session);
}

#[test]
fn line_highlighter_preserves_text_and_reports_refresh_control_results() {
    let root = super::layout::repository(&[
        (
            "alpha.ts",
            "export const alpha = 1;\n",
            "export const alphaValue = 2;\n",
        ),
        (
            "beta.ts",
            "export const beta = 1;\n",
            "export const betaValue = 1;\n",
        ),
    ]);
    let extension_root = tempfile::tempdir().unwrap();
    let extension = super::file_views::example(extension_root.path(), "pty-extension-probe");
    fs::write(extension.join("fixture-kind"), "highlight").unwrap();
    let mut session = Session::launch_in(
        "",
        &[
            "diff",
            "--mode",
            "stack",
            "--extension",
            extension.to_str().unwrap(),
        ],
        false,
        140,
        24,
        None,
        Some(root.path()),
    );
    let initial = session.wait(|text| text.contains("export const alphaValue = 2;"));
    assert!(initial.contains("alpha.ts"));
    assert!(!initial.contains("line highlighter"));
    session.write(b"\x1b[18~");
    let refreshed = session.wait(|text| text.contains("marks refreshed"));
    assert!(!refreshed.contains("unknown line highlighter"));
    assert!(refreshed.contains("export const alphaValue = 2;"));
    session.write(b"\x1b[19~");
    session
        .wait(|text| text.contains("Extension fixture targeted unknown line highlighter \"nope\""));
    drop(session);
}

#[test]
fn queued_command_resumes_after_background_highlight_releases_connection() {
    let root = super::layout::repository(&[(
        "alpha.ts",
        "export const alpha = 1;\n",
        "export const alphaValue = 2;\n",
    )]);
    let extension_root = tempfile::tempdir().unwrap();
    let extension = super::file_views::example(extension_root.path(), "pty-extension-probe");
    fs::write(extension.join("fixture-kind"), "highlight").unwrap();
    let hold = extension.join("hold-line-highlight");
    fs::write(&hold, "hold\n").unwrap();
    let mut session = Session::launch_in(
        "",
        &[
            "diff",
            "--mode",
            "stack",
            "--extension",
            extension.to_str().unwrap(),
        ],
        false,
        140,
        24,
        None,
        Some(root.path()),
    );
    session.wait(|text| text.contains("export const alphaValue = 2;"));
    let blocked = extension.join("line-highlight-blocked");
    let deadline = Instant::now() + Duration::from_secs(3);
    while !blocked.exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        blocked.exists(),
        "background highlight never acquired the connection"
    );
    session.write(b"\x1b[19~");
    session.wait(|text| text.contains("Second fixture action"));
    assert!(
        !extension.join("line-highlight-released").exists(),
        "highlight hold expired before the command queued"
    );
    fs::remove_file(hold).unwrap();
    session
        .wait(|text| text.contains("Extension fixture targeted unknown line highlighter \"nope\""));
    drop(session);
}

fn reveal_case(held: bool) {
    let before = (1..=130)
        .map(|line| format!("export const line{line:03} = {line};\n"))
        .collect::<String>();
    let after = (1..=130)
        .map(|line| {
            if line == 111 {
                "export const needle = \"REVEALLINETOKEN\";\n".into()
            } else {
                format!("export const line{line:03} = {};\n", line + 1000)
            }
        })
        .collect::<String>();
    let root = super::layout::repository(&[("tall.ts", &before, &after)]);
    let extension_root = tempfile::tempdir().unwrap();
    let extension = super::file_views::example(extension_root.path(), "pty-extension-probe");
    fs::write(
        extension.join("fixture-kind"),
        if held { "held-reveal" } else { "reveal" },
    )
    .unwrap();
    let mut session = Session::launch_in(
        "",
        &[
            "diff",
            "--mode",
            "stack",
            "--extension",
            extension.to_str().unwrap(),
        ],
        false,
        140,
        24,
        None,
        Some(root.path()),
    );
    let initial =
        session.wait(|text| text.contains("tall.ts") && (!held || text.contains("CAPTURE PANE")));
    assert!(!initial.contains("REVEALLINETOKEN"));
    session.write(b"\x1b[18~");
    let revealed = session.wait(|text| text.contains("REVEALLINETOKEN"));
    let row = revealed
        .lines()
        .position(|line| line.contains("REVEALLINETOKEN"))
        .unwrap();
    assert!(row > 0 && row < 12, "{revealed}");
    if !held {
        session.write(b"\x1b[19~");
        session.wait(|text| text.contains("Extension fixture revealLine found no new line 9001"));
    }
    drop(session);
}

#[test]
fn reveal_line_lands_deep_inside_a_tall_hunk_near_the_viewport_top() {
    reveal_case(false);
}

#[test]
fn pane_navigation_retained_since_mount_reveals_the_measured_line() {
    reveal_case(true);
}
