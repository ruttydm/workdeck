//! Native MIT translation of Hunk's `benchmarks/resize-memory.ts` workload.
//!
//! A single Ratatui review is mounted while its terminal viewport is resized through the exact
//! width sequence used by the source diagnostic. JavaScript heap counters are represented only
//! by native allocator data when available; RSS remains the portable retained-memory signal.

use super::*;
use ratatui::layout::Rect;
use serde::Serialize;
use std::time::Instant;

const DEFAULT_FILE_COUNT: usize = 180;
const DEFAULT_LINES_PER_FILE: usize = 120;
const DEFAULT_HEIGHT: usize = 28;
const DEFAULT_WIDTHS: [usize; 6] = [160, 200, 240, 280, 220, 180];
const DEFAULT_CYCLES: usize = 2;
const DEFAULT_MAX_HEAP_GROWTH_MB: f64 = 384.0;
const DEFAULT_MAX_RSS_GROWTH_MB: f64 = 1024.0;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    file_count: usize,
    lines_per_file: usize,
    height: usize,
    widths: Vec<usize>,
    cycles: usize,
    gc: bool,
    max_heap_growth_mb: f64,
    max_rss_growth_mb: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_out: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            file_count: DEFAULT_FILE_COUNT,
            lines_per_file: DEFAULT_LINES_PER_FILE,
            height: DEFAULT_HEIGHT,
            widths: DEFAULT_WIDTHS.to_vec(),
            cycles: DEFAULT_CYCLES,
            gc: true,
            max_heap_growth_mb: DEFAULT_MAX_HEAP_GROWTH_MB,
            max_rss_growth_mb: DEFAULT_MAX_RSS_GROWTH_MB,
            json_out: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct MemorySample {
    label: String,
    step: usize,
    width: usize,
    rss_bytes: u64,
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

fn parse_widths(value: Option<String>) -> Result<Vec<usize>> {
    let value = value.ok_or_else(|| anyhow::anyhow!("Missing value for --widths."))?;
    if value.is_empty() {
        bail!("Missing value for --widths.");
    }
    value
        .split(',')
        .map(|part| {
            let width = sample_number(part.trim());
            if !width.is_finite() || width < 40.0 {
                bail!("Expected --widths to be comma-separated terminal widths >= 40.");
            }
            truncate_size("--widths", width)
        })
        .collect()
}

fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Option<Options>> {
    let mut options = Options::default();
    let mut file_count = options.file_count as f64;
    let mut lines_per_file = options.lines_per_file as f64;
    let mut height = options.height as f64;
    let mut cycles = options.cycles as f64;
    let mut max_heap_growth_mb = options.max_heap_growth_mb;
    let mut max_rss_growth_mb = options.max_rss_growth_mb;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--file-count" => file_count = parse_non_negative(&arg, args.next())?,
            "--lines-per-file" => lines_per_file = parse_non_negative(&arg, args.next())?,
            "--height" => height = parse_non_negative(&arg, args.next())?,
            "--widths" => options.widths = parse_widths(args.next())?,
            "--cycles" => cycles = parse_non_negative(&arg, args.next())?,
            "--no-gc" => options.gc = false,
            "--max-heap-growth-mb" => {
                max_heap_growth_mb = parse_non_negative(&arg, args.next())?;
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
    options.file_count = truncate_size("--file-count", file_count)?.max(1);
    options.lines_per_file = truncate_size("--lines-per-file", lines_per_file)?.max(1);
    options.height = truncate_size("--height", height)?.max(10);
    options.cycles = truncate_size("--cycles", cycles)?.max(1);
    options.max_heap_growth_mb = max_heap_growth_mb;
    options.max_rss_growth_mb = max_rss_growth_mb;
    ensure!(
        !options.widths.is_empty(),
        "Expected --widths to include at least one width."
    );
    Ok(Some(options))
}

fn sample_memory(label: &str, step: usize, width: usize) -> Result<MemorySample> {
    let snapshot = native_memory::snapshot()?;
    Ok(MemorySample {
        label: label.to_owned(),
        step,
        width,
        rss_bytes: snapshot.rss_bytes,
        heap_used_bytes: snapshot.malloc_in_use_bytes,
        heap_total_bytes: None,
        external_bytes: None,
        array_buffers_bytes: None,
    })
}

fn percentile(values: &[f64], percent: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let index = ((percent / 100.0 * sorted.len() as f64).ceil() - 1.0)
        .max(0.0)
        .min((sorted.len() - 1) as f64) as usize;
    sorted[index]
}

fn growth(before: &MemorySample, after: &MemorySample, heap: bool) -> Option<i128> {
    let before = if heap {
        before.heap_used_bytes?
    } else {
        before.rss_bytes
    };
    let after = if heap {
        after.heap_used_bytes?
    } else {
        after.rss_bytes
    };
    Some(i128::from(after) - i128::from(before))
}

fn measure(options: &Options, memory: bool) -> Result<serde_json::Value> {
    ensure!(
        options.height <= u16::MAX as usize,
        "--height exceeds terminal dimensions"
    );
    ensure!(
        options
            .widths
            .iter()
            .all(|width| *width <= u16::MAX as usize),
        "--widths exceeds terminal dimensions"
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
    let first_width = options.widths[0];
    let mut samples = Vec::new();
    if memory {
        samples.push(sample_memory("after_bootstrap", 0, first_width)?);
    }
    let mut setup = large_stream::Renderer::from_bootstrap_at_viewport(
        bootstrap,
        Rect::new(0, 0, first_width as u16, options.height as u16),
    );
    setup.render_pass(1);
    if memory {
        samples.push(sample_memory("after_first_frame", 0, first_width)?);
    }

    let mut resize_durations_ms = Vec::new();
    let mut step = 0usize;
    for _cycle in 0..options.cycles {
        for &width in &options.widths {
            step += 1;
            let resize_started = Instant::now();
            setup.resize(width as u16, options.height as u16);
            setup.render_pass(1);
            setup.render_pass(1);
            resize_durations_ms.push(resize_started.elapsed().as_secs_f64() * 1000.0);
            if memory {
                samples.push(sample_memory("resize", step, width)?);
            }
        }
    }
    drop(setup);

    let baseline = samples
        .iter()
        .find(|sample| sample.label == "after_first_frame")
        .or_else(|| samples.first());
    let resize_samples = samples
        .iter()
        .filter(|sample| sample.label == "resize")
        .collect::<Vec<_>>();
    let Some(baseline) = baseline else {
        return Ok(serde_json::json!({
            "options": options,
            "elapsedMs": started_at.elapsed().as_secs_f64() * 1000.0,
            "sampleCount": 0,
            "resizeCount": resize_durations_ms.len(),
            "resizeTotalMs": resize_durations_ms.iter().sum::<f64>(),
            "resizeP95Ms": percentile(&resize_durations_ms, 95.0),
            "passed": true,
            "samples": samples,
        }));
    };
    let final_sample = samples.last().unwrap_or(baseline);
    let peak_heap = resize_samples
        .iter()
        .filter_map(|sample| sample.heap_used_bytes)
        .chain(baseline.heap_used_bytes)
        .max();
    let peak_rss = resize_samples
        .iter()
        .map(|sample| sample.rss_bytes)
        .chain(std::iter::once(baseline.rss_bytes))
        .max();
    let heap_growth = growth(baseline, final_sample, true);
    let rss_growth = growth(baseline, final_sample, false).unwrap_or_default();
    let peak_heap_growth = peak_heap.and_then(|peak| {
        baseline
            .heap_used_bytes
            .map(|baseline| i128::from(peak) - i128::from(baseline))
    });
    let peak_rss_growth = peak_rss.map(|peak| i128::from(peak) - i128::from(baseline.rss_bytes));
    let heap_passed = peak_heap_growth
        .is_none_or(|growth| growth <= (options.max_heap_growth_mb * 1024.0 * 1024.0) as i128);
    let rss_passed = peak_rss_growth
        .is_none_or(|growth| growth <= (options.max_rss_growth_mb * 1024.0 * 1024.0) as i128);
    Ok(serde_json::json!({
        "options": options,
        "elapsedMs": started_at.elapsed().as_secs_f64() * 1000.0,
        "sampleCount": samples.len(),
        "resizeCount": resize_durations_ms.len(),
        "resizeTotalMs": resize_durations_ms.iter().sum::<f64>(),
        "resizeP95Ms": percentile(&resize_durations_ms, 95.0),
        "baselineHeapUsedBytes": baseline.heap_used_bytes,
        "finalHeapUsedBytes": final_sample.heap_used_bytes,
        "peakHeapUsedBytes": peak_heap,
        "heapGrowthBytes": heap_growth,
        "peakHeapGrowthBytes": peak_heap_growth,
        "baselineRssBytes": baseline.rss_bytes,
        "finalRssBytes": final_sample.rss_bytes,
        "peakRssBytes": peak_rss,
        "rssGrowthBytes": rss_growth,
        "peakRssGrowthBytes": peak_rss_growth,
        "passed": heap_passed && rss_passed,
        "memorySemantics": "Current process RSS and, where available, native allocator usage; JavaScript heapUsed, heapTotal, external, and ArrayBuffer counters are unavailable in Rust and remain null. No forced garbage collection.",
        "samples": samples,
    }))
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let Some(options) = parse_options(args)? else {
        println!(
            "Usage: cargo xtask benchmark resize-memory [options]\n\nOptions:\n  --file-count <n>          One-hunk files in the synthetic review (default 180)\n  --lines-per-file <n>      Lines per synthetic file (default 120)\n  --height <n>              Native terminal height (default 28)\n  --widths <csv>            Comma-separated resize widths (default 160,200,240,280,220,180)\n  --cycles <n>              Number of times to repeat the width sequence (default 2)\n  --no-gc                   Preserve the source flag; native Rust never forces GC\n  --max-heap-growth-mb <n>  Fail if native allocator usage grows beyond this (default 384)\n  --max-rss-growth-mb <n>   Fail if RSS grows beyond this (default 1024)\n  --json-out <path>         Write full sample summary JSON"
        );
        return Ok(());
    };
    native_memory::snapshot()?;
    println!(
        "resize memory fixture files={} lines={} widths={} cycles={} gc={}",
        options.file_count,
        options.lines_per_file,
        options
            .widths
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(","),
        options.cycles,
        if options.gc { "on" } else { "off" }
    );
    let report = measure(&options, true)?;
    for (name, key) in [
        ("files", "fileCount"),
        ("lines_per_file", "linesPerFile"),
        ("resize_count", "resizeCount"),
    ] {
        if let Some(value) = report[key].as_u64() {
            println!("METRIC {name}={value}");
        }
    }
    for (name, key) in [
        ("resize_total_ms", "resizeTotalMs"),
        ("resize_p95_ms", "resizeP95Ms"),
        ("elapsed_ms", "elapsedMs"),
    ] {
        if let Some(value) = report[key].as_f64() {
            println!("METRIC {name}={}", fixed(value, 2));
        }
    }
    for (name, key) in [
        ("baseline_heap_used_bytes", "baselineHeapUsedBytes"),
        ("final_heap_used_bytes", "finalHeapUsedBytes"),
        ("peak_heap_used_bytes", "peakHeapUsedBytes"),
        ("heap_growth_bytes", "heapGrowthBytes"),
        ("peak_heap_growth_bytes", "peakHeapGrowthBytes"),
        ("baseline_rss_bytes", "baselineRssBytes"),
        ("final_rss_bytes", "finalRssBytes"),
        ("peak_rss_bytes", "peakRssBytes"),
        ("rss_growth_bytes", "rssGrowthBytes"),
        ("peak_rss_growth_bytes", "peakRssGrowthBytes"),
    ] {
        if let Some(value) = report[key]
            .as_u64()
            .or_else(|| report[key].as_i64().map(|v| v as u64))
        {
            println!("METRIC {name}={value}");
        }
    }
    if let Some(path) = &options.json_out {
        let path = PathBuf::from(path);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(
            &path,
            format!("{}\n", serde_json::to_string_pretty(&report)?),
        )?;
        println!("Wrote {}", path.display());
    }
    ensure!(
        report["passed"].as_bool() == Some(true),
        "resize memory budget failed"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_options_preserve_source_defaults_width_parsing_and_clamping() {
        assert_eq!(
            parse_options(std::iter::empty()).unwrap(),
            Some(Options::default())
        );
        let options = parse_options(
            [
                "--file-count",
                "2.9",
                "--lines-per-file",
                "0x78",
                "--height",
                "1",
                "--widths",
                "40.9, 80, 0xA0",
                "--cycles",
                "0",
                "--no-gc",
                "--max-heap-growth-mb",
                "12.5",
                "--json-out",
                "reports/resize.json",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap()
        .unwrap();
        assert_eq!(options.file_count, 2);
        assert_eq!(options.lines_per_file, 120);
        assert_eq!(options.height, 10);
        assert_eq!(options.widths, vec![40, 80, 160]);
        assert_eq!(options.cycles, 1);
        assert!(!options.gc);
        assert_eq!(options.max_heap_growth_mb, 12.5);
        assert_eq!(options.json_out.as_deref(), Some("reports/resize.json"));
        assert_eq!(
            parse_options(["--help", "--bad"].into_iter().map(str::to_owned)).unwrap(),
            None
        );
        assert!(parse_options(["--widths", "39"].into_iter().map(str::to_owned)).is_err());
        assert!(parse_options(["--widths", ""].into_iter().map(str::to_owned)).is_err());
        assert!(parse_options(["--widths", "40,"].into_iter().map(str::to_owned)).is_err());
        for value in ["-1", "Infinity", "NaN", "text", "1e999"] {
            assert!(
                parse_options(["--height", value].into_iter().map(str::to_owned)).is_err(),
                "{value}"
            );
        }
    }

    #[test]
    fn native_resize_mount_renders_each_width_and_reports_metrics() {
        let options = Options {
            file_count: 3,
            lines_per_file: 120,
            height: 10,
            widths: vec![80, 100],
            cycles: 1,
            gc: false,
            ..Options::default()
        };
        let report = measure(&options, false).unwrap();
        assert_eq!(report["sampleCount"], 0);
        assert_eq!(report["resizeCount"], 2);
        assert_eq!(report["passed"], true);
        assert!(report["resizeTotalMs"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn resize_memory_rejects_unknown_run_options() {
        assert!(run(["--unexpected".to_owned()].into_iter()).is_err());
    }
}
