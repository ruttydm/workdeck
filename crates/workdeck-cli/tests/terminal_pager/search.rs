//! Native PTY translation of Hunk's MIT-licensed test/pty/search-integration.test.ts.

use super::harness::repository;
use super::{Duration, Session};
use std::path::Path;

/// Sixty context lines between the two hunks, so the second sits well below
/// the fold of a short terminal.
fn filler() -> String {
    (0..60)
        .map(|index| format!("const filler{index} = {index};"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// `readConfig` appears in two hunks of a tall first file (the second well
/// below the fold of a short terminal) and once in a second file.
fn search_repo() -> tempfile::TempDir {
    repository(
        &[
            (
                "alpha.ts",
                &format!("const top = 1;\n{}\nconst bottom = 2;\n", filler()),
                &format!(
                    "const top = readConfig(\"first\");\n{}\nconst bottom = readConfig(\"second\");\n",
                    filler()
                ),
            ),
            (
                "beta.ts",
                "const other = 1;\n",
                "const other = readConfig(\"third\");\n",
            ),
        ],
        |_| {},
    )
}

fn launch(root: &Path) -> Session {
    Session::launch_in(
        "",
        &["diff", "--mode", "stack", "--no-watch", "--no-sidebar"],
        false,
        120,
        14,
        None,
        Some(root),
    )
}

/// The last non-blank terminal row, where the status line paints.
fn status_row(text: &str) -> &str {
    text.trim_end().lines().last().unwrap_or_default()
}

fn line_index_of(text: &str, needle: &str) -> Option<usize> {
    text.lines().position(|line| line.contains(needle))
}

/// The background painted under the first `needle` cell on the terminal row
/// that shows it. Count cells, not UTF-8 bytes.
fn background_under(
    session: &Session,
    needle: &str,
) -> Option<qwertty_term_vt::snapshot::SnapshotColor> {
    let snapshot = session.parser.terminal().snapshot();
    let rows = snapshot.visible_window(0);
    for row in rows {
        let text: String = row.cells.iter().map(|cell| cell.ch.to_string()).collect();
        if let Some(column) = text.find(needle) {
            let column = text[..column].chars().count();
            return row.cells.get(column).map(|cell| cell.style.bg);
        }
    }
    None
}

fn ready(session: &mut Session) -> String {
    session.wait(|text| text.contains("Navigate"))
}

#[test]
fn slash_searches_enter_reveals_the_match_and_n_and_capital_n_step_through() {
    let root = search_repo();
    let mut session = launch(root.path());
    let initial = ready(&mut session);
    // The second match sits below the fold, so a landing there has to scroll.
    assert!(initial.contains("readConfig(\"first\")"));
    assert!(!initial.contains("readConfig(\"second\")"));

    session.write(b"/");
    session.wait(|text| status_row(text).trim_start().starts_with("/ search diff"));
    session.write(b"readconfig");
    session.wait(|text| status_row(text).contains("/ readconfig"));
    session.write(b"\r");

    // Strictly forward from the current hunk (alpha's first), so the first
    // landing is alpha's second hunk, revealed a little below the top edge.
    let landed = session.wait(|text| status_row(text).contains("[2/3] alpha.ts:62"));
    assert!(
        landed.contains("— const bottom = readConfig(\"second\");"),
        "{landed}"
    );
    let row = line_index_of(&landed, "readConfig(\"second\")").unwrap();
    assert!(row > 1 && row < 8, "row {row}:\n{landed}");

    // The landed match is painted with the inverted "current" mark while the
    // other matches carry a tinted mark. Marks are prepared after the landing
    // paints, so poll until all three backgrounds are distinct.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let current = background_under(&session, "readConfig(\"second\")");
        let other = background_under(&session, "readConfig(\"third\")");
        let plain = background_under(&session, "other = readConfig");
        if let (Some(current), Some(other), Some(plain)) = (current, other, plain)
            && current != other
            && other != plain
            && current != plain
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "search marks never painted:\n{}",
            session.parser.terminal().plain_string()
        );
        session.wait_for(Duration::from_millis(60), |_| false);
    }

    session.write(b"n");
    session.wait(|text| status_row(text).contains("[3/3] beta.ts:1"));
    session.write(b"n");
    let wrapped = session.wait(|text| status_row(text).contains("[1/3] alpha.ts:1"));
    assert!(status_row(&wrapped).contains("wrapped"), "{wrapped}");
    assert!(line_index_of(&wrapped, "readConfig(\"first\")").unwrap() < 8);

    session.write(b"N");
    session.wait(|text| {
        status_row(text).contains("[3/3] beta.ts:1") && status_row(text).contains("wrapped")
    });

    // Tab still opens the file filter beside the persistent search item.
    session.write(b"\t");
    session.wait(|text| text.contains("filter: type to filter files"));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("filter: type to filter files"));
    session.quit();
}

