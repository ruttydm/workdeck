use super::{panel_controller::RepositoryPanels, *};
use std::{
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
enum Read {
    Load(
        PanelRequest,
        mpsc::Sender<Result<PanelSnapshot, PanelError>>,
    ),
    Preview(PanelTarget, mpsc::Sender<Result<PanelPreview, PanelError>>),
}
#[derive(Debug)]
struct Controlled {
    source: Mutex<RepositoryPanelSource>,
    reads: mpsc::Sender<Read>,
}
impl RepositoryPanelProvider for Controlled {
    fn source(&self) -> RepositoryPanelSource {
        self.source.lock().unwrap().clone()
    }
    fn load(&self, request: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
        let (send, receive) = mpsc::channel();
        self.reads.send(Read::Load(request.clone(), send)).unwrap();
        receive
            .recv()
            .unwrap_or_else(|_| Err(PanelError::new("Controlled read canceled")))
    }
    fn preview(&self, target: &PanelTarget) -> Result<PanelPreview, PanelError> {
        let (send, receive) = mpsc::channel();
        self.reads
            .send(Read::Preview(target.clone(), send))
            .unwrap();
        receive
            .recv()
            .unwrap_or_else(|_| Err(PanelError::new("Controlled read canceled")))
    }
}
fn controlled() -> (Arc<Controlled>, RepositoryPanels, mpsc::Receiver<Read>) {
    let (send, receive) = mpsc::channel();
    let provider = Arc::new(Controlled {
        source: Mutex::new(RepositoryPanelSource {
            root: "/repository".into(),
            identity: "repository-one".into(),
        }),
        reads: send,
    });
    let panels = RepositoryPanels::new(provider.clone());
    (provider, panels, receive)
}
fn entry(id: &str, target: PanelTarget) -> PanelEntry {
    PanelEntry {
        id: id.into(),
        label: id.into(),
        detail: String::new(),
        section: String::new(),
        target,
        changes: None,
    }
}
fn directory(id: &str) -> PanelEntry {
    entry(id, PanelTarget::Directory { path: id.into() })
}
fn file(id: &str) -> PanelEntry {
    entry(
        id,
        PanelTarget::File {
            path: id.into(),
            line: None,
        },
    )
}
fn snapshot(page: PanelPage, entries: Vec<PanelEntry>) -> PanelSnapshot {
    PanelSnapshot {
        page,
        title: page.title().into(),
        summary: "Repository data".into(),
        entries,
        truncated: false,
    }
}
fn preview(body: &str) -> PanelPreview {
    PanelPreview {
        title: "Preview".into(),
        body: body.into(),
        kind: PanelPreviewKind::Source,
        truncated: false,
        binary: false,
    }
}
fn poll_until(panels: &mut RepositoryPanels, done: impl Fn(&RepositoryPanels) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        panels.poll();
        if done(panels) {
            return;
        }
        assert!(Instant::now() < deadline, "Timed out: {panels:?}");
        thread::sleep(Duration::from_millis(1));
    }
}
fn read(receiver: &mpsc::Receiver<Read>) -> Read {
    receiver.recv_timeout(Duration::from_secs(3)).unwrap()
}
fn load_response(
    receiver: &mpsc::Receiver<Read>,
) -> (
    PanelRequest,
    mpsc::Sender<Result<PanelSnapshot, PanelError>>,
) {
    let Read::Load(request, send) = read(receiver) else {
        panic!("expected load")
    };
    (request, send)
}

#[test]
fn pending_reads_coalesce_and_old_query_results_never_publish() {
    let (_, mut panels, reads) = controlled();
    panels.open(PanelPage::Search);
    let (_, first) = load_response(&reads);
    panels.state(PanelPage::Search).query = "discarded".into();
    panels.refresh(PanelPage::Search);
    panels.state(PanelPage::Search).query = "latest".into();
    panels.refresh(PanelPage::Search);
    first
        .send(Ok(snapshot(PanelPage::Search, vec![directory("stale")])))
        .unwrap();
    poll_until(&mut panels, |panels| {
        panels.pages[&PanelPage::Search].snapshot.is_none()
            && !panels.pages[&PanelPage::Search].query.is_empty()
    });
    // Pump the completion until the newest queued read has been dispatched.
    let deadline = Instant::now() + Duration::from_secs(3);
    let (request, send) = loop {
        panels.poll();
        if let Ok(Read::Load(request, send)) = reads.try_recv() {
            break (request, send);
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    assert_eq!(request.query, "latest");
    assert!(panels.state(PanelPage::Search).snapshot.is_none());
    send.send(Ok(snapshot(PanelPage::Search, vec![directory("latest")])))
        .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Search].loading);
    assert_eq!(
        panels.state(PanelPage::Search).selected().unwrap().id,
        "latest"
    );
}

