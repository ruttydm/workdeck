use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, style::Color};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;
use workdeck_core::{
    Changeset, ChangesetSource, FileSourceSnapshots, ReviewSelection, ReviewSnapshot, SourceOrigin,
    SourceSnapshot,
};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_examples::file_view_gallery_extension::{
    CHANGE_ATLAS_VIEW_ID, COMMAND_ID, DEPENDENCY_DELTA_VIEW_ID, PALETTE_DELTA_VIEW_ID,
    create_change_atlas_layout, create_css_palette_layout, create_dependency_layout,
    file_view_matches, gallery_view_for_file, impact_meter, invoke_command, registrations,
    required_capabilities, version_change_highlights,
};
use workdeck_extension_api::{
    Capability, CommandInvocation, ExtensionDiffFile, ExtensionDiffHunk, ExtensionDiffStats,
    ExtensionFileSide, ExtensionFileViewLayout, ExtensionHostAction, ExtensionManifest,
    ExtensionNotifyType, FileViewLayoutRequest, Registration, ViewNode,
};
use workdeck_extension_host::{
    ExtensionRequestCancellation, LoadedExtension, create_file_view_input,
    create_file_view_input_snapshot, validate_file_view_layout,
};
use workdeck_tui::{ReviewApp, ReviewOptions, render};

const CHANGE_BEFORE: &str =
    include_str!("../extensions/file-view-gallery/fixtures/change-atlas/before.rs");
const CHANGE_AFTER: &str =
    include_str!("../extensions/file-view-gallery/fixtures/change-atlas/after.rs");
const CSS_BEFORE: &str =
    include_str!("../extensions/file-view-gallery/fixtures/css-palette/before.css");
const CSS_AFTER: &str =
    include_str!("../extensions/file-view-gallery/fixtures/css-palette/after.css");
const CARGO_BEFORE: &str =
    include_str!("../extensions/file-view-gallery/fixtures/package-dependencies/before/Cargo.toml");
const CARGO_AFTER: &str =
    include_str!("../extensions/file-view-gallery/fixtures/package-dependencies/after/Cargo.toml");

fn oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/file-view-gallery.json"
    ))
    .unwrap()
}

fn request_from_sources(
    before: &str,
    after: &str,
    path: &str,
    view_id: &str,
) -> (workdeck_core::DiffFile, FileViewLayoutRequest) {
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "before",
            contents: before,
            name: path,
        },
        FileSnapshot {
            cache_key: "after",
            contents: after,
            name: path,
        },
        FileComparisonOptions { context_radius: 3 },
    )
    .unwrap();
    file.set_sources(FileSourceSnapshots {
        old: Some(SourceSnapshot::new(
            before.into(),
            SourceOrigin::Revision {
                revision: "before".into(),
            },
            true,
        )),
        new: Some(SourceSnapshot::new(
            after.into(),
            SourceOrigin::File { path: path.into() },
            true,
        )),
    });
    let snapshot = create_file_view_input_snapshot(&file);
    let request = FileViewLayoutRequest {
        view_id: view_id.into(),
        file: snapshot.file.as_ref().clone(),
        width: 100,
        changes: snapshot.changes.to_vec(),
        documents: BTreeMap::from([
            (ExtensionFileSide::Old, Some(before.into())),
            (ExtensionFileSide::New, Some(after.into())),
        ]),
        aborted: false,
    };
    (file, request)
}

