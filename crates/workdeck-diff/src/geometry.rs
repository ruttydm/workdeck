//! Grapheme-safe terminal-cell planning for styled diff spans.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::sanitize_terminal_line;

/// A styled text fragment whose style type is owned by the caller's renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextSegment<S> {
    pub text: String,
    pub style: S,
}

/// One visible horizontal window plus the cells its retained spans occupy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentWindow<S> {
    pub segments: Vec<TextSegment<S>>,
    pub used_width: usize,
}

fn append_segment<S: Clone + PartialEq>(
    target: &mut Vec<TextSegment<S>>,
    text: impl Into<String>,
    style: &S,
) {
    let text = text.into();
    if text.is_empty() {
        return;
    }
    if let Some(previous) = target.last_mut().filter(|span| span.style == *style) {
        previous.text.push_str(&text);
    } else {
        target.push(TextSegment {
            text,
            style: style.clone(),
        });
    }
}

fn is_printable_ascii_text(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

fn is_grapheme_prepend(codepoint: u32) -> bool {
    matches!(
        codepoint,
        0x0600..=0x0605
            | 0x06dd
            | 0x070f
            | 0x0890..=0x0891
            | 0x08e2
            | 0x0d4e
            | 0x110bd
            | 0x110cd
            | 0x111c2..=0x111c3
            | 0x1193f
            | 0x11941
            | 0x11a3a
            | 0x11a84..=0x11a89
            | 0x11d46
            | 0x11f02
    )
}

fn scalar_requires_grapheme_composition(character: char) -> bool {
    let codepoint = character as u32;
    character.width().is_none_or(|width| width == 0)
        || (0x1f3fb..=0x1f3ff).contains(&codepoint)
        || (0x1f1e6..=0x1f1ff).contains(&codepoint)
        || is_grapheme_prepend(codepoint)
        || matches!(codepoint, 0x0e33 | 0x0eb3 | 0xff9e | 0xff9f)
        || (0x1100..=0x11ff).contains(&codepoint)
        || (0xa960..=0xa97f).contains(&codepoint)
        || (0xd7b0..=0xd7ff).contains(&codepoint)
}

fn measure_simple_sanitized_text_width(text: &str) -> Option<usize> {
    let mut width = 0_usize;
    for character in text.chars() {
        if scalar_requires_grapheme_composition(character) {
            return None;
        }
        width = width.saturating_add(character.width().unwrap_or_default());
    }
    Some(width)
}

fn measure_sanitized_text_width(text: &str) -> usize {
    if is_printable_ascii_text(text) {
        return text.len();
    }
    measure_simple_sanitized_text_width(text).unwrap_or_else(|| {
        UnicodeSegmentation::graphemes(text, true)
            .map(UnicodeWidthStr::width)
            .sum()
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TextWidthSlice {
    text: String,
    width: usize,
}

fn slice_sanitized_text_by_width(text: &str, offset: usize, width: usize) -> TextWidthSlice {
    if width == 0 {
        return TextWidthSlice {
            text: String::new(),
            width: 0,
        };
    }
    if is_printable_ascii_text(text) {
        let visible = text
            .as_bytes()
            .get(offset..offset.saturating_add(width).min(text.len()))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or_default()
            .to_owned();
        return TextWidthSlice {
            width: visible.len(),
            text: visible,
        };
    }

    let mut cursor = 0_usize;
    let mut used_width = 0_usize;
    let mut visible = String::new();
    for cluster in UnicodeSegmentation::graphemes(text, true) {
        let cluster_width = cluster.width();
        let cluster_start = cursor;
        let cluster_end = cursor.saturating_add(cluster_width);
        cursor = cluster_end;
        if cluster_end <= offset {
            continue;
        }
        if cluster_start < offset {
            let hidden = cluster_end.min(offset.saturating_add(width)) - offset;
            visible.extend(std::iter::repeat_n(' ', hidden));
            used_width = used_width.saturating_add(hidden);
            continue;
        }
        if used_width.saturating_add(cluster_width) > width {
            break;
        }
        visible.push_str(cluster);
        used_width = used_width.saturating_add(cluster_width);
    }
    TextWidthSlice {
        text: visible,
        width: used_width,
    }
}

fn spans_may_split_grapheme<S>(spans: &[TextSegment<S>]) -> bool {
    spans.windows(2).any(|boundary| {
        let left = boundary[0].text.chars().next_back();
        let right = boundary[1].text.chars().next();
        left.is_some_and(scalar_requires_grapheme_composition)
            || right.is_some_and(scalar_requires_grapheme_composition)
    })
}

/// Merge indivisible graphemes while preserving the style where each cluster starts.
fn merge_cross_span_graphemes<S: Clone + PartialEq>(
    spans: &[TextSegment<S>],
) -> Vec<TextSegment<S>> {
    let mut normalized = Vec::new();
    let text = spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>();
    let mut source_index = 0_usize;
    let mut source_end = spans.first().map_or(0, |span| span.text.len());
    for (cursor, cluster) in UnicodeSegmentation::grapheme_indices(text.as_str(), true) {
        while cursor >= source_end && source_index < spans.len().saturating_sub(1) {
            source_index += 1;
            source_end = source_end.saturating_add(spans[source_index].text.len());
        }
        if let Some(source) = spans.get(source_index) {
            append_segment(&mut normalized, cluster, &source.style);
        }
    }
    normalized
}

fn preserve_cross_span_graphemes<S: Clone + PartialEq>(
    spans: &[TextSegment<S>],
) -> Vec<TextSegment<S>> {
    if spans_may_split_grapheme(spans) {
        merge_cross_span_graphemes(spans)
    } else {
        spans.to_vec()
    }
}

fn sanitize_segments<S: Clone + PartialEq>(spans: &[TextSegment<S>]) -> Vec<TextSegment<S>> {
    let mut safe = Vec::with_capacity(spans.len());
    for span in spans {
        let text = sanitize_terminal_line(&span.text);
        append_segment(&mut safe, text, &span.style);
    }
    safe
}

/// Slice styled spans to one visible cell window while preserving style runs.
#[must_use]
pub fn slice_segments_window<S: Clone + PartialEq>(
    spans: &[TextSegment<S>],
    offset: usize,
    width: usize,
) -> SegmentWindow<S> {
    if width == 0 {
        return SegmentWindow {
            segments: Vec::new(),
            used_width: 0,
        };
    }

    let mut sliced = Vec::new();
    let mut remaining_offset = offset;
    let mut remaining = width;
    let mut used_width = 0_usize;
    for span in spans {
        if remaining == 0 {
            break;
        }
        let span_width = measure_sanitized_text_width(&span.text);
        if span_width == 0 {
            append_segment(&mut sliced, span.text.clone(), &span.style);
            continue;
        }
        if remaining_offset >= span_width {
            remaining_offset -= span_width;
            continue;
        }
        if remaining_offset == 0 && span_width <= remaining {
            append_segment(&mut sliced, span.text.clone(), &span.style);
            remaining -= span_width;
            used_width = used_width.saturating_add(span_width);
            continue;
        }

        let visible = slice_sanitized_text_by_width(&span.text, remaining_offset, remaining);
        remaining_offset = 0;
        if visible.text.is_empty() {
            continue;
        }
        append_segment(&mut sliced, visible.text, &span.style);
        remaining -= visible.width;
        used_width = used_width.saturating_add(visible.width);
    }
    SegmentWindow {
        segments: sliced,
        used_width,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WrappedTextChunk {
    text: String,
    width: usize,
    starts_new_line: bool,
}

fn wrap_sanitized_text_by_width(
    text: &str,
    line_width: usize,
    first_line_width: usize,
    first_line_has_content: bool,
) -> Vec<WrappedTextChunk> {
    if line_width == 0 || text.is_empty() {
        return Vec::new();
    }
    let mut remaining = first_line_width.min(line_width);
    let mut chunks = Vec::new();
    let mut chunk_text = String::new();
    let mut chunk_width = 0_usize;
    let mut starts_new_line = false;
    let mut existing_line_has_content = first_line_has_content;

    let flush = |chunks: &mut Vec<WrappedTextChunk>,
                 text: &mut String,
                 width: &mut usize,
                 starts_new_line: &mut bool| {
        if text.is_empty() {
            return;
        }
        chunks.push(WrappedTextChunk {
            text: std::mem::take(text),
            width: *width,
            starts_new_line: *starts_new_line,
        });
        *width = 0;
        *starts_new_line = false;
    };

    for cluster in UnicodeSegmentation::graphemes(text, true) {
        let width = cluster.width();
        if width > remaining {
            let row_started =
                existing_line_has_content || remaining < line_width || !chunk_text.is_empty();
            flush(
                &mut chunks,
                &mut chunk_text,
                &mut chunk_width,
                &mut starts_new_line,
            );
            remaining = line_width;
            starts_new_line = row_started;
            existing_line_has_content = false;
            if width > line_width {
                if row_started {
                    chunks.push(WrappedTextChunk {
                        text: String::new(),
                        width: 0,
                        starts_new_line: true,
                    });
                }
                starts_new_line = false;
                continue;
            }
        }
        chunk_text.push_str(cluster);
        chunk_width = chunk_width.saturating_add(width);
        remaining -= width;
    }
    flush(
        &mut chunks,
        &mut chunk_text,
        &mut chunk_width,
        &mut starts_new_line,
    );
    chunks
}

const SINGLE_PASS_WRAP_LINE_THRESHOLD: usize = 8;

/// Wrap styled text on terminal-cell boundaries without splitting grapheme clusters.
#[must_use]
pub fn wrap_segments<S: Clone + PartialEq>(
    segments: Vec<TextSegment<S>>,
    width: usize,
) -> Vec<Vec<TextSegment<S>>> {
    if width == 0 {
        return vec![Vec::new()];
    }

    let safe = sanitize_segments(&segments);
    let has_composition_sensitive_span = safe
        .iter()
        .any(|span| measure_simple_sanitized_text_width(&span.text).is_none());
    let planned = if safe.len() > 1 && has_composition_sensitive_span {
        merge_cross_span_graphemes(&safe)
    } else {
        safe
    };
    let simple_widths = planned
        .iter()
        .map(|span| measure_simple_sanitized_text_width(&span.text))
        .collect::<Vec<_>>();
    let mut lines = vec![Vec::new()];
    let mut remaining = width;

    for (span, simple_width) in planned.iter().zip(simple_widths) {
        let span_width = simple_width.unwrap_or_else(|| measure_sanitized_text_width(&span.text));
        if span_width == 0 {
            append_segment(
                lines.last_mut().expect("wrapped lines are never empty"),
                span.text.clone(),
                &span.style,
            );
            continue;
        }

        if span_width > width.saturating_mul(SINGLE_PASS_WRAP_LINE_THRESHOLD)
            || simple_width.is_none()
            || (width == 1 && !is_printable_ascii_text(&span.text))
        {
            let line_has_content = lines.last().is_some_and(|line| !line.is_empty());
            for chunk in
                wrap_sanitized_text_by_width(&span.text, width, remaining, line_has_content)
            {
                if chunk.starts_new_line {
                    lines.push(Vec::new());
                    remaining = width;
                }
                if !chunk.text.is_empty() {
                    append_segment(
                        lines.last_mut().expect("wrapped lines are never empty"),
                        chunk.text,
                        &span.style,
                    );
                }
                remaining -= chunk.width;
            }
            continue;
        }

        let mut offset = 0_usize;
        while offset < span_width {
            if remaining == 0 {
                lines.push(Vec::new());
                remaining = width;
            }
            let visible = slice_sanitized_text_by_width(&span.text, offset, remaining);
            if visible.width == 0 {
                if lines.last().is_some_and(|line| !line.is_empty()) || remaining < width {
                    lines.push(Vec::new());
                    remaining = width;
                }
                let forced = slice_sanitized_text_by_width(&span.text, offset, width);
                if forced.width == 0 {
                    break;
                }
                lines
                    .last_mut()
                    .expect("wrapped lines are never empty")
                    .push(TextSegment {
                        text: forced.text,
                        style: span.style.clone(),
                    });
                offset = offset.saturating_add(forced.width);
                remaining = width.saturating_sub(forced.width);
                continue;
            }
            append_segment(
                lines.last_mut().expect("wrapped lines are never empty"),
                visible.text,
                &span.style,
            );
            offset = offset.saturating_add(visible.width);
            remaining -= visible.width;
        }
    }
    lines
}

/// Count wrapped visual lines without allocating styled row arrays.
#[must_use]
pub fn measure_wrapped_segments_line_count<S: Clone + PartialEq>(
    spans: &[TextSegment<S>],
    width: usize,
) -> usize {
    if width == 0 {
        return 1;
    }
    let safe = preserve_cross_span_graphemes(&sanitize_segments(spans));
    let mut line_count = 1_usize;
    let mut remaining = width;
    let mut current_line_has_content = false;
    for span in safe {
        for chunk in
            wrap_sanitized_text_by_width(&span.text, width, remaining, current_line_has_content)
        {
            if chunk.starts_new_line {
                line_count = line_count.saturating_add(1);
                remaining = width;
                current_line_has_content = false;
            }
            remaining -= chunk.width;
            current_line_has_content |= !chunk.text.is_empty();
        }
    }
    line_count
}

/// Clip styled text to a cell width.
#[must_use]
pub fn clip_segments<S: Clone + PartialEq>(
    segments: Vec<TextSegment<S>>,
    width: usize,
) -> Vec<TextSegment<S>> {
    slice_segments_window(&segments, 0, width).segments
}

/// Measure styled text using terminal cells rather than bytes or scalar count.
#[must_use]
pub fn segments_width<S>(segments: &[TextSegment<S>]) -> usize {
    segments
        .iter()
        .map(|segment| measure_sanitized_text_width(&segment.text))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_cell_windows_and_keeps_complete_wide_graphemes() {
        let spans = vec![
            TextSegment {
                text: "a日本".into(),
                style: 1,
            },
            TextSegment {
                text: "b".into(),
                style: 2,
            },
        ];
        assert_eq!(
            slice_segments_window(&spans, 1, 4),
            SegmentWindow {
                segments: vec![TextSegment {
                    text: "日本".into(),
                    style: 1
                }],
                used_width: 4
            }
        );
        assert_eq!(
            slice_segments_window(&spans, 2, 4),
            SegmentWindow {
                segments: vec![
                    TextSegment {
                        text: " 本".into(),
                        style: 1
                    },
                    TextSegment {
                        text: "b".into(),
                        style: 2
                    }
                ],
                used_width: 4
            }
        );
    }

    #[test]
    fn wraps_by_cells_and_preserves_color_runs() {
        let rows = wrap_segments(
            vec![
                TextSegment {
                    text: "ab界".into(),
                    style: 1,
                },
                TextSegment {
                    text: "cd".into(),
                    style: 2,
                },
            ],
            4,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0][0].text, "ab界");
        assert_eq!(rows[0][0].style, 1);
        assert_eq!(rows[1][0].text, "cd");
        assert_eq!(segments_width(&rows[0]), 4);
    }

    #[test]
    fn preserves_cross_span_graphemes_with_the_starting_style() {
        let rows = wrap_segments(
            vec![
                TextSegment {
                    text: "e".into(),
                    style: 1,
                },
                TextSegment {
                    text: "\u{301}x".into(),
                    style: 2,
                },
            ],
            1,
        );
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            [TextSegment {
                text: "e\u{301}".into(),
                style: 1
            }]
        );
        assert_eq!(
            rows[1],
            [TextSegment {
                text: "x".into(),
                style: 2
            }]
        );
    }

    #[test]
    fn concrete_wrapping_and_allocation_free_measurement_agree() {
        for width in 0..=8 {
            for spans in [
                vec![TextSegment {
                    text: "abcdefghijk".into(),
                    style: 1,
                }],
                vec![
                    TextSegment {
                        text: "a🧑‍💻".into(),
                        style: 1,
                    },
                    TextSegment {
                        text: "日本e\u{301}x".into(),
                        style: 2,
                    },
                ],
            ] {
                assert_eq!(
                    measure_wrapped_segments_line_count(&spans, width),
                    wrap_segments(spans, width).len()
                );
            }
        }
    }

    #[test]
    fn wrapping_sanitizes_controls_and_zero_width_returns_one_row() {
        let rows = wrap_segments(
            vec![TextSegment {
                text: "safe\x1b]52;c;eA==\x07text".into(),
                style: (),
            }],
            20,
        );
        assert_eq!(rows[0][0].text, "safetext");
        assert_eq!(wrap_segments::<()>(Vec::new(), 0), vec![Vec::new()]);
    }
}
