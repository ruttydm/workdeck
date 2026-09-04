use std::collections::BTreeSet;

use ratatui::style::Modifier;
use serde_json::{Value, json};
use workdeck_core::{AgentAnnotation, DiffFile, FileChangeKind, LineRange, ReviewSide};
use workdeck_diff::VisibleBodyBounds;
use workdeck_extension_api::{
    ExtensionFileViewHunkRows, ExtensionFileViewLayout, ExtensionFileViewRow,
    ExtensionFileViewRowComponent, ExtensionFileViewSpan, ValidatedFileViewLayout, ViewNode,
};
use workdeck_extension_host::validate_file_view_layout;
use workdeck_review::{VisibleFileViewNote, build_file_view_render_plan};

use super::*;
use crate::{measure_file_view_geometry, resolve_theme};

fn file(id: &str, path: &str) -> DiffFile {
    DiffFile {
        key: id.into(),
        runtime_id: id.into(),
        path: path.into(),
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        language: None,
        stats: Default::default(),
        flags: Default::default(),
        patch: String::new(),
        split_row_count: 0,
        stack_row_count: 0,
        hunks: Vec::new(),
        content_identity: String::new(),
        sources: Default::default(),
        source_identity: None,
        source_attested: false,
        agent: None,
    }
}

fn resolved(value: &Value, hunk_count: usize, width: usize) -> ValidatedFileViewLayout {
    validate_file_view_layout(value, hunk_count, width).expect("test layout is valid")
}

