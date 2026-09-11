//! Native daemon/session memory workload.
//!
//! This is the Rust counterpart of Hunk's daemon-memory-check harness.  The
//! native server is exercised through its authenticated session protocol; the
//! workload deliberately keeps the same register/update/API/cleanup shape but
//! does not spawn a JavaScript runtime or invent a managed-heap metric.

use super::*;
use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use workdeck_core::{ChangesetSource, ReviewSide};
use workdeck_review::{CommentAnchor, ReviewComment, ReviewState};
use workdeck_session::{ReviewSessionServer, SessionAction, SessionClient};

const DEFAULT_CYCLES: usize = 50;
const DEFAULT_WARMUP_CYCLES: usize = 5;
const DEFAULT_SESSIONS_PER_CYCLE: usize = 2;
const DEFAULT_FILES_PER_SESSION: usize = 30;
const DEFAULT_HUNKS_PER_FILE: usize = 4;
const DEFAULT_LINES_PER_HUNK: usize = 18;
const DEFAULT_API_REQUESTS_PER_CYCLE: usize = 4;
const DEFAULT_SNAPSHOT_UPDATES_PER_CYCLE: usize = 3;
const DEFAULT_SETTLE_MS: usize = 50;
const DEFAULT_MAX_RSS_GROWTH_MB: usize = 96;
const DEFAULT_MAX_RSS_SLOPE_KB: usize = 768;

const SOURCE_PATH: &str = "scripts/daemon-memory-check.ts";
const SOURCE_BYTES: usize = 21_227;
const SOURCE_LINES: usize = 624;
const SOURCE_SHA256: &str = "72130a835789b27384af8fdb6d9d8700387622bd8648754f59347866ab116ee4";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Options {
    cycles: usize,
    warmup_cycles: usize,
    sessions_per_cycle: usize,
    files_per_session: usize,
    hunks_per_file: usize,
    lines_per_hunk: usize,
    api_requests_per_cycle: usize,
    snapshot_updates_per_cycle: usize,
    settle_ms: usize,
    max_rss_growth_mb: usize,
    max_rss_slope_kb: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    json_out: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            cycles: DEFAULT_CYCLES,
            warmup_cycles: DEFAULT_WARMUP_CYCLES,
            sessions_per_cycle: DEFAULT_SESSIONS_PER_CYCLE,
            files_per_session: DEFAULT_FILES_PER_SESSION,
            hunks_per_file: DEFAULT_HUNKS_PER_FILE,
            lines_per_hunk: DEFAULT_LINES_PER_HUNK,
            api_requests_per_cycle: DEFAULT_API_REQUESTS_PER_CYCLE,
            snapshot_updates_per_cycle: DEFAULT_SNAPSHOT_UPDATES_PER_CYCLE,
            settle_ms: DEFAULT_SETTLE_MS,
            max_rss_growth_mb: DEFAULT_MAX_RSS_GROWTH_MB,
            max_rss_slope_kb: DEFAULT_MAX_RSS_SLOPE_KB,
            json_out: None,
        }
    }
}

fn number(name: &str, value: &str) -> Result<usize> {
    let parsed = value
        .parse::<f64>()
        .with_context(|| format!("Expected {name} to be a non-negative number."))?;
    ensure!(
        parsed.is_finite() && parsed >= 0.0,
        "Expected {name} to be a non-negative number."
    );
    Ok(parsed.trunc() as usize)
}

fn parse(args: impl Iterator<Item = String>) -> Result<Options> {
    let mut options = Options::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .filter(|value| !value.is_empty())
                .with_context(|| format!("Missing value for {arg}."))
        };
        match arg.as_str() {
            "--help" | "-h" => {
                println!("cargo xtask benchmark daemon-memory [options]");
                println!("  --cycles N --warmup-cycles N --sessions-per-cycle N");
                println!("  --files-per-session N --hunks-per-file N --lines-per-hunk N");
                println!("  --api-requests-per-cycle N --snapshot-updates-per-cycle N");
                println!(
                    "  --settle-ms N --max-rss-growth-mb N --max-rss-slope-kb N --json-out PATH"
                );
                return Ok(options);
            }
            "--cycles" => options.cycles = number(&arg, &value()?)?,
            "--warmup-cycles" => options.warmup_cycles = number(&arg, &value()?)?,
            "--sessions-per-cycle" => options.sessions_per_cycle = number(&arg, &value()?)?,
            "--files-per-session" => options.files_per_session = number(&arg, &value()?)?,
            "--hunks-per-file" => options.hunks_per_file = number(&arg, &value()?)?,
            "--lines-per-hunk" => options.lines_per_hunk = number(&arg, &value()?)?,
            "--api-requests-per-cycle" => options.api_requests_per_cycle = number(&arg, &value()?)?,
            "--snapshot-updates-per-cycle" => {
                options.snapshot_updates_per_cycle = number(&arg, &value()?)?
            }
            "--settle-ms" => options.settle_ms = number(&arg, &value()?)?,
            "--max-rss-growth-mb" => options.max_rss_growth_mb = number(&arg, &value()?)?,
            "--max-rss-slope-kb" => options.max_rss_slope_kb = number(&arg, &value()?)?,
            "--json-out" => options.json_out = Some(PathBuf::from(value()?)),
            _ => bail!("Unknown option: {arg}"),
        }
    }
    options.warmup_cycles = options.warmup_cycles.min(options.cycles.saturating_sub(1));
    Ok(options)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Sample {
    label: &'static str,
    cycle: usize,
    rss_bytes: u64,
    heap_bytes: Option<u64>,
    high_water_rss_bytes: Option<u64>,
    sessions: usize,
    pending_commands: usize,
}

