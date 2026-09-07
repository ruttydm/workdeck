//! Hunk MIT: all 23 baseline cases from test/pty/layout.test.ts (18 in stable).
use super::*;
use std::path::Path;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(super) fn repository(files: &[(&str, &str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    git(root.path(), &["config", "user.name", "PTY fixture"]);
    git(
        root.path(),
        &["config", "user.email", "pty@example.invalid"],
    );
    for (path, before, _) in files {
        let path = root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, before).unwrap();
    }
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "baseline"]);
    for (path, _, after) in files {
        fs::write(root.path().join(path), after).unwrap();
    }
    root
}

pub(super) fn two_files(nested: bool) -> tempfile::TempDir {
    repository(&[
        (
            if nested {
                "src/ui/alpha.ts"
            } else {
                "alpha.ts"
            },
            "export const alpha = 1;\n",
            "export const alpha = 2;\nexport const add = true;\n",
        ),
        (
            if nested { "src/ui/beta.ts" } else { "beta.ts" },
            "export const beta = 1;\n",
            "export const betaValue = 1;\n",
        ),
    ])
}

fn launch(root: &Path, args: &[&str], cols: u16, rows: u16) -> Session {
    Session::launch_in("", args, false, cols, rows, None, Some(root))
}

fn split(text: &str) -> bool {
    text.lines().any(|line| line.matches('▌').count() >= 2)
}

fn long_line() -> (tempfile::TempDir, Session) {
    super::notes::pair(
        "export const message = 'short';\n",
        "export const message = 'this is a very long wrapped line for tuistory integration coverage';\n",
        "split",
        102,
        20,
        &[],
    )
}

#[test]
fn wide_characters_keep_split_dividers_in_the_same_cell_column() {
    let (_root, mut session) = super::notes::pair(
        "export const wide = '日本語';\nexport const plain = 'before';\n",
        "export const wide = '한국어';\nexport const plain = 'after';\n",
        "split",
        140,
        16,
        &[],
    );
    let snapshot = session.wait(|text| text.contains("日本語") && text.contains("plain"));
    let divider = |needle: &str| {
        let line = snapshot.lines().find(|line| line.contains(needle)).unwrap();
        let index = line.match_indices('▌').nth(1).unwrap().0;
        unicode_width::UnicodeWidthStr::width(&line[..index])
    };
    assert_eq!(divider("日本語"), divider("plain"));
    drop(session);
}

#[test]
fn cli_tab_width_reaches_the_interactive_renderer() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("before.txt"), "a\tbefore\n").unwrap();
    fs::write(root.path().join("after.txt"), "a\tafter\n").unwrap();
    let mut session = launch(
        root.path(),
        &[
            "diff",
            "--files",
            "before.txt",
            "after.txt",
            "--mode",
            "stack",
            "-x8",
        ],
        100,
        12,
    );
    session.wait(|text| text.contains("a       after"));
    drop(session);
}

#[test]
fn wrap_hotkey_reveals_and_hides_the_long_line_tail() {
    let (_root, mut session) = long_line();
    let initial = session.wait(|text| text.contains("this is a very long"));
    assert!(initial.contains("before.ts") && initial.contains("after.ts"));
    assert!(!initial.contains("ge';"));
    session.write(b"w");
    session.wait(|text| text.contains("ge';"));
    session.write(b"w");
    session.wait(|text| !text.contains("ge';") && text.contains("this is a very long"));
    drop(session);
}

fn scroll_until(session: &mut Session, input: &[u8], predicate: impl Fn(&str) -> bool) -> String {
    for _ in 0..96 {
        session.write(input);
        if let Some(text) = session.wait_for(Duration::from_millis(80), &predicate) {
            return text;
        }
    }
    panic!(
        "horizontal target not reached:\n{}",
        session.parser.terminal().plain_string()
    );
}

