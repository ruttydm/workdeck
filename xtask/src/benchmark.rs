//! MIT translation of Hunk benchmark result aggregation (Modem Labs Inc.).
//! Historical thresholds describe source reports, not the strict semantic-port release gate.

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::{cmp::Ordering, fs};

const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";

/// Verify the native workflow replacement for Hunk's benchmark workflow.
///
/// The pinned workflow is read through Git so the audit covers its trigger,
/// concurrency, workload, summary, and artifact behavior without retaining or
/// executing a Bun workflow. The checked-in replacement invokes the Rust
/// benchmark owners directly.
pub(crate) fn verify_workflow(repo: &Path) -> Result<()> {
    let source = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("{BASELINE}:.github/workflows/benchmarks.yml"),
        ],
    )?;
    ensure!(
        source.len() == 1_970,
        "pinned benchmark workflow changed size: {} != 1970",
        source.len()
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "name: Benchmarks",
        "branches:",
        "paths-ignore:",
        "workflow_dispatch",
        "group: benchmarks-${{ github.workflow }}-${{ github.ref }}",
        "cancel-in-progress: true",
        "name: Benchmark scripts",
        "runs-on: ubuntu-latest",
        "uses: actions/checkout@",
        "uses: oven-sh/setup-bun@",
        "bun-version: 1.3.14",
        "bun install --frozen-lockfile",
        "bench:bootstrap-load",
        "bench:highlight-prefetch",
        "bench:large-stream",
        "bench:wrapped-cjk",
        "GITHUB_STEP_SUMMARY",
        "actions/upload-artifact@",
    ] {
        ensure!(
            source.contains(marker),
            "pinned benchmark workflow lost marker {marker:?}"
        );
    }

    let native_path = repo.join(".github/workflows/benchmarks.yml");
    let native = std::fs::read_to_string(&native_path)
        .with_context(|| format!("read native benchmark workflow {}", native_path.display()))?;
    for marker in [
        "name: Benchmarks",
        "workflow_dispatch",
        "group: benchmarks-${{ github.workflow }}-${{ github.ref }}",
        "cancel-in-progress: true",
        "name: Native benchmark suite",
        "uses: actions/checkout@v5",
        "uses: dtolnay/rust-toolchain@stable",
        "uses: Swatinem/rust-cache@v2",
        "cargo xtask benchmark bootstrap-load",
        "cargo xtask benchmark highlight-prefetch",
        "cargo xtask benchmark large-stream",
        "cargo xtask benchmark wrapped-cjk",
        "GITHUB_STEP_SUMMARY",
        "uses: actions/upload-artifact@v4",
    ] {
        ensure!(
            native.contains(marker),
            "native benchmark workflow is missing {marker:?}"
        );
    }
    for forbidden in ["bun", "node", "npm", "opentui", "wasm", "hunk"] {
        let pattern = regex::Regex::new(&format!(
            r"(?i)(?:^|[^a-z]){}(?:$|[^a-z])",
            regex::escape(forbidden)
        ))
        .expect("forbidden runtime token pattern is valid");
        ensure!(
            !pattern.is_match(&native),
            "native benchmark workflow retains forbidden runtime token {forbidden:?}"
        );
    }
    let migration = std::fs::read_to_string(repo.join("docs/benchmark-workflow-migration.md"))?;
    for marker in [
        ".github/workflows/benchmarks.yml",
        "cargo xtask benchmark bootstrap-load",
        "summary",
        "artifact contract",
        "Bun",
        "not executed",
    ] {
        ensure!(
            migration.contains(marker),
            "benchmark workflow migration is missing {marker:?}"
        );
    }
    Ok(())
}

const TERMINAL_WIDTH_PATH: &str = "benchmarks/terminal-width.ts";
const TERMINAL_WIDTH_BYTES: usize = 3_267;
const TERMINAL_WIDTH_LINES: usize = 72;
const TERMINAL_WIDTH_SHA256: &str =
    "948a538a51c09018f85953ebe33184ba893ee5e3286b420610a50df55bd65fd8";

const GEOMETRY_MEMORY_PATH: &str = "benchmarks/geometry-memory.ts";
const GEOMETRY_MEMORY_BYTES: usize = 6_932;
const GEOMETRY_MEMORY_LINES: usize = 194;
const GEOMETRY_MEMORY_SHA256: &str =
    "5a3707a75a0b730392016642d1d28ee6be5b9c04eef8e0870947055318e9f5c8";

const BENCHMARK_RUN_PATH: &str = "benchmarks/run.ts";
const BENCHMARK_RUN_BYTES: usize = 5_568;
const BENCHMARK_RUN_LINES: usize = 217;
const BENCHMARK_RUN_SHA256: &str =
    "0a86be0791cf64a61771b451e4a38aa2e77ebfa51cc2dd5bc083fb3b4cc17891";