fn sample(label: &'static str, cycle: usize, sessions: usize) -> Result<Sample> {
    let memory = native_memory::snapshot()?;
    Ok(Sample {
        label,
        cycle,
        rss_bytes: memory.rss_bytes,
        heap_bytes: None,
        high_water_rss_bytes: Some(native_memory::peak_rss_bytes()?),
        sessions,
        pending_commands: 0,
    })
}

fn slope(samples: &[Sample]) -> f64 {
    if samples.len() < 2 {
        return 0.0;
    }
    let mean_x = samples
        .iter()
        .map(|sample| sample.cycle as f64)
        .sum::<f64>()
        / samples.len() as f64;
    let mean_y = samples
        .iter()
        .map(|sample| sample.rss_bytes as f64)
        .sum::<f64>()
        / samples.len() as f64;
    let numerator = samples.iter().fold(0.0, |sum, sample| {
        sum + (sample.cycle as f64 - mean_x) * (sample.rss_bytes as f64 - mean_y)
    });
    let denominator = samples.iter().fold(0.0, |sum, sample| {
        sum + (sample.cycle as f64 - mean_x).powi(2)
    });
    if denominator == 0.0 {
        0.0
    } else {
        numerator / denominator
    }
}

fn changeset(session_id: &str, options: &Options) -> Result<workdeck_core::Changeset> {
    let lines = options
        .lines_per_hunk
        .saturating_mul(options.hunks_per_file.saturating_add(3))
        .max(1);
    let mut files = (0..options.files_per_session)
        .map(|index| stream::file(index, lines, 1, options.lines_per_hunk.max(1), false))
        .collect::<Result<Vec<_>>>()?;
    for file in &mut files {
        // Session comments address the provider-neutral key.  The benchmark
        // fixture uses path keys so every cycle has a stable, inspectable
        // target independent of invocation-specific runtime identities.
        file.key = file.path.clone();
    }
    Ok(workdeck_core::Changeset {
        id: format!("daemon-memory:{session_id}"),
        source_label: format!("/tmp/workdeck-daemon-memory/{session_id}"),
        title: format!("memory test {session_id}"),
        summary: None,
        agent_summary: None,
        source: ChangesetSource::WorkingTree { staged: false },
        files,
    })
}

fn comment(session_id: &str, cycle: usize, update: usize) -> ReviewComment {
    // `stream::file` is the production fixture generator and names the first
    // file `src/stream0.ts`; use that canonical key so the server validates the
    // comment against the actual changeset rather than accepting a fake path.
    let path = "src/stream0.ts".to_owned();
    ReviewComment {
        id: format!("{session_id}-comment-{cycle}-{update}"),
        parent_id: None,
        source: "daemon-memory-check".into(),
        author: Some("daemon-memory-check".into()),
        created_at: Some(format!("cycle-{cycle}-{update}")),
        file_path: Some(path.clone()),
        hunk_index: Some(0),
        side: Some(ReviewSide::New),
        line: Some(1),
        summary: format!("Memory harness comment {cycle}.{update}"),
        rationale: Some(format!(
            "Snapshot churn rationale {session_id} {cycle} {update}"
        )),
        markup: None,
        title: None,
        tags: Vec::new(),
        confidence: None,
        updated_at: None,
        resolution: Default::default(),
        anchor: CommentAnchor {
            file_key: path,
            old_range: None,
            new_range: None,
            preferred_side: Some(ReviewSide::New),
            preferred_line: Some(1),
            intersecting_hunk_indices: vec![0],
            owner_hunk_index: Some(0),
        },
        editable: true,
    }
}

