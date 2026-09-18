//! MIT translation of Hunk's non-ASCII review-stream latency workload.

use super::*;
use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
use std::time::Instant;

const FILE_COUNT: usize = 120;
const LINES_PER_FILE: usize = 120;
const SCROLL_TICKS: usize = 8;

fn renderer() -> Result<large_stream::Renderer> {
    large_stream::Renderer::with_content(FILE_COUNT, LINES_PER_FILE, true)
}

fn first_frame() -> Result<f64> {
    let mut setup = renderer()?;
    let start = Instant::now();
    setup.render_pass(1);
    Ok(start.elapsed().as_secs_f64() * 1000.0)
}

fn scrolling() -> Result<(Vec<f64>, usize)> {
    let mut setup = renderer()?;
    setup.render_pass(2);
    let mut latencies = Vec::with_capacity(SCROLL_TICKS);
    for _ in 0..SCROLL_TICKS {
        let start = Instant::now();
        setup.app.handle_mouse_event(MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 170,
            row: 12,
            modifiers: KeyModifiers::NONE,
        });
        setup.render_pass(1);
        std::thread::yield_now();
        latencies.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    Ok((latencies, setup.app.review_scroll()))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark non-ascii-stream accepts no arguments");
    }
    println!(
        "METRIC non_ascii_cold_first_frame_ms={}",
        fixed(first_frame()?, 2)
    );
    let (latencies, _) = scrolling()?;
    for (suffix, percent) in [("median", 50.0), ("p95", 95.0)] {
        println!(
            "METRIC non_ascii_scroll_tick_{suffix}_ms={}",
            fixed(percentile(&latencies, percent), 2)
        );
    }
    println!("METRIC scroll_ticks={SCROLL_TICKS}");
    println!("METRIC files={FILE_COUNT}");
    println!("METRIC lines_per_file={LINES_PER_FILE}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workload_uses_non_ascii_content_at_the_source_scale() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-non-ascii-stream.json"
        ))
        .unwrap();
        for run in oracle["runs"].as_array().unwrap() {
            assert_eq!(run["exitCode"], 0);
            let metrics = runner::parse_metrics(run["combinedOutput"].as_str().unwrap());
            assert_eq!(metrics.len(), 6);
            for (name, expected) in [
                ("files", FILE_COUNT),
                ("lines_per_file", LINES_PER_FILE),
                ("scroll_ticks", SCROLL_TICKS),
            ] {
                assert_eq!(
                    metrics.iter().find(|(key, _)| key == name).unwrap().1,
                    expected as f64
                );
                assert_eq!(oracle["counts"][name], expected);
            }
        }
        let bootstrap = stream::large_bootstrap(
            std::env::current_dir().unwrap(),
            FILE_COUNT,
            LINES_PER_FILE,
            37,
            84,
            true,
        )
        .unwrap();
        assert_eq!(bootstrap.changeset.files.len(), 120);
        assert!(bootstrap.changeset.files.iter().all(|file| {
            [&file.sources.old, &file.sources.new]
                .into_iter()
                .all(|source| {
                    let content = &source.as_ref().unwrap().content;
                    content.lines().count() == LINES_PER_FILE
                        && content.contains("🚀")
                        && content.contains("┌")
                })
        }));
        assert_eq!(large_stream::VIEWPORT.width, 240);
        assert_eq!(large_stream::VIEWPORT.height, 28);
        assert_eq!(oracle["viewport"]["width"], large_stream::VIEWPORT.width);
        assert_eq!(oracle["viewport"]["height"], large_stream::VIEWPORT.height);
    }

    #[test]
    fn non_ascii_workload_drives_real_frames_and_eight_scroll_events() {
        let cold = first_frame().unwrap();
        assert!(cold.is_finite() && cold > 0.0);
        let (latencies, offset) = scrolling().unwrap();
        assert_eq!(latencies.len(), 8);
        assert!(
            latencies
                .iter()
                .all(|value| value.is_finite() && *value > 0.0)
        );
        assert!(
            offset > 0,
            "wheel events must move the actual review viewport"
        );
        assert!(run(["unexpected".into()].into_iter()).is_err());
    }
}
