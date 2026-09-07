//! Hunk MIT: thirteen cases from test/pty/file-views-integration.test.ts.
use super::*;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

fn example(root: &Path, name: &str) -> PathBuf {
    static BUILT: OnceLock<()> = OnceLock::new();
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    BUILT.get_or_init(|| {
        let output = Command::new(env!("CARGO"))
            .args([
                "build",
                "-p",
                "workdeck-examples",
                "--bin",
                "workdeck-example-rendered-markdown-extension",
                "--bin",
                "workdeck-example-inline-edit-extension",
                "--bin",
                "workdeck-example-jsx-file-view-extension",
                "--bin",
                "workdeck-example-file-view-gallery-extension",
                "--bin",
                "workdeck-example-cursor-mode-extension",
            ])
            .current_dir(workspace)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    });
    let source = workspace.join("examples/extensions").join(name);
    let destination = root.join(name);
    fs::create_dir_all(destination.join("bin")).unwrap();
    fs::copy(
        source.join("workdeck-extension.toml"),
        destination.join("workdeck-extension.toml"),
    )
    .unwrap();
    let executable = format!("workdeck-example-{name}-extension");
    let built = Path::new(env!("CARGO_BIN_EXE_workdeck"))
        .parent()
        .unwrap()
        .join(&executable);
    fs::copy(built, destination.join("bin").join(executable)).unwrap();
    destination
}

fn markdown(root: &Path, range: [u32; 2]) {
    fs::write(root.join("before.md"), "# Heading\n\n- old item\n").unwrap();
    fs::write(root.join("after.md"), "# Heading\n\n- new item\n").unwrap();
    fs::write(root.join("agent.json"), serde_json::json!({"version":1,"files":[{"path":"after.md","annotations":[{"newRange":range,"summary":"Review the new item."}]}]}).to_string()).unwrap();
}

fn launch(root: &Path, extension: Option<&Path>, notes: bool) -> Session {
    let mut args = vec![
        "diff",
        "--mode",
        "stack",
        "--files",
        "before.md",
        "after.md",
    ];
    if let Some(extension) = extension {
        args.extend(["--extension", extension.to_str().unwrap()]);
    }
    if notes {
        args.extend(["--agent-context", "agent.json", "--agent-notes"]);
    }
    Session::launch_in("", &args, false, 140, 24, None, Some(root))
}

#[test]
fn markdown_is_not_loaded_without_explicit_installation() {
    let root = tempfile::tempdir().unwrap();
    markdown(root.path(), [3, 3]);
    let mut session = launch(root.path(), None, false);
    session.wait(|text| text.contains("before.md"));
    session.click_label("View");
    let menu = session.wait(|text| text.contains("File presentation: Raw diff"));
    assert!(!menu.contains("File presentation: Rendered Markdown"));
    drop(session);
}

#[test]
fn markdown_extension_loads_and_preserves_hunk_navigation() {
    let root = tempfile::tempdir().unwrap();
    markdown(root.path(), [3, 3]);
    let extension = example(root.path(), "rendered-markdown");
    let mut session = launch(root.path(), Some(&extension), false);
    session.wait(|text| text.contains("before.md"));
    session.click_label("View");
    let menu = session.wait(|text| text.contains("File presentation: Rendered Markdown"));
    assert!(menu.contains("File presentation: Raw diff"));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("File presentation:"));
    session.write(b"\x1b[19~");
    session.wait(|text| text.contains("• new item"));
    session.click_label("View");
    let menu = session.wait(|text| text.contains("[x] File presentation: Rendered Markdown"));
    assert!(!menu.contains("# Heading"));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("File presentation:"));
    session.write(b"]");
    session.wait_for(Duration::from_millis(100), |_| false);
    drop(session);
}

#[test]
fn bound_notes_render_inside_the_markdown_presentation() {
    let root = tempfile::tempdir().unwrap();
    markdown(root.path(), [3, 3]);
    let extension = example(root.path(), "rendered-markdown");
    let mut session = launch(root.path(), Some(&extension), true);
    session.wait(|text| text.contains("before.md"));
    session.write(b"\x1b[19~");
    let preview =
        session.wait(|text| text.contains("• new item") && text.contains("Review the new item."));
    assert!(!preview.contains("old item"));
    session.click_label("View");
    let menu = session.wait(|text| text.contains("[x] File presentation: Rendered Markdown"));
    assert!(menu.contains("File presentation: Raw diff"));
    drop(session);
}