#[test]
fn repository_identity_change_rejects_in_flight_result() {
    let (provider, mut panels, reads) = controlled();
    panels.open(PanelPage::Files);
    let (_, send) = load_response(&reads);
    provider.source.lock().unwrap().identity = "different repository".into();
    send.send(Ok(snapshot(PanelPage::Files, vec![directory("wrong")])))
        .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
    assert!(panels.state(PanelPage::Files).snapshot.is_none());
    assert!(
        panels
            .state(PanelPage::Files)
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("source changed")
    );
}

#[test]
fn refresh_reloads_selected_preview_and_retains_selection() {
    let (_, mut panels, reads) = controlled();
    panels.open(PanelPage::Files);
    let (_, send) = load_response(&reads);
    send.send(Ok(snapshot(
        PanelPage::Files,
        vec![file("one"), file("two")],
    )))
    .unwrap();
    poll_until(&mut panels, |p| {
        p.pages[&PanelPage::Files].snapshot.is_some()
    });
    let Read::Preview(target, send) = read(&reads) else {
        panic!()
    };
    assert_eq!(target, file("one").target);
    send.send(Ok(preview("old bytes"))).unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].preview_loading);
    panels.refresh(PanelPage::Files);
    let (_, send) = load_response(&reads);
    send.send(Ok(snapshot(
        PanelPage::Files,
        vec![file("two"), file("one")],
    )))
    .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
    assert_eq!(panels.state(PanelPage::Files).selected().unwrap().id, "one");
    let Read::Preview(_, send) = read(&reads) else {
        panic!()
    };
    send.send(Ok(preview("new bytes"))).unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].preview_loading);
    assert_eq!(
        panels
            .state(PanelPage::Files)
            .preview
            .as_ref()
            .unwrap()
            .1
            .body,
        "new bytes"
    );
}

#[test]
fn duplicate_snapshot_identifiers_are_rejected_without_losing_previous_data() {
    let (_, mut panels, reads) = controlled();
    panels.open(PanelPage::Files);
    let (_, send) = load_response(&reads);
    send.send(Ok(snapshot(
        PanelPage::Files,
        vec![directory("same"), directory("same")],
    )))
    .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
    assert!(panels.state(PanelPage::Files).snapshot.is_none());
    assert!(
        panels
            .state(PanelPage::Files)
            .error
            .as_ref()
            .unwrap()
            .message
            .contains("duplicate")
    );
}

#[test]
fn directory_history_restores_selection_and_preview_scroll() {
    let (_, mut panels, reads) = controlled();
    panels.open(PanelPage::Files);
    let (_, send) = load_response(&reads);
    send.send(Ok(snapshot(
        PanelPage::Files,
        vec![directory("a"), directory("b")],
    )))
    .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
    panels.select(PanelPage::Files, "b");
    panels.state(PanelPage::Files).location.preview_scroll = 9;
    panels.set_directory(PanelPage::Files, "b".into());
    let (request, send) = load_response(&reads);
    assert_eq!(request.directory, "b");
    send.send(Ok(snapshot(PanelPage::Files, vec![directory("b/child")])))
        .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
    assert_eq!(
        panels
            .state(PanelPage::Files)
            .snapshot
            .as_ref()
            .unwrap()
            .entries[0]
            .label,
        ".."
    );
    panels.set_directory(PanelPage::Files, String::new());
    let (_, send) = load_response(&reads);
    send.send(Ok(snapshot(
        PanelPage::Files,
        vec![directory("a"), directory("b")],
    )))
    .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
    assert_eq!(panels.state(PanelPage::Files).selected().unwrap().id, "b");
    assert_eq!(panels.state(PanelPage::Files).location.preview_scroll, 9);
}

