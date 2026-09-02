use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::readme_screenshot_after::{
    review_copy::{review_button_label, review_timestamp_label},
    review_summary_card::{ReviewSummaryCard, ReviewSummaryCardProps},
};

static REVIEWED: AtomicBool = AtomicBool::new(false);

fn review() {
    REVIEWED.store(true, Ordering::SeqCst);
}

#[test]
fn switches_the_card_copy_to_review_oriented_labels() {
    REVIEWED.store(false, Ordering::SeqCst);
    let area = Rect::new(0, 0, 64, 13);
    let mut buffer = Buffer::empty(area);
    let card = ReviewSummaryCard::new(ReviewSummaryCardProps {
        heading: "Review summary",
        supporting_text: "Three changes",
        file_count: 3,
        last_updated: "2m ago",
        on_review: review,
    });

    (&card).render(area, &mut buffer);
    card.review();

    let contents = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(contents.contains("[ Review 3 files ]"));
    assert!(contents.contains("Updated 2m ago"));
    assert_eq!(review_button_label(3), "Review 3 files");
    assert_eq!(review_timestamp_label("2m ago"), "Updated 2m ago");
    assert!(REVIEWED.load(Ordering::SeqCst));
}
