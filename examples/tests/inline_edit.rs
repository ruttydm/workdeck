use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tempfile::TempDir;
use workdeck_core::{
    Changeset, ChangesetSource, CliInput, CommonOptions, FileSourceSnapshots, ReviewSelection,
    ReviewSnapshot, SourceOrigin, SourceSnapshot, VcsDiffCommandInput,
};
use workdeck_diff::parse_patch;
use workdeck_examples::inline_edit_extension::{
    COMMAND_ID, InlineEditExtension, REWRITE_COMMAND_ID, VIEW_ID,
};
use workdeck_extension_api::{
    Capability, CommandInvocation, ExtensionFileChangeKind, ExtensionFileChangeRange,
    ExtensionFileSide, ExtensionHostAction, ExtensionKeyEvent, ExtensionNotifyType,
    ExtensionWorkspaceWriteCompletion, ExtensionWorkspaceWriteResult, FileViewLayoutRequest,
    FileViewModeKeyRequest, FileViewModeLifecycleRequest, KeyRoutingResult, Registration,
};
use workdeck_extension_host::{
    LoadedExtension, create_file_view_input_snapshot, validate_file_view_layout,
};
use workdeck_tui::{ReviewApp, ReviewOptions, render};

fn oracle() -> serde_json::Value {
    serde_json::from_str(include_str!("../../port/hunk/oracles/inline-edit.json")).unwrap()
}

fn changeset(document: &str) -> Changeset {
    let mut changeset = parse_patch(
        "diff --git a/alpha.ts b/alpha.ts\n--- a/alpha.ts\n+++ b/alpha.ts\n@@ -2 +2 @@\n-beta\n+beta\n",
        "inline-edit",
        "Inline edit",
        ChangesetSource::WorkingTree { staged: false },
    )
    .unwrap();
    let file = &mut changeset.files[0];
    file.set_sources(FileSourceSnapshots {
        old: Some(SourceSnapshot::new(
            "alpha\nbeta\n".into(),
            SourceOrigin::Revision {
                revision: "HEAD".into(),
            },
            true,
        )),
        new: Some(SourceSnapshot::new(
            document.into(),
            SourceOrigin::WorkingTree,
            true,
        )),
    });
    changeset
}

fn whole_file_changeset() -> Changeset {
    let mut value = changeset("alpha\nbeta\ngamma\n");
    value.files[0].hunks[0].new_start = 1;
    value.files[0].hunks[0].new_count = 3;
    value.files[0].hunks[0].old_start = 1;
    value.files[0].hunks[0].old_count = 3;
    value
}

fn invocation(changeset: &Changeset) -> CommandInvocation {
    CommandInvocation {
        command_id: COMMAND_ID.into(),
        snapshot: ReviewSnapshot {
            generation: 1,
            changeset: changeset.clone(),
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
        workspace: None,
    }
}

fn writable_input() -> CliInput {
    CliInput::Vcs(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions::default(),
    })
}

struct Harness {
    extension: InlineEditExtension,
    changeset: Changeset,
}

impl Harness {
    fn new(document: &str) -> Self {
        Self {
            extension: InlineEditExtension::default(),
            changeset: changeset(document),
        }
    }

    fn whole_file() -> Self {
        Self {
            extension: InlineEditExtension::default(),
            changeset: whole_file_changeset(),
        }
    }

    fn file(&self) -> workdeck_extension_api::ExtensionDiffFile {
        create_file_view_input_snapshot(&self.changeset.files[0])
            .file
            .as_ref()
            .clone()
    }

    fn start(&mut self) -> Vec<ExtensionHostAction> {
        let actions = self
            .extension
            .invoke_command(&invocation(&self.changeset))
            .unwrap()
            .actions;
        let enter = self.mode_request();
        assert!(self.extension.enter_mode(&enter).actions.is_empty());
        actions
    }

    fn mode_request(&self) -> FileViewModeLifecycleRequest {
        FileViewModeLifecycleRequest {
            view_id: VIEW_ID.into(),
            file: self.file(),
            cwd: "/repo".into(),
            review_generation: 1,
        }
    }

