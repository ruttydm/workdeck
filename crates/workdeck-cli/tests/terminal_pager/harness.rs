//! Incremental Rust translation of Hunk's MIT-licensed `test/pty/harness.ts`.
//! The source file remains unmapped until all helpers and fixture factories are covered.

use super::{Duration, Session};
use std::path::Path;

pub(super) fn direct_file_pair(name: &str) -> tempfile::TempDir {
    let numbered = |count: usize, padded: bool, offset: usize| {
        (1..=count)
            .map(|n| {
                let label = if padded {
                    format!("{n:02}")
                } else {
                    n.to_string()
                };
                format!("export const line{label} = {};\n", n + offset)
            })
            .collect::<String>()
    };
    let (before, after) = match name {
        "createLongWrapFilePair" => (
            "export const message = 'short';\n".into(),
            "export const message = 'this is a very long wrapped line for tuistory integration coverage';\n".into(),
        ),
        "createWideCharacterFilePair" => (
            "export const wide = '日本語';\nexport const plain = 'before';\n".into(),
            "export const wide = '한국어';\nexport const plain = 'after';\n".into(),
        ),
        "createTabbedFilePair" => ("a\tbefore\n".into(), "a\tafter\n".into()),
        "createDeletionOnlyFilePair" => (
            "export const keep = true;\nexport const removeMe = true;\n".into(),
            "export const keep = true;\n".into(),
        ),
        "createMultiHunkFilePair" => {
            let before = numbered(80, false, 0);
            let mut after = before.clone();
            for n in std::iter::once(1).chain(60..=65) {
                after = after.replace(
                    &format!("export const line{n} = {n};\n"),
                    &format!("export const line{n} = {};\n", n * 100),
                );
            }
            (before, after)
        }
        "createExpandableContextFilePair" => {
            let before = numbered(30, true, 0).replacen("line01", "hiddenLine01", 1);
            let after = before.replace("line05 = 5;", "line05 = 500;");
            (before, after)
        }
        "createScrollableFilePair" => (numbered(18, true, 0), numbered(18, true, 100)),
        "createWatchFilePair" => (
            "export const watchedValue = 'before';\n".into(),
            "export const watchedValue = 'initial change';\n".into(),
        ),
        _ => panic!("unknown pinned file-pair factory: {name}"),
    };
    let root = tempfile::tempdir().unwrap();
    if name == "createWatchFilePair" {
        git(root.path(), &["init", "-q"]);
    }
    let extension = if name == "createTabbedFilePair" {
        "txt"
    } else {
        "ts"
    };
    std::fs::write(root.path().join(format!("before.{extension}")), before).unwrap();
    std::fs::write(root.path().join(format!("after.{extension}")), after).unwrap();
    root
}

#[test]
fn direct_file_pair_bytes_match_both_frozen_upstream_oracles() {
    use sha2::{Digest, Sha256};
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../port/hunk/oracles/pty-harness-file-pairs.json"
    ))
    .unwrap();
    assert_eq!(oracle["baselines"].as_array().unwrap().len(), 2);
    let fixtures = oracle["fixtures"].as_array().unwrap();
    assert_eq!(fixtures.len(), 8);
    for fixture in fixtures {
        let name = fixture["name"].as_str().unwrap();
        let root = direct_file_pair(name);
        let extension = if name == "createTabbedFilePair" {
            "txt"
        } else {
            "ts"
        };
        for side in ["before", "after"] {
            let bytes = std::fs::read(root.path().join(format!("{side}.{extension}"))).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(bytes)),
                fixture[side],
                "{name} {side}"
            );
        }
        assert_eq!(
            root.path().join(".git").exists(),
            name == "createWatchFilePair"
        );
        if name == "createWatchFilePair" {
            assert_eq!(git(root.path(), &["ls-files"]), "");
        }
    }
}

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
