use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::TempDir;
use workdeck_core::{ChangesetSource, FileSourceSnapshots, SourceOrigin, SourceSnapshot};
use workdeck_diff::parse_patch;
use workdeck_examples::jsx_file_view_extension::{
    COMMAND_ID, VIEW_ID, create_jsx_file_view_layout, matches_jsx_file_view, required_capabilities,
};
use workdeck_extension_api::{
    Capability, ExtensionDiffFile, ExtensionFileSide, ExtensionHostAction, Registration,
};
use workdeck_extension_host::{
    ExtensionDocumentReader, ExtensionRequestCancellation, FileViewInput, LoadedExtension,
};
use workdeck_tui::{ReviewApp, ReviewOptions, render};

fn settle_extension_commands(app: &mut ReviewApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while app.has_pending_extension_commands() {
        app.poll_extension_commands();
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn oracle() -> Value {
    serde_json::from_str(include_str!("../../port/hunk/oracles/jsx-file-view.json")).unwrap()
}

fn oracle_file() -> ExtensionDiffFile {
    serde_json::from_value(oracle()["input"].clone()).unwrap()
}

fn hunk_serializable_projection(layout: &Value) -> Value {
    let mut layout = layout.clone();
    for row in layout["rows"].as_array_mut().unwrap() {
        if let Some(height) = row["component"]["height"].as_u64() {
            row["component"] = serde_json::json!({ "height": height });
        }
    }
    layout
}

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-jsx-file-view-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-jsx-file-view-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/jsx-file-view/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

#[test]
fn native_layout_matches_the_frozen_hunk_tsx_projection() {
    let oracle = oracle();
    assert_eq!(
        oracle["baselineCommits"],
        serde_json::json!([
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        ])
    );
    let file = oracle_file();
    let layout = create_jsx_file_view_layout(&file).unwrap();
    assert_eq!(
        hunk_serializable_projection(&serde_json::to_value(layout).unwrap()),
        oracle["layout"]
    );

    let mut one_hunk = file;
    one_hunk.hunks.truncate(1);
    assert!(!matches_jsx_file_view(&one_hunk));
    assert_eq!(create_jsx_file_view_layout(&one_hunk), None);
    assert_eq!(oracle["oneHunkLayout"], Value::Null);
}

#[test]
fn native_components_preserve_fixed_geometry_state_and_fallbacks() {
    let layout = create_jsx_file_view_layout(&oracle_file()).unwrap();
    assert_eq!(layout.rows.len(), 4);
    assert_eq!(layout.hunk_rows[0].start_row, 0);
    assert_eq!(layout.hunk_rows[1].end_row, 3);
    assert_eq!(layout.rows[1].source_ranges, []);
    assert_eq!(layout.rows[2].source_ranges.len(), 1);
    for (row_index, row) in layout.rows.iter().enumerate() {
        let component = row.component.as_ref().unwrap();
        assert_eq!(component.height, 2);
        assert!(component.expanded_content.is_some());
        assert!(component.toggle_expanded_on_left_mouse_up);
        assert_eq!(component.selection_prefix.as_ref().unwrap().selected, "▶ ");
        let collapsed = serde_json::to_string(&component.content).unwrap();
        assert!(collapsed.contains(&format!("row {row_index} · click for detail")));
        assert!(!row.spans.is_empty());
    }
}

#[test]
fn compiled_protocol_matches_layouts_and_toggles_the_view() {
    let (_directory, manifest) = staged_extension();
    let mut loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    assert!(loaded.handshake.registrations.iter().any(|registration| {
        matches!(registration, Registration::FileView { id, title, .. }
            if id == VIEW_ID && title == "JSX hunk cards (POC)")
    }));
    let file = oracle_file();
    assert!(loaded.file_view_matches(VIEW_ID, file.clone()).unwrap());
    let source = (1..=30)
        .map(|line| format!("line {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    let input = FileViewInput {
        file: Arc::new(file),
        width: 80,
        cancellation: ExtensionRequestCancellation::default(),
        changes: Arc::from([]),
        frozen_documents: BTreeMap::from([
            (ExtensionFileSide::Old, Some(source.clone())),
            (ExtensionFileSide::New, Some(source.clone())),
        ]),
        documents: ExtensionDocumentReader::new(move |_side| Ok(Some(source.clone()))),
    };
    let layout = loaded
        .layout_file_view(VIEW_ID, input)
        .unwrap()
        .expect("multi-hunk input has a layout");
    assert_eq!(layout.row_heights, [2, 2, 2, 2]);
    assert_eq!(
        loaded
            .invoke_command(
                COMMAND_ID,
                workdeck_core::ReviewSnapshot {
                    generation: 1,
                    changeset: workdeck_core::Changeset {
                        id: "test".into(),
                        source_label: "test".into(),
                        title: "test".into(),
                        summary: None,
                        agent_summary: None,
                        source: ChangesetSource::Patch {
                            label: "test".into(),
                        },
                        files: Vec::new(),
                    },
                    selection: workdeck_core::ReviewSelection::default(),
                },
                Vec::new(),
            )
            .unwrap()
            .actions,
        [ExtensionHostAction::ToggleFileView { id: VIEW_ID.into() }]
    );
}

#[test]
fn compiled_process_boundary_ignores_ambient_source_runtimes() {
    let (directory, manifest) = staged_extension();
    fs::create_dir_all(directory.path().join("node_modules/react")).unwrap();
    fs::write(
        directory.path().join("node_modules/react/index.js"),
        "module.exports = { useState() { throw Error('wrong runtime') } };\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("package.json"),
        r#"{"main":"index.tsx","dependencies":{"react":"0.0.1"}}"#,
    )
    .unwrap();
    fs::write(
        directory.path().join("index.tsx"),
        "import React from 'react'; export default <text />;\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("helper.ts"),
        "export const helper = 'source-only';\n",
    )
    .unwrap();

    let mut loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    assert_eq!(loaded.manifest.id, "example.jsx-file-view");
    assert_eq!(loaded.handshake.extension_api_version, 1);
    assert!(loaded.file_view_matches(VIEW_ID, oracle_file()).unwrap());
}

fn multi_hunk_changeset() -> workdeck_core::Changeset {
    let mut changeset = parse_patch(
        "diff --git a/src/lib.rs b/src/lib.rs\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1 +1 @@\n-old one\n+new one\n@@ -4 +4 @@\n-old four\n+new four\n",
        "jsx-cards",
        "JSX cards",
        ChangesetSource::Patch {
            label: "jsx-cards".into(),
        },
    )
    .unwrap();
    changeset.files[0].set_sources(FileSourceSnapshots {
        old: Some(SourceSnapshot::new(
            "old one\ntwo\nthree\nold four\n".into(),
            SourceOrigin::Revision {
                revision: "HEAD".into(),
            },
            true,
        )),
        new: Some(SourceSnapshot::new(
            "new one\ntwo\nthree\nnew four\n".into(),
            SourceOrigin::WorkingTree,
            false,
        )),
    });
    changeset
}

fn terminal_rows(terminal: &Terminal<TestBackend>) -> Vec<String> {
    let area = terminal.backend().buffer().area;
    (area.y..area.bottom())
        .map(|row| {
            (area.x..area.right())
                .map(|column| terminal.backend().buffer()[(column, row)].symbol())
                .collect::<String>()
        })
        .collect()
}

fn settle_file_view_frame(
    app: &ReviewApp,
    terminal: &mut Terminal<TestBackend>,
    ready: impl Fn(&Terminal<TestBackend>) -> bool,
) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), app))
            .unwrap();
        if ready(terminal) {
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native file-view preparation did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

#[test]
fn ratatui_rows_toggle_only_on_an_undragged_left_mouse_up() {
    let (_directory, manifest) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let changeset = multi_hunk_changeset();
    let file_id = changeset.files[0].runtime_id.clone();
    let mut app = ReviewApp::new_with_extensions(
        changeset,
        ReviewOptions {
            sidebar: false,
            ..ReviewOptions::default()
        },
        vec![loaded],
    );
    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    settle_extension_commands(&mut app);

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    settle_file_view_frame(&app, &mut terminal, |terminal| {
        terminal_rows(terminal)
            .iter()
            .any(|line| line.contains("Hunk 1"))
    });
    let rows = terminal_rows(&terminal);
    let row = rows
        .iter()
        .position(|line| line.contains("Hunk 1"))
        .expect("first component is visible") as u16;
    let (hit_x, hit_y, hit_width, hit_height) = app
        .extension_file_view_component_bounds(&file_id, "hunk-0-summary")
        .expect("first component publishes host-owned pointer bounds");
    assert!(row >= hit_y && row < hit_y + hit_height);
    assert!(
        rows.iter()
            .any(|line| line.contains("row 0 · click for detail"))
    );
    assert!(!app.extension_file_view_component_expanded(&file_id, "hunk-0-summary"));

    let mouse = |kind| MouseEvent {
        kind,
        column: hit_x + hit_width.min(5).saturating_sub(1),
        row,
        modifiers: KeyModifiers::NONE,
    };
    app.handle_mouse_event(mouse(MouseEventKind::Down(MouseButton::Left)));
    app.handle_mouse_event(mouse(MouseEventKind::Up(MouseButton::Left)));
    assert!(app.extension_file_view_component_expanded(&file_id, "hunk-0-summary"));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(
        terminal_rows(&terminal)
            .iter()
            .any(|line| line.contains("lines 1–1 ·"))
    );

    app.handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(
        terminal_rows(&terminal)
            .iter()
            .any(|line| line.contains("▶ Hunk"))
    );
    assert!(app.extension_file_view_component_expanded(&file_id, "hunk-0-summary"));

    app.handle_mouse_event(mouse(MouseEventKind::Down(MouseButton::Left)));
    app.handle_mouse_event(mouse(MouseEventKind::Drag(MouseButton::Left)));
    app.handle_mouse_event(mouse(MouseEventKind::Up(MouseButton::Left)));
    assert!(app.extension_file_view_component_expanded(&file_id, "hunk-0-summary"));

    app.handle_mouse_event(MouseEvent {
        kind: MouseEventKind::ScrollDown,
        column: 5,
        row,
        modifiers: KeyModifiers::NONE,
    });
    assert!(app.extension_file_view_component_expanded(&file_id, "hunk-0-summary"));
}

#[test]
fn manifest_declares_the_exact_native_capabilities() {
    let (_directory, manifest_path) = staged_extension();
    let manifest = workdeck_extension_api::ExtensionManifest::load(&manifest_path).unwrap();
    assert_eq!(
        manifest.capabilities,
        vec![Capability::Commands, Capability::FileViews]
    );
    assert_eq!(
        required_capabilities(),
        vec![Capability::Commands, Capability::FileViews]
    );
}

#[test]
fn source_side_wire_names_remain_lowercase() {
    assert_eq!(
        serde_json::to_value(ExtensionFileSide::New).unwrap(),
        serde_json::json!("new")
    );
}