    fn press(&mut self, key: ExtensionKeyEvent) -> workdeck_extension_api::KeyboardModeExecution {
        let request = FileViewModeKeyRequest {
            view_id: VIEW_ID.into(),
            file: self.file(),
            key,
            cwd: "/repo".into(),
            review_generation: 1,
        };
        self.extension.key(&request)
    }

    fn layout(&self, width: usize) -> workdeck_extension_api::ExtensionFileViewLayout {
        let file = self.file();
        let document = self.changeset.files[0]
            .sources
            .new
            .as_ref()
            .map(|source| source.content.clone());
        self.extension
            .layout(&FileViewLayoutRequest {
                view_id: VIEW_ID.into(),
                file,
                width,
                changes: vec![ExtensionFileChangeRange {
                    hunk_index: 0,
                    kind: ExtensionFileChangeKind::Added,
                    range: [2, 2],
                }],
                documents: BTreeMap::from([
                    (ExtensionFileSide::Old, None),
                    (ExtensionFileSide::New, document),
                ]),
                aborted: false,
            })
            .unwrap()
    }

    fn exit(&mut self) -> Vec<ExtensionHostAction> {
        let request = self.mode_request();
        self.extension.exit_mode(&request).actions
    }
}

fn key(name: &str, sequence: &str) -> ExtensionKeyEvent {
    ExtensionKeyEvent {
        name: name.into(),
        sequence: sequence.into(),
        ..ExtensionKeyEvent::default()
    }
}

fn row_text(layout: &workdeck_extension_api::ExtensionFileViewLayout, index: usize) -> String {
    layout.rows[index]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect()
}

fn refresh_file_id(actions: &[ExtensionHostAction]) -> Option<&str> {
    actions.iter().find_map(|action| match action {
        ExtensionHostAction::RefreshFileView { file_id, .. } => file_id.as_deref(),
        _ => None,
    })
}

fn requested_write(actions: &[ExtensionHostAction]) -> Option<(&str, &str)> {
    actions.iter().find_map(|action| match action {
        ExtensionHostAction::RequestWorkspaceWrite {
            request_id, text, ..
        } => Some((request_id.as_str(), text.as_str())),
        _ => None,
    })
}

#[test]
fn registers_one_interactive_file_view_and_both_workspace_commands() {
    let registrations = InlineEditExtension::registrations();
    assert!(registrations.iter().any(|registration| {
        matches!(registration, Registration::Command(command)
            if command.id == COMMAND_ID && command.default_keys == ["ctrl+e"])
    }));
    assert!(registrations.iter().any(|registration| {
        matches!(registration, Registration::Command(command)
            if command.id == REWRITE_COMMAND_ID && command.default_keys.is_empty())
    }));
    assert!(registrations.iter().any(|registration| {
        matches!(registration, Registration::FileView { id, interactive_mode: true, .. }
            if id == VIEW_ID)
    }));
    assert_eq!(
        InlineEditExtension::required_capabilities(),
        [
            Capability::Commands,
            Capability::FileViews,
            Capability::Notifications,
            Capability::WorkspaceRead,
            Capability::WorkspaceWrite,
        ]
    );
    let mut file = Harness::new("alpha\nbeta\n").file();
    assert!(InlineEditExtension::matches(&file));
    file.is_binary = true;
    assert!(!InlineEditExtension::matches(&file));
}

#[test]
fn native_rows_match_the_frozen_hunk_oracle_at_both_pinned_baselines() {
    let oracle = oracle();
    assert_eq!(
        oracle["baselineCommits"],
        serde_json::json!([
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2",
            "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"
        ])
    );
    let mut harness = Harness::new("alpha\nbeta\n");
    assert_eq!(
        serde_json::to_value(harness.layout(40)).unwrap(),
        oracle["readOnlyLayout"]
    );
    harness.start();
    assert_eq!(
        serde_json::to_value(harness.layout(40)).unwrap(),
        oracle["editingLayout"]
    );
    harness.press(key("z", "z"));
    assert_eq!(
        serde_json::to_value(harness.layout(40)).unwrap(),
        oracle["typedLayout"]
    );
}