#[test]
fn late_preview_cannot_replace_current_selected_target() {
    let (_, mut panels, reads) = controlled();
    panels.open(PanelPage::Files);
    let (_, send) = load_response(&reads);
    send.send(Ok(snapshot(PanelPage::Files, vec![file("a"), file("b")])))
        .unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
    let Read::Preview(target, old) = read(&reads) else {
        panic!()
    };
    assert_eq!(target, file("a").target);
    panels.select(PanelPage::Files, "b");
    old.send(Ok(preview("wrong selected file"))).unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    let send = loop {
        panels.poll();
        if let Ok(Read::Preview(target, send)) = reads.try_recv() {
            assert_eq!(target, file("b").target);
            break send;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    };
    assert!(panels.state(PanelPage::Files).preview.is_none());
    send.send(Ok(preview("selected file b"))).unwrap();
    poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].preview_loading);
    assert_eq!(
        panels
            .state(PanelPage::Files)
            .preview
            .as_ref()
            .unwrap()
            .1
            .body,
        "selected file b"
    );
}

fn review_app(provider: Arc<dyn RepositoryPanelProvider>) -> crate::ReviewApp {
    let root = provider.source().root;
    let changeset = workdeck_diff::changeset_from_patch(
        "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1 +1 @@\n-old\n+native review canvas\n",
        "Review",
        "Review",
        "test",
        workdeck_core::ChangesetSource::WorkingTree { staged: false },
        None,
    );
    crate::ReviewApp::new(
        changeset,
        crate::ReviewOptions {
            workbench: Some(WorkbenchOptions::new(root.clone())),
            repository_panels: Some(provider),
            repo: Some(root),
            highlight: false,
            ..Default::default()
        },
    )
}
fn key(app: &mut crate::ReviewApp, code: crossterm::event::KeyCode) {
    app.handle_key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::NONE,
    ));
}
fn screen(app: &crate::ReviewApp, width: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, 32)).unwrap();
    terminal
        .draw(|frame| crate::render(frame.area(), frame.buffer_mut(), app))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}
fn poll_app(app: &mut crate::ReviewApp, page: PanelPage) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        app.poll_workbench();
        let ready = {
            let shell = app.workbench.as_ref().unwrap().lock().unwrap();
            let state = &shell.panels.as_ref().unwrap().pages[&page];
            !state.loading && !state.preview_loading
        };
        if ready {
            return;
        }
        assert!(Instant::now() < deadline);
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn blocked_or_failed_repository_read_keeps_review_issues_and_native_modal_responsive() {
    use crossterm::event::KeyCode;
    let (provider, _, reads) = controlled();
    let mut app = review_app(provider);
    let document = app.with_state(|state| state.changeset_snapshot());
    key(&mut app, KeyCode::F(5));
    let (_, send) = load_response(&reads);
    assert!(screen(&app, 110).contains("loading"));
    key(&mut app, KeyCode::F(2));
    assert!(screen(&app, 110).contains("native review canvas"));
    assert!(Arc::ptr_eq(
        &document,
        &app.with_state(|state| state.changeset_snapshot())
    ));
    app.show_help = true;
    key(&mut app, KeyCode::F(7));
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Review
    );
    app.show_help = false;
    key(&mut app, KeyCode::F(3));
    assert!(screen(&app, 90).contains("source unavailable"));
    send.send(Err(PanelError::new("Controlled provider failure")))
        .unwrap();
    poll_app(&mut app, PanelPage::Changes);
    key(&mut app, KeyCode::F(5));
    assert!(screen(&app, 110).contains("Controlled provider failure"));
}

