//! Incremental Rust translation of Hunk's MIT-licensed `test/pty/harness.ts`.
//! The source file remains unmapped until all helpers and fixture factories are covered.

use super::{Duration, Session};
use std::path::Path;

pub(super) fn git(root: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

pub(super) fn repository(
    files: &[(&str, &str, &str)],
    prepare: impl FnOnce(&Path),
) -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    git(root.path(), &["init", "-q"]);
    git(root.path(), &["config", "user.name", "Pi"]);
    git(root.path(), &["config", "user.email", "pi@example.com"]);
    for (path, before, _) in files {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, before).unwrap();
    }
    prepare(root.path());
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-qm", "initial"]);
    for (path, _, after) in files {
        std::fs::write(root.path().join(path), after).unwrap();
    }
    root
}

#[test]
fn repository_factory_commits_baseline_and_prepared_entries_before_changes() {
    let root = repository(&[("nested/file.txt", "before\n", "after\n")], |root| {
        std::fs::write(root.join("prepared.txt"), "retained\n").unwrap();
    });
    assert_eq!(
        git(root.path(), &["show", "HEAD:nested/file.txt"]),
        "before\n"
    );
    assert_eq!(
        git(root.path(), &["show", "HEAD:prepared.txt"]),
        "retained\n"
    );
    assert_eq!(
        git(root.path(), &["diff", "--name-only"]),
        "nested/file.txt\n"
    );
    assert_eq!(
        git(root.path(), &["ls-files", "--others", "--exclude-standard"]),
        ""
    );
    assert_eq!(
        git(root.path(), &["log", "-1", "--format=%an <%ae> %s"]),
        "Pi <pi@example.com> initial\n"
    );
    let path = root.path().to_owned();
    drop(root);
    assert!(!path.exists());
}

/// Match the source harness's five interpolated, zero-based mouse positions.
fn drag_positions(start: (usize, usize), end: (usize, usize)) -> [(usize, usize); 5] {
    std::array::from_fn(|index| {
        let interpolate = |start: usize, end: usize| {
            // Coordinates are nonnegative; floor(x + 0.5) matches Math.round.
            (start as f64 + (end as f64 - start as f64) * (index + 1) as f64 / 5.0 + 0.5).floor()
                as usize
        };
        (interpolate(start.0, end.0), interpolate(start.1, end.1))
    })
}

pub(super) fn drag_mouse(session: &mut Session, start: (usize, usize), end: (usize, usize)) {
    session.write(format!("\x1b[<0;{};{}M", start.0 + 1, start.1 + 1).as_bytes());
    session.wait_for(Duration::from_millis(10), |_| false);
    for (x, y) in drag_positions(start, end) {
        session.write(format!("\x1b[<32;{};{}M", x + 1, y + 1).as_bytes());
        session.wait_for(Duration::from_millis(10), |_| false);
    }
    session.write(format!("\x1b[<0;{};{}m", end.0 + 1, end.1 + 1).as_bytes());
    session.wait_for(Duration::from_millis(60), |_| false);
}

#[test]
fn mouse_drag_interpolates_all_five_source_steps_in_both_directions() {
    assert_eq!(
        drag_positions((8, 6), (28, 11)),
        [(12, 7), (16, 8), (20, 9), (24, 10), (28, 11)]
    );
    assert_eq!(
        drag_positions((28, 11), (8, 6)),
        [(24, 10), (20, 9), (16, 8), (12, 7), (8, 6)]
    );
    assert_eq!(
        drag_positions((1, 1), (3, 0)),
        [(1, 1), (2, 1), (2, 0), (3, 0), (3, 0)]
    );
    assert_eq!(drag_positions((0, 0), (0, 0)), [(0, 0); 5]);
}