/// Verify the executable native replacement for Hunk's terminal-width
/// benchmark. The source is read from both protected pins; no TypeScript or
/// string-width runtime is copied into the final tree.
pub(crate) fn verify_terminal_width(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "terminal-width verifier received unexpected baseline {baseline}"
    );
    let source =
        crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{TERMINAL_WIDTH_PATH}")])?;
    let stable = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd:{TERMINAL_WIDTH_PATH}"),
        ],
    )?;
    for (pin, bytes) in [(BASELINE, &source), ("stable-v0.20.1", &stable)] {
        ensure!(
            bytes.len() == TERMINAL_WIDTH_BYTES,
            "pinned {TERMINAL_WIDTH_PATH} {pin} changed size: {} != {TERMINAL_WIDTH_BYTES}",
            bytes.len()
        );
        ensure!(
            bytes.split(|byte| *byte == b'\n').count() == TERMINAL_WIDTH_LINES + 1,
            "pinned {TERMINAL_WIDTH_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(bytes)) == TERMINAL_WIDTH_SHA256,
            "pinned {TERMINAL_WIDTH_PATH} {pin} changed SHA-256"
        );
    }
    ensure!(
        source == stable,
        "pinned terminal-width benchmark diverged between pins"
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "performance.now",
        "string-width",
        "measureTextWidth",
        "ITERATIONS = 2_000",
        "WARMUP_ITERATIONS = 50",
        "CJK_SCALAR_LINES",
        "EMOJI_SCALAR_LINES",
        "COMPLEX_CLUSTER_LINES",
        "measureWidthCalls",
        "measureScenario",
        "Width checksum",
        "competitor_string_width",
        "METRIC",
    ] {
        ensure!(
            source.contains(marker),
            "pinned terminal-width benchmark is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        (
            "xtask/src/benchmark/terminal_width.rs",
            "pub(super) fn run(",
        ),
        (
            "xtask/src/benchmark/terminal_width.rs",
            "fn exact_source_width_corpora_match_both_pinned_checksums_after_warmup",
        ),
        (
            "xtask/src/benchmark/terminal_width.rs",
            "fn measure_width_calls(",
        ),
        ("xtask/src/benchmark.rs", "benchmark terminal-width"),
        ("docs/benchmarks.md", "cargo xtask benchmark terminal-width"),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read terminal-width native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "terminal-width native surface {path} is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/terminal-width-benchmark-migration.md"))
        .context("read terminal-width migration documentation")?;
    for marker in [
        TERMINAL_WIDTH_PATH,
        "3,267",
        TERMINAL_WIDTH_SHA256,
        "CJK",
        "emoji",
        "combining",
        "warmup",
        "Cargo",
        "no JavaScript runtime",
    ] {
        ensure!(
            docs.contains(marker),
            "terminal-width migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

/// Verify the native retained-geometry benchmark replacing Hunk's memory
/// diagnostic.  Allocator fields are intentionally represented by native
/// snapshots rather than fabricated JavaScript heap counters.
pub(crate) fn verify_geometry_memory(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "geometry-memory verifier received unexpected baseline {baseline}"
    );
    let source = crate::git_stdout_bytes(
        repo,
        ["show", &format!("{BASELINE}:{GEOMETRY_MEMORY_PATH}")],
    )?;
    let stable = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd:{GEOMETRY_MEMORY_PATH}"),
        ],
    )?;
    for (pin, bytes) in [(BASELINE, &source), ("stable-v0.20.1", &stable)] {
        ensure!(
            bytes.len() == GEOMETRY_MEMORY_BYTES,
            "pinned {GEOMETRY_MEMORY_PATH} {pin} changed size: {} != {GEOMETRY_MEMORY_BYTES}",
            bytes.len()
        );
        ensure!(
            bytes.split(|byte| *byte == b'\n').count() == GEOMETRY_MEMORY_LINES + 1,
            "pinned {GEOMETRY_MEMORY_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(bytes)) == GEOMETRY_MEMORY_SHA256,
            "pinned {GEOMETRY_MEMORY_PATH} {pin} changed SHA-256"
        );
    }
    ensure!(
        source == stable,
        "pinned geometry-memory benchmark diverged between pins"
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "Track retained memory",
        "heapStats",
        "performance.now",
        "measureDiffSectionGeometry",
        "createGiantSingleDiffFile",
        "DEFAULT_FILE_COUNT",
        "GIANT_SINGLE_FILE_LINES",
        "sampleMemory",
        "printMemory",
        "materialized_planned_rows",
        "giant_first_copy_plan_ms",
    ] {
        ensure!(
            source.contains(marker),
            "pinned geometry-memory benchmark is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        (
            "xtask/src/benchmark/geometry_memory.rs",
            "pub(super) fn run(",
        ),
        (
            "xtask/src/benchmark/geometry_memory.rs",
            "geometry_diagnostic_retains_lazy_plans_and_materializes_copy_rows",
        ),
        ("xtask/src/benchmark.rs", "Some(\"geometry-memory\")"),
        ("xtask/src/benchmark/runner.rs", "geometry-memory.ts"),
        (
            "docs/benchmarks.md",
            "cargo xtask benchmark geometry-memory",
        ),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read geometry-memory native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "geometry-memory native surface {path} is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/geometry-memory-benchmark-migration.md"))
        .context("read geometry-memory migration documentation")?;
    for marker in [
        GEOMETRY_MEMORY_PATH,
        "6,932",
        GEOMETRY_MEMORY_SHA256,
        "retained geometry",
        "giant",
        "RSS",
        "allocator",
        "no JavaScript heap",
        "Rust",
    ] {
        ensure!(
            docs.contains(marker),
            "geometry-memory migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

/// Verify the native Rust benchmark orchestrator replacing Hunk's Bun runner.
pub(crate) fn verify_runner(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == BASELINE,
        "benchmark runner verifier received unexpected baseline {baseline}"
    );
    let source =
        crate::git_stdout_bytes(repo, ["show", &format!("{BASELINE}:{BENCHMARK_RUN_PATH}")])?;
    let stable = crate::git_stdout_bytes(
        repo,
        [
            "show",
            &format!("4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd:{BENCHMARK_RUN_PATH}"),
        ],
    )?;
    for (pin, bytes) in [(BASELINE, &source), ("stable-v0.20.1", &stable)] {
        ensure!(
            bytes.len() == BENCHMARK_RUN_BYTES,
            "pinned {BENCHMARK_RUN_PATH} {pin} changed size: {} != {BENCHMARK_RUN_BYTES}",
            bytes.len()
        );
        ensure!(
            bytes.split(|byte| *byte == b'\n').count() == BENCHMARK_RUN_LINES + 1,
            "pinned {BENCHMARK_RUN_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(bytes)) == BENCHMARK_RUN_SHA256,
            "pinned {BENCHMARK_RUN_PATH} {pin} changed SHA-256"
        );
    }
    ensure!(
        source == stable,
        "pinned benchmark runner diverged between pins"
    );
    let source = std::str::from_utf8(&source)?;
    for marker in [
        "#!/usr/bin/env bun",
        "defaultScripts",
        "RunOptions",
        "readArgValue",
        "parseArgs",
        "gitSha",
        "packageVersion",
        "parseMetrics",
        "runScript",
        "aggregateMetric",
        "localeCompare",
        "BenchmarkRunResult",
        "includeCompetitors",
        "includeHuge",
        "samplesByMetric",
    ] {
        ensure!(
            source.contains(marker),
            "pinned benchmark runner is missing marker {marker:?}"
        );
    }
    for (path, marker) in [
        (
            "xtask/src/benchmark/runner.rs",
            "pub(super) fn run_command(",
        ),
        ("xtask/src/benchmark/runner.rs", "fn collect("),
        (
            "xtask/src/benchmark/runner.rs",
            "pub(super) fn parse_metrics(",
        ),
        ("xtask/src/benchmark/runner.rs", "package_version"),
        ("xtask/src/benchmark.rs", "Some(\"run\")"),
        ("docs/benchmarks.md", "cargo xtask benchmark run --samples"),
    ] {
        let native = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read benchmark runner native surface {path}"))?;
        ensure!(
            native.contains(marker),
            "benchmark runner native surface {path} is missing {marker:?}"
        );
    }
    let docs = fs::read_to_string(repo.join("docs/benchmark-runner-migration.md"))
        .context("read benchmark runner migration documentation")?;
    for marker in [
        BENCHMARK_RUN_PATH,
        "5,568",
        BENCHMARK_RUN_SHA256,
        "samples",
        "competitors",
        "huge",
        "metrics",
        "Rust",
        "no Bun",
    ] {
        ensure!(
            docs.contains(marker),
            "benchmark runner migration documentation is missing {marker:?}"
        );
    }
    Ok(())
}

mod bootstrap;
mod changeset_parse;
pub(crate) mod competitors;
mod fixtures;
mod geometry_memory;
mod highlight_prefetch;
mod historical;
mod huge_stream;
pub(crate) use historical::{
    verify as verify_historical, verify_for_baseline as verify_historical_for_baseline,
};
mod interaction_latency;
mod large_stream;
mod large_stream_profile;
mod native_memory;
mod non_ascii_stream;
mod release;
mod render_layout;
mod runner;
mod stream;
mod terminal_width;
mod working_tree;
mod wrapped_cjk;

#[derive(Debug, Serialize)]
struct ReleaseRunOptions {
    version: String,
    samples: f64,
    out: PathBuf,
}

fn sample_number(value: &str) -> f64 {
    let value = super::release_channel::trim_source_whitespace(value);
    if value.is_empty() {
        return 0.0;
    }
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ] {
        if let Some(digits) = value.strip_prefix(prefix) {
            if digits.is_empty() {
                return f64::NAN;
            }
            let mut result = 0.0;
            for digit in digits.chars() {
                let Some(digit) = digit.to_digit(radix) else {
                    return f64::NAN;
                };
                result = result * f64::from(radix) + f64::from(digit);
            }
            return result;
        }
    }
    // Rust also accepts spellings such as "inf" that JavaScript Number does not.
    let decimal = regex::Regex::new(
        r"^[+-]?(?:(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?|Infinity)$",
    )
    .expect("literal regex");
    if !decimal.is_match(value) {
        return f64::NAN;
    }
    value.parse().unwrap_or(f64::NAN)
}