#[test]
fn horizontal_arrows_reveal_hidden_columns_and_restore_the_start() {
    let (_root, mut session) = long_line();
    let initial = session.wait(|text| text.contains("this is a very long"));
    assert!(!initial.contains("ge';"));
    let shifted = scroll_until(&mut session, b"\x1b[C", |text| text.contains("ge';"));
    assert!(!shifted.contains("this is a very long"));
    scroll_until(&mut session, b"\x1b[D", |text| {
        text.contains("this is a very long") && !text.contains("ge';")
    });
    drop(session);
}

#[test]
fn shifted_wheel_scrolls_code_horizontally() {
    let (_root, mut session) = long_line();
    let initial = session.wait(|text| text.contains("this is a very long"));
    assert!(!initial.contains("ge';"));
    let shifted = scroll_until(&mut session, b"\x1b[<69;61;11M", |text| {
        text.contains("ge';")
    });
    assert!(!shifted.contains("this is a very long"));
    drop(session);
}

#[test]
fn toggling_wrap_resets_horizontal_scroll() {
    let (_root, mut session) = long_line();
    session.wait(|text| text.contains("this is a very long"));
    scroll_until(&mut session, b"\x1b[C", |text| text.contains("ge';"));
    session.write(b"w");
    session.wait(|text| {
        text.contains("this is a very lo") && text.contains("wrapped line") && text.contains("ge';")
    });
    session.write(b"w");
    session.wait(|text| text.contains("this is a very long") && !text.contains("ge';"));
    drop(session);
}

#[test]
fn explicit_sidebar_is_visible_below_the_automatic_cutoff() {
    let root = two_files(false);
    let mut session = launch(
        root.path(),
        &["diff", "--mode", "split", "--sidebar"],
        150,
        18,
    );
    session.wait(|text| text.matches("alpha.ts").count() >= 2);
    drop(session);
    assert!(!root.path().join(".agents").exists());
}

#[test]
fn explicit_hidden_sidebar_can_be_toggled_open() {
    let root = two_files(false);
    let mut session = launch(
        root.path(),
        &["diff", "--mode", "split", "--no-sidebar"],
        220,
        18,
    );
    let initial = session.wait(|text| text.contains("betaValue = 1"));
    assert_eq!(initial.matches("alpha.ts").count(), 1);
    session.write(b"s");
    session.wait(|text| text.matches("alpha.ts").count() >= 2);
    drop(session);
}

#[test]
fn explicit_split_survives_a_narrow_resize() {
    let root = two_files(false);
    let mut session = launch(root.path(), &["diff", "--mode", "split"], 220, 24);
    session.wait(|text| split(text) && text.matches("alpha.ts").count() >= 2);
    session.resize(140, 24);
    session.wait(|text| {
        split(text) && text.matches("alpha.ts").count() == 1 && text.contains("betaValue = 1")
    });
    drop(session);
}

#[test]
fn explicit_stack_survives_a_wide_resize() {
    let root = two_files(false);
    let mut session = launch(root.path(), &["diff", "--mode", "stack"], 140, 24);
    let initial = session.wait(|text| text.contains("betaValue = 1"));
    assert!(!split(&initial));
    assert_eq!(initial.matches("alpha.ts").count(), 1);
    session.resize(220, 24);
    session.wait(|text| {
        !split(text)
            && text.matches("alpha.ts").count() >= 2
            && text.contains("1   -  export const alpha = 1;")
    });
    drop(session);
}

#[test]
fn direct_hotkeys_switch_split_stack_and_auto() {
    let root = two_files(false);
    let mut session = launch(root.path(), &["diff", "--mode", "stack"], 220, 24);
    session.wait(|text| !split(text) && text.contains("1   -  export const alpha = 1;"));
    session.write(b"1");
    session.wait(|text| split(text) && text.matches("alpha.ts").count() >= 2);
    session.write(b"2");
    session.wait(|text| !split(text) && text.contains("1   -  export const alpha = 1;"));
    session.write(b"0");
    session.wait(|text| split(text) && text.matches("alpha.ts").count() >= 2);
    drop(session);
}

