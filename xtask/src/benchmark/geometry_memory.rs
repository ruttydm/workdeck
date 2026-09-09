//! Partial MIT port of Hunk benchmarks/geometry-memory.ts.
//! Native diagnostics are not substitutes for JavaScript heap statistics.
use super::*;
use std::time::Instant;
use workdeck_review::LayoutMode;
use workdeck_tui::{DiffSectionGeometryCache, DiffSectionGeometryOptions, resolve_theme};

#[derive(Debug, PartialEq, Eq)]
struct Options {
    files: usize,
    lines: usize,
    width: usize,
    gc_requested: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            files: stream::DEFAULT_FILE_COUNT,
            lines: stream::DEFAULT_LINES_PER_FILE,
            width: 240,
            gc_requested: true,
        }
    }
}

fn parse_options(mut args: impl Iterator<Item = String>) -> Result<Option<Options>> {
    let mut options = Options::default();
    let mut numbers = [
        options.files as f64,
        options.lines as f64,
        options.width as f64,
    ];
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--no-gc" => options.gc_requested = false,
            "--file-count" | "--lines-per-file" | "--width" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("Missing value for {arg}."))?;
                let number = sample_number(&value);
                if !number.is_finite() || number < 0.0 {
                    bail!("Expected {arg} to be a non-negative number.");
                }
                match arg.as_str() {
                    "--file-count" => numbers[0] = number,
                    "--lines-per-file" => numbers[1] = number,
                    _ => numbers[2] = number,
                }
            }
            _ => bail!("Unknown option: {arg}"),
        }
    }
    let native_size = |name: &str, number: f64, minimum: f64| -> Result<usize> {
        let number = number.trunc().max(minimum);
        if number >= usize::MAX as f64 {
            bail!("{name} exceeds the native addressable size.");
        }
        Ok(number as usize)
    };
    options.files = native_size("--file-count", numbers[0], 1.0)?;
    options.lines = native_size("--lines-per-file", numbers[1], 1.0)?;
    options.width = native_size("--width", numbers[2], 40.0)?;
    Ok(Some(options))
}