#[derive(Debug)]
struct Immediate {
    root: std::path::PathBuf,
}
impl RepositoryPanelProvider for Immediate {
    fn source(&self) -> RepositoryPanelSource {
        RepositoryPanelSource {
            root: self.root.clone(),
            identity: "fixture".into(),
        }
    }
    fn load(&self, request: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
        let mut entries = match request.page {
            PanelPage::Files => {
                if request.directory.is_empty() {
                    vec![directory("src"), file("root.rs")]
                } else {
                    vec![file("src/one.rs"), file("src/two.rs")]
                }
            }
            PanelPage::Changes => vec![entry(
                "a.rs",
                PanelTarget::Change {
                    path: "a.rs".into(),
                    staged: false,
                },
            )],
            PanelPage::Git => vec![
                entry(
                    "commit",
                    PanelTarget::Commit {
                        reference: "HEAD".into(),
                    },
                ),
                entry(
                    "branch",
                    PanelTarget::Branch {
                        reference: "topic".into(),
                    },
                ),
                entry(
                    "stash",
                    PanelTarget::Stash {
                        reference: "stash@{0}".into(),
                    },
                ),
            ],
            PanelPage::Agents => vec![entry(
                "recorded",
                PanelTarget::AgentSession {
                    id: "historical".into(),
                },
            )],
            PanelPage::Search => vec![entry(
                "symbol",
                PanelTarget::File {
                    path: "src/one.rs".into(),
                    line: Some(20),
                },
            )],
        };
        entries.retain(|entry| entry.label.contains(&request.query));
        Ok(snapshot(request.page, entries))
    }
    fn preview(&self, target: &PanelTarget) -> Result<PanelPreview, PanelError> {
        Ok(preview(&format!(
            "Preview content for {target:?}\nsecond preview line"
        )))
    }
}

#[test]
fn repository_copy_uses_selected_reference_and_native_clipboard_feedback() {
    use crossterm::event::KeyCode;
    let directory = tempfile::tempdir().unwrap();
    let mut app = review_app(Arc::new(Immediate {
        root: directory.path().into(),
    }));
    app.set_clipboard_copy_supported(true);
    key(&mut app, KeyCode::F(7));
    poll_app(&mut app, PanelPage::Files);
    key(&mut app, KeyCode::End);
    poll_app(&mut app, PanelPage::Files);
    key(&mut app, KeyCode::Char('y'));
    app.process_workbench_effect(&mut |_, _, _| panic!("copy must not reload"));
    assert_eq!(
        app.take_clipboard_copy_request().as_deref(),
        Some("root.rs")
    );
    app.report_clipboard_copy_failure("controlled clipboard failure");
    assert!(screen(&app, 110).contains("controlled clipboard failure"));
    key(&mut app, KeyCode::F(6));
    poll_app(&mut app, PanelPage::Git);
    key(&mut app, KeyCode::End);
    poll_app(&mut app, PanelPage::Git);
    key(&mut app, KeyCode::Char('y'));
    app.process_workbench_effect(&mut |_, _, _| panic!("copy must not reload"));
    assert_eq!(
        app.take_clipboard_copy_request().as_deref(),
        Some("stash@{0}")
    );
    assert!(!directory.path().join(".workdeck").exists());
    assert!(!directory.path().join(".agents").exists());
}

#[test]
fn mounted_panels_keep_directory_query_selection_preview_scroll_and_drafts() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let dir = tempfile::tempdir().unwrap();
    workdeck_pm::Repository::init(dir.path(), "WD").unwrap();
    let mut app = review_app(Arc::new(Immediate {
        root: dir.path().into(),
    }));
    key(&mut app, KeyCode::F(3));
    key(&mut app, KeyCode::Char('n'));
    for ch in "Retained planning draft".chars() {
        key(&mut app, KeyCode::Char(ch));
    }
    key(&mut app, KeyCode::F(7));
    poll_app(&mut app, PanelPage::Files);
    key(&mut app, KeyCode::Enter);
    poll_app(&mut app, PanelPage::Files);
    key(&mut app, KeyCode::Down);
    poll_app(&mut app, PanelPage::Files);
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::PageDown);
    {
        let shell = app.workbench.as_ref().unwrap().lock().unwrap();
        let state = &shell.panels.as_ref().unwrap().pages[&PanelPage::Files];
        assert_eq!(state.directory, "src");
        assert_eq!(state.location.selected.as_deref(), Some("src/one.rs"));
        assert_eq!(state.location.preview_scroll, 12);
    }
    key(&mut app, KeyCode::F(3));
    assert!(screen(&app, 110).contains("Retained planning draft"));
    key(&mut app, KeyCode::F(7));
    key(&mut app, KeyCode::Char('/'));
    app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for ch in "two".chars() {
        key(&mut app, KeyCode::Char(ch));
    }
    key(&mut app, KeyCode::Enter);
    poll_app(&mut app, PanelPage::Files);
    assert!(screen(&app, 110).contains("/ two"));
    assert!(screen(&app, 70).contains("Preview"));
    key(&mut app, KeyCode::Esc);
    assert!(screen(&app, 70).contains("src/two.rs"));
    key(&mut app, KeyCode::F(8));
    poll_app(&mut app, PanelPage::Agents);
    assert!(screen(&app, 110).contains("recorded sessions are read-only"));
    key(&mut app, KeyCode::F(9));
    poll_app(&mut app, PanelPage::Search);
    assert!(screen(&app, 110).contains("symbol"));
    key(&mut app, KeyCode::F(7));
    assert!(screen(&app, 110).contains("/ two"));
}