#[test]
fn repeated_occurrences_on_the_landed_line_each_carry_a_mark() {
    let root = search_repo();
    std::fs::write(
        root.path().join("beta.ts"),
        "const pair = needle(\"first\") + needle(\"second\");\n",
    )
    .unwrap();
    let mut session = launch(root.path());
    ready(&mut session);

    session.write(b"/");
    session.wait(|text| status_row(text).contains("/ search diff"));
    session.write(b"needle");
    session.write(b"\r");
    session.wait(|text| status_row(text).contains("[1/1] beta.ts:1"));

    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let first = background_under(&session, "needle(\"first\")");
        let second = background_under(&session, "needle(\"second\")");
        let plain = background_under(&session, "pair =");
        // The landed first occurrence carries the inverted current mark;
        // the later one keeps the ordinary tinted match mark; both differ
        // from the row's unmarked cells.
        if let (Some(first), Some(second), Some(plain)) = (first, second, plain)
            && first != second
            && first != plain
            && second != plain
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "repeated match marks never painted:\n{}",
            session.parser.terminal().plain_string()
        );
        session.wait_for(Duration::from_millis(60), |_| false);
    }
    session.quit();
}

#[test]
fn an_empty_query_is_refused_escape_keeps_the_search_and_the_emptied_prompt_clears_it() {
    let root = search_repo();
    let mut session = launch(root.path());
    ready(&mut session);

    session.write(b"n");
    session.wait(|text| status_row(text).contains("No search yet — press / to search"));

    session.write(b"/");
    session.wait(|text| text.contains("/ search diff"));
    session.write(b"zzz");
    session.write(b"\r");
    session.wait(|text| status_row(text).contains("No match for \"zzz\""));

    // Reopen: the last query is prefilled; Escape clears it, Escape again
    // cancels and leaves the last report in place.
    session.write(b"/");
    session.wait(|text| status_row(text).contains("/ zzz"));
    session.write(b"\x1b");
    session.wait(|text| status_row(text).contains("/ search diff"));
    session.write(b"\x1b");
    session.wait(|text| status_row(text).contains("No match for \"zzz\""));

    // Escape then Enter submits the emptied prompt, which ends the search.
    session.write(b"/");
    session.wait(|text| status_row(text).contains("/ zzz"));
    session.write(b"\x1b");
    session.wait(|text| status_row(text).contains("/ search diff"));
    session.write(b"\r");
    session.wait(|text| {
        !text.contains("No match")
            && !text.contains("/ search diff")
            && !status_row(text).trim_start().starts_with('/')
    });
    session.write(b"n");
    session.wait(|text| status_row(text).contains("No search yet"));
    session.quit();
}

#[test]
fn remapping_the_filter_onto_slash_takes_the_key_back_from_search() {
    let root = search_repo();
    let config = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(config.path().join("workdeck")).unwrap();
    std::fs::write(
        config.path().join("workdeck/config.toml"),
        "[keybindings]\n\"workdeck.review.focusFilter\" = \"/\"\n",
    )
    .unwrap();
    let mut session = Session::launch_in_config(
        "",
        &["diff", "--mode", "stack", "--no-watch", "--no-sidebar"],
        false,
        120,
        14,
        None,
        Some(root.path()),
        Some(config.path()),
    );
    let initial = ready(&mut session);
    // An exclusive user binding is not a conflict: no warning names the
    // bundled command.
    assert!(!initial.contains("workdeck.search.find"));

    session.write(b"/");
    session.wait(|text| text.contains("filter: type to filter files"));
    session.write(b"\x1b");
    session.wait(|text| !text.contains("filter: type to filter files"));

    // Search kept `n` / `N` and is still reachable from the Navigate menu.
    session.write(b"n");
    session.wait(|text| status_row(text).contains("No search yet"));
    session.quit();
}
