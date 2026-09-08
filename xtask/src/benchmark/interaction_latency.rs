//! Partial MIT translation of Hunk benchmarks/interaction-latency.ts.
//! Not admitted to the suite: retained-memory measurements and frozen oracles remain missing.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use std::time::Instant;

const NAVIGATION_PRESSES: usize = 6;
const SCROLL_TICKS: usize = 8;

fn renderer() -> Result<large_stream::Renderer> {
    large_stream::Renderer::with_content(
        stream::DEFAULT_FILE_COUNT,
        stream::DEFAULT_LINES_PER_FILE,
        false,
    )
}

#[test]
fn source_interaction_sequence_drives_real_navigation_and_fresh_scroll_state() {
    let mut navigation = renderer().unwrap();
    let start = Instant::now();
    navigation.render_pass(1);
    let first_frame_ms = start.elapsed().as_secs_f64() * 1000.0;
    assert!(first_frame_ms.is_finite() && first_frame_ms > 0.0);
    navigation.render_pass(2);
    let mut presses = Vec::new();
    for _ in 0..NAVIGATION_PRESSES {
        let before = navigation.app.shared_state().lock().unwrap().selection();
        let start = Instant::now();
        navigation
            .app
            .handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        navigation.render_pass(1);
        std::thread::yield_now();
        presses.push(start.elapsed().as_secs_f64() * 1000.0);
        let after = navigation.app.shared_state().lock().unwrap().selection();
        assert_ne!(
            before, after,
            "each navigation press must change the real selection"
        );
    }
    drop(navigation);
    let mut scrolling = renderer().unwrap();
    scrolling.render_pass(2);
    let initial_scroll = scrolling.app.review_scroll();
    let mut ticks = Vec::new();
    for _ in 0..SCROLL_TICKS {
        let start = Instant::now();
        scrolling.app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 170,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
        scrolling.render_pass(1);
        std::thread::yield_now();
        ticks.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    assert!(scrolling.app.review_scroll() > initial_scroll);
    assert_eq!((presses.len(), ticks.len()), (6, 8));
    assert_eq!(
        (stream::DEFAULT_FILE_COUNT, stream::DEFAULT_LINES_PER_FILE),
        (180, 120)
    );
    assert_eq!(
        (large_stream::VIEWPORT.width, large_stream::VIEWPORT.height),
        (240, 28)
    );
    for distribution in [presses, ticks] {
        assert!(
            distribution
                .iter()
                .all(|value| value.is_finite() && *value > 0.0)
        );
        assert!(percentile(&distribution, 95.0) >= percentile(&distribution, 50.0));
    }
}