#[test]
fn repository_navigation_uses_core_reload_callback_and_preserves_return_state() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let mut app = review_app(Arc::new(Immediate {
        root: dir.path().into(),
    }));
    for (page, key_number, index) in [
        (PanelPage::Git, 6, 0),
        (PanelPage::Git, 6, 1),
        (PanelPage::Git, 6, 2),
        (PanelPage::Files, 7, 1),
        (PanelPage::Search, 9, 0),
    ] {
        key(&mut app, KeyCode::F(key_number));
        app.process_workbench_effect(&mut |_, _, _| Ok(()));
        poll_app(&mut app, page);
        {
            let mut shell = app.workbench.as_ref().unwrap().lock().unwrap();
            shell
                .panels
                .as_mut()
                .unwrap()
                .move_selection(page, isize::MIN);
            shell.panels.as_mut().unwrap().move_selection(page, index);
        }
        let before = {
            let shell = app.workbench.as_ref().unwrap().lock().unwrap();
            let state = &shell.panels.as_ref().unwrap().pages[&page];
            state.location.selected.clone()
        };
        key(&mut app, KeyCode::Char('v'));
        let mut captured = None;
        app.process_workbench_effect(&mut |_, input, root| {
            assert_eq!(root, dir.path());
            captured = Some(input.clone());
            Err("controlled reload boundary".into())
        });
        let input = captured.unwrap();
        match (page, index, input) {
            (PanelPage::Git, 0, workdeck_core::CliInput::Show(_))
            | (PanelPage::Git, 1, workdeck_core::CliInput::Vcs(_))
            | (PanelPage::Git, 2, workdeck_core::CliInput::StashShow(_))
            | (PanelPage::Files, 1, workdeck_core::CliInput::Files(_))
            | (PanelPage::Search, 0, workdeck_core::CliInput::Files(_)) => {}
            other => panic!("Unexpected core route: {other:?}"),
        }
        let shell = app.workbench.as_ref().unwrap().lock().unwrap();
        assert_eq!(shell.tab, WorkbenchTab::Repository(page));
        assert_eq!(
            shell.panels.as_ref().unwrap().pages[&page]
                .location
                .selected,
            before
        );
    }
}

#[test]
fn cross_repository_review_cannot_create_linked_issue_in_original_workbench() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let repository = workdeck_pm::Repository::init(dir.path(), "WD").unwrap();
    let mut app = review_app(Arc::new(Immediate {
        root: dir.path().into(),
    }));
    app.options.repo = Some("/another-repository".into());
    key(&mut app, KeyCode::F(4));
    app.process_workbench_effect(&mut |_, _, _| panic!("must not reload"));
    assert!(repository.list_issues().unwrap().is_empty());
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    assert!(shell.controller.drafts().is_empty());
    assert!(
        shell
            .notice
            .as_ref()
            .unwrap()
            .contains("another repository")
    );
}

#[test]
fn trust_dialog_displays_supplied_native_or_legacy_directory() {
    for directory in ["/r/.workdeck/extensions", "/r/.agents/workdeck/extensions"] {
        let mut app = review_app(Arc::new(Immediate { root: "/r".into() }));
        app.options.pending_extension_trust_directory = Some(directory.into());
        app.reconcile_extension_trust_repo_root(Some("/r".into()));
        let text = screen(&app, 110);
        assert!(
            text.contains(directory.strip_prefix("/r/").unwrap_or(directory)),
            "{text}"
        );
    }
}