fn atlas_oracle_request(value: &Value) -> FileViewLayoutRequest {
    let fixture = &value["fixtures"]["atlas"];
    let hunks = fixture["file"]["hunks"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(index, hunk)| ExtensionDiffHunk {
            index,
            header: hunk["header"].as_str().unwrap().into(),
            old_range: serde_json::from_value(hunk["oldRange"].clone()).unwrap(),
            new_range: serde_json::from_value(hunk["newRange"].clone()).unwrap(),
        })
        .collect();
    FileViewLayoutRequest {
        view_id: CHANGE_ATLAS_VIEW_ID.into(),
        file: ExtensionDiffFile {
            id: fixture["file"]["id"].as_str().unwrap().into(),
            path: fixture["file"]["path"].as_str().unwrap().into(),
            previous_path: None,
            patch: String::new(),
            language: fixture["file"]["language"].as_str().map(str::to_owned),
            stats: ExtensionDiffStats {
                additions: fixture["file"]["stats"]["additions"].as_u64().unwrap() as usize,
                deletions: fixture["file"]["stats"]["deletions"].as_u64().unwrap() as usize,
            },
            change_type: "change".into(),
            stats_truncated: false,
            hunks,
            agent: None,
            is_untracked: false,
            is_binary: false,
            is_too_large: false,
        },
        width: 100,
        changes: serde_json::from_value(fixture["changes"].clone()).unwrap(),
        documents: BTreeMap::new(),
        aborted: false,
    }
}

fn normalized_layout(layout: &ExtensionFileViewLayout) -> Value {
    let rows = layout
        .rows
        .iter()
        .map(|row| {
            json!({
                "id": row.id,
                "spans": row.spans.iter().map(|span| json!({
                    "text": span.text,
                    "tone": serde_json::to_value(span.tone.unwrap()).unwrap(),
                })).collect::<Vec<_>>(),
                "sourceRanges": row.source_ranges,
                "component": row.component.as_ref().map(|component| json!({
                    "height": component.height,
                })),
            })
        })
        .collect::<Vec<_>>();
    json!({ "rows": rows, "hunkRows": layout.hunk_rows })
}

