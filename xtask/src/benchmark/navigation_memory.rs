//! Native MIT translation of Hunk's `benchmarks/navigation-memory.ts` workload.
//!
//! The benchmark keeps one Ratatui review mounted while repeatedly dispatching the same
//! keyboard navigation that a user sends in the reviewer. Rust does not have Bun's managed
//! JavaScript heap counters, so compatible `heap*` fields are populated only when the host
//! exposes native allocator usage; RSS remains available on supported CI platforms.

use super::*;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use serde::Serialize;
use std::time::Instant;

const DEFAULT_NAVIGATIONS: usize = 180;
const DEFAULT_WARMUP_NAVIGATIONS: usize = 60;
const DEFAULT_SAMPLE_EVERY: usize = 10;
const DEFAULT_FILE_COUNT: usize = 90;
const DEFAULT_LINES_PER_FILE: usize = 120;
const DEFAULT_WIDTH: usize = 240;
const DEFAULT_HEIGHT: usize = 28;
const DEFAULT_MAX_HEAP_GROWTH_MB: f64 = 192.0;
const DEFAULT_MAX_HEAP_SLOPE_KB: f64 = 2048.0;
const DEFAULT_MAX_RSS_GROWTH_MB: f64 = 384.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
enum NavigationMode {
    Bounce,
    Forward,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    navigations: usize,
    warmup_navigations: usize,
    sample_every: usize,
    file_count: usize,
    lines_per_file: usize,
    width: usize,
    height: usize,
    gc: bool,
    mode: NavigationMode,
    max_heap_growth_mb: f64,
    max_heap_slope_kb: f64,
    max_rss_growth_mb: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_out: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            navigations: DEFAULT_NAVIGATIONS,
            warmup_navigations: DEFAULT_WARMUP_NAVIGATIONS,
            sample_every: DEFAULT_SAMPLE_EVERY,
            file_count: DEFAULT_FILE_COUNT,
            lines_per_file: DEFAULT_LINES_PER_FILE,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            gc: true,
            mode: NavigationMode::Bounce,
            max_heap_growth_mb: DEFAULT_MAX_HEAP_GROWTH_MB,
            max_heap_slope_kb: DEFAULT_MAX_HEAP_SLOPE_KB,
            max_rss_growth_mb: DEFAULT_MAX_RSS_GROWTH_MB,
            json_out: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemorySample {
    label: String,
    navigation: usize,
    rss_bytes: u64,
    /// Native allocator usage where the platform exposes it (macOS currently).
    heap_used_bytes: Option<u64>,
    heap_total_bytes: Option<u64>,
    external_bytes: Option<u64>,
    array_buffers_bytes: Option<u64>,
}

fn parse_non_negative(name: &str, value: Option<String>) -> Result<f64> {
    let value = value.ok_or_else(|| anyhow::anyhow!("Missing value for {name}."))?;
    let parsed = sample_number(&value);
    if !parsed.is_finite() || parsed < 0.0 {
        bail!("Expected {name} to be a non-negative number.");
    }
    Ok(parsed)
}

fn truncate_size(name: &str, value: f64) -> Result<usize> {
    let value = value.trunc();
    if value >= usize::MAX as f64 {
        bail!("{name} exceeds the native addressable size.");
    }
    Ok(value as usize)
}

fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Option<Options>> {
    let mut options = Options::default();
    let mut navigations = options.navigations as f64;
    let mut warmup_navigations = options.warmup_navigations as f64;
    let mut sample_every = options.sample_every as f64;
    let mut file_count = options.file_count as f64;
    let mut lines_per_file = options.lines_per_file as f64;
    let mut width = options.width as f64;
    let mut height = options.height as f64;
    let mut max_heap_growth_mb = options.max_heap_growth_mb;
    let mut max_heap_slope_kb = options.max_heap_slope_kb;
    let mut max_rss_growth_mb = options.max_rss_growth_mb;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--no-gc" => options.gc = false,
            "--navigations" => navigations = parse_non_negative(&arg, args.next())?,
            "--warmup-navigations" => {
                warmup_navigations = parse_non_negative(&arg, args.next())?;
            }
            "--sample-every" => sample_every = parse_non_negative(&arg, args.next())?,
            "--file-count" => file_count = parse_non_negative(&arg, args.next())?,
            "--lines-per-file" => lines_per_file = parse_non_negative(&arg, args.next())?,
            "--width" => width = parse_non_negative(&arg, args.next())?,
            "--height" => height = parse_non_negative(&arg, args.next())?,
            "--mode" => {
                let mode = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing value for --mode."))?;
                options.mode = match mode.as_str() {
                    "bounce" => NavigationMode::Bounce,
                    "forward" => NavigationMode::Forward,
                    _ => bail!("Expected --mode to be either bounce or forward."),
                };
            }
            "--max-heap-growth-mb" => {
                max_heap_growth_mb = parse_non_negative(&arg, args.next())?;
            }
            "--max-heap-slope-kb" => {
                max_heap_slope_kb = parse_non_negative(&arg, args.next())?;
            }
            "--max-rss-growth-mb" => {
                max_rss_growth_mb = parse_non_negative(&arg, args.next())?;
            }
            "--json-out" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing value for --json-out."))?;
                if value.is_empty() {
                    bail!("Missing value for --json-out.");
                }
                options.json_out = Some(value);
            }
            _ => bail!("Unknown option: {arg}"),
        }
    }

    options.navigations = truncate_size("--navigations", navigations)?;
    options.warmup_navigations = truncate_size("--warmup-navigations", warmup_navigations)?
        .min(options.navigations.saturating_sub(1));
    options.sample_every = truncate_size("--sample-every", sample_every)?.max(1);
    options.file_count = truncate_size("--file-count", file_count)?.max(1);
    options.lines_per_file = truncate_size("--lines-per-file", lines_per_file)?.max(1);
    options.width = truncate_size("--width", width)?.max(40);
    options.height = truncate_size("--height", height)?.max(10);
    options.max_heap_growth_mb = max_heap_growth_mb;
    options.max_heap_slope_kb = max_heap_slope_kb;
    options.max_rss_growth_mb = max_rss_growth_mb;
    Ok(Some(options))
}

