use super::*;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};

const WIDTH: u16 = 92;

fn capture(width: u16, height: u16, render: impl FnOnce(Rect, &mut Buffer)) -> String {
    let area = Rect::new(0, 0, width, height);
    let mut buffer = Buffer::empty(area);
    render(area, &mut buffer);
    buffer
        .content()
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn create_example_diff() -> DiffFile {
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "before",
            contents: "export const value = 1;\n",
            name: "example.ts",
        },
        FileSnapshot {
            cache_key: "after",
            contents: "export const value = 2;\nexport const added = true;\n",
            name: "example.ts",
        },
        FileComparisonOptions { context_radius: 3 },
    )
    .unwrap();
    file.runtime_id = "example".into();
    create_workdeck_diff_file(file)
}

fn with_identity(mut file: DiffFile, id: &str, path: &str) -> DiffFile {
    file.runtime_id = id.into();
    file.path = path.into();
    file.refresh_identity();
    file
}

fn sidebar_file(id: &str, path: &str) -> DiffFile {
    with_identity(create_example_diff(), id, path)
}

fn annotation(summary: &str) -> AgentAnnotation {
    AgentAnnotation {
        id: None,
        old_range: None,
        new_range: None,
        summary: summary.into(),
        rationale: None,
        markup: None,
        tags: Vec::new(),
        confidence: None,
        source: None,
        title: None,
        author: None,
        created_at: None,
        updated_at: None,
        editable: false,
    }
}

fn entry_label(entry: &FileSidebarEntry) -> String {
    match entry {
        FileSidebarEntry::Group { label, .. } | FileSidebarEntry::Directory { label, .. } => {
            label.clone()
        }
        FileSidebarEntry::File(file) => format!("{}{}", "  ".repeat(file.depth), file.name),
    }
}

fn sidebar_entry_json(entry: &FileSidebarEntry) -> serde_json::Value {
    match entry {
        FileSidebarEntry::Group { id, label } => serde_json::json!({
            "kind": "group",
            "id": id,
            "label": label,
        }),
        FileSidebarEntry::Directory { id, label, depth } => serde_json::json!({
            "kind": "directory",
            "id": id,
            "label": label,
            "depth": depth,
        }),
        FileSidebarEntry::File(file) => serde_json::json!({
            "kind": "file",
            "id": file.id,
            "name": file.name,
            "depth": file.depth,
            "agentCommentsText": file.agent_comments_text,
            "additionsText": file.additions_text,
            "deletionsText": file.deletions_text,
            "changeType": match file.change_type {
                SidebarFileChangeType::Change => "change",
                SidebarFileChangeType::New => "new",
                SidebarFileChangeType::Deleted => "deleted",
                SidebarFileChangeType::RenamePure => "rename-pure",
                SidebarFileChangeType::RenameChanged => "rename-changed",
            },
            "isUntracked": file.is_untracked,
        }),
    }
}

