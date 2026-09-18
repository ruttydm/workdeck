use std::fs;
use std::path::PathBuf;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Paragraph, Widget},
};
use workdeck_tui::{
    WorkdeckDiffFile, WorkdeckDiffLayout, WorkdeckDiffViewOptions, fit_text,
    render_workdeck_diff_view,
};

const TITLE: Color = Color::Rgb(0xd8, 0xb4, 0xfe);
const SUBTITLE: Color = Color::Rgb(0x8f, 0x9b, 0xb3);
const LABEL: Color = Color::Rgb(0x6b, 0x72, 0x80);
const ACTIVE_BACKGROUND: Color = Color::Rgb(0x45, 0x26, 0x50);
const ACTIVE_FOREGROUND: Color = Color::Rgb(0xff, 0xf0, 0xff);
const INACTIVE_BACKGROUND: Color = Color::Rgb(0x1f, 0x24, 0x30);

#[derive(Debug, Clone)]
pub struct ExampleProps {
    pub title: String,
    pub subtitle: String,
    pub diff: WorkdeckDiffFile,
    pub layout: WorkdeckDiffLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExampleRenderMap {
    pub split_button: Rect,
    pub stack_button: Rect,
}

#[derive(Debug, Clone)]
pub struct ExampleApp {
    props: ExampleProps,
    active_layout: WorkdeckDiffLayout,
    vertical_offset: usize,
}

impl ExampleApp {
    #[must_use]
    pub fn new(props: ExampleProps) -> Self {
        let active_layout = props.layout;
        Self {
            props,
            active_layout,
            vertical_offset: 0,
        }
    }

    #[must_use]
    pub const fn active_layout(&self) -> WorkdeckDiffLayout {
        self.active_layout
    }

    pub const fn set_layout(&mut self, layout: WorkdeckDiffLayout) {
        self.active_layout = layout;
    }

    pub const fn set_vertical_offset(&mut self, offset: usize) {
        self.vertical_offset = offset;
    }

    pub fn select_at(&mut self, column: u16, row: u16, map: ExampleRenderMap) -> bool {
        let next = if contains(map.split_button, column, row) {
            Some(WorkdeckDiffLayout::Split)
        } else if contains(map.stack_button, column, row) {
            Some(WorkdeckDiffLayout::Stack)
        } else {
            None
        };
        if let Some(layout) = next {
            self.set_layout(layout);
            true
        } else {
            false
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer) -> ExampleRenderMap {
        let content = Rect::new(
            area.x.saturating_add(1),
            area.y.saturating_add(1),
            area.width.saturating_sub(2),
            area.height.saturating_sub(2),
        );
        render_text(
            buffer,
            row(content, 0),
            &fit_text(&self.props.title, usize::from(content.width), None),
            TITLE,
        );
        render_text(
            buffer,
            row(content, 1),
            &fit_text(&self.props.subtitle, usize::from(content.width), None),
            SUBTITLE,
        );
        render_text(
            buffer,
            Rect::new(content.x, content.y + 2, 6, 1),
            "layout",
            LABEL,
        );

        let split_button = Rect::new(content.x.saturating_add(7), content.y + 2, 7, 1);
        let stack_button = Rect::new(content.x.saturating_add(15), content.y + 2, 7, 1);
        render_layout_button(
            buffer,
            split_button,
            "Split",
            self.active_layout == WorkdeckDiffLayout::Split,
        );
        render_layout_button(
            buffer,
            stack_button,
            "Stack",
            self.active_layout == WorkdeckDiffLayout::Stack,
        );

        let diff_area = Rect::new(
            content.x,
            content.y.saturating_add(4),
            content.width,
            content.height.saturating_sub(4),
        );
        render_workdeck_diff_view(
            diff_area,
            buffer,
            Some(&self.props.diff),
            &WorkdeckDiffViewOptions {
                vertical_offset: self.vertical_offset,
                body: workdeck_tui::WorkdeckDiffBodyOptions {
                    layout: self.active_layout,
                    theme: "midnight".into(),
                    ..workdeck_tui::WorkdeckDiffBodyOptions::default()
                },
                ..WorkdeckDiffViewOptions::default()
            },
        );

        ExampleRenderMap {
            split_button,
            stack_button,
        }
    }
}

pub fn read_example_file(name: &str) -> Result<String, std::io::Error> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("7-ratatui-component")
        .join(name);
    fs::read_to_string(path)
}

fn row(area: Rect, offset: u16) -> Rect {
    Rect::new(area.x, area.y.saturating_add(offset), area.width, 1)
}

fn render_text(buffer: &mut Buffer, area: Rect, text: &str, color: Color) {
    if area.width > 0 && area.height > 0 {
        Paragraph::new(text)
            .style(Style::default().fg(color))
            .render(area, buffer);
    }
}

fn render_layout_button(buffer: &mut Buffer, area: Rect, label: &str, active: bool) {
    let style = if active {
        Style::default().fg(ACTIVE_FOREGROUND).bg(ACTIVE_BACKGROUND)
    } else {
        Style::default().fg(SUBTITLE).bg(INACTIVE_BACKGROUND)
    };
    Paragraph::new(format!(" {label} "))
        .style(style)
        .render(area, buffer);
}

const fn contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}
