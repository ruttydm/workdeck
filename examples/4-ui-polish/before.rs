use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    widgets::{Block, Padding, Paragraph, Widget},
};

const BORDER: Color = Color::Rgb(0x33, 0x41, 0x55);
const BACKGROUND: Color = Color::Rgb(0x0f, 0x17, 0x2a);
const TITLE: Color = Color::Rgb(0xf8, 0xfa, 0xfc);
const NOTE: Color = Color::Rgb(0x94, 0xa3, 0xb8);
const CHANGES: Color = Color::Rgb(0x38, 0xbd, 0xf8);
const SYNCED: Color = Color::Rgb(0x64, 0x74, 0x8b);

#[derive(Clone, Copy)]
pub struct ChangeSummaryCardProps<'a> {
    pub title: &'a str,
    pub note: &'a str,
    pub changes: usize,
    pub last_synced: &'a str,
    pub on_open: fn(),
}

pub struct ChangeSummaryCard<'a> {
    props: ChangeSummaryCardProps<'a>,
}

impl<'a> ChangeSummaryCard<'a> {
    #[must_use]
    pub const fn new(props: ChangeSummaryCardProps<'a>) -> Self {
        Self { props }
    }

    pub fn open(&self) {
        (self.props.on_open)();
    }
}

impl Widget for &ChangeSummaryCard<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let block = Block::bordered()
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BACKGROUND))
            .padding(Padding::uniform(2));
        let inner = block.inner(area);
        block.render(area, buffer);

        render_row(buffer, inner, 0, self.props.title, TITLE);
        render_row(buffer, inner, 1, self.props.note, NOTE);
        render_metadata(
            buffer,
            inner,
            3,
            &format!("{} files changed", self.props.changes),
            &format!("Synced {}", self.props.last_synced),
        );
        render_row(buffer, inner, 4, "[ Open diff ]", TITLE);
    }
}

fn render_row(buffer: &mut Buffer, area: Rect, offset: u16, text: &str, color: Color) {
    if offset >= area.height {
        return;
    }
    Paragraph::new(text)
        .style(Style::default().fg(color))
        .render(Rect::new(area.x, area.y + offset, area.width, 1), buffer);
}

fn render_metadata(buffer: &mut Buffer, area: Rect, offset: u16, left: &str, right: &str) {
    if offset >= area.height {
        return;
    }
    let row = Rect::new(area.x, area.y + offset, area.width, 1);
    Paragraph::new(left)
        .style(Style::default().fg(CHANGES))
        .render(row, buffer);
    Paragraph::new(right)
        .alignment(Alignment::Right)
        .style(Style::default().fg(SYNCED))
        .render(row, buffer);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    static OPENED: AtomicBool = AtomicBool::new(false);

    fn open() {
        OPENED.store(true, Ordering::SeqCst);
    }

    #[test]
    fn renders_the_original_card_and_activates_open() {
        OPENED.store(false, Ordering::SeqCst);
        let area = Rect::new(0, 0, 64, 13);
        let mut buffer = Buffer::empty(area);
        let card = ChangeSummaryCard::new(ChangeSummaryCardProps {
            title: "Working tree",
            note: "Review local edits",
            changes: 4,
            last_synced: "just now",
            on_open: open,
        });

        (&card).render(area, &mut buffer);
        card.open();

        let contents = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(contents.contains("Working tree"));
        assert!(contents.contains("4 files changed"));
        assert!(contents.contains("Synced just now"));
        assert!(contents.contains("[ Open diff ]"));
        assert_eq!(buffer[(3, 3)].fg, TITLE);
        assert!(OPENED.load(Ordering::SeqCst));
    }
}