fn exercise_session(
    server: &ReviewSessionServer,
    session_id: &str,
    options: &Options,
    cycle: usize,
) -> Result<()> {
    let descriptor = server.descriptor();
    SessionClient::request(descriptor, SessionAction::Health)?;
    SessionClient::request(descriptor, SessionAction::Snapshot)?;
    for update in 0..options.snapshot_updates_per_cycle {
        SessionClient::request(
            descriptor,
            SessionAction::Review {
                include_patch: true,
                include_source: true,
                include_agent_context: true,
            },
        )?;
        SessionClient::request(
            descriptor,
            SessionAction::CommentAdd {
                comment: Box::new(comment(session_id, cycle, update)),
            },
        )?;
        SessionClient::request(descriptor, SessionAction::CommentList)?;
        SessionClient::request(descriptor, SessionAction::NavigateFile { file_index: 0 })?;
    }
    for request in 0..options.api_requests_per_cycle {
        match request % 4 {
            0 => {
                SessionClient::request(descriptor, SessionAction::Snapshot)?;
            }
            1 => {
                SessionClient::request(descriptor, SessionAction::CommentList)?;
            }
            2 => {
                SessionClient::request(
                    descriptor,
                    SessionAction::Review {
                        include_patch: true,
                        include_source: false,
                        include_agent_context: false,
                    },
                )?;
            }
            _ => {
                SessionClient::request(descriptor, SessionAction::Reload)?;
            }
        }
    }
    Ok(())
}