fn text(lines: &[ratatui::text::Line<'_>]) -> String {
    lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn identity(generation: u64) -> FileViewPaintIdentity<'static> {
    FileViewPaintIdentity {
        extension_id: "test",
        view_id: "view",
        registration_identity: 1,
        layout_generation: generation,
    }
}

fn annotation(id: &str, summary: &str, line: u32) -> AgentAnnotation {
    AgentAnnotation {
        id: Some(id.into()),
        old_range: None,
        new_range: Some(LineRange {
            start: line,
            end: line,
        }),
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

fn note(id: &str, summary: &str, line: u32) -> VisibleFileViewNote {
    VisibleFileViewNote {
        id: id.into(),
        annotation: annotation(id, summary, line),
        thread_depth: 0,
        has_actions: false,
    }
}

#[test]
fn highlights_every_rendered_row_inside_the_selected_hunk_bounds() {
    let layout = ExtensionFileViewLayout {
        rows: Vec::new(),
        hunk_rows: vec![
            ExtensionFileViewHunkRows {
                start_row: 0,
                end_row: 0,
            },
            ExtensionFileViewHunkRows {
                start_row: 0,
                end_row: 0,
            },
            ExtensionFileViewHunkRows {
                start_row: 1,
                end_row: 2,
            },
        ],
    };
    assert!(!is_file_view_row_selected(&layout, 0, Some(2)));
    assert!(is_file_view_row_selected(&layout, 1, Some(2)));
    assert!(is_file_view_row_selected(&layout, 2, Some(2)));
    assert!(!is_file_view_row_selected(&layout, 1, Some(1)));
}

#[test]
fn preserves_the_symbolic_only_renderer() {
    let value = json!({
        "rows": [
            {"id": "heading", "spans": [{"text": "Heading", "tone": "accent", "attributes": ["bold"]}]},
            {"id": "body", "spans": [{"text": "Body", "tone": "added", "attributes": ["italic"]}]},
            {"id": "tail", "spans": [{"text": "Tail", "tone": "removed", "attributes": ["underline", "strikethrough"]}]}
        ],
        "hunkRows": [
            {"startRow": 0, "endRow": 0},
            {"startRow": 0, "endRow": 0},
            {"startRow": 1, "endRow": 2}
        ]
    });
    let resolved = resolved(&value, 3, 20);
    let plan = build_file_view_render_plan(&resolved.layout, &[]);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 20);
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let file = file("symbolic", "symbolic.ts");
    let cursor = CursorHighlight {
        stable_key: "file-view:body".into(),
        style: crate::CursorHighlightStyle::Row,
        side: ReviewSide::New,
    };
    let painted = paint_file_view(FileViewViewOptions {
        file: &file,
        resolved: &resolved,
        geometry: &geometry,
        cursor_highlight: Some(&cursor),
        selected_hunk_index: Some(2),
        theme: &theme,
        visible_body_bounds: None,
        width: 20,
        identity: identity(1),
        expanded_row_ids: &BTreeSet::new(),
        now_ms: 0,
    });

    assert_eq!(text(&painted.lines()), "Heading\nBody\nTail");
    assert_eq!(
        painted.rows[1].review_row_id,
        review_row_id("file-view:body")
    );
    assert_eq!(painted.rows[1].height, 1);
    assert!(!painted.rows[0].selected);
    assert!(painted.rows[1].selected);
    assert!(painted.rows[2].selected);
    assert!(painted.rows[1].cursor_highlighted);
    assert_eq!(
        painted.rows[1].lines[0].style.bg,
        Some(ratatui_theme_color(&cursor_line_highlight_background(
            &theme.selected_hunk,
            &theme
        )))
    );
    assert_eq!(
        painted.rows[1].lines[0].spans[0].style.fg,
        Some(ratatui_theme_color(&theme.file_new))
    );
    assert!(
        painted.rows[1].lines[0].spans[0]
            .style
            .add_modifier
            .contains(Modifier::ITALIC)
    );
    assert_eq!(
        painted.rows[2].lines[0].spans[0].style.fg,
        Some(ratatui_theme_color(&theme.file_deleted))
    );
    let remaining_tones = paint_symbolic_file_view_row(
        &[
            ExtensionFileViewSpan {
                text: "m".into(),
                tone: Some(workdeck_extension_api::ExtensionFileViewTone::Muted),
                attributes: Vec::new(),
            },
            ExtensionFileViewSpan {
                text: "a".into(),
                tone: Some(workdeck_extension_api::ExtensionFileViewTone::AccentMuted),
                attributes: Vec::new(),
            },
            ExtensionFileViewSpan {
                text: "s".into(),
                tone: Some(workdeck_extension_api::ExtensionFileViewTone::Syntax),
                attributes: Vec::new(),
            },
            ExtensionFileViewSpan {
                text: "t".into(),
                tone: None,
                attributes: Vec::new(),
            },
        ],
        &theme,
        20,
    );
    assert_eq!(
        remaining_tones[0].spans[0].style.fg,
        Some(ratatui_theme_color(&theme.muted))
    );
    assert_eq!(
        remaining_tones[0].spans[1].style.fg,
        Some(ratatui_theme_color(&theme.accent_muted))
    );
    assert_eq!(
        remaining_tones[0].spans[2].style.fg,
        Some(ratatui_theme_color(&theme.syntax_colors.default))
    );
    assert_eq!(remaining_tones[0].spans[2].content, "st");
    assert_eq!(theme.syntax_colors.default, theme.text);
}

#[test]
fn renders_a_host_owned_note_immediately_after_its_bound_alternate_row() {
    let value = json!({
        "rows": [{
            "id": "bound",
            "spans": [{"text": "BOUND PRESENTATION"}],
            "sourceRanges": [{"side": "new", "range": [1, 2]}]
        }],
        "hunkRows": [{"startRow": 0, "endRow": 0}]
    });
    let resolved = resolved(&value, 1, 60);
    let notes = [note("note", "Review bound output", 1)];
    let plan = build_file_view_render_plan(&resolved.layout, &notes);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 60);
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let file = file("noted", "noted.ts");
    let painted = paint_file_view(FileViewViewOptions {
        file: &file,
        resolved: &resolved,
        geometry: &geometry,
        cursor_highlight: None,
        selected_hunk_index: Some(0),
        theme: &theme,
        visible_body_bounds: None,
        width: 60,
        identity: identity(1),
        expanded_row_ids: &BTreeSet::new(),
        now_ms: 0,
    });

    let frame = text(&painted.lines());
    assert!(frame.contains("BOUND PRESENTATION"));
    assert!(frame.contains("Review bound output"));
    assert!(frame.find("Review bound output") > frame.find("BOUND PRESENTATION"));
    assert_eq!(
        painted.rows[1].review_row_id,
        review_row_id("inline-note:note:file-view:bound:0")
    );
}

#[test]
fn mounts_custom_components_only_inside_the_host_row_window_with_bounded_props() {
    let value = json!({
        "rows": [
            {"id": "before", "spans": [{"text": "BEFORE"}]},
            {"id": "custom-a", "spans": [{"text": "FALLBACK A"}], "component": {"height": 2, "content": {"type": "empty"}}},
            {"id": "custom-b", "spans": [{"text": "FALLBACK B"}], "component": {"height": 2, "content": {"type": "empty"}}}
        ],
        "hunkRows": [
            {"startRow": 1, "endRow": 1},
            {"startRow": 2, "endRow": 2}
        ]
    });
    let resolved = resolved(&value, 2, 20);
    let plan = build_file_view_render_plan(&resolved.layout, &[]);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 20);
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let file = file("custom", "custom.ts");
    let mut props = Vec::new();
    let painted = paint_file_view_with(
        FileViewViewOptions {
            file: &file,
            resolved: &resolved,
            geometry: &geometry,
            cursor_highlight: None,
            selected_hunk_index: Some(0),
            theme: &theme,
            visible_body_bounds: Some(VisibleBodyBounds { top: 1, height: 2 }),
            width: 20,
            identity: identity(1),
            expanded_row_ids: &BTreeSet::new(),
            now_ms: 0,
        },
        |request| {
            props.push((
                request.width,
                request.height,
                request.selected,
                request.row_index,
                request.theme.appearance,
                request.theme.text.clone(),
            ));
            Ok(vec![ratatui::text::Line::from(format!(
                "CUSTOM {}",
                if request.row_index == 1 { "A" } else { "B" }
            ))])
        },
    );

    let frame = text(&painted.lines());
    assert!(frame.contains("CUSTOM A"));
    assert!(!frame.contains("CUSTOM B"));
    assert!(!frame.contains("BEFORE"));
    assert_eq!(props.len(), 1);
    assert_eq!(props[0].0, 20);
    assert_eq!(props[0].1, 2);
    assert!(props[0].2);
    assert_eq!(props[0].3, 1);
    assert_eq!(
        props[0].4,
        workdeck_extension_api::ExtensionThemeAppearance::Dark
    );
    assert_eq!(props[0].5, theme.text);
    assert_eq!(painted.top_spacer_height, 1);
    assert_eq!(painted.bottom_spacer_height, 2);
}