#[test]
fn native_sidebar_matches_the_executed_pinned_hunk_oracle() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../port/hunk/oracles/default-sidebar.json"
    ))
    .unwrap();
    let baseline = &oracle["baseline"];

    let mut alpha = sidebar_file("alpha", "src/ui/a.ts");
    alpha.stats.additions = 5;
    alpha.stats.deletions = 0;
    alpha.stats.truncated = true;
    alpha.change_kind = FileChangeKind::Added;
    alpha.agent = Some(AgentFileContext {
        path: alpha.path.clone(),
        summary: None,
        annotations: vec![annotation("one"), annotation("two")],
    });
    let mut renamed = sidebar_file("renamed", "src/new/name.ts");
    renamed.previous_path = Some("legacy/old.ts\n".into());
    renamed.stats.additions = 0;
    renamed.stats.deletions = 3;
    renamed.change_kind = FileChangeKind::Renamed;
    let mut root = sidebar_file("root", "README\tguide.md");
    root.stats.additions = 0;
    root.stats.deletions = 0;
    root.flags.untracked = true;
    let mut absolute = sidebar_file("absolute", "/tmp/project/a.ts");
    absolute.stats.additions = 1;
    absolute.stats.deletions = 1;
    let mut unc = sidebar_file("unc", "//server/share/b.ts");
    unc.stats.additions = 1;
    unc.stats.deletions = 1;
    let files = [alpha, renamed, root, absolute, unc];

    assert_eq!(
        serde_json::json!({
            "width31": match resolve_file_sidebar_mode(31) { FileSidebarMode::Flat => "flat", FileSidebarMode::Tree => "tree" },
            "width32": match resolve_file_sidebar_mode(32) { FileSidebarMode::Flat => "flat", FileSidebarMode::Tree => "tree" },
        }),
        baseline["modes"]
    );
    assert_eq!(
        serde_json::Value::Array(
            build_flat_sidebar_entries(&files)
                .iter()
                .map(sidebar_entry_json)
                .collect()
        ),
        baseline["flat"]
    );
    assert_eq!(
        serde_json::Value::Array(
            build_tree_sidebar_entries(&files)
                .iter()
                .map(sidebar_entry_json)
                .collect()
        ),
        baseline["tree"]
    );
    let FileSidebarEntry::File(first) = &build_flat_sidebar_entries(&files)[1] else {
        panic!("first file row");
    };
    let stats = sidebar_entry_stats(first)
        .into_iter()
        .map(|stat| {
            serde_json::json!({
                "kind": match stat.kind {
                    SidebarStatKind::AgentComment => "agent-comment",
                    SidebarStatKind::Addition => "addition",
                    SidebarStatKind::Deletion => "deletion",
                },
                "text": stat.text,
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(serde_json::Value::Array(stats), baseline["firstFileStats"]);
    assert_eq!(
        serde_json::json!(sidebar_entry_stats_width(first)),
        baseline["firstFileStatsWidth"]
    );
    assert_eq!(
        serde_json::json!({
            "filename": crate::file_header_label_parts(&{
                let mut file = sidebar_file("label", "agents/pi/extensions/notify.ts");
                file.previous_path = Some("pi/extensions/loop.ts\n".into());
                file.change_kind = FileChangeKind::Renamed;
                file
            }).0,
            "stateLabel": serde_json::Value::Null,
        }),
        baseline["renameLabel"]
    );
    assert_eq!(oracle["stable"]["status"], "absent");
}

#[test]
fn file_row_cells_keep_selection_state_icon_and_individual_badge_colors() {
    let mut file = sidebar_file("selected", "src/a.rs");
    file.change_kind = FileChangeKind::Added;
    file.stats.additions = 2;
    file.stats.deletions = 1;
    file.agent = Some(AgentFileContext {
        path: file.path.clone(),
        summary: None,
        annotations: vec![annotation("note")],
    });
    let area = Rect::new(0, 0, 32, 4);
    let mut buffer = Buffer::empty(area);
    render_workdeck_file_nav(
        area,
        &mut buffer,
        &[file],
        &WorkdeckFileNavOptions {
            selected_file_id: Some("selected".into()),
            ..WorkdeckFileNavOptions::default()
        },
    );
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let row = buffer
        .content()
        .chunks(usize::from(area.width))
        .nth(1)
        .unwrap();
    let text = row.iter().map(|cell| cell.symbol()).collect::<String>();
    assert!(text.contains("A a.rs"), "{text}");
    assert!(text.contains("*1 +2 -1"), "{text}");
    assert_eq!(row[0].bg, crate::ratatui_theme_color(&theme.accent));
    assert!(
        row.iter()
            .any(|cell| cell.symbol() == "A"
                && cell.fg == crate::ratatui_theme_color(&theme.file_new))
    );
    assert!(row.iter().any(|cell| {
        cell.symbol() == "*" && cell.fg == crate::ratatui_theme_color(&theme.note_border)
    }));
    assert!(row.iter().any(|cell| {
        cell.symbol() == "+" && cell.fg == crate::ratatui_theme_color(&theme.badge_added)
    }));
    assert!(row.iter().any(|cell| {
        cell.symbol() == "-" && cell.fg == crate::ratatui_theme_color(&theme.badge_removed)
    }));
    assert!(
        row.iter()
            .skip(1)
            .all(|cell| cell.bg == crate::ratatui_theme_color(&theme.panel_alt))
    );
}

#[test]
fn file_nav_window_culls_rows_and_translates_mouse_hits() {
    let files = (0..8)
        .map(|index| sidebar_file(&format!("file-{index}"), &format!("src/file-{index}.rs")))
        .collect::<Vec<_>>();
    let area = Rect::new(0, 0, 32, 3);
    let mut buffer = Buffer::empty(area);
    let map = render_workdeck_file_nav_window(
        area,
        &mut buffer,
        &files,
        &WorkdeckFileNavOptions::default(),
        4,
    );
    let rendered = buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(rendered.contains("file-3.rs"), "{rendered}");
    assert!(rendered.contains("file-5.rs"), "{rendered}");
    assert!(!rendered.contains("file-0.rs"));
    assert_eq!(workdeck_file_nav_selection_at(&map, 0), Some("file-3"));
    assert_eq!(workdeck_file_nav_selection_at(&map, 2), Some("file-5"));
}

#[test]
fn flat_sidebar_hides_zero_value_stats_and_keeps_rename_names() {
    let mut only_add = sidebar_file("only-add", "src/ui/only-add.ts");
    only_add.stats.additions = 5;
    only_add.stats.deletions = 0;
    let mut only_remove = sidebar_file("only-remove", "src/ui/only-remove.ts");
    only_remove.stats.additions = 0;
    only_remove.stats.deletions = 3;
    let mut renamed = sidebar_file("rename-only", "src/ui/Renamed.tsx");
    renamed.previous_path = Some("src/ui/Legacy.tsx".into());
    renamed.change_kind = FileChangeKind::Renamed;
    renamed.stats.additions = 0;
    renamed.stats.deletions = 0;

    let files = build_flat_sidebar_entries(&[only_add, only_remove, renamed])
        .into_iter()
        .filter_map(|entry| match entry {
            FileSidebarEntry::File(file) => Some(file),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 3);
    assert_eq!(files[0].name, "only-add.ts");
    assert_eq!(files[0].additions_text.as_deref(), Some("+5"));
    assert_eq!(files[0].deletions_text, None);
    assert_eq!(files[1].additions_text, None);
    assert_eq!(files[1].deletions_text.as_deref(), Some("-3"));
    assert_eq!(files[2].name, "Legacy.tsx -> Renamed.tsx");
    assert_eq!(files[2].additions_text, None);
    assert_eq!(files[2].deletions_text, None);
}

#[test]
fn flat_sidebar_counts_all_file_comments_before_diff_stats() {
    let mut file = sidebar_file("all-comments", "src/ui/commented.ts");
    file.stats.additions = 2;
    file.stats.deletions = 2;
    file.agent = Some(AgentFileContext {
        path: file.path.clone(),
        summary: None,
        annotations: vec![
            annotation("first hunk"),
            annotation("another first hunk"),
            annotation("off-range note"),
        ],
    });
    let FileSidebarEntry::File(entry) = &build_flat_sidebar_entries(&[file])[1] else {
        panic!("file row");
    };
    assert_eq!(entry.agent_comments_text.as_deref(), Some("*3"));
    assert_eq!(entry.additions_text.as_deref(), Some("+2"));
    assert_eq!(entry.deletions_text.as_deref(), Some("-2"));
    assert_eq!(
        sidebar_entry_stats(entry),
        vec![
            SidebarEntryStat {
                kind: SidebarStatKind::AgentComment,
                text: "*3".into(),
            },
            SidebarEntryStat {
                kind: SidebarStatKind::Addition,
                text: "+2".into(),
            },
            SidebarEntryStat {
                kind: SidebarStatKind::Deletion,
                text: "-2".into(),
            },
        ]
    );
    assert_eq!(sidebar_entry_stats_width(entry), 8);
}

#[test]
fn flat_sidebar_marks_each_root_file_run_in_place() {
    let entries = build_flat_sidebar_entries(&[
        sidebar_file("nested-a", "src/a.ts"),
        sidebar_file("root-a", "README.md"),
        sidebar_file("root-b", "package.json"),
        sidebar_file("nested-b", "test/b.ts"),
        sidebar_file("root-c", "LICENSE"),
    ]);
    assert_eq!(
        entries.iter().map(entry_label).collect::<Vec<_>>(),
        [
            "src/",
            "a.ts",
            "./",
            "README.md",
            "package.json",
            "test/",
            "b.ts",
            "./",
            "LICENSE",
        ]
    );
}

#[test]
fn sidebar_mode_switches_at_the_exact_content_width() {
    assert_eq!(resolve_file_sidebar_mode(31), FileSidebarMode::Flat);
    assert_eq!(resolve_file_sidebar_mode(32), FileSidebarMode::Tree);
}

#[test]
fn tree_sidebar_expands_paths_without_changing_file_order() {
    let files = vec![
        sidebar_file("ui-a", "src/ui/a.ts"),
        sidebar_file("ui-b", "src/ui/b.ts"),
        sidebar_file("root", "README.md"),
        sidebar_file("core", "src/core/c.ts"),
        sidebar_file("test", "test/d.ts"),
        sidebar_file("src-root", "src/e.ts"),
    ];
    let entries = build_tree_sidebar_entries(&files);
    assert_eq!(
        entries.iter().map(entry_label).collect::<Vec<_>>(),
        [
            "src/",
            "ui/",
            "    a.ts",
            "    b.ts",
            "README.md",
            "src/",
            "core/",
            "    c.ts",
            "test/",
            "  d.ts",
            "src/",
            "  e.ts",
        ]
    );
    let ids = entries
        .iter()
        .filter_map(|entry| match entry {
            FileSidebarEntry::File(file) => Some(file.id.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ids, files.iter().map(public_file_id).collect::<Vec<_>>());
}

#[test]
fn repeated_tree_directory_branches_have_unique_ids() {
    let directories = build_tree_sidebar_entries(&[
        sidebar_file("src-a", "src/a.ts"),
        sidebar_file("root", "README.md"),
        sidebar_file("src-b", "src/b.ts"),
    ])
    .into_iter()
    .filter_map(|entry| match entry {
        FileSidebarEntry::Directory { id, label, .. } => Some((id, label)),
        _ => None,
    })
    .collect::<Vec<_>>();
    assert_eq!(
        directories
            .iter()
            .map(|(_, label)| label.as_str())
            .collect::<Vec<_>>(),
        ["src/", "src/"]
    );
    assert_ne!(directories[0].0, directories[1].0);
}

#[test]
fn tree_sidebar_uses_current_rename_path_and_rename_filename() {
    let mut renamed = sidebar_file("renamed", "src/new/name.ts");
    renamed.previous_path = Some("legacy/old.ts".into());
    renamed.change_kind = FileChangeKind::Renamed;
    let entries = build_tree_sidebar_entries(&[renamed]);
    assert!(
        matches!(&entries[0], FileSidebarEntry::Directory { label, depth: 0, .. } if label == "src/")
    );
    assert!(
        matches!(&entries[1], FileSidebarEntry::Directory { label, depth: 1, .. } if label == "new/")
    );
    assert!(
        matches!(&entries[2], FileSidebarEntry::File(file) if file.name == "old.ts -> name.ts" && file.depth == 2)
    );
}

#[test]
fn tree_sidebar_preserves_absolute_and_unc_roots() {
    let entries = build_tree_sidebar_entries(&[
        sidebar_file("absolute", "/tmp/project/a.ts"),
        sidebar_file("unc", "//server/share/b.ts"),
    ]);
    assert_eq!(
        entries.iter().map(entry_label).collect::<Vec<_>>(),
        [
            "/",
            "tmp/",
            "project/",
            "      a.ts",
            "//",
            "server/",
            "share/",
            "      b.ts",
        ]
    );
}

#[test]
fn file_labels_and_sidebar_paths_escape_tabs_and_strip_rename_line_endings() {
    let tabbed = sidebar_file("tabbed", "src/tab\tname.ts");
    assert_eq!(
        crate::file_header_label_parts(&tabbed),
        ("src/tab\\tname.ts".into(), None)
    );
    assert!(matches!(
        &build_flat_sidebar_entries(&[tabbed])[1],
        FileSidebarEntry::File(file) if file.name == "tab\\tname.ts"
    ));

    let mut renamed = sidebar_file("rename", "agents/pi/extensions/notify.ts");
    renamed.previous_path = Some("pi/extensions/loop.ts\n".into());
    renamed.change_kind = FileChangeKind::Renamed;
    assert_eq!(
        crate::file_header_label_parts(&renamed),
        (
            "pi/extensions/loop.ts -> agents/pi/extensions/notify.ts".into(),
            None
        )
    );
}

#[test]
fn file_labels_keep_semantic_state_suffixes() {
    let mut untracked = sidebar_file("untracked", "draft.rs");
    untracked.flags.untracked = true;
    assert_eq!(
        crate::file_header_label_parts(&untracked).1,
        Some(" (untracked)")
    );
    let mut added = sidebar_file("added", "new.rs");
    added.change_kind = FileChangeKind::Added;
    assert_eq!(crate::file_header_label_parts(&added).1, Some(" (new)"));
    let mut deleted = sidebar_file("deleted", "old.rs");
    deleted.change_kind = FileChangeKind::Deleted;
    assert_eq!(
        crate::file_header_label_parts(&deleted).1,
        Some(" (deleted)")
    );
}

#[test]
fn annotation_merge_preserves_existing_summary_and_annotations() {
    let mut file = sidebar_file("annotated", "src/a.rs");
    file.agent = Some(AgentFileContext {
        path: file.path.clone(),
        summary: Some("existing summary".into()),
        annotations: vec![annotation("existing")],
    });
    let untouched = sidebar_file("untouched", "src/b.rs");
    let merged = merge_file_annotations_by_file_id(
        &[file, untouched.clone()],
        &BTreeMap::from([("annotated".into(), vec![annotation("new")])]),
    );
    let agent = merged[0].agent.as_ref().unwrap();
    assert_eq!(agent.summary.as_deref(), Some("existing summary"));
    assert_eq!(
        agent
            .annotations
            .iter()
            .map(|annotation| annotation.summary.as_str())
            .collect::<Vec<_>>(),
        ["existing", "new"]
    );
    assert_eq!(merged[1], untouched);
}

#[test]
fn renders_a_diff_through_the_public_ratatui_entrypoint() {
    let diff = create_example_diff();
    let frame = capture(WIDTH, 12, |area, buffer| {
        render_workdeck_diff_view(
            Rect { width: 88, ..area },
            buffer,
            Some(&diff),
            &WorkdeckDiffViewOptions {
                body: WorkdeckDiffBodyOptions {
                    layout: LayoutMode::Split,
                    theme: "github-dark-default".into(),
                    ..WorkdeckDiffBodyOptions::default()
                },
                scrollable: false,
                vertical_offset: 0,
            },
        );
    });

    assert!(frame.contains("@@ -1,1 +1,2 @@"), "{frame}");
    assert!(frame.contains("1 - export const value = 1;"));
    assert!(frame.contains("1 + export const value = 2;"));
    assert!(frame.contains("2 + export const added = true;"));
}

#[test]
fn renders_the_lower_level_single_file_body_primitive() {
    let diff = create_example_diff();
    let frame = capture(WIDTH, 12, |area, buffer| {
        render_workdeck_diff_body(
            Rect { width: 88, ..area },
            buffer,
            Some(&diff),
            &WorkdeckDiffBodyOptions {
                layout: LayoutMode::Stack,
                highlight: false,
                ..WorkdeckDiffBodyOptions::default()
            },
        );
    });

    assert!(frame.contains("@@ -1,1 +1,2 @@"), "{frame}");
    assert!(frame.contains("1   -  export const value = 1;"));
    assert!(frame.contains("  1 +  export const value = 2;"));
}

#[test]
fn accepts_a_custom_tab_width_through_the_public_body_primitive() {
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            cache_key: "tabs-before",
            contents: "a\tb\n",
            name: "tabs.txt",
        },
        FileSnapshot {
            cache_key: "tabs-after",
            contents: "a\tc\n",
            name: "tabs.txt",
        },
        FileComparisonOptions::default(),
    )
    .unwrap();
    file.runtime_id = "tabs".into();
    let frame = capture(WIDTH, 8, |area, buffer| {
        render_workdeck_diff_body(
            Rect { width: 88, ..area },
            buffer,
            Some(&file),
            &WorkdeckDiffBodyOptions {
                layout: LayoutMode::Stack,
                tab_width: 8,
                highlight: false,
                ..WorkdeckDiffBodyOptions::default()
            },
        );
    });

    assert!(frame.contains("a       c"));
}

#[test]
fn inserts_planned_hunk_gap_rows_before_later_hunk_headers() {
    let file = create_workdeck_diff_files_from_patch(
        "diff --git a/multi.ts b/multi.ts\n--- a/multi.ts\n+++ b/multi.ts\n@@ -2 +2 @@\n-export const line2 = 2;\n+export const line2 = 200;\n@@ -11 +11 @@\n-export const line11 = 11;\n+export const line11 = 1100;\n",
        "gaps",
    )
    .unwrap()
    .remove(0);
    let frame = capture(WIDTH, 24, |area, buffer| {
        render_workdeck_diff_body(
            Rect { width: 88, ..area },
            buffer,
            Some(&file),
            &WorkdeckDiffBodyOptions {
                layout: LayoutMode::Stack,
                hunk_gap: 2,
                highlight: false,
                ..WorkdeckDiffBodyOptions::default()
            },
        );
    });
    let lines = frame.lines().collect::<Vec<_>>();
    let headers = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| line.contains("@@").then_some(index))
        .collect::<Vec<_>>();

    assert!(headers.len() >= 2);
    assert!(lines[headers[1] - 2].trim().is_empty());
    assert!(lines[headers[1] - 1].trim().is_empty());
}

#[test]
fn renders_reusable_file_header_and_multi_file_review_stream_primitives() {
    let diff = create_example_diff();
    let frame = capture(WIDTH, 14, |area, buffer| {
        render_workdeck_diff_file_header(
            Rect {
                width: 88,
                height: 1,
                ..area
            },
            buffer,
            &diff,
            &WorkdeckDiffFileHeaderOptions {
                theme: "github-light-default".into(),
                ..WorkdeckDiffFileHeaderOptions::default()
            },
        );
        render_workdeck_review_stream(
            Rect {
                y: 1,
                width: 88,
                height: 13,
                ..area
            },
            buffer,
            std::slice::from_ref(&diff),
            &WorkdeckReviewStreamOptions {
                body: WorkdeckDiffBodyOptions {
                    theme: "github-light-default".into(),
                    ..WorkdeckDiffBodyOptions::default()
                },
                ..WorkdeckReviewStreamOptions::default()
            },
        );
    });

    assert!(frame.contains("example.ts"));
    assert!(frame.contains("+2 -1"));
    assert!(frame.contains("@@ -1,1 +1,2 @@"), "{frame}");
}

#[test]
fn renders_filename_tabs_as_fixed_width_escapes_in_headers_and_navigation() {
    let diff = with_identity(create_example_diff(), "tabbed-path", "src/tab\tname.ts");
    let frame = capture(WIDTH, 8, |area, buffer| {
        render_workdeck_diff_file_header(
            Rect {
                width: 88,
                height: 1,
                ..area
            },
            buffer,
            &diff,
            &WorkdeckDiffFileHeaderOptions::default(),
        );
        render_workdeck_file_nav(
            Rect {
                y: 1,
                width: 32,
                height: 7,
                ..area
            },
            buffer,
            std::slice::from_ref(&diff),
            &WorkdeckFileNavOptions {
                selected_file_id: Some("tabbed-path".into()),
                ..WorkdeckFileNavOptions::default()
            },
        );
    });

    assert!(frame.contains("src/tab\\tname.ts"));
    assert!(frame.contains("tab\\tname.ts"));
    assert!(!frame.contains('\t'));
}

#[test]
fn renders_the_dedicated_file_navigation_primitive() {
    let diff = create_example_diff();
    let frame = capture(36, 8, |area, buffer| {
        render_workdeck_file_nav(
            Rect { width: 32, ..area },
            buffer,
            std::slice::from_ref(&diff),
            &WorkdeckFileNavOptions {
                selected_file_id: Some("example".into()),
                ..WorkdeckFileNavOptions::default()
            },
        );
    });

    assert!(frame.contains("example.ts"));
    assert!(frame.contains("+2 -1"));
}

#[test]
fn uses_a_single_ellipsis_when_a_file_navigation_name_is_truncated() {
    let file = with_identity(
        create_example_diff(),
        "long-name",
        "src/extraordinarily-long-component-name",
    );
    let frame = capture(22, 6, |area, buffer| {
        render_workdeck_file_nav(
            Rect { width: 18, ..area },
            buffer,
            std::slice::from_ref(&file),
            &WorkdeckFileNavOptions::default(),
        );
    });

    assert!(frame.contains('…'));
    assert!(!frame.contains("..."));
}

#[test]
fn adapts_file_navigation_from_grouped_paths_to_an_expanded_hierarchy() {
    let example = create_example_diff();
    let files = vec![
        with_identity(example.clone(), "alpha", "src/ui/alpha.ts"),
        with_identity(example, "beta", "src/ui/beta.ts"),
    ];
    let narrow = capture(36, 8, |area, buffer| {
        render_workdeck_file_nav(
            Rect { width: 32, ..area },
            buffer,
            &files,
            &WorkdeckFileNavOptions::default(),
        );
    });
    let wide = capture(36, 8, |area, buffer| {
        render_workdeck_file_nav(
            Rect { width: 33, ..area },
            buffer,
            &files,
            &WorkdeckFileNavOptions::default(),
        );
    });

    assert!(narrow.contains("src/ui/"));
    assert!(!wide.contains("src/ui/"));
    assert!(wide.contains("src/"));
    assert!(wide.contains("ui/"));
    assert_eq!(
        wide.lines()
            .find(|line| line.contains("src/"))
            .and_then(|line| line.find("src/")),
        Some(1)
    );
    assert_eq!(
        wide.lines()
            .find(|line| line.contains("ui/"))
            .and_then(|line| line.find("ui/")),
        Some(3)
    );
    assert!(wide.contains("alpha.ts"));
    assert!(wide.contains("beta.ts"));
}

#[test]
fn creates_public_file_models_from_patch_text() {
    let files = create_workdeck_diff_files_from_patch(
        "diff --git a/example.ts b/example.ts\n--- a/example.ts\n+++ b/example.ts\n@@ -1 +1,2 @@\n-export const value = 1;\n+export const value = 2;\n+export const added = true;\n",
        "patch",
    )
    .unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "example.ts");
    assert_eq!(files[0].stats.additions, 2);
    assert_eq!(files[0].stats.deletions, 1);
    assert!(
        files[0]
            .patch
            .contains("diff --git a/example.ts b/example.ts")
    );
}

#[test]
fn normalizes_noprefix_patch_text_for_public_file_models() {
    let files = create_workdeck_diff_files_from_patch(
        "diff --git example.ts example.ts\n--- example.ts\n+++ example.ts\n@@ -1 +1 @@\n-before\n+after\n",
        "patch",
    )
    .unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "example.ts");
    assert!(
        files[0]
            .patch
            .contains("diff --git a/example.ts b/example.ts")
    );
}

#[test]
fn decodes_git_quoted_unicode_paths_for_public_file_models() {
    let escaped = r"\345\233\275\351\232\233\345\214\226/\346\227\245\346\234\254\350\252\236-\360\237\247\252.txt";
    let files = create_workdeck_diff_files_from_patch(
        &format!(
            "diff --git \"a/{escaped}\" \"b/{escaped}\"\n--- \"a/{escaped}\"\n+++ \"b/{escaped}\"\n@@ -1 +1 @@\n-before\n+after\n"
        ),
        "patch",
    )
    .unwrap();

    assert_eq!(files.len(), 1);
    assert_eq!(files[0].path, "国際化/日本語-🧪.txt");
}

#[test]
fn preserves_exact_trailing_controls_from_git_quoted_public_patch_paths() {
    let escaped = r"line\n";
    let files = create_workdeck_diff_files_from_patch(
        &format!(
            "diff --git \"a/{escaped}\" \"b/{escaped}\"\n--- \"a/{escaped}\"\n+++ \"b/{escaped}\"\n@@ -1 +1 @@\n-before\n+after\n"
        ),
        "patch",
    )
    .unwrap();

    assert_eq!(files[0].path, "line\n");
}

#[test]
fn exports_the_bundled_theme_names() {
    for theme in [
        "github-dark-default",
        "github-light-default",
        "dracula",
        "catppuccin-mocha",
    ] {
        assert!(WORKDECK_DIFF_THEME_NAMES.contains(&theme));
    }
    assert_eq!(WORKDECK_DIFF_THEME_NAMES.len(), 65);
}

#[test]
fn horizontal_offset_is_cell_safe_and_wrapping_deliberately_ignores_it() {
    let file = create_workdeck_diff_files_from_patch(
        "diff --git a/offset.txt b/offset.txt\n--- a/offset.txt\n+++ b/offset.txt\n@@ -1 +1 @@\n-0123456789\n+ab界cdefgh\n",
        "offset",
    )
    .unwrap()
    .remove(0);
    let scrolled = capture(30, 8, |area, buffer| {
        render_workdeck_diff_body(
            area,
            buffer,
            Some(&file),
            &WorkdeckDiffBodyOptions {
                layout: LayoutMode::Stack,
                show_line_numbers: false,
                horizontal_offset: 4,
                highlight: false,
                ..WorkdeckDiffBodyOptions::default()
            },
        );
    });
    let wrapped = capture(12, 12, |area, buffer| {
        render_workdeck_diff_body(
            area,
            buffer,
            Some(&file),
            &WorkdeckDiffBodyOptions {
                layout: LayoutMode::Stack,
                show_line_numbers: false,
                horizontal_offset: 4,
                wrap_lines: true,
                highlight: false,
                ..WorkdeckDiffBodyOptions::default()
            },
        );
    });

    assert!(scrolled.contains("456789"));
    assert!(scrolled.contains("cdefgh"));
    assert!(wrapped.contains("012345678"), "{wrapped}");
    assert!(wrapped.contains("ab界 cdefg"), "{wrapped}");
}

#[test]
fn public_hit_maps_preserve_callback_targets_without_owning_input() {
    let diff = create_example_diff();
    let area = Rect::new(0, 0, 32, 8);
    let mut buffer = Buffer::empty(area);
    let map = render_workdeck_file_nav(
        area,
        &mut buffer,
        std::slice::from_ref(&diff),
        &WorkdeckFileNavOptions::default(),
    );

    assert_eq!(workdeck_file_nav_selection_at(&map, 1), Some("example"));
    assert_eq!(workdeck_file_nav_selection_at(&map, 0), None);
}

#[test]
fn empty_public_surfaces_explain_absent_files_and_each_non_text_change() {
    let no_file = capture(48, 3, |area, buffer| {
        render_workdeck_diff_body(area, buffer, None, &WorkdeckDiffBodyOptions::default());
    });
    let no_files = capture(48, 3, |area, buffer| {
        render_workdeck_review_stream(area, buffer, &[], &WorkdeckReviewStreamOptions::default());
    });
    assert!(no_file.contains("No file selected."));
    assert!(no_files.contains("No files to render."));

    let mut empty = create_workdeck_diff_files_from_patch(
        "diff --git a/a.txt b/a.txt\n--- a/a.txt\n+++ b/a.txt\n",
        "empty",
    )
    .unwrap()
    .remove(0);
    for (kind, binary, too_large, expected) in [
        (
            FileChangeKind::Renamed,
            false,
            false,
            "only renames the file",
        ),
        (FileChangeKind::Modified, true, false, "Binary file skipped"),
        (
            FileChangeKind::Modified,
            false,
            true,
            "too large to render automatically",
        ),
        (FileChangeKind::Added, false, false, "marked as new"),
        (FileChangeKind::Deleted, false, false, "marked as deleted"),
        (
            FileChangeKind::Modified,
            false,
            false,
            "No textual hunks to render",
        ),
    ] {
        empty.change_kind = kind;
        empty.flags.binary = binary;
        empty.flags.too_large = too_large;
        let frame = capture(80, 3, |area, buffer| {
            render_workdeck_diff_body(
                area,
                buffer,
                Some(&empty),
                &WorkdeckDiffBodyOptions::default(),
            );
        });
        assert!(frame.contains(expected), "{frame}");
    }
}

#[test]
fn public_body_honors_header_number_and_selected_hunk_options() {
    let file = create_example_diff();
    let area = Rect::new(0, 0, 60, 8);
    let mut buffer = Buffer::empty(area);
    render_workdeck_diff_body(
        area,
        &mut buffer,
        Some(&file),
        &WorkdeckDiffBodyOptions {
            layout: LayoutMode::Stack,
            show_line_numbers: false,
            show_hunk_headers: false,
            selected_hunk_index: Some(0),
            highlight: false,
            ..WorkdeckDiffBodyOptions::default()
        },
    );
    let frame = buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!frame.contains("@@"));
    assert!(!frame.contains("1   -"));
    assert!(frame.contains("▌- export const value = 1;"));
    let code_cell = buffer
        .content()
        .iter()
        .find(|cell| cell.symbol() == "e")
        .expect("rendered code cell");
    assert_eq!(code_cell.bg, Color::Rgb(57, 45, 20));
}

