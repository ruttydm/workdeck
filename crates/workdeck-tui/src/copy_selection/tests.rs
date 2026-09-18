use super::*;

use workdeck_core::DiffFile;
use workdeck_diff::{
    DIFF_RAIL_PREFIX_WIDTH, DiffRow, FileComparisonOptions, FileSnapshot, SplitLineKind,
    StackLineKind, diff_from_file_snapshots,
};

use crate::{
    DiffSectionGeometryCache, DiffSectionGeometryOptions, LineCursorTarget,
    build_file_section_layouts, resolve_theme,
};

struct TestHarness {
    files: Vec<DiffFile>,
    geometries: Vec<Arc<DiffSectionGeometry>>,
    layouts: Vec<FileSectionLayout>,
    layout: LayoutMode,
    width: usize,
    copy_decorations: bool,
    wrap_lines: bool,
    reserve_add_note_column: bool,
    show_line_numbers: bool,
}

impl TestHarness {
    fn context(&self) -> CopySelectionContext<'_> {
        CopySelectionContext {
            code_horizontal_offset: 0,
            copy_decorations: self.copy_decorations,
            files: &self.files,
            file_section_layouts: &self.layouts,
            header_label_width: 60,
            header_stats_width: 12,
            layout: self.layout,
            pinned_header_file: self.files.first(),
            reserve_add_note_column: self.reserve_add_note_column,
            section_geometry: &self.geometries,
            show_hunk_headers: true,
            show_line_numbers: self.show_line_numbers,
            width: self.width,
            wrap_lines: self.wrap_lines,
        }
    }
}

fn test_diff_file(before: &str, after: &str, id: &str, path: &str) -> DiffFile {
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            name: path,
            contents: before,
            cache_key: &format!("{id}-before"),
        },
        FileSnapshot {
            name: path,
            contents: after,
            cache_key: &format!("{id}-after"),
        },
        FileComparisonOptions { context_radius: 3 },
    )
    .expect("test snapshots differ");
    file.runtime_id = id.into();
    file
}

fn example_file() -> DiffFile {
    test_diff_file(
        "export const answer = 41;\nexport const stable = true;\n",
        "export const answer = 42;\nexport const stable = true;\nexport const added = true;\n",
        "example",
        "example.ts",
    )
}

fn build_harness(
    files: Vec<DiffFile>,
    layout: LayoutMode,
    width: usize,
    copy_decorations: bool,
    wrap_lines: bool,
    reserve_add_note_column: bool,
    show_line_numbers: bool,
) -> TestHarness {
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let mut cache = DiffSectionGeometryCache::default();
    let geometries = files
        .iter()
        .map(|file| {
            let mut options = DiffSectionGeometryOptions::new(file, layout, &theme);
            options.width = width;
            options.wrap_lines = wrap_lines;
            options.reserve_add_note_column = reserve_add_note_column;
            options.show_line_numbers = show_line_numbers;
            cache.measure(options)
        })
        .collect::<Vec<_>>();
    let heights = geometries
        .iter()
        .map(|geometry| i64::try_from(geometry.body_height).unwrap())
        .collect::<Vec<_>>();
    let layouts = build_file_section_layouts(&files, &heights, None, 1);
    TestHarness {
        files,
        geometries,
        layouts,
        layout,
        width,
        copy_decorations,
        wrap_lines,
        reserve_add_note_column,
        show_line_numbers,
    }
}

fn default_harness(layout: LayoutMode) -> TestHarness {
    build_harness(vec![example_file()], layout, 120, true, false, false, true)
}

fn review_point(column: usize, visual_row: i64) -> CopySelectionPoint {
    CopySelectionPoint::ReviewRow { column, visual_row }
}

fn pinned_point(column: usize, file_id: &str, next_visual_row: i64) -> CopySelectionPoint {
    CopySelectionPoint::PinnedHeader {
        column,
        file_id: file_id.into(),
        next_visual_row,
    }
}

fn added_row_index(geometry: &DiffSectionGeometry, layout: LayoutMode) -> usize {
    geometry
        .planned_rows()
        .iter()
        .position(|row| match (layout, row.diff_row()) {
            (LayoutMode::Stack, Some(DiffRow::StackLine { cell, .. })) => {
                cell.kind == StackLineKind::Addition
            }
            (LayoutMode::Split, Some(DiffRow::SplitLine { right, .. })) => {
                right.kind == SplitLineKind::Addition
            }
            _ => false,
        })
        .expect("addition row")
}

