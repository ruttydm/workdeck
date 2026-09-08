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
fn exited_native_transport_is_closed_not_retryable_busy() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({"exitOnHighlight":true}),
    )
    .unwrap();
    for _ in 0..2 {
        assert!(matches!(
            extension.highlight_file("attention", &review_file("request.rs")),
            Err(HostError::Closed(_))
        ));
    }
}

#[test]
fn four_native_parents_share_one_child_and_receive_reversed_responses() {
    assert_four_native_parents(false, false, false);
}

#[test]
fn cancelling_one_native_parent_preserves_other_parent_results() {
    assert_four_native_parents(true, false, false);
}

#[test]
fn concurrent_native_document_callbacks_keep_parent_source_authority_separate() {
    assert_four_native_parents(false, true, false);
}

#[test]
fn concurrent_native_document_failure_does_not_replace_peer_sources() {
    assert_four_native_parents(false, true, true);
}

fn assert_four_native_parents(cancel_one: bool, documents: bool, fail_one: bool) {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({"includeHang":false,"batchFour":true,"cancelBatch":cancel_one,"batchDocuments":documents}),
    )
    .unwrap();
    let cancel_second = Arc::new(AtomicBool::new(false));
    let workers = (0..4)
        .map(|index| {
            let mut extension = extension.clone();
            let cancel_second = cancel_second.clone();
            std::thread::spawn(move || {
                let path = format!("file-{index}.rs");
                let file = review_file(&path);
                let deadline = Instant::now() + Duration::from_secs(5);
                let source_text = path.clone();
                let reader = workdeck_extension_host::ExtensionDocumentReader::new(move |side| {
                    assert_eq!(side, workdeck_extension_api::ExtensionFileSide::New);
                    if fail_one && index == 1 {
                        return Err("one captured provider failed".into());
                    }
                    Ok(Some(source_text.clone()))
                });
                loop {
                    let not_cancelled = AtomicBool::new(false);
                    let result = if documents {
                        extension.highlight_file_with_document_reader(
                            "attention",
                            &file,
                            &not_cancelled,
                            reader.clone(),
                        )
                    } else {
                        extension.highlight_file_cancellable(
                            "attention",
                            &file,
                            if index == 1 {
                                &cancel_second
                            } else {
                                &not_cancelled
                            },
                        )
                    };
                    match result {
                        Err(HostError::Busy(_)) if Instant::now() < deadline => {
                            std::thread::yield_now()
                        }
                        result => {
                            if cancel_one && index == 1 {
                                assert!(matches!(result, Err(HostError::Cancelled(_))));
                                break;
                            }
                            let expected = if documents {
                                let text = if fail_one && index == 1 {
                                    None
                                } else {
                                    Some(&path)
                                };
                                serde_json::json!({"path":path,"text":text,"simultaneous":4})
                            } else {
                                serde_json::json!({"path":path,"simultaneous":4})
                            };
                            assert_eq!(result.unwrap(), expected);
                            if cancel_one && index == 0 {
                                cancel_second.store(true, Ordering::Release);
                            }
                            break;
                        }
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn cancelling_lazy_read_keeps_host_responsive_and_shared_read_alive() {
    assert_retired_lazy_read_keeps_shared_read_alive(true);
}

#[test]
fn timed_out_lazy_read_keeps_host_responsive_and_shared_read_alive() {
    assert_retired_lazy_read_keeps_shared_read_alive(false);
}

fn assert_retired_lazy_read_keeps_shared_read_alive(cancel: bool) {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test").unwrap();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let release_rx = std::sync::Mutex::new(release_rx);
    let reader = workdeck_extension_host::ExtensionDocumentReader::new(move |_| {
        started_tx.send(()).unwrap();
        release_rx
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        Ok(Some("new\n".into()))
    });
    let retained = reader.clone();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancelled.clone();
    let mut worker_extension = extension.clone();
    let worker = std::thread::spawn(move || {
        worker_extension.highlight_file_with_document_reader(
            "attention",
            &review_file("request.rs"),
            &worker_cancelled,
            reader,
        )
    });
    started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let started = Instant::now();
    if cancel {
        cancelled.store(true, Ordering::Release);
    }
    let result = worker.join().unwrap();
    if cancel {
        assert!(matches!(result, Err(HostError::Cancelled(_))));
        assert!(started.elapsed() < Duration::from_millis(500));
    } else {
        assert!(matches!(result, Err(HostError::Timeout(_))));
        assert!(started.elapsed() < Duration::from_secs(3));
    }
    assert!(
        extension
            .highlight_file("attention", &review_file("request.rs"))
            .is_ok()
    );
    release_tx.send(()).unwrap();
    assert_eq!(
        retained
            .read_document(workdeck_extension_api::ExtensionFileSide::New)
            .wait_until(
                &workdeck_extension_host::ExtensionRequestCancellation::default(),
                Instant::now() + Duration::from_secs(5)
            )
            .unwrap(),
        Some("new\n".into())
    );
    assert!(
        extension
            .highlight_file("attention", &review_file("request.rs"))
            .is_ok()
    );
}

#[test]
fn lazy_native_highlighter_reads_only_requested_side_once_per_parent() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test").unwrap();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = reads.clone();
    let reader = workdeck_extension_host::ExtensionDocumentReader::new(move |side| {
        assert_eq!(side, workdeck_extension_api::ExtensionFileSide::New);
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(Some("new\n".into()))
    });
    let mut file = review_file("request.rs");
    file.sources = FileSourceSnapshots::default();
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let marks = extension
        .highlight_file_with_document_reader("attention", &file, &AtomicBool::new(false), reader)
        .unwrap();
    assert_eq!(marks[0]["range"], serde_json::json!([0, 3]));
    assert_eq!(reads.load(Ordering::SeqCst), 1);
    assert!(
        extension
            .highlight_file("attention", &review_file("request.rs"))
            .is_ok()
    );
}

#[test]
fn native_highlighter_without_document_requests_never_starts_source_io() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({"includeHang": false, "requireCleanup": true, "skipDocuments": true}),
    )
    .unwrap();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = reads.clone();
    let reader = workdeck_extension_host::ExtensionDocumentReader::new(move |_| {
        observed.fetch_add(1, Ordering::SeqCst);
        Err("source must not be read".into())
    });
    let mut file = review_file("request.rs");
    file.sources = FileSourceSnapshots::default();
    for _ in 0..2 {
        let result = extension
            .highlight_file_with_document_reader(
                "attention",
                &file,
                &AtomicBool::new(false),
                reader.clone(),
            )
            .unwrap();
        assert_eq!(result, serde_json::json!([]));
        assert!(!extension.request_pending());
    }
    assert_eq!(reads.load(Ordering::SeqCst), 0);
}

#[test]
fn failed_native_document_reads_are_null_and_deduplicated_per_parent() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({"includeHang": false, "requireCleanup": true, "expectMissing": true}),
    )
    .unwrap();
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let file = review_file("request.rs");
    for expected in 1..=2 {
        let observed = reads.clone();
        let reader = workdeck_extension_host::ExtensionDocumentReader::new(move |side| {
            assert_eq!(side, workdeck_extension_api::ExtensionFileSide::New);
            observed.fetch_add(1, Ordering::SeqCst);
            Err("provider unavailable".into())
        });
        let result = extension
            .highlight_file_with_document_reader(
                "attention",
                &file,
                &AtomicBool::new(false),
                reader,
            )
            .unwrap();
        assert_eq!(result, serde_json::json!([]));
        assert_eq!(reads.load(Ordering::SeqCst), expected);
        assert!(!extension.request_pending());
    }
}

#[test]
fn native_request_cleanup_precedes_the_next_call_after_success_or_failure() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({ "includeHang": false, "requireCleanup": true }),
    )
    .unwrap();
    let file = review_file("request.rs");
    assert!(extension.highlight_file("attention", &file).is_ok());
    assert!(extension.highlight_file("attention", &file).is_ok());
    let mut invalid = file.clone();
    invalid.sources = FileSourceSnapshots::default();
    let error = extension.highlight_file("attention", &invalid).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("expected immutable old/new documents")
    );
    assert!(extension.highlight_file("attention", &file).is_ok());
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
fn saved_note_changes_reach_native_highlighter_and_terminal_marks() {
    for deferred in [false, true] {
        assert_saved_note_changes_reach_native_highlighter(deferred);
    }
}