fn sample_memory(label: &str, navigation: usize) -> Result<MemorySample> {
    let snapshot = native_memory::snapshot()?;
    Ok(MemorySample {
        label: label.to_owned(),
        navigation,
        rss_bytes: snapshot.rss_bytes,
        heap_used_bytes: snapshot.malloc_in_use_bytes,
        heap_total_bytes: None,
        external_bytes: None,
        array_buffers_bytes: None,
    })
}

fn next_navigation_key(
    position: usize,
    direction: isize,
    options: &Options,
) -> (char, usize, isize) {
    if options.mode == NavigationMode::Forward {
        return (
            ']',
            position
                .saturating_add(1)
                .min(options.file_count.saturating_sub(1)),
            1,
        );
    }

    let next_direction = if position >= options.file_count.saturating_sub(1) {
        -1
    } else if position == 0 {
        1
    } else {
        direction
    };
    let next_position = if next_direction > 0 {
        position.saturating_add(1)
    } else {
        position.saturating_sub(1)
    }
    .min(options.file_count.saturating_sub(1));
    (
        if next_direction > 0 { ']' } else { '[' },
        next_position,
        next_direction,
    )
}

fn linear_slope(
    samples: &[&MemorySample],
    field: impl Fn(&MemorySample) -> Option<u64>,
) -> Option<f64> {
    let values = samples
        .iter()
        .filter_map(|sample| field(sample).map(|value| (sample.navigation as f64, value as f64)))
        .collect::<Vec<_>>();
    if values.len() < 2 {
        return values.first().map(|_| 0.0);
    }
    let mean_x = values.iter().map(|(x, _)| *x).sum::<f64>() / values.len() as f64;
    let mean_y = values.iter().map(|(_, y)| *y).sum::<f64>() / values.len() as f64;
    let numerator = values
        .iter()
        .map(|(x, y)| (x - mean_x) * (y - mean_y))
        .sum::<f64>();
    let denominator = values
        .iter()
        .map(|(x, _)| (x - mean_x).powi(2))
        .sum::<f64>();
    Some(if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    })
}