#[test]
fn read_only_layout_has_numbers_added_tone_bindings_and_valid_geometry() {
    let harness = Harness::new("alpha\nbeta\n");
    let layout = harness.layout(40);
    assert_eq!(row_text(&layout, 0), " 1 alpha");
    assert_eq!(row_text(&layout, 1), " 2 beta");
    assert!(layout.rows[0].source_ranges.is_empty());
    assert_eq!(layout.rows[1].source_ranges[0].range, [2, 2]);
    assert_eq!(
        layout.rows[1].spans[1].tone,
        Some(workdeck_extension_api::ExtensionFileViewTone::Added)
    );
    assert_eq!(
        layout.hunk_rows[0],
        workdeck_extension_api::ExtensionFileViewHunkRows {
            start_row: 1,
            end_row: 1
        }
    );
    assert!(validate_file_view_layout(&serde_json::to_value(layout).unwrap(), 1, 40).is_ok());
}

#[test]
fn unreadable_document_declines_layout_and_non_worktree_review_refuses_editing() {
    let mut harness = Harness::new("alpha\nbeta\n");
    let mut request = FileViewLayoutRequest {
        view_id: VIEW_ID.into(),
        file: harness.file(),
        width: 40,
        changes: Vec::new(),
        documents: BTreeMap::from([
            (ExtensionFileSide::Old, None),
            (ExtensionFileSide::New, None),
        ]),
        aborted: false,
    };
    assert!(harness.extension.layout(&request).is_none());
    request.aborted = true;
    assert!(harness.extension.layout(&request).is_none());

    harness.changeset.source = ChangesetSource::Patch {
        label: "stdin".into(),
    };
    let actions = harness
        .extension
        .invoke_command(&invocation(&harness.changeset))
        .unwrap()
        .actions;
    assert!(matches!(
        &actions[0],
        ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning }
            if message.contains("working-tree diff")
    ));
}

#[test]
fn one_command_enters_the_view_mode_and_scopes_the_first_refresh() {
    let mut harness = Harness::new("alpha\nbeta\n");
    let file_id = harness.changeset.files[0].runtime_id.clone();
    let actions = harness.start();
    assert_eq!(
        actions,
        [
            ExtensionHostAction::EnterFileViewMode { id: VIEW_ID.into() },
            ExtensionHostAction::RefreshFileView {
                id: VIEW_ID.into(),
                file_id: Some(file_id),
            },
        ]
    );
    let layout = harness.layout(40);
    assert_eq!(row_text(&layout, 0), "EDITING — Esc exits · ctrl+s writes");
    assert_eq!(row_text(&layout, 1), " 1 alpha");
    assert_eq!(row_text(&layout, 2), "▎2 beta");
    assert_eq!(layout.hunk_rows[0].start_row, 2);
    assert!(
        !layout
            .rows
            .iter()
            .flat_map(|row| &row.spans)
            .any(|span| span.tone == Some(workdeck_extension_api::ExtensionFileViewTone::Added))
    );
}

#[test]
fn types_splits_joins_moves_and_refreshes_only_the_edited_file() {
    let mut harness = Harness::new("alpha\nbeta\n");
    let file_id = harness.changeset.files[0].runtime_id.clone();
    harness.start();
    for event in [
        key("z", "z"),
        key("space", " "),
        key("backspace", "\u{7f}"),
        key("return", "\r"),
        key("backspace", "\u{7f}"),
        key("up", ""),
        key("right", ""),
    ] {
        let execution = harness.press(event);
        assert_eq!(execution.result, KeyRoutingResult::Handled);
        assert_eq!(refresh_file_id(&execution.actions), Some(file_id.as_str()));
    }
    let layout = harness.layout(40);
    assert_eq!(row_text(&layout, 1), "▎1 alpha");
    assert_eq!(layout.rows[1].spans[2].text, "p");
    assert!(row_text(&layout, 0).contains("MODIFIED"));
}