#[test]
fn unbound_notes_force_raw_and_hiding_notes_restores_the_selected_view() {
    let root = tempfile::tempdir().unwrap();
    markdown(root.path(), [99, 99]);
    let extension = example(root.path(), "rendered-markdown");
    let mut session = launch(root.path(), Some(&extension), true);
    session.wait(|text| text.contains("before.md"));
    session.write(b"\x1b[19~");
    session.wait_for(Duration::from_millis(200), |_| false);
    let raw = session.wait(|text| text.contains("old item"));
    assert!(!raw.contains("• new item"));
    session.click_label("View");
    session.wait(|text| text.contains("[x] File presentation: Rendered Markdown"));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("File presentation:"));
    session.write(b"a");
    let restored = session.wait(|text| text.contains("• new item"));
    assert!(!restored.contains("old item"));
    drop(session);
}

fn inline_editor(root: &Path, notes: bool) -> (tempfile::TempDir, Session) {
    let extension_root = tempfile::tempdir().unwrap();
    let extension = example(extension_root.path(), "inline-edit");
    let mut args = vec![
        "diff",
        "--extension",
        extension.to_str().unwrap(),
        "--mode",
        "stack",
    ];
    if notes {
        args.extend(["--agent-context", "agent.json", "--agent-notes"]);
    }
    let session = Session::launch_in("", &args, false, 140, 24, None, Some(root));
    (extension_root, session)
}

fn wait_for_file(session: &mut Session, path: &Path, expected: &str) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while fs::read_to_string(path).unwrap() != expected {
        assert!(Instant::now() < deadline, "host write did not land");
        session.wait_for(Duration::from_millis(100), |_| false);
    }
}

#[test]
fn inline_edit_types_requests_confirmation_writes_and_exits_mode_after_reload() {
    let root = super::layout::two_files(false);
    let (_extension_root, mut session) = inline_editor(root.path(), false);
    session.wait(|text| text.contains("alpha.ts"));
    session.write(b"\x05");
    session.wait(|text| {
        !text.contains("alpha = 1")
            && text.contains("EDITING — Esc exits · ctrl+s writes")
            && text.contains("inline-edit:inline-edit mode — Esc exits")
    });
    session.write(b"zzz");
    let typed = session.wait(|text| text.contains("zzzexport const alpha = 2;"));
    assert!(typed.contains("MODIFIED"));
    session.write(b"?");
    session.wait(|text| text.contains("Controls help"));
    session.write(b"\x1b");
    session.wait(|text| {
        !text.contains("Controls help") && text.contains("inline-edit:inline-edit mode — Esc exits")
    });
    session.write(b"\x13");
    let prompt = session
        .wait(|text| text.contains("Write alpha.ts?") && text.contains("ext example.inline-edit"));
    assert!(prompt.contains("replace this file's contents on disk"));
    session.write(b"\r");
    wait_for_file(
        &mut session,
        &root.path().join("alpha.ts"),
        "zzzexport const alpha = 2;\nexport const add = true;\n",
    );
    session.wait(|text| text.contains("zzzexport const alpha = 2;") && !text.contains("Esc exits"));
    session.write(b"z");
    session.wait_for(Duration::from_millis(100), |_| false);
    let after = session.parser.terminal().plain_string();
    assert!(after.contains("zzzexport const alpha = 2;") && !after.contains("zzzz"));
    drop(session);
}

#[test]
fn inline_edit_keeps_a_joined_annotated_line_visible() {
    let root = super::layout::two_files(false);
    fs::write(root.path().join("agent.json"), serde_json::json!({"version":1,"files":[{"path":"alpha.ts","annotations":[{"newRange":[2,2],"summary":"Keep this note visible."}]}]}).to_string()).unwrap();
    let (_extension_root, mut session) = inline_editor(root.path(), true);
    session.wait(|text| text.contains("Keep this note visible."));
    session.write(b"\x05");
    session.wait(|text| text.contains("EDITING — Esc exits"));
    session.write(b"\x1b[B\x7f");
    let joined =
        session.wait(|text| text.contains("export const alpha = 2;export const add = true;"));
    assert!(joined.contains("EDITING — Esc exits") && joined.contains("Keep this note visible."));
    session.write(b"z");
    session.wait(|text| text.contains("export const alpha = 2;zexport const add = true;"));
    session.write(b"\x1b");
    drop(session);
}

#[test]
fn inline_edit_deletes_and_writes_a_whole_emoji() {
    let root = super::layout::two_files(false);
    let path = root.path().join("alpha.ts");
    fs::write(&path, "😀\n").unwrap();
    let (_extension_root, mut session) = inline_editor(root.path(), false);
    session.wait(|text| text.contains("😀"));
    session.write(b"\x05");
    session.wait(|text| text.contains("EDITING — Esc exits"));
    session.write(b"\x1b[C\x7f\x13");
    session.wait(|text| text.contains("Write alpha.ts?"));
    session.write(b"\r");
    wait_for_file(&mut session, &path, "\n");
    drop(session);
}