fn release_run_options(
    root: &Path,
    cwd: &Path,
    version: String,
    samples_env: Option<&str>,
    mut args: impl Iterator<Item = String>,
) -> Result<ReleaseRunOptions> {
    let output = |version: &str| -> Result<PathBuf> {
        release_version(version)?;
        Ok(root
            .join("benchmarks/release")
            .join(format!("bench-{version}.json")))
    };
    let mut options = ReleaseRunOptions {
        out: output(&version)?,
        version,
        samples: sample_number(samples_env.unwrap_or("5")),
    };
    let mut explicit = false;
    while let Some(arg) = args.next() {
        if !matches!(arg.as_str(), "--version" | "--samples" | "--out") {
            bail!("Unknown release benchmark argument: {arg}");
        }
        let value = args
            .next()
            .filter(|v| !v.is_empty())
            .ok_or_else(|| anyhow::anyhow!("Missing value for {arg}"))?;
        match arg.as_str() {
            "--version" => {
                options.version = value;
                if !explicit {
                    options.out = output(&options.version)?;
                }
            }
            "--samples" => options.samples = sample_number(&value),
            "--out" => {
                let absolute = std::path::absolute(cwd.join(value))?;
                let mut resolved = PathBuf::new();
                for component in absolute.components() {
                    match component {
                        std::path::Component::CurDir => {}
                        std::path::Component::ParentDir => {
                            resolved.pop();
                        }
                        other => resolved.push(other.as_os_str()),
                    }
                }
                options.out = resolved;
                explicit = true;
            }
            _ => unreachable!(),
        }
    }
    if !options.samples.is_finite() || options.samples < 1.0 {
        bail!("--samples must be a positive number");
    }
    Ok(options)
}

fn release_plan(args: impl Iterator<Item = String>) -> Result<()> {
    let root = super::repo_root()?;
    let metadata = cargo_metadata::MetadataCommand::new()
        .current_dir(&root)
        .no_deps()
        .exec()?;
    let version = metadata
        .packages
        .iter()
        .find(|p| p.name.as_str() == "workdeck-cli")
        .ok_or_else(|| anyhow::anyhow!("workspace does not contain workdeck-cli"))?
        .version
        .to_string();
    let samples = std::env::var("WORKDECK_RELEASE_BENCHMARK_SAMPLES").ok();
    let options = release_run_options(
        &root,
        &std::env::current_dir()?,
        version,
        samples.as_deref(),
        args,
    )?;
    println!("{}", serde_json::to_string(&options)?);
    Ok(())
}

struct ReleaseVersion {
    parts: [f64; 3],
    prerelease: Option<String>,
}

fn release_version(version: &str) -> Result<ReleaseVersion> {
    let (stable, prerelease) = version
        .split_once('-')
        .map_or((version, None), |(stable, pre)| (stable, Some(pre)));
    let parts: Vec<_> = stable.split('.').collect();
    if parts.len() != 3
        || parts
            .iter()
            .any(|s| s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()))
        || prerelease.is_some_and(|s| {
            s.is_empty()
                || !s
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        })
    {
        bail!("Invalid release benchmark version: {version}");
    }
    Ok(ReleaseVersion {
        parts: [parts[0].parse()?, parts[1].parse()?, parts[2].parse()?],
        prerelease: prerelease.map(str::to_owned),
    })
}