fn row_code_start(
    context: CopySelectionContext<'_>,
    geometry: &DiffSectionGeometry,
    row_index: usize,
    side: Option<CopySelectionSide>,
) -> usize {
    let plan = plan_code_row_layout(
        &geometry.planned_rows()[row_index],
        CodeRowLayoutOptions {
            width: context.width,
            line_number_digits: geometry.line_number_digits,
            show_line_numbers: context.show_line_numbers,
            wrap_lines: context.wrap_lines,
            reserve_add_note_column: context.reserve_add_note_column,
            show_add_note_badge: false,
        },
    )
    .expect("code row layout");
    match plan {
        CodeRowLayoutPlan::Stack { cell, .. } => cell.prefix_width + cell.gutter_width,
        CodeRowLayoutPlan::Split {
            left,
            right,
            left_pane_width,
            ..
        } => match side {
            Some(CopySelectionSide::Left) => left.prefix_width + left.gutter_width,
            Some(CopySelectionSide::Right) | None => {
                left_pane_width + right.prefix_width + right.gutter_width
            }
        },
    }
}

fn row_visual_top(harness: &TestHarness, row_index: usize) -> i64 {
    harness.layouts[0].body_top
        + i64::try_from(harness.geometries[0].row_bounds[row_index].bounds.top).unwrap()
}

fn render_whole_section(harness: &TestHarness, context: CopySelectionContext<'_>) -> String {
    let section = &harness.layouts[0];
    render_copy_selection_text(
        context,
        &review_point(0, section.body_top),
        &review_point(context.width.saturating_sub(1), section.section_bottom - 1),
        None,
    )
}

fn assert_no_unsafe_terminal_controls(text: &str) {
    for control in [
        "\u{1b}]52;c;SGVsbG8=\u{7}",
        "\u{1b}[2J",
        "\u{1b}Pqpayload\u{1b}\\",
    ] {
        assert!(!text.contains(control));
    }
    for control in ['\u{7}', '\r', '\u{8}', '\u{1b}'] {
        assert!(!text.contains(control));
    }
}

#[test]
fn clamps_below_zero_to_zero() {
    assert_eq!(clamp_copy_column(-5, 10), 0);
}

#[test]
fn clamps_above_the_rendered_width() {
    assert_eq!(clamp_copy_column(99, 10), 9);
}

#[test]
fn returns_zero_when_width_is_zero() {
    assert_eq!(clamp_copy_column(5, 0), 0);
}

#[test]
fn resolves_exact_split_sides_and_treats_context_rows_as_one_cursor() {
    let harness = default_harness(LayoutMode::Split);
    let geometry = &harness.geometries[0];
    let changed_index = added_row_index(geometry, LayoutMode::Split);
    let changed = &geometry.row_bounds[changed_index];
    let old = LineCursor {
        file_id: "example".into(),
        hunk_index: 0,
        stable_key: changed.stable_key.clone(),
        target: LineCursorTarget {
            side: ReviewSide::Old,
            line: 1,
        },
        expanded_gap_key: None,
    };
    let new = LineCursor {
        target: LineCursorTarget {
            side: ReviewSide::New,
            line: 1,
        },
        ..old.clone()
    };
    let cursors = vec![old.clone(), new.clone()];
    let point = review_point(10, row_visual_top(&harness, changed_index));
    assert_eq!(
        find_line_cursor_for_click(
            &cursors,
            &harness.layouts,
            &point,
            &harness.geometries,
            Some(CopySelectionSide::Left),
        ),
        Some(&old)
    );
    assert_eq!(
        find_line_cursor_for_click(
            &cursors,
            &harness.layouts,
            &point,
            &harness.geometries,
            Some(CopySelectionSide::Right),
        ),
        Some(&new)
    );

    let context_index = geometry
        .row_bounds
        .iter()
        .position(|bounds| is_context_line_stable_key(&bounds.stable_key))
        .expect("context row");
    let context_cursor = LineCursor {
        file_id: "example".into(),
        hunk_index: 0,
        stable_key: geometry.row_bounds[context_index].stable_key.clone(),
        target: LineCursorTarget {
            side: ReviewSide::New,
            line: 2,
        },
        expanded_gap_key: None,
    };
    assert_eq!(
        find_line_cursor_for_click(
            std::slice::from_ref(&context_cursor),
            &harness.layouts,
            &review_point(10, row_visual_top(&harness, context_index)),
            &harness.geometries,
            Some(CopySelectionSide::Left),
        ),
        Some(&context_cursor)
    );
}

#[test]
fn resolves_a_stacked_row_and_ignores_non_line_rows() {
    let harness = default_harness(LayoutMode::Stack);
    let row_index = added_row_index(&harness.geometries[0], LayoutMode::Stack);
    let cursor = LineCursor {
        file_id: "example".into(),
        hunk_index: 0,
        stable_key: harness.geometries[0].row_bounds[row_index]
            .stable_key
            .clone(),
        target: LineCursorTarget {
            side: ReviewSide::New,
            line: 1,
        },
        expanded_gap_key: None,
    };
    assert_eq!(
        find_line_cursor_for_click(
            std::slice::from_ref(&cursor),
            &harness.layouts,
            &review_point(10, row_visual_top(&harness, row_index)),
            &harness.geometries,
            None,
        ),
        Some(&cursor)
    );
    assert_eq!(
        find_line_cursor_for_click(
            std::slice::from_ref(&cursor),
            &harness.layouts,
            &review_point(10, harness.layouts[0].body_top),
            &harness.geometries,
            None,
        ),
        None
    );
}

