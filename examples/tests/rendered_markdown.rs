use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;
use workdeck_core::{
    ChangesetSource, FileSourceSnapshots, ReviewSelection, ReviewSnapshot, SourceOrigin,
    SourceSnapshot,
};
use workdeck_diff::parse_patch;
use workdeck_examples::rendered_markdown_extension::{
    MAX_MARKDOWN_SOURCE_LENGTH, VIEW_ID, create_rendered_markdown_layout, has_unterminated_fence,
    matches_markdown_file, required_capabilities,
};
use workdeck_extension_api::{
    Capability, ExtensionDiffFile, ExtensionDiffHunk, ExtensionDiffStats, ExtensionFileChangeRange,
    ExtensionFileSide, ExtensionFileViewLayout, ExtensionHostAction, ExtensionManifest,
    FileViewLayoutRequest, Registration,
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
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/rendered-markdown.json"
    ))
    .unwrap()
}

fn advanced_oracle() -> Value {
    serde_json::from_str(include_str!(
        "../../port/hunk/oracles/rendered-markdown-advanced.json"
    ))
    .unwrap()
}

fn oracle_file(value: &Value) -> ExtensionDiffFile {
    ExtensionDiffFile {
        id: "readme".into(),
        path: "README.md".into(),
        previous_path: None,
        patch: String::new(),
        language: Some("markdown".into()),
        stats: ExtensionDiffStats {
            additions: 3,
            deletions: 0,
        },
        metadata: serde_json::json!({ "hunks": [] }),
        change_type: Some(workdeck_extension_api::ExtensionVcsFileChangeType::Change),
        stats_truncated: false,
        hunks: serde_json::from_value(value["hunks"].clone()).unwrap(),
        agent: None,
        is_untracked: false,
        is_binary: false,
        is_too_large: false,
    }
}

fn oracle_request(value: &Value) -> FileViewLayoutRequest {
    FileViewLayoutRequest {
        view_id: VIEW_ID.into(),
        file: oracle_file(value),
        width: value["width"].as_u64().unwrap() as usize,
        changes: serde_json::from_value(value["changes"].clone()).unwrap(),
        documents: BTreeMap::from([
            (ExtensionFileSide::Old, None),
            (
                ExtensionFileSide::New,
                Some(value["source"].as_str().unwrap().to_owned()),
            ),
        ]),
        aborted: false,
    }
}

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-rendered-markdown-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    let source = env!("CARGO_BIN_EXE_workdeck-example-rendered-markdown-extension");
    fs::copy(source, binary_directory.join(binary_name)).unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/rendered-markdown/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

#[test]
fn native_layout_matches_the_frozen_marked_17_oracle() {
    let oracle = oracle();
    assert_eq!(
        oracle["baselineCommits"],
        serde_json::json!([
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        ])
    );
    let actual = create_rendered_markdown_layout(&oracle_request(&oracle)).unwrap();
    assert_eq!(serde_json::to_value(actual).unwrap(), oracle["layout"]);
}

#[test]
fn advanced_layout_matches_the_frozen_marked_17_oracle() {
    let oracle = advanced_oracle();
    let actual = create_rendered_markdown_layout(&oracle_request(&oracle)).unwrap();
    assert_eq!(serde_json::to_value(actual).unwrap(), oracle["layout"]);
}

#[test]
fn matching_and_fallbacks_preserve_the_extension_contract() {
    let oracle = oracle();
    let mut file = oracle_file(&oracle);
    assert!(matches_markdown_file(&file));
    file.path = "guide.MDOWN".into();
    assert!(matches_markdown_file(&file));
    file.path = "guide.MARKDOWN".into();
    assert!(!matches_markdown_file(&file));
    file.path = "README.md".into();
    file.is_binary = true;
    assert!(!matches_markdown_file(&file));
    file.is_binary = false;
    file.is_too_large = true;
    assert!(!matches_markdown_file(&file));

    let mut request = oracle_request(&oracle);
    request
        .documents
        .insert(ExtensionFileSide::New, Some(String::new()));
    assert!(create_rendered_markdown_layout(&request).is_none());
    request.documents.insert(
        ExtensionFileSide::New,
        Some("```rust\nfn main() {}\n".into()),
    );
    assert!(has_unterminated_fence("```rust\nfn main() {}\n"));
    assert!(create_rendered_markdown_layout(&request).is_none());
    request.documents.insert(
        ExtensionFileSide::New,
        Some("x".repeat(MAX_MARKDOWN_SOURCE_LENGTH + 1)),
    );
    assert!(create_rendered_markdown_layout(&request).is_none());
    request
        .documents
        .insert(ExtensionFileSide::New, Some("# heading\n".into()));
    request.aborted = true;
    assert!(create_rendered_markdown_layout(&request).is_none());
}