#[test]
fn repaints_live_semantic_theme_props_without_remounting_or_relayout() {
    let value = json!({
        "rows": [{"id": "themed", "spans": [{"text": "fallback"}], "component": {"height": 1, "content": {"type": "empty"}}}],
        "hunkRows": [{"startRow": 0, "endRow": 0}]
    });
    let resolved = resolved(&value, 1, 20);
    let plan = build_file_view_render_plan(&resolved.layout, &[]);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 20);
    let file = file("themed", "themed.ts");
    let dark = resolve_theme(Some("github-dark-default"), None, &[]);
    let light = resolve_theme(Some("github-light-default"), None, &[]);
    let mut paints = Vec::new();
    let mut render = |request: FileViewComponentPaintRequest<'_>| {
        paints.push((request.theme.appearance, request.theme.text.clone()));
        Ok(vec![ratatui::text::Line::from(format!(
            "{:?}",
            request.theme.appearance
        ))])
    };
    let dark_paint = paint_file_view_with(
        FileViewViewOptions {
            file: &file,
            resolved: &resolved,
            geometry: &geometry,
            cursor_highlight: None,
            selected_hunk_index: Some(0),
            theme: &dark,
            visible_body_bounds: None,
            width: 20,
            identity: identity(1),
            expanded_row_ids: &BTreeSet::new(),
            now_ms: 0,
        },
        &mut render,
    );
    let light_paint = paint_file_view_with(
        FileViewViewOptions {
            file: &file,
            resolved: &resolved,
            geometry: &geometry,
            cursor_highlight: None,
            selected_hunk_index: Some(0),
            theme: &light,
            visible_body_bounds: None,
            width: 20,
            identity: identity(1),
            expanded_row_ids: &BTreeSet::new(),
            now_ms: 0,
        },
        &mut render,
    );

    assert_eq!(
        paints[0].0,
        workdeck_extension_api::ExtensionThemeAppearance::Dark
    );
    assert_eq!(
        paints[1].0,
        workdeck_extension_api::ExtensionThemeAppearance::Light
    );
    assert_ne!(paints[0].1, paints[1].1);
    assert_eq!(resolved.row_heights, [1]);
    assert_eq!(
        dark_paint.rows[0].paint_identity,
        light_paint.rows[0].paint_identity
    );
}

