//! Partial MIT translation of Hunk benchmarks/interaction-latency.ts.
//! Not admitted to the suite: native retained-memory measurements and acceptance runs remain missing.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use std::time::Instant;

const NAVIGATION_PRESSES: usize = 6;
const SCROLL_TICKS: usize = 8;

#[test]
fn pinned_oracles_preserve_latency_memory_and_workload_metrics() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../port/hunk/oracles/benchmark-interaction-latency.json"
    ))
    .unwrap();
    assert_eq!(oracle["runs"].as_array().unwrap().len(), 2);
    for run in oracle["runs"].as_array().unwrap() {
        assert_eq!(run["exitCode"], 0);
        let metrics = runner::parse_metrics(run["combinedOutput"].as_str().unwrap());
        assert_eq!(metrics.len(), 13);
        for (key, count) in [
            ("navigation_presses", NAVIGATION_PRESSES),
            ("scroll_ticks", SCROLL_TICKS),
            ("files", stream::DEFAULT_FILE_COUNT),
            ("lines_per_file", stream::DEFAULT_LINES_PER_FILE),
        ] {
            assert_eq!(
                metrics.iter().find(|(name, _)| name == key).unwrap().1,
                count as f64
            );
        }
        for key in [
            "first_frame_ms",
            "hunk_nav_press_median_ms",
            "hunk_nav_press_p95_ms",
            "scroll_tick_median_ms",
            "scroll_tick_p95_ms",
            "after_first_frame_rss_bytes",
            "after_first_frame_heap_used_bytes",
            "after_navigation_rss_bytes",
            "after_navigation_heap_used_bytes",
        ] {
            assert!(metrics.iter().find(|(name, _)| name == key).unwrap().1 > 0.0);
        }
    }
}

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