#[test]
fn public_view_scrolls_rows_and_translates_its_hunk_hit_map() {
    let file = create_example_diff();
    let area = Rect::new(0, 0, 60, 6);
    let mut buffer = Buffer::empty(area);
    let map = render_workdeck_diff_view(
        area,
        &mut buffer,
        Some(&file),
        &WorkdeckDiffViewOptions {
            body: WorkdeckDiffBodyOptions::default(),
            scrollable: true,
            vertical_offset: 1,
        },
    );
    let frame = buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!frame.lines().next().unwrap_or_default().contains("@@"));
    assert!(map.hunk_rows.is_empty());
}

#[test]
fn public_stream_can_hide_headers_keep_separators_and_report_file_hits() {
    let first = create_example_diff();
    let second = with_identity(first.clone(), "second", "src/second.ts");
    let files = [first, second];
    let area = Rect::new(0, 0, 60, 16);
    let mut buffer = Buffer::empty(area);
    let map = render_workdeck_review_stream(
        area,
        &mut buffer,
        &files,
        &WorkdeckReviewStreamOptions {
            show_file_headers: false,
            show_file_separators: true,
            file_gap: 2,
            ..WorkdeckReviewStreamOptions::default()
        },
    );
    let frame = buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");

    assert!(!frame.contains("example.ts"));
    assert!(!frame.contains("second.ts"));
    assert!(frame.contains("────"));
    assert!(map.file_rows.is_empty());
    assert!(map.hunk_rows.iter().any(|(file_id, _)| file_id == "second"));
}

