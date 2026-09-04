use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{
    Changeset, ChangesetSource, FileSourceSnapshots, SourceOrigin, SourceSnapshot,
};
use workdeck_diff::parse_patch;
use workdeck_extension_api::Registration;
use workdeck_extension_host::{HostError, LoadedExtension};
use workdeck_tui::{ReviewApp, ReviewOptions, ratatui_theme_color, render};

fn staged_extension() -> (TempDir, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-line-highlighter-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-line-highlighter-extension"),
        binary_directory.join(binary_name),
    )
    .unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/line-highlighter/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn review_changeset(path: &str) -> Changeset {
    let mut changeset = parse_patch(
        &format!(
            "diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-old\n+new\n"
        ),
        "line-highlighter",
        "Line highlighter",
        ChangesetSource::Patch {
            label: "line-highlighter".into(),
        },
    )
    .unwrap();
    changeset.files[0].sources = FileSourceSnapshots {
        old: Some(SourceSnapshot::new(
            "old\n".into(),
            SourceOrigin::Revision {
                revision: "HEAD".into(),
            },
            true,
        )),
        new: Some(SourceSnapshot::new(
            "new\n".into(),
            SourceOrigin::WorkingTree,
            true,
        )),
    };
    changeset
}

fn review_file(path: &str) -> workdeck_core::DiffFile {
    review_changeset(path).files.remove(0)
}

#[test]
fn native_line_highlighter_receives_frozen_documents_and_returns_declarative_marks() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test").unwrap();
    assert!(
        extension
            .handshake
            .registrations
            .iter()
            .any(|registration| {
                matches!(registration, Registration::LineHighlighter { id } if id == "attention")
            })
    );

    let result = extension
        .highlight_file("attention", &review_file("request.rs"))
        .unwrap();
    assert_eq!(
        result,
        serde_json::json!([{
            "side": "new",
            "line": 1,
            "range": [0, 3],
            "tone": "warning"
        }])
    );
    assert!(!extension.request_pending());
    assert!(
        extension
            .highlight_file("attention", &review_file("request.rs"))
            .is_ok()
    );
}

#[test]
fn compiled_highlighter_marks_reach_the_live_ratatui_cell_buffer() {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({ "includeHang": false }),
    )
    .unwrap();
    let options = ReviewOptions {
        sidebar: false,
        line_numbers: false,
        highlight: false,
        ..ReviewOptions::default()
    };
    let expected_base = ratatui_theme_color(&options.theme.added_content_bg);
    let app =
        ReviewApp::new_with_extensions(review_changeset("request.rs"), options, vec![extension]);
    let backend = TestBackend::new(100, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut observed = Vec::new();
    loop {
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut painted = false;
        observed.clear();
        for y in buffer.area.y..buffer.area.bottom() {
            for x in buffer.area.x..buffer.area.right().saturating_sub(2) {
                let Some(first) = buffer.cell((x, y)) else {
                    continue;
                };
                if first.symbol() == "n"
                    && buffer
                        .cell((x + 1, y))
                        .is_some_and(|cell| cell.symbol() == "e")
                    && buffer
                        .cell((x + 2, y))
                        .is_some_and(|cell| cell.symbol() == "w")
                {
                    let second = buffer.cell((x + 1, y)).unwrap();
                    let third = buffer.cell((x + 2, y)).unwrap();
                    observed.push(format!(
                        "({x},{y})={:?}/{:?}/{:?} expected-base={expected_base:?}",
                        first.bg, second.bg, third.bg
                    ));
                    painted =
                        first.bg != expected_base && second.bg == first.bg && third.bg == first.bg;
                }
            }
        }
        if painted {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "native marks did not reach the terminal cell buffer; observed {}",
            observed.join(", ")
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn hung_native_line_highlighter_times_out_without_holding_the_caller_forever() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test").unwrap();
    let started = Instant::now();
    let error = extension
        .highlight_file("hang", &review_file("hang.rs"))
        .unwrap_err();
    assert!(matches!(error, HostError::Timeout(_)));
    assert!(started.elapsed() >= Duration::from_millis(1_400));
    assert!(started.elapsed() < Duration::from_secs(3));
    extension.retire();
}

#[test]
fn superseded_native_line_highlighter_observes_cancellation_promptly() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test").unwrap();
    let mut request = extension.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let request_cancelled = Arc::clone(&cancelled);
    let file = review_file("hang.rs");
    let started = Instant::now();
    let pending = std::thread::spawn(move || {
        request.highlight_file_cancellable("hang", &file, &request_cancelled)
    });
    std::thread::sleep(Duration::from_millis(50));
    cancelled.store(true, Ordering::Release);
    assert!(matches!(
        pending.join().unwrap(),
        Err(HostError::Cancelled(_))
    ));
    assert!(started.elapsed() < Duration::from_millis(500));
    extension.retire();
}