#[test]
fn accepts_one_cell_mouse_jitter_around_a_click() {
    let drag = CopySelectionDrag {
        anchor: review_point(20, 8),
        focus: review_point(21, 9),
        moved: true,
        expanded: false,
    };
    assert!(copy_selection_drag_is_click(&drag));
}

#[test]
fn rejects_deliberate_drags_and_double_click_expansion() {
    let deliberate = CopySelectionDrag {
        anchor: review_point(20, 8),
        focus: review_point(22, 8),
        moved: true,
        expanded: false,
    };
    assert!(!copy_selection_drag_is_click(&deliberate));
    assert!(!copy_selection_drag_is_click(&CopySelectionDrag {
        focus: review_point(21, 8),
        expanded: true,
        ..deliberate
    }));
}

#[test]
fn rejects_different_kinds_even_at_the_same_column() {
    assert!(!copy_selection_points_equal(
        &review_point(1, 1),
        &pinned_point(1, "example", 1)
    ));
}

#[test]
fn matches_identical_review_row_points() {
    assert!(copy_selection_points_equal(
        &review_point(2, 4),
        &review_point(2, 4)
    ));
}

#[test]
fn treats_pinned_header_points_with_different_file_ids_as_distinct() {
    assert!(!copy_selection_points_equal(
        &pinned_point(0, "one", 0),
        &pinned_point(0, "two", 0)
    ));
}

#[test]
fn matches_review_row_points_on_the_same_visual_row() {
    assert!(copy_selection_points_share_row(
        &review_point(2, 4),
        &review_point(20, 4)
    ));
}

#[test]
fn rejects_review_row_points_on_different_visual_rows() {
    assert!(!copy_selection_points_share_row(
        &review_point(2, 4),
        &review_point(2, 5)
    ));
}

#[test]
fn orders_forward_selections_by_row_then_column() {
    let anchor = review_point(2, 1);
    let focus = review_point(8, 1);
    assert_eq!(
        normalize_copy_selection_range(&anchor, &focus),
        NormalizedCopySelectionRange {
            start: anchor,
            end: focus
        }
    );
}

#[test]
fn flips_reverse_selections_so_start_precedes_end() {
    let anchor = review_point(5, 3);
    let focus = review_point(2, 1);
    assert_eq!(
        normalize_copy_selection_range(&anchor, &focus),
        NormalizedCopySelectionRange {
            start: focus,
            end: anchor
        }
    );
}

#[test]
fn sorts_a_pinned_header_point_above_its_body() {
    let header = pinned_point(0, "example", 2);
    let body = review_point(0, 2);
    assert_eq!(
        normalize_copy_selection_range(&body, &header),
        NormalizedCopySelectionRange {
            start: header,
            end: body
        }
    );
}

#[test]
fn returns_a_review_row_point_for_a_row_inside_the_body() {
    let harness = default_harness(LayoutMode::Stack);
    let probe = harness.layouts[0].body_top;
    assert_eq!(
        find_copy_selection_point(4, true, &harness.layouts, &harness.geometries, probe, 120,),
        Some(review_point(4, probe))
    );
}

#[test]
fn returns_null_for_rows_past_the_end_of_the_stream() {
    let harness = default_harness(LayoutMode::Stack);
    assert_eq!(
        find_copy_selection_point(
            0,
            true,
            &harness.layouts,
            &harness.geometries,
            harness.layouts[0].section_bottom + 50,
            120,
        ),
        None
    );
}

#[test]
fn produces_decorated_text_for_a_single_row_drag() {
    let harness = default_harness(LayoutMode::Stack);
    let row = harness.layouts[0].body_top;
    let text = render_copy_selection_text(
        harness.context(),
        &review_point(0, row),
        &review_point(119, row),
        None,
    );
    assert!(text.starts_with('▌'));
}

#[test]
fn strips_all_decorations_when_copy_decorations_is_disabled() {
    let harness = default_harness(LayoutMode::Stack);
    let mut context = harness.context();
    context.copy_decorations = false;
    let text = render_whole_section(&harness, context);
    assert!(!text.contains('▌'));
    assert!(text.contains("export const answer = 41;"));
    assert!(text.contains("export const answer = 42;"));
}

#[test]
fn code_only_single_row_selections_preserve_selected_columns() {
    let harness = default_harness(LayoutMode::Stack);
    let mut context = harness.context();
    context.copy_decorations = false;
    let row_index = added_row_index(&harness.geometries[0], LayoutMode::Stack);
    let code_start = row_code_start(context, &harness.geometries[0], row_index, None);
    let visual_row = row_visual_top(&harness, row_index);
    assert_eq!(
        render_copy_selection_text(
            context,
            &review_point(code_start + 7, visual_row),
            &review_point(code_start + 11, visual_row),
            None,
        ),
        "const"
    );
}