#[test]
fn public_theme_option_reaches_native_header_and_code_palettes() {
    let file = create_example_diff();
    let area = Rect::new(0, 0, 60, 8);
    let mut buffer = Buffer::empty(area);
    render_workdeck_diff_file_header(
        Rect { height: 1, ..area },
        &mut buffer,
        &file,
        &WorkdeckDiffFileHeaderOptions {
            theme: "github-light-default".into(),
            selected: false,
        },
    );
    render_workdeck_diff_body(
        Rect {
            y: 1,
            height: 7,
            ..area
        },
        &mut buffer,
        Some(&file),
        &WorkdeckDiffBodyOptions {
            layout: LayoutMode::Stack,
            theme: "github-light-default".into(),
            highlight: false,
            selected_hunk_index: None,
            ..WorkdeckDiffBodyOptions::default()
        },
    );

    let header_text = buffer
        .content()
        .iter()
        .find(|cell| cell.symbol() == "e")
        .expect("header path cell");
    assert_eq!(header_text.fg, Color::Rgb(31, 35, 40));
    assert_eq!(header_text.bg, Color::Rgb(255, 255, 255));
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.bg == Color::Rgb(226, 236, 229))
    );
    assert!(
        buffer
            .content()
            .iter()
            .any(|cell| cell.bg == Color::Rgb(249, 228, 230))
    );
}

#[test]
fn public_palette_uses_exact_bundled_surfaces_foregrounds_and_accents() {
    let dracula = public_palette("dracula");
    assert_eq!(dracula.panel, Color::Rgb(40, 42, 54));
    assert_eq!(dracula.text, Color::Rgb(248, 248, 242));
    assert_eq!(dracula.added, Color::Rgb(80, 250, 123));
    assert_eq!(dracula.removed, Color::Rgb(255, 85, 85));
    assert_eq!(dracula.accent, Color::Rgb(139, 233, 253));

    let dawn = public_palette("rose-pine-dawn");
    assert_eq!(dawn.panel, Color::Rgb(250, 244, 237));
    assert_eq!(dawn.text, Color::Rgb(87, 82, 121));
    assert_eq!(dawn.added, Color::Rgb(86, 148, 159));
    assert_eq!(dawn.removed, Color::Rgb(180, 99, 122));
    assert_eq!(dawn.accent, Color::Rgb(198, 120, 116));

    assert_eq!(
        public_palette("graphite"),
        public_palette("github-dark-default")
    );
}
