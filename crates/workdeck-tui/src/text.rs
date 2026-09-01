//! Grapheme-safe terminal-cell measurement, slicing, fitting, and wrapping.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_diff::sanitize_terminal_line;

pub const CLUSTER_WIDTH_CACHE_MAX_ENTRIES: usize = 256;
pub const CLUSTER_WIDTH_CACHE_MAX_KEY_CODE_UNITS: usize = 64;

#[must_use]
pub fn is_printable_ascii_text(text: &str) -> bool {
    text.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
}

#[must_use]
pub fn text_clusters(text: &str) -> Vec<&str> {
    UnicodeSegmentation::graphemes(text, true).collect()
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

fn is_regional_indicator(codepoint: u32) -> bool {
    (0x1f1e6..=0x1f1ff).contains(&codepoint)
}

fn is_emoji_modifier(codepoint: u32) -> bool {
    (0x1f3fb..=0x1f3ff).contains(&codepoint)
}

fn scalar_requires_grapheme_composition(character: char) -> bool {
    let codepoint = character as u32;
    character.width().is_none_or(|width| width == 0)
        || is_emoji_modifier(codepoint)
        || is_regional_indicator(codepoint)
        || is_grapheme_prepend(codepoint)
        || matches!(codepoint, 0x0e33 | 0x0eb3 | 0xff9e | 0xff9f)
        || (0x1100..=0x11ff).contains(&codepoint)
        || (0xa960..=0xa97f).contains(&codepoint)
        || (0xd7b0..=0xd7ff).contains(&codepoint)
}

/// Return a direct width for independent scalars, or `None` when grapheme
/// composition is required.
#[must_use]
pub fn measure_simple_sanitized_text_width(text: &str) -> Option<usize> {
    let mut width = 0;
    for character in text.chars() {
        if scalar_requires_grapheme_composition(character) {
            return None;
        }
        width += character.width().unwrap_or_default();
    }
    Some(width)
}

/// FIFO cache that cannot retain an unbounded count or size of source clusters.
#[derive(Debug, Clone)]
pub struct BoundedClusterWidthCache {
    max_entries: usize,
    max_key_code_units: usize,
    entries: HashMap<String, usize>,
    order: VecDeque<String>,
}

impl BoundedClusterWidthCache {
    #[must_use]
    pub fn new(max_entries: usize, max_key_code_units: usize) -> Self {
        Self {
            max_entries,
            max_key_code_units,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    #[must_use]
    pub fn get(&self, cluster: &str) -> Option<usize> {
        self.entries.get(cluster).copied()
    }

    pub fn set(&mut self, cluster: &str, width: usize) {
        if cluster.encode_utf16().count() > self.max_key_code_units {
            return;
        }
        if !self.entries.contains_key(cluster) {
            self.order.push_back(cluster.to_owned());
        }
        self.entries.insert(cluster.to_owned(), width);
        while self.entries.len() > self.max_entries {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

fn global_cluster_width_cache() -> &'static Mutex<BoundedClusterWidthCache> {
    static CACHE: OnceLock<Mutex<BoundedClusterWidthCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(BoundedClusterWidthCache::new(
            CLUSTER_WIDTH_CACHE_MAX_ENTRIES,
            CLUSTER_WIDTH_CACHE_MAX_KEY_CODE_UNITS,
        ))
    })
}

/// Measure one grapheme cluster with a bounded shared cache.
#[must_use]
pub fn measure_cluster_width(cluster: &str) -> usize {
    if cluster.len() == 1 && is_printable_ascii_text(cluster) {
        return 1;
    }
    if cluster.is_empty() {
        return 0;
    }
    let cache = global_cluster_width_cache();
    if let Some(width) = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(cluster)
    {
        return width;
    }
    let width = cluster.width();
    cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .set(cluster, width);
    width
}

fn repeated_scalar(text: &str) -> Option<char> {
    let mut characters = text.chars();
    let first = characters.next()?;
    let mut count = 1;
    for character in characters {
        if character != first {
            return None;
        }
        count += 1;
    }
    (count >= 2).then_some(first)
}

/// Measure text that has already passed terminal sanitization.
#[must_use]
pub fn measure_sanitized_text_width(text: &str) -> usize {
    if is_printable_ascii_text(text) {
        return text.len();
    }
    if let Some(character) = repeated_scalar(text) {
        let scalar = character.to_string();
        let width = measure_cluster_width(&scalar);
        if width > 0 && !scalar_requires_grapheme_composition(character) {
            return width * text.chars().count();
        }
    }
    if let Some(width) = measure_simple_sanitized_text_width(text) {
        return width;
    }
    UnicodeSegmentation::graphemes(text, true)
        .map(measure_cluster_width)
        .sum()
}

/// Measure sanitized terminal cells, treating CJK and emoji clusters as wide.
#[must_use]
pub fn measure_text_width(text: &str) -> usize {
    measure_sanitized_text_width(&sanitize_terminal_line(text))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextWidthSlice {
    pub text: String,
    pub width: usize,
}

/// Slice already-sanitized text by terminal cells without splitting clusters.
#[must_use]
pub fn slice_sanitized_text_by_width(
    safe_text: &str,
    offset: usize,
    width: usize,
) -> TextWidthSlice {
    if width == 0 {
        return TextWidthSlice {
            text: String::new(),
            width: 0,
        };
    }
    if is_printable_ascii_text(safe_text) {
        let text = safe_text
            .as_bytes()
            .get(offset..offset.saturating_add(width).min(safe_text.len()))
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .unwrap_or_default()
            .to_owned();
        let width = text.len();
        return TextWidthSlice { text, width };
    }

    let mut cursor = 0;
    let mut used_width = 0;
    let mut visible = String::new();
    for cluster in UnicodeSegmentation::graphemes(safe_text, true) {
        let cluster_width = measure_cluster_width(cluster);
        let cluster_start = cursor;
        let cluster_end = cursor + cluster_width;
        cursor = cluster_end;
        if cluster_end <= offset {
            continue;
        }
        if cluster_start < offset {
            let hidden = cluster_end.min(offset.saturating_add(width)) - offset;
            if hidden > 0 {
                visible.extend(std::iter::repeat_n(' ', hidden));
                used_width += hidden;
            }
            continue;
        }
        if used_width + cluster_width > width {
            break;
        }
        visible.push_str(cluster);
        used_width += cluster_width;
    }
    TextWidthSlice {
        text: visible,
        width: used_width,
    }
}

#[must_use]
pub fn slice_text_by_width(text: &str, offset: usize, width: usize) -> TextWidthSlice {
    slice_sanitized_text_by_width(&sanitize_terminal_line(text), offset, width)
}

/// Wrap prose at word boundaries and hard-split words by terminal cells.
#[must_use]
pub fn wrap_text(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let normalized = sanitize_terminal_line(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if normalized.is_empty() {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for word in normalized.split(' ') {
        let word_width = measure_text_width(word);
        if word_width > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            let mut offset = 0;
            while offset < word_width {
                let chunk = slice_text_by_width(word, offset, width);
                if chunk.width == 0 {
                    let rest = slice_text_by_width(word, offset, usize::MAX);
                    if !rest.text.is_empty() {
                        lines.push(rest.text);
                    }
                    break;
                }
                offset += chunk.width;
                lines.push(chunk.text);
            }
            continue;
        }

        let next_width = if current.is_empty() {
            word_width
        } else {
            current_width + 1 + word_width
        };
        if next_width <= width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            current_width = next_width;
        } else {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrappedTextChunk {
    pub text: String,
    pub width: usize,
    pub starts_new_line: bool,
}

/// Wrap already-sanitized text while retaining first-line remaining capacity.
#[must_use]
pub fn wrap_sanitized_text_by_width(
    safe_text: &str,
    line_width: usize,
    first_line_width: Option<usize>,
    first_line_has_content: bool,
) -> Vec<WrappedTextChunk> {
    let full_width = line_width;
    if full_width == 0 || safe_text.is_empty() {
        return Vec::new();
    }
    let mut remaining = first_line_width.unwrap_or(full_width).min(full_width);
    let mut chunks = Vec::new();
    let mut text = String::new();
    let mut chunk_width = 0;
    let mut starts_new_line = false;
    let mut existing_line_has_content = first_line_has_content;

    let flush = |chunks: &mut Vec<WrappedTextChunk>,
                 text: &mut String,
                 chunk_width: &mut usize,
                 starts_new_line: &mut bool| {
        if text.is_empty() {
            return;
        }
        chunks.push(WrappedTextChunk {
            text: std::mem::take(text),
            width: *chunk_width,
            starts_new_line: *starts_new_line,
        });
        *chunk_width = 0;
        *starts_new_line = false;
    };

    for cluster in UnicodeSegmentation::graphemes(safe_text, true) {
        let width = measure_cluster_width(cluster);
        if width > remaining {
            let row_started =
                existing_line_has_content || remaining < full_width || !text.is_empty();
            flush(
                &mut chunks,
                &mut text,
                &mut chunk_width,
                &mut starts_new_line,
            );
            remaining = full_width;
            starts_new_line = row_started;
            existing_line_has_content = false;
            if width > full_width {
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
        text.push_str(cluster);
        chunk_width += width;
        remaining -= width;
    }
    flush(
        &mut chunks,
        &mut text,
        &mut chunk_width,
        &mut starts_new_line,
    );
    chunks
}

#[must_use]
pub fn wrap_text_by_width(
    text: &str,
    line_width: usize,
    first_line_width: Option<usize>,
    first_line_has_content: bool,
) -> Vec<WrappedTextChunk> {
    wrap_sanitized_text_by_width(
        &sanitize_terminal_line(text),
        line_width,
        first_line_width,
        first_line_has_content,
    )
}

/// Inclusive terminal-cell range converted to Hunk-compatible UTF-16 bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Utf16TextRange {
    pub start_index: usize,
    pub end_index: usize,
}

#[must_use]
pub fn cell_range_to_utf16_range(text: &str, start_cell: usize, end_cell: usize) -> Utf16TextRange {
    let text_units = text.encode_utf16().count();
    if is_printable_ascii_text(text) {
        let start_index = text_units.min(start_cell);
        return Utf16TextRange {
            start_index,
            end_index: text_units.min(end_cell.saturating_add(1).max(start_index)),
        };
    }

    let mut cell_cursor = 0;
    let mut unit_cursor = 0;
    let mut start_index = None;
    let mut end_index = text_units;
    for cluster in UnicodeSegmentation::graphemes(text, true) {
        if cell_cursor > end_cell {
            end_index = unit_cursor;
            break;
        }
        let width = measure_cluster_width(cluster);
        let covers_start = if width > 0 {
            cell_cursor + width > start_cell
        } else {
            cell_cursor >= start_cell
        };
        if start_index.is_none() && covers_start {
            start_index = Some(unit_cursor);
        }
        cell_cursor += width;
        unit_cursor += cluster.encode_utf16().count();
    }
    let start_index = start_index.unwrap_or(text_units);
    Utf16TextRange {
        start_index,
        end_index: end_index.max(start_index),
    }
}

/// Clamp text to a fixed cell width with a cell-aware overflow marker.
#[must_use]
pub fn fit_text(text: &str, width: usize, overflow_marker: Option<&str>) -> String {
    if width == 0 {
        return String::new();
    }
    let safe_text = sanitize_terminal_line(text);
    if measure_sanitized_text_width(&safe_text) <= width {
        return safe_text;
    }
    let safe_marker = sanitize_terminal_line(overflow_marker.unwrap_or("."));
    let marker = slice_sanitized_text_by_width(&safe_marker, 0, width);
    let text_width = width.saturating_sub(marker.width);
    format!(
        "{}{}",
        slice_sanitized_text_by_width(&safe_text, 0, text_width).text,
        marker.text
    )
}

/// Clamp and right-pad text to an exact cell width.
#[must_use]
pub fn pad_text(text: &str, width: usize) -> String {
    let fitted = fit_text(text, width, None);
    let padding = width.saturating_sub(measure_text_width(&fitted));
    format!("{fitted}{}", " ".repeat(padding))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slice(text: &str, offset: usize, width: usize) -> TextWidthSlice {
        slice_text_by_width(text, offset, width)
    }

    #[test]
    fn fit_pad_measure_and_slice_by_terminal_cells() {
        assert_eq!(fit_text("hello", 0, None), "");
        assert_eq!(fit_text("hello", 1, None), ".");
        assert_eq!(fit_text("hello", 4, None), "hel.");
        assert_eq!(pad_text("hello", 4), "hel.");
        assert_eq!(pad_text("ok", 4), "ok  ");
        assert_eq!(measure_text_width("日本語"), 6);
        assert_eq!(
            slice("a日本b", 1, 4),
            TextWidthSlice {
                text: "日本".into(),
                width: 4
            }
        );
        assert_eq!(
            slice("a日本b", 2, 4),
            TextWidthSlice {
                text: " 本b".into(),
                width: 4
            }
        );
        assert_eq!(
            slice("日本b", 3, 3),
            TextWidthSlice {
                text: " b".into(),
                width: 2
            }
        );
        assert_eq!(
            slice("日", 1, 1),
            TextWidthSlice {
                text: " ".into(),
                width: 1
            }
        );
        assert_eq!(slice("👍🏽x", 0, 2).text, "👍🏽");
        assert_eq!(
            slice("🧑‍💻x", 1, 1),
            TextWidthSlice {
                text: " ".into(),
                width: 1
            }
        );
        assert_eq!(slice("e\u{301}x", 0, 1).text, "e\u{301}");
        assert_eq!(slice("♥️x", 0, 2).text, "♥️");
        assert_eq!(fit_text("日本語", 5, None), "日本.");
        assert_eq!(measure_text_width(&pad_text("日本", 6)), 6);
    }

    #[test]
    fn width_wrapping_keeps_graphemes_and_remaining_line_capacity() {
        assert_eq!(
            wrap_text_by_width("a日本b", 4, None, false),
            [
                WrappedTextChunk {
                    text: "a日".into(),
                    width: 3,
                    starts_new_line: false
                },
                WrappedTextChunk {
                    text: "本b".into(),
                    width: 3,
                    starts_new_line: true
                },
            ]
        );
        assert_eq!(
            wrap_text_by_width("abcdef", 4, Some(2), false),
            [
                WrappedTextChunk {
                    text: "ab".into(),
                    width: 2,
                    starts_new_line: false
                },
                WrappedTextChunk {
                    text: "cdef".into(),
                    width: 4,
                    starts_new_line: true
                },
            ]
        );
        assert_eq!(wrap_text_by_width("e\u{301}x", 1, None, false).len(), 2);
        assert_eq!(slice("🇯🇵", 0, 1).width, 0);
        assert!(wrap_text_by_width("🇯🇵", 1, None, false).is_empty());
        assert_eq!(slice("\u{d4e}കx", 0, 1).text, "\u{d4e}ക");
        assert_eq!(wrap_text_by_width("\u{d4e}കx", 1, None, false).len(), 2);
        for cluster in ["กำ", "ກຳ", "ｶﾞ", "ｶﾟ"] {
            let width = cluster.width();
            assert_eq!(measure_text_width(cluster), width);
            assert_eq!(slice(cluster, 0, width).text, cluster);
            assert_eq!(wrap_text_by_width(cluster, width, None, false).len(), 1);
        }
    }

    #[test]
    fn cell_ranges_return_exact_utf16_slice_bounds() {
        let range = |text, start, end| cell_range_to_utf16_range(text, start, end);
        assert_eq!(
            range("hello", 1, 3),
            Utf16TextRange {
                start_index: 1,
                end_index: 4
            }
        );
        assert_eq!(
            range("hello", 0, 99),
            Utf16TextRange {
                start_index: 0,
                end_index: 5
            }
        );
        assert_eq!(
            range("a日本b", 1, 2),
            Utf16TextRange {
                start_index: 1,
                end_index: 2
            }
        );
        assert_eq!(
            range("a日本b", 3, 5),
            Utf16TextRange {
                start_index: 2,
                end_index: 4
            }
        );
        assert_eq!(
            range("a日本b", 6, 9),
            Utf16TextRange {
                start_index: 4,
                end_index: 4
            }
        );
        assert_eq!(
            range("a日本b", 2, 3),
            Utf16TextRange {
                start_index: 1,
                end_index: 3
            }
        );
        assert_eq!(
            range("👍a", 1, 2),
            Utf16TextRange {
                start_index: 0,
                end_index: 3
            }
        );
        assert_eq!(
            range("👍a", 2, 2),
            Utf16TextRange {
                start_index: 2,
                end_index: 3
            }
        );
        assert_eq!(
            range("🧑‍💻x", 1, 1),
            Utf16TextRange {
                start_index: 0,
                end_index: 5
            }
        );
        assert_eq!(
            range("🧑‍💻x", 2, 2),
            Utf16TextRange {
                start_index: 5,
                end_index: 6
            }
        );
        assert_eq!(
            range("\u{200b}ab", 0, 0),
            Utf16TextRange {
                start_index: 0,
                end_index: 2
            }
        );
        assert_eq!(
            range("\u{200b}ab", 1, 1),
            Utf16TextRange {
                start_index: 2,
                end_index: 3
            }
        );
    }

    #[test]
    fn cache_is_fifo_and_rejects_oversized_utf16_keys() {
        let mut cache = BoundedClusterWidthCache::new(
            CLUSTER_WIDTH_CACHE_MAX_ENTRIES,
            CLUSTER_WIDTH_CACHE_MAX_KEY_CODE_UNITS,
        );
        for index in 0..CLUSTER_WIDTH_CACHE_MAX_ENTRIES {
            cache.set(&format!("cluster-{index}"), index);
        }
        assert_eq!(cache.get("cluster-0"), Some(0));
        cache.set("cluster-new", CLUSTER_WIDTH_CACHE_MAX_ENTRIES);
        assert_eq!(cache.len(), CLUSTER_WIDTH_CACHE_MAX_ENTRIES);
        assert_eq!(cache.get("cluster-0"), None);
        assert_eq!(
            cache.get("cluster-new"),
            Some(CLUSTER_WIDTH_CACHE_MAX_ENTRIES)
        );
        let oversized = "x".repeat(CLUSTER_WIDTH_CACHE_MAX_KEY_CODE_UNITS + 1);
        cache.set(&oversized, 999);
        assert_eq!(cache.len(), CLUSTER_WIDTH_CACHE_MAX_ENTRIES);
        assert_eq!(cache.get(&oversized), None);
        assert_eq!(cache.get("cluster-1"), Some(1));
    }

    #[test]
    fn repeated_and_complex_cluster_widths_stay_exact() {
        assert_eq!(measure_text_width(&"─".repeat(240)), 240);
        assert_eq!(fit_text(&"─".repeat(240), 240, None), "─".repeat(240));
        assert_eq!(
            fit_text(&"─".repeat(300), 240, None),
            format!("{}.", "─".repeat(239))
        );
        assert_eq!(measure_text_width(&"好".repeat(120)), 240);
        assert_eq!(fit_text(&"好".repeat(4), 6, None), "好好.");
        assert_eq!(measure_text_width(&"👍".repeat(3)), 6);
        assert_eq!(measure_text_width(&"\u{301}".repeat(4)), 0);
        assert_eq!(measure_text_width("e\u{301}"), 1);
        for line in [
            "日本語 scalar text 👍 🚀",
            "🧑‍💻 👩‍🔬 terminal tools",
            "👍🏽 emoji modifier",
            "1️⃣ keycap sequence",
            "♥️ variation selector",
            "🇯🇵 regional indicators",
            "e\u{301} a\u{308} combining text",
            "\u{1100}\u{1161}\u{11a8} Hangul Jamo",
        ] {
            assert_eq!(measure_text_width(line), line.width(), "{line}");
        }
    }

    #[test]
    fn prose_wrapping_prefers_words_and_never_splits_clusters() {
        assert_eq!(wrap_text("alpha beta gamma", 8), ["alpha", "beta", "gamma"]);
        assert_eq!(
            wrap_text("supercalifragilistic", 6),
            ["superc", "alifra", "gilist", "ic"]
        );
        assert_eq!(wrap_text("こんにちは世界", 8), ["こんにち", "は世界"]);
        assert_eq!(
            wrap_text("これは全角文字の長い注釈です", 10),
            ["これは全角", "文字の長い", "注釈です"]
        );
        assert_eq!(
            wrap_text("fix 説明が長い日本語のまま続く", 10),
            ["fix", "説明が長い", "日本語のま", "ま続く"]
        );
        assert_eq!(wrap_text("🎉🎉🎉", 4), ["🎉🎉", "🎉"]);
        assert_eq!(wrap_text("日本語", 3), ["日", "本", "語"]);
        assert_eq!(wrap_text("ab cd", 5), ["ab cd"]);
        assert_eq!(wrap_text("日日", 1), ["日日"]);
        assert_eq!(wrap_text("🧑‍💻🧑‍💻", 2), ["🧑‍💻", "🧑‍💻"]);
    }
}
