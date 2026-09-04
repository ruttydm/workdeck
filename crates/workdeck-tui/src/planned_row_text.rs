//! Plain-text projection for canonical planned review rows.
//!
//! This is a Rust reimplementation of Hunk's `src/ui/diff/plannedRowText.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.

use workdeck_core::{
    DiffFile, ReviewEmptyDiffReason, ReviewSide, review_empty_diff_reason, review_file_change_kind,
};
use workdeck_diff::{
    DiffRow, RenderSpan, SplitLineCell, SplitLineKind, StackLineCell, TerminalSpan, TextSegment,
    sanitize_terminal_line, sanitize_terminal_spans, wrap_segments,
};

use crate::{
    CodeCellLayoutPlan, CodeRowLayoutOptions, CodeRowLayoutPlan, PlannedReviewRow,
    diff_rail_marker, inline_note_title, measure_sanitized_text_width, pad_text,
    plan_code_row_layout, slice_sanitized_text_by_width, slice_text_by_width, split_gutter_text,
    stack_gutter_text, wrap_text,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitProjectionSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlannedRowTextOptions {
    pub width: usize,
    pub line_number_digits: usize,
    pub show_line_numbers: bool,
    pub show_hunk_headers: bool,
    pub wrap_lines: bool,
    pub code_horizontal_offset: usize,
    pub reserve_add_note_column: bool,
    pub show_add_note_badge: bool,
    pub side: Option<SplitProjectionSide>,
}

impl PlannedRowTextOptions {
    fn layout(self) -> CodeRowLayoutOptions {
        CodeRowLayoutOptions {
            width: self.width,
            line_number_digits: self.line_number_digits,
            show_line_numbers: self.show_line_numbers,
            wrap_lines: self.wrap_lines,
            reserve_add_note_column: self.reserve_add_note_column,
            show_add_note_badge: self.show_add_note_badge,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlainCellLine {
    pub content_width: usize,
    pub gutter_width: usize,
    pub spans_text: String,
}

/// Clamp a sanitized label to one terminal row with Hunk's ellipsis marker.
#[must_use]
pub fn fit_planned_row_text(text: &str, width: usize) -> String {
    let safe_text = sanitize_terminal_line(text);
    if width == 0 {
        return String::new();
    }
    if measure_sanitized_text_width(&safe_text) <= width {
        return safe_text;
    }
    if width == 1 {
        return "…".into();
    }
    format!(
        "{}…",
        slice_sanitized_text_by_width(&safe_text, 0, width - 1).text
    )
}

fn sanitized_spans_text(spans: &[RenderSpan]) -> String {
    let terminal_spans = spans
        .iter()
        .map(|span| TerminalSpan {
            text: span.text.clone(),
            style: (),
        })
        .collect::<Vec<_>>();
    sanitize_terminal_spans(&terminal_spans)
        .iter()
        .map(|span| span.text.as_str())
        .collect()
}

fn spans_to_plain_text(spans: &[RenderSpan], width: usize, horizontal_offset: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let text = sanitized_spans_text(spans);
    let visible = slice_text_by_width(&text, horizontal_offset, width);
    pad_text(&visible.text, width)
}

fn cell_code_text(spans: &[RenderSpan], horizontal_offset: usize) -> String {
    slice_text_by_width(&sanitized_spans_text(spans), horizontal_offset, usize::MAX).text
}

fn wrapped_span_lines(spans: &[RenderSpan], width: usize) -> Vec<Vec<RenderSpan>> {
    wrap_segments(
        spans
            .iter()
            .map(|span| TextSegment {
                text: span.text.clone(),
                style: span.clone(),
            })
            .collect(),
        width,
    )
    .into_iter()
    .map(|line| {
        line.into_iter()
            .map(|segment| RenderSpan {
                text: segment.text,
                ..segment.style
            })
            .collect()
    })
    .collect()
}

fn pad_gutter(mut gutter: String, width: usize) -> String {
    let gutter_width = measure_sanitized_text_width(&gutter);
    if gutter_width < width {
        gutter.push_str(&" ".repeat(width - gutter_width));
    }
    gutter
}

#[must_use]
pub fn build_plain_split_cell_lines(
    cell: &SplitLineCell,
    geometry: &CodeCellLayoutPlan<'_>,
    line_number_digits: usize,
    show_line_numbers: bool,
    wrap_lines: bool,
    code_horizontal_offset: usize,
) -> Vec<PlainCellLine> {
    let gutter = split_gutter_text(
        cell.sign.chars().next().unwrap_or(' '),
        cell.line_number.and_then(|line| u32::try_from(line).ok()),
        line_number_digits,
        show_line_numbers,
    );
    let gutter = pad_gutter(gutter, geometry.gutter_width);
    if !wrap_lines {
        return vec![PlainCellLine {
            content_width: geometry.content_width,
            gutter_width: geometry.gutter_width,
            spans_text: format!(
                "{gutter}{}",
                spans_to_plain_text(&cell.spans, geometry.content_width, code_horizontal_offset,)
            ),
        }];
    }

    wrapped_span_lines(&cell.spans, geometry.content_width)
        .into_iter()
        .enumerate()
        .map(|(index, spans)| PlainCellLine {
            content_width: geometry.content_width,
            gutter_width: geometry.gutter_width,
            spans_text: format!(
                "{}{}",
                if index == 0 {
                    gutter.clone()
                } else {
                    " ".repeat(geometry.gutter_width)
                },
                spans_to_plain_text(&spans, geometry.content_width, 0),
            ),
        })
        .collect()
}

#[must_use]
pub fn build_plain_stack_cell_lines(
    cell: &StackLineCell,
    geometry: &CodeCellLayoutPlan<'_>,
    line_number_digits: usize,
    show_line_numbers: bool,
    wrap_lines: bool,
    code_horizontal_offset: usize,
) -> Vec<PlainCellLine> {
    let gutter = stack_gutter_text(
        cell.sign.chars().next().unwrap_or(' '),
        cell.old_line_number
            .and_then(|line| u32::try_from(line).ok()),
        cell.new_line_number
            .and_then(|line| u32::try_from(line).ok()),
        line_number_digits,
        show_line_numbers,
    );
    let gutter = pad_gutter(gutter, geometry.gutter_width);
    if !wrap_lines {
        return vec![PlainCellLine {
            content_width: geometry.content_width,
            gutter_width: geometry.gutter_width,
            spans_text: format!(
                "{gutter}{}",
                spans_to_plain_text(&cell.spans, geometry.content_width, code_horizontal_offset,)
            ),
        }];
    }

    wrapped_span_lines(&cell.spans, geometry.content_width)
        .into_iter()
        .enumerate()
        .map(|(index, spans)| PlainCellLine {
            content_width: geometry.content_width,
            gutter_width: geometry.gutter_width,
            spans_text: format!(
                "{}{}",
                if index == 0 {
                    gutter.clone()
                } else {
                    " ".repeat(geometry.gutter_width)
                },
                spans_to_plain_text(&spans, geometry.content_width, 0),
            ),
        })
        .collect()
}

#[must_use]
pub fn render_header_row_text(text: &str, width: usize) -> String {
    let label_width = width.saturating_sub(1);
    format!(
        "{}{}",
        diff_rail_marker(),
        pad_text(&fit_planned_row_text(text, label_width), label_width)
    )
}

fn empty_cell_line(cell: &CodeCellLayoutPlan<'_>) -> PlainCellLine {
    PlainCellLine {
        gutter_width: cell.gutter_width,
        content_width: cell.content_width,
        spans_text: " ".repeat(cell.width.saturating_sub(cell.prefix_width)),
    }
}

#[must_use]
pub fn render_decorated_planned_row_text(
    row: &PlannedReviewRow,
    options: PlannedRowTextOptions,
) -> Vec<String> {
    if options.width == 0 {
        return Vec::new();
    }

    match row {
        PlannedReviewRow::InlineNote {
            annotation,
            note_index,
            note_count,
            ..
        } => {
            let title = inline_note_title(annotation, *note_index, *note_count);
            let mut lines = vec![fit_planned_row_text(&title, options.width)];
            lines.extend(
                wrap_text(&annotation.summary, options.width)
                    .into_iter()
                    .map(|line| fit_planned_row_text(&line, options.width)),
            );
            if let Some(rationale) = annotation.rationale.as_deref() {
                lines.extend(
                    wrap_text(rationale, options.width)
                        .into_iter()
                        .map(|line| fit_planned_row_text(&line, options.width)),
                );
            }
            return lines;
        }
        PlannedReviewRow::HunkGap { height, .. } => {
            return vec![String::new(); *height];
        }
        PlannedReviewRow::DiffRow { .. } => {}
    }

    let Some(prepared_row) = row.diff_row() else {
        return Vec::new();
    };
    match prepared_row {
        DiffRow::HunkHeader { text, .. } => {
            if options.show_hunk_headers {
                vec![render_header_row_text(text, options.width)]
            } else {
                Vec::new()
            }
        }
        DiffRow::Collapsed { text, .. } => vec![render_header_row_text(
            &format!("··· {text} ···"),
            options.width,
        )],
        DiffRow::SplitLine { left, right, .. } => {
            let Some(CodeRowLayoutPlan::Split {
                left: left_layout,
                right: right_layout,
                note_guide_side,
                ..
            }) = plan_code_row_layout(row, options.layout())
            else {
                return Vec::new();
            };
            let guide_on_old_side = note_guide_side == Some(ReviewSide::Old);
            let guide_on_new_side = note_guide_side == Some(ReviewSide::New);
            let left_prefix = if guide_on_old_side {
                "│"
            } else {
                diff_rail_marker()
            };
            let right_prefix = "▌";
            let left_lines = build_plain_split_cell_lines(
                left,
                &left_layout,
                options.line_number_digits,
                options.show_line_numbers,
                options.wrap_lines,
                options.code_horizontal_offset,
            );
            let right_lines = build_plain_split_cell_lines(
                right,
                &right_layout,
                options.line_number_digits,
                options.show_line_numbers,
                options.wrap_lines,
                options.code_horizontal_offset,
            );
            let visual_line_count = left_lines.len().max(right_lines.len());
            (0..visual_line_count)
                .map(|index| {
                    let left_line = left_lines
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| empty_cell_line(&left_layout));
                    let right_line = right_lines
                        .get(index)
                        .cloned()
                        .unwrap_or_else(|| empty_cell_line(&right_layout));
                    let normalized_left = pad_text(
                        &format!("{left_prefix}{}", left_line.spans_text),
                        left_layout.width,
                    );
                    let normalized_right = pad_text(
                        &format!("{right_prefix}{}", right_line.spans_text),
                        right_layout.width,
                    );
                    match options.side {
                        Some(SplitProjectionSide::Left) => normalized_left,
                        Some(SplitProjectionSide::Right) => format!(
                            "{normalized_right}{}",
                            if guide_on_new_side { "│" } else { "" }
                        ),
                        None => format!(
                            "{normalized_left}{normalized_right}{}",
                            if guide_on_new_side { "│" } else { "" }
                        ),
                    }
                })
                .collect()
        }
        DiffRow::StackLine { cell, .. } => {
            let Some(CodeRowLayoutPlan::Stack {
                cell: cell_layout,
                note_guide_side,
                ..
            }) = plan_code_row_layout(row, options.layout())
            else {
                return Vec::new();
            };
            let prefix = if note_guide_side == Some(ReviewSide::Old) {
                "│"
            } else {
                diff_rail_marker()
            };
            let guide = if note_guide_side == Some(ReviewSide::New) {
                "│"
            } else {
                ""
            };
            build_plain_stack_cell_lines(
                cell,
                &cell_layout,
                options.line_number_digits,
                options.show_line_numbers,
                options.wrap_lines,
                options.code_horizontal_offset,
            )
            .into_iter()
            .map(|line| {
                let normalized = pad_text(
                    &format!("{prefix}{}", line.spans_text),
                    cell_layout.width.max(1),
                );
                format!("{normalized}{guide}")
            })
            .collect()
        }
    }
}

fn non_empty(text: String) -> Option<String> {
    (!text.is_empty()).then_some(text)
}

fn split_cell_code_text(cell: &SplitLineCell, horizontal_offset: usize) -> Option<String> {
    (cell.kind != SplitLineKind::Empty)
        .then(|| cell_code_text(&cell.spans, horizontal_offset))
        .and_then(non_empty)
}

#[must_use]
pub fn render_code_only_planned_row_text(
    row: &PlannedReviewRow,
    options: PlannedRowTextOptions,
) -> Vec<String> {
    if options.width == 0 {
        return Vec::new();
    }
    let PlannedReviewRow::DiffRow { row: prepared, .. } = row else {
        return Vec::new();
    };
    match prepared {
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => Vec::new(),
        DiffRow::StackLine { cell, .. } if !options.wrap_lines => {
            non_empty(cell_code_text(&cell.spans, options.code_horizontal_offset))
                .into_iter()
                .collect()
        }
        DiffRow::StackLine { cell, .. } => {
            let Some(CodeRowLayoutPlan::Stack {
                cell: cell_layout, ..
            }) = plan_code_row_layout(row, options.layout())
            else {
                return Vec::new();
            };
            wrapped_span_lines(&cell.spans, cell_layout.content_width)
                .into_iter()
                .filter_map(|spans| non_empty(sanitized_spans_text(&spans)))
                .collect()
        }
        DiffRow::SplitLine { left, right, .. } if !options.wrap_lines => {
            let left_text = split_cell_code_text(left, options.code_horizontal_offset);
            let right_text = split_cell_code_text(right, options.code_horizontal_offset);
            match options.side {
                Some(SplitProjectionSide::Left) => left_text.into_iter().collect(),
                Some(SplitProjectionSide::Right) => right_text.into_iter().collect(),
                None if left_text.is_some() && left_text == right_text => {
                    left_text.into_iter().collect()
                }
                None => left_text.into_iter().chain(right_text).collect(),
            }
        }
        DiffRow::SplitLine { left, right, .. } => {
            let Some(CodeRowLayoutPlan::Split {
                left: left_layout,
                right: right_layout,
                ..
            }) = plan_code_row_layout(row, options.layout())
            else {
                return Vec::new();
            };
            let left_lines = wrapped_span_lines(&left.spans, left_layout.content_width);
            let right_lines = wrapped_span_lines(&right.spans, right_layout.content_width);
            let mut lines = Vec::new();
            for index in 0..left_lines.len().max(right_lines.len()) {
                let left_text = (left.kind != SplitLineKind::Empty)
                    .then(|| {
                        left_lines
                            .get(index)
                            .map_or_else(String::new, |spans| sanitized_spans_text(spans))
                    })
                    .and_then(non_empty);
                let right_text = (right.kind != SplitLineKind::Empty)
                    .then(|| {
                        right_lines
                            .get(index)
                            .map_or_else(String::new, |spans| sanitized_spans_text(spans))
                    })
                    .and_then(non_empty);
                match options.side {
                    Some(SplitProjectionSide::Left) => lines.extend(left_text),
                    Some(SplitProjectionSide::Right) => lines.extend(right_text),
                    None if left_text.is_some() && left_text == right_text => {
                        lines.extend(left_text);
                    }
                    None => {
                        lines.extend(left_text);
                        lines.extend(right_text);
                    }
                }
            }
            lines
        }
    }
}

#[must_use]
pub const fn diff_message_for_reason(reason: ReviewEmptyDiffReason) -> &'static str {
    match reason {
        ReviewEmptyDiffReason::RenameOnly => "No textual hunks. This change only renames the file.",
        ReviewEmptyDiffReason::Binary => "Binary file skipped",
        ReviewEmptyDiffReason::TooLarge => "File too large to render automatically.",
        ReviewEmptyDiffReason::NewFile => "No textual hunks. The file is marked as new.",
        ReviewEmptyDiffReason::DeletedFile => "No textual hunks. The file is marked as deleted.",
        ReviewEmptyDiffReason::NoHunks => "No textual hunks to render for this file.",
    }
}

#[must_use]
pub fn diff_message(file: &DiffFile) -> &'static str {
    diff_message_for_reason(review_empty_diff_reason(
        review_file_change_kind(file),
        file.flags.binary,
        file.flags.too_large,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use workdeck_core::{
        AgentAnnotation, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats,
    };
    use workdeck_diff::{
        CollapsedGapPosition, SplitLineKind, StackLineKind, resolve_split_pane_widths,
    };
    use workdeck_review::ResolvedReviewNoteAnchor;

    use crate::{VisibleAgentNote, measure_text_width};

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn split_row(left: SplitLineCell, right: SplitLineCell) -> PlannedReviewRow {
        crate::legacy_planned_diff_row(
            DiffRow::SplitLine {
                key: "file:split:1".into(),
                file_id: "file".into(),
                hunk_index: 0,
                left,
                right,
                is_expansion_row: false,
                expanded_gap_key: None,
            },
            None,
            None,
        )
    }

    fn split_cell(kind: SplitLineKind, sign: &str, line: usize, text: &str) -> SplitLineCell {
        SplitLineCell {
            kind,
            sign: sign.into(),
            line_number: Some(line),
            move_kind: None,
            spans: vec![span(text)],
        }
    }

    fn stack_row(text: &str) -> PlannedReviewRow {
        crate::legacy_planned_diff_row(
            DiffRow::StackLine {
                key: "file:stack:1".into(),
                file_id: "file".into(),
                hunk_index: 0,
                cell: StackLineCell {
                    kind: StackLineKind::Addition,
                    sign: "+".into(),
                    old_line_number: None,
                    new_line_number: Some(1),
                    move_kind: None,
                    spans: vec![span(text)],
                },
                is_expansion_row: false,
                expanded_gap_key: None,
            },
            None,
            None,
        )
    }

    fn options(width: usize) -> PlannedRowTextOptions {
        PlannedRowTextOptions {
            width,
            line_number_digits: 1,
            show_line_numbers: true,
            show_hunk_headers: true,
            wrap_lines: false,
            code_horizontal_offset: 0,
            reserve_add_note_column: false,
            show_add_note_badge: false,
            side: None,
        }
    }

    fn annotation(summary: &str) -> AgentAnnotation {
        AgentAnnotation {
            id: Some("note-1".into()),
            old_range: None,
            new_range: None,
            summary: summary.into(),
            rationale: None,
            markup: None,
            tags: Vec::new(),
            confidence: None,
            source: Some("agent".into()),
            title: None,
            author: Some("Codex".into()),
            created_at: None,
            updated_at: None,
            editable: false,
        }
    }

    fn inline_note(annotation: AgentAnnotation) -> PlannedReviewRow {
        let note = VisibleAgentNote {
            id: "note-1".into(),
            annotation: annotation.clone(),
            anchor: ResolvedReviewNoteAnchor {
                old_range: None,
                new_range: None,
                preferred: None,
                intersecting_hunk_indices: vec![0],
                owner_hunk_index: Some(0),
            },
            source: None,
            editable: false,
            thread: None,
            actions: None,
            draft: None,
        };
        PlannedReviewRow::InlineNote {
            key: "note:key".into(),
            stable_key: "note:key".into(),
            file_id: "file".into(),
            hunk_index: 0,
            annotation_id: "note-1".into(),
            annotation,
            note: Box::new(note),
            anchor_side: Some(ReviewSide::New),
            note_count: 1,
            note_index: 0,
        }
    }

    #[test]
    fn split_rows_are_copyable_and_keep_wide_separator_aligned() {
        let row = split_row(
            split_cell(SplitLineKind::Deletion, "-", 1, "export const answer = 41;"),
            split_cell(SplitLineKind::Addition, "+", 1, "export const answer = 42;"),
        );
        let rendered = render_decorated_planned_row_text(&row, options(80));
        assert_eq!(rendered.len(), 1);
        assert!(rendered[0].contains("- export const answer = 41;"));
        assert!(rendered[0].contains("+ export const answer = 42;"));

        let wide = split_row(
            split_cell(SplitLineKind::Deletion, "-", 1, "const x = '日本語';"),
            split_cell(SplitLineKind::Addition, "+", 1, "const x = 'abc';"),
        );
        let line = &render_decorated_planned_row_text(&wide, options(80))[0];
        let separator = line[diff_rail_marker().len()..]
            .find(diff_rail_marker())
            .expect("center separator")
            + diff_rail_marker().len();
        assert!(line.contains("日本語"));
        assert_eq!(
            measure_text_width(&line[..separator]),
            resolve_split_pane_widths(80).left_width
        );
    }

    #[test]
    fn stack_rows_apply_horizontal_copy_offset() {
        let row = stack_row("export const answer = 42;");
        let mut options = options(40);
        options.code_horizontal_offset = 7;
        let line = &render_decorated_planned_row_text(&row, options)[0];
        assert!(line.contains("nst answer = 42;"));
        assert!(!line.contains("export const"));
    }

    #[test]
    fn code_only_projection_omits_decorations_deduplicates_context_and_filters_sides() {
        let header = crate::legacy_planned_diff_row(
            DiffRow::HunkHeader {
                key: "header".into(),
                file_id: "file".into(),
                hunk_index: 0,
                text: "@@ -1 +1 @@".into(),
            },
            None,
            None,
        );
        assert!(render_code_only_planned_row_text(&header, options(80)).is_empty());

        let changed = split_row(
            split_cell(SplitLineKind::Deletion, "-", 1, "export const answer = 41;"),
            split_cell(SplitLineKind::Addition, "+", 1, "export const answer = 42;"),
        );
        assert_eq!(
            render_code_only_planned_row_text(&changed, options(80)),
            ["export const answer = 41;", "export const answer = 42;"]
        );
        let context = split_row(
            split_cell(SplitLineKind::Context, " ", 2, "same"),
            split_cell(SplitLineKind::Context, " ", 2, "same"),
        );
        assert_eq!(
            render_code_only_planned_row_text(&context, options(80)),
            ["same"]
        );
        let mut right = options(80);
        right.side = Some(SplitProjectionSide::Right);
        assert_eq!(
            render_code_only_planned_row_text(&changed, right),
            ["export const answer = 42;"]
        );
    }

    #[test]
    fn wrapping_side_projection_and_guides_use_the_planned_geometry() {
        let mut row = split_row(
            split_cell(SplitLineKind::Deletion, "-", 1, "abcdefghij"),
            split_cell(SplitLineKind::Addition, "+", 1, "klmnopqrst"),
        );
        if let PlannedReviewRow::DiffRow {
            note_guide_side, ..
        } = &mut row
        {
            *note_guide_side = Some(ReviewSide::New);
        }
        let mut wrapped = options(14);
        wrapped.show_line_numbers = false;
        wrapped.wrap_lines = true;
        let all = render_decorated_planned_row_text(&row, wrapped);
        assert!(all.len() > 1);
        assert!(all.iter().all(|line| line.ends_with('│')));
        assert!(all.iter().all(|line| measure_text_width(line) == 14));

        wrapped.side = Some(SplitProjectionSide::Left);
        let left = render_decorated_planned_row_text(&row, wrapped);
        assert!(left.iter().all(|line| !line.ends_with('│')));
        assert_eq!(
            render_code_only_planned_row_text(&row, wrapped),
            ["abcd", "efgh", "ij"]
        );
    }

    #[test]
    fn headers_notes_gaps_and_hostile_text_follow_plain_projection_rules() {
        assert_eq!(fit_planned_row_text("abcdef", 1), "…");
        assert_eq!(fit_planned_row_text("abcdef", 4), "abc…");
        assert_eq!(fit_planned_row_text("ok\x1b[2J", 4), "ok");
        assert_eq!(render_header_row_text("header", 5), "▌hea…");

        let collapsed = crate::legacy_planned_diff_row(
            DiffRow::Collapsed {
                key: "gap".into(),
                file_id: "file".into(),
                hunk_index: 0,
                text: "5 unchanged lines".into(),
                position: CollapsedGapPosition::Before,
                old_range: [1, 5],
                new_range: [1, 5],
            },
            None,
            None,
        );
        assert!(
            render_decorated_planned_row_text(&collapsed, options(30))[0]
                .contains("··· 5 unchanged lines ···")
        );

        let mut note = annotation("summary words wrap");
        note.rationale = Some("because\x1b[2J safe".into());
        let note_lines = render_decorated_planned_row_text(&inline_note(note), options(12));
        assert_eq!(note_lines[0], "Codex note");
        assert!(note_lines.iter().all(|line| !line.contains('\x1b')));

        let gap = PlannedReviewRow::HunkGap {
            key: "gap".into(),
            stable_key: "gap".into(),
            file_id: "file".into(),
            hunk_index: 0,
            height: 3,
        };
        assert_eq!(
            render_decorated_planned_row_text(&gap, options(10)),
            ["", "", ""]
        );
        let mut zero = options(0);
        zero.wrap_lines = true;
        assert!(render_decorated_planned_row_text(&gap, zero).is_empty());
    }

    fn file(change_kind: FileChangeKind, flags: FileFlags, stats: FileStats) -> DiffFile {
        DiffFile {
            key: "file".into(),
            runtime_id: "file".into(),
            path: "file.rs".into(),
            previous_path: None,
            change_kind,
            language: Some("rust".into()),
            stats,
            flags,
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: Vec::new(),
            content_identity: "content".into(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        }
    }

    #[test]
    fn all_empty_diff_reasons_keep_hunks_wording_and_precedence() {
        assert_eq!(
            diff_message(&file(
                FileChangeKind::Renamed,
                FileFlags::default(),
                FileStats::default(),
            )),
            "No textual hunks. This change only renames the file."
        );
        assert_eq!(
            diff_message(&file(
                FileChangeKind::Modified,
                FileFlags {
                    binary: true,
                    ..FileFlags::default()
                },
                FileStats::default(),
            )),
            "Binary file skipped"
        );
        assert_eq!(
            diff_message(&file(
                FileChangeKind::Modified,
                FileFlags {
                    too_large: true,
                    ..FileFlags::default()
                },
                FileStats::default(),
            )),
            "File too large to render automatically."
        );
        assert_eq!(
            diff_message(&file(
                FileChangeKind::Added,
                FileFlags::default(),
                FileStats::default(),
            )),
            "No textual hunks. The file is marked as new."
        );
        assert_eq!(
            diff_message(&file(
                FileChangeKind::Deleted,
                FileFlags::default(),
                FileStats::default(),
            )),
            "No textual hunks. The file is marked as deleted."
        );
        assert_eq!(
            diff_message(&file(
                FileChangeKind::Modified,
                FileFlags::default(),
                FileStats::default(),
            )),
            "No textual hunks to render for this file."
        );
        assert_eq!(
            diff_message(&file(
                FileChangeKind::Renamed,
                FileFlags::default(),
                FileStats {
                    additions: 1,
                    deletions: 0,
                    truncated: false,
                },
            )),
            "No textual hunks to render for this file."
        );
    }

    #[test]
    fn frozen_hunk_projection_vectors_match_native() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/code-row-layout-and-planned-text.json"
        ))
        .expect("valid frozen Hunk projection oracle");
        assert_eq!(
            oracle["baseline"],
            "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2"
        );
        let vectors = &oracle["projectionVectors"];
        assert_eq!(
            serde_json::json!([
                fit_planned_row_text("abcdef", 0),
                fit_planned_row_text("abcdef", 1),
                fit_planned_row_text("abcdef", 4),
                fit_planned_row_text("日本語", 4),
            ]),
            vectors["fit"]
        );

        let mut split = split_row(
            split_cell(SplitLineKind::Deletion, "-", 1, "const x = 日本語;"),
            split_cell(SplitLineKind::Addition, "+", 1, "const x = abc;"),
        );
        if let PlannedReviewRow::DiffRow {
            note_guide_side, ..
        } = &mut split
        {
            *note_guide_side = Some(ReviewSide::New);
        }
        let split_options = options(32);
        assert_eq!(
            serde_json::json!(render_decorated_planned_row_text(&split, split_options)),
            vectors["split"]
        );
        let mut right_options = split_options;
        right_options.side = Some(SplitProjectionSide::Right);
        assert_eq!(
            serde_json::json!(render_decorated_planned_row_text(&split, right_options)),
            vectors["splitRight"]
        );

        let context = split_row(
            split_cell(SplitLineKind::Context, " ", 2, "same line"),
            split_cell(SplitLineKind::Context, " ", 2, "same line"),
        );
        assert_eq!(
            serde_json::json!(render_code_only_planned_row_text(&context, options(32))),
            vectors["contextCode"]
        );

        let mut stack = stack_row("export const answer = 42;");
        if let PlannedReviewRow::DiffRow {
            row: DiffRow::StackLine { cell, .. },
            note_guide_side,
            ..
        } = &mut stack
        {
            cell.new_line_number = Some(4);
            *note_guide_side = Some(ReviewSide::Old);
        }
        let mut stack_options = options(26);
        stack_options.code_horizontal_offset = 7;
        assert_eq!(
            serde_json::json!(render_decorated_planned_row_text(&stack, stack_options)),
            vectors["stackOffset"]
        );

        let mut note = annotation("summary words wrap");
        note.rationale = Some("because safe".into());
        let mut note_row = inline_note(note);
        if let PlannedReviewRow::InlineNote { note_count, .. } = &mut note_row {
            *note_count = 2;
        }
        assert_eq!(
            serde_json::json!(render_decorated_planned_row_text(&note_row, options(12))),
            vectors["note"]
        );

        let message_files = [
            file(
                FileChangeKind::Renamed,
                FileFlags::default(),
                FileStats::default(),
            ),
            file(
                FileChangeKind::Modified,
                FileFlags {
                    binary: true,
                    ..FileFlags::default()
                },
                FileStats::default(),
            ),
            file(
                FileChangeKind::Modified,
                FileFlags {
                    too_large: true,
                    ..FileFlags::default()
                },
                FileStats::default(),
            ),
            file(
                FileChangeKind::Added,
                FileFlags::default(),
                FileStats::default(),
            ),
            file(
                FileChangeKind::Deleted,
                FileFlags::default(),
                FileStats::default(),
            ),
            file(
                FileChangeKind::Modified,
                FileFlags::default(),
                FileStats::default(),
            ),
        ];
        assert_eq!(
            serde_json::json!(message_files.iter().map(diff_message).collect::<Vec<_>>()),
            vectors["messages"]
        );
    }
}