#[test]
fn compiled_protocol_matches_layouts_and_toggles_the_registered_view() {
    let oracle = oracle();
    let (_directory, manifest) = staged_extension();
    let mut loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    assert!(loaded.handshake.registrations.iter().any(|registration| {
        matches!(registration, Registration::FileView { id, title, .. }
            if id == VIEW_ID && title == "Rendered Markdown")
    }));
    let file = oracle_file(&oracle);
    assert!(loaded.file_view_matches(VIEW_ID, file.clone()).unwrap());
    let request = oracle_request(&oracle);
    let source = request
        .documents
        .get(&ExtensionFileSide::New)
        .cloned()
        .flatten();
    let cancellation = ExtensionRequestCancellation::default();
    let source_calls = Arc::new(Mutex::new(Vec::new()));
    let input = FileViewInput {
        file: Arc::new(file),
        width: request.width,
        cancellation: cancellation.clone(),
        changes: Arc::from(request.changes),
        frozen_documents: request.documents.clone(),
        documents: ExtensionDocumentReader::new({
            let source_calls = Arc::clone(&source_calls);
            move |side| {
                source_calls.lock().unwrap().push(side);
                Ok((side == ExtensionFileSide::New)
                    .then(|| source.clone())
                    .flatten())
            }
        }),
    };
    let layout = loaded
        .layout_file_view(VIEW_ID, input)
        .unwrap()
        .expect("oracle input produces a layout");
    assert!(cancellation.is_cancelled());
    assert_eq!(
        *source_calls.lock().unwrap(),
        [ExtensionFileSide::New],
        "only the exact source side bound by the accepted layout is read"
    );
    let expected: ExtensionFileViewLayout =
        serde_json::from_value(oracle["layout"].clone()).unwrap();
    assert_eq!(layout.layout, expected);
    let snapshot = ReviewSnapshot {
        generation: 1,
        changeset: parse_patch(
            "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old\n+new\n",
            "markdown",
            "Markdown",
            ChangesetSource::Patch {
                label: "markdown".into(),
            },
        )
        .unwrap(),
        selection: ReviewSelection::default(),
    };
    assert_eq!(
        loaded
            .invoke_command("toggle-rendered-markdown", snapshot, Vec::new())
            .unwrap()
            .actions,
        [ExtensionHostAction::ToggleFileView { id: VIEW_ID.into() }]
    );
}

#[test]
fn review_shell_accepts_and_clears_the_native_file_view_selection() {
    let (_directory, manifest) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let changeset = parse_patch(
        "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old\n+new\n",
        "markdown",
        "Markdown",
        ChangesetSource::Patch {
            label: "markdown".into(),
        },
    )
    .unwrap();
    let file_id = changeset.files[0].runtime_id.clone();
    let mut app = ReviewApp::new_with_extensions(changeset, ReviewOptions::default(), vec![loaded]);
    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    settle_extension_commands(&mut app);
    assert_eq!(
        app.selected_extension_file_view(&file_id).as_deref(),
        Some("example.rendered-markdown:rendered-markdown")
    );
    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    settle_extension_commands(&mut app);
    assert_eq!(app.selected_extension_file_view(&file_id), None);
}

#[test]
fn review_shell_paints_the_selected_symbolic_rows_and_restores_raw_diff() {
    let (_directory, manifest) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let mut changeset = parse_patch(
        "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1,2 @@\n-old\n+# Heading\n+paragraph\n",
        "markdown-render",
        "Markdown render",
        ChangesetSource::Patch {
            label: "markdown-render".into(),
        },
    )
    .unwrap();
    changeset.files[0].set_sources(FileSourceSnapshots {
        old: Some(SourceSnapshot::new(
            "old\n".into(),
            SourceOrigin::Revision {
                revision: "HEAD".into(),
            },
            true,
        )),
        new: Some(SourceSnapshot::new(
            "# Heading\nparagraph\n".into(),
            SourceOrigin::File {
                path: "README.md".into(),
            },
            true,
        )),
    });
    let mut app = ReviewApp::new_with_extensions(changeset, ReviewOptions::default(), vec![loaded]);
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();

    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    settle_extension_commands(&mut app);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    let rendered = loop {
        terminal
            .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
            .unwrap();
        let rendered = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        if !rendered.contains("-old") {
            break rendered;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "native file-view preparation did not settle"
        );
        std::thread::sleep(std::time::Duration::from_millis(2));
    };
    assert!(rendered.contains("Heading"));
    assert!(rendered.contains("paragraph"));
    assert!(!rendered.contains("-old"));

    app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE));
    settle_extension_commands(&mut app);
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let raw = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(raw.contains("old"));
    assert!(raw.contains("# Heading"));
}

#[test]
fn manifest_and_registrations_declare_only_the_native_capabilities() {
    assert_eq!(
        required_capabilities(),
        [Capability::Commands, Capability::FileViews]
    );
    let (_directory, manifest_path) = staged_extension();
    let manifest = ExtensionManifest::load(&manifest_path).unwrap();
    assert_eq!(
        manifest.capabilities,
        [Capability::Commands, Capability::FileViews]
    );
}

#[test]
fn range_models_round_trip_without_javascript_number_or_case_drift() {
    let oracle = oracle();
    let changes: Vec<ExtensionFileChangeRange> =
        serde_json::from_value(oracle["changes"].clone()).unwrap();
    let hunks: Vec<ExtensionDiffHunk> = serde_json::from_value(oracle["hunks"].clone()).unwrap();
    assert_eq!(serde_json::to_value(changes).unwrap(), oracle["changes"]);
    assert_eq!(serde_json::to_value(hunks).unwrap(), oracle["hunks"]);
}
