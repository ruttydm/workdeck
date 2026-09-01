//! Terminal-cell dialog, modal, and adaptive scroll geometry.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
use workdeck_diff::sanitize_terminal_line;
use workdeck_extension_api::ExtensionKeyEvent;

pub const MODAL_FRAME_CHROME_ROWS: u16 = 5;
pub const RAPID_SCROLL_OVERSCAN_IDLE_MS: u64 = 160;
pub const VIEWPORT_READ_COALESCE_MS: u64 = 16;
pub const CODE_ROW_ADD_NOTE_BADGE_TEXT: &str = "[+]";
pub const CODE_ROW_ADD_NOTE_BADGE_WIDTH: u16 = 3;

const RAPID_SCROLL_MIN_DELTA_ROWS: u64 = 4;
const RAPID_SCROLL_MIN_VIEWPORT_MULTIPLIER: u64 = 3;
const RAPID_SCROLL_MAX_OVERSCAN_ROWS: u64 = 240;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowedDialogText {
    pub lines: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostKeyEvent<'a> {
    pub name: &'a str,
    pub sequence: &'a str,
    pub ctrl: bool,
    pub meta: bool,
    pub option: bool,
    pub shift: bool,
}

/// Copy a host keyboard event into the owned, method-free extension API shape.
#[must_use]
pub fn to_extension_key_event(key: HostKeyEvent<'_>) -> ExtensionKeyEvent {
    ExtensionKeyEvent {
        name: key.name.into(),
        sequence: key.sequence.into(),
        ctrl: key.ctrl,
        meta: key.meta,
        option: key.option,
        shift: key.shift,
    }
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

/// Estimate review viewport height before a backend publishes exact pane geometry.
#[must_use]
pub fn estimate_initial_render_viewport_height(
    renderer_height: i64,
    screen_top: i64,
    pane_height: Option<i64>,
) -> i64 {
    let available_renderer_height = renderer_height.saturating_sub(screen_top.max(0));
    pane_height
        .map_or(available_renderer_height, |height| {
            available_renderer_height.min(height)
        })
        .max(1)
}

/// Prefer measured viewport geometry while retaining a non-empty first-paint estimate.
#[must_use]
pub fn resolve_render_viewport_height(measured_height: i64, estimated_height: i64) -> i64 {
    if measured_height > 0 {
        measured_height
    } else {
        estimated_height.max(1)
    }
}

/// Clamp a dragged sidebar width into the active terminal layout's allowed range.
#[must_use]
pub fn resize_sidebar_width(
    start_width: i64,
    drag_origin_x: i64,
    current_x: i64,
    min_width: i64,
    max_width: i64,
) -> i64 {
    start_width
        .saturating_add(current_x.saturating_sub(drag_origin_x))
        .max(min_width)
        .min(max_width)
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
    fn sidebar_drag_delta_clamps_at_both_layout_boundaries() {
        assert_eq!(resize_sidebar_width(30, 40, 47, 20, 50), 37);
        assert_eq!(resize_sidebar_width(30, 40, 5, 20, 50), 20);
        assert_eq!(resize_sidebar_width(30, 40, 100, 20, 50), 50);
        assert_eq!(resize_sidebar_width(30, 40, 40, 40, 20), 20);
    }

    #[test]
    fn add_note_badge_reserves_its_exact_terminal_columns() {
        assert_eq!(CODE_ROW_ADD_NOTE_BADGE_TEXT.width(), 3);
        assert_eq!(CODE_ROW_ADD_NOTE_BADGE_WIDTH, 3);
    }

    #[test]
    fn extension_key_event_is_an_owned_method_free_snapshot() {
        let snapshot = to_extension_key_event(HostKeyEvent {
            name: "g",
            sequence: "G",
            ctrl: false,
            meta: false,
            option: true,
            shift: true,
        });
        assert_eq!(
            snapshot,
            ExtensionKeyEvent {
                name: "g".into(),
                sequence: "G".into(),
                ctrl: false,
                meta: false,
                option: true,
                shift: true,
            }
        );
        assert_eq!(
            serde_json::to_value(snapshot).unwrap(),
            serde_json::json!({
                "name": "g",
                "sequence": "G",
                "ctrl": false,
                "meta": false,
                "option": true,
                "shift": true,
            })
        );
    }

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

    #[test]
    fn initial_viewport_subtracts_pane_screen_top_from_renderer_height() {
        assert_eq!(estimate_initial_render_viewport_height(80, 2, None), 78);
    }

    #[test]
    fn initial_viewport_never_returns_empty_while_geometry_is_unknown() {
        assert_eq!(estimate_initial_render_viewport_height(0, 0, None), 1);
    }

    #[test]
    fn initial_viewport_excludes_a_bottom_extension_pane() {
        assert_eq!(estimate_initial_render_viewport_height(100, 1, Some(5)), 5);
    }

    #[test]
    fn render_viewport_falls_back_while_measured_height_is_zero() {
        assert_eq!(resolve_render_viewport_height(0, 48), 48);
    }

    #[test]
    fn render_viewport_keeps_measured_height_once_available() {
        assert_eq!(resolve_render_viewport_height(36, 48), 36);
    }
}