fn pinned_fixture(path: &str) -> Vec<u8> {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let output = Command::new("git")
        .args([
            "show",
            &format!("2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:{path}"),
        ])
        .current_dir(workspace)
        .output()
        .unwrap();
    assert!(output.status.success(), "missing pinned fixture {path}");
    output.stdout
}

fn gallery_demo(before: &str, after: &str, title: &str, first: &str, second: &str, raw: &str) {
    let root = tempfile::tempdir().unwrap();
    let extension = example(root.path(), "file-view-gallery");
    let before_path = root
        .path()
        .join("before")
        .join(Path::new(before).file_name().unwrap());
    let after_path = root
        .path()
        .join("after")
        .join(Path::new(after).file_name().unwrap());
    fs::create_dir_all(before_path.parent().unwrap()).unwrap();
    fs::create_dir_all(after_path.parent().unwrap()).unwrap();
    fs::write(
        &before_path,
        pinned_fixture(&format!(
            "examples/extensions/jsx-file-view-gallery/fixtures/{before}"
        )),
    )
    .unwrap();
    fs::write(
        &after_path,
        pinned_fixture(&format!(
            "examples/extensions/jsx-file-view-gallery/fixtures/{after}"
        )),
    )
    .unwrap();
    let mut session = Session::launch_in(
        "",
        &[
            "diff",
            "--extension",
            extension.to_str().unwrap(),
            "--mode",
            "stack",
            "--files",
            before_path.to_str().unwrap(),
            after_path.to_str().unwrap(),
        ],
        false,
        140,
        24,
        None,
        Some(root.path()),
    );
    session.wait(|text| text.contains("before.") || text.contains("package.json"));
    session.click_label("View");
    session.wait(|text| text.contains(title));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("File presentation:"));
    session.write(b"\x1b[19~");
    session.wait(|text| text.contains(first));
    session.write(b"]");
    session.wait(|text| {
        text.lines()
            .any(|line| line.contains('▶') && line.contains(second))
    });
    session.click_label("View");
    session.wait(|text| text.contains("File presentation: Raw diff"));
    session.click_label("File presentation: Raw diff");
    session.wait(|text| text.contains(raw));
    drop(session);
}

#[test]
fn checked_in_change_atlas_runs_against_the_pinned_diff() {
    gallery_demo(
        "change-atlas/before.ts",
        "change-atlas/after.ts",
        "File presentation: JSX demo: Change atlas",
        "▶ CHANGE 01",
        "CHANGE 02",
        "const percent = Math.min",
    );
}

#[test]
fn checked_in_css_palette_runs_against_the_pinned_diff() {
    gallery_demo(
        "css-palette/before.css",
        "css-palette/after.css",
        "File presentation: JSX demo: CSS palette delta",
        "▶ --accent",
        "--card-highlight",
        "--canvas: #090d18",
    );
}

#[test]
fn checked_in_dependency_delta_runs_against_the_pinned_diff() {
    gallery_demo(
        "package-dependencies/before/package.json",
        "package-dependencies/after/package.json",
        "File presentation: JSX demo: Dependency delta",
        "▶ Package metadata hunk 1",
        "@opentui/core",
        "\"@opentui/core\": \"0.4.3\"",
    );
}

fn multi_hunk_pair(root: &Path) {
    let before = (1..=80)
        .map(|line| format!("export const line{line} = {line};\n"))
        .collect::<String>();
    let mut after = before.replace("line1 = 1;", "line1 = 100;");
    for line in 60..=65 {
        after = after.replace(
            &format!("line{line} = {line};"),
            &format!("line{line} = {line}00;"),
        );
    }
    fs::write(root.join("before.ts"), before).unwrap();
    fs::write(root.join("after.ts"), after).unwrap();
}

fn multi_hunk_view(root: &Path, name: &str) -> Session {
    multi_hunk_pair(root);
    let extension = example(root, name);
    Session::launch_in(
        "",
        &[
            "diff",
            "--extension",
            extension.to_str().unwrap(),
            "--mode",
            "stack",
            "--files",
            "before.ts",
            "after.ts",
        ],
        false,
        140,
        24,
        None,
        Some(root),
    )
}