#[test]
fn unicode_editing_preserves_whole_graphemes_and_vertical_columns() {
    let mut harness = Harness::new("😀\n");
    harness.start();
    assert_eq!(
        harness.layout(40).rows[2.min(harness.layout(40).rows.len() - 1)].id,
        "line:1"
    );
    harness.press(key("right", ""));
    harness.press(key("backspace", "\u{7f}"));
    let save = harness.press(ExtensionKeyEvent {
        name: "s".into(),
        ctrl: true,
        ..ExtensionKeyEvent::default()
    });
    assert_eq!(requested_write(&save.actions).unwrap().1, "\n");
    let request_id = requested_write(&save.actions).unwrap().0.to_owned();
    harness
        .extension
        .complete_write(&ExtensionWorkspaceWriteCompletion {
            request_id,
            result: ExtensionWorkspaceWriteResult::Written,
        });
    harness.press(key("😀", "😀"));
    let save = harness.press(ExtensionKeyEvent {
        name: "s".into(),
        ctrl: true,
        ..ExtensionKeyEvent::default()
    });
    assert_eq!(requested_write(&save.actions).unwrap().1, "😀\n");

    let mut vertical = Harness::new("e\u{301}x\nab\n");
    vertical.changeset.files[0].hunks[0].new_start = 1;
    vertical.start();
    vertical.press(key("right", ""));
    vertical.press(key("down", ""));
    let layout = vertical.layout(40);
    let cursor = layout
        .rows
        .iter()
        .find(|row| {
            row_text(
                &layout,
                layout
                    .rows
                    .iter()
                    .position(|candidate| candidate.id == row.id)
                    .unwrap(),
            )
            .starts_with('▎')
        })
        .unwrap();
    assert_eq!(
        cursor
            .spans
            .iter()
            .map(|span| span.text.as_str())
            .collect::<String>(),
        "▎2 ab"
    );
    assert_eq!(cursor.spans[2].text, "b");
}

#[test]
fn truncates_wide_graphemes_by_terminal_cells_without_wrapping() {
    let mut harness = Harness::new("界界\n");
    harness.changeset.files[0].hunks[0].new_start = 1;
    harness.start();
    let layout = harness.layout(4);
    assert_eq!(row_text(&layout, 1), "▎1 …");
    assert!(validate_file_view_layout(&serde_json::to_value(layout).unwrap(), 1, 4).is_ok());
}

#[test]
fn mode_claims_editor_keys_but_preserves_documented_review_shortcuts() {
    let mut harness = Harness::new("alpha\nbeta\n");
    harness.start();
    assert_eq!(
        harness.press(key("z", "z")).result,
        KeyRoutingResult::Handled
    );
    for event in [
        key("]", "]"),
        ExtensionKeyEvent {
            name: "?".into(),
            sequence: "?".into(),
            shift: true,
            ..ExtensionKeyEvent::default()
        },
        key("q", "q"),
        key("tab", "\t"),
        key("f8", ""),
        ExtensionKeyEvent {
            name: "g".into(),
            sequence: "g".into(),
            ctrl: true,
            ..ExtensionKeyEvent::default()
        },
    ] {
        assert_eq!(harness.press(event).result, KeyRoutingResult::Pass);
    }
}

#[test]
fn ctrl_s_handles_flagged_and_bare_control_forms_and_preserves_newlines() {
    let mut harness = Harness::new("alpha\nbeta\n");
    harness.start();
    let unchanged = harness.press(ExtensionKeyEvent {
        name: "s".into(),
        ctrl: true,
        ..ExtensionKeyEvent::default()
    });
    assert_eq!(unchanged.result, KeyRoutingResult::Handled);
    assert!(
        matches!(&unchanged.actions[0], ExtensionHostAction::Notify { message, .. } if message == "No unsaved edits")
    );

    harness.press(key("z", "z"));
    let flagged = harness.press(ExtensionKeyEvent {
        name: "s".into(),
        ctrl: true,
        ..ExtensionKeyEvent::default()
    });
    let (request_id, text) = requested_write(&flagged.actions).unwrap();
    assert_eq!(text, "alpha\nzbeta\n");
    let request_id = request_id.to_owned();
    let actions = harness
        .extension
        .complete_write(&ExtensionWorkspaceWriteCompletion {
            request_id,
            result: ExtensionWorkspaceWriteResult::Written,
        })
        .actions;
    assert!(
        matches!(&actions[0], ExtensionHostAction::Notify { message, .. } if message == "Wrote alpha.ts")
    );

    harness.press(key("y", "y"));
    let bare = harness.press(key("", "\u{13}"));
    assert_eq!(requested_write(&bare.actions).unwrap().1, "alpha\nzybeta\n");

    let mut cr = Harness::new("alpha\rbeta\r");
    cr.start();
    cr.press(key("z", "z"));
    let save = cr.press(ExtensionKeyEvent {
        name: "s".into(),
        ctrl: true,
        ..ExtensionKeyEvent::default()
    });
    assert_eq!(requested_write(&save.actions).unwrap().1, "alpha\rzbeta\r");
}