#[test]
fn includes_the_pinned_header_when_the_drag_starts_in_it() {
    let harness = default_harness(LayoutMode::Stack);
    let body_top = harness.layouts[0].body_top;
    let text = render_copy_selection_text(
        harness.context(),
        &pinned_point(0, "example", body_top),
        &review_point(119, body_top),
        None,
    );
    assert!(text.contains("example.ts"));
}

#[test]
fn does_not_include_terminal_controls_from_copied_paths_or_code() {
    let payload =
        "\u{1b}]52;c;SGVsbG8=\u{7}\u{1b}[2J\u{1b}Pqpayload\u{1b}\\\u{7}\rspoof\u{8}hidden\u{1b}";
    let file = test_diff_file(
        &format!("export const answer = \"before{payload}\";\n"),
        &format!("export const answer = \"after{payload}\";\n"),
        "malicious",
        &format!("evil{payload}.ts"),
    );
    let harness = build_harness(vec![file], LayoutMode::Stack, 160, true, false, false, true);
    let section = &harness.layouts[0];
    let text = render_copy_selection_text(
        harness.context(),
        &pinned_point(0, "malicious", section.body_top),
        &review_point(159, section.section_bottom - 1),
        None,
    );
    assert!(text.contains("evil"));
    assert!(text.contains("before"));
    assert!(text.contains("after"));
    assert_no_unsafe_terminal_controls(&text);
}

#[test]
fn clips_wrapped_code_only_selections_across_partial_first_middle_and_last_lines() {
    let source = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let file = test_diff_file("", &format!("{source}\n"), "wrapped", "wrapped.txt");
    let harness = build_harness(vec![file], LayoutMode::Stack, 24, false, true, false, true);
    let context = harness.context();
    let geometry = &harness.geometries[0];
    let row_index = added_row_index(geometry, LayoutMode::Stack);
    let bounds = &geometry.row_bounds[row_index].bounds;
    assert!(bounds.height > 2);
    let code_start = row_code_start(context, geometry, row_index, None);
    let CodeRowLayoutPlan::Stack { cell, .. } = plan_code_row_layout(
        &geometry.planned_rows()[row_index],
        CodeRowLayoutOptions {
            width: 24,
            line_number_digits: geometry.line_number_digits,
            show_line_numbers: true,
            wrap_lines: true,
            reserve_add_note_column: false,
            show_add_note_badge: false,
        },
    )
    .unwrap() else {
        panic!("stack row")
    };
    let chunks = source
        .as_bytes()
        .chunks(cell.content_width)
        .map(|chunk| std::str::from_utf8(chunk).unwrap())
        .collect::<Vec<_>>();
    let row_top = row_visual_top(&harness, row_index);
    let text = render_copy_selection_text(
        context,
        &review_point(code_start + 2, row_top),
        &review_point(
            code_start + 4,
            row_top + i64::try_from(bounds.height).unwrap() - 1,
        ),
        None,
    );
    let mut expected = vec![chunks[0][2..].to_owned()];
    expected.extend(
        chunks[1..chunks.len() - 1]
            .iter()
            .map(|chunk| (*chunk).to_owned()),
    );
    expected.push(chunks.last().unwrap()[..5].to_owned());
    assert_eq!(text, expected.join("\n"));
}

#[test]
fn omits_blank_source_lines_from_code_only_output() {
    let file = test_diff_file(
        "const first = 1;\nconst last = 2;\n",
        "const first = 1;\n\nconst last = 2;\n",
        "blank",
        "blank.ts",
    );
    let harness = build_harness(
        vec![file],
        LayoutMode::Stack,
        120,
        false,
        false,
        false,
        true,
    );
    let text = render_whole_section(&harness, harness.context());
    assert_eq!(text, "const first = 1;\nconst last = 2;");
    assert!(!text.contains("\n\n"));
}

#[test]
fn retains_an_empty_partial_first_line_only_in_decorated_output() {
    let harness = default_harness(LayoutMode::Stack);
    let section = &harness.layouts[0];
    let start = review_point(119, section.body_top);
    let end = review_point(119, section.body_top + 1);
    let decorated = render_copy_selection_text(harness.context(), &start, &end, None);
    let mut code_context = harness.context();
    code_context.copy_decorations = false;
    let code = render_copy_selection_text(code_context, &start, &end, None);
    assert!(decorated.starts_with('\n'));
    assert!(!code.starts_with('\n'));
    assert!(code.contains("export const answer = 41;"));
}

#[test]
fn clips_an_in_stream_file_header_at_the_end_of_a_multi_file_selection() {
    let first = test_diff_file(
        "first file before\n",
        "first file after\n",
        "first",
        "first.txt",
    );
    let second = test_diff_file(
        "second file before\n",
        "second file after\n",
        "second",
        "second.txt",
    );
    let harness = build_harness(
        vec![first, second],
        LayoutMode::Stack,
        120,
        true,
        false,
        false,
        true,
    );
    let text = render_copy_selection_text(
        harness.context(),
        &review_point(0, harness.layouts[0].section_bottom - 1),
        &review_point(7, harness.layouts[1].header_top),
        None,
    );
    assert!(text.contains("first file after"));
    assert_eq!(text.lines().next_back(), Some(" second."));
    assert!(!text.contains("second file after"));
}

