//! Shared confirmation modal and mouse-action footer.
//!
//! This ports Hunk's `src/ui/components/chrome/ConfirmDialog.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Ratatui owns painting while
//! callers retain keyboard routing and action execution.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use unicode_segmentation::UnicodeSegmentation;

use crate::{
    AppTheme, ModalFrameOptions, ModalFramePlan, measure_text_width, ratatui_theme_color,
    render_modal_frame,
};

pub const CONFIRM_DIALOG_CHROME_ROWS: usize =
    crate::ui_geometry::MODAL_FRAME_CHROME_ROWS as usize + 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDialogAction {
    pub key_label: String,
    pub label: String,
}

impl ConfirmDialogAction {
    #[must_use]
    pub fn new(key_label: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            key_label: key_label.into(),
            label: label.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogActionHit {
    pub index: usize,
    pub key_label: String,
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmDialogRenderMap {
    pub modal: ModalFramePlan,
    pub body: Rect,
    pub body_row_count: usize,
    pub action_gap: Option<Rect>,
    pub action_row: Option<Rect>,
    pub action_hits: Vec<DialogActionHit>,
    pub show_actions: bool,
    pub show_action_gap: bool,
}

/// Height requested by a confirmation with this many fixed one-row children.
#[must_use]
pub const fn confirm_dialog_height(body_rows: usize) -> usize {
    body_rows.saturating_add(CONFIRM_DIALOG_CHROME_ROWS)
}

fn clamped_row(area: Rect, y: u16) -> Rect {
    if y >= area.bottom() {
        Rect::new(area.x, area.bottom(), area.width, 0)
    } else {
        Rect::new(area.x, y, area.width, 1)
    }
}

fn paint_dialog_action_row(
    area: Rect,
    buffer: &mut Buffer,
    actions: &[ConfirmDialogAction],
    hovered_action_key: Option<&str>,
    theme: &AppTheme,
) -> Vec<DialogActionHit> {
    if area.width == 0 || area.height == 0 {
        return Vec::new();
    }
    let mut x = area.x;
    let mut hits = Vec::new();
    for (index, action) in actions.iter().enumerate() {
        if index > 0 {
            let separator_width = 3_u16.min(area.right().saturating_sub(x));
            if separator_width > 0 {
                paint_transparent_text(
                    buffer,
                    Rect::new(x, area.y, separator_width, 1),
                    " · ",
                    Style::default()
                        .fg(ratatui_theme_color(&theme.badge_neutral))
                        .bg(ratatui_theme_color(&theme.panel)),
                );
                x = x.saturating_add(separator_width);
            }
        }
        if x >= area.right() {
            break;
        }
        let requested_width = measure_text_width(&action.key_label)
            .saturating_add(measure_text_width(&action.label))
            .saturating_add(3);
        let width = u16::try_from(requested_width)
            .unwrap_or(u16::MAX)
            .min(area.right().saturating_sub(x));
        if width == 0 {
            break;
        }
        let bounds = Rect::new(x, area.y, width, 1);
        let hovered = hovered_action_key == Some(action.key_label.as_str());
        let background = if hovered {
            ratatui_theme_color(&theme.accent_muted)
        } else {
            ratatui_theme_color(&theme.panel)
        };
        buffer.set_style(bounds, Style::default().bg(background));
        let key_x = bounds.x.saturating_add(1);
        let key_width = u16::try_from(measure_text_width(&action.key_label)).unwrap_or(u16::MAX);
        paint_transparent_text(
            buffer,
            Rect::new(key_x, bounds.y, bounds.right().saturating_sub(key_x), 1),
            &action.key_label,
            Style::default()
                .fg(ratatui_theme_color(&theme.accent))
                .bg(background),
        );
        let label_x = key_x.saturating_add(key_width);
        paint_transparent_text(
            buffer,
            Rect::new(label_x, bounds.y, bounds.right().saturating_sub(label_x), 1),
            &format!(" {} ", action.label),
            Style::default()
                .fg(ratatui_theme_color(if hovered {
                    &theme.text
                } else {
                    &theme.muted
                }))
                .bg(background),
        );
        hits.push(DialogActionHit {
            index,
            key_label: action.key_label.clone(),
            bounds,
        });
        x = x.saturating_add(width);
    }
    hits
}

fn paint_transparent_text(buffer: &mut Buffer, area: Rect, text: &str, style: Style) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let mut x = area.x;
    for grapheme in text.graphemes(true) {
        let width = u16::try_from(measure_text_width(grapheme)).unwrap_or(u16::MAX);
        if width == 0 {
            continue;
        }
        if x.saturating_add(width) > area.right() {
            break;
        }
        if !grapheme.chars().all(char::is_whitespace) {
            buffer.set_stringn(x, area.y, grapheme, usize::from(width), style);
        }
        x = x.saturating_add(width);
    }
}

/// Paint one fixed child text row without erasing a border underneath source
/// overflow. Ordinary in-frame spaces remain visually identical.
pub(crate) fn paint_confirm_dialog_text(buffer: &mut Buffer, area: Rect, text: &str, style: Style) {
    paint_transparent_text(buffer, area, text, style);
}

/// Paint shared modal chrome and the optional action footer, returning the
/// rectangle where the caller paints its fixed body rows.
#[allow(clippy::too_many_arguments)]
pub fn render_confirm_dialog(
    area: Rect,
    buffer: &mut Buffer,
    width: u16,
    height: u16,
    title: &str,
    closeable: bool,
    body_row_count: usize,
    actions: &[ConfirmDialogAction],
    hovered_action_key: Option<&str>,
    theme: &AppTheme,
) -> ConfirmDialogRenderMap {
    let modal = render_modal_frame(
        area,
        buffer,
        ModalFrameOptions {
            width,
            height,
            closeable,
            has_mouse_scroll_handler: false,
            terminal_width: area.width,
            terminal_height: area.height,
            theme,
            title,
        },
    );
    let show_actions =
        usize::from(modal.frame.height) > usize::from(crate::ui_geometry::MODAL_FRAME_CHROME_ROWS);
    let show_action_gap = usize::from(modal.frame.height) >= confirm_dialog_height(0);
    // When the frame is too short for all documented chrome, OpenTUI flexes
    // away the normal blank row between title and children.
    let body_y = if show_action_gap {
        modal.content.y
    } else {
        modal.frame.y.saturating_add(3)
    };
    let body = Rect::new(
        modal.frame.x.saturating_add(2),
        body_y,
        modal.frame.width.saturating_sub(4),
        u16::try_from(body_row_count)
            .unwrap_or(u16::MAX)
            .min(area.bottom().saturating_sub(body_y)),
    );
    let gap_y = body_y.saturating_add(u16::try_from(body_row_count).unwrap_or(u16::MAX));
    let body_column = Rect::new(
        body.x,
        body_y,
        body.width,
        area.bottom().saturating_sub(body_y),
    );
    let action_gap = show_action_gap.then(|| clamped_row(body_column, gap_y));
    let action_y = gap_y.saturating_add(u16::from(show_action_gap));
    let action_row = show_actions.then(|| clamped_row(body_column, action_y));
    let action_hits = action_row
        .map(|row| paint_dialog_action_row(row, buffer, actions, hovered_action_key, theme))
        .unwrap_or_default();
    ConfirmDialogRenderMap {
        modal,
        body,
        body_row_count,
        action_gap,
        action_row,
        action_hits,
        show_actions,
        show_action_gap,
    }
}

/// Return the action under one pointer position.
#[must_use]
pub fn dialog_action_at(
    map: &ConfirmDialogRenderMap,
    column: u16,
    row: u16,
) -> Option<&DialogActionHit> {
    map.action_hits.iter().find(|hit| {
        column >= hit.bounds.x
            && column < hit.bounds.right()
            && row >= hit.bounds.y
            && row < hit.bounds.bottom()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_theme;

    fn frame(buffer: &Buffer, width: u16) -> String {
        buffer
            .content()
            .chunks(usize::from(width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn actions() -> Vec<ConfirmDialogAction> {
        vec![
            ConfirmDialogAction::new("enter/y", "accept"),
            ConfirmDialogAction::new("esc/n", "cancel"),
        ]
    }

    fn render_specimen(height: u16, hovered: Option<&str>) -> (Buffer, ConfirmDialogRenderMap) {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let area = Rect::new(0, 0, 80, height);
        let mut buffer = Buffer::empty(area);
        let map = render_confirm_dialog(
            area,
            &mut buffer,
            40,
            u16::try_from(confirm_dialog_height(2)).unwrap(),
            "Confirm action?",
            true,
            2,
            &actions(),
            hovered,
            &theme,
        );
        if map.body.height > 0 {
            for (offset, text) in ["first body", "second body"].into_iter().enumerate() {
                let y = map
                    .body
                    .y
                    .saturating_add(u16::try_from(offset).unwrap_or(u16::MAX));
                if y >= map.body.bottom() {
                    break;
                }
                paint_transparent_text(
                    &mut buffer,
                    Rect::new(map.body.x, y, map.body.width, 1),
                    text,
                    Style::default(),
                );
            }
        }
        (buffer, map)
    }

    #[test]
    fn height_helper_reserves_frame_gap_and_action_rows() {
        assert_eq!(crate::ui_geometry::MODAL_FRAME_CHROME_ROWS, 5);
        assert_eq!(CONFIRM_DIALOG_CHROME_ROWS, 7);
        assert_eq!(confirm_dialog_height(0), 7);
        assert_eq!(confirm_dialog_height(4), 11);
    }

    #[test]
    fn roomy_frame_matches_the_frozen_opentui_geometry_and_content() {
        let (buffer, map) = render_specimen(20, None);
        let output = frame(&buffer, 80);
        for expected in [
            "Confirm action?",
            "first body",
            "second body",
            "enter/y accept  ·  esc/n cancel",
        ] {
            assert!(output.contains(expected), "missing {expected:?}");
        }
        assert_eq!(map.modal.frame, Rect::new(20, 5, 40, 9));
        assert_eq!(map.body, Rect::new(22, 9, 36, 2));
        assert_eq!(map.action_gap, Some(Rect::new(22, 11, 36, 1)));
        assert_eq!(map.action_row, Some(Rect::new(22, 12, 36, 1)));
        assert_eq!(map.action_hits[0].bounds, Rect::new(22, 12, 16, 1));
        assert_eq!(map.action_hits[1].bounds, Rect::new(41, 12, 14, 1));
        assert_eq!(
            workdeck_core::review_digest(format!("{output}\n\n").as_bytes()),
            "7440b4285775971f98b83b87ef89b84162e15a7d2c8a382104b0e6bef7578f22"
        );
    }

    #[test]
    fn clamped_frames_match_action_and_body_overflow_thresholds() {
        let (six_buffer, six) = render_specimen(8, None);
        let six_output = frame(&six_buffer, 80);
        assert!(six.show_actions);
        assert!(!six.show_action_gap);
        assert!(six_output.contains("└──enter/y─accept──·──esc/n─cancel─────┘"));
        assert_eq!(
            workdeck_core::review_digest(format!("{six_output}\n\n").as_bytes()),
            "2b5937af0510829cfe1bb05a81f8b1147cbbf54aa45d24764623fc4a391b322e"
        );

        let (five_buffer, five) = render_specimen(7, None);
        let five_output = frame(&five_buffer, 80);
        assert!(!five.show_actions);
        assert!(!five.show_action_gap);
        assert!(five_output.contains("└─second─body──────────────────────────┘"));
        assert_eq!(
            workdeck_core::review_digest(format!("{five_output}\n\n").as_bytes()),
            "4d8adf81bd044ddd229928814ed05b94c3be4898230d243378067201d9a5df80"
        );
    }

    #[test]
    fn hover_changes_only_the_named_button_and_hit_testing_uses_padding() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let (buffer, map) = render_specimen(20, Some("enter/y"));
        let first = &map.action_hits[0];
        let second = &map.action_hits[1];
        assert_eq!(
            buffer[first.bounds.as_position()].bg,
            ratatui_theme_color(&theme.accent_muted)
        );
        assert_eq!(
            buffer[second.bounds.as_position()].bg,
            ratatui_theme_color(&theme.panel)
        );
        assert_eq!(dialog_action_at(&map, 22, 12).map(|hit| hit.index), Some(0));
        assert_eq!(dialog_action_at(&map, 37, 12).map(|hit| hit.index), Some(0));
        assert!(dialog_action_at(&map, 39, 12).is_none());
        assert_eq!(dialog_action_at(&map, 41, 12).map(|hit| hit.index), Some(1));
    }
}