#[test]
fn cancelled_and_failed_writes_keep_the_buffer_and_report_only_failure() {
    let mut harness = Harness::new("alpha\nbeta\n");
    harness.start();
    harness.press(key("z", "z"));
    let save = harness.press(ExtensionKeyEvent {
        name: "s".into(),
        ctrl: true,
        ..ExtensionKeyEvent::default()
    });
    let request_id = requested_write(&save.actions).unwrap().0.to_owned();
    assert!(
        harness
            .extension
            .complete_write(&ExtensionWorkspaceWriteCompletion {
                request_id,
                result: ExtensionWorkspaceWriteResult::Cancelled {
                    detail: "The write to src/demo.md was declined.".into(),
                }
            })
            .actions
            .is_empty()
    );

    let save = harness.press(ExtensionKeyEvent {
        name: "s".into(),
        ctrl: true,
        ..ExtensionKeyEvent::default()
    });
    let request_id = requested_write(&save.actions).unwrap().0.to_owned();
    let actions = harness
        .extension
        .complete_write(&ExtensionWorkspaceWriteCompletion {
            request_id,
            result: ExtensionWorkspaceWriteResult::Failed {
                detail: "Failed to write alpha.ts • EACCES".into(),
            },
        })
        .actions;
    assert!(
        matches!(&actions[0], ExtensionHostAction::Notify { message, notification_type: ExtensionNotifyType::Warning } if message.ends_with("EACCES"))
    );
    assert!(row_text(&harness.layout(40), 0).contains("MODIFIED"));
}

#[test]
fn a_second_command_cannot_replace_a_live_session_and_failed_entry_retries() {
    let mut harness = Harness::new("alpha\nbeta\n");
    harness.start();
    harness.press(key("z", "z"));
    let actions = harness
        .extension
        .invoke_command(&invocation(&harness.changeset))
        .unwrap()
        .actions;
    assert!(
        matches!(&actions[0], ExtensionHostAction::Notify { message, .. } if message == "Already editing alpha.ts — Esc exits, ctrl+s writes")
    );
    assert_eq!(row_text(&harness.layout(40), 2), "▎2 zbeta");

    harness.exit();
    harness.changeset.source = ChangesetSource::Patch {
        label: "patch".into(),
    };
    assert_eq!(
        harness
            .extension
            .invoke_command(&invocation(&harness.changeset))
            .unwrap()
            .actions
            .len(),
        1
    );
    assert_eq!(
        harness
            .extension
            .invoke_command(&invocation(&harness.changeset))
            .unwrap()
            .actions
            .len(),
        1
    );
}

#[test]
fn provenance_survives_mid_buffer_splits_and_hunk_geometry_follows_it() {
    let mut harness = Harness::whole_file();
    harness.start();
    harness.press(key("down", ""));
    harness.press(key("left", ""));
    harness.press(key("return", "\r"));
    let layout = harness.layout(40);
    assert_eq!(
        layout.rows[1..]
            .iter()
            .map(|row| row
                .spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>())
            .collect::<Vec<_>>(),
        [" 1 alpha", "▎2  ", " 3 beta", " 4 gamma"]
    );
    assert_eq!(layout.rows[1].source_ranges[0].range, [1, 1]);
    assert!(layout.rows[2].source_ranges.is_empty());
    assert_eq!(layout.rows[3].source_ranges[0].range, [2, 2]);
    assert_eq!(layout.rows[4].source_ranges[0].range, [3, 3]);
    assert_eq!(
        layout.hunk_rows[0],
        workdeck_extension_api::ExtensionFileViewHunkRows {
            start_row: 1,
            end_row: 4
        }
    );
    assert!(validate_file_view_layout(&serde_json::to_value(layout).unwrap(), 1, 40).is_ok());
}

