//! Deterministic paint composition for split and stack diff code cells.
//!
//! This is a native Ratatui reimplementation of Hunk's
//! `src/ui/diff/CodeCellView.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Canonical row and geometry
//! plans own text measurement; this layer only assigns final foreground and
//! background colors without changing terminal-cell geometry.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use workdeck_core::ReviewSide;
use workdeck_diff::{
    DiffRow, RenderForegroundTransform, RenderSpan, SplitLineCell, SplitLineKind, StackLineCell,
    StackLineKind, TextSegment, sanitize_terminal_line, slice_segments_window, wrap_segments,
};
use workdeck_extension_api::HighlightTone;

use crate::{
    AppTheme, CodeCellLayoutPlan, CodeRowLayoutPlan, CopySelectedRowRange, DEFAULT_DIM_RATIO,
    LineHighlightPaintIndex, LineHighlightSpanStyle, LineHighlightToneStyle, RowCellKind,
    apply_prepared_line_highlights_to_spans, cursor_line_highlight_background, diff_rail_marker,
    dim_span_foreground, line_highlight_tone_style, measure_text_width, ratatui_theme_color,
    selection_highlight_background, split_cell_palette, split_gutter_text, stack_cell_palette,
    stack_gutter_text,
};

/// Selects the complete content window while preserving wrapped paint semantics.
pub const FULL_CODE_CELL_COL_RANGE: CopySelectedRowRange = CopySelectedRowRange {
    start_col: 0,
    end_col: usize::MAX,
};

/// The two background transforms a code cell can receive from review state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeCellHighlightKind {
    Selection,
    Cursor,
}

/// One row highlight passed from review selection policy into cell painting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeCellHighlight {
    pub kind: CodeCellHighlightKind,
    /// Inclusive global review-stream columns. `None` paints only prefix and gutter.
    pub col_range: Option<CopySelectedRowRange>,
}

impl CodeCellHighlight {
    fn background(self, base: &str, theme: &AppTheme) -> String {
        match self.kind {
            CodeCellHighlightKind::Selection => selection_highlight_background(base, theme),
            CodeCellHighlightKind::Cursor => cursor_line_highlight_background(base, theme),
        }
    }
}

/// Fixed rail/separator prefix supplied by the owning code-row view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeCellPrefix {
    pub text: String,
    pub foreground: String,
    pub background: String,
}

/// One final renderer-neutral terminal run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaintedCodeCellRun {
    pub text: String,
    pub foreground: Option<String>,
    pub background: Option<String>,
}

/// One final visual line, ready to become a Ratatui [`Line`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaintedCodeCellLine {
    pub runs: Vec<PaintedCodeCellRun>,
}

impl PaintedCodeCellLine {
    #[must_use]
    pub fn text(&self) -> String {
        self.runs.iter().map(|run| run.text.as_str()).collect()
    }

    #[must_use]
    pub fn width(&self) -> usize {
        measure_text_width(&self.text())
    }

    #[must_use]
    pub fn ratatui_line(&self) -> Line<'static> {
        Line::from(
            self.runs
                .iter()
                .map(|run| {
                    let mut style = Style::default();
                    if let Some(foreground) = run.foreground.as_deref() {
                        style = style.fg(ratatui_theme_color(foreground));
                    }
                    if let Some(background) = run.background.as_deref() {
                        style = style.bg(ratatui_theme_color(background));
                    }
                    Span::styled(run.text.clone(), style)
                })
                .collect::<Vec<_>>(),
        )
    }
}

/// Inputs shared by nowrap and wrapped cell painters.
#[derive(Debug, Clone, Copy)]
pub struct CodeCellPaintOptions<'a> {
    pub line_number_digits: usize,
    pub show_line_numbers: bool,
    pub theme: &'a AppTheme,
    pub horizontal_offset: usize,
    pub guide_on_new_side: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SourcePaintStyle {
    foreground: Option<String>,
    background: Option<String>,
    transform_foreground: Option<RenderForegroundTransform>,
}

#[derive(Debug, Clone)]
struct WrappedCellLayout {
    gutter_width: usize,
    content_width: usize,
    palette: OwnedCellPalette,
    lines: Vec<Vec<TextSegment<SourcePaintStyle>>>,
}

#[derive(Debug, Clone)]
struct OwnedCellPalette {
    gutter_background: String,
    content_background: String,
    number_color: String,
}

fn append_run(
    target: &mut PaintedCodeCellLine,
    text: impl Into<String>,
    foreground: Option<String>,
    background: Option<String>,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    if let Some(previous) = target
        .runs
        .last_mut()
        .filter(|previous| previous.foreground == foreground && previous.background == background)
    {
        previous.text.push_str(&text);
    } else {
        target.runs.push(PaintedCodeCellRun {
            text,
            foreground,
            background,
        });
    }
}

fn source_segments(spans: &[RenderSpan], sanitize: bool) -> Vec<TextSegment<SourcePaintStyle>> {
    spans
        .iter()
        .filter_map(|span| {
            let text = if sanitize {
                sanitize_terminal_line(&span.text)
            } else {
                span.text.clone()
            };
            (!text.is_empty()).then(|| TextSegment {
                text,
                style: SourcePaintStyle {
                    foreground: span.foreground.clone(),
                    background: span.background.clone(),
                    transform_foreground: span.transform_foreground.clone(),
                },
            })
        })
        .collect()
}

fn rendered_foreground(
    style: &SourcePaintStyle,
    fallback: &str,
    rendered_background: &str,
) -> String {
    style.transform_foreground.as_ref().map_or_else(
        || {
            style
                .foreground
                .clone()
                .unwrap_or_else(|| fallback.to_owned())
        },
        |transform| {
            transform.apply(
                Some(style.foreground.as_deref().unwrap_or(fallback)),
                rendered_background,
            )
        },
    )
}