fn run_workload(options: &Options) -> Result<serde_json::Value> {
    let root = tempfile::tempdir()?;
    let discovery = root.path().join("sessions");
    let mut samples = vec![sample("startup", 0, 0)?];
    for cycle in 1..=options.cycles {
        let mut servers = Vec::new();
        let mut ids = Vec::new();
        for session_index in 0..options.sessions_per_cycle {
            let id = format!("memory-{cycle}-{session_index}");
            let state = Arc::new(Mutex::new(ReviewState::new(changeset(&id, options)?)));
            servers.push(ReviewSessionServer::spawn(
                state,
                PathBuf::from(format!(
                    "/tmp/workdeck-daemon-memory/{cycle}/{session_index}"
                )),
                discovery.clone(),
            )?);
            ids.push(id);
        }
        for (server, id) in servers.iter().zip(&ids) {
            exercise_session(server, id, options, cycle)?;
        }
        samples.push(sample("live", cycle, servers.len())?);
        drop(servers);
        std::thread::sleep(Duration::from_millis(options.settle_ms as u64));
        samples.push(sample("cleanup", cycle, 0)?);
    }
    let cleanup = samples
        .iter()
        .filter(|sample| sample.label == "cleanup")
        .cloned()
        .collect::<Vec<_>>();
    let analyzed = cleanup
        .iter()
        .filter(|sample| sample.cycle > options.warmup_cycles)
        .cloned()
        .collect::<Vec<_>>();
    let first = analyzed
        .first()
        .or_else(|| cleanup.first())
        .or_else(|| samples.first())
        .context("memory workload did not produce samples")?;
    let last = analyzed
        .last()
        .or_else(|| cleanup.last())
        .or_else(|| samples.last())
        .context("memory workload did not produce a final sample")?;
    let growth = i128::from(last.rss_bytes) - i128::from(first.rss_bytes);
    let slope = slope(&analyzed);
    let max_rss = samples
        .iter()
        .map(|sample| sample.rss_bytes)
        .max()
        .unwrap_or_default();
    let max_hwm = samples
        .iter()
        .filter_map(|sample| sample.high_water_rss_bytes)
        .max()
        .unwrap_or_default();
    let passed = growth <= i128::from(options.max_rss_growth_mb as u64) * 1024 * 1024
        && slope <= (options.max_rss_slope_kb as f64) * 1024.0;
    Ok(serde_json::json!({
        "options": options,
        "sampleCount": samples.len(),
        "analyzedCleanupSamples": analyzed.len(),
        "firstAnalyzedRssBytes": first.rss_bytes,
        "lastAnalyzedRssBytes": last.rss_bytes,
        "growthBytes": growth,
        "slopeBytesPerCycle": slope,
        "maxRssBytes": max_rss,
        "maxHwmBytes": max_hwm,
        "passed": passed,
        "samples": samples,
        "memorySemantics": "Current native RSS and lifetime high-water resident bytes; managed JavaScript heap is not fabricated."
    }))
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let options = parse(args)?;
    let report = run_workload(&options)?;
    println!("METRIC daemon_rss_growth_bytes={}", report["growthBytes"]);
    println!(
        "METRIC daemon_rss_slope_bytes_per_cycle={}",
        report["slopeBytesPerCycle"]
    );
    println!("METRIC daemon_max_rss_bytes={}", report["maxRssBytes"]);
    println!("METRIC daemon_max_hwm_bytes={}", report["maxHwmBytes"]);
    if let Some(path) = &options.json_out {
        let mut bytes = serde_json::to_vec_pretty(&report)?;
        bytes.push(b'\n');
        fs::write(path, bytes)?;
        println!("wrote {}", path.display());
    }
    ensure!(
        report["passed"].as_bool().unwrap_or(false),
        "daemon memory growth exceeded configured threshold"
    );
    Ok(())
}

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    ensure!(
        baseline == super::BASELINE,
        "daemon-memory verifier received unexpected baseline {baseline}"
    );
    for pin in [super::BASELINE, "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd"] {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{pin}:{SOURCE_PATH}")])?;
        ensure!(
            source.len() == SOURCE_BYTES,
            "pinned {SOURCE_PATH} {pin} changed size: {} != {SOURCE_BYTES}",
            source.len()
        );
        ensure!(
            source.split(|byte| *byte == b'\n').count() == SOURCE_LINES + 1,
            "pinned {SOURCE_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", sha2::Sha256::digest(&source)) == SOURCE_SHA256,
            "pinned {SOURCE_PATH} {pin} changed SHA-256"
        );
    }
    for (path, marker) in [
        ("xtask/src/benchmark/daemon_memory.rs", "fn run_workload("),
        (
            "xtask/src/benchmark/daemon_memory.rs",
            "SessionAction::CommentAdd",
        ),
        ("xtask/src/benchmark.rs", "Some(\"daemon-memory\")"),
        ("docs/daemon-memory-benchmark-migration.md", "daemon-memory"),
    ] {
        let contents = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read daemon-memory native surface {path}"))?;
        ensure!(
            contents.contains(marker),
            "daemon-memory native surface {path} is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_defaults_clamps_warmup_and_rejects_unknown_or_negative_values() {
        let defaults = parse(std::iter::empty()).unwrap();
        assert_eq!(defaults.cycles, DEFAULT_CYCLES);
        assert_eq!(defaults.warmup_cycles, DEFAULT_WARMUP_CYCLES);
        let options = parse(
            [
                "--cycles",
                "2",
                "--warmup-cycles",
                "99",
                "--sessions-per-cycle",
                "1",
                "--json-out",
                "report.json",
            ]
            .into_iter()
            .map(str::to_owned),
        )
        .unwrap();
        assert_eq!(options.warmup_cycles, 1);
        assert_eq!(options.json_out, Some(PathBuf::from("report.json")));
        assert!(parse(["--cycles", "-1"].into_iter().map(str::to_owned)).is_err());
        assert!(parse(["--unknown"].into_iter().map(str::to_owned)).is_err());
    }

    #[test]
    fn reduced_workload_exercises_authenticated_sessions_and_reports_cleanup_metrics() {
        let options = Options {
            cycles: 2,
            warmup_cycles: 0,
            sessions_per_cycle: 2,
            files_per_session: 3,
            hunks_per_file: 2,
            lines_per_hunk: 4,
            api_requests_per_cycle: 4,
            snapshot_updates_per_cycle: 2,
            settle_ms: 0,
            max_rss_growth_mb: 512,
            max_rss_slope_kb: 1024 * 1024,
            json_out: None,
        };
        let report = run_workload(&options).unwrap();
        assert_eq!(report["sampleCount"], 5);
        assert_eq!(report["analyzedCleanupSamples"], 2);
        assert!(report["maxRssBytes"].as_u64().unwrap() > 0);
        assert!(report["maxHwmBytes"].as_u64().unwrap() > 0);
        assert!(
            report["samples"]
                .as_array()
                .unwrap()
                .iter()
                .any(|sample| sample["label"] == "live")
        );
    }

    #[test]
    fn slope_handles_single_and_constant_samples_without_division_errors() {
        assert_eq!(slope(&[]), 0.0);
        assert_eq!(
            slope(&[Sample {
                label: "cleanup",
                cycle: 1,
                rss_bytes: 1,
                heap_bytes: None,
                high_water_rss_bytes: None,
                sessions: 0,
                pending_commands: 0
            }]),
            0.0
        );
        let samples = (0..3)
            .map(|cycle| Sample {
                label: "cleanup",
                cycle,
                rss_bytes: 100,
                heap_bytes: None,
                high_water_rss_bytes: None,
                sessions: 0,
                pending_commands: 0,
            })
            .collect::<Vec<_>>();
        assert_eq!(slope(&samples), 0.0);
    }

    #[test]
    fn pinned_daemon_memory_source_is_verified_from_both_anchors() {
        verify(&crate::repo_root().unwrap(), super::BASELINE).unwrap();
    }
}
