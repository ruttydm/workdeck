use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use std::fs;
use tempfile::TempDir;
use workdeck_core::{Changeset, ChangesetSource, ReviewSelection, ReviewSnapshot};
use workdeck_diff::parse_patch;
use workdeck_extension_api::{ExtensionHostAction, PanePlacement, PaneRenderRequest, Registration};
use workdeck_extension_host::LoadedExtension;
use workdeck_tui::{ReviewApp, ReviewOptions, render, to_extension_paint_theme};

fn settle_extension_commands(app: &mut ReviewApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.has_pending_extension_commands() {
        app.poll_extension_commands();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn staged_extension() -> (TempDir, std::path::PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-pane-layout-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-pane-layout-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/pane-layout/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn changeset() -> Changeset {
    parse_patch(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old\n+new\n",
        "pane-example",
        "pane example",
        ChangesetSource::Patch {
            label: "pane example".into(),
        },
    )
    .unwrap()
}

fn snapshot() -> ReviewSnapshot {
    ReviewSnapshot {
        generation: 1,
        changeset: changeset(),
        selection: ReviewSelection::default(),
    }
}

fn rendered_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn compiled_extension_registers_exact_panes_and_toggles_atomically() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let panes = extension
        .handshake
        .registrations
        .iter()
        .filter_map(|registration| match registration {
            Registration::Pane(pane) => Some(pane),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(panes.len(), 3);
    assert_eq!(panes[0].placement, PanePlacement::Right);
    assert_eq!(panes[0].width.as_ref().unwrap().preferred, 28);
    assert_eq!(panes[0].width.as_ref().unwrap().min, Some(18));
    assert_eq!(panes[0].width.as_ref().unwrap().max, Some(44));
    assert!(panes.iter().all(|pane| !pane.default_open));
    assert_eq!(panes[1].height.as_ref().unwrap().min, Some(2));
    assert_eq!(panes[2].height.as_ref().unwrap().max, Some(2));

    let execution = extension
        .invoke_command("toggle", snapshot(), Vec::new())
        .unwrap();
    assert_eq!(execution.actions.len(), 3);
    assert!(
        execution
            .actions
            .iter()
            .all(|action| matches!(action, ExtensionHostAction::OpenPane { .. }))
    );
    let execution = extension
        .invoke_command(
            "toggle",
            snapshot(),
            vec!["example.pane-layout:side".into()],
        )
        .unwrap();
    assert!(
        execution
            .actions
            .iter()
            .all(|action| matches!(action, ExtensionHostAction::ClosePane { .. }))
    );
}

#[test]
fn compiled_extension_receives_exact_geometry_selection_and_theme() {
    let (_directory, manifest) = staged_extension();
    let mut extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let options = ReviewOptions::default();
    let view = extension
        .render_pane(PaneRenderRequest {
            pane_id: "side".into(),
            snapshot: snapshot(),
            placement: PanePlacement::Right,
            width: 28,
            height: 21,
            theme: to_extension_paint_theme(&options.theme),
        })
        .unwrap();
    let encoded = serde_json::to_string(&view.content).unwrap();
    assert!(encoded.contains("RIGHT PANE · 28×21"));
    assert!(encoded.contains("src/lib.rs"));
    assert!(encoded.contains(&options.theme.accent));
    assert!(encoded.contains(&options.theme.text));
}

#[test]
fn review_shell_routes_ctrl_p_and_resizes_the_right_pane() {
    let (_directory, manifest) = staged_extension();
    let extension = LoadedExtension::spawn(&manifest, "test-host").unwrap();
    let mut app =
        ReviewApp::new_with_extensions(changeset(), ReviewOptions::default(), vec![extension]);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(!rendered_text(&terminal).contains("RIGHT PANE"));

    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    settle_extension_commands(&mut app);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let opened = rendered_text(&terminal);
    assert!(opened.contains("RIGHT PANE · 28×22"), "{opened}");
    assert!(opened.contains("TOP PANE · 71×2"));
    assert!(opened.contains("BOTTOM PANE · 71×2"));
    assert!(opened.contains("src/lib.rs"));

    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: 71,
        row: 10,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Drag(MouseButton::Left),
        column: 61,
        row: 10,
        modifiers: KeyModifiers::NONE,
    });
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 61,
        row: 10,
        modifiers: KeyModifiers::NONE,
    });
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("RIGHT PANE · 38×22"));

    app.handle_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    settle_extension_commands(&mut app);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(!rendered_text(&terminal).contains("RIGHT PANE"));

    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 31,
        row: 0,
        modifiers: KeyModifiers::NONE,
    });
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(rendered_text(&terminal).contains("Toggle pane layout example"));
    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::Up(MouseButton::Left),
        column: 31,
        row: 2,
        modifiers: KeyModifiers::NONE,
    });
    settle_extension_commands(&mut app);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let rendered = rendered_text(&terminal);
    assert!(rendered.contains("RIGHT PANE · 38×22"), "{rendered}");
}

#[test]
fn declared_capabilities_match_the_manifest() {
    assert_eq!(
        workdeck_examples::pane_layout_extension::required_capabilities(),
        [
            workdeck_extension_api::Capability::Commands,
            workdeck_extension_api::Capability::Panes,
        ]
    );
}