#[test]
fn long_panel_lists_keep_selected_rows_visible_and_changes_controls_retain_state() {
    use crossterm::event::KeyCode;
    #[derive(Debug)]
    struct LongList;
    impl RepositoryPanelProvider for LongList {
        fn source(&self) -> RepositoryPanelSource {
            RepositoryPanelSource {
                root: "/repository".into(),
                identity: "long-list".into(),
            }
        }
        fn load(&self, request: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
            Ok(snapshot(
                request.page,
                (0..80)
                    .map(|index| {
                        let mut row = file(&format!("src/file{index:03}.rs"));
                        row.section = "src/".into();
                        row.changes = Some(PanelChangeStats {
                            additions: 2,
                            deletions: 1,
                            ..Default::default()
                        });
                        row
                    })
                    .collect(),
            ))
        }
        fn preview(&self, _: &PanelTarget) -> Result<PanelPreview, PanelError> {
            Ok(preview("Selected source preview"))
        }
    }
    let mut app = review_app(Arc::new(LongList));
    for (page, shortcut) in [
        (PanelPage::Changes, 5),
        (PanelPage::Files, 7),
        (PanelPage::Search, 9),
    ] {
        key(&mut app, KeyCode::F(shortcut));
        poll_app(&mut app, page);
        key(&mut app, KeyCode::End);
        poll_app(&mut app, page);
        {
            let shell = app.workbench.as_ref().unwrap().lock().unwrap();
            assert_eq!(
                shell.panels.as_ref().unwrap().pages[&page]
                    .location
                    .selected
                    .as_deref(),
                Some("src/file079.rs")
            );
        }
        for width in [45, 110] {
            let rendered = screen(&app, width);
            assert!(
                rendered.contains("› src/file079.rs"),
                "selection hidden at {width}: {rendered}"
            );
            assert!(
                !rendered.contains("src/file000.rs"),
                "viewport did not follow selection"
            );
        }
    }
    key(&mut app, KeyCode::F(5));
    key(&mut app, KeyCode::Home);
    poll_app(&mut app, PanelPage::Changes);
    assert!(screen(&app, 110).contains("src/ +160 -80"));
    key(&mut app, KeyCode::Char('d'));
    assert!(!screen(&app, 110).contains("+160 -80"));
    key(&mut app, KeyCode::Char('g'));
    key(&mut app, KeyCode::F(2));
    key(&mut app, KeyCode::F(5));
    let shell = app.workbench.as_ref().unwrap().lock().unwrap();
    let state = &shell.panels.as_ref().unwrap().pages[&PanelPage::Changes];
    assert!(!state.grouped);
    assert!(!state.dirstat);
    assert_eq!(state.location.selected.as_deref(), Some("src/file000.rs"));
}

#[test]
fn panel_render_and_mouse_hits_stay_inside_allocated_rectangles() {
    use ratatui::{buffer::Buffer, layout::Rect};
    let dir = tempfile::tempdir().unwrap();
    let mut shell = WorkbenchShell::open(WorkbenchOptions::new(dir.path()), false);
    shell.attach_panels(Some(Arc::new(Immediate {
        root: dir.path().into(),
    })));
    shell.panels.as_mut().unwrap().open(PanelPage::Files);
    poll_until(shell.panels.as_mut().unwrap(), |panels| {
        !panels.pages[&PanelPage::Files].loading
    });
    for (width, height) in [(1, 1), (40, 15), (110, 30)] {
        let bounds = Rect::new(0, 0, 130, 40);
        let area = Rect::new(5, 3, width, height);
        let mut buffer = Buffer::empty(bounds);
        for cell in &mut buffer.content {
            cell.set_symbol(".");
        }
        shell.render_repository_panel(
            PanelPage::Files,
            area,
            &mut buffer,
            &crate::ReviewOptions::default().theme,
        );
        for y in 0..bounds.height {
            for x in 0..bounds.width {
                if !area.contains((x, y).into()) {
                    assert_eq!(buffer[(x, y)].symbol(), ".", "outside {area:?} at {x},{y}");
                }
            }
        }
        for (hit, _) in &shell.panel_hits {
            assert!(area.contains((hit.x, hit.y).into()));
            assert!(hit.right() <= area.right() && hit.bottom() <= area.bottom());
        }
    }
    let mut app = review_app(Arc::new(Immediate {
        root: dir.path().into(),
    }));
    screen(&app, 110);
    let hit = {
        let shell = app.workbench.as_ref().unwrap().lock().unwrap();
        shell
            .nav_hits
            .iter()
            .find(|(_, key)| *key == crossterm::event::KeyCode::F(9))
            .unwrap()
            .0
    };
    assert!(app.handle_workbench_mouse(&crossterm::event::MouseEvent {
        kind: crossterm::event::MouseEventKind::Up(crossterm::event::MouseButton::Left),
        column: hit.x,
        row: hit.y,
        modifiers: crossterm::event::KeyModifiers::NONE
    }));
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Repository(PanelPage::Search)
    );
}