fn append_piece(
    target: &mut PaintedCodeCellLine,
    segment: &TextSegment<SourcePaintStyle>,
    fallback_foreground: &str,
    fallback_background: &str,
    highlight: Option<CodeCellHighlight>,
    theme: &AppTheme,
) {
    let base_background = segment
        .style
        .background
        .as_deref()
        .unwrap_or(fallback_background);
    let rendered_background = highlight.map_or_else(
        || base_background.to_owned(),
        |highlight| highlight.background(base_background, theme),
    );
    append_run(
        target,
        segment.text.clone(),
        Some(rendered_foreground(
            &segment.style,
            fallback_foreground,
            &rendered_background,
        )),
        Some(rendered_background),
    );
}

fn split_segment(
    segment: &TextSegment<SourcePaintStyle>,
    offset: usize,
    width: usize,
) -> Option<TextSegment<SourcePaintStyle>> {
    slice_segments_window(std::slice::from_ref(segment), offset, width)
        .segments
        .into_iter()
        .next()
}

#[allow(clippy::too_many_arguments)]
fn append_content(
    target: &mut PaintedCodeCellLine,
    segments: &[TextSegment<SourcePaintStyle>],
    width: usize,
    horizontal_offset: usize,
    fallback_foreground: &str,
    fallback_background: &str,
    highlight: Option<CodeCellHighlight>,
    selection: Option<(usize, usize)>,
    theme: &AppTheme,
) {
    let window = slice_segments_window(segments, horizontal_offset, width);
    let full_highlight =
        highlight.filter(|_| selection.is_some_and(|(start, end)| start == 0 && end >= width));
    let partial = highlight
        .zip(selection)
        .filter(|_| full_highlight.is_none());
    let mut column = 0_usize;

    for segment in &window.segments {
        let segment_width = measure_text_width(&segment.text);
        if let Some((highlight, (selection_start, selection_end))) = partial {
            let segment_start = column;
            let segment_end = column.saturating_add(segment_width);
            let overlap_start = selection_start.max(segment_start);
            let overlap_end = selection_end.min(segment_end);
            if overlap_start < overlap_end {
                let before = overlap_start - segment_start;
                if let Some(piece) = split_segment(segment, 0, before) {
                    append_piece(
                        target,
                        &piece,
                        fallback_foreground,
                        fallback_background,
                        None,
                        theme,
                    );
                }
                if let Some(piece) = split_segment(segment, before, overlap_end - overlap_start) {
                    append_piece(
                        target,
                        &piece,
                        fallback_foreground,
                        fallback_background,
                        Some(highlight),
                        theme,
                    );
                }
                if let Some(piece) = split_segment(
                    segment,
                    overlap_end - segment_start,
                    segment_end - overlap_end,
                ) {
                    append_piece(
                        target,
                        &piece,
                        fallback_foreground,
                        fallback_background,
                        None,
                        theme,
                    );
                }
            } else {
                append_piece(
                    target,
                    segment,
                    fallback_foreground,
                    fallback_background,
                    None,
                    theme,
                );
            }
        } else {
            append_piece(
                target,
                segment,
                fallback_foreground,
                fallback_background,
                full_highlight,
                theme,
            );
        }
        column = column.saturating_add(segment_width);
    }

    let padding = width.saturating_sub(window.used_width);
    if padding == 0 {
        return;
    }
    let padding_style = SourcePaintStyle {
        foreground: Some(fallback_foreground.to_owned()),
        background: Some(fallback_background.to_owned()),
        transform_foreground: None,
    };
    let padding_segment = TextSegment {
        text: " ".repeat(padding),
        style: padding_style,
    };
    if let Some((highlight, (selection_start, selection_end))) = partial {
        let padding_start = window.used_width;
        let padding_end = padding_start.saturating_add(padding);
        let overlap_start = selection_start.max(padding_start);
        let overlap_end = selection_end.min(padding_end);
        if overlap_start < overlap_end {
            let before = overlap_start - padding_start;
            if let Some(piece) = split_segment(&padding_segment, 0, before) {
                append_piece(
                    target,
                    &piece,
                    fallback_foreground,
                    fallback_background,
                    None,
                    theme,
                );
            }
            if let Some(piece) =
                split_segment(&padding_segment, before, overlap_end - overlap_start)
            {
                append_piece(
                    target,
                    &piece,
                    fallback_foreground,
                    fallback_background,
                    Some(highlight),
                    theme,
                );
            }
            if let Some(piece) = split_segment(
                &padding_segment,
                overlap_end - padding_start,
                padding_end - overlap_end,
            ) {
                append_piece(
                    target,
                    &piece,
                    fallback_foreground,
                    fallback_background,
                    None,
                    theme,
                );
            }
            return;
        }
    }
    append_piece(
        target,
        &padding_segment,
        fallback_foreground,
        fallback_background,
        full_highlight,
        theme,
    );
}

fn local_content_selection(
    highlight: Option<CodeCellHighlight>,
    global_content_start: usize,
    content_width: usize,
) -> Option<(usize, usize)> {
    let range = highlight?.col_range?;
    (global_content_start < range.end_col).then(|| {
        (
            range.start_col.saturating_sub(global_content_start),
            content_width.min(
                range
                    .end_col
                    .saturating_sub(global_content_start)
                    .saturating_add(1),
            ),
        )
    })
}