#[test]
fn joins_keep_all_source_bindings_and_refuse_cross_hunk_ownership() {
    let mut harness = Harness::whole_file();
    harness.start();
    harness.press(key("down", ""));
    harness.press(key("backspace", ""));
    harness.press(key("z", "z"));
    let layout = harness.layout(40);
    assert_eq!(row_text(&layout, 1), "▎1 alphazbeta");
    assert_eq!(
        layout.rows[1]
            .source_ranges
            .iter()
            .map(|range| range.range)
            .collect::<Vec<_>>(),
        [[1, 1], [2, 2]]
    );
    assert_eq!(layout.rows[2].source_ranges[0].range, [3, 3]);
    assert_eq!(
        layout.hunk_rows[0],
        workdeck_extension_api::ExtensionFileViewHunkRows {
            start_row: 1,
            end_row: 2
        }
    );

    let mut two_hunks = Harness::whole_file();
    let mut second = two_hunks.changeset.files[0].hunks[0].clone();
    two_hunks.changeset.files[0].hunks[0].new_start = 1;
    two_hunks.changeset.files[0].hunks[0].new_count = 1;
    second.new_start = 2;
    second.new_count = 1;
    two_hunks.changeset.files[0].hunks.push(second);
    two_hunks.start();
    two_hunks.press(key("down", ""));
    two_hunks.press(key("backspace", ""));
    let layout = two_hunks.layout(40);
    assert_eq!(row_text(&layout, 1), " 1 alpha");
    assert_eq!(row_text(&layout, 2), "▎2 beta");
}

#[test]
fn a_single_line_hunk_stays_bound_when_its_line_joins_the_row_above() {
    let mut harness = Harness::new("alpha\nbeta\ngamma\n");
    harness.start();
    harness.press(key("backspace", ""));
    let layout = harness.layout(40);
    assert_eq!(row_text(&layout, 1), "▎1 alphabeta");
    assert_eq!(
        layout.rows[1]
            .source_ranges
            .iter()
            .map(|range| range.range)
            .collect::<Vec<_>>(),
        [[1, 1], [2, 2]]
    );
    assert_eq!(
        layout.hunk_rows[0],
        workdeck_extension_api::ExtensionFileViewHunkRows {
            start_row: 1,
            end_row: 1
        }
    );
    assert!(validate_file_view_layout(&serde_json::to_value(layout).unwrap(), 1, 40).is_ok());
}

#[test]
fn escape_discards_unsaved_edits_and_restores_read_only_layout() {
    let mut harness = Harness::new("alpha\nbeta\n");
    harness.start();
    harness.press(key("z", "z"));
    let actions = harness.exit();
    assert!(
        matches!(&actions[1], ExtensionHostAction::Notify { message, .. } if message == "Discarded unsaved edits to alpha.ts")
    );
    let layout = harness.layout(40);
    assert_eq!(row_text(&layout, 0), " 1 alpha");
    assert_eq!(harness.press(key("z", "z")).result, KeyRoutingResult::Pass);
}

fn staged_extension() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let binary_directory = directory.path().join("bin");
    fs::create_dir_all(&binary_directory).unwrap();
    let binary_name = format!(
        "workdeck-example-inline-edit-extension{}",
        std::env::consts::EXE_SUFFIX
    );
    fs::copy(
        env!("CARGO_BIN_EXE_workdeck-example-inline-edit-extension"),
        binary_directory.join(binary_name),
    )
    .unwrap();
    let manifest = directory.path().join("workdeck-extension.toml");
    fs::write(
        &manifest,
        include_str!("../extensions/inline-edit/workdeck-extension.toml"),
    )
    .unwrap();
    (directory, manifest)
}