#[test]
fn first_frame_fills_the_bottom_with_the_next_file() {
    let lines = |start: usize, offset: usize| {
        (start..start + 16)
            .map(|line| format!("export const line{line:02} = {};\n", line + offset))
            .collect::<String>()
    };
    let root = repository(&[
        ("first.ts", &lines(1, 0), &lines(1, 100)),
        ("second.ts", &lines(17, 0), &lines(17, 100)),
    ]);
    let mut session = launch(root.path(), &["diff", "--mode", "split"], 120, 28);
    session.wait(|text| text.contains("second.ts") && text.contains("line17 = 117"));
    drop(session);
}

#[test]
fn tall_first_frame_renders_beyond_the_first_neighbor() {
    let files = (0..8)
        .map(|index| {
            (
                format!("short-{index}.ts"),
                format!("export const short{index} = {index};\n"),
                format!("export const short{index} = {};\n", index + 10),
            )
        })
        .collect::<Vec<_>>();
    let borrowed = files
        .iter()
        .map(|(path, before, after)| (path.as_str(), before.as_str(), after.as_str()))
        .collect::<Vec<_>>();
    let root = repository(&borrowed);
    let mut session = launch(
        root.path(),
        &["diff", "--mode", "stack", "--no-sidebar"],
        100,
        40,
    );
    session.wait(|text| text.contains("short-3.ts") && text.contains("short3 = 13"));
    drop(session);
}

#[test]
fn file_gap_inserts_two_blank_rows_and_a_separator_before_the_header() {
    let root = two_files(false);
    let mut session = launch(
        root.path(),
        &["diff", "--mode", "stack", "--no-sidebar", "--file-gap", "3"],
        100,
        24,
    );
    let snapshot = session.wait(|text| text.contains("alpha.ts") && text.contains("beta.ts"));
    let lines = snapshot.lines().collect::<Vec<_>>();
    let index = lines
        .iter()
        .position(|line| line.contains("beta.ts"))
        .unwrap();
    assert!(index > 2);
    assert!(lines[index - 3].trim().is_empty(), "{snapshot}");
    assert!(lines[index - 2].trim().is_empty(), "{snapshot}");
    assert!(lines[index - 1].contains('─'), "{snapshot}");
    drop(session);
}

#[test]
fn hunk_gap_inserts_blank_rows_before_the_second_hunk() {
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
    let (_root, mut session) = super::notes::pair(
        &before,
        &after,
        "stack",
        100,
        32,
        &["--no-sidebar", "--hunk-gap", "2"],
    );
    let snapshot =
        session.wait(|text| text.lines().filter(|line| line.contains("@@")).count() >= 2);
    let lines = snapshot.lines().collect::<Vec<_>>();
    let index = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| line.contains("@@"))
        .nth(1)
        .unwrap()
        .0;
    assert!(
        lines[index - 2].trim().is_empty() && lines[index - 1].trim().is_empty(),
        "{snapshot}"
    );
    drop(session);
}

#[test]
fn context_hotkey_expands_and_collapses_source_lines() {
    let before = (1..=30)
        .map(|line| {
            if line == 1 {
                "export const hiddenLine01 = 1;\n".into()
            } else {
                format!("export const line{line:02} = {line};\n")
            }
        })
        .collect::<String>();
    let after = before.replace("line05 = 5;", "line05 = 500;");
    let (_root, mut session) = super::notes::pair(&before, &after, "split", 140, 16, &[]);
    let initial = session.wait(|text| text.contains("▾ 1 unchanged line"));
    assert!(!initial.contains("hiddenLine01"));
    session.write(b"z");
    session.wait(|text| text.contains("Hide 1 unchanged line") && text.contains("hiddenLine01"));
    session.write(b"z");
    session.wait(|text| text.contains("▾ 1 unchanged line") && !text.contains("hiddenLine01"));
    drop(session);
}