#[test]
fn returns_no_side_in_stack_layout() {
    assert_eq!(
        resolve_copy_selection_side(10, LayoutMode::Stack, 120),
        None
    );
    assert_eq!(
        resolve_copy_selection_side(80, LayoutMode::Stack, 120),
        None
    );
}

#[test]
fn returns_left_for_columns_inside_the_split_left_pane() {
    assert_eq!(
        resolve_copy_selection_side(0, LayoutMode::Split, 120),
        Some(CopySelectionSide::Left)
    );
    assert_eq!(
        resolve_copy_selection_side(10, LayoutMode::Split, 120),
        Some(CopySelectionSide::Left)
    );
}

#[test]
fn returns_right_for_columns_at_or_past_the_split_midpoint() {
    assert_eq!(
        resolve_copy_selection_side(100, LayoutMode::Split, 120),
        Some(CopySelectionSide::Right)
    );
}

#[test]
fn clips_partial_code_only_text_against_the_split_right_pane_origin() {
    let harness = default_harness(LayoutMode::Split);
    let mut context = harness.context();
    context.copy_decorations = false;
    let row_index = added_row_index(&harness.geometries[0], LayoutMode::Split);
    let code_start = row_code_start(
        context,
        &harness.geometries[0],
        row_index,
        Some(CopySelectionSide::Right),
    );
    let visual_row = row_visual_top(&harness, row_index);
    assert_eq!(
        render_copy_selection_text(
            context,
            &review_point(code_start + 7, visual_row),
            &review_point(code_start + 11, visual_row),
            Some(CopySelectionSide::Right),
        ),
        "const"
    );
}

#[test]
fn includes_only_the_left_side_when_decorations_are_off() {
    let harness = default_harness(LayoutMode::Split);
    let mut context = harness.context();
    context.copy_decorations = false;
    let section = &harness.layouts[0];
    let text = render_copy_selection_text(
        context,
        &review_point(0, section.body_top),
        &review_point(10, section.section_bottom - 1),
        Some(CopySelectionSide::Left),
    );
    assert!(text.contains("export const answer = 41;"));
    assert!(!text.contains("export const answer = 42;"));
}

#[test]
fn includes_only_the_right_side_when_decorations_are_off() {
    let harness = default_harness(LayoutMode::Split);
    let mut context = harness.context();
    context.copy_decorations = false;
    let section = &harness.layouts[0];
    let text = render_copy_selection_text(
        context,
        &review_point(0, section.body_top),
        &review_point(10, section.section_bottom - 1),
        Some(CopySelectionSide::Right),
    );
    assert!(text.contains("export const answer = 42;"));
    assert!(!text.contains("export const answer = 41;"));
}

#[test]
fn returns_an_empty_map_when_the_drag_has_not_moved() {
    let harness = default_harness(LayoutMode::Stack);
    let point = review_point(0, 0);
    let drag = CopySelectionDrag {
        anchor: point.clone(),
        focus: point,
        moved: false,
        expanded: false,
    };
    assert!(
        build_copy_selected_row_keys(Some(&drag), &harness.layouts, &harness.geometries, 120,)
            .is_empty()
    );
}

fn selected_range_case(
    row_top: i64,
    row_height: usize,
    reverse: bool,
    pinned_start: bool,
) -> CopySelectedRowRange {
    let start = if pinned_start {
        pinned_point(11, "example", 15)
    } else {
        review_point(11, 15)
    };
    let end = review_point(29, 20);
    let normalized = if reverse {
        normalize_copy_selection_range(&end, &start)
    } else {
        normalize_copy_selection_range(&start, &end)
    };
    let (start_row, end_row) = copy_selection_body_range(&normalized.start, &normalized.end);
    selected_range_for_row_bounds(
        10 + row_top,
        row_height,
        start_row,
        end_row,
        normalized.start.column(),
        normalized.end.column(),
        80,
    )
    .unwrap()
}

#[test]
fn clips_an_unwrapped_row_at_the_selection_start_column() {
    assert_eq!(
        selected_range_case(5, 1, false, false),
        CopySelectedRowRange {
            start_col: 11,
            end_col: 79
        }
    );
}

#[test]
fn selects_an_unwrapped_row_inside_the_selection_at_full_width() {
    assert_eq!(
        selected_range_case(7, 1, false, false),
        CopySelectedRowRange {
            start_col: 0,
            end_col: 79
        }
    );
}

#[test]
fn clips_an_unwrapped_row_at_the_inclusive_selection_end_column() {
    assert_eq!(
        selected_range_case(10, 1, false, false),
        CopySelectedRowRange {
            start_col: 0,
            end_col: 29
        }
    );
}

#[test]
fn clips_a_wrapped_row_beginning_before_the_selection() {
    assert_eq!(
        selected_range_case(4, 3, false, false),
        CopySelectedRowRange {
            start_col: 11,
            end_col: 79
        }
    );
}

