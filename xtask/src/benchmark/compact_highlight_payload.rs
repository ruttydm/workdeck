//! Native MIT translation of Hunk's `benchmarks/compact-highlight-payload.ts` workload.
//!
//! The source compares a raw worker response with a text-free compact payload and then measures
//! the highlighting operation. Workdeck performs the same exercise using its production
//! syntect/Oniguruma output and compact worker protocol, without a JavaScript runtime.

use super::*;
use serde::Serialize;
use std::time::Instant;
use workdeck_core::{DiffFile, DiffLineKind};
use workdeck_diff::{
    CompactHighlightLineLengths, FileComparisonOptions, FileSnapshot, HighlightLineArrays,
    HighlightedDiffCode, HighlightedFile, HighlightedLine, compact_highlight_runs_for_line,
    compact_highlighted_diff_byte_length, diff_from_file_snapshots, encode_compact_syntax_lines,
    validate_compact_highlighted_diff,
};
use workdeck_tui::{AppTheme, HighlightedDiffRuntime, resolve_theme};

const DEFAULT_LINE_COUNT: usize = 8_000;
const DEFAULT_SAMPLES: usize = 7;
const MAX_POLLS: usize = 10_000;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    line_count: usize,
    samples: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            line_count: DEFAULT_LINE_COUNT,
            samples: DEFAULT_SAMPLES,
        }
    }
}

fn env_number(name: &str, default: usize) -> Result<usize> {
    let Some(value) = std::env::var_os(name) else {
        return Ok(default);
    };
    let value = value.to_string_lossy();
    let parsed = sample_number(&value);
    ensure!(
        parsed.is_finite() && parsed >= 1.0,
        "{name} must be a positive number"
    );
    let parsed = parsed.trunc();
    ensure!(
        parsed < usize::MAX as f64,
        "{name} exceeds native addressable size"
    );
    Ok(parsed as usize)
}

fn options_from_environment() -> Result<Options> {
    Ok(Options {
        line_count: env_number("WORKDECK_COMPACT_HIGHLIGHT_LINES", DEFAULT_LINE_COUNT)?,
        samples: env_number("WORKDECK_COMPACT_HIGHLIGHT_SAMPLES", DEFAULT_SAMPLES)?,
    })
}

fn create_large_diff_file(lines: usize) -> Result<DiffFile> {
    let additions = (0..lines)
        .map(|index| {
            format!("export const item{index} = {{ id: {index}, label: `item-{index}` }};\n")
        })
        .collect::<String>();
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            name: "compact.ts",
            contents: "",
            cache_key: "compact-before",
        },
        FileSnapshot {
            name: "compact.ts",
            contents: &additions,
            cache_key: "compact-after",
        },
        FileComparisonOptions { context_radius: 3 },
    )?;
    file.runtime_id = "compact-benchmark".into();
    file.language = Some("typescript".into());
    file.sources = workdeck_core::FileSourceSnapshots::default();
    file.flags.partial = true;
    file.refresh_identity();
    Ok(file)
}

fn raw_line_lengths(file: &DiffFile) -> CompactHighlightLineLengths {
    let mut deletion = Vec::new();
    let mut addition = Vec::new();
    for line in file.hunks.iter().flat_map(|hunk| &hunk.lines) {
        let length = u32::try_from(line.content.encode_utf16().count()).unwrap_or(u32::MAX);
        match line.kind {
            DiffLineKind::Deletion => deletion.push(length),
            DiffLineKind::Addition => addition.push(length),
            DiffLineKind::Context => {
                deletion.push(length);
                addition.push(length);
            }
        }
    }
    CompactHighlightLineLengths { deletion, addition }
}

fn syntax_sides(code: &HighlightedDiffCode) -> HighlightLineArrays<HighlightedLine> {
    let mut deletion_lines = Vec::with_capacity(code.deletion_line_count);
    let mut addition_lines = Vec::with_capacity(code.addition_line_count);
    for hunk in &code.highlighted {
        for line in hunk {
            if let Some(deletion) = &line.deletion {
                deletion_lines.push(Some(deletion.clone()));
            }
            if let Some(addition) = &line.addition {
                addition_lines.push(Some(addition.clone()));
            }
        }
    }
    HighlightLineArrays {
        deletion_lines,
        addition_lines,
    }
}

