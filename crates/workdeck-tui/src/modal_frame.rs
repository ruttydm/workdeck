//! Reusable centered modal chrome and pointer routing.
//!
//! This is the native Ratatui counterpart of Hunk's
//! `src/ui/components/chrome/ModalFrame.tsx` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`. Callers render their own body
//! inside the returned content rectangle while this boundary owns centering,
//! frame paint, title fitting, the optional escape affordance, and modal mouse
//! propagation.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};

use crate::{
    AppTheme, fit_text, measure_text_width, pad_text, ratatui_theme_color, resolve_modal_geometry,
    slice_text_by_width, wrap_text,
};

pub const MODAL_BACKDROP_Z_INDEX: u16 = 55;
pub const MODAL_FRAME_Z_INDEX: u16 = 60;
pub const MODAL_CLOSE_TEXT: &str = "[Esc]";

#[derive(Debug, Clone, Copy)]
pub struct ModalFrameOptions<'a> {
    pub width: u16,
    pub height: u16,
    pub closeable: bool,
    pub has_mouse_scroll_handler: bool,
    pub terminal_width: u16,
    pub terminal_height: u16,
    pub theme: &'a AppTheme,
    pub title: &'a str,
}

/// Exact rectangles and text allocated by the reusable frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModalFramePlan {
    pub backdrop: Rect,
    pub frame: Rect,
    pub content: Rect,
    pub title: Rect,
    pub title_source_width: usize,
    pub title_rows: Vec<String>,
    pub close: Option<Rect>,
    pub close_text: Option<&'static str>,
    pub backdrop_z_index: u16,
    pub frame_z_index: u16,
    pub border_color: String,
    pub background: String,
    pub text_color: String,
    pub close_color: String,
    pub column: bool,
    pub frame_stops_mouse_up: bool,
    pub frame_stops_mouse_scroll: bool,
    pub forwards_mouse_scroll: bool,
}

fn empty_at(x: u16, y: u16) -> Rect {
    Rect::new(x, y, 0, 0)
}

/// Resolve all geometry relative to the supplied terminal rectangle.
#[must_use]
pub fn plan_modal_frame(origin: Rect, options: ModalFrameOptions<'_>) -> ModalFramePlan {
    let geometry = resolve_modal_geometry(
        options.width,
        options.height,
        options.terminal_width,
        options.terminal_height,
    );
    let frame = Rect::new(
        origin.x.saturating_add(geometry.left),
        origin.y.saturating_add(geometry.top),
        geometry.width,
        geometry.height,
    );
    let inner_x = frame.x.saturating_add(1);
    let inner_y = frame.y.saturating_add(1);
    let inner_width = frame.width.saturating_sub(2);
    let inner_height = frame.height.saturating_sub(2);
    let padded_x = inner_x.saturating_add(u16::from(inner_width > 0));
    let padded_width = inner_width.saturating_sub(2);
    let title_y = inner_y.saturating_add(u16::from(inner_height > 0));
    let close_width = if options.closeable {
        u16::try_from(MODAL_CLOSE_TEXT.len())
            .unwrap_or(u16::MAX)
            .min(padded_width)
    } else {
        0
    };
    let title_source_width = usize::from(
        frame
            .width
            .saturating_sub(2)
            .saturating_sub(if options.closeable {
                u16::try_from(MODAL_CLOSE_TEXT.len() + 1).unwrap_or(u16::MAX)
            } else {
                0
            })
            .max(1),
    );
    let fitted_title = pad_text(
        &fit_text(options.title, title_source_width, None),
        title_source_width,
    );
    let title_allocation = if options.closeable {
        let minimum_word_width = fitted_title
            .split_whitespace()
            .map(measure_text_width)
            .max()
            .unwrap_or(1);
        usize::from(padded_width.saturating_sub(close_width))
            .max(minimum_word_width)
            .min(title_source_width)
    } else {
        usize::from(padded_width.max(1))
    };
    let title_rows = if options.closeable {
        vec![slice_text_by_width(&fitted_title, 0, title_allocation).text]
    } else {
        wrap_text(&fitted_title, title_allocation)
    };
    let title_rows = if title_rows.is_empty() {
        vec![String::new()]
    } else {
        title_rows
    };
    let title_height = u16::try_from(title_rows.len())
        .unwrap_or(u16::MAX)
        .min(inner_height.saturating_sub(1));
    let title = Rect::new(
        padded_x,
        title_y,
        u16::try_from(title_allocation).unwrap_or(u16::MAX),
        title_height,
    );
    let close = options.closeable.then(|| {
        Rect::new(
            padded_x.saturating_add(u16::try_from(title_allocation).unwrap_or(u16::MAX)),
            title_y,
            close_width,
            u16::from(close_width > 0 && title_y < frame.bottom().saturating_sub(1)),
        )
    });
    // OpenTUI's growing content box leaves the source component's documented
    // blank spacer row between the title and its first child.
    let body_y = title_y.saturating_add(title_height).saturating_add(1);
    let body_bottom = frame.bottom().saturating_sub(2);
    let content = if body_y < body_bottom && padded_width > 0 {
        Rect::new(
            padded_x,
            body_y,
            padded_width,
            body_bottom.saturating_sub(body_y),
        )
    } else {
        empty_at(padded_x, body_y.min(frame.bottom()))
    };

    ModalFramePlan {
        backdrop: Rect::new(
            origin.x,
            origin.y,
            options.terminal_width,
            options.terminal_height,
        ),
        frame,
        content,
        title,
        title_source_width,
        title_rows,
        close,
        close_text: options.closeable.then_some(MODAL_CLOSE_TEXT),
        backdrop_z_index: MODAL_BACKDROP_Z_INDEX,
        frame_z_index: MODAL_FRAME_Z_INDEX,
        border_color: options.theme.accent.clone(),
        background: options.theme.panel.clone(),
        text_color: options.theme.text.clone(),
        close_color: options.theme.badge_neutral.clone(),
        column: true,
        frame_stops_mouse_up: true,
        frame_stops_mouse_scroll: true,
        forwards_mouse_scroll: options.has_mouse_scroll_handler,
    }
}