#[test]
fn clips_a_wrapped_row_ending_after_the_selection() {
    assert_eq!(
        selected_range_case(9, 3, false, false),
        CopySelectedRowRange {
            start_col: 0,
            end_col: 29
        }
    );
}

#[test]
fn clips_a_wrapped_row_spanning_the_selection_during_a_reverse_drag() {
    assert_eq!(
        selected_range_case(4, 8, true, false),
        CopySelectedRowRange {
            start_col: 11,
            end_col: 29
        }
    );
}

#[test]
fn selects_a_wrapped_row_inside_the_selection_at_full_width() {
    assert_eq!(
        selected_range_case(7, 2, false, false),
        CopySelectedRowRange {
            start_col: 0,
            end_col: 79
        }
    );
}

#[test]
fn keeps_reverse_body_to_pinned_header_drag_on_body_row_boundaries() {
    assert_eq!(
        selected_range_case(5, 1, true, true),
        CopySelectedRowRange {
            start_col: 11,
            end_col: 79
        }
    );
}

fn addition_line(harness: &TestHarness) -> (usize, usize, i64, String) {
    let context = harness.context();
    let geometry = &harness.geometries[0];
    let row_index = added_row_index(geometry, harness.layout);
    let side = (harness.layout == LayoutMode::Split).then_some(CopySelectionSide::Right);
    let code_start = row_code_start(context, geometry, row_index, side);
    let visual_row = row_visual_top(harness, row_index);
    let text = render_code_only_planned_row_text(
        &geometry.planned_rows()[row_index],
        row_text_options(context, side, geometry.line_number_digits),
    )
    .into_iter()
    .next()
    .unwrap();
    (row_index, code_start, visual_row, text)
}

#[test]
fn triple_click_with_code_only_copy_selects_the_code_line() {
    let harness = default_harness(LayoutMode::Stack);
    let mut context = harness.context();
    context.copy_decorations = false;
    let (_, code_start, visual_row, line) = addition_line(&harness);
    assert_eq!(
        expand_selection_point(&review_point(code_start + 10, visual_row), 3, context),
        Some(ExpandedCopySelectionRange {
            start_col: code_start,
            end_col: code_start + measure_text_width(&line) - 1
        })
    );
}

#[test]
fn triple_click_in_stack_selects_the_full_width() {
    let harness = default_harness(LayoutMode::Stack);
    assert_eq!(
        expand_selection_point(
            &review_point(40, harness.layouts[0].body_top),
            3,
            harness.context(),
        ),
        Some(ExpandedCopySelectionRange {
            start_col: 0,
            end_col: 119
        })
    );
}

#[test]
fn triple_click_in_split_on_left_stays_within_left_pane() {
    let harness = default_harness(LayoutMode::Split);
    let left_width = resolve_split_pane_widths(120).left_width;
    let result = expand_selection_point(
        &review_point(5, harness.layouts[0].body_top),
        3,
        harness.context(),
    );
    assert_eq!(
        result,
        Some(ExpandedCopySelectionRange {
            start_col: 0,
            end_col: left_width - 1
        })
    );
    assert_eq!(
        resolve_copy_selection_side(result.unwrap().start_col, LayoutMode::Split, 120),
        Some(CopySelectionSide::Left)
    );
}

#[test]
fn triple_click_in_split_on_right_stays_within_right_pane() {
    let harness = default_harness(LayoutMode::Split);
    let left_width = resolve_split_pane_widths(120).left_width;
    let result = expand_selection_point(
        &review_point(
            left_width + DIFF_RAIL_PREFIX_WIDTH + 1,
            harness.layouts[0].body_top,
        ),
        3,
        harness.context(),
    );
    assert_eq!(
        result,
        Some(ExpandedCopySelectionRange {
            start_col: left_width,
            end_col: 119
        })
    );
    assert_eq!(
        resolve_copy_selection_side(result.unwrap().start_col, LayoutMode::Split, 120),
        Some(CopySelectionSide::Right)
    );
}

#[test]
fn double_click_on_whitespace_selects_the_character_itself() {
    let harness = default_harness(LayoutMode::Stack);
    let (_, code_start, visual_row, line) = addition_line(&harness);
    let local = line.find(' ').unwrap();
    let space_col = code_start + local;
    assert_eq!(
        expand_selection_point(&review_point(space_col, visual_row), 2, harness.context(),),
        Some(ExpandedCopySelectionRange {
            start_col: space_col,
            end_col: space_col
        })
    );
}

#[test]
fn double_click_on_a_word_stops_at_code_punctuation() {
    let harness = default_harness(LayoutMode::Stack);
    let (_, code_start, visual_row, line) = addition_line(&harness);
    let number = line.find("42").unwrap();
    assert_eq!(
        expand_selection_point(
            &review_point(code_start + number, visual_row),
            2,
            harness.context(),
        ),
        Some(ExpandedCopySelectionRange {
            start_col: code_start + number,
            end_col: code_start + number + 1
        })
    );
}