#[test]
fn retains_ephemeral_state_across_selection_but_loses_it_on_unmount_and_generation() {
    let value = json!({
        "rows": [{"id": "stateful", "spans": [{"text": "fallback"}], "component": {"height": 1, "content": {"type": "text", "text": "state"}}}],
        "hunkRows": [{"startRow": 0, "endRow": 0}]
    });
    let resolved = resolved(&value, 1, 20);
    let plan = build_file_view_render_plan(&resolved.layout, &[]);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 20);
    let file = file("stateful", "state.ts");
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let expanded = BTreeSet::new();
    let mut mounts = FileViewComponentMountState::default();
    let make = |selected_hunk_index, visible_body_bounds, generation| {
        paint_file_view(FileViewViewOptions {
            file: &file,
            resolved: &resolved,
            geometry: &geometry,
            cursor_highlight: None,
            selected_hunk_index,
            theme: &theme,
            visible_body_bounds,
            width: 20,
            identity: identity(generation),
            expanded_row_ids: &expanded,
            now_ms: 0,
        })
    };

    assert_eq!(mounts.synchronize(&make(Some(0), None, 1)), [1]);
    assert_eq!(mounts.synchronize(&make(None, None, 1)), [1]);
    assert!(
        mounts
            .synchronize(&make(
                Some(0),
                Some(VisibleBodyBounds { top: 1, height: 0 }),
                1
            ))
            .is_empty()
    );
    assert_eq!(mounts.synchronize(&make(Some(0), None, 1)), [2]);
    assert_eq!(mounts.synchronize(&make(Some(0), None, 2)), [3]);
}

#[test]
fn mounts_only_visible_painters_from_a_one_thousand_row_component_layout() {
    let rows = (0..1_000)
        .map(|index| ExtensionFileViewRow {
            id: format!("row-{index}"),
            spans: vec![ExtensionFileViewSpan {
                text: format!("fallback {index}"),
                tone: None,
                attributes: Vec::new(),
            }],
            source_ranges: if index == 500 {
                vec![workdeck_extension_api::ExtensionFileViewSourceRange {
                    side: workdeck_extension_api::ExtensionFileSide::New,
                    range: [500, 500],
                }]
            } else {
                Vec::new()
            },
            component: Some(ExtensionFileViewRowComponent {
                height: 1,
                content: ViewNode::Empty,
                selected_content: None,
                expanded_content: None,
                selected_expanded_content: None,
                toggle_expanded_on_left_mouse_up: false,
                selection_prefix: None,
            }),
        })
        .collect::<Vec<_>>();
    let resolved = ValidatedFileViewLayout {
        layout: ExtensionFileViewLayout {
            rows,
            hunk_rows: vec![ExtensionFileViewHunkRows {
                start_row: 0,
                end_row: 999,
            }],
        },
        row_heights: vec![1; 1_000],
    };
    let notes = [note("windowed-note", "WINDOWED NOTE", 500)];
    let plan = build_file_view_render_plan(&resolved.layout, &notes);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 20);
    let file = file("large", "large.ts");
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let mut mounted = Vec::new();
    let painted = paint_file_view_with(
        FileViewViewOptions {
            file: &file,
            resolved: &resolved,
            geometry: &geometry,
            cursor_highlight: None,
            selected_hunk_index: Some(0),
            theme: &theme,
            visible_body_bounds: Some(VisibleBodyBounds {
                top: 500,
                height: 8,
            }),
            width: 20,
            identity: identity(1),
            expanded_row_ids: &BTreeSet::new(),
            now_ms: 0,
        },
        |request| {
            mounted.push(request.row_index);
            Ok(vec![ratatui::text::Line::from(format!(
                "paint {}",
                request.row_index
            ))])
        },
    );

    assert!(
        painted.rows.iter().any(|row| row.review_row_id
            == review_row_id("inline-note:windowed-note:file-view:row-500:0"))
    );
    assert_eq!(mounted, [500, 501, 502, 503]);
    assert!(!mounted.contains(&499));
    assert!(!mounted.contains(&504));
}