#[test]
fn loading_query_cannot_open_a_target_from_previous_snapshot() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    for code in [KeyCode::Enter, KeyCode::Char('v')] {
        let (provider, _, reads) = controlled();
        let mut shell = WorkbenchShell::open(WorkbenchOptions::new("/repository"), false);
        shell.attach_panels(Some(provider));
        shell.tab = WorkbenchTab::Repository(PanelPage::Files);
        shell.panels.as_mut().unwrap().open(PanelPage::Files);
        let (_, send) = load_response(&reads);
        send.send(Ok(snapshot(
            PanelPage::Files,
            vec![directory("old-directory")],
        )))
        .unwrap();
        poll_until(shell.panels.as_mut().unwrap(), |p| {
            !p.pages[&PanelPage::Files].loading
        });
        shell.panels.as_mut().unwrap().state(PanelPage::Files).query = "new query".into();
        shell.panels.as_mut().unwrap().refresh(PanelPage::Files);
        let (_, send) = load_response(&reads);
        shell.key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(shell.effect.is_none());
        assert!(
            shell
                .panels
                .as_mut()
                .unwrap()
                .state(PanelPage::Files)
                .directory
                .is_empty()
        );
        send.send(Ok(snapshot(PanelPage::Files, Vec::new())))
            .unwrap();
    }
}

#[test]
fn historical_agent_open_stays_read_only_and_shows_actionable_notice() {
    use crossterm::event::KeyCode;
    let dir = tempfile::tempdir().unwrap();
    let mut app = review_app(Arc::new(Immediate {
        root: dir.path().into(),
    }));
    key(&mut app, KeyCode::F(8));
    app.process_workbench_effect(&mut |_, _, _| Ok(()));
    poll_app(&mut app, PanelPage::Agents);
    key(&mut app, KeyCode::Char('v'));
    app.process_workbench_effect(&mut |_, _, _| {
        panic!("historical record must not reload or execute")
    });
    assert!(screen(&app, 120).contains("read-only preview"));
    assert_eq!(
        app.workbench.as_ref().unwrap().lock().unwrap().tab,
        WorkbenchTab::Repository(PanelPage::Agents)
    );
}

#[test]
fn returning_to_cached_preview_clears_other_targets_pending_or_failed_state() {
    for late in [false, true] {
        let (_, mut panels, reads) = controlled();
        panels.open(PanelPage::Files);
        let (_, send) = load_response(&reads);
        send.send(Ok(snapshot(PanelPage::Files, vec![file("a"), file("b")])))
            .unwrap();
        poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].loading);
        let Read::Preview(_, send) = read(&reads) else {
            panic!()
        };
        send.send(Ok(preview("cached a"))).unwrap();
        poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].preview_loading);
        panels.select(PanelPage::Files, "b");
        let Read::Preview(_, send) = read(&reads) else {
            panic!()
        };
        if !late {
            send.send(Err(PanelError::new("B failed"))).unwrap();
            poll_until(&mut panels, |p| !p.pages[&PanelPage::Files].preview_loading);
        }
        panels.select(PanelPage::Files, "a");
        assert!(!panels.state(PanelPage::Files).preview_loading);
        assert!(panels.state(PanelPage::Files).preview_error.is_none());
        assert_eq!(
            panels
                .state(PanelPage::Files)
                .preview
                .as_ref()
                .unwrap()
                .1
                .body,
            "cached a"
        );
        if late {
            send.send(Ok(preview("late b"))).unwrap();
            let deadline = Instant::now() + Duration::from_secs(3);
            while panels.poll() == 0 {
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(1));
            }
            assert_eq!(
                panels
                    .state(PanelPage::Files)
                    .preview
                    .as_ref()
                    .unwrap()
                    .1
                    .body,
                "cached a"
            );
        }
    }
}