#[test]
fn decorated_right_side_uses_the_correct_column_offset() {
    let harness = default_harness(LayoutMode::Split);
    let section = &harness.layouts[0];
    let left_width = resolve_split_pane_widths(120).left_width;
    let text = render_copy_selection_text(
        harness.context(),
        &review_point(left_width + DIFF_RAIL_PREFIX_WIDTH + 1, section.body_top),
        &review_point(
            left_width + DIFF_RAIL_PREFIX_WIDTH + 1,
            section.section_bottom - 1,
        ),
        Some(CopySelectionSide::Right),
    );
    assert!(!text.is_empty());
    assert!(text.contains("export const answer = 42;"));
}

#[test]
fn decorated_left_side_stays_intact() {
    let harness = default_harness(LayoutMode::Split);
    let section = &harness.layouts[0];
    let text = render_copy_selection_text(
        harness.context(),
        &review_point(DIFF_RAIL_PREFIX_WIDTH + 1, section.body_top),
        &review_point(DIFF_RAIL_PREFIX_WIDTH + 1, section.section_bottom - 1),
        Some(CopySelectionSide::Left),
    );
    assert!(text.contains("export const answer = 41;"));
    assert!(!text.contains("export const answer = 42;"));
}

#[test]
fn decorated_right_side_multi_line_selection_includes_all_lines() {
    let harness = default_harness(LayoutMode::Split);
    let section = &harness.layouts[0];
    let left_width = resolve_split_pane_widths(120).left_width;
    let text = render_copy_selection_text(
        harness.context(),
        &review_point(left_width + DIFF_RAIL_PREFIX_WIDTH + 1, section.body_top),
        &review_point(119, section.section_bottom - 1),
        Some(CopySelectionSide::Right),
    );
    assert!(text.contains("export const answer = 42;"));
    assert!(text.lines().count() > 1);
}

fn wrapped_boundary_harness(layout: LayoutMode, reserve: bool) -> TestHarness {
    build_harness(
        vec![test_diff_file("", "1234567\n", "boundary", "boundary.ts")],
        layout,
        if layout == LayoutMode::Split { 20 } else { 10 },
        true,
        true,
        reserve,
        false,
    )
}

fn assert_wrapped_add_note_parity(layout: LayoutMode) {
    let unreserved = wrapped_boundary_harness(layout, false);
    let harness = wrapped_boundary_harness(layout, true);
    let geometry = &harness.geometries[0];
    let row_index = added_row_index(geometry, layout);
    let unreserved_index = added_row_index(&unreserved.geometries[0], layout);
    assert_eq!(
        unreserved.geometries[0].row_bounds[unreserved_index]
            .bounds
            .height,
        1
    );
    assert_eq!(geometry.row_bounds[row_index].bounds.height, 2);
    let context = harness.context();
    let side = (layout == LayoutMode::Split).then_some(CopySelectionSide::Right);
    let pane_start = if layout == LayoutMode::Split {
        resolve_split_pane_widths(context.width).left_width
    } else {
        0
    };
    let row_top = row_visual_top(&harness, row_index);
    let row_end =
        row_top + i64::try_from(geometry.row_bounds[row_index].bounds.height).unwrap() - 1;
    let decorated = render_copy_selection_text(
        context,
        &review_point(pane_start, row_top),
        &review_point(context.width - 1, row_end),
        side,
    );
    assert_eq!(decorated.lines().count(), 2);
    let mut code_context = context;
    code_context.copy_decorations = false;
    assert_eq!(
        render_copy_selection_text(
            code_context,
            &review_point(pane_start, row_top),
            &review_point(context.width - 1, row_end),
            side,
        ),
        "1234\n567"
    );
    let code_start = row_code_start(code_context, geometry, row_index, side);
    let continuation = review_point(code_start, row_top + 1);
    assert_eq!(
        expand_selection_point(&continuation, 2, code_context),
        Some(ExpandedCopySelectionRange {
            start_col: code_start,
            end_col: code_start + 2
        })
    );
    assert_eq!(
        render_copy_selection_text(
            code_context,
            &continuation,
            &review_point(code_start + 2, row_top + 1),
            side,
        ),
        "567"
    );
}

#[test]
fn split_continuation_rows_match_measured_copy_and_word_boundaries() {
    assert_wrapped_add_note_parity(LayoutMode::Split);
}

#[test]
fn stack_continuation_rows_match_measured_copy_and_word_boundaries() {
    assert_wrapped_add_note_parity(LayoutMode::Stack);
}

fn cjk_harness() -> (TestHarness, usize, i64) {
    let file = test_diff_file(
        "export const message = 'hello'; // greeting\n",
        "export const message = 'こんにちは'; // greeting\n",
        "i18n",
        "i18n.ts",
    );
    let harness = build_harness(vec![file], LayoutMode::Stack, 120, true, false, false, true);
    let (_, code_start, visual_row, line) = addition_line(&harness);
    assert!(line.contains("こんにちは"));
    (harness, code_start, visual_row)
}

