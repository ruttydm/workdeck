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