fn growth(samples: &[&MemorySample], field: impl Fn(&MemorySample) -> Option<u64>) -> Option<i128> {
    let first = samples.first().and_then(|sample| field(sample))?;
    let last = samples.last().and_then(|sample| field(sample))?;
    Some(i128::from(last) - i128::from(first))
}

fn format_bytes(bytes: i128) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn measure(options: &Options, memory: bool) -> Result<serde_json::Value> {
    ensure!(
        options.width <= u16::MAX as usize,
        "--width exceeds terminal dimensions"
    );
    ensure!(
        options.height <= u16::MAX as usize,
        "--height exceeds terminal dimensions"
    );
    let started_at = Instant::now();
    let bootstrap = stream::large_bootstrap(
        std::env::current_dir()?,
        options.file_count,
        options.lines_per_file,
        37,
        84,
        false,
    )?;
    let mut samples = Vec::new();
    if memory {
        samples.push(sample_memory("after_bootstrap", 0)?);
    }

    let viewport = Rect::new(0, 0, options.width as u16, options.height as u16);
    let mut setup = large_stream::Renderer::from_bootstrap_at_viewport(bootstrap, viewport);
    setup.render_pass(1);
    if memory {
        samples.push(sample_memory("after_first_frame", 0)?);
    }

    let mut position = 0usize;
    let mut direction = 1isize;
    for navigation in 1..=options.navigations {
        let (key, next_position, next_direction) =
            next_navigation_key(position, direction, options);
        let before = setup.app.shared_state().lock().unwrap().selection();
        setup
            .app
            .handle_key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::NONE));
        setup.render_pass(1);
        let after = setup.app.shared_state().lock().unwrap().selection();
        if options.file_count > 1 {
            ensure!(
                before != after,
                "navigation key {key} did not change the mounted review selection"
            );
        }
        position = next_position;
        direction = next_direction;
        if memory && (navigation % options.sample_every == 0 || navigation == options.navigations) {
            samples.push(sample_memory("navigation", navigation)?);
        }
    }

    drop(setup);
    if memory {
        samples.push(sample_memory("after_destroy", options.navigations)?);
    }

    let navigation_samples = samples
        .iter()
        .filter(|sample| {
            sample.label == "navigation" && sample.navigation >= options.warmup_navigations
        })
        .collect::<Vec<_>>();
    let first = navigation_samples.first().copied().or_else(|| {
        samples
            .iter()
            .find(|sample| sample.label == "after_first_frame")
    });
    let last = navigation_samples
        .last()
        .copied()
        .or_else(|| samples.last());
    let analyzed = navigation_samples.as_slice();
    let heap_growth = growth(analyzed, |sample| sample.heap_used_bytes);
    let rss_growth = growth(analyzed, |sample| Some(sample.rss_bytes));
    let heap_slope = linear_slope(analyzed, |sample| sample.heap_used_bytes);
    let rss_slope = linear_slope(analyzed, |sample| Some(sample.rss_bytes));
    let max_heap = samples
        .iter()
        .filter_map(|sample| sample.heap_used_bytes)
        .max();
    let max_rss = samples.iter().map(|sample| sample.rss_bytes).max();
    let heap_passed = heap_growth
        .is_none_or(|growth| growth <= (options.max_heap_growth_mb * 1024.0 * 1024.0) as i128)
        && heap_slope.is_none_or(|slope| slope <= options.max_heap_slope_kb * 1024.0);
    let rss_passed = rss_growth
        .is_none_or(|growth| growth <= (options.max_rss_growth_mb * 1024.0 * 1024.0) as i128);
    let passed = heap_passed && rss_passed;

    Ok(serde_json::json!({
        "options": options,
        "elapsedMs": started_at.elapsed().as_secs_f64() * 1000.0,
        "sampleCount": samples.len(),
        "analyzedNavigationSamples": navigation_samples.len(),
        "firstAnalyzedHeapBytes": first.and_then(|sample| sample.heap_used_bytes),
        "lastAnalyzedHeapBytes": last.and_then(|sample| sample.heap_used_bytes),
        "heapGrowthBytes": heap_growth,
        "rssGrowthBytes": rss_growth,
        "heapSlopeBytesPerNavigation": heap_slope,
        "rssSlopeBytesPerNavigation": rss_slope,
        "maxHeapBytes": max_heap,
        "maxRssBytes": max_rss,
        "passed": passed,
        "memorySemantics": "Current process RSS and, where available, native allocator usage; JavaScript heapUsed, heapTotal, external, and ArrayBuffer counters are unavailable in Rust and remain null. No forced garbage collection.",
        "samples": samples,
    }))
}