#[test]
fn folder_hunk_cards_toggle_by_key_and_menu_across_two_hunks() {
    let root = tempfile::tempdir().unwrap();
    let mut session = multi_hunk_view(root.path(), "jsx-file-view");
    session.wait(|text| text.contains("before.ts"));
    session.write(b"\x1b[19~");
    let custom = session.wait(|text| {
        text.contains("▶ Hunk 1")
            && text.contains("Hunk 2")
            && text.contains("row 0 · click for detail")
    });
    assert!(!custom.contains("invalid span"));
    session.click_label("▶ Hunk 1");
    let detail = session.wait(|text| text.contains("lines 1–4 · @@ -1,4 +1,4 @@"));
    assert!(!detail.contains("row 0 · click for detail"));
    session.write(b"]");
    let second = session.wait(|text| text.contains("▶ Hunk 2"));
    assert!(!second.contains("▶ Hunk 1"));
    session.write(b"\x1b[19~");
    let raw = session.wait(|text| text.contains("line60 = 6000"));
    assert!(!raw.contains("Hunk 1"));
    session.click_label("Extensions");
    let menu = session.wait(|text| {
        text.lines()
            .any(|line| line.contains("Toggle JSX hunk cards (POC)") && line.contains("F8"))
    });
    assert!(
        menu.lines()
            .any(|line| line.contains("Toggle JSX hunk cards (POC)") && line.contains("F8")),
        "{menu}"
    );
    session.click_label("Toggle JSX hunk cards (POC)");
    let custom = session.wait(|text| text.contains("▶ Hunk 2"));
    assert!(custom.contains("Hunk 1"));
    drop(session);
}

#[test]
fn interactive_file_view_routes_handled_passed_and_escape_keys() {
    let root = tempfile::tempdir().unwrap();
    let mut session = multi_hunk_view(root.path(), "cursor-mode");
    session.wait(|text| text.contains("before.ts"));
    session.write(b"\x1b[20~");
    session.wait(|text| {
        text.contains("CURSOR AT 0") && text.contains("cursor-mode:cursor mode — Esc exits")
    });
    session.write(b"j");
    session.wait(|text| text.contains("CURSOR AT 1"));
    session.write(b"j");
    session.wait(|text| text.contains("CURSOR AT 2"));
    session.write(b"?");
    session.wait(|text| text.contains("Controls help"));
    session.write(b"\x1b");
    session.wait(|text| {
        !text.contains("Controls help") && text.contains("cursor-mode:cursor mode — Esc exits")
    });
    session.wait_for(Duration::from_millis(150), |_| false);
    session.write(b"\x1b");
    session.wait(|text| text.contains("CURSOR AT 2") && !text.contains("Esc exits"));
    session.write(b"\x1b[19~");
    let raw = session.wait(|text| text.contains("line60 = 6000"));
    assert!(!raw.contains("CURSOR AT"));
    drop(session);
}

#[test]
fn retains_three_preview_types_between_raw_diffs_in_one_scrollable_review_stream() {
    let sources = [
        (
            "README.md",
            "mixed-review/fixtures/before/README.md",
            "mixed-review/fixtures/after/README.md",
        ),
        (
            "package.json",
            "fixtures/package-dependencies/before/package.json",
            "fixtures/package-dependencies/after/package.json",
        ),
        (
            "scripts/deploy.py",
            "mixed-review/fixtures/before/scripts/deploy.py",
            "mixed-review/fixtures/after/scripts/deploy.py",
        ),
        (
            "src/invoice.ts",
            "fixtures/change-atlas/before.ts",
            "fixtures/change-atlas/after.ts",
        ),
        (
            "styles/theme.css",
            "fixtures/css-palette/before.css",
            "fixtures/css-palette/after.css",
        ),
    ];
    let files = sources.map(|(path, before, after)| {
        let read = |suffix| {
            String::from_utf8(pinned_fixture(&format!(
                "examples/extensions/jsx-file-view-gallery/{suffix}"
            )))
            .unwrap()
        };
        (path, read(before), read(after))
    });
    let borrowed = files
        .iter()
        .map(|(path, before, after)| (*path, before.as_str(), after.as_str()))
        .collect::<Vec<_>>();
    let root = super::layout::repository(&borrowed);
    let extension_root = tempfile::tempdir().unwrap();
    let extension = example(extension_root.path(), "file-view-gallery");
    let mut session = Session::launch_in(
        "",
        &[
            "diff",
            "--extension",
            extension.to_str().unwrap(),
            "--mode",
            "stack",
        ],
        false,
        220,
        24,
        None,
        Some(root.path()),
    );
    session.wait(|text| text.contains("README.md"));
    for (file, preview) in [
        ("package.json", "Package metadata hunk 1"),
        ("invoice.ts", "CHANGE 01"),
        ("theme.css", "--accent"),
    ] {
        session.click_label(file);
        session.write(b"\x1b[19~");
        session.wait(|text| text.contains(preview));
    }
    session.click_label("package.json");
    session.wait(|text| text.contains("@opentui/core"));
    session.click_label("README.md");
    session.wait(|text| text.contains("understanding release changes"));
    let mut reached_preview = false;
    for _ in 0..10 {
        for _ in 0..8 {
            session.write(b"\x1b[<65;60;6M");
        }
        if session
            .wait_for(Duration::from_millis(750), |text| {
                text.contains("Package metadata hunk 1")
            })
            .is_some()
        {
            reached_preview = true;
            break;
        }
    }
    assert!(
        reached_preview,
        "{}",
        session.parser.terminal().plain_string()
    );
    drop(session);
}