fn row_text(layout: &ExtensionFileViewLayout) -> String {
    layout
        .rows
        .iter()
        .flat_map(|row| &row.spans)
        .map(|span| span.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn view_text(node: &ViewNode, output: &mut String) {
    match node {
        ViewNode::Text { text, .. } => output.push_str(text),
        ViewNode::Row { children, .. } | ViewNode::Column { children, .. } => {
            for child in children {
                view_text(child, output);
            }
        }
        ViewNode::List { items, .. } => {
            for item in items {
                view_text(item, output);
            }
        }
        ViewNode::Action { child, .. } => view_text(child, output),
        ViewNode::Divider | ViewNode::Empty => {}
    }
}

fn painter_content(value: &Value, output: &mut String) {
    match value {
        Value::Array(values) => {
            for value in values {
                painter_content(value, output);
            }
        }
        Value::Object(object) => {
            if let Some(content) = object.get("content").and_then(Value::as_str) {
                output.push_str(content);
            }
            for (key, value) in object {
                if key != "content" {
                    painter_content(value, output);
                }
            }
        }
        _ => {}
    }
}

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-file-view-gallery-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-file-view-gallery-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/file-view-gallery/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn changeset(mut file: workdeck_core::DiffFile) -> Changeset {
    file.refresh_identity();
    Changeset {
        id: "gallery".into(),
        title: "Native gallery".into(),
        source: ChangesetSource::Files {
            left: "before".into(),
            right: "after".into(),
        },
        files: vec![file],
    }
}

fn invocation(file: workdeck_core::DiffFile, command_id: &str) -> CommandInvocation {
    CommandInvocation {
        command_id: command_id.into(),
        snapshot: ReviewSnapshot {
            generation: 1,
            changeset: changeset(file),
            selection: ReviewSelection {
                file_index: 0,
                hunk_index: Some(0),
                side: None,
                line: None,
            },
        },
        cwd: "/repo".into(),
        review: None,
        open_panes: Vec::new(),
        active_keyboard_mode: None,
    }
}

fn rendered_text(terminal: &Terminal<TestBackend>) -> String {
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

fn git_blob_id(path: &Path) -> String {
    let output = Command::new("git")
        .args(["hash-object", "--"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().into()
}

#[test]
fn frozen_hunk_oracle_covers_both_identical_pinned_baselines() {
    let value = oracle();
    assert_eq!(
        value["baselineCommits"],
        json!([
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        ])
    );
    assert_eq!(value["fixtures"]["atlas"]["valid"], true);
    assert_eq!(value["fixtures"]["css"]["valid"], true);
    assert_eq!(value["fixtures"]["dependencies"]["valid"], true);
}

#[test]
fn retained_css_and_python_fixtures_are_byte_exact_hunk_blobs() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("extensions/file-view-gallery");
    for (relative, blob) in [
        (
            "fixtures/css-palette/after.css",
            "791fec88fb0acab5c8e3697217e023479758babc",
        ),
        (
            "fixtures/css-palette/before.css",
            "610c2d4c6e9c3018ed901c3d48ba0196a0fba04c",
        ),
        (
            "mixed-review/fixtures/after/scripts/deploy.py",
            "4bee60e5ba020fc341ac282b683ad68a5c0bb18a",
        ),
        (
            "mixed-review/fixtures/before/scripts/deploy.py",
            "f634aec2839dff522d1afc169fd58c02307e5764",
        ),
    ] {
        assert_eq!(git_blob_id(&root.join(relative)), blob);
    }
}

#[test]
fn impact_atlas_matches_the_three_hunk_hunk_oracle() {
    let value = oracle();
    let request = atlas_oracle_request(&value);
    let layout = create_change_atlas_layout(&request).unwrap();
    assert_eq!(request.file.hunks.len(), 3);
    assert_eq!(
        normalized_layout(&layout),
        value["fixtures"]["atlas"]["layout"]
    );
    assert_eq!(impact_meter(0, 4, 6), "░░░░░░");
    assert_eq!(impact_meter(1, 4, 6), "██░░░░");
    assert_eq!(impact_meter(4, 4, 6), "██████");
    assert!(validate_file_view_layout(&serde_json::to_value(&layout).unwrap(), 3, 100).is_ok());

    let component = layout.rows[0].component.as_ref().unwrap();
    let mut actual_unselected = String::new();
    view_text(&component.content, &mut actual_unselected);
    let mut expected_unselected = String::new();
    painter_content(
        &value["fixtures"]["atlas"]["painter"]["unselected"],
        &mut expected_unselected,
    );
    assert_eq!(actual_unselected, expected_unselected);
    let mut actual_selected = String::new();
    view_text(
        component.selected_content.as_ref().unwrap(),
        &mut actual_selected,
    );
    let mut expected_selected = String::new();
    painter_content(
        &value["fixtures"]["atlas"]["painter"]["selected"],
        &mut expected_selected,
    );
    assert_eq!(actual_selected, expected_selected);
}

#[test]
fn added_and_deleted_atlas_rows_omit_the_nonexistent_source_side() {
    for (before, after, expected) in [
        (
            "",
            "pub const ADDED: bool = true;\n",
            ExtensionFileSide::New,
        ),
        (
            "pub const REMOVED: bool = true;\n",
            "",
            ExtensionFileSide::Old,
        ),
    ] {
        let (_, request) = request_from_sources(before, after, "change.rs", CHANGE_ATLAS_VIEW_ID);
        let layout = create_change_atlas_layout(&request).unwrap();
        assert!(!request.file.hunks.is_empty());
        assert!(
            validate_file_view_layout(
                &serde_json::to_value(&layout).unwrap(),
                request.file.hunks.len(),
                100,
            )
            .is_ok()
        );
        assert_eq!(
            layout
                .rows
                .iter()
                .flat_map(|row| &row.source_ranges)
                .map(|range| range.side)
                .collect::<Vec<_>>(),
            [expected]
        );
    }
}

#[test]
fn css_palette_matches_exact_documents_and_frozen_hunk_layout() {
    let value = oracle();
    let (_, request) =
        request_from_sources(CSS_BEFORE, CSS_AFTER, "theme.css", PALETTE_DELTA_VIEW_ID);
    let layout = create_css_palette_layout(&request).unwrap();
    assert_eq!(request.file.hunks.len(), 2);
    assert_eq!(
        normalized_layout(&layout),
        value["fixtures"]["css"]["layout"]
    );
    assert!(row_text(&layout).contains("--accent: #7aa2f7 → #b48ead"));
    assert!(row_text(&layout).contains("--card-highlight: #24304a → #3b3150"));
    assert!(validate_file_view_layout(&serde_json::to_value(&layout).unwrap(), 2, 100).is_ok());

    let component = layout.rows[0].component.as_ref().unwrap();
    let mut actual = String::new();
    view_text(component.selected_content.as_ref().unwrap(), &mut actual);
    let mut expected = String::new();
    painter_content(
        &value["fixtures"]["css"]["painter"]["selected"],
        &mut expected,
    );
    for content in ["▶ --canvas", " OLD #0b1020", "  →  ", " NEW #090d18"] {
        assert!(actual.contains(content));
        assert!(expected.contains(content));
    }
}

#[test]
fn version_segment_highlighting_matches_every_hunk_case() {
    let actual = [
        version_change_highlights(Some("1.2.3"), Some("1.2.4")),
        version_change_highlights(Some("1.2.9"), Some("1.3.0")),
        version_change_highlights(Some("1.9.4"), Some("2.1.0")),
        version_change_highlights(Some("^1.2.3"), Some("~1.2.4")),
        version_change_highlights(Some("1.2.3-beta.1"), Some("1.2.4-beta.2")),
    ];
    assert_eq!(
        serde_json::to_value(actual).unwrap(),
        oracle()["versionHighlights"]
    );
}

#[test]
fn translated_cargo_dependency_fixture_preserves_multi_hunk_semantics() {
    let (_, request) = request_from_sources(
        CARGO_BEFORE,
        CARGO_AFTER,
        "Cargo.toml",
        DEPENDENCY_DELTA_VIEW_ID,
    );
    let layout = create_dependency_layout(&request).unwrap();
    assert_eq!(request.file.hunks.len(), 2);
    assert_eq!(layout.hunk_rows.len(), 2);
    assert!(layout.rows.iter().all(|row| row.component.is_some()));
    let text = row_text(&layout);
    assert!(text.contains("ratatui: 0.28.1 → 0.29.0 (dependencies)"));
    assert!(text.contains("toml: 0.8.19 → 0.9.8 (devDependencies)"));
    assert!(validate_file_view_layout(&serde_json::to_value(&layout).unwrap(), 2, 100).is_ok());

    let hunk = &oracle()["fixtures"]["dependencies"];
    assert_eq!(hunk["hunkCount"], 2);
    let original = hunk["layout"]["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["spans"][0]["text"].as_str().unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(original.contains("react: 19.1.0 → 19.2.0 (dependencies)"));
    assert!(original.contains("typescript: 5.7.3 → 5.9.2 (devDependencies)"));
}

#[test]
fn conservative_parsers_preserve_all_fallback_policies() {
    let css_cases = [
        (
            ":root {\n  --accent: #12345;\n}\n",
            ":root {\n  --accent: #1234567;\n}\n",
            false,
        ),
        (
            ":root {\n  --accent: #1234;\n}\n",
            ":root {\n  --accent: #12345678;\n}\n",
            false,
        ),
        (
            ".a {\n  --accent: #111111;\n}\n.b {\n  --accent: #222222;\n}\n",
            ".a {\n  --accent: #333333;\n}\n.b {\n  --accent: #444444;\n}\n",
            false,
        ),
        ("", ":root {\n  --accent: #123456;\n}\n", true),
    ];
    for (before, after, expected_some) in css_cases {
        let (_, request) = request_from_sources(before, after, "theme.css", PALETTE_DELTA_VIEW_ID);
        assert_eq!(create_css_palette_layout(&request).is_some(), expected_some);
    }

    for (before, after) in [
        (
            "{\n  \"name\": \"demo\",\n  \"scripts\": { \"test\": \"cargo test\" }\n}\n",
            "{\n  \"name\": \"demo\",\n  \"scripts\": { \"test\": \"cargo test --watch\" }\n}\n",
        ),
        (
            "{\n  \"dependencies\": {\n    \"x\": \"1.0.0\",\n    \"x\": \"2.0.0\"\n  }\n}\n",
            "{\n  \"dependencies\": {\n    \"x\": \"1.0.1\",\n    \"x\": \"2.0.1\"\n  }\n}\n",
        ),
    ] {
        let (_, request) =
            request_from_sources(before, after, "package.json", DEPENDENCY_DELTA_VIEW_ID);
        assert!(create_dependency_layout(&request).is_none());
    }

    let (_, mut request) =
        request_from_sources(CSS_BEFORE, CSS_AFTER, "theme.css", PALETTE_DELTA_VIEW_ID);
    request.aborted = true;
    assert!(create_css_palette_layout(&request).is_none());
}

#[test]
fn registrations_matching_and_contextual_f8_actions_are_precise() {
    let registrations = registrations();
    assert_eq!(
        registrations
            .iter()
            .filter_map(|registration| match registration {
                Registration::FileView { id, .. } => Some(id.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            CHANGE_ATLAS_VIEW_ID,
            PALETTE_DELTA_VIEW_ID,
            DEPENDENCY_DELTA_VIEW_ID,
        ]
    );
    assert!(registrations.iter().any(|registration| {
        matches!(registration, Registration::Command(command)
            if command.id == COMMAND_ID && command.default_keys == ["f8"])
    }));

    let (rust_file, _) = request_from_sources(
        CHANGE_BEFORE,
        CHANGE_AFTER,
        "src/invoice.rs",
        CHANGE_ATLAS_VIEW_ID,
    );
    let rust_public = create_file_view_input_snapshot(&rust_file).file;
    assert_eq!(
        gallery_view_for_file(&rust_public),
        Some(CHANGE_ATLAS_VIEW_ID)
    );
    assert!(file_view_matches(CHANGE_ATLAS_VIEW_ID, &rust_public));
    let (_, css_request) =
        request_from_sources(CSS_BEFORE, CSS_AFTER, "theme.css", PALETTE_DELTA_VIEW_ID);
    assert_eq!(
        gallery_view_for_file(&css_request.file),
        Some(PALETTE_DELTA_VIEW_ID)
    );
    let (_, cargo_request) = request_from_sources(
        CARGO_BEFORE,
        CARGO_AFTER,
        "fixtures/Cargo.toml",
        DEPENDENCY_DELTA_VIEW_ID,
    );
    assert_eq!(
        gallery_view_for_file(&cargo_request.file),
        Some(DEPENDENCY_DELTA_VIEW_ID)
    );

    let mut near_miss = rust_public.as_ref().clone();
    near_miss.path = "theme.css.map".into();
    near_miss.language = None;
    assert_eq!(gallery_view_for_file(&near_miss), None);
    near_miss.path = "notes/package.json.md".into();
    assert_eq!(gallery_view_for_file(&near_miss), None);

    assert_eq!(
        invoke_command(&invocation(rust_file.clone(), COMMAND_ID))
            .unwrap()
            .actions,
        [ExtensionHostAction::ToggleFileView {
            id: CHANGE_ATLAS_VIEW_ID.into(),
        }]
    );
    near_miss.path = "notes.txt".into();
    let mut unsupported = rust_file;
    unsupported.path = near_miss.path;
    unsupported.language = None;
    assert!(matches!(
        &invoke_command(&invocation(unsupported, COMMAND_ID)).unwrap().actions[0],
        ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Info }
            if message.contains("no demo")
    ));
}

#[test]
fn selected_component_fields_round_trip_and_are_strictly_validated() {
    let request = atlas_oracle_request(&oracle());
    let layout = create_change_atlas_layout(&request).unwrap();
    let value = serde_json::to_value(&layout).unwrap();
    assert!(value["rows"][0]["component"]["selectedContent"].is_object());
    let validated = validate_file_view_layout(&value, 3, 100).unwrap();
    assert_eq!(validated.layout, layout);

    let mut invalid = value;
    invalid["rows"][0]["component"]["selectedContent"] = json!({"type": "unknown"});
    assert!(
        validate_file_view_layout(&invalid, 3, 100)
            .unwrap_err()
            .contains("selectedContent")
    );
}

#[test]
fn compiled_subprocess_serves_matches_layouts_and_commands() {
    let (_directory, manifest_path) = staged_extension();
    let manifest = ExtensionManifest::load(&manifest_path).unwrap();
    assert_eq!(manifest.capabilities, required_capabilities());
    assert_eq!(
        manifest.capabilities,
        [
            Capability::Commands,
            Capability::FileViews,
            Capability::Notifications
        ]
    );
    let mut loaded = LoadedExtension::spawn(&manifest_path, "test").unwrap();
    assert_eq!(loaded.handshake.registrations, registrations());

    let (file, request) =
        request_from_sources(CSS_BEFORE, CSS_AFTER, "theme.css", PALETTE_DELTA_VIEW_ID);
    assert!(
        loaded
            .file_view_matches(PALETTE_DELTA_VIEW_ID, request.file)
            .unwrap()
    );
    let input = create_file_view_input(&file, 100, ExtensionRequestCancellation::default(), None);
    let remote = loaded
        .layout_file_view(PALETTE_DELTA_VIEW_ID, input)
        .unwrap()
        .unwrap();
    assert_eq!(
        normalized_layout(&remote.layout),
        oracle()["fixtures"]["css"]["layout"]
    );
    let snapshot = ReviewSnapshot {
        generation: 1,
        changeset: changeset(file),
        selection: ReviewSelection {
            file_index: 0,
            hunk_index: Some(0),
            side: None,
            line: None,
        },
    };
    assert_eq!(
        loaded
            .invoke_command(COMMAND_ID, snapshot, Vec::new())
            .unwrap()
            .actions,
        [ExtensionHostAction::ToggleFileView {
            id: PALETTE_DELTA_VIEW_ID.into(),
        }]
    );
}

#[test]
fn ratatui_paints_selection_sensitive_atlas_and_exact_css_swatches() {
    let (_directory, manifest_path) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest_path, "test").unwrap();
    let (file, _) = request_from_sources(
        CHANGE_BEFORE,
        CHANGE_AFTER,
        "invoice.rs",
        CHANGE_ATLAS_VIEW_ID,
    );
    let file_id = file.runtime_id.clone();
    let mut app =
        ReviewApp::new_with_extensions(changeset(file), ReviewOptions::default(), vec![loaded]);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    assert_eq!(
        app.selected_extension_file_view(&file_id).as_deref(),
        Some("example.file-view-gallery:change-atlas")
    );
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let first = rendered_text(&terminal);
    assert!(first.contains("▶ CHANGE 01"));
    assert!(first.contains("◇ CHANGE 02"));

    app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let second = rendered_text(&terminal);
    assert!(second.contains("◇ CHANGE 01"));
    assert!(second.contains("▶ CHANGE 02"));

    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    assert_eq!(app.selected_extension_file_view(&file_id), None);

    let (_directory, manifest_path) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest_path, "test").unwrap();
    let (file, _) = request_from_sources(CSS_BEFORE, CSS_AFTER, "theme.css", PALETTE_DELTA_VIEW_ID);
    let mut app =
        ReviewApp::new_with_extensions(changeset(file), ReviewOptions::default(), vec![loaded]);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.bg == Color::Rgb(11, 16, 32))
    );
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|cell| cell.bg == Color::Rgb(9, 13, 24))
    );
}

#[test]
fn native_mixed_review_runner_prepares_exactly_five_files_without_starting_a_tui() {
    let output = Command::new(env!(
        "CARGO_BIN_EXE_workdeck-example-file-view-gallery-mixed-review"
    ))
    .arg("--prepare-only")
    .output()
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        "Prepared native gallery review: Cargo.toml, README.md, scripts/deploy.py, src/invoice.rs, styles/theme.css"
    );
}