fn terminal_text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn ratatui_host_confirms_guards_writes_and_exits_the_mode_after_success() {
    let repository = TempDir::new().unwrap();
    fs::write(repository.path().join("alpha.ts"), "alpha\nbeta\n").unwrap();
    let (_extension_directory, manifest) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let mut app = ReviewApp::new_with_extensions(
        changeset("alpha\nbeta\n"),
        ReviewOptions {
            repo: Some(repository.path().into()),
            command_cwd: Some(repository.path().into()),
            review_input: Some(writable_input()),
            ..ReviewOptions::default()
        },
        vec![loaded],
    );

    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    assert_eq!(app.active_keyboard_mode_title().as_deref(), Some(VIEW_ID));
    let file_id = app.shared_state().lock().unwrap().changeset().files[0]
        .runtime_id
        .clone();
    assert_eq!(
        app.selected_extension_file_view(&file_id).as_deref(),
        Some("example.inline-edit:inline-edit")
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(app.has_extension_dialog());
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    terminal
        .draw(|frame| render(frame.area(), frame.buffer_mut(), &app))
        .unwrap();
    let frame = terminal_text(&terminal);
    assert!(frame.contains("ext example.inline-edit"));
    assert!(frame.contains("Write alpha.ts?"));
    assert!(
        frame.contains("Extension example.inline-edit will replace this file's contents on disk.")
    );

    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(!app.has_extension_dialog());
    assert_eq!(
        fs::read_to_string(repository.path().join("alpha.ts")).unwrap(),
        "alpha\nbeta\n"
    );
    assert_eq!(app.active_keyboard_mode_title().as_deref(), Some(VIEW_ID));

    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(
        fs::read_to_string(repository.path().join("alpha.ts")).unwrap(),
        "alpha\nzbeta\n"
    );
    assert!(app.take_reload_requested());
    assert!(!app.take_reload_requested());
    assert_eq!(app.active_keyboard_mode_title(), None);
}

#[test]
fn ratatui_host_rejects_a_stale_working_tree_without_losing_the_edit_session() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.ts");
    fs::write(&path, "alpha\nbeta\n").unwrap();
    let (_extension_directory, manifest) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let mut app = ReviewApp::new_with_extensions(
        changeset("alpha\nbeta\n"),
        ReviewOptions {
            repo: Some(repository.path().into()),
            command_cwd: Some(repository.path().into()),
            review_input: Some(writable_input()),
            ..ReviewOptions::default()
        },
        vec![loaded],
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    fs::write(&path, "outside change\n").unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    assert_eq!(fs::read_to_string(&path).unwrap(), "outside change\n");
    assert_eq!(app.active_keyboard_mode_title().as_deref(), Some(VIEW_ID));
    assert!(!app.has_extension_dialog());
}

#[test]
fn ratatui_host_refuses_a_missing_target_before_and_after_consent() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.ts");
    fs::write(&path, "alpha\nbeta\n").unwrap();
    let (_extension_directory, manifest) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let mut app = ReviewApp::new_with_extensions(
        changeset("alpha\nbeta\n"),
        ReviewOptions {
            repo: Some(repository.path().into()),
            command_cwd: Some(repository.path().into()),
            review_input: Some(writable_input()),
            ..ReviewOptions::default()
        },
        vec![loaded],
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));

    fs::remove_file(&path).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(!app.has_extension_dialog());
    assert_eq!(app.active_keyboard_mode_title().as_deref(), Some(VIEW_ID));
    assert!(!app.take_reload_requested());

    fs::write(&path, "alpha\nbeta\n").unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(app.has_extension_dialog());
    fs::remove_file(&path).unwrap();
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!path.exists());
    assert!(!app.has_extension_dialog());
    assert_eq!(app.active_keyboard_mode_title().as_deref(), Some(VIEW_ID));
    assert!(!app.take_reload_requested());
}

#[test]
fn ratatui_host_retires_a_pending_workspace_write_when_the_review_reloads() {
    let repository = TempDir::new().unwrap();
    let path = repository.path().join("alpha.ts");
    fs::write(&path, "alpha\nbeta\n").unwrap();
    let (_extension_directory, manifest) = staged_extension();
    let loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let mut app = ReviewApp::new_with_extensions(
        changeset("alpha\nbeta\n"),
        ReviewOptions {
            repo: Some(repository.path().into()),
            command_cwd: Some(repository.path().into()),
            review_input: Some(writable_input()),
            ..ReviewOptions::default()
        },
        vec![loaded],
    );
    app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL));
    app.handle_key(KeyEvent::new(KeyCode::Char('z'), KeyModifiers::NONE));
    app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(app.has_extension_dialog());

    app.reload(changeset("alpha\nbeta\n"));
    assert!(!app.has_extension_dialog());
    assert_eq!(app.active_keyboard_mode_title(), None);
    app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(fs::read_to_string(path).unwrap(), "alpha\nbeta\n");
}