const CJK_LINE: &str = "export const message = 'こんにちは'; // greeting";
const THROUGH_WIDE: &str = "export const message = 'こんにちは";

#[test]
fn code_only_selection_ending_after_the_wide_run_copies_selected_cells() {
    let (harness, code_start, visual_row) = cjk_harness();
    let mut context = harness.context();
    context.copy_decorations = false;
    assert_eq!(
        render_copy_selection_text(
            context,
            &review_point(code_start, visual_row),
            &review_point(
                code_start + measure_text_width(THROUGH_WIDE) - 1,
                visual_row
            ),
            None,
        ),
        THROUGH_WIDE
    );
}

#[test]
fn code_only_selection_starting_after_the_wide_run_copies_selected_cells() {
    let (harness, code_start, visual_row) = cjk_harness();
    let mut context = harness.context();
    context.copy_decorations = false;
    assert_eq!(
        render_copy_selection_text(
            context,
            &review_point(code_start + measure_text_width(THROUGH_WIDE), visual_row),
            &review_point(119, visual_row),
            None,
        ),
        "'; // greeting"
    );
}

#[test]
fn decorated_selection_ending_after_the_wide_run_copies_selected_cells() {
    let (harness, code_start, visual_row) = cjk_harness();
    assert_eq!(
        render_copy_selection_text(
            harness.context(),
            &review_point(code_start, visual_row),
            &review_point(
                code_start + measure_text_width(THROUGH_WIDE) - 1,
                visual_row
            ),
            None,
        ),
        THROUGH_WIDE
    );
}

#[test]
fn double_click_on_a_word_after_the_wide_run_uses_cell_columns() {
    let (harness, code_start, visual_row) = cjk_harness();
    let word_start = measure_text_width("export const message = 'こんにちは'; // ");
    assert_eq!(
        expand_selection_point(
            &review_point(code_start + word_start + 2, visual_row),
            2,
            harness.context(),
        ),
        Some(ExpandedCopySelectionRange {
            start_col: code_start + word_start,
            end_col: code_start + word_start + "greeting".len() - 1
        })
    );
}

#[test]
fn double_click_on_a_wide_character_selects_both_terminal_cells() {
    let (harness, code_start, visual_row) = cjk_harness();
    let wide_start = measure_text_width("export const message = 'こ");
    assert_eq!(
        expand_selection_point(
            &review_point(code_start + wide_start + 1, visual_row),
            2,
            harness.context(),
        ),
        Some(ExpandedCopySelectionRange {
            start_col: code_start + wide_start,
            end_col: code_start + wide_start + 1
        })
    );
}

#[test]
fn code_only_triple_click_covers_the_full_cell_width_of_the_line() {
    let (harness, code_start, visual_row) = cjk_harness();
    let mut context = harness.context();
    context.copy_decorations = false;
    assert_eq!(
        expand_selection_point(&review_point(code_start + 2, visual_row), 3, context),
        Some(ExpandedCopySelectionRange {
            start_col: code_start,
            end_col: code_start + measure_text_width(CJK_LINE) - 1
        })
    );
}

#[test]
fn pinned_header_selection_stays_cell_aligned_for_wide_filenames() {
    let mut file = test_diff_file(
        "export const a = 1;\n",
        "export const a = 2;\n",
        "cjk-path",
        "日本語.ts",
    );
    file.stats.additions = 1;
    file.stats.deletions = 1;
    let harness = build_harness(vec![file], LayoutMode::Stack, 120, true, false, false, true);
    let next = harness.layouts[0].body_top;
    let cells = |start, end| {
        render_copy_selection_text(
            harness.context(),
            &pinned_point(start, "cjk-path", next),
            &pinned_point(end, "cjk-path", next),
            None,
        )
    };
    assert_eq!(cells(1, 9), "日本語.ts");
    assert_eq!(cells(116, 117), "-1");
}

#[test]
fn selection_starting_at_a_zero_width_boundary_keeps_the_invisible_character() {
    let file = test_diff_file(
        "const x = 1;\n",
        "const x = 1;\nconst zw = 'a\u{200b}b';\n",
        "zero",
        "zero.ts",
    );
    let harness = build_harness(
        vec![file],
        LayoutMode::Stack,
        120,
        false,
        false,
        false,
        true,
    );
    let context = harness.context();
    let geometry = &harness.geometries[0];
    let row_index = geometry
        .planned_rows()
        .iter()
        .enumerate()
        .find_map(|(index, row)| {
            render_code_only_planned_row_text(
                row,
                row_text_options(context, None, geometry.line_number_digits),
            )
            .iter()
            .any(|line| line.contains("zw"))
            .then_some(index)
        })
        .expect("zero-width addition row");
    let code_start = row_code_start(context, geometry, row_index, None);
    let visual_row = row_visual_top(&harness, row_index);
    assert_eq!(
        render_copy_selection_text(
            context,
            &review_point(code_start + 13, visual_row),
            &review_point(119, visual_row),
            None,
        ),
        "\u{200b}b';"
    );
}
