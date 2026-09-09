//! Partial MIT port of Hunk huge-stream.ts. Native diagnostic, not heap parity.
use super::{large_stream::Renderer, native_memory, stream};
use anyhow::{Result, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use std::time::Instant;

fn measure(bootstrap: workdeck_core::AppBootstrap) -> Result<serde_json::Value> {
    let files = bootstrap.changeset.files.len();
    let mut setup = Renderer::from_bootstrap(bootstrap);
    let start = Instant::now();
    setup.render_pass(1);
    let first_frame = start.elapsed().as_secs_f64() * 1000.0;
    let first_memory = native_memory::snapshot()?;
    setup.render_pass(2);
    let mut scroll = Vec::new();
    let before_scroll = setup.app.review_scroll();
    for _ in 0..6 {
        let start = Instant::now();
        setup.app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 170,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
        setup.render_pass(1);
        std::thread::yield_now();
        scroll.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    ensure!(
        setup.app.review_scroll() > before_scroll,
        "huge workload did not scroll"
    );
    let mut navigation = Vec::new();
    for _ in 0..4 {
        let before = setup.app.shared_state().lock().unwrap().selection();
        let start = Instant::now();
        setup
            .app
            .handle_key(KeyEvent::new(KeyCode::Char(']'), KeyModifiers::NONE));
        setup.render_pass(1);
        std::thread::yield_now();
        navigation.push(start.elapsed().as_secs_f64() * 1000.0);
        ensure!(
            setup.app.shared_state().lock().unwrap().selection() != before,
            "huge workload navigation did not change selection"
        );
    }
    let after_navigation = native_memory::snapshot()?;
    Ok(serde_json::json!({
        "diagnosticOnly":true,"files":files,"firstFrameMs":first_frame,
        "scrollTickMs":scroll,"navigationPressMs":navigation,
        "afterFirstFrame":first_memory,"afterNavigation":after_navigation,
        "sequence":"one renderer: first frame, two settle frames, six scroll ticks, four navigation presses",
        "memorySemantics":"Current native RSS and available malloc usage; not peak RSS or JavaScript heapUsed"
    }))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    ensure!(
        args.next().is_none(),
        "huge-stream-diagnostic accepts no arguments"
    );
    native_memory::snapshot()?;
    let start = Instant::now();
    let bootstrap = stream::huge_bootstrap(std::env::current_dir()?)?;
    let fixture_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut report = measure(bootstrap)?;
    report["peakProcessRssBytes"] = serde_json::json!(native_memory::peak_rss_bytes()?);
    report["peakMemorySemantics"] = serde_json::json!(
        "Process lifetime peak resident/working-set bytes, including fixture construction; not JavaScript heapUsed"
    );
    report["fixtureBuildMs"] = serde_json::json!(fixture_ms);
    report["linesPerFile"] = serde_json::json!(stream::HUGE_LINES_PER_FILE);
    report["giantFileLines"] = serde_json::json!(stream::GIANT_SINGLE_FILE_LINES);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[test]
fn both_pinned_huge_runs_preserve_the_full_workload_and_metric_set() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../port/hunk/oracles/benchmark-huge-stream.json"
    ))
    .unwrap();
    let runs = oracle["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    for run in runs {
        assert_eq!(run["exitCode"], 0);
        let metrics = super::runner::parse_metrics(run["combinedOutput"].as_str().unwrap());
        assert_eq!(metrics.len(), 15);
        for (name, value) in &metrics {
            assert!(value.is_finite() && *value > 0.0, "invalid metric {name}");
        }
        for (name, count) in [
            ("files", stream::HUGE_FILE_COUNT + 1),
            ("lines_per_file", stream::HUGE_LINES_PER_FILE),
            ("giant_file_lines", stream::GIANT_SINGLE_FILE_LINES),
            ("navigation_presses", 4),
            ("scroll_ticks", 6),
        ] {
            assert_eq!(
                metrics.iter().find(|(key, _)| key == name).unwrap().1,
                count as f64
            );
        }
    }
}

#[test]
fn source_process_peak_evidence_distinguishes_resident_peak_from_footprint() {
    let oracle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../port/hunk/oracles/benchmark-huge-stream-process-peak.json"
    ))
    .unwrap();
    let runs = oracle["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 2);
    for run in runs {
        assert_eq!(run["exitCode"], 0);
        let output = run["combinedOutput"].as_str().unwrap();
        let read = |label: &str| {
            let values = output
                .lines()
                .filter_map(|line| line.trim().strip_suffix(label))
                .map(|value| value.trim().parse::<u64>().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(values.len(), 1);
            values[0]
        };
        let peak = read("maximum resident set size");
        assert!(peak > 0);
        assert_eq!(run["peakProcessRssBytes"], peak);
        assert_ne!(peak, read("peak memory footprint"));
        let metrics = super::runner::parse_metrics(output);
        assert_eq!(metrics.len(), 15);
        assert!(
            metrics
                .iter()
                .filter(|(name, _)| name.ends_with("_rss_bytes"))
                .all(|(_, value)| *value <= peak as f64)
        );
    }
}

#[test]
fn huge_interaction_sequence_executes_on_a_small_fixture() {
    let fixture =
        stream::large_bootstrap(std::env::current_dir().unwrap(), 6, 120, 37, 84, false).unwrap();
    let report = measure(fixture).unwrap();
    assert_eq!(report["files"], 6);
    assert_eq!(report["scrollTickMs"].as_array().unwrap().len(), 6);
    assert_eq!(report["navigationPressMs"].as_array().unwrap().len(), 4);
    assert!(report["firstFrameMs"].as_f64().unwrap() > 0.0);
    assert!(run(["unexpected".into()].into_iter()).is_err());
}