fn append_prefix_and_gutter(
    target: &mut PaintedCodeCellLine,
    prefix: &CodeCellPrefix,
    gutter_text: String,
    palette: &OwnedCellPalette,
    highlight: Option<CodeCellHighlight>,
    theme: &AppTheme,
) {
    let prefix_background = highlight.map_or_else(
        || prefix.background.clone(),
        |highlight| highlight.background(&prefix.background, theme),
    );
    let gutter_background = highlight.map_or_else(
        || palette.gutter_background.clone(),
        |highlight| highlight.background(&palette.gutter_background, theme),
    );
    append_run(
        target,
        prefix.text.clone(),
        Some(prefix.foreground.clone()),
        Some(prefix_background),
    );
    append_run(
        target,
        gutter_text,
        Some(palette.number_color.clone()),
        Some(gutter_background),
    );
}

fn owned_palette(palette: crate::RowCellPalette<'_>) -> OwnedCellPalette {
    OwnedCellPalette {
        gutter_background: palette.gutter_background.to_owned(),
        content_background: palette.content_background.to_owned(),
        number_color: palette.number_color.to_owned(),
    }
}

fn split_kind(kind: SplitLineKind) -> RowCellKind {
    match kind {
        SplitLineKind::Context => RowCellKind::Context,
        SplitLineKind::Addition => RowCellKind::Addition,
        SplitLineKind::Deletion => RowCellKind::Deletion,
        SplitLineKind::Empty => RowCellKind::Empty,
    }
}

fn stack_kind(kind: StackLineKind) -> RowCellKind {
    match kind {
        StackLineKind::Context => RowCellKind::Context,
        StackLineKind::Addition => RowCellKind::Addition,
        StackLineKind::Deletion => RowCellKind::Deletion,
    }
}

fn split_palette(cell: &SplitLineCell, theme: &AppTheme) -> OwnedCellPalette {
    owned_palette(split_cell_palette(
        split_kind(cell.kind),
        theme,
        cell.move_kind.is_some(),
    ))
}

fn stack_palette(cell: &StackLineCell, theme: &AppTheme) -> OwnedCellPalette {
    owned_palette(stack_cell_palette(
        stack_kind(cell.kind),
        theme,
        cell.move_kind.is_some(),
    ))
}

fn split_gutter(cell: &SplitLineCell, width: usize, digits: usize, show_numbers: bool) -> String {
    let sign = cell.sign.chars().next().unwrap_or(' ');
    let line = cell.line_number.and_then(|line| u32::try_from(line).ok());
    format!(
        "{:<width$}",
        split_gutter_text(sign, line, digits, show_numbers)
    )
}

fn stack_gutter(cell: &StackLineCell, width: usize, digits: usize, show_numbers: bool) -> String {
    let sign = cell.sign.chars().next().unwrap_or(' ');
    let old_line = cell
        .old_line_number
        .and_then(|line| u32::try_from(line).ok());
    let new_line = cell
        .new_line_number
        .and_then(|line| u32::try_from(line).ok());
    format!(
        "{:<width$}",
        stack_gutter_text(sign, old_line, new_line, digits, show_numbers)
    )
}

