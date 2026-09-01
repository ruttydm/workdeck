//! Terminal-cell dialog, modal, and adaptive scroll geometry.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_diff::sanitize_terminal_line;

pub const MODAL_FRAME_CHROME_ROWS: u16 = 5;
pub const RAPID_SCROLL_OVERSCAN_IDLE_MS: u64 = 160;

const RAPID_SCROLL_MIN_DELTA_ROWS: u64 = 4;
const RAPID_SCROLL_MIN_VIEWPORT_MULTIPLIER: u64 = 3;
const RAPID_SCROLL_MAX_OVERSCAN_ROWS: u64 = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowedDialogText {
    pub lines: Vec<String>,
    pub truncated: bool,
}

/// Wrap prose to terminal cells and reserve the final allocated row for overflow.
#[must_use]
pub fn window_dialog_text(
    source_lines: &[impl AsRef<str>],
    width: usize,
    max_rows: usize,
) -> WindowedDialogText {
    let wrapped = source_lines
        .iter()
        .flat_map(|line| wrap_prose(line.as_ref(), width))
        .collect::<Vec<_>>();
    if wrapped.len() <= max_rows {
        return WindowedDialogText {
            lines: wrapped,
            truncated: false,
        };
    }
    if max_rows == 0 {
        return WindowedDialogText {
            lines: Vec::new(),
            truncated: !wrapped.is_empty(),
        };
    }
    let mut lines = wrapped
        .into_iter()
        .take(max_rows.saturating_sub(1))
        .collect::<Vec<_>>();
    lines.push("…".into());
    WindowedDialogText {
        lines,
        truncated: true,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalGeometry {
    pub width: u16,
    pub height: u16,
    pub left: u16,
    pub top: u16,
}

/// Clamp a requested modal to one-cell viewport margins and center it.
#[must_use]
pub fn resolve_modal_geometry(
    width: u16,
    height: u16,
    terminal_width: u16,
    terminal_height: u16,
) -> ModalGeometry {
    let available_width = terminal_width.saturating_sub(2).max(1);
    let available_height = terminal_height.saturating_sub(2).max(1);
    let resolved_width = width.min(available_width).max(1);
    let resolved_height = height.min(available_height).max(1);
    ModalGeometry {
        width: resolved_width,
        height: resolved_height,
        left: terminal_width.saturating_sub(resolved_width) / 2,
        top: terminal_height.saturating_sub(resolved_height) / 2,
    }
}

/// Expand virtual-render overscan for bursty scrolling and cap retained rows.
#[must_use]
pub fn compute_rapid_scroll_overscan_rows(delta_rows: i64, viewport_height: i64) -> usize {
    let absolute_delta = delta_rows.unsigned_abs();
    if absolute_delta < RAPID_SCROLL_MIN_DELTA_ROWS {
        return 0;
    }
    let viewport_rows = viewport_height.max(1) as u64;
    absolute_delta
        .saturating_mul(2)
        .max(viewport_rows.saturating_mul(RAPID_SCROLL_MIN_VIEWPORT_MULTIPLIER))
        .min(RAPID_SCROLL_MAX_OVERSCAN_ROWS) as usize
}

fn wrap_prose(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }
    let safe = sanitize_terminal_line(text);
    let words = safe.split_whitespace().collect::<Vec<_>>();
    if words.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    for word in words {
        let word_width = word.width();
        if word_width > width {
            push_current(&mut lines, &mut current, &mut current_width);
            split_long_word(word, width, &mut lines);
            continue;
        }
        let next_width = if current.is_empty() {
            word_width
        } else {
            current_width.saturating_add(1).saturating_add(word_width)
        };
        if next_width <= width {
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
            current_width = next_width;
        } else {
            push_current(&mut lines, &mut current, &mut current_width);
            current.push_str(word);
            current_width = word_width;
        }
    }
    push_current(&mut lines, &mut current, &mut current_width);
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn push_current(lines: &mut Vec<String>, current: &mut String, current_width: &mut usize) {
    if !current.is_empty() {
        lines.push(std::mem::take(current));
        *current_width = 0;
    }
}

fn split_long_word(word: &str, width: usize, lines: &mut Vec<String>) {
    let mut current = String::new();
    let mut used = 0_usize;
    for character in word.chars() {
        let character_width = character.width().unwrap_or(0);
        if used > 0 && used.saturating_add(character_width) > width {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push(character);
        used = used.saturating_add(character_width);
        if used > width && current.chars().count() == 1 {
            lines.push(std::mem::take(&mut current));
            used = 0;
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_text_wraps_prose_within_available_terminal_rows() {
        assert_eq!(
            window_dialog_text(&["one two three"], 7, 3),
            WindowedDialogText {
                lines: vec!["one two".into(), "three".into()],
                truncated: false,
            }
        );
    }

    #[test]
    fn dialog_text_pins_overflow_marker_to_final_allocated_row() {
        assert_eq!(
            window_dialog_text(&["one two three four"], 7, 2),
            WindowedDialogText {
                lines: vec!["one two".into(), "…".into()],
                truncated: true,
            }
        );
        assert_eq!(
            window_dialog_text(&["overflow"], 3, 0),
            WindowedDialogText {
                lines: Vec::new(),
                truncated: true,
            }
        );
    }

    #[test]
    fn modal_centers_requested_dimensions_inside_terminal() {
        assert_eq!(
            resolve_modal_geometry(40, 10, 80, 24),
            ModalGeometry {
                width: 40,
                height: 10,
                left: 20,
                top: 7,
            }
        );
    }

    #[test]
    fn modal_uses_real_narrow_viewport_without_artificial_minimum() {
        assert_eq!(
            resolve_modal_geometry(72, 20, 30, 8),
            ModalGeometry {
                width: 28,
                height: 6,
                left: 1,
                top: 1,
            }
        );
    }

    #[test]
    fn slow_scroll_uses_default_window() {
        assert_eq!(compute_rapid_scroll_overscan_rows(1, 30), 0);
        assert_eq!(compute_rapid_scroll_overscan_rows(-3, 30), 0);
    }

    #[test]
    fn rapid_scroll_threshold_is_inclusive() {
        assert_eq!(compute_rapid_scroll_overscan_rows(4, 30), 90);
    }

    #[test]
    fn rapid_scroll_expands_to_at_least_three_viewports() {
        assert_eq!(compute_rapid_scroll_overscan_rows(8, 30), 90);
    }

    #[test]
    fn rapid_scroll_scales_with_large_jumps_but_stays_bounded() {
        assert_eq!(compute_rapid_scroll_overscan_rows(80, 20), 160);
        assert_eq!(compute_rapid_scroll_overscan_rows(-1_000, 40), 240);
    }
}