fn stable_difference(left: &ReleaseVersion, right: &ReleaseVersion) -> f64 {
    for (left, right) in left.parts.iter().zip(right.parts) {
        let delta = left - right;
        if delta != 0.0 {
            return delta;
        }
    }
    // Used only with a stable left operand when selecting a previous stable snapshot.
    if right.prerelease.is_some() { 1.0 } else { 0.0 }
}

fn previous_release(version: &str, directory: &Path) -> Result<Option<(String, PathBuf)>> {
    let current = release_version(version)?;
    if !directory.exists() {
        return Ok(None);
    }
    let mut names = std::fs::read_dir(directory)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<std::io::Result<Vec<_>>>()?;
    names.sort();
    let mut candidates = Vec::new();
    for name in names {
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(raw) = name
            .strip_prefix("bench-")
            .and_then(|s| s.strip_suffix(".json"))
        else {
            continue;
        };
        let Ok(candidate) = release_version(raw) else {
            continue;
        };
        if candidate.prerelease.is_some() || stable_difference(&candidate, &current) >= 0.0 {
            continue;
        }
        candidates.push((raw.to_owned(), directory.join(name), candidate));
    }
    candidates.sort_by(|left, right| {
        stable_difference(&right.2, &left.2)
            .partial_cmp(&0.0)
            .unwrap_or(Ordering::Equal)
    });
    Ok(candidates
        .into_iter()
        .next()
        .map(|(version, path, _)| (version, path)))
}

