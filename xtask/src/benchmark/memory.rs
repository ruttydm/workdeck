//! Native MIT port of Hunk's benchmarks/memory.ts workload.
//!
//! The Rust process exposes RSS and, where available, allocator usage. It does
//! not invent JavaScript heap counters; the native fields remain explicit in
//! the emitted metric stream.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;
use workdeck_tui::{
    ReviewRenderPlanOptions, build_review_render_plan, build_split_rows, resolve_theme,
};

const FILES: usize = 120;
const LINES_PER_FILE: usize = 120;
const NAVIGATION_PRESSES: usize = 6;

fn snapshot_metrics(report: &serde_json::Value, key: &str, prefix: &str) {
    let Some(snapshot) = report.get(key).and_then(serde_json::Value::as_object) else {
        return;
    };
    if let Some(rss) = snapshot.get("rssBytes").and_then(serde_json::Value::as_u64) {
        println!("METRIC {prefix}_rss_bytes={rss}");
    }
    if let Some(allocator) = snapshot
        .get("mallocInUseBytes")
        .and_then(serde_json::Value::as_u64)
    {
        println!("METRIC {prefix}_allocator_bytes={allocator}");
    }
}

fn measure() -> Result<serde_json::Value> {
    let theme = resolve_theme(Some("midnight"), None, &[]);
    let started = Instant::now();
    let bootstrap = stream::large_bootstrap(
        std::env::current_dir()?,
        FILES,
        LINES_PER_FILE,
        37,
        84,
        false,
    )?;
    let bootstrap_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let after_bootstrap = native_memory::snapshot()?;

    let started = Instant::now();
    let planned_rows: usize = bootstrap
        .changeset
        .files
        .iter()
        .map(|file| {
            let rows = build_split_rows(file, None, &theme, 4);
            build_review_render_plan(ReviewRenderPlanOptions::new(&file.runtime_id, &rows, true))
                .len()
        })
        .sum();
    let planning_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let after_planning = native_memory::snapshot()?;

    let mut renderer = large_stream::Renderer::from_bootstrap(bootstrap);
    let started = Instant::now();
    renderer.render_pass(1);
    let first_frame_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let after_first_frame = native_memory::snapshot()?;

    renderer.render_pass(2);
    let started = Instant::now();
    for _ in 0..NAVIGATION_PRESSES {
        renderer
            .app
            .handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        renderer.render_pass(1);
    }
    let next_hunk_navigation_ms = started.elapsed().as_secs_f64() * 1_000.0;
    let after_navigation = native_memory::snapshot()?;
    Ok(serde_json::json!({
        "files": FILES,
        "linesPerFile": LINES_PER_FILE,
        "bootstrapFixtureMs": bootstrap_ms,
        "planningMs": planning_ms,
        "plannedRows": planned_rows,
        "firstFrameMs": first_frame_ms,
        "nextHunkNavigationMs": next_hunk_navigation_ms,
        "afterBootstrap": after_bootstrap,
        "afterPlanning": after_planning,
        "afterFirstFrame": after_first_frame,
        "afterNavigation": after_navigation,
        "viewport": {"width": 240, "height": 28},
        "navigationPresses": NAVIGATION_PRESSES,
        "memorySemantics": "Current process RSS and native allocator usage where exposed; JavaScript heapUsed is unavailable and is not fabricated."
    }))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark memory accepts no arguments");
    }
    let report = measure()?;
    println!(
        "METRIC bootstrap_fixture_ms={}",
        fixed(report["bootstrapFixtureMs"].as_f64().unwrap_or_default(), 2)
    );
    snapshot_metrics(&report, "afterBootstrap", "after_bootstrap");
    println!(
        "METRIC planning_ms={}",
        fixed(report["planningMs"].as_f64().unwrap_or_default(), 2)
    );
    println!(
        "METRIC planned_rows={}",
        report["plannedRows"].as_u64().unwrap_or_default()
    );
    snapshot_metrics(&report, "afterPlanning", "after_planning");
    println!(
        "METRIC first_frame_ms={}",
        fixed(report["firstFrameMs"].as_f64().unwrap_or_default(), 2)
    );
    snapshot_metrics(&report, "afterFirstFrame", "after_first_frame");
    println!(
        "METRIC next_hunk_navigation_ms={}",
        fixed(
            report["nextHunkNavigationMs"].as_f64().unwrap_or_default(),
            2
        )
    );
    snapshot_metrics(&report, "afterNavigation", "after_navigation");
    println!("METRIC files={FILES}");
    println!("METRIC lines_per_file={LINES_PER_FILE}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_memory_workload_drives_planning_rendering_and_navigation() {
        let report = measure().unwrap();
        assert_eq!(report["files"], FILES);
        assert_eq!(report["linesPerFile"], LINES_PER_FILE);
        assert_eq!(report["navigationPresses"], NAVIGATION_PRESSES);
        assert_eq!(
            report["viewport"],
            serde_json::json!({"width":240,"height":28})
        );
        assert!(report["plannedRows"].as_u64().unwrap() > 0);
        for key in [
            "bootstrapFixtureMs",
            "planningMs",
            "firstFrameMs",
            "nextHunkNavigationMs",
        ] {
            assert!(report[key].as_f64().unwrap().is_finite());
        }
        assert!(report["afterBootstrap"]["rssBytes"].as_u64().unwrap() > 0);
        assert!(report["afterPlanning"]["rssBytes"].as_u64().unwrap() > 0);
        assert!(report["afterFirstFrame"]["rssBytes"].as_u64().unwrap() > 0);
        assert!(report["afterNavigation"]["rssBytes"].as_u64().unwrap() > 0);
    }

    #[test]
    fn memory_benchmark_rejects_options() {
        assert!(run(["--unexpected".into()].into_iter()).is_err());
    }
}
