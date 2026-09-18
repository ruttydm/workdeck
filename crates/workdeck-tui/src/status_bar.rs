//! Status-bar geometry and Unicode-safe file-filter editing.

use crate::{measure_text_width, slice_text_by_width};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBarInputView {
    pub text: String,
    pub cursor_column: usize,
    pub scroll: usize,
}

/// Match Hunk's capped keyboard-mode badge width.
#[must_use]
pub fn status_bar_mode_width(mode_text: Option<&str>, terminal_width: u16) -> u16 {
    mode_text.map_or(0, |text| {
        u16::try_from(text.width().saturating_add(2))
            .unwrap_or(u16::MAX)
            .min((terminal_width / 2).max(6))
    })
}

/// Project the single-line filter editor through OpenTUI's 20% scroll margin.
#[must_use]
pub fn status_bar_input_view(
    value: &str,
    cursor: usize,
    input_width: usize,
    previous_scroll: usize,
) -> StatusBarInputView {
    if input_width == 0 {
        return StatusBarInputView {
            text: String::new(),
            cursor_column: 0,
            scroll: 0,
        };
    }
    let cursor = clamp_filter_cursor(value, cursor);
    let cursor_byte = value
        .char_indices()
        .nth(cursor)
        .map_or(value.len(), |(index, _)| index);
    let cursor_cells = measure_text_width(&value[..cursor_byte]);
    let value_width = measure_text_width(value);
    let margin = (input_width / 5).max(1).min(input_width.saturating_sub(1));
    let right_guard = input_width.saturating_sub(margin.saturating_add(1));
    let mut scroll = previous_scroll.min(value_width);
    if cursor_cells < scroll.saturating_add(margin) {
        scroll = cursor_cells.saturating_sub(margin);
    } else if cursor_cells > scroll.saturating_add(right_guard) {
        scroll = cursor_cells.saturating_sub(right_guard);
    }
    let visible = slice_text_by_width(value, scroll, input_width);
    StatusBarInputView {
        text: visible.text,
        cursor_column: cursor_cells.saturating_sub(scroll).min(input_width - 1),
        scroll,
    }
}

#[must_use]
pub fn clamp_filter_cursor(value: &str, cursor: usize) -> usize {
    cursor.min(value.chars().count())
}

pub fn insert_filter_character(value: &mut String, cursor: &mut usize, character: char) {
    *cursor = clamp_filter_cursor(value, *cursor);
    let byte = value
        .char_indices()
        .nth(*cursor)
        .map_or(value.len(), |(index, _)| index);
    value.insert(byte, character);
    *cursor = cursor.saturating_add(1);
}

pub fn remove_filter_character_before(value: &mut String, cursor: &mut usize) {
    *cursor = clamp_filter_cursor(value, *cursor);
    let Some(previous) = cursor.checked_sub(1) else {
        return;
    };
    let start = value
        .char_indices()
        .nth(previous)
        .map_or(value.len(), |(index, _)| index);
    let end = value
        .char_indices()
        .nth(*cursor)
        .map_or(value.len(), |(index, _)| index);
    value.replace_range(start..end, "");
    *cursor = previous;
}

pub fn remove_filter_character_at(value: &mut String, cursor: &mut usize) {
    *cursor = clamp_filter_cursor(value, *cursor);
    let start = value
        .char_indices()
        .nth(*cursor)
        .map_or(value.len(), |(index, _)| index);
    let end = value
        .char_indices()
        .nth(cursor.saturating_add(1))
        .map_or(value.len(), |(index, _)| index);
    if start < end {
        value.replace_range(start..end, "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_width_adds_padding_caps_at_half_and_keeps_the_six_cell_floor() {
        assert_eq!(status_bar_mode_width(None, 80), 0);
        assert_eq!(status_bar_mode_width(Some("vim"), 80), 5);
        assert_eq!(status_bar_mode_width(Some(&"x".repeat(80)), 80), 40);
        assert_eq!(status_bar_mode_width(Some("long mode"), 8), 6);
    }

    #[test]
    fn filter_edits_by_unicode_scalar_without_splitting_utf8() {
        let mut value = "a界b".to_owned();
        let mut cursor = 2;
        insert_filter_character(&mut value, &mut cursor, '🙂');
        assert_eq!(value, "a界🙂b");
        assert_eq!(cursor, 3);
        remove_filter_character_before(&mut value, &mut cursor);
        assert_eq!(value, "a界b");
        assert_eq!(cursor, 2);
        remove_filter_character_at(&mut value, &mut cursor);
        assert_eq!(value, "a界");
    }

    #[test]
    fn filter_editing_clamps_stale_cursors_and_boundaries() {
        let mut value = "ab".to_owned();
        let mut cursor = 99;
        remove_filter_character_at(&mut value, &mut cursor);
        assert_eq!(value, "ab");
        assert_eq!(cursor, 2);
        remove_filter_character_before(&mut value, &mut cursor);
        assert_eq!(value, "a");
        assert_eq!(cursor, 1);
    }

    #[test]
    fn input_view_matches_the_pinned_scroll_margin_and_cursor_frames() {
        assert_eq!(
            status_bar_input_view("beta", 4, 29, 0),
            StatusBarInputView {
                text: "beta".into(),
                cursor_column: 4,
                scroll: 0,
            }
        );
        let long = status_bar_input_view("0123456789abcdefghijklmnopqrstuvwxyz", 36, 29, 0);
        assert_eq!(long.text, "defghijklmnopqrstuvwxyz");
        assert_eq!(long.cursor_column, 23);
        assert_eq!(long.scroll, 13);

        let one_left =
            status_bar_input_view("0123456789abcdefghijklmnopqrstuvwxyz", 35, 29, long.scroll);
        assert_eq!(one_left.scroll, 13);
        assert_eq!(one_left.cursor_column, 22);
    }

    #[test]
    fn input_view_slices_wide_text_without_emitting_partial_glyphs() {
        let view = status_bar_input_view("a界🙂b", 4, 4, 0);
        assert!(view.text.is_char_boundary(view.text.len()));
        assert!(view.cursor_column < 4);
        assert!(measure_text_width(&view.text) <= 4);
    }
}