#[derive(Debug)]
struct Cycling {
    root: std::path::PathBuf,
    selected: Mutex<usize>,
    loads: Mutex<usize>,
}
impl RepositoryPanelProvider for Cycling {
    fn source(&self) -> RepositoryPanelSource {
        RepositoryPanelSource {
            root: self.root.clone(),
            identity: "cycling".into(),
        }
    }
    fn load(&self, request: &PanelRequest) -> Result<PanelSnapshot, PanelError> {
        *self.loads.lock().unwrap() += 1;
        Ok(snapshot(
            request.page,
            vec![entry("summary", PanelTarget::GitSummary)],
        ))
    }
    fn preview(&self, _target: &PanelTarget) -> Result<PanelPreview, PanelError> {
        Ok(preview("base-dependent summary"))
    }
    fn git_base_key(&self) -> Option<String> {
        Some("b".into())
    }
    fn cycle_git_base_branch(&self) -> Result<String, PanelError> {
        const BASES: [&str; 2] = ["origin/main", "release"];
        let mut selected = self.selected.lock().unwrap();
        *selected = (*selected + 1) % BASES.len();
        Ok(BASES[*selected].into())
    }
}

#[test]
fn configured_base_key_rotates_the_git_comparison_base_and_refreshes() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(Cycling {
        root: dir.path().into(),
        selected: Mutex::new(0),
        loads: Mutex::new(0),
    });
    let mut shell = WorkbenchShell::open(WorkbenchOptions::new(dir.path()), false);
    shell.attach_panels(Some(provider.clone()));
    shell.tab = WorkbenchTab::Repository(PanelPage::Git);
    shell.panels.as_mut().unwrap().open(PanelPage::Git);
    poll_until(shell.panels.as_mut().unwrap(), |panels| {
        !panels.pages[&PanelPage::Git].loading
    });
    assert_eq!(*provider.loads.lock().unwrap(), 1);

    shell.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(shell.notice.as_deref(), Some("base branch release"));
    poll_until(shell.panels.as_mut().unwrap(), |panels| {
        !panels.pages[&PanelPage::Git].loading
    });

    shell.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(shell.notice.as_deref(), Some("base branch origin/main"));
    poll_until(shell.panels.as_mut().unwrap(), |panels| {
        !panels.pages[&PanelPage::Git].loading
    });
    // Each rotation re-reads the panel against the new base, and unbound keys
    // keep the fixed panel behavior.
    assert_eq!(*provider.loads.lock().unwrap(), 3);
    shell.key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
    assert_eq!(shell.notice.as_deref(), Some("base branch origin/main"));
    assert_eq!(*provider.loads.lock().unwrap(), 3);
    // A modified press never cycles and falls through to the fixed keys.
    shell.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL));
    assert_eq!(shell.notice.as_deref(), Some("base branch origin/main"));
    assert_eq!(*provider.loads.lock().unwrap(), 3);
    // Other repository pages ignore the binding entirely.
    shell.key(KeyEvent::new(KeyCode::F(7), KeyModifiers::NONE));
    poll_until(shell.panels.as_mut().unwrap(), |panels| {
        !panels.pages[&PanelPage::Files].loading
    });
    let files_loads = *provider.loads.lock().unwrap();
    shell.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert_eq!(*provider.loads.lock().unwrap(), files_loads);
    // While the Git query is being edited the binding types into the query
    // instead of cycling.
    shell.key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
    poll_until(shell.panels.as_mut().unwrap(), |panels| {
        !panels.pages[&PanelPage::Git].loading
    });
    shell.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    shell.key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::NONE));
    assert!(
        shell.panels.as_ref().unwrap().pages[&PanelPage::Git]
            .query
            .contains('b')
    );
    shell.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
}