fn previous_command(mut args: impl Iterator<Item = String>) -> Result<()> {
    let (Some(version), Some(directory)) = (args.next(), args.next()) else {
        bail!("benchmark previous requires VERSION RELEASE_DIRECTORY");
    };
    if args.next().is_some() {
        bail!("unexpected benchmark previous argument");
    }
    let result = previous_release(&version, Path::new(&directory))?
        .map(|(version, path)| serde_json::json!({"version":version,"path":path}));
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Run {
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    generated_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    git_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    package_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<RuntimeInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    samples_per_benchmark: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    accepted_regressions: Option<Vec<AcceptedRegression>>,
    results: Vec<Metric>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeInfo {
    // Read historical oracle metadata; this never enables or invokes a runtime.
    #[serde(skip_serializing_if = "Option::is_none")]
    bun_version: Option<String>,
    platform: String,
    arch: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct AcceptedRegression {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Comparison {
    version: u32,
    generated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_sha: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    head_sha: Option<String>,
    failed: bool,
    rows: Vec<Row>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    name: String,
    unit: String,
    base_median: f64,
    head_median: f64,
    absolute_delta: f64,
    relative_delta: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    threshold: Option<Threshold>,
    status: &'static str,
    source: String,
}

fn material_regression(base: f64, head: f64, threshold: &Threshold) -> bool {
    let delta = head - base;
    if delta <= 0.0 || delta < threshold.min_absolute_regression {
        return false;
    }
    if base == 0.0 {
        return head > 0.0;
    }
    head / base >= threshold.max_regression_ratio
}

fn compare(base: &Run, head: &Run, generated_at: String) -> Comparison {
    let base_by_name: BTreeMap<_, _> = base.results.iter().map(|m| (m.name.as_str(), m)).collect();
    let head_by_name: BTreeMap<_, _> = head.results.iter().map(|m| (m.name.as_str(), m)).collect();
    let mut names: Vec<_> = base_by_name
        .keys()
        .chain(head_by_name.keys())
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    // JavaScript's default sort compares UTF-16 code units, not Unicode scalar values.
    names.sort_by(|left, right| left.encode_utf16().cmp(right.encode_utf16()));
    let accepted: BTreeSet<_> = head
        .accepted_regressions
        .iter()
        .flatten()
        .map(|a| a.name.as_str())
        .collect();
    let rows: Vec<_> = names
        .into_iter()
        .map(|name| {
            let base = base_by_name.get(name).copied();
            let head = head_by_name.get(name).copied();
            let metadata = head.or(base).expect("union contains a metric");
            let threshold = head
                .and_then(|m| m.threshold.clone())
                .or_else(|| base.and_then(|m| m.threshold.clone()));
            let base_median = base.map_or(0.0, |m| m.median);
            let head_median = head.map_or(0.0, |m| m.median);
            let (relative_delta, status) = match (base, head) {
                (None, Some(m)) => (
                    f64::INFINITY,
                    if m.comparable {
                        "missing-base"
                    } else {
                        "informational"
                    },
                ),
                (Some(m), None) => (
                    -1.0,
                    if m.comparable {
                        "missing-head"
                    } else {
                        "informational"
                    },
                ),
                (Some(_), Some(m)) => {
                    let delta = if base_median == 0.0 {
                        if head_median == 0.0 {
                            0.0
                        } else {
                            f64::INFINITY
                        }
                    } else {
                        head_median / base_median - 1.0
                    };
                    let status = match threshold.as_ref().filter(|_| m.comparable) {
                        None => "informational",
                        Some(t) if material_regression(base_median, head_median, t) => {
                            if accepted.contains(name) {
                                "accepted"
                            } else {
                                "fail"
                            }
                        }
                        Some(_) => "pass",
                    };
                    (delta, status)
                }
                (None, None) => unreachable!(),
            };
            Row {
                name: name.into(),
                unit: metadata.unit.clone(),
                base_median,
                head_median,
                absolute_delta: head_median - base_median,
                relative_delta,
                threshold,
                status,
                source: metadata.source.clone(),
            }
        })
        .collect();
    Comparison {
        version: 1,
        generated_at,
        base_sha: base.git_sha.clone(),
        head_sha: head.git_sha.clone(),
        failed: rows
            .iter()
            .any(|r| matches!(r.status, "fail" | "missing-head")),
        rows,
    }
}

// Number.toFixed rounds the exact binary value, with ties away from zero. Scaling an f64
// first can introduce a second rounding (for example 2.55 * 10), so use its integer significand.
fn fixed(value: f64, digits: u32) -> String {
    assert!(digits <= 2);
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into();
    }
    if value.abs() >= 1e21 {
        let result = format!("{value:e}");
        return if result.contains("e-") {
            result
        } else {
            result.replace('e', "e+")
        };
    }
    let absolute = value.abs();
    let bits = absolute.to_bits();
    let exponent_bits = ((bits >> 52) & 0x7ff) as i32;
    let fraction = bits & ((1_u64 << 52) - 1);
    let (significand, exponent) = if exponent_bits == 0 {
        (fraction, -1074)
    } else {
        (fraction | (1_u64 << 52), exponent_bits - 1023 - 52)
    };
    let scaled = u128::from(significand) * 10_u128.pow(digits);
    let rounded = if exponent >= 0 {
        scaled << exponent
    } else if -exponent >= 128 {
        0
    } else {
        let shift = -exponent as u32;
        let quotient = scaled >> shift;
        let remainder = scaled & ((1_u128 << shift) - 1);
        quotient + u128::from(remainder >= (1_u128 << (shift - 1)))
    };
    let sign = if value < 0.0 { "-" } else { "" };
    if digits == 0 {
        return format!("{sign}{rounded}");
    }
    let divisor = 10_u128.pow(digits);
    format!(
        "{sign}{}.{:0width$}",
        rounded / divisor,
        rounded % divisor,
        width = digits as usize
    )
}

fn number(value: f64) -> String {
    if !value.is_finite() {
        return "∞".into();
    }
    fixed(value, if value.abs() >= 100.0 { 1 } else { 2 })
}

fn markdown(comparison: &Comparison, base: &str, head: &str) -> String {
    let failures = comparison
        .rows
        .iter()
        .filter(|r| matches!(r.status, "fail" | "missing-head"))
        .count();
    let result = if comparison.failed {
        format!(
            "❌ {failures} material benchmark regression{} found.",
            if failures == 1 { "" } else { "s" }
        )
    } else {
        "✅ No unaccepted material benchmark regressions found.".into()
    };
    let mut lines = vec![
        "## Release benchmark gate".into(),
        String::new(),
        result,
        String::new(),
        format!("Base: `{base}`  "),
        format!("Head: `{head}`"),
        String::new(),
        "| Status | Metric | Base median | Head median | Δ | Threshold |".into(),
        "| --- | --- | ---: | ---: | ---: | --- |".into(),
    ];
    for row in &comparison.rows {
        let unit = if row.unit == "bytes" { "B" } else { &row.unit };
        let icon = match row.status {
            "fail" | "missing-head" => "❌",
            "accepted" => "⚠️",
            _ => "✅",
        };
        let delta = if !row.relative_delta.is_finite() {
            "+∞".into()
        } else {
            format!(
                "{}{}%",
                if row.relative_delta > 0.0 { "+" } else { "" },
                fixed(row.relative_delta * 100.0, 1)
            )
        };
        let threshold = row.threshold.as_ref().map_or_else(
            || "—".into(),
            |t| {
                let absolute = if row.unit == "bytes" {
                    format!(
                        "{} MiB",
                        number(t.min_absolute_regression / (1024.0 * 1024.0))
                    )
                } else {
                    format!("{} {unit}", number(t.min_absolute_regression))
                };
                format!(
                    "+{}% and +{absolute}",
                    fixed((t.max_regression_ratio - 1.0) * 100.0, 0)
                )
            },
        );
        lines.push(format!(
            "| {icon} {} | `{}` | {} {unit} | {} {unit} | {delta} | {threshold} |",
            row.status,
            row.name,
            number(row.base_median),
            number(row.head_median)
        ));
    }
    format!("{}\n", lines.join("\n"))
}

fn compare_files(mut args: impl Iterator<Item = String>, as_markdown: bool) -> Result<()> {
    let (Some(base), Some(head)) = (args.next(), args.next()) else {
        bail!("benchmark compare-json requires BASE_JSON HEAD_JSON");
    };
    if args.next().is_some() {
        bail!("unexpected benchmark compare-json argument");
    }
    let load = |path: &str| -> Result<Run> {
        let run: Run = serde_json::from_slice(&std::fs::read(path)?)?;
        if run.version != 1 {
            bail!("Invalid benchmark result file: {path}");
        }
        Ok(run)
    };
    let comparison = compare(
        &load(&base)?,
        &load(&head)?,
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
    );
    if as_markdown {
        print!("{}", markdown(&comparison, &base, &head));
    } else {
        println!("{}", serde_json::to_string_pretty(&comparison)?);
    }
    if comparison.failed {
        bail!(
            "Historical benchmark comparison failed; this command is not the strict semantic-port performance gate"
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Threshold {
    pub max_regression_ratio: f64,
    pub min_absolute_regression: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(super) struct Metric {
    pub name: String,
    pub unit: String,
    pub samples: Vec<f64>,
    pub median: f64,
    pub p75: f64,
    pub p95: f64,
    pub min: f64,
    pub max: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threshold: Option<Threshold>,
    pub comparable: bool,
    pub source: String,
}

fn percentile(samples: &[f64], percent: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|left, right| (left - right).partial_cmp(&0.0).unwrap_or(Ordering::Equal));
    let index = ((percent / 100.0 * sorted.len() as f64).ceil() - 1.0)
        .max(0.0)
        .min((sorted.len() - 1) as f64) as usize;
    sorted[index]
}

fn classify(name: &str) -> (&'static str, bool, Option<Threshold>) {
    if name.starts_with("competitor_") {
        return ("ms", false, None);
    }
    if name.ends_with("_ms") {
        return (
            "ms",
            true,
            Some(Threshold {
                max_regression_ratio: 1.15,
                min_absolute_regression: 5.0,
            }),
        );
    }
    if name.starts_with("is_")
        || name.ends_with("_ready_before_move")
        || name.ends_with("_available")
    {
        return ("boolean", false, None);
    }
    if name.contains("rss") || name.contains("heap") {
        return (
            "bytes",
            true,
            Some(Threshold {
                max_regression_ratio: 1.2,
                min_absolute_regression: 8.0 * 1024.0 * 1024.0,
            }),
        );
    }
    if name.ends_with("_bytes") {
        return ("bytes", false, None);
    }
    ("count", false, None)
}

fn aggregate(source: &str, name: &str, samples: Vec<f64>) -> Metric {
    let (unit, comparable, threshold) = classify(name);
    Metric {
        name: format!("{source}/{name}"),
        source: source.into(),
        median: percentile(&samples, 50.0),
        p75: percentile(&samples, 75.0),
        p95: percentile(&samples, 95.0),
        min: percentile(&samples, 0.0),
        max: percentile(&samples, 100.0),
        samples,
        unit: unit.into(),
        comparable,
        threshold,
    }
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let command = args.next();
    if command.as_deref() == Some("huge-stream-diagnostic") {
        return huge_stream::run(args);
    }
    if command.as_deref() == Some("historical-release") {
        return historical::run(args);
    }
    if command.as_deref() == Some("geometry-memory") {
        return geometry_memory::run(args);
    }
    if command.as_deref() == Some("interaction-diagnostic") {
        return interaction_latency::run(args);
    }
    if command.as_deref() == Some("memory-snapshot") {
        return native_memory::run(args);
    }
    if command.as_deref() == Some("bootstrap-load") {
        return bootstrap::run(args);
    }
    if command.as_deref() == Some("changeset-parse") {
        return changeset_parse::run(args);
    }
    if command.as_deref() == Some("highlight-prefetch") {
        return highlight_prefetch::run(args);
    }
    if command.as_deref() == Some("large-stream") {
        return large_stream::run(args);
    }
    if command.as_deref() == Some("large-stream-profile") {
        return large_stream_profile::run(args);
    }
    if command.as_deref() == Some("non-ascii-stream") {
        return non_ascii_stream::run(args);
    }
    if command.as_deref() == Some("wrapped-cjk") {
        return wrapped_cjk::run(args);
    }
    if command.as_deref() == Some("terminal-width") {
        return terminal_width::run(args);
    }
    if command.as_deref() == Some("competitors") {
        return competitors::run(args);
    }
    if command.as_deref() == Some("run") {
        return runner::run_command(args);
    }
    if command.as_deref() == Some("runner-plan") {
        return runner::plan_command(args);
    }
    if command.as_deref() == Some("parse-metrics") {
        return runner::parse_command(args);
    }
    if command.as_deref() == Some("stream-fixture") {
        return stream::run(args);
    }
    if command.as_deref() == Some("render-layout") {
        return render_layout::run(args);
    }
    if command.as_deref() == Some("synthetic-patch") {
        return fixtures::run(args);
    }
    if command.as_deref() == Some("working-tree") {
        return working_tree::run(args);
    }
    if command.as_deref() == Some("compare-release") {
        return release::run(args);
    }
    if command.as_deref() == Some("release-plan") {
        return release_plan(args);
    }
    if command.as_deref() == Some("previous") {
        return previous_command(args);
    }
    if command.as_deref() == Some("compare-json") {
        return compare_files(args, false);
    }
    if command.as_deref() == Some("compare-markdown") {
        return compare_files(args, true);
    }
    if command.as_deref() != Some("aggregate") {
        bail!("benchmark requires aggregate SOURCE METRIC SAMPLES_JSON");
    }
    let source = args.next();
    let name = args.next();
    let samples = args.next();
    let (Some(source), Some(name), Some(samples)) = (source, name, samples) else {
        bail!("benchmark requires aggregate SOURCE METRIC SAMPLES_JSON");
    };
    if args.next().is_some() {
        bail!("unexpected benchmark aggregate argument");
    }
    let samples: Vec<f64> = serde_json::from_str(&samples)?;
    println!(
        "{}",
        serde_json::to_string(&aggregate(&source, &name, samples))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_benchmark_workflow_replaces_the_complete_pinned_bun_contract() {
        let repo = super::super::repo_root().unwrap();
        super::verify_workflow(&repo).unwrap();
    }

    #[test]
    fn native_terminal_width_source_is_verified_at_both_pins() {
        let repo = super::super::repo_root().unwrap();
        super::verify_terminal_width(&repo, BASELINE).unwrap();
    }

    #[test]
    fn native_geometry_memory_source_is_verified_at_both_pins() {
        let repo = super::super::repo_root().unwrap();
        super::verify_geometry_memory(&repo, BASELINE).unwrap();
    }

    #[test]
    fn native_benchmark_runner_source_is_verified_at_both_pins() {
        let repo = super::super::repo_root().unwrap();
        super::verify_runner(&repo, BASELINE).unwrap();
    }

    #[test]
    fn benchmark_run_round_trip_preserves_runtime_and_regression_provenance() {
        let value = serde_json::json!({
            "version": 1,
            "generatedAt": "2026-09-07T00:00:00.000Z",
            "gitSha": "source-commit",
            "packageVersion": "0.20.1",
            "runtime": {"bunVersion":"1.3.14", "platform":"darwin", "arch":"arm64"},
            "samplesPerBenchmark": 2.5,
            "acceptedRegressions": [{"name":"fixture/load_ms", "reason":"historical explanation, not a port waiver"}],
            "results": []
        });
        let run: Run = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&run).unwrap(), value);
        let native = serde_json::json!({"version":1,"runtime":{"platform":"linux","arch":"x64"},"results":[]});
        let run: Run = serde_json::from_value(native.clone()).unwrap();
        assert_eq!(serde_json::to_value(run).unwrap(), native);
        // Existing comparison inputs may omit metadata; do not fabricate provenance.
        let minimal = serde_json::json!({"version":1,"results":[]});
        let run: Run = serde_json::from_value(minimal.clone()).unwrap();
        assert_eq!(serde_json::to_value(run).unwrap(), minimal);
        let explicit_empty = serde_json::json!({"version":1,"acceptedRegressions":[],"results":[]});
        let run: Run = serde_json::from_value(explicit_empty.clone()).unwrap();
        assert_eq!(serde_json::to_value(run).unwrap(), explicit_empty);
    }

    #[test]
    fn pinned_historical_reports_retain_every_field_through_the_native_model() {
        fn equal(left: &serde_json::Value, right: &serde_json::Value) {
            use serde_json::Value;
            match (left, right) {
                (Value::Number(a), Value::Number(b)) => assert_eq!(a.as_f64(), b.as_f64()),
                (Value::Array(a), Value::Array(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (a, b) in a.iter().zip(b) {
                        equal(a, b);
                    }
                }
                (Value::Object(a), Value::Object(b)) => {
                    assert_eq!(a.len(), b.len());
                    for (key, value) in a {
                        equal(value, b.get(key).expect(key));
                    }
                }
                _ => assert_eq!(left, right),
            }
        }
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let git = |args: &[&str]| {
            let output = std::process::Command::new("git")
                .current_dir(root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8(output.stdout).unwrap()
        };
        let pin = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
        let paths = git(&["ls-tree", "--name-only", pin, "benchmarks/release/"]);
        let mut count = 0;
        for path in paths.lines().filter(|p| p.ends_with(".json")) {
            let source = git(&["show", &format!("{pin}:{path}")]);
            let value: serde_json::Value = serde_json::from_str(&source).unwrap();
            let run: Run = serde_json::from_str(&source).unwrap();
            equal(&value, &serde_json::to_value(run).unwrap());
            count += 1;
        }
        assert_eq!(count, 22);
    }

    #[test]
    fn explicit_release_output_survives_a_later_version_option() {
        let root = tempfile::tempdir().unwrap();
        let out = root.path().join("custom-release-benchmark.json");
        let options = release_run_options(
            root.path(),
            root.path(),
            "0.1.0".into(),
            None,
            [
                "--out".into(),
                out.to_string_lossy().into_owned(),
                "--version".into(),
                "0.16.0".into(),
            ]
            .into_iter(),
        )
        .unwrap();
        assert_eq!(options.version, "0.16.0");
        assert_eq!(options.out, out);
        assert_eq!(options.samples, 5.0);
        let normalized = release_run_options(
            root.path(),
            root.path(),
            "0.1.0".into(),
            None,
            ["--out", "nested/../custom.json"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(normalized.out, root.path().join("custom.json"));
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn release_plan_preserves_defaults_fractional_samples_and_argument_validation_order() {
        let root = tempfile::tempdir().unwrap();
        let parse = |args: &[&str], env| {
            release_run_options(
                root.path(),
                root.path(),
                "0.1.0".into(),
                env,
                args.iter().map(|s| (*s).to_owned()),
            )
        };
        let default = parse(&["--version", "0.2.0"], Some("3")).unwrap();
        assert_eq!(
            default.out,
            root.path().join("benchmarks/release/bench-0.2.0.json")
        );
        assert_eq!(default.samples, 3.0);
        assert_eq!(parse(&["--samples", "2.5"], None).unwrap().samples, 2.5);
        // Source validates a version while deriving a default output, not after an explicit output.
        assert!(parse(&["--version", "invalid"], None).is_err());
        assert!(parse(&["--out", "custom.json", "--version", "invalid"], None).is_ok());
        for args in [
            vec!["--samples", "0"],
            vec!["--samples", "NaN"],
            vec!["--samples", "Infinity"],
            vec!["--out"],
            vec!["--out", ""],
            vec!["--unknown"],
        ] {
            assert!(parse(&args, None).is_err());
        }
        for (value, expected) in [
            ("0x10", 16.0),
            ("0b11", 3.0),
            ("0o10", 8.0),
            ("\u{feff}2\u{a0}", 2.0),
            ("1e2", 100.0),
            ("", 0.0),
        ] {
            assert_eq!(sample_number(value), expected);
        }
        assert!(sample_number("inf").is_nan());
    }

    #[test]
    fn selects_latest_lower_stable_release_and_skips_prereleases_and_unrelated_names() {
        let root = tempfile::tempdir().unwrap();
        for version in ["0.14.1", "0.15.0", "0.15.3-beta.1", "0.15.3"] {
            std::fs::write(root.path().join(format!("bench-{version}.json")), b"{}\n").unwrap();
        }
        std::fs::write(root.path().join("bench-invalid.json"), b"{}").unwrap();
        assert_eq!(
            previous_release("0.15.4", root.path()).unwrap().unwrap().0,
            "0.15.3"
        );
        assert_eq!(
            previous_release("0.15.3-beta.2", root.path())
                .unwrap()
                .unwrap()
                .0,
            "0.15.0"
        );
        assert!(previous_release("0.14.1", root.path()).unwrap().is_none());
        assert!(
            previous_release("0.20.0", &root.path().join("missing"))
                .unwrap()
                .is_none()
        );
        assert!(previous_release("bad", &root.path().join("missing")).is_err());
        for invalid in ["v1.2.3", "1.2", "1.2.3+build", "1.2.3-", "１.2.3"] {
            assert!(release_version(invalid).is_err());
        }
        for valid in ["01.2.3", "1.2.3-beta.01", "1.2.3--"] {
            assert!(release_version(valid).is_ok());
        }
    }

    #[test]
    fn frozen_dual_pin_markdown_and_decimal_rounding_match_exactly() {
        assert_eq!(fixed(f64::INFINITY, 1), "Infinity");
        assert_eq!(fixed(f64::NEG_INFINITY, 1), "-Infinity");
        assert_eq!(fixed(f64::NAN, 1), "NaN");
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/benchmark-markdown.json"
        ))
        .unwrap();
        let base = serde_json::from_value(oracle["base"].clone()).unwrap();
        let head = serde_json::from_value(oracle["head"].clone()).unwrap();
        let rendered = markdown(&compare(&base, &head, "frozen".into()), "base", "head");
        assert_eq!(rendered, oracle["markdown"].as_str().unwrap());
        assert!(rendered.contains("+15% and +5.00 ms"));
        assert!(rendered.contains("+20% and +8.00 MiB"));
        for case in oracle["fixed"].as_array().unwrap() {
            assert_eq!(
                fixed(
                    case["value"].as_f64().unwrap(),
                    case["digits"].as_u64().unwrap() as u32
                ),
                case["expected"].as_str().unwrap(),
                "{case}"
            );
        }
    }

    #[test]
    fn frozen_dual_pin_comparisons_preserve_all_statuses_and_duplicate_resolution() {
        fn numeric_json(value: serde_json::Value) -> serde_json::Value {
            use serde_json::Value;
            match value {
                Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
                Value::Array(values) => {
                    Value::Array(values.into_iter().map(numeric_json).collect())
                }
                Value::Object(values) => Value::Object(
                    values
                        .into_iter()
                        .map(|(k, v)| (k, numeric_json(v)))
                        .collect(),
                ),
                other => other,
            }
        }
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/benchmark-comparison.json"
        ))
        .unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            let base = serde_json::from_value(case["base"].clone()).unwrap();
            let head = serde_json::from_value(case["head"].clone()).unwrap();
            let actual = serde_json::to_value(compare(&base, &head, "frozen".into())).unwrap();
            assert_eq!(
                numeric_json(actual),
                numeric_json(case["expected"].clone()),
                "{}",
                case["name"]
            );
        }
    }

    #[test]
    fn material_regression_requires_both_thresholds_and_positive_growth() {
        let threshold = Threshold {
            max_regression_ratio: 1.15,
            min_absolute_regression: 5.0,
        };
        for (base, head, expected) in [
            (100.0, 116.0, true),
            (100.0, 104.0, false),
            (10.0, 12.0, false),
            (100.0, 90.0, false),
            (0.0, 5.0, true),
            (0.0, 4.0, false),
        ] {
            assert_eq!(material_regression(base, head, &threshold), expected);
        }
    }

    #[test]
    fn comparison_cli_reads_versioned_files_and_propagates_failure() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path().join("base.json");
        let head = root.path().join("head.json");
        let document = |value| serde_json::json!({"version":1,"results":[aggregate("fixture","render_ms",vec![value]) ]});
        std::fs::write(&base, serde_json::to_vec(&document(100.0)).unwrap()).unwrap();
        std::fs::write(&head, serde_json::to_vec(&document(110.0)).unwrap()).unwrap();
        let args = || {
            [
                base.to_string_lossy().into_owned(),
                head.to_string_lossy().into_owned(),
            ]
            .into_iter()
        };
        compare_files(args(), false).unwrap();
        compare_files(args(), true).unwrap();
        std::fs::write(&head, serde_json::to_vec(&document(120.0)).unwrap()).unwrap();
        assert!(
            compare_files(args(), false)
                .unwrap_err()
                .to_string()
                .contains("Historical benchmark comparison failed")
        );
        std::fs::write(&head, b"{\"version\":2,\"results\":[]}").unwrap();
        assert!(
            compare_files(args(), false)
                .unwrap_err()
                .to_string()
                .contains("Invalid benchmark result file")
        );
        assert!(compare_files(std::iter::empty(), false).is_err());
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 2);
    }

    #[test]
    fn frozen_dual_pin_aggregation_matches_every_native_field() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../port/hunk/oracles/benchmark-aggregation.json"
        ))
        .unwrap();
        for expected in oracle["metrics"].as_array().unwrap() {
            let expected: Metric = serde_json::from_value(expected.clone()).unwrap();
            let name = expected.name.strip_prefix("fixture/").unwrap();
            assert_eq!(
                aggregate("fixture", name, vec![9.0, 1.0, 5.0, 3.0]),
                expected
            );
        }
        for case in oracle["percentiles"].as_array().unwrap() {
            assert_eq!(
                percentile(&[9.0, 1.0, 5.0, 3.0], case["percent"].as_f64().unwrap()),
                case["value"].as_f64().unwrap()
            );
        }
        let empty: Metric = serde_json::from_value(oracle["empty"].clone()).unwrap();
        assert_eq!(aggregate("fixture", "render_ms", vec![]), empty);
    }

    #[test]
    fn nearest_rank_keeps_input_order_and_handles_empty_and_boundary_percentiles() {
        let samples = [9.0, 1.0, 5.0, 3.0];
        for (percent, expected) in [
            (-10.0, 1.0),
            (0.0, 1.0),
            (50.0, 3.0),
            (75.0, 5.0),
            (95.0, 9.0),
            (110.0, 9.0),
        ] {
            assert_eq!(percentile(&samples, percent), expected);
        }
        assert_eq!(percentile(&[], 50.0), 0.0);
        let result = aggregate("fixture", "render_ms", samples.to_vec());
        assert_eq!(result.samples, samples);
        assert_eq!(
            (
                result.median,
                result.p75,
                result.p95,
                result.min,
                result.max
            ),
            (3.0, 5.0, 9.0, 1.0, 9.0)
        );
    }

    #[test]
    fn classification_preserves_ordered_source_rules() {
        for (name, unit, comparable) in [
            ("competitor_heap_ms", "ms", false),
            ("is_ready_ms", "ms", true),
            ("is_ready", "boolean", false),
            ("scroll_ready_before_move", "boolean", false),
            ("git_available", "boolean", false),
            ("peak_rss", "bytes", true),
            ("heap_used", "bytes", true),
            ("patch_bytes", "bytes", false),
            ("rows", "count", false),
        ] {
            let metric = aggregate("fixture", name, vec![]);
            assert_eq!(metric.unit, unit, "{name}");
            assert_eq!(metric.comparable, comparable, "{name}");
            assert_eq!(metric.threshold.is_some(), comparable, "{name}");
            assert_eq!(metric.min, 0.0);
            assert_eq!(metric.max, 0.0);
        }
        assert_eq!(classify("render_ms").2.unwrap().max_regression_ratio, 1.15);
        assert_eq!(
            classify("rss_bytes").2.unwrap().min_absolute_regression,
            8_388_608.0
        );
    }

    #[test]
    fn aggregate_cli_rejects_missing_extra_and_malformed_samples() {
        for args in [
            vec![],
            vec!["aggregate"],
            vec!["aggregate", "source", "metric", "null"],
            vec!["aggregate", "source", "metric", "[]", "extra"],
        ] {
            assert!(run(args.into_iter().map(str::to_owned)).is_err());
        }
        run(["aggregate", "source", "render_ms", "[9,1,5,3]"]
            .into_iter()
            .map(str::to_owned))
        .unwrap();
    }
}