#[test]
fn clips_oversized_custom_output_to_fixed_host_geometry_and_retains_stable_row_ids() {
    let value = json!({
        "rows": [
            {
                "id": "clipped",
                "spans": [{"text": "CLIPPED FALLBACK"}],
                "component": {
                    "height": 1,
                    "content": {
                        "type": "column",
                        "children": [
                            {"type": "text", "text": "VISIBLE CUSTOM"},
                            {"type": "text", "text": "HIDDEN OVERFLOW"},
                            {"type": "text", "text": "HIDDEN OVERFLOW"}
                        ]
                    }
                }
            },
            {"id": "after", "spans": [{"text": "AFTER ROW"}]}
        ],
        "hunkRows": [{"startRow": 0, "endRow": 1}]
    });
    let resolved = resolved(&value, 1, 20);
    let plan = build_file_view_render_plan(&resolved.layout, &[]);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 20);
    let file = file("clipped", "clipped.ts");
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let painted = paint_file_view(FileViewViewOptions {
        file: &file,
        resolved: &resolved,
        geometry: &geometry,
        cursor_highlight: None,
        selected_hunk_index: Some(0),
        theme: &theme,
        visible_body_bounds: None,
        width: 20,
        identity: identity(1),
        expanded_row_ids: &BTreeSet::new(),
        now_ms: 0,
    });

    let frame = text(&painted.lines());
    assert!(frame.contains("VISIBLE CUSTOM"));
    assert!(!frame.contains("HIDDEN OVERFLOW"));
    assert_eq!(frame.lines().nth(1), Some("AFTER ROW"));
    assert_eq!(
        painted.rows[0].review_row_id,
        review_row_id("file-view:clipped")
    );
    assert_eq!(painted.rows[0].height, 1);
    assert_eq!(
        painted.rows[1].review_row_id,
        review_row_id("file-view:after")
    );
    assert_eq!(painted.rows[1].height, 1);
}

#[test]
fn contains_a_component_render_error_to_its_symbolic_row_fallback() {
    let value = json!({
        "rows": [{
            "id": "broken",
            "spans": [{"text": "SAFE FALLBACK"}],
            "component": {"height": 2, "content": {"type": "empty"}}
        }],
        "hunkRows": [{"startRow": 0, "endRow": 0}]
    });
    let resolved = resolved(&value, 1, 20);
    let plan = build_file_view_render_plan(&resolved.layout, &[]);
    let geometry = measure_file_view_geometry(&resolved, &plan.rows, 20);
    let file = file("broken", "broken.ts");
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let painted = paint_file_view_with(
        FileViewViewOptions {
            file: &file,
            resolved: &resolved,
            geometry: &geometry,
            cursor_highlight: None,
            selected_hunk_index: Some(0),
            theme: &theme,
            visible_body_bounds: None,
            width: 20,
            identity: identity(7),
            expanded_row_ids: &BTreeSet::new(),
            now_ms: 0,
        },
        |_| Err("broken custom row".into()),
    );

    assert!(text(&painted.lines()).contains("SAFE FALLBACK"));
    assert_eq!(painted.failures.len(), 1);
    assert_eq!(painted.failures[0].extension_id, "test");
    assert_eq!(painted.failures[0].view_id, "view");
    assert_eq!(painted.failures[0].file_id, "broken");
    assert_eq!(painted.failures[0].file_path, "broken.ts");
    assert_eq!(painted.failures[0].row_id, "broken");
    assert_eq!(painted.failures[0].layout_generation, 7);
    assert_eq!(painted.failures[0].message, "broken custom row");
    assert_eq!(painted.rows[0].height, 2);
    assert_eq!(painted.rows[0].lines.len(), 2);
    assert_eq!(
        painted.rows[0].lines[1].style.bg,
        Some(ratatui_theme_color(&theme.selected_hunk))
    );
}