#[test]
fn narrow_headers_keep_stats_and_three_dot_path_truncation() {
    let root = repository(&[(
        "packages/visual-studio-code-vscode/extension-postgres.ts",
        "export const value = 1;\n",
        "export const value = 2;\n",
    )]);
    let mut session = launch(root.path(), &["diff", "--mode", "auto"], 40, 12);
    let snapshot = session.wait(|text| text.contains("packages/visual-studio-cod... +1 -1"));
    assert!(!snapshot.contains("packages/visual-studio-code-."));
    drop(session);
}

fn divider(text: &str) -> usize {
    text.lines()
        .filter_map(|line| line.chars().position(|ch| ch == '│'))
        .min()
        .expect("sidebar divider")
}

fn sidebar(text: &str) -> String {
    let column = divider(text);
    text.lines()
        .map(|line| line.chars().take(column).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn rightmost(text: &str, needle: &str) -> usize {
    text.lines()
        .filter_map(|line| {
            line.rfind(needle)
                .map(|index| line[..index].chars().count())
        })
        .max()
        .unwrap()
}

fn drag(session: &mut Session, from: usize, to: usize) {
    session.write(format!("\x1b[<0;{};7M", from + 1).as_bytes());
    session.wait_for(Duration::from_millis(20), |_| false);
    session.write(format!("\x1b[<32;{};7M", to + 1).as_bytes());
    session.wait_for(Duration::from_millis(20), |_| false);
    session.write(format!("\x1b[<0;{};7m", to + 1).as_bytes());
}

#[test]
fn auto_layout_tracks_tree_flat_hidden_and_stack_resize_thresholds() {
    let root = two_files(true);
    let mut session = launch(root.path(), &["diff", "--mode", "auto"], 220, 24);
    let wide = session.wait(|text| text.matches("alpha.ts").count() >= 2 && split(text));
    assert!(!sidebar(&wide).contains("src/ui/"));
    session.resize(180, 24);
    session.wait(|text| {
        text.lines().any(|line| line.contains('│'))
            && sidebar(text).contains("src/ui/")
            && split(text)
            && text.matches("alpha.ts").count() >= 2
    });
    session.resize(150, 24);
    session.wait(|text| text.matches("alpha.ts").count() == 1 && split(text));
    session.resize(110, 24);
    session.wait(|text| text.matches("alpha.ts").count() == 1 && !split(text));
    drop(session);
}

#[test]
fn dragging_sidebar_divider_resizes_the_review_pane() {
    let root = two_files(false);
    let mut session = launch(root.path(), &["diff", "--mode", "split"], 220, 18);
    let initial = session
        .wait(|text| text.matches("alpha.ts").count() >= 2 && text.contains("betaValue = 1"));
    let main_column = rightmost(&initial, "alpha.ts");
    let column = divider(&initial);
    assert!(column > 0 && main_column > column);
    drag(&mut session, column - 2, column + 18);
    let resized = session.wait(|text| rightmost(text, "alpha.ts") >= main_column + 3);
    assert!(resized.contains("beta.ts"));
    drop(session);
}

#[test]
fn sidebar_drag_survives_the_first_motion_switching_projection() {
    let root = two_files(true);
    let mut session = launch(root.path(), &["diff", "--mode", "split"], 220, 18);
    let initial = session
        .wait(|text| text.matches("alpha.ts").count() >= 2 && text.contains("betaValue = 1"));
    let main_column = rightmost(&initial, "alpha.ts");
    let column = divider(&initial);
    session.write(format!("\x1b[<0;{};7M", column - 1).as_bytes());
    session.wait_for(Duration::from_millis(20), |_| false);
    session.write(format!("\x1b[<32;{};7M", column - 3).as_bytes());
    session.wait(|text| {
        text.lines().any(|line| {
            line.chars()
                .take(column - 2)
                .collect::<String>()
                .contains("src/ui/")
        })
    });
    session.write(format!("\x1b[<32;{};7M", column - 15).as_bytes());
    session.wait_for(Duration::from_millis(20), |_| false);
    session.write(format!("\x1b[<0;{};7m", column - 15).as_bytes());
    session.wait(|text| rightmost(text, "alpha.ts") <= main_column - 8);
    drop(session);
}

#[test]
fn sidebar_drag_crossing_thirty_one_columns_switches_to_compact_paths() {
    let root = two_files(true);
    let mut session = launch(root.path(), &["diff", "--mode", "split"], 220, 18);
    let initial = session
        .wait(|text| text.matches("alpha.ts").count() >= 2 && text.contains("betaValue = 1"));
    let column = divider(&initial);
    let tree = sidebar(&initial);
    assert!(!tree.contains("src/ui/") && tree.contains("src/") && tree.contains("ui/"));
    assert_eq!(
        tree.lines()
            .find(|line| line.contains("src/"))
            .unwrap()
            .find("src/"),
        Some(2)
    );
    drag(&mut session, column - 2, column - 4);
    let compact = session.wait(|text| {
        text.lines().any(|line| {
            line.chars()
                .take(column - 2)
                .collect::<String>()
                .contains("src/ui/")
        })
    });
    let compact = compact
        .lines()
        .map(|line| line.chars().take(column - 2).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        compact
            .lines()
            .find(|line| line.contains("src/ui/"))
            .unwrap()
            .find("src/ui/"),
        Some(2)
    );
    assert!(compact.contains("alpha.ts") && compact.contains("beta.ts"));
    drop(session);
}

#[test]
fn unicode_rename_paths_remain_unescaped_in_sidebar_and_header() {
    let root = repository(&[(
        "国際化/日本語.txt",
        "shared\nold-only\nshared\n",
        "shared\nold-only\nshared\n",
    )]);
    git(root.path(), &["config", "core.quotePath", "false"]);
    git(
        root.path(),
        &["mv", "国際化/日本語.txt", "国際化/한국어-🧪.txt"],
    );
    fs::write(
        root.path().join("国際化/한국어-🧪.txt"),
        "shared\nnew-only\nshared\n",
    )
    .unwrap();
    git(root.path(), &["add", "."]);
    let mut session = launch(
        root.path(),
        &["diff", "--staged", "--mode", "split"],
        220,
        16,
    );
    let snapshot =
        session.wait(|text| text.contains("한국어-🧪.txt") && text.contains("日本語.txt"));
    assert!(snapshot.contains("国際化/"));
    assert!(!snapshot.contains("\\345\\233\\275"));
    drop(session);
}

#[test]
fn layout_hotkeys_preserve_the_scrolled_source_anchor() {
    let lines = |offset| {
        (1..=18)
            .map(|line| format!("export const line{line:02} = {};\n", line + offset))
            .collect::<String>()
    };
    let (_root, mut session) = super::notes::pair(&lines(0), &lines(100), "split", 220, 12, &[]);
    let mut anchored = session.wait(|text| text.contains("line01 = 101"));
    assert!(!anchored.contains("line08 = 108"));
    for _ in 0..24 {
        session.write(b"\x1b[B");
        session.wait_for(Duration::from_millis(200), |_| false);
        anchored = session.parser.terminal().plain_string();
        if anchored.contains("line08 = 108") && !anchored.contains("line01 = 101") {
            break;
        }
    }
    assert!(anchored.contains("line08 = 108") && !anchored.contains("line01 = 101"));
    let offset = anchored.find("line").unwrap();
    let anchor = &anchored[offset..offset + 6];
    session.write(b"2");
    session.wait(|text| !split(text) && text.contains(anchor));
    session.write(b"1");
    session.wait(|text| split(text) && text.contains(anchor));
    drop(session);
}