#[test]
fn compiled_subprocess_preserves_command_mode_layout_and_write_lifecycle() {
    let (_directory, manifest) = staged_extension();
    let mut loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let changeset = changeset("alpha\nbeta\n");
    let workspace = workdeck_tui::build_extension_workspace_snapshot(
        &changeset.files,
        &writable_input(),
        std::path::Path::new("/repo"),
        1,
    );
    let actions = loaded
        .invoke_command_with_workspace_context(
            COMMAND_ID,
            invocation(&changeset).snapshot,
            Vec::new(),
            None,
            "/repo".into(),
            None,
            Some(workspace),
        )
        .unwrap()
        .actions;
    assert!(matches!(
        actions[0],
        ExtensionHostAction::EnterFileViewMode { .. }
    ));
    let file = create_file_view_input_snapshot(&changeset.files[0]).file;
    let lifecycle = FileViewModeLifecycleRequest {
        view_id: VIEW_ID.into(),
        file: file.as_ref().clone(),
        cwd: "/repo".into(),
        review_generation: 1,
    };
    assert!(
        loaded
            .file_view_mode_lifecycle("workdeck/file-view-mode/enter", lifecycle.clone())
            .unwrap()
            .actions
            .is_empty()
    );
    let typed = loaded
        .route_file_view_mode_key(FileViewModeKeyRequest {
            view_id: VIEW_ID.into(),
            file: file.as_ref().clone(),
            key: key("z", "z"),
            cwd: "/repo".into(),
            review_generation: 1,
        })
        .unwrap();
    assert_eq!(typed.result, KeyRoutingResult::Handled);
    let save = loaded
        .route_file_view_mode_key(FileViewModeKeyRequest {
            view_id: VIEW_ID.into(),
            file: file.as_ref().clone(),
            key: ExtensionKeyEvent {
                name: "s".into(),
                ctrl: true,
                ..ExtensionKeyEvent::default()
            },
            cwd: "/repo".into(),
            review_generation: 1,
        })
        .unwrap();
    let request_id = requested_write(&save.actions).unwrap().0.to_owned();
    let completion = loaded
        .complete_workspace_write(ExtensionWorkspaceWriteCompletion {
            request_id,
            result: ExtensionWorkspaceWriteResult::Written,
        })
        .unwrap();
    assert!(
        matches!(&completion.actions[0], ExtensionHostAction::Notify { message, .. } if message == "Wrote alpha.ts")
    );
    assert!(loaded.file_view_mode_lifecycle("workdeck/file-view-mode/exit", lifecycle).unwrap().actions.iter().all(|action| !matches!(action, ExtensionHostAction::Notify { message, .. } if message.contains("Discarded"))));
}

#[test]
fn compiled_command_context_reads_and_requests_a_consented_workspace_write() {
    let (_directory, manifest) = staged_extension();
    let mut loaded = LoadedExtension::spawn(&manifest, "test").unwrap();
    let changeset = changeset("alpha\nbeta\n");
    let workspace = workdeck_tui::build_extension_workspace_snapshot(
        &changeset.files,
        &writable_input(),
        std::path::Path::new("/repo"),
        1,
    );
    let mut request = invocation(&changeset);
    request.command_id = REWRITE_COMMAND_ID.into();
    let actions = loaded
        .invoke_command_with_workspace_context(
            REWRITE_COMMAND_ID,
            request.snapshot,
            Vec::new(),
            None,
            "/repo".into(),
            None,
            Some(workspace),
        )
        .unwrap()
        .actions;
    assert!(matches!(
        actions.as_slice(),
        [ExtensionHostAction::RequestWorkspaceWrite { file_id, text, .. }]
            if file_id == &changeset.files[0].runtime_id && text == "ALPHA\nBETA\n"
    ));
}
