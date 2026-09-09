//! Partial MIT translation of Hunk benchmarks/interaction-latency.ts.
//! Diagnostic only: cross-runtime heap semantics and acceptance runs remain incomplete.

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

fn measure(memory: bool) -> Result<serde_json::Value> {
    let mut navigation = renderer()?;
    let start = Instant::now();
    navigation.render_pass(1);
    let first_frame_ms = start.elapsed().as_secs_f64() * 1000.0;
    let first_memory = memory.then(native_memory::snapshot).transpose()?;
    navigation.render_pass(2);
    let mut presses = Vec::new();
    let mut navigation_dispatch = Vec::new();
    let mut navigation_render = Vec::new();
    for _ in 0..NAVIGATION_PRESSES {
        let before = navigation.app.shared_state().lock().unwrap().selection();
        let start = Instant::now();
        navigation
            .app
            .handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        let dispatched = Instant::now();
        navigation.render_pass(1);
        let rendered = Instant::now();
        std::thread::yield_now();
        presses.push(start.elapsed().as_secs_f64() * 1000.0);
        navigation_dispatch.push(dispatched.duration_since(start).as_secs_f64() * 1000.0);
        navigation_render.push(rendered.duration_since(dispatched).as_secs_f64() * 1000.0);
        let after = navigation.app.shared_state().lock().unwrap().selection();
        anyhow::ensure!(
            before != after,
            "each navigation press must change the real selection"
        );
    }
    let navigation_memory = memory.then(native_memory::snapshot).transpose()?;
    drop(navigation);
    let mut scrolling = renderer()?;
    scrolling.render_pass(2);
    let initial_scroll = scrolling.app.review_scroll();
    let mut ticks = Vec::new();
    let mut scroll_dispatch = Vec::new();
    let mut scroll_render = Vec::new();
    for _ in 0..SCROLL_TICKS {
        let start = Instant::now();
        scrolling.app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 170,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
        let dispatched = Instant::now();
        scrolling.render_pass(1);
        let rendered = Instant::now();
        std::thread::yield_now();
        ticks.push(start.elapsed().as_secs_f64() * 1000.0);
        scroll_dispatch.push(dispatched.duration_since(start).as_secs_f64() * 1000.0);
        scroll_render.push(rendered.duration_since(dispatched).as_secs_f64() * 1000.0);
    }
    anyhow::ensure!(
        scrolling.app.review_scroll() > initial_scroll,
        "wheel ticks did not scroll"
    );
    for distribution in [&presses, &ticks] {
        anyhow::ensure!(
            distribution
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            "invalid timing sample"
        );
    }
    Ok(serde_json::json!({
        "diagnosticOnly": true,
        "firstFrameMs": first_frame_ms,
        "navigationPressMs": presses,
        "scrollTickMs": ticks,
        "navigationDispatchMs": navigation_dispatch,
        "navigationRenderMs": navigation_render,
        "scrollDispatchMs": scroll_dispatch,
        "scrollRenderMs": scroll_render,
        "stageSemantics": "Dispatch includes input handling and its geometry/events; render includes the complete frame pass. Total includes both stages and the scheduler yield. Two additional clock reads per interaction instrument the boundaries.",
        "afterFirstFrame": first_memory,
        "afterNavigation": navigation_memory,
        "peakProcessRssBytes": memory.then(native_memory::peak_rss_bytes).transpose()?,
        "peakMemorySemantics": "Process lifetime peak resident/working-set bytes, including both renderer fixtures; not a per-stage peak or JavaScript heapUsed",
        "files": stream::DEFAULT_FILE_COUNT,
        "linesPerFile": stream::DEFAULT_LINES_PER_FILE,
        "viewport": {"width": large_stream::VIEWPORT.width, "height": large_stream::VIEWPORT.height},
        "memorySemantics": "Current RSS and, where available, native malloc-zone usage (null when unavailable), not peak RSS or JavaScript heapUsed"
    }))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark interaction-diagnostic accepts no arguments");
    }
    native_memory::snapshot()?; // Fail unsupported backends before constructing the workload.
    println!("{}", serde_json::to_string_pretty(&measure(true)?)?);
    Ok(())
}

#[test]
fn source_interaction_sequence_drives_real_navigation_and_fresh_scroll_state() {
    let report = measure(cfg!(any(target_os = "macos", target_os = "linux", windows))).unwrap();
    assert_eq!(report["navigationPressMs"].as_array().unwrap().len(), 6);
    assert_eq!(report["scrollTickMs"].as_array().unwrap().len(), 8);
    for (total, dispatch, render) in [
        (
            "navigationPressMs",
            "navigationDispatchMs",
            "navigationRenderMs",
        ),
        ("scrollTickMs", "scrollDispatchMs", "scrollRenderMs"),
    ] {
        let totals = report[total].as_array().unwrap();
        let dispatches = report[dispatch].as_array().unwrap();
        let renders = report[render].as_array().unwrap();
        assert_eq!(totals.len(), dispatches.len());
        assert_eq!(totals.len(), renders.len());
        for ((total, dispatch), render) in totals.iter().zip(dispatches).zip(renders) {
            let dispatch = dispatch.as_f64().unwrap();
            let render = render.as_f64().unwrap();
            assert!(dispatch.is_finite() && dispatch >= 0.0);
            assert!(render.is_finite() && render >= 0.0);
            assert!(dispatch + render <= total.as_f64().unwrap() + 1e-9);
        }
    }
    assert_eq!(report["files"], 180);
    assert_eq!(report["linesPerFile"], 120);
    assert_eq!(
        report["viewport"],
        serde_json::json!({"width":240,"height":28})
    );
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    assert!(report["peakProcessRssBytes"].as_u64().unwrap() > 0);
    #[cfg(any(target_os = "macos", target_os = "linux", windows))]
    for key in ["afterFirstFrame", "afterNavigation"] {
        assert!(report[key]["rssBytes"].as_u64().unwrap() > 0);
        #[cfg(target_os = "macos")]
        assert!(report[key]["mallocInUseBytes"].as_u64().unwrap() > 0);
        #[cfg(not(target_os = "macos"))]
        assert!(report[key]["mallocInUseBytes"].is_null());
    }
    assert!(run(["unexpected".into()].into_iter()).is_err());
}
