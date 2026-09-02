use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Rect},
    style::{Color, Style},
    widgets::{Block, Padding, Paragraph, Widget},
};

const BORDER: Color = Color::Rgb(0x33, 0x41, 0x55);
const BACKGROUND: Color = Color::Rgb(0x0f, 0x17, 0x2a);
const HEADING: Color = Color::Rgb(0xf8, 0xfa, 0xfc);
const SUPPORTING_TEXT: Color = Color::Rgb(0x94, 0xa3, 0xb8);
const FILE_COUNT: Color = Color::Rgb(0x38, 0xbd, 0xf8);
const UPDATED: Color = Color::Rgb(0x64, 0x74, 0x8b);

#[derive(Clone, Copy)]
pub struct ReviewSummaryCardProps<'a> {
    pub heading: &'a str,
    pub supporting_text: &'a str,
    pub file_count: usize,
    pub last_updated: &'a str,
    pub on_review: fn(),
}

pub struct ReviewSummaryCard<'a> {
    props: ReviewSummaryCardProps<'a>,
}

impl<'a> ReviewSummaryCard<'a> {
    #[must_use]
    pub const fn new(props: ReviewSummaryCardProps<'a>) -> Self {
        Self { props }
    }

    pub fn review(&self) {
        (self.props.on_review)();
    }
}

#[must_use]
pub fn review_button_label(file_count: usize) -> String {
    if file_count == 1 {
        "Review 1 file".to_owned()
    } else {
        format!("Review {file_count} files")
    }
}

impl Widget for &ReviewSummaryCard<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let block = Block::bordered()
            .border_style(Style::default().fg(BORDER))
            .style(Style::default().bg(BACKGROUND))
            .padding(Padding::uniform(2));
        let inner = block.inner(area);
        block.render(area, buffer);

        render_row(buffer, inner, 0, self.props.heading, HEADING);
        render_row(
            buffer,
            inner,
            2,
            self.props.supporting_text,
            SUPPORTING_TEXT,
        );
        render_metadata(
            buffer,
            inner,
            4,
            &format!("{} files ready for review", self.props.file_count),
            &format!("Updated {}", self.props.last_updated),
        );
        render_row(
            buffer,
            inner,
            6,
            &format!("[ {} ]", review_button_label(self.props.file_count)),
            HEADING,
        );
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
        .style(Style::default().fg(FILE_COUNT))
        .render(row, buffer);
    Paragraph::new(right)
        .alignment(Alignment::Right)
        .style(Style::default().fg(UPDATED))
        .render(row, buffer);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    static REVIEWED: AtomicBool = AtomicBool::new(false);

    fn review() {
        REVIEWED.store(true, Ordering::SeqCst);
    }

    #[test]
    fn renders_the_polished_card_and_activates_review() {
        REVIEWED.store(false, Ordering::SeqCst);
        let area = Rect::new(0, 0, 64, 13);
        let mut buffer = Buffer::empty(area);
        let card = ReviewSummaryCard::new(ReviewSummaryCardProps {
            heading: "Working tree",
            supporting_text: "Review local edits",
            file_count: 4,
            last_updated: "just now",
            on_review: review,
        });

        (&card).render(area, &mut buffer);
        card.review();

        let contents = buffer
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(contents.contains("Working tree"));
        assert!(contents.contains("4 files ready for review"));
        assert!(contents.contains("Updated just now"));
        assert!(contents.contains("[ Review 4 files ]"));
        assert_eq!(buffer[(3, 3)].fg, HEADING);
        assert_eq!(review_button_label(1), "Review 1 file");
        assert!(REVIEWED.load(Ordering::SeqCst));
    }
}