fn measure(
    file_count: usize,
    lines: usize,
    width: usize,
    giant_lines: usize,
    memory: bool,
) -> Result<serde_json::Value> {
    let sample = || memory.then(native_memory::snapshot).transpose();
    let theme = resolve_theme(Some("midnight"), None, &[]);
    let started = Instant::now();
    let bootstrap = stream::large_bootstrap(PathBuf::from("."), file_count, lines, 37, 84, false)?;
    let bootstrap_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after_bootstrap = sample()?;
    let mut cache = DiffSectionGeometryCache::default();
    let started = Instant::now();
    let geometries = bootstrap
        .changeset
        .files
        .iter()
        .map(|file| {
            let mut options = DiffSectionGeometryOptions::new(file, LayoutMode::Split, &theme);
            options.width = width;
            cache.measure(options)
        })
        .collect::<Vec<_>>();
    let geometry_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after_geometry = sample()?;
    let lazy_before_materialization = geometries
        .iter()
        .all(|geometry| !geometry.planned_rows_are_initialized());
    let started = Instant::now();
    let materialized_rows: usize = geometries
        .iter()
        .map(|geometry| geometry.planned_rows().len())
        .sum();
    let materialize_ms = started.elapsed().as_secs_f64() * 1000.0;
    let after_materialized = sample()?;
    // The giant fixture is deliberately constructed after the retained-memory samples.
    let giant = stream::giant_file(file_count + 1, giant_lines, 1000, 45000)?;
    let mut options = DiffSectionGeometryOptions::new(&giant, LayoutMode::Split, &theme);
    options.width = width;
    let giant_geometry = cache.measure(options);
    let started = Instant::now();
    let giant_rows = giant_geometry.planned_rows().len();
    let giant_first_copy_ms = started.elapsed().as_secs_f64() * 1000.0;
    std::hint::black_box((&bootstrap, &geometries, &giant_geometry));
    Ok(serde_json::json!({
        "diagnosticOnly": true,
        "files": file_count, "linesPerFile": lines, "width": width,
        "bootstrapFixtureMs": bootstrap_ms, "geometryMs": geometry_ms,
        "geometryBodyRows": geometries.iter().map(|geometry| geometry.body_height).sum::<usize>(),
        "geometryRowBounds": geometries.iter().map(|geometry| geometry.row_bounds.len()).sum::<usize>(),
        "lazyBeforeMaterialization": lazy_before_materialization,
        "materializePlannedRowsMs": materialize_ms, "materializedPlannedRows": materialized_rows,
        "afterBootstrap": after_bootstrap, "afterGeometry": after_geometry,
        "afterMaterializedPlannedRows": after_materialized,
        "giantFirstCopyPlanMs": giant_first_copy_ms, "giantMaterializedPlannedRows": giant_rows,
        "giantFileLines": giant_lines,
        "memorySemantics": "Current process RSS and native malloc-zone usage, not peak RSS, JavaScript heap size, extra memory, or object counts. No forced garbage collection."
    }))
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let Some(options) = parse_options(args)? else {
        println!(
            "Usage: cargo xtask benchmark geometry-memory [options]\n\nOptions:\n  --file-count <n>      Synthetic review files (default 180)\n  --lines-per-file <n>  Source lines per synthetic file (default 120)\n  --width <n>           Geometry measurement width (default 240)\n  --no-gc              Disable the source GC request (native diagnostics never force GC)\n"
        );
        return Ok(());
    };
    native_memory::snapshot()?;
    let mut report = measure(
        options.files,
        options.lines,
        options.width,
        stream::GIANT_SINGLE_FILE_LINES,
        true,
    )?;
    report["sourceGcRequested"] = serde_json::json!(options.gc_requested);
    report["nativeForcedGc"] = serde_json::json!(false);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[test]
fn geometry_options_preserve_numeric_coercion_clamping_and_order() {
    let parse = |args: &[&str]| parse_options(args.iter().map(|arg| (*arg).to_owned()));
    assert_eq!(parse(&[]).unwrap(), Some(Options::default()));
    assert_eq!(
        parse(&[
            "--file-count",
            "2.9",
            "--lines-per-file",
            "0x78",
            "--width",
            "",
            "--no-gc"
        ])
        .unwrap(),
        Some(Options {
            files: 2,
            lines: 120,
            width: 40,
            gc_requested: false
        })
    );
    assert_eq!(
        parse(&["--file-count", "0", "--file-count", "3"])
            .unwrap()
            .unwrap()
            .files,
        3
    );
    assert_eq!(parse(&["--help", "--unknown"]).unwrap(), None);
    assert_eq!(parse(&["--width", "1e100", "--help"]).unwrap(), None);
    assert_eq!(
        parse(&["--width", "1e100", "--width", "80"])
            .unwrap()
            .unwrap()
            .width,
        80
    );
    assert!(parse(&["--unknown", "--help"]).is_err());
    for value in ["-1", "Infinity", "NaN", "inf", "text", "1e999"] {
        assert!(parse(&["--width", value]).is_err(), "{value}");
    }
    assert_eq!(
        parse(&["--width"]).unwrap_err().to_string(),
        "Missing value for --width."
    );
    assert!(parse(&["--width", "1e100"]).is_err());
    assert!(run(["--help".into()].into_iter()).is_ok());
}

#[test]
fn geometry_options_and_rows_match_both_frozen_source_oracles() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../port/hunk/oracles/geometry-memory.json"
    ))
    .unwrap();
    let report = measure(2, 120, 80, stream::GIANT_SINGLE_FILE_LINES, false).unwrap();
    let captures = fixture["captures"].as_array().unwrap();
    assert_eq!(captures.len(), 2);
    for capture in captures {
        let cases = capture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 8);
        for case in cases {
            let args = case["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|arg| arg.as_str().unwrap().to_owned());
            let parsed = parse_options(args);
            let output = case["combinedOutput"].as_str().unwrap();
            if case["exitCode"] == 1 {
                let error = parsed.unwrap_err().to_string();
                assert!(output.lines().any(|line| line == format!("error: {error}")));
            } else if output.starts_with("Usage:") {
                assert_eq!(case["exitCode"], 0);
                assert_eq!(parsed.unwrap(), None);
            } else {
                assert_eq!(case["exitCode"], 0);
                assert_eq!(
                    parsed.unwrap(),
                    Some(Options {
                        files: 2,
                        lines: 120,
                        width: 80,
                        gc_requested: false,
                    })
                );
                for (source, native) in [
                    ("files", "files"),
                    ("lines_per_file", "linesPerFile"),
                    ("geometry_body_rows", "geometryBodyRows"),
                    ("geometry_row_bounds", "geometryRowBounds"),
                    ("materialized_planned_rows", "materializedPlannedRows"),
                    (
                        "giant_materialized_planned_rows",
                        "giantMaterializedPlannedRows",
                    ),
                    ("giant_file_lines", "giantFileLines"),
                ] {
                    let prefix = format!("METRIC {source}=");
                    let expected: u64 = output
                        .lines()
                        .find_map(|line| line.strip_prefix(&prefix))
                        .unwrap()
                        .parse()
                        .unwrap();
                    assert_eq!(report[native], expected, "{}: {source}", capture["kind"]);
                }
            }
        }
    }
}

#[test]
fn geometry_diagnostic_retains_lazy_plans_and_materializes_copy_rows() {
    let report = measure(2, 120, 80, 1200, false).unwrap();
    assert_eq!(report["files"], 2);
    assert_eq!(report["lazyBeforeMaterialization"], true);
    assert_eq!(
        report["geometryRowBounds"],
        report["materializedPlannedRows"]
    );
    assert!(report["geometryBodyRows"].as_u64().unwrap() > 0);
    assert!(report["giantMaterializedPlannedRows"].as_u64().unwrap() > 0);
    assert!(report["afterBootstrap"].is_null());
    assert!(run(["--unknown".into()].into_iter()).is_err());
}