/// Paint modal chrome and return the body/hit-test geometry to the caller.
pub fn render_modal_frame(
    origin: Rect,
    buffer: &mut Buffer,
    options: ModalFrameOptions<'_>,
) -> ModalFramePlan {
    let plan = plan_modal_frame(origin, options);
    Clear.render(plan.frame, buffer);
    let block = Block::default()
        .borders(Borders::ALL)
        .style(Style::default().bg(ratatui_theme_color(&plan.background)))
        .border_style(Style::default().fg(ratatui_theme_color(&plan.border_color)));
    block.render(plan.frame, buffer);

    let title_style = Style::default()
        .fg(ratatui_theme_color(&plan.text_color))
        .bg(ratatui_theme_color(&plan.background));
    if plan.title.width > 0 && plan.title.height > 0 {
        Paragraph::new(
            plan.title_rows
                .iter()
                .take(usize::from(plan.title.height))
                .cloned()
                .map(|row| Line::styled(row, title_style))
                .collect::<Vec<_>>(),
        )
        .render(plan.title, buffer);
    }
    if let (Some(close), Some(close_text)) = (plan.close, plan.close_text)
        && close.width > 0
        && close.height > 0
    {
        Paragraph::new(Line::from(Span::styled(
            slice_text_by_width(close_text, 0, usize::from(close.width)).text,
            Style::default()
                .fg(ratatui_theme_color(&plan.close_color))
                .bg(ratatui_theme_color(&plan.background)),
        )))
        .render(close, buffer);
    }
    plan
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalFrameMouseTarget {
    Backdrop,
    Frame,
    Close,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModalFrameMouseEffect {
    pub stop_propagation: bool,
    pub close: bool,
    pub forward_scroll: bool,
}

/// Route mouse-up exactly like the backdrop, frame, and close affordance.
#[must_use]
pub const fn modal_frame_mouse_up(
    target: ModalFrameMouseTarget,
    closeable: bool,
) -> ModalFrameMouseEffect {
    match target {
        ModalFrameMouseTarget::Backdrop => ModalFrameMouseEffect {
            stop_propagation: false,
            close: closeable,
            forward_scroll: false,
        },
        ModalFrameMouseTarget::Frame => ModalFrameMouseEffect {
            stop_propagation: true,
            close: false,
            forward_scroll: false,
        },
        ModalFrameMouseTarget::Close => ModalFrameMouseEffect {
            stop_propagation: true,
            close: closeable,
            forward_scroll: false,
        },
    }
}

/// A frame-owned wheel event always stops at the modal and optionally reaches its child handler.
#[must_use]
pub const fn modal_frame_mouse_scroll(has_handler: bool) -> ModalFrameMouseEffect {
    ModalFrameMouseEffect {
        stop_propagation: true,
        close: false,
        forward_scroll: has_handler,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve_theme;

    fn symbols(buffer: &Buffer, area: Rect) -> String {
        buffer
            .content()
            .chunks(usize::from(area.width))
            .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn closeable_frame_matches_the_frozen_opentui_chrome() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let area = Rect::new(0, 0, 30, 11);
        let mut buffer = Buffer::empty(area);
        let plan = render_modal_frame(
            area,
            &mut buffer,
            ModalFrameOptions {
                width: 20,
                height: 7,
                closeable: true,
                has_mouse_scroll_handler: false,
                terminal_width: 30,
                terminal_height: 11,
                theme: &theme,
                title: "A very long modal title",
            },
        );
        assert_eq!(plan.frame, Rect::new(5, 2, 20, 7));
        assert_eq!(plan.backdrop, area);
        assert_eq!(plan.title_source_width, 12);
        assert_eq!(plan.title_rows, ["A very long"]);
        assert_eq!(plan.title, Rect::new(7, 4, 11, 1));
        assert_eq!(plan.close, Some(Rect::new(18, 4, 5, 1)));
        assert_eq!(plan.content, Rect::new(7, 6, 16, 1));
        let frame = symbols(&buffer, area);
        let lines = frame.lines().collect::<Vec<_>>();
        assert_eq!(lines[2], "     ┌──────────────────┐     ");
        assert_eq!(lines[3], "     │                  │     ");
        assert_eq!(lines[4], "     │ A very long[Esc] │     ");
        assert_eq!(lines[8], "     └──────────────────┘     ");
    }

    #[test]
    fn fixed_frame_wraps_the_fitted_title_and_clamps_to_real_terminal_margins() {
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let fixed = plan_modal_frame(
            Rect::new(0, 0, 30, 11),
            ModalFrameOptions {
                width: 20,
                height: 7,
                closeable: false,
                has_mouse_scroll_handler: true,
                terminal_width: 30,
                terminal_height: 11,
                theme: &theme,
                title: "A very long modal title",
            },
        );
        assert_eq!(fixed.title_source_width, 18);
        assert_eq!(fixed.title_rows, ["A very long", "modal."]);
        assert_eq!(fixed.title, Rect::new(7, 4, 16, 2));
        assert_eq!(fixed.content.height, 0);
        assert!(fixed.close.is_none());
        assert!(fixed.forwards_mouse_scroll);

        let clamped = plan_modal_frame(
            Rect::new(10, 20, 12, 6),
            ModalFrameOptions {
                width: 40,
                height: 20,
                closeable: true,
                has_mouse_scroll_handler: false,
                terminal_width: 12,
                terminal_height: 6,
                theme: &theme,
                title: "A very long modal title",
            },
        );
        assert_eq!(clamped.frame, Rect::new(11, 21, 10, 4));
        assert_eq!(clamped.title_source_width, 2);
        assert_eq!(clamped.title_rows, ["A."]);
        assert_eq!(clamped.title, Rect::new(13, 23, 2, 1));
        assert_eq!(clamped.close, Some(Rect::new(15, 23, 5, 1)));
        assert_eq!(clamped.content.height, 0);
    }

    #[test]
    fn pointer_routing_closes_only_from_the_backdrop_or_escape_affordance() {
        assert_eq!(
            modal_frame_mouse_up(ModalFrameMouseTarget::Backdrop, true),
            ModalFrameMouseEffect {
                stop_propagation: false,
                close: true,
                forward_scroll: false,
            }
        );
        assert_eq!(
            modal_frame_mouse_up(ModalFrameMouseTarget::Frame, true),
            ModalFrameMouseEffect {
                stop_propagation: true,
                close: false,
                forward_scroll: false,
            }
        );
        assert_eq!(
            modal_frame_mouse_up(ModalFrameMouseTarget::Close, false),
            ModalFrameMouseEffect {
                stop_propagation: true,
                close: false,
                forward_scroll: false,
            }
        );
        assert_eq!(
            modal_frame_mouse_scroll(true),
            ModalFrameMouseEffect {
                stop_propagation: true,
                close: false,
                forward_scroll: true,
            }
        );
    }
}