#[derive(Debug, Serialize)]
struct RawToken<'a> {
    text: &'a str,
    foreground: [u8; 3],
    bold: bool,
    italic: bool,
    underline: bool,
}

fn raw_tokens<'a>(line: Option<&'a HighlightedLine>) -> Vec<RawToken<'a>> {
    line.into_iter()
        .flatten()
        .map(|token| RawToken {
            text: &token.text,
            foreground: [
                token.foreground.red,
                token.foreground.green,
                token.foreground.blue,
            ],
            bold: token.bold,
            italic: token.italic,
            underline: token.underline,
        })
        .collect()
}

fn raw_response_bytes(file: &HighlightedFile) -> usize {
    let response = file
        .iter()
        .map(|hunk| {
            hunk.iter()
                .map(|line| {
                    serde_json::json!({
                        "deletion": raw_tokens(line.deletion.as_ref()),
                        "addition": raw_tokens(line.addition.as_ref()),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    serde_json::to_vec(&response).map_or(0, |bytes| bytes.len())
}

fn decode_all_compact_lines(payload: &workdeck_diff::CompactHighlightedDiff) {
    for side in ["deletion", "addition"] {
        let line_count = match side {
            "deletion" => payload.deletion.line_offsets.len().saturating_sub(1),
            _ => payload.addition.line_offsets.len().saturating_sub(1),
        };
        for line_index in 0..line_count {
            let _ = compact_highlight_runs_for_line(payload, side, line_index);
        }
    }
}

fn load_highlighted(
    runtime: &mut HighlightedDiffRuntime,
    file: &DiffFile,
    theme: &AppTheme,
    offload_large_diff: bool,
) -> Result<HighlightedDiffCode> {
    for _ in 0..MAX_POLLS {
        if let Some(code) = runtime.prefetch_highlighted_diff(file, theme, offload_large_diff) {
            return Ok(code);
        }
        std::thread::yield_now();
    }
    bail!("native highlight worker did not settle")
}

fn operation_sample(
    runtime: &mut HighlightedDiffRuntime,
    file: &DiffFile,
    theme: &AppTheme,
    offload_large_diff: bool,
) -> Result<f64> {
    let started = Instant::now();
    let highlighted = load_highlighted(runtime, file, theme, offload_large_diff)?;
    // This is the native equivalent of buildSplitRows: walk every visible line and token so the
    // operation includes row materialization rather than only the worker response.
    let rows = highlighted
        .highlighted
        .iter()
        .flat_map(|hunk| hunk.iter())
        .map(|line| {
            line.deletion.as_ref().map_or(0, Vec::len) + line.addition.as_ref().map_or(0, Vec::len)
        })
        .sum::<usize>();
    std::hint::black_box(rows);
    Ok(started.elapsed().as_secs_f64() * 1000.0)
}

fn measure_operation(
    file: &DiffFile,
    theme: &AppTheme,
    samples: usize,
    offload_large_diff: bool,
) -> Result<(Vec<f64>, Vec<f64>)> {
    let mut runtime = HighlightedDiffRuntime::default();
    let _ = load_highlighted(&mut runtime, file, theme, offload_large_diff)?;
    let mut wall = Vec::with_capacity(samples);
    let mut stalls = Vec::with_capacity(samples);
    for _ in 0..samples {
        let elapsed = operation_sample(&mut runtime, file, theme, offload_large_diff)?;
        wall.push(elapsed);
        // Rust has no JavaScript event-loop interval. A synchronous operation's native stall is
        // represented by its wall duration, preserving a conservative diagnostic signal.
        stalls.push(elapsed);
    }
    runtime.dispose_worker();
    Ok((wall, stalls))
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    sorted[sorted.len() / 2]
}

fn median_usize(values: &[usize]) -> usize {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len() / 2]
}

fn percentile(values: &[f64], percentile_value: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(Ordering::Equal));
    let index = ((sorted.len() as f64 * percentile_value).ceil() - 1.0)
        .max(0.0)
        .min((sorted.len() - 1) as f64) as usize;
    sorted[index]
}

fn metric(name: &str, value: f64) {
    println!("METRIC {name}={}", fixed(value, 2));
}

fn measure(options: Options) -> Result<serde_json::Value> {
    let file = create_large_diff_file(options.line_count)?;
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let line_lengths = raw_line_lengths(&file);

    // Warm the production highlighter and native worker before measuring response handling.
    let mut warm_runtime = HighlightedDiffRuntime::default();
    let warm_result = load_highlighted(&mut warm_runtime, &file, &theme, false)?;
    let _ = load_highlighted(&mut warm_runtime, &file, &theme, true)?;
    let raw_hast_bytes = raw_response_bytes(&warm_result.highlighted);
    let mut raw_clone_ms = Vec::with_capacity(options.samples);
    let mut compact_encode_ms = Vec::with_capacity(options.samples);
    let mut compact_transfer_ms = Vec::with_capacity(options.samples);
    let mut compact_decode_ms = Vec::with_capacity(options.samples);
    let mut compact_bytes = Vec::with_capacity(options.samples);

    for sample in 0..options.samples {
        let highlighted = if sample == 0 {
            warm_result.clone()
        } else {
            load_highlighted(&mut warm_runtime, &file, &theme, false)?
        };
        let started = Instant::now();
        std::hint::black_box(highlighted.clone());
        raw_clone_ms.push(started.elapsed().as_secs_f64() * 1000.0);

        let started = Instant::now();
        let sides = syntax_sides(&highlighted);
        let compact = encode_compact_syntax_lines(&sides)?;
        compact_encode_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        compact_bytes.push(compact_highlighted_diff_byte_length(&compact));

        let started = Instant::now();
        let received = compact.clone();
        compact_transfer_ms.push(started.elapsed().as_secs_f64() * 1000.0);

        validate_compact_highlighted_diff(&received, Some(&line_lengths))?;
        let started = Instant::now();
        decode_all_compact_lines(&received);
        compact_decode_ms.push(started.elapsed().as_secs_f64() * 1000.0);
    }
    warm_runtime.dispose_worker();

    let (inline_wall, inline_stall) = measure_operation(&file, &theme, options.samples, false)?;
    let (worker_wall, worker_stall) = measure_operation(&file, &theme, options.samples, true)?;

    let inline_wall_median = median(&inline_wall);
    let inline_wall_p95 = percentile(&inline_wall, 0.95);
    let inline_stall_median = median(&inline_stall);
    let inline_stall_p95 = percentile(&inline_stall, 0.95);
    let worker_wall_median = median(&worker_wall);
    let worker_wall_p95 = percentile(&worker_wall, 0.95);
    let worker_stall_median = median(&worker_stall);
    let worker_stall_p95 = percentile(&worker_stall, 0.95);
    Ok(serde_json::json!({
        "options": options,
        "rawHastJsonBytes": raw_hast_bytes,
        "rawCloneMs": raw_clone_ms,
        "compactBytes": compact_bytes,
        "compactEncodeMs": compact_encode_ms,
        "compactTransferMs": compact_transfer_ms,
        "compactDecodeMs": compact_decode_ms,
        "rawCloneMsMedian": median(&raw_clone_ms),
        "rawCloneMsP95": percentile(&raw_clone_ms, 0.95),
        "compactPayloadBytesMedian": median_usize(&compact_bytes),
        "compactEncodeMsMedian": median(&compact_encode_ms),
        "compactEncodeMsP95": percentile(&compact_encode_ms, 0.95),
        "compactTransferMsMedian": median(&compact_transfer_ms),
        "compactTransferMsP95": percentile(&compact_transfer_ms, 0.95),
        "compactDecodeAllMsMedian": median(&compact_decode_ms),
        "compactDecodeAllMsP95": percentile(&compact_decode_ms, 0.95),
        "compactResponseByteRatio": median_usize(&compact_bytes) as f64 / raw_hast_bytes as f64,
        "inlineOperationWallMsMedian": inline_wall_median,
        "inlineOperationWallMsP95": inline_wall_p95,
        "inlineOperationStallMsMedian": inline_stall_median,
        "inlineOperationStallMsP95": inline_stall_p95,
        "compactWorkerOperationWallMsMedian": worker_wall_median,
        "compactWorkerOperationWallMsP95": worker_wall_p95,
        "compactWorkerOperationStallMsMedian": worker_stall_median,
        "compactWorkerOperationStallMsP95": worker_stall_p95,
        "memorySemantics": "Native syntect token payloads and compact typed-array-equivalent ranges; no HAST or JavaScript structured-clone runtime is present.",
    }))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark compact-highlight-payload accepts no arguments; use environment options");
    }
    let options = options_from_environment()?;
    let report = measure(options)?;
    for (name, key) in [("lines", "lineCount"), ("samples", "samples")] {
        if let Some(value) = report["options"][key].as_u64() {
            metric(name, value as f64);
        }
    }
    for (name, key) in [
        ("raw_hast_json_bytes", "rawHastJsonBytes"),
        ("compact_payload_bytes_median", "compactPayloadBytesMedian"),
    ] {
        if let Some(value) = report[key]
            .as_f64()
            .or_else(|| report[key].as_u64().map(|v| v as f64))
        {
            metric(name, value);
        }
    }
    for (name, key) in [
        ("raw_hast_clone_ms_median", "rawCloneMsMedian"),
        ("raw_hast_clone_ms_p95", "rawCloneMsP95"),
        ("compact_encode_ms_median", "compactEncodeMsMedian"),
        ("compact_encode_ms_p95", "compactEncodeMsP95"),
        ("compact_transfer_ms_median", "compactTransferMsMedian"),
        ("compact_transfer_ms_p95", "compactTransferMsP95"),
        ("compact_decode_all_ms_median", "compactDecodeAllMsMedian"),
        ("compact_decode_all_ms_p95", "compactDecodeAllMsP95"),
        ("compact_response_byte_ratio", "compactResponseByteRatio"),
        (
            "inline_operation_wall_ms_median",
            "inlineOperationWallMsMedian",
        ),
        ("inline_operation_wall_ms_p95", "inlineOperationWallMsP95"),
        (
            "inline_operation_stall_ms_median",
            "inlineOperationStallMsMedian",
        ),
        ("inline_operation_stall_ms_p95", "inlineOperationStallMsP95"),
        (
            "compact_worker_operation_wall_ms_median",
            "compactWorkerOperationWallMsMedian",
        ),
        (
            "compact_worker_operation_wall_ms_p95",
            "compactWorkerOperationWallMsP95",
        ),
        (
            "compact_worker_operation_stall_ms_median",
            "compactWorkerOperationStallMsMedian",
        ),
        (
            "compact_worker_operation_stall_ms_p95",
            "compactWorkerOperationStallMsP95",
        ),
    ] {
        if let Some(value) = report[key].as_f64() {
            metric(name, value);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_payload_source_file_has_expected_shape_and_line_ranges() {
        let file = create_large_diff_file(64).unwrap();
        assert_eq!(file.language.as_deref(), Some("typescript"));
        assert!(file.flags.partial);
        assert_eq!(file.sources.old, None);
        assert_eq!(file.sources.new, None);
        let lengths = raw_line_lengths(&file);
        assert_eq!(lengths.deletion.len(), 0);
        assert_eq!(lengths.addition.len(), 64);
    }

    #[test]
    fn compact_payload_round_trip_validates_utf16_ranges_and_decodes_every_line() {
        let file = create_large_diff_file(96).unwrap();
        let theme = resolve_theme(Some("github-dark-default"), None, &[]);
        let mut runtime = HighlightedDiffRuntime::default();
        let highlighted = load_highlighted(&mut runtime, &file, &theme, false).unwrap();
        let payload = encode_compact_syntax_lines(&syntax_sides(&highlighted)).unwrap();
        validate_compact_highlighted_diff(&payload, Some(&raw_line_lengths(&file))).unwrap();
        decode_all_compact_lines(&payload);
        assert!(compact_highlighted_diff_byte_length(&payload) > 0);
        assert!(raw_response_bytes(&highlighted.highlighted) > 0);
        runtime.dispose_worker();
    }

    #[test]
    fn compact_payload_operation_reports_all_native_latency_series() {
        let report = measure(Options {
            line_count: 96,
            samples: 2,
        })
        .unwrap();
        for key in [
            "rawHastJsonBytes",
            "compactResponseByteRatio",
            "inlineOperationWallMsMedian",
            "compactWorkerOperationWallMsMedian",
        ] {
            assert!(report[key].as_f64().unwrap_or_default() > 0.0, "{key}");
        }
        assert_eq!(report["options"]["lineCount"], 96);
        assert_eq!(report["options"]["samples"], 2);
    }

    #[test]
    fn compact_payload_rejects_run_options_and_invalid_environment_values() {
        assert!(run(["--unexpected".to_owned()].into_iter()).is_err());
        let parsed = sample_number("8.5");
        assert_eq!(parsed.trunc(), 8.0);
        assert!(sample_number("NaN").is_nan());
        assert!(!sample_number("Infinity").is_finite());
    }
}