fn print_optional_metric(name: &str, value: Option<i128>) {
    if let Some(value) = value {
        println!("METRIC {name}={value}");
    }
}

fn print_optional_float_metric(name: &str, value: Option<f64>) {
    if let Some(value) = value {
        println!("METRIC {name}={}", fixed(value, 2));
    }
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let Some(options) = parse_options(args)? else {
        println!(
            "Usage: cargo xtask benchmark navigation-memory [options]\n\nOptions:\n  --navigations <n>         Key-driven workdeck navigations (default 180)\n  --warmup-navigations <n>  Navigations ignored for trend analysis (default 60)\n  --sample-every <n>        Sample every N navigations (default 10)\n  --file-count <n>          One-hunk files in the synthetic review (default 90)\n  --lines-per-file <n>      Lines per synthetic file (default 120)\n  --width <n>               Native terminal width (default 240)\n  --height <n>              Native terminal height (default 28)\n  --mode <bounce|forward>   Bounce avoids end-of-review no-ops (default bounce)\n  --no-gc                   Preserve the source flag; native Rust never forces GC\n  --max-heap-growth-mb <n>  Fail if native allocator usage grows beyond this (default 192)\n  --max-heap-slope-kb <n>   Fail if native allocator slope exceeds this per navigation (default 2048)\n  --max-rss-growth-mb <n>   Fail if RSS grows beyond this (default 384)\n  --json-out <path>         Write full sample summary JSON"
        );
        return Ok(());
    };

    native_memory::snapshot()?;
    println!(
        "navigation memory fixture files={} lines={} navigations={} mode={:?} gc={}",
        options.file_count,
        options.lines_per_file,
        options.navigations,
        options.mode,
        if options.gc { "on" } else { "off" }
    );
    let report = measure(&options, true)?;
    let first_heap = report["firstAnalyzedHeapBytes"].as_u64().map(i128::from);
    let last_heap = report["lastAnalyzedHeapBytes"].as_u64().map(i128::from);
    let heap_growth = report["heapGrowthBytes"].as_i64().map(i128::from);
    let rss_growth = report["rssGrowthBytes"].as_i64().map(i128::from);

    println!("\nNavigation memory summary");
    println!(
        "  analyzed samples:       {}",
        report["analyzedNavigationSamples"]
    );
    if let Some(value) = first_heap {
        println!("  first native heap:      {}", format_bytes(value));
    }
    if let Some(value) = last_heap {
        println!("  last native heap:       {}", format_bytes(value));
    }
    if let Some(value) = heap_growth {
        println!("  native heap growth:     {}", format_bytes(value));
    }
    if let Some(value) = report["heapSlopeBytesPerNavigation"].as_f64() {
        println!(
            "  native heap slope:      {} / navigation",
            format_bytes(value as i128)
        );
    }
    if let Some(value) = rss_growth {
        println!("  RSS growth:             {}", format_bytes(value));
    }
    if let Some(value) = report["rssSlopeBytesPerNavigation"].as_f64() {
        println!(
            "  RSS slope:              {} / navigation",
            format_bytes(value as i128)
        );
    }
    if let Some(value) = report["maxHeapBytes"].as_u64() {
        println!(
            "  max native heap:        {}",
            format_bytes(i128::from(value))
        );
    }
    if let Some(value) = report["maxRssBytes"].as_u64() {
        println!(
            "  max RSS:                {}",
            format_bytes(i128::from(value))
        );
    }
    print_optional_metric("navigation_heap_growth_bytes", heap_growth);
    print_optional_float_metric(
        "navigation_heap_slope_bytes_per_navigation",
        report["heapSlopeBytesPerNavigation"].as_f64(),
    );
    print_optional_metric("navigation_rss_growth_bytes", rss_growth);
    print_optional_float_metric(
        "navigation_rss_slope_bytes_per_navigation",
        report["rssSlopeBytesPerNavigation"].as_f64(),
    );
    if let Some(value) = report["maxHeapBytes"].as_u64() {
        println!("METRIC navigation_max_heap_bytes={value}");
    }
    if let Some(value) = report["maxRssBytes"].as_u64() {
        println!("METRIC navigation_max_rss_bytes={value}");
    }
    if let Some(path) = &options.json_out {
        std::fs::write(
            path,
            format!("{}\n", serde_json::to_string_pretty(&report)?),
        )?;
        println!("wrote {path}");
    }
    ensure!(
        report["passed"].as_bool() == Some(true),
        "Navigation memory growth exceeded configured threshold."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_options_preserve_source_defaults_clamping_and_errors() {
        assert_eq!(
            parse_options(std::iter::empty()).unwrap(),
            Some(Options::default())
        );
        let options = parse_options(
            [
                "--navigations",
                "8.9",
                "--warmup-navigations",
                "99",
                "--sample-every",
                "0",
                "--file-count",
                "0",
                "--lines-per-file",
                "0x20",
                "--width",
                "1",
                "--height",
                "9",
                "--mode",
                "forward",
                "--no-gc",
                "--max-rss-growth-mb",
                "12.5",
                "--json-out",
                "report.json",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap()
        .unwrap();
        assert_eq!(options.navigations, 8);
        assert_eq!(options.warmup_navigations, 7);
        assert_eq!(options.sample_every, 1);
        assert_eq!(options.file_count, 1);
        assert_eq!(options.lines_per_file, 32);
        assert_eq!(options.width, 40);
        assert_eq!(options.height, 10);
        assert_eq!(options.mode, NavigationMode::Forward);
        assert!(!options.gc);
        assert_eq!(options.max_rss_growth_mb, 12.5);
        assert_eq!(options.json_out.as_deref(), Some("report.json"));
        assert_eq!(
            parse_options(["--help", "--bad"].into_iter().map(str::to_owned)).unwrap(),
            None
        );
        for value in ["-1", "Infinity", "NaN", "inf", "text", "1e999"] {
            assert!(
                parse_options(["--width", value].into_iter().map(str::to_owned)).is_err(),
                "{value}"
            );
        }
        assert_eq!(
            parse_options(["--width"].into_iter().map(str::to_owned))
                .unwrap_err()
                .to_string(),
            "Missing value for --width."
        );
        assert!(parse_options(["--mode", "sideways"].into_iter().map(str::to_owned)).is_err());
        assert!(parse_options(["--json-out", ""].into_iter().map(str::to_owned)).is_err());
    }

    #[test]
    fn navigation_key_sequence_matches_bounce_and_forward_contract() {
        let mut options = Options {
            file_count: 3,
            mode: NavigationMode::Bounce,
            ..Options::default()
        };
        let mut position = 0;
        let mut direction = 1;
        let mut keys = Vec::new();
        for _ in 0..6 {
            let (key, next, next_direction) = next_navigation_key(position, direction, &options);
            keys.push(key);
            position = next;
            direction = next_direction;
        }
        assert_eq!(keys, vec![']', ']', '[', '[', ']', ']']);
        assert_eq!((position, direction), (2, 1));
        options.mode = NavigationMode::Forward;
        let mut position = 0;
        let mut direction = 1;
        for _ in 0..6 {
            let (key, next, next_direction) = next_navigation_key(position, direction, &options);
            assert_eq!(key, ']');
            position = next;
            direction = next_direction;
        }
        assert_eq!((position, direction), (2, 1));
    }

    #[test]
    fn native_navigation_mount_renders_and_changes_selection() {
        let options = Options {
            navigations: 4,
            warmup_navigations: 2,
            sample_every: 2,
            file_count: 3,
            lines_per_file: 120,
            width: 80,
            height: 10,
            gc: false,
            ..Options::default()
        };
        let report = measure(&options, false).unwrap();
        assert_eq!(report["sampleCount"], 0);
        assert_eq!(report["passed"], true);
        assert!(report["elapsedMs"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn navigation_memory_rejects_unknown_run_options() {
        assert!(run(["--unexpected".to_owned()].into_iter()).is_err());
    }
}
