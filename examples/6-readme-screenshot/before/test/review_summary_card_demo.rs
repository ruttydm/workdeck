use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::{buffer::Buffer, layout::Rect, widgets::Widget};

use crate::readme_screenshot_before::review_summary_card::{
    ChangeSummaryCard, ChangeSummaryCardProps,
};

static OPENED: AtomicBool = AtomicBool::new(false);

fn open() {
    OPENED.store(true, Ordering::SeqCst);
}

#[test]
fn keeps_the_old_sync_oriented_labels() {
    OPENED.store(false, Ordering::SeqCst);
    let area = Rect::new(0, 0, 64, 13);
    let mut buffer = Buffer::empty(area);
    let card = ChangeSummaryCard::new(ChangeSummaryCardProps {
        title: "Review summary",
        note: "Three changes",
        changes: 3,
        last_synced: "2m ago",
        on_open: open,
    });

    (&card).render(area, &mut buffer);
    card.open();

    let contents = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(contents.contains("[ Open diff ]"));
    assert!(contents.contains("Synced 2m ago"));
    assert!(OPENED.load(Ordering::SeqCst));
}