fn assert_saved_note_changes_reach_native_highlighter(deferred: bool) {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({ "includeHang": false, "markAnnotations": true }),
    )
    .unwrap();
    let initial = review_changeset("request.rs");
    let reads = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let (document, source_capabilities) = if deferred {
        let reads = Arc::clone(&reads);
        let (document, sources) = workdeck_vcs::materialize_vcs_patch_result_deferred(
            workdeck_vcs::VcsPatchResult {
                repo_root: ".".into(),
                source_label: "notes".into(),
                title: "notes".into(),
                patch_text: initial.files[0].patch.clone(),
                untracked_paths: vec![],
                extra_files: vec![],
                source_cache_key: Some("notes-snapshot".into()),
                source_reader: Some(Arc::new(move |request| {
                    assert_eq!(request.path, "request.rs");
                    assert_eq!(request.side, workdeck_core::ReviewSide::New);
                    reads.fetch_add(1, Ordering::SeqCst);
                    Ok(workdeck_vcs::VcsFileSourceResult::Source(
                        SourceSnapshot::new(
                            match request.side {
                                workdeck_core::ReviewSide::Old => "old\n",
                                workdeck_core::ReviewSide::New => "new\n",
                            }
                            .into(),
                            SourceOrigin::WorkingTree,
                            true,
                        ),
                    ))
                })),
            },
            "notes",
            ChangesetSource::WorkingTree { staged: false },
        )
        .unwrap();
        (document, Some(sources))
    } else {
        (initial, None)
    };
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let mut probe = extension.clone();
    let options = ReviewOptions {
        sidebar: false,
        line_numbers: false,
        highlight: false,
        source_capabilities,
        ..ReviewOptions::default()
    };
    let base = ratatui_theme_color(&options.theme.added_content_bg);
    let app = ReviewApp::new_with_extensions(document.clone(), options, vec![extension]);
    assert_eq!(reads.load(Ordering::SeqCst), 0);
    let state = app.shared_state();
    let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
    let note = workdeck_review::build_live_comment(
        &document.files[0],
        workdeck_review::CommentTargetInput {
            file_path: "request.rs".into(),
            hunk_index: Some(0),
            side: None,
            line: None,
            summary: "x".into(),
            rationale: None,
            markup: None,
            author: None,
        },
        "live-note".into(),
        "2026-09-08T00:00:00Z".into(),
        workdeck_review::ResolvedCommentTarget {
            hunk_index: 0,
            side: workdeck_core::ReviewSide::New,
            line: 1,
        },
    );
    state.lock().unwrap().add_comment(note.clone()).unwrap();
    for (phase, width) in [1, 2, 0, 1].into_iter().enumerate() {
        if phase == 3 {
            let mut draft = note.clone();
            draft.id = "draft".into();
            draft.source = "user-draft".into();
            let mut orphan = note.clone();
            orphan.id = "orphan".into();
            orphan.resolution = workdeck_review::ReviewNoteResolution::Orphaned;
            let mut notes = state.lock().unwrap();
            notes.add_comment(draft).unwrap();
            notes.add_comment(orphan).unwrap();
            notes.add_comment(note.clone()).unwrap();
        }
        if width == 2 {
            state
                .lock()
                .unwrap()
                .edit_comment_summary("live-note", "xx".into())
                .unwrap();
        } else if width == 0 {
            state.lock().unwrap().remove_comment("live-note").unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            terminal
                .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let mut matched = false;
            for y in 0..buffer.area.height {
                for x in 0..buffer.area.width.saturating_sub(2) {
                    if (0..3)
                        .map(|offset| buffer.cell((x + offset, y)).unwrap().symbol())
                        .collect::<String>()
                        == "new"
                    {
                        matched |= (0..3).all(|offset| {
                            (buffer.cell((x + offset, y)).unwrap().bg != base)
                                == (usize::from(offset) < width)
                        });
                    }
                }
            }
            if matched
                && probe
                    .request(
                        "example/last-annotation-width",
                        serde_json::json!({}),
                        Duration::from_millis(500),
                    )
                    .is_ok_and(|observed| observed == serde_json::json!(width))
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "note-derived highlight width {width} did not render"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    assert_eq!(state.lock().unwrap().changeset(), &document);
    assert_eq!(reads.load(Ordering::SeqCst), if deferred { 1 } else { 0 });
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
    let mut extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({ "requireCleanup": true }),
    )
    .unwrap();
    let started = Instant::now();
    let error = extension
        .highlight_file("hang", &review_file("hang.rs"))
        .unwrap_err();
    assert!(matches!(error, HostError::Timeout(_)));
    assert!(started.elapsed() >= Duration::from_millis(1_400));
    assert!(started.elapsed() < Duration::from_secs(3));
    assert!(
        extension
            .highlight_file("attention", &review_file("request.rs"))
            .is_ok()
    );
    extension.retire();
}

#[test]
fn superseded_native_line_highlighter_observes_cancellation_promptly() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn_with_configuration(
        &manifest,
        "test",
        serde_json::json!({ "requireCleanup": true }),
    )
    .unwrap();
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
    assert!(
        extension
            .highlight_file("attention", &review_file("request.rs"))
            .is_ok()
    );
    extension.retire();
}
