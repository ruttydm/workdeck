//! Terminal-cell copy selection for the continuous review stream.
//!
//! This is a native Rust reimplementation of Hunk's
//! `src/ui/components/panes/copySelection.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. The selection model keeps
//! terminal coordinates independent from UTF-8 byte indexes, preserves wide
//! and zero-width grapheme clusters, and projects the canonical review rows
//! already owned by the Ratatui renderer.

use std::collections::HashMap;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;
use workdeck_core::{DiffFile, ReviewSide};
use workdeck_diff::resolve_split_pane_widths;
use workdeck_review::LayoutMode;

use crate::{
    CodeRowLayoutOptions, CodeRowLayoutPlan, CopySelectedRowRange, DiffSectionGeometry,
    DiffSectionRowBounds, FileSectionLayout, LineCursor, PlannedReviewRow, PlannedRowTextOptions,
    SplitProjectionSide, file_header_stats, fit_file_header_label, measure_text_width,
    plan_code_row_layout, render_code_only_planned_row_text, render_decorated_planned_row_text,
    review_file_id, slice_text_by_width,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopySelectionPoint {
    ReviewRow {
        column: usize,
        visual_row: i64,
    },
    PinnedHeader {
        column: usize,
        file_id: String,
        next_visual_row: i64,
    },
}

impl CopySelectionPoint {
    #[must_use]
    pub const fn column(&self) -> usize {
        match self {
            Self::ReviewRow { column, .. } | Self::PinnedHeader { column, .. } => *column,
        }
    }

    fn with_column(&self, column: usize) -> Self {
        match self {
            Self::ReviewRow { visual_row, .. } => Self::ReviewRow {
                column,
                visual_row: *visual_row,
            },
            Self::PinnedHeader {
                file_id,
                next_visual_row,
                ..
            } => Self::PinnedHeader {
                column,
                file_id: file_id.clone(),
                next_visual_row: *next_visual_row,
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopySelectionSide {
    Left,
    Right,
}

impl CopySelectionSide {
    const fn projection(self) -> SplitProjectionSide {
        match self {
            Self::Left => SplitProjectionSide::Left,
            Self::Right => SplitProjectionSide::Right,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopySelectionDrag {
    pub anchor: CopySelectionPoint,
    pub focus: CopySelectionPoint,
    pub moved: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct CopySelectionContext<'a> {
    pub code_horizontal_offset: usize,
    pub copy_decorations: bool,
    pub files: &'a [DiffFile],
    pub file_section_layouts: &'a [FileSectionLayout],
    pub header_label_width: usize,
    pub header_stats_width: usize,
    pub layout: LayoutMode,
    pub pinned_header_file: Option<&'a DiffFile>,
    pub reserve_add_note_column: bool,
    pub section_geometry: &'a [Arc<DiffSectionGeometry>],
    pub show_hunk_headers: bool,
    pub show_line_numbers: bool,
    pub width: usize,
    pub wrap_lines: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedCopySelectionRange {
    pub start: CopySelectionPoint,
    pub end: CopySelectionPoint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpandedCopySelectionRange {
    pub start_col: usize,
    pub end_col: usize,
}

/// Resolve which pane owns a terminal column in split layout.
#[must_use]
pub fn resolve_copy_selection_side(
    column: usize,
    layout: LayoutMode,
    width: usize,
) -> Option<CopySelectionSide> {
    if layout != LayoutMode::Split {
        return None;
    }
    let panes = resolve_split_pane_widths(width);
    Some(if column < panes.left_width {
        CopySelectionSide::Left
    } else {
        CopySelectionSide::Right
    })
}

/// Clamp one pointer column into the rendered body.
#[must_use]
pub fn clamp_copy_column(column: i64, width: usize) -> usize {
    usize::try_from(column.max(0))
        .unwrap_or(usize::MAX)
        .min(width.saturating_sub(1))
}

fn row_bounds_contains_visual_row(bounds: &DiffSectionRowBounds, visual_row: i64) -> bool {
    let Ok(visual_row) = usize::try_from(visual_row) else {
        return false;
    };
    bounds.bounds.height > 0
        && visual_row >= bounds.bounds.top
        && visual_row < bounds.bounds.top.saturating_add(bounds.bounds.height)
}

fn copy_selection_sort_row(point: &CopySelectionPoint) -> i64 {
    match point {
        CopySelectionPoint::ReviewRow { visual_row, .. } => *visual_row,
        CopySelectionPoint::PinnedHeader {
            next_visual_row, ..
        } => next_visual_row.saturating_sub(1),
    }
}

fn copy_selection_body_range(start: &CopySelectionPoint, end: &CopySelectionPoint) -> (i64, i64) {
    let start_row = match start {
        CopySelectionPoint::ReviewRow { visual_row, .. } => *visual_row,
        CopySelectionPoint::PinnedHeader {
            next_visual_row, ..
        } => *next_visual_row,
    };
    let end_row = match end {
        CopySelectionPoint::ReviewRow { visual_row, .. } => *visual_row,
        CopySelectionPoint::PinnedHeader {
            next_visual_row, ..
        } => next_visual_row.saturating_sub(1),
    };
    (start_row, end_row)
}

fn is_context_line_stable_key(stable_key: &str) -> bool {
    let mut parts = stable_key.split(':');
    parts.next() == Some("line")
        && parts
            .next()
            .is_some_and(|value| value.parse::<usize>().is_ok())
        && parts.next() == Some("context")
        && parts
            .next()
            .is_some_and(|value| value.parse::<u32>().is_ok())
        && parts
            .next()
            .is_some_and(|value| value.parse::<u32>().is_ok())
        && parts.next().is_none()
}

/// Resolve the keyboard line cursor addressed by an un-dragged row click.
#[must_use]
pub fn find_line_cursor_for_click<'a>(
    cursors: &'a [LineCursor],
    file_section_layouts: &[FileSectionLayout],
    point: &CopySelectionPoint,
    section_geometry: &[Arc<DiffSectionGeometry>],
    side: Option<CopySelectionSide>,
) -> Option<&'a LineCursor> {
    let CopySelectionPoint::ReviewRow { visual_row, .. } = point else {
        return None;
    };

    for section in file_section_layouts {
        if *visual_row < section.body_top || *visual_row >= section.section_bottom {
            continue;
        }
        let geometry = section_geometry.get(section.section_index)?;
        let body_row = visual_row.saturating_sub(section.body_top);
        let bounds = geometry
            .row_bounds
            .iter()
            .find(|bounds| row_bounds_contains_visual_row(bounds, body_row))?;
        let owns_key = |cursor: &&LineCursor| {
            cursor.file_id == section.file_id
                && (cursor.stable_key == bounds.stable_key
                    || bounds
                        .stable_keys
                        .iter()
                        .any(|stable_key| stable_key == &cursor.stable_key))
        };
        let mut row_cursors = cursors.iter().filter(owns_key);
        let first = row_cursors.next()?;
        let Some(side) = side else {
            return Some(first);
        };
        let target_side = match side {
            CopySelectionSide::Left => ReviewSide::Old,
            CopySelectionSide::Right => ReviewSide::New,
        };
        if first.target.side == target_side {
            return Some(first);
        }
        return row_cursors
            .find(|cursor| cursor.target.side == target_side)
            .or_else(|| is_context_line_stable_key(&bounds.stable_key).then_some(first));
    }
    None
}

const COPY_SELECTION_CLICK_SLOP_CELLS: usize = 1;

/// Treat one-cell pointer jitter as a click while preserving deliberate and expanded drags.
#[must_use]
pub fn copy_selection_drag_is_click(drag: &CopySelectionDrag) -> bool {
    if drag.expanded {
        return false;
    }
    if !drag.moved {
        return true;
    }
    if std::mem::discriminant(&drag.anchor) != std::mem::discriminant(&drag.focus)
        || drag.anchor.column().abs_diff(drag.focus.column()) > COPY_SELECTION_CLICK_SLOP_CELLS
    {
        return false;
    }
    match (&drag.anchor, &drag.focus) {
        (
            CopySelectionPoint::PinnedHeader {
                file_id: left_file,
                next_visual_row: left_row,
                ..
            },
            CopySelectionPoint::PinnedHeader {
                file_id: right_file,
                next_visual_row: right_row,
                ..
            },
        ) => {
            left_file == right_file
                && left_row.abs_diff(*right_row) <= COPY_SELECTION_CLICK_SLOP_CELLS as u64
        }
        (
            CopySelectionPoint::ReviewRow {
                visual_row: left_row,
                ..
            },
            CopySelectionPoint::ReviewRow {
                visual_row: right_row,
                ..
            },
        ) => left_row.abs_diff(*right_row) <= COPY_SELECTION_CLICK_SLOP_CELLS as u64,
        _ => false,
    }
}

/// Return whether two points identify the same selectable terminal cell.
#[must_use]
pub fn copy_selection_points_equal(left: &CopySelectionPoint, right: &CopySelectionPoint) -> bool {
    left == right
}

/// Return whether two points occupy the same selectable terminal row.
#[must_use]
pub fn copy_selection_points_share_row(
    left: &CopySelectionPoint,
    right: &CopySelectionPoint,
) -> bool {
    match (left, right) {
        (
            CopySelectionPoint::ReviewRow {
                visual_row: left, ..
            },
            CopySelectionPoint::ReviewRow {
                visual_row: right, ..
            },
        ) => left == right,
        (
            CopySelectionPoint::PinnedHeader {
                file_id: left_file,
                next_visual_row: left_row,
                ..
            },
            CopySelectionPoint::PinnedHeader {
                file_id: right_file,
                next_visual_row: right_row,
                ..
            },
        ) => left_file == right_file && left_row == right_row,
        _ => false,
    }
}

/// Order two selection points by visible row and then terminal column.
#[must_use]
pub fn normalize_copy_selection_range(
    anchor: &CopySelectionPoint,
    focus: &CopySelectionPoint,
) -> NormalizedCopySelectionRange {
    let anchor_row = copy_selection_sort_row(anchor);
    let focus_row = copy_selection_sort_row(focus);
    if anchor_row < focus_row || (anchor_row == focus_row && anchor.column() <= focus.column()) {
        NormalizedCopySelectionRange {
            start: anchor.clone(),
            end: focus.clone(),
        }
    } else {
        NormalizedCopySelectionRange {
            start: focus.clone(),
            end: anchor.clone(),
        }
    }
}

fn trim_copied_line(line: &str) -> String {
    line.trim_end_matches([' ', '\t']).to_owned()
}

/// Convert an inclusive cell range into UTF-8 byte slice bounds without splitting graphemes.
fn cell_range_to_byte_range(text: &str, start_cell: usize, end_cell: usize) -> (usize, usize) {
    if text.is_ascii() {
        let start = text.len().min(start_cell);
        let end = text.len().min(end_cell.saturating_add(1).max(start));
        return (start, end);
    }

    let mut cell_cursor = 0;
    let mut start_index = None;
    let mut end_index = text.len();
    for (byte_index, cluster) in text.grapheme_indices(true) {
        if cell_cursor > end_cell {
            end_index = byte_index;
            break;
        }
        let cluster_width = measure_text_width(cluster);
        let covers_start = if cluster_width > 0 {
            cell_cursor.saturating_add(cluster_width) > start_cell
        } else {
            cell_cursor >= start_cell
        };
        if start_index.is_none() && covers_start {
            start_index = Some(byte_index);
        }
        cell_cursor = cell_cursor.saturating_add(cluster_width);
    }
    let start_index = start_index.unwrap_or(text.len());
    (start_index, end_index.max(start_index))
}

fn slice_line_by_cells(line: &str, start_cell: usize, end_cell: usize) -> &str {
    let (start, end) = cell_range_to_byte_range(line, start_cell, end_cell);
    &line[start..end]
}

#[derive(Debug)]
struct CopyVisualLine {
    visual_row: i64,
    text: String,
    global_column_offset: usize,
    retain_empty: bool,
}

fn clip_selected_visual_line(
    line: &CopyVisualLine,
    start: &CopySelectionPoint,
    end: &CopySelectionPoint,
) -> Option<String> {
    let start_row = copy_selection_sort_row(start);
    let end_row = copy_selection_sort_row(end);
    if line.visual_row < start_row || line.visual_row > end_row {
        return None;
    }
    let start_column = if line.visual_row == start_row {
        start.column().saturating_sub(line.global_column_offset)
    } else {
        0
    };
    let end_column = if line.visual_row == end_row {
        end.column().saturating_sub(line.global_column_offset)
    } else {
        usize::MAX
    };
    let copied = trim_copied_line(slice_line_by_cells(&line.text, start_column, end_column));
    (!copied.is_empty() || line.retain_empty).then_some(copied)
}

fn row_text_options(
    context: CopySelectionContext<'_>,
    side: Option<CopySelectionSide>,
    line_number_digits: usize,
) -> PlannedRowTextOptions {
    PlannedRowTextOptions {
        width: context.width,
        line_number_digits,
        show_line_numbers: context.show_line_numbers,
        show_hunk_headers: context.show_hunk_headers,
        wrap_lines: context.wrap_lines,
        code_horizontal_offset: context.code_horizontal_offset,
        reserve_add_note_column: context.reserve_add_note_column,
        show_add_note_badge: false,
        side: side.map(CopySelectionSide::projection),
    }
}

fn resolve_copy_visual_line_offset(
    context: CopySelectionContext<'_>,
    copy_side: Option<CopySelectionSide>,
    line_number_digits: usize,
    row: &PlannedReviewRow,
) -> usize {
    let split_panes =
        (context.layout == LayoutMode::Split).then(|| resolve_split_pane_widths(context.width));
    if context.copy_decorations {
        return if copy_side == Some(CopySelectionSide::Right) {
            split_panes.map_or(0, |panes| panes.left_width)
        } else {
            0
        };
    }
    let layout = plan_code_row_layout(
        row,
        CodeRowLayoutOptions {
            width: context.width,
            line_number_digits,
            show_line_numbers: context.show_line_numbers,
            wrap_lines: context.wrap_lines,
            reserve_add_note_column: context.reserve_add_note_column,
            show_add_note_badge: false,
        },
    );
    match layout {
        Some(CodeRowLayoutPlan::Stack { cell, .. }) => cell.prefix_width + cell.gutter_width,
        Some(CodeRowLayoutPlan::Split {
            left,
            right,
            left_pane_width,
            ..
        }) => match copy_side {
            Some(CopySelectionSide::Left) => left.prefix_width + left.gutter_width,
            Some(CopySelectionSide::Right) => {
                left_pane_width + right.prefix_width + right.gutter_width
            }
            None => 0,
        },
        None => 0,
    }
}

fn render_file_header_copy_text(
    file: &DiffFile,
    header_label_width: usize,
    header_stats_width: usize,
    width: usize,
) -> String {
    let stats = file_header_stats(file).text;
    let stats_text = format!("{stats:>header_stats_width$}");
    let label = fit_file_header_label(file, header_label_width);
    let label = format!(
        "{}{}",
        label.filename,
        label.state_label.unwrap_or_default()
    );
    let gap = width
        .saturating_sub(
            2_usize
                .saturating_add(measure_text_width(&label))
                .saturating_add(stats_text.len()),
        )
        .max(1);
    let line = format!(" {label}{}{stats_text} ", " ".repeat(gap));
    let clamped = slice_text_by_width(&line, 0, width);
    format!(
        "{}{}",
        clamped.text,
        " ".repeat(width.saturating_sub(clamped.width))
    )
}

/// Resolve a selectable point against the scrolling review body.
#[must_use]
pub fn find_copy_selection_point(
    column: i64,
    copy_decorations: bool,
    file_section_layouts: &[FileSectionLayout],
    section_geometry: &[Arc<DiffSectionGeometry>],
    visual_row: i64,
    width: usize,
) -> Option<CopySelectionPoint> {
    for section in file_section_layouts {
        if copy_decorations
            && section.header_top < section.body_top
            && visual_row >= section.header_top
            && visual_row < section.body_top
        {
            return Some(CopySelectionPoint::ReviewRow {
                column: clamp_copy_column(column, width),
                visual_row,
            });
        }
        if visual_row < section.body_top
            || visual_row >= section.body_top.saturating_add(section.body_height)
        {
            continue;
        }
        let geometry = section_geometry.get(section.section_index)?;
        let body_row = visual_row.saturating_sub(section.body_top);
        geometry
            .row_bounds
            .iter()
            .find(|bounds| row_bounds_contains_visual_row(bounds, body_row))?;
        return Some(CopySelectionPoint::ReviewRow {
            column: clamp_copy_column(column, width),
            visual_row,
        });
    }
    None
}

/// Render normalized selected review cells into clipboard text.
#[must_use]
pub fn render_copy_selection_text(
    context: CopySelectionContext<'_>,
    start: &CopySelectionPoint,
    end: &CopySelectionPoint,
    side: Option<CopySelectionSide>,
) -> String {
    let mut lines = Vec::new();
    let copy_side = side.or_else(|| {
        (context.layout == LayoutMode::Split
            && matches!(start, CopySelectionPoint::ReviewRow { .. }))
        .then(|| resolve_copy_selection_side(start.column(), context.layout, context.width))
        .flatten()
    });

    if context.copy_decorations
        && let (
            Some(file),
            CopySelectionPoint::PinnedHeader {
                file_id,
                next_visual_row,
                ..
            },
        ) = (context.pinned_header_file, start)
        && file_id == review_file_id(file)
    {
        let line = CopyVisualLine {
            visual_row: next_visual_row.saturating_sub(1),
            text: render_file_header_copy_text(
                file,
                context.header_label_width,
                context.header_stats_width,
                context.width,
            ),
            global_column_offset: 0,
            retain_empty: true,
        };
        let pinned_end = match end {
            CopySelectionPoint::PinnedHeader {
                file_id: end_file, ..
            } if end_file == file_id => end.clone(),
            _ => end.with_column(usize::MAX),
        };
        if let Some(copied) = clip_selected_visual_line(&line, start, &pinned_end) {
            lines.push(copied);
        }
    }

    let (start_row, end_row) = copy_selection_body_range(start, end);
    for section in context.file_section_layouts {
        if section.section_bottom <= start_row || section.header_top > end_row {
            continue;
        }
        if context.copy_decorations
            && section.header_top < section.body_top
            && section.header_top >= start_row
            && section.header_top <= end_row
            && let Some(file) = context.files.get(section.section_index)
        {
            let line = CopyVisualLine {
                visual_row: section.header_top,
                text: render_file_header_copy_text(
                    file,
                    context.header_label_width,
                    context.header_stats_width,
                    context.width,
                ),
                global_column_offset: 0,
                retain_empty: true,
            };
            if let Some(copied) = clip_selected_visual_line(&line, start, end) {
                lines.push(copied);
            }
        }
        let Some(geometry) = context.section_geometry.get(section.section_index) else {
            continue;
        };
        for (row_index, bounds) in geometry.row_bounds.iter().enumerate() {
            let Some(row) = geometry.planned_rows().get(row_index) else {
                continue;
            };
            if bounds.bounds.height == 0 {
                continue;
            }
            let row_top = section
                .body_top
                .saturating_add(i64::try_from(bounds.bounds.top).unwrap_or(i64::MAX));
            let row_bottom =
                row_top.saturating_add(i64::try_from(bounds.bounds.height).unwrap_or(i64::MAX));
            if row_bottom <= start_row || row_top > end_row {
                continue;
            }
            let options = row_text_options(context, copy_side, geometry.line_number_digits);
            let rendered = if context.copy_decorations {
                render_decorated_planned_row_text(row, options)
            } else {
                render_code_only_planned_row_text(row, options)
            };
            let offset = resolve_copy_visual_line_offset(
                context,
                copy_side,
                geometry.line_number_digits,
                row,
            );
            for (line_index, text) in rendered.into_iter().enumerate() {
                let line = CopyVisualLine {
                    visual_row: row_top
                        .saturating_add(i64::try_from(line_index).unwrap_or(i64::MAX)),
                    text,
                    global_column_offset: offset,
                    retain_empty: context.copy_decorations,
                };
                if let Some(copied) = clip_selected_visual_line(&line, start, end) {
                    lines.push(copied);
                }
            }
        }
    }
    lines.join("\n").trim_end_matches('\n').to_owned()
}

fn is_copy_word_char(character: Option<char>) -> bool {
    character.is_some_and(|character| {
        character.is_ascii_alphanumeric() || matches!(character, '_' | '$')
    })
}

/// Expand a point to word or rendered-line boundaries for double/triple click.
#[must_use]
pub fn expand_selection_point(
    point: &CopySelectionPoint,
    click_count: u8,
    context: CopySelectionContext<'_>,
) -> Option<ExpandedCopySelectionRange> {
    let CopySelectionPoint::ReviewRow { column, visual_row } = point else {
        return None;
    };
    for section in context.file_section_layouts {
        if *visual_row < section.body_top
            || *visual_row >= section.body_top.saturating_add(section.body_height)
        {
            continue;
        }
        let geometry = context.section_geometry.get(section.section_index)?;
        let body_row = usize::try_from(visual_row.saturating_sub(section.body_top)).ok()?;
        let row_index = geometry.row_bounds.iter().position(|bounds| {
            row_bounds_contains_visual_row(bounds, i64::try_from(body_row).unwrap_or(i64::MAX))
        })?;
        let row = geometry.planned_rows().get(row_index)?;

        if click_count == 3 && context.copy_decorations {
            if context.layout == LayoutMode::Split {
                let panes = resolve_split_pane_widths(context.width);
                return Some(
                    if resolve_copy_selection_side(*column, context.layout, context.width)
                        == Some(CopySelectionSide::Right)
                    {
                        ExpandedCopySelectionRange {
                            start_col: panes.left_width,
                            end_col: context.width.saturating_sub(1),
                        }
                    } else {
                        ExpandedCopySelectionRange {
                            start_col: 0,
                            end_col: panes.left_width.saturating_sub(1),
                        }
                    },
                );
            }
            return Some(ExpandedCopySelectionRange {
                start_col: 0,
                end_col: context.width.saturating_sub(1),
            });
        }

        let side = resolve_copy_selection_side(*column, context.layout, context.width);
        let text_options = row_text_options(context, side, geometry.line_number_digits);
        let layout = plan_code_row_layout(
            row,
            CodeRowLayoutOptions {
                width: context.width,
                line_number_digits: geometry.line_number_digits,
                show_line_numbers: context.show_line_numbers,
                wrap_lines: context.wrap_lines,
                reserve_add_note_column: context.reserve_add_note_column,
                show_add_note_badge: false,
            },
        );
        let global_content_start = match layout {
            Some(CodeRowLayoutPlan::Split {
                left,
                right,
                left_pane_width,
                ..
            }) => match side {
                Some(CopySelectionSide::Left) => left.prefix_width + left.gutter_width,
                Some(CopySelectionSide::Right) | None => {
                    left_pane_width + right.prefix_width + right.gutter_width
                }
            },
            Some(CodeRowLayoutPlan::Stack { cell, .. }) => cell.prefix_width + cell.gutter_width,
            None => 0,
        };
        let line_index = body_row.saturating_sub(geometry.row_bounds[row_index].bounds.top);
        let code_text = render_code_only_planned_row_text(row, text_options);
        let line_text = code_text.get(line_index)?;
        let line_width = measure_text_width(line_text);
        if line_text.is_empty() || line_width == 0 {
            return None;
        }
        if click_count == 3 {
            return Some(ExpandedCopySelectionRange {
                start_col: global_content_start,
                end_col: global_content_start.saturating_add(line_width.saturating_sub(1)),
            });
        }
        if click_count != 2 {
            return None;
        }
        let local_cell = column
            .saturating_sub(global_content_start)
            .min(line_width.saturating_sub(1));
        let (cluster_start, cluster_end) =
            cell_range_to_byte_range(line_text, local_cell, local_cell);
        let cluster_character = line_text[cluster_start..cluster_end].chars().next();
        if !is_copy_word_char(cluster_character) {
            let cluster_start_cell = measure_text_width(&line_text[..cluster_start]);
            let cluster_width = measure_text_width(&line_text[cluster_start..cluster_end]).max(1);
            return Some(ExpandedCopySelectionRange {
                start_col: global_content_start.saturating_add(cluster_start_cell),
                end_col: global_content_start
                    .saturating_add(cluster_start_cell)
                    .saturating_add(cluster_width.saturating_sub(1)),
            });
        }

        let mut word_start = cluster_start;
        while word_start > 0 && is_copy_word_char(line_text[..word_start].chars().next_back()) {
            word_start = line_text[..word_start]
                .char_indices()
                .next_back()
                .map_or(0, |(index, _)| index);
        }
        let mut word_end = cluster_start;
        for (offset, character) in line_text[cluster_start..].char_indices() {
            if !is_copy_word_char(Some(character)) {
                break;
            }
            word_end = cluster_start + offset + character.len_utf8();
        }
        return Some(ExpandedCopySelectionRange {
            start_col: global_content_start + measure_text_width(&line_text[..word_start]),
            end_col: global_content_start + measure_text_width(&line_text[..word_end]) - 1,
        });
    }
    None
}

fn selected_range_for_row_bounds(
    row_top: i64,
    row_height: usize,
    start_row: i64,
    end_row: i64,
    start_column: usize,
    end_column: usize,
    width: usize,
) -> Option<CopySelectedRowRange> {
    let row_bottom = row_top.saturating_add(i64::try_from(row_height).unwrap_or(i64::MAX));
    if row_height == 0 || row_bottom <= start_row || row_top > end_row {
        return None;
    }
    Some(CopySelectedRowRange {
        start_col: if row_top <= start_row {
            start_column
        } else {
            0
        },
        end_col: if row_bottom > end_row {
            end_column
        } else {
            width.saturating_sub(1)
        },
    })
}

/// Build file-local row-key ranges for the visible selection highlight.
#[must_use]
pub fn build_copy_selected_row_keys(
    drag: Option<&CopySelectionDrag>,
    file_section_layouts: &[FileSectionLayout],
    section_geometry: &[Arc<DiffSectionGeometry>],
    width: usize,
) -> HashMap<String, HashMap<String, CopySelectedRowRange>> {
    let mut selected = HashMap::new();
    let Some(drag) = drag.filter(|drag| drag.moved) else {
        return selected;
    };
    let normalized = normalize_copy_selection_range(&drag.anchor, &drag.focus);
    let (start_row, end_row) = copy_selection_body_range(&normalized.start, &normalized.end);
    for section in file_section_layouts {
        if section.body_top.saturating_add(section.body_height) <= start_row
            || section.body_top > end_row
        {
            continue;
        }
        let Some(geometry) = section_geometry.get(section.section_index) else {
            continue;
        };
        for bounds in &geometry.row_bounds {
            let row_top = section
                .body_top
                .saturating_add(i64::try_from(bounds.bounds.top).unwrap_or(i64::MAX));
            let Some(range) = selected_range_for_row_bounds(
                row_top,
                bounds.bounds.height,
                start_row,
                end_row,
                normalized.start.column(),
                normalized.end.column(),
                width,
            ) else {
                continue;
            };
            selected
                .entry(section.file_id.clone())
                .or_insert_with(HashMap::new)
                .insert(bounds.key.clone(), range);
        }
    }
    selected
}

#[cfg(test)]
mod tests;