fn append_note_guide(target: &mut PaintedCodeCellLine, enabled: bool, theme: &AppTheme) {
    if enabled {
        append_run(target, "│", Some(theme.note_border.clone()), None);
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_split_cell(
    target: &mut PaintedCodeCellLine,
    cell: &SplitLineCell,
    geometry: &CodeCellLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
    prefix: &CodeCellPrefix,
    highlight: Option<CodeCellHighlight>,
    pane_offset: usize,
    segments: &[TextSegment<SourcePaintStyle>],
    gutter_text: String,
) {
    let palette = split_palette(cell, options.theme);
    append_prefix_and_gutter(
        target,
        prefix,
        gutter_text,
        &palette,
        highlight,
        options.theme,
    );
    let content_start = pane_offset
        .saturating_add(geometry.prefix_width)
        .saturating_add(geometry.gutter_width);
    append_content(
        target,
        segments,
        geometry.content_width,
        options.horizontal_offset,
        &options.theme.syntax_colors.default,
        &palette.content_background,
        highlight,
        local_content_selection(highlight, content_start, geometry.content_width),
        options.theme,
    );
}

#[allow(clippy::too_many_arguments)]
fn paint_stack_cell(
    target: &mut PaintedCodeCellLine,
    cell: &StackLineCell,
    geometry: &CodeCellLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
    prefix: &CodeCellPrefix,
    highlight: Option<CodeCellHighlight>,
    segments: &[TextSegment<SourcePaintStyle>],
    gutter_text: String,
) {
    let palette = stack_palette(cell, options.theme);
    append_prefix_and_gutter(
        target,
        prefix,
        gutter_text,
        &palette,
        highlight,
        options.theme,
    );
    let content_start = geometry.prefix_width.saturating_add(geometry.gutter_width);
    append_content(
        target,
        segments,
        geometry.content_width,
        options.horizontal_offset,
        &options.theme.syntax_colors.default,
        &palette.content_background,
        highlight,
        local_content_selection(highlight, content_start, geometry.content_width),
        options.theme,
    );
}

/// Paint one non-wrapped split row through the canonical cell geometry.
#[must_use]
pub fn paint_nowrap_split_code_cells(
    row: &DiffRow,
    layout: &CodeRowLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
    left_prefix: &CodeCellPrefix,
    right_prefix: &CodeCellPrefix,
    left_highlight: Option<CodeCellHighlight>,
    right_highlight: Option<CodeCellHighlight>,
) -> Option<PaintedCodeCellLine> {
    let DiffRow::SplitLine { left, right, .. } = row else {
        return None;
    };
    let CodeRowLayoutPlan::Split {
        left: left_geometry,
        right: right_geometry,
        left_pane_width,
        ..
    } = layout
    else {
        return None;
    };
    let mut line = PaintedCodeCellLine::default();
    paint_split_cell(
        &mut line,
        left,
        left_geometry,
        options,
        left_prefix,
        left_highlight,
        0,
        &source_segments(&left.spans, true),
        split_gutter(
            left,
            left_geometry.gutter_width,
            options.line_number_digits,
            options.show_line_numbers,
        ),
    );
    paint_split_cell(
        &mut line,
        right,
        right_geometry,
        options,
        right_prefix,
        right_highlight,
        *left_pane_width,
        &source_segments(&right.spans, true),
        split_gutter(
            right,
            right_geometry.gutter_width,
            options.line_number_digits,
            options.show_line_numbers,
        ),
    );
    append_note_guide(&mut line, options.guide_on_new_side, options.theme);
    Some(line)
}

/// Paint one non-wrapped stack row through the canonical cell geometry.
#[must_use]
pub fn paint_nowrap_stack_code_cell(
    row: &DiffRow,
    layout: &CodeRowLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
    prefix: &CodeCellPrefix,
    highlight: Option<CodeCellHighlight>,
) -> Option<PaintedCodeCellLine> {
    let DiffRow::StackLine { cell, .. } = row else {
        return None;
    };
    let CodeRowLayoutPlan::Stack { cell: geometry, .. } = layout else {
        return None;
    };
    let mut line = PaintedCodeCellLine::default();
    paint_stack_cell(
        &mut line,
        cell,
        geometry,
        options,
        prefix,
        highlight,
        &source_segments(&cell.spans, true),
        stack_gutter(
            cell,
            geometry.gutter_width,
            options.line_number_digits,
            options.show_line_numbers,
        ),
    );
    append_note_guide(&mut line, options.guide_on_new_side, options.theme);
    Some(line)
}

fn wrapped_split_cell(
    cell: &SplitLineCell,
    geometry: &CodeCellLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
) -> WrappedCellLayout {
    WrappedCellLayout {
        gutter_width: geometry.gutter_width,
        content_width: geometry.content_width,
        palette: split_palette(cell, options.theme),
        lines: wrap_segments(source_segments(&cell.spans, false), geometry.content_width),
    }
}

fn wrapped_stack_cell(
    cell: &StackLineCell,
    geometry: &CodeCellLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
) -> WrappedCellLayout {
    WrappedCellLayout {
        gutter_width: geometry.gutter_width,
        content_width: geometry.content_width,
        palette: stack_palette(cell, options.theme),
        lines: wrap_segments(source_segments(&cell.spans, false), geometry.content_width),
    }
}

#[allow(clippy::too_many_arguments)]
fn append_wrapped_cell(
    target: &mut PaintedCodeCellLine,
    line_segments: &[TextSegment<SourcePaintStyle>],
    gutter_text: String,
    layout: &WrappedCellLayout,
    options: CodeCellPaintOptions<'_>,
    prefix: &CodeCellPrefix,
    highlight: Option<CodeCellHighlight>,
    pane_offset: usize,
) {
    append_prefix_and_gutter(
        target,
        prefix,
        gutter_text,
        &layout.palette,
        highlight,
        options.theme,
    );
    let content_start = pane_offset
        .saturating_add(measure_text_width(&prefix.text))
        .saturating_add(layout.gutter_width);
    append_content(
        target,
        line_segments,
        layout.content_width,
        0,
        &options.theme.syntax_colors.default,
        &layout.palette.content_background,
        highlight,
        local_content_selection(highlight, content_start, layout.content_width),
        options.theme,
    );
}

/// Build every wrapped visual line for one split row.
#[must_use]
#[allow(clippy::too_many_arguments)]
pub fn paint_wrapped_split_code_cells(
    row: &DiffRow,
    layout: &CodeRowLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
    left_prefix: &CodeCellPrefix,
    right_prefix: &CodeCellPrefix,
    left_highlight: Option<CodeCellHighlight>,
    right_highlight: Option<CodeCellHighlight>,
    trailing_width: usize,
) -> Option<Vec<PaintedCodeCellLine>> {
    let DiffRow::SplitLine { left, right, .. } = row else {
        return None;
    };
    let CodeRowLayoutPlan::Split {
        left: left_geometry,
        right: right_geometry,
        left_pane_width,
        ..
    } = layout
    else {
        return None;
    };
    let left_layout = wrapped_split_cell(left, left_geometry, options);
    let right_layout = wrapped_split_cell(right, right_geometry, options);
    let count = left_layout.lines.len().max(right_layout.lines.len());
    Some(
        (0..count)
            .map(|index| {
                let empty_left = Vec::new();
                let empty_right = Vec::new();
                let mut line = PaintedCodeCellLine::default();
                append_wrapped_cell(
                    &mut line,
                    left_layout.lines.get(index).unwrap_or(&empty_left),
                    if index == 0 {
                        split_gutter(
                            left,
                            left_layout.gutter_width,
                            options.line_number_digits,
                            options.show_line_numbers,
                        )
                    } else {
                        " ".repeat(left_layout.gutter_width)
                    },
                    &left_layout,
                    options,
                    left_prefix,
                    left_highlight,
                    0,
                );
                append_wrapped_cell(
                    &mut line,
                    right_layout.lines.get(index).unwrap_or(&empty_right),
                    if index == 0 {
                        split_gutter(
                            right,
                            right_layout.gutter_width,
                            options.line_number_digits,
                            options.show_line_numbers,
                        )
                    } else {
                        " ".repeat(right_layout.gutter_width)
                    },
                    &right_layout,
                    options,
                    right_prefix,
                    right_highlight,
                    *left_pane_width,
                );
                append_note_guide(&mut line, options.guide_on_new_side, options.theme);
                append_run(
                    &mut line,
                    " ".repeat(trailing_width),
                    None,
                    Some(right_layout.palette.content_background.clone()),
                );
                line
            })
            .collect(),
    )
}

/// Build every wrapped visual line for one stack row.
#[must_use]
pub fn paint_wrapped_stack_code_cell(
    row: &DiffRow,
    layout: &CodeRowLayoutPlan<'_>,
    options: CodeCellPaintOptions<'_>,
    prefix: &CodeCellPrefix,
    highlight: Option<CodeCellHighlight>,
    trailing_width: usize,
) -> Option<Vec<PaintedCodeCellLine>> {
    let DiffRow::StackLine { cell, .. } = row else {
        return None;
    };
    let CodeRowLayoutPlan::Stack { cell: geometry, .. } = layout else {
        return None;
    };
    let wrapped = wrapped_stack_cell(cell, geometry, options);
    Some(
        wrapped
            .lines
            .iter()
            .enumerate()
            .map(|(index, spans)| {
                let mut line = PaintedCodeCellLine::default();
                append_wrapped_cell(
                    &mut line,
                    spans,
                    if index == 0 {
                        stack_gutter(
                            cell,
                            wrapped.gutter_width,
                            options.line_number_digits,
                            options.show_line_numbers,
                        )
                    } else {
                        " ".repeat(wrapped.gutter_width)
                    },
                    &wrapped,
                    options,
                    prefix,
                    highlight,
                    0,
                );
                append_note_guide(&mut line, options.guide_on_new_side, options.theme);
                append_run(
                    &mut line,
                    " ".repeat(trailing_width),
                    None,
                    Some(wrapped.palette.content_background.clone()),
                );
                line
            })
            .collect(),
    )
}

fn resolve_line_highlight_style(
    tone: HighlightTone,
    content_background: &str,
    theme: &AppTheme,
) -> Option<LineHighlightSpanStyle> {
    match line_highlight_tone_style(tone, content_background, theme)? {
        LineHighlightToneStyle::Colors {
            background,
            foreground,
        } => Some(LineHighlightSpanStyle {
            background: Some(background),
            foreground,
            transform_foreground: None,
        }),
        LineHighlightToneStyle::Dim => {
            let theme = theme.clone();
            Some(LineHighlightSpanStyle {
                background: None,
                foreground: None,
                transform_foreground: Some(RenderForegroundTransform::new(
                    move |source_foreground, rendered_background| {
                        dim_span_foreground(
                            source_foreground,
                            rendered_background,
                            &theme,
                            DEFAULT_DIM_RATIO,
                        )
                    },
                )),
            })
        }
    }
}

fn line_number_u64(line: usize) -> Option<u64> {
    u64::try_from(line).ok()
}

fn paint_split_line_highlights(
    cell: &SplitLineCell,
    side: ReviewSide,
    index: &LineHighlightPaintIndex,
    theme: &AppTheme,
) -> SplitLineCell {
    if cell.kind == SplitLineKind::Empty {
        return cell.clone();
    }
    let Some(ranges) = cell
        .line_number
        .and_then(line_number_u64)
        .and_then(|line| index.get(side, line))
    else {
        return cell.clone();
    };
    let content_background = split_palette(cell, theme).content_background;
    let mut painted = cell.clone();
    painted.spans = apply_prepared_line_highlights_to_spans(&cell.spans, ranges, |tone| {
        resolve_line_highlight_style(tone, &content_background, theme)
    });
    painted
}

/// Apply geometry-neutral extension line highlights before final cell paint.
#[must_use]
pub fn apply_code_cell_line_highlights(
    row: &DiffRow,
    index: Option<&LineHighlightPaintIndex>,
    theme: &AppTheme,
) -> DiffRow {
    let Some(index) = index.filter(|index| !index.is_empty()) else {
        return row.clone();
    };
    match row {
        DiffRow::SplitLine {
            key,
            file_id,
            hunk_index,
            left,
            right,
            is_expansion_row,
            expanded_gap_key,
        } => DiffRow::SplitLine {
            key: key.clone(),
            file_id: file_id.clone(),
            hunk_index: *hunk_index,
            left: paint_split_line_highlights(left, ReviewSide::Old, index, theme),
            right: paint_split_line_highlights(right, ReviewSide::New, index, theme),
            is_expansion_row: *is_expansion_row,
            expanded_gap_key: expanded_gap_key.clone(),
        },
        DiffRow::StackLine {
            key,
            file_id,
            hunk_index,
            cell,
            is_expansion_row,
            expanded_gap_key,
        } => {
            let ranges = cell
                .new_line_number
                .and_then(line_number_u64)
                .and_then(|line| index.get(ReviewSide::New, line))
                .or_else(|| {
                    cell.old_line_number
                        .and_then(line_number_u64)
                        .and_then(|line| index.get(ReviewSide::Old, line))
                });
            let mut painted = cell.clone();
            if let Some(ranges) = ranges {
                let content_background = stack_palette(cell, theme).content_background;
                painted.spans =
                    apply_prepared_line_highlights_to_spans(&cell.spans, ranges, |tone| {
                        resolve_line_highlight_style(tone, &content_background, theme)
                    });
            }
            DiffRow::StackLine {
                key: key.clone(),
                file_id: file_id.clone(),
                hunk_index: *hunk_index,
                cell: painted,
                is_expansion_row: *is_expansion_row,
                expanded_gap_key: expanded_gap_key.clone(),
            }
        }
        DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => row.clone(),
    }
}

/// Return a separately mountable reserved-column spacer.
#[must_use]
pub fn code_cell_spacer(width: usize, background: &str) -> PaintedCodeCellLine {
    let mut line = PaintedCodeCellLine::default();
    append_run(
        &mut line,
        " ".repeat(width),
        None,
        Some(background.to_owned()),
    );
    line
}

/// Conventional diff rail prefix for tests and renderer adapters.
#[must_use]
pub fn default_code_cell_prefix(foreground: String, theme: &AppTheme) -> CodeCellPrefix {
    CodeCellPrefix {
        text: diff_rail_marker().into(),
        foreground,
        background: theme.panel.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CodeRowLayoutOptions, contrast_ratio, legacy_planned_diff_row, plan_code_row_layout,
        resolve_theme, split_left_rail_color, split_right_rail_color, stack_rail_color,
        with_transparent_surfaces,
    };
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::widgets::Widget;

    fn span(text: &str) -> RenderSpan {
        RenderSpan {
            text: text.into(),
            foreground: None,
            background: None,
            transform_foreground: None,
        }
    }

    fn stack_row(key: &str, kind: StackLineKind, text: &str) -> DiffRow {
        DiffRow::StackLine {
            key: key.into(),
            file_id: "paint".into(),
            hunk_index: 0,
            cell: StackLineCell {
                kind,
                sign: match kind {
                    StackLineKind::Addition => "+",
                    StackLineKind::Deletion => "-",
                    StackLineKind::Context => " ",
                }
                .into(),
                old_line_number: (kind != StackLineKind::Addition).then_some(1),
                new_line_number: (kind != StackLineKind::Deletion).then_some(1),
                move_kind: None,
                spans: vec![span(text)],
            },
            is_expansion_row: false,
            expanded_gap_key: None,
        }
    }

    fn split_context_row() -> DiffRow {
        let cell = SplitLineCell {
            kind: SplitLineKind::Context,
            sign: " ".into(),
            line_number: Some(1),
            move_kind: None,
            spans: vec![span("shared")],
        };
        DiffRow::SplitLine {
            key: "paint:split-context".into(),
            file_id: "paint".into(),
            hunk_index: 0,
            left: cell.clone(),
            right: cell,
            is_expansion_row: false,
            expanded_gap_key: None,
        }
    }

    fn layout<'a>(
        planned: &'a crate::PlannedReviewRow,
        width: usize,
        wrap_lines: bool,
    ) -> CodeRowLayoutPlan<'a> {
        plan_code_row_layout(
            planned,
            CodeRowLayoutOptions {
                width,
                line_number_digits: 1,
                show_line_numbers: false,
                wrap_lines,
                reserve_add_note_column: false,
                show_add_note_badge: false,
            },
        )
        .expect("code row layout")
    }

    fn options(theme: &AppTheme, guide: bool) -> CodeCellPaintOptions<'_> {
        CodeCellPaintOptions {
            line_number_digits: 1,
            show_line_numbers: false,
            theme,
            horizontal_offset: 0,
            guide_on_new_side: guide,
        }
    }

    fn run_for_text<'a>(line: &'a PaintedCodeCellLine, text: &str) -> &'a PaintedCodeCellRun {
        line.runs
            .iter()
            .find(|run| run.text.contains(text))
            .unwrap_or_else(|| panic!("missing run containing {text:?} in {:?}", line.runs))
    }

    fn stack_prefix(row: &DiffRow, theme: &AppTheme, selected: bool) -> CodeCellPrefix {
        let DiffRow::StackLine { cell, .. } = row else {
            panic!("stack row")
        };
        default_code_cell_prefix(
            stack_rail_color(stack_kind(cell.kind), theme, selected),
            theme,
        )
    }

    fn paint_stack(
        row: &DiffRow,
        width: usize,
        wrap: bool,
        theme: &AppTheme,
        selected: bool,
        highlight: Option<CodeCellHighlight>,
        guide: bool,
    ) -> Vec<PaintedCodeCellLine> {
        let planned = legacy_planned_diff_row(row.clone(), None, guide.then_some(ReviewSide::New));
        let layout = layout(&planned, width, wrap);
        let prefix = stack_prefix(row, theme, selected);
        if wrap {
            paint_wrapped_stack_code_cell(
                row,
                &layout,
                options(theme, guide),
                &prefix,
                highlight,
                0,
            )
            .expect("wrapped stack paint")
        } else {
            vec![
                paint_nowrap_stack_code_cell(
                    row,
                    &layout,
                    options(theme, guide),
                    &prefix,
                    highlight,
                )
                .expect("nowrap stack paint"),
            ]
        }
    }

    #[test]
    fn partial_copy_ranges_match_the_frozen_nowrap_and_wrapped_oracle() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = stack_row("paint:stack", StackLineKind::Addition, "abcd");
        for wrap in [false, true] {
            let planned = legacy_planned_diff_row(row.clone(), None, None);
            let CodeRowLayoutPlan::Stack { cell, .. } = layout(&planned, 12, wrap) else {
                panic!("stack layout")
            };
            let start = cell.prefix_width + cell.gutter_width;
            let lines = paint_stack(
                &row,
                12,
                wrap,
                &theme,
                true,
                Some(CodeCellHighlight {
                    kind: CodeCellHighlightKind::Selection,
                    col_range: Some(CopySelectedRowRange {
                        start_col: start + 1,
                        end_col: start + 2,
                    }),
                }),
                false,
            );
            assert_eq!(lines.len(), 1);
            assert_eq!(lines[0].text(), "▌+ abcd     ");
            assert_eq!(lines[0].width(), 12);
            assert_eq!(
                lines[0].runs,
                vec![
                    PaintedCodeCellRun {
                        text: "▌".into(),
                        foreground: Some("#2ea043".into()),
                        background: Some("#322b19".into()),
                    },
                    PaintedCodeCellRun {
                        text: "+ ".into(),
                        foreground: Some("#2ea043".into()),
                        background: Some("#2f2b16".into()),
                    },
                    PaintedCodeCellRun {
                        text: "a".into(),
                        foreground: Some("#e6edf3".into()),
                        background: Some("#12251d".into()),
                    },
                    PaintedCodeCellRun {
                        text: "bc".into(),
                        foreground: Some("#e6edf3".into()),
                        background: Some("#2f2b16".into()),
                    },
                    PaintedCodeCellRun {
                        text: "d     ".into(),
                        foreground: Some("#e6edf3".into()),
                        background: Some("#12251d".into()),
                    },
                ]
            );
        }
    }

    #[test]
    fn extension_highlights_are_geometry_neutral_for_wide_and_combining_text() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = stack_row("paint:wide", StackLineKind::Addition, "a\u{301}日bc");
        let index = LineHighlightPaintIndex::from_line_ranges([(
            ReviewSide::New,
            1,
            vec![crate::LineHighlightColRange {
                start_col: 1,
                end_col: 3,
                tone: HighlightTone::Match,
            }],
        )]);
        let marked = apply_code_cell_line_highlights(&row, Some(&index), &theme);
        for wrap in [false, true] {
            let plain_lines = paint_stack(&row, 7, wrap, &theme, false, None, false);
            let marked_lines = paint_stack(&marked, 7, wrap, &theme, false, None, false);
            assert_eq!(
                plain_lines
                    .iter()
                    .map(PaintedCodeCellLine::text)
                    .collect::<Vec<_>>(),
                marked_lines
                    .iter()
                    .map(PaintedCodeCellLine::text)
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                plain_lines
                    .iter()
                    .map(PaintedCodeCellLine::width)
                    .collect::<Vec<_>>(),
                marked_lines
                    .iter()
                    .map(PaintedCodeCellLine::width)
                    .collect::<Vec<_>>()
            );
            assert_eq!(plain_lines[0].text(), "▌+ a\u{301}日b");
            if wrap {
                assert_eq!(plain_lines[1].text(), "▌  c   ");
            }
            assert_eq!(
                run_for_text(&marked_lines[0], "日").background.as_deref(),
                Some("#454017")
            );

            let mut buffer = Buffer::empty(Rect::new(0, 0, 7, 1));
            marked_lines[0]
                .ratatui_line()
                .render(Rect::new(0, 0, 7, 1), &mut buffer);
            assert_eq!(buffer[(4, 0)].bg, ratatui_theme_color("#454017"));
            assert_eq!(buffer[(4, 0)].symbol(), "日");
            // Ratatui reserves the continuation cell of a wide glyph; the
            // terminal paints both columns from the lead cell's style.
            assert_eq!(buffer[(5, 0)].symbol(), " ");
        }
    }

    fn dim_row(key: &str) -> (DiffRow, AppTheme, LineHighlightPaintIndex) {
        let theme = resolve_theme(Some("ayu-light"), None, &[]);
        let mut row = stack_row(key, StackLineKind::Addition, "dimtext");
        let DiffRow::StackLine { cell, .. } = &mut row else {
            unreachable!()
        };
        cell.spans[0].foreground = Some(theme.syntax_colors.default.clone());
        let index = LineHighlightPaintIndex::from_line_ranges([(
            ReviewSide::New,
            1,
            vec![crate::LineHighlightColRange {
                start_col: 0,
                end_col: 7,
                tone: HighlightTone::Dim,
            }],
        )]);
        (row, theme, index)
    }

    #[test]
    fn dim_foregrounds_resolve_against_the_final_cursor_background() {
        let (row, theme, index) = dim_row("paint:dim-cursor");
        let marked = apply_code_cell_line_highlights(&row, Some(&index), &theme);
        for wrap in [false, true] {
            let lines = paint_stack(
                &marked,
                12,
                wrap,
                &theme,
                false,
                Some(CodeCellHighlight {
                    kind: CodeCellHighlightKind::Cursor,
                    col_range: Some(FULL_CODE_CELL_COL_RANGE),
                }),
                false,
            );
            let run = run_for_text(&lines[0], "dimtext");
            assert_eq!(run.foreground.as_deref(), Some("#9ba19f"));
            assert_eq!(run.background.as_deref(), Some("#cfd6ce"));
            assert!(
                contrast_ratio(
                    run.foreground.as_deref().unwrap(),
                    run.background.as_deref().unwrap()
                ) >= 1.6
            );
        }
    }

    #[test]
    fn dim_selection_pieces_resolve_against_their_own_backgrounds() {
        let (row, theme, index) = dim_row("paint:dim-copy-selection");
        let marked = apply_code_cell_line_highlights(&row, Some(&index), &theme);
        for wrap in [false, true] {
            let planned = legacy_planned_diff_row(marked.clone(), None, None);
            let CodeRowLayoutPlan::Stack { cell, .. } = layout(&planned, 12, wrap) else {
                panic!("stack layout")
            };
            let start = cell.prefix_width + cell.gutter_width;
            let lines = paint_stack(
                &marked,
                12,
                wrap,
                &theme,
                true,
                Some(CodeCellHighlight {
                    kind: CodeCellHighlightKind::Selection,
                    col_range: Some(CopySelectedRowRange {
                        start_col: start + 1,
                        end_col: start + 3,
                    }),
                }),
                false,
            );
            let ordinary = run_for_text(&lines[0], "d");
            let selected = run_for_text(&lines[0], "imt");
            assert_eq!(ordinary.foreground.as_deref(), Some("#abb1ae"));
            assert_eq!(ordinary.background.as_deref(), Some("#ecf3e8"));
            assert_eq!(selected.foreground.as_deref(), Some("#a4acb2"));
            assert_eq!(selected.background.as_deref(), Some("#dfeaf0"));
            for run in [ordinary, selected] {
                assert!(
                    contrast_ratio(
                        run.foreground.as_deref().unwrap(),
                        run.background.as_deref().unwrap()
                    ) >= 1.6
                );
            }
        }
    }

    #[test]
    fn transparent_cursor_paint_keeps_split_and_stack_note_guides() {
        let theme =
            with_transparent_surfaces(&resolve_theme(Some("github-dark-default"), None, &[]));
        let rows = [
            split_context_row(),
            stack_row("paint:stack-context", StackLineKind::Context, "shared"),
        ];
        for row in rows {
            for wrap in [false, true] {
                let planned = legacy_planned_diff_row(row.clone(), None, Some(ReviewSide::New));
                let width = if matches!(row, DiffRow::SplitLine { .. }) {
                    24
                } else {
                    12
                };
                let layout = layout(&planned, width, wrap);
                let highlight = Some(CodeCellHighlight {
                    kind: CodeCellHighlightKind::Cursor,
                    col_range: Some(FULL_CODE_CELL_COL_RANGE),
                });
                let lines = match &row {
                    DiffRow::StackLine { cell, .. } => {
                        let prefix = default_code_cell_prefix(
                            stack_rail_color(stack_kind(cell.kind), &theme, false),
                            &theme,
                        );
                        if wrap {
                            paint_wrapped_stack_code_cell(
                                &row,
                                &layout,
                                options(&theme, true),
                                &prefix,
                                highlight,
                                0,
                            )
                            .unwrap()
                        } else {
                            vec![
                                paint_nowrap_stack_code_cell(
                                    &row,
                                    &layout,
                                    options(&theme, true),
                                    &prefix,
                                    highlight,
                                )
                                .unwrap(),
                            ]
                        }
                    }
                    DiffRow::SplitLine { left, right, .. } => {
                        let left_prefix = default_code_cell_prefix(
                            split_left_rail_color(split_kind(left.kind), &theme, false),
                            &theme,
                        );
                        let right_prefix = default_code_cell_prefix(
                            split_right_rail_color(split_kind(right.kind), &theme, false),
                            &theme,
                        );
                        if wrap {
                            paint_wrapped_split_code_cells(
                                &row,
                                &layout,
                                options(&theme, true),
                                &left_prefix,
                                &right_prefix,
                                highlight,
                                highlight,
                                0,
                            )
                            .unwrap()
                        } else {
                            vec![
                                paint_nowrap_split_code_cells(
                                    &row,
                                    &layout,
                                    options(&theme, true),
                                    &left_prefix,
                                    &right_prefix,
                                    highlight,
                                    highlight,
                                )
                                .unwrap(),
                            ]
                        }
                    }
                    DiffRow::Collapsed { .. } | DiffRow::HunkHeader { .. } => unreachable!(),
                };
                assert!(lines[0].text().ends_with('│'));
                assert_eq!(lines[0].width(), width);
                assert_eq!(
                    run_for_text(&lines[0], "shared").background.as_deref(),
                    Some("#2e2f31")
                );
                assert_eq!(
                    lines[0].runs.last().unwrap().foreground.as_deref(),
                    Some("#bb8009")
                );
            }
        }
    }

    #[test]
    fn spacers_and_ratatui_projection_preserve_exact_width_and_colors() {
        let spacer = code_cell_spacer(3, "#112233");
        assert_eq!(spacer.text(), "   ");
        assert_eq!(spacer.width(), 3);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        spacer
            .ratatui_line()
            .render(Rect::new(0, 0, 3, 1), &mut buffer);
        assert!((0..3).all(|column| buffer[(column, 0)].bg == ratatui_theme_color("#112233")));
    }

    #[test]
    fn nowrap_offsets_sanitized_text_and_number_cursor_stays_out_of_content() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = stack_row("paint:offset", StackLineKind::Addition, "\u{1b}[2Jabcdef");
        let planned = legacy_planned_diff_row(row.clone(), None, None);
        let layout = layout(&planned, 7, false);
        let prefix = stack_prefix(&row, &theme, false);
        let line = paint_nowrap_stack_code_cell(
            &row,
            &layout,
            CodeCellPaintOptions {
                horizontal_offset: 2,
                ..options(&theme, false)
            },
            &prefix,
            Some(CodeCellHighlight {
                kind: CodeCellHighlightKind::Cursor,
                col_range: None,
            }),
        )
        .unwrap();
        assert_eq!(line.text(), "▌+ cdef");
        assert_eq!(line.width(), 7);
        assert_eq!(
            line.runs[0].background.as_deref(),
            Some(cursor_line_highlight_background(&theme.panel, &theme).as_str())
        );
        assert_eq!(
            line.runs[1].background.as_deref(),
            Some(cursor_line_highlight_background(&theme.added_bg, &theme).as_str())
        );
        assert_eq!(
            run_for_text(&line, "cdef").background.as_deref(),
            Some("#12251d")
        );
    }

    #[test]
    fn wrapped_rows_reserve_and_fill_the_add_note_column() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let row = stack_row("paint:badge", StackLineKind::Addition, "abcdef");
        let planned = legacy_planned_diff_row(row.clone(), None, None);
        let layout = plan_code_row_layout(
            &planned,
            CodeRowLayoutOptions {
                width: 10,
                line_number_digits: 1,
                show_line_numbers: false,
                wrap_lines: true,
                reserve_add_note_column: true,
                show_add_note_badge: false,
            },
        )
        .unwrap();
        let prefix = stack_prefix(&row, &theme, false);
        let lines =
            paint_wrapped_stack_code_cell(&row, &layout, options(&theme, false), &prefix, None, 3)
                .unwrap();
        assert_eq!(
            lines
                .iter()
                .map(PaintedCodeCellLine::text)
                .collect::<Vec<_>>(),
            ["▌+ abcd   ", "▌  ef     "]
        );
        assert!(lines.iter().all(|line| line.width() == 10));
        assert!(lines.iter().all(|line| {
            line.runs.last().is_some_and(|run| {
                run.background.as_deref() == Some("#12251d") && run.text.ends_with("   ")
            })
        }));
    }
}
