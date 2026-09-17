//! Reproducible, editor-authored datasets; never uses a real repository/backlog.
//! cargo run --release -p workdeck-pm --example projection_bench -- features 40000
//! cargo run --release -p workdeck-pm --example projection_bench -- issues 10000
use serde_json::json;
use std::{fs, path::Path, time::Instant};
use workdeck_pm::{projection::*, *};

fn document<T: serde::Serialize>(metadata: &T, body: &str) -> Vec<u8> {
    format!(
        "---\n{}---\n{body}",
        serde_yaml_ng::to_string(metadata).unwrap()
    )
    .into_bytes()
}

fn write(path: &Path, bytes: &[u8]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, bytes).unwrap();
}

fn feature_id(index: usize) -> FeatureId {
    format!("FEAT-{index:026}").parse().unwrap()
}
fn issue_id(index: usize) -> IssueId {
    format!("WD-{index:026}").parse().unwrap()
}

fn percentile(samples: &[f64], percent: usize) -> f64 {
    let mut ordered = samples.to_vec();
    ordered.sort_by(f64::total_cmp);
    ordered[(ordered.len() * percent).div_ceil(100).saturating_sub(1)]
}

// These are qualification budgets for the documented full-size synthetic
// workloads. They are intentionally generous enough to cover the measured
// native filesystem and registered-policy runs while still catching a clear
// regression. Warm query feedback remains an interaction budget; refresh is a
// bounded background operation over the complete captured source.
const WARM_FILTER_P95_MAX_MS: f64 = 100.0;
const FEATURES_INCREMENTAL_REFRESH_MAX_MS: f64 = 30_000.0;
const ISSUES_INCREMENTAL_REFRESH_MAX_MS: f64 = 15_000.0;

fn peak_rss_bytes() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
            return None;
        }
        let peak = u64::try_from(unsafe { usage.assume_init() }.ru_maxrss).ok()?;
        #[cfg(target_os = "macos")]
        return Some(peak);
        #[cfg(not(target_os = "macos"))]
        return Some(peak.saturating_mul(1024));
    }
    #[cfg(not(unix))]
    None
}

fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(2..=4).contains(&args.len())
        || !matches!(args[0].as_str(), "features" | "issues")
        || args[2..].iter().any(|argument| {
            !matches!(
                argument.as_str(),
                "--enforce-targets" | "--registered-policy"
            )
        })
        || args[2..]
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != args.len() - 2
    {
        return Err("usage: projection_bench features COUNT | issues COUNT [--enforce-targets] [--registered-policy]".into());
    }
    let enforce_targets = args[2..]
        .iter()
        .any(|argument| argument == "--enforce-targets");
    let registered_policy = args[2..]
        .iter()
        .any(|argument| argument == "--registered-policy");
    let family = args[0].as_str();
    let count: usize = args[1].parse()?;
    let maximum = if family == "features" { 40_000 } else { 10_000 };
    if count == 0 || count > maximum {
        return Err(format!("count must be 1..={maximum}").into());
    }
    let temporary = tempfile::tempdir()?;
    let repository = Repository::init(temporary.path(), "WD")?;
    if registered_policy {
        for actor in ["agent-a", "agent-b"] {
            let mut user = UserDefinition::new(actor);
            user.kind = UserKind::Agent;
            repository.mutate_user(
                actor,
                None,
                &UserMutation::Create { user },
                &RequestId::new(),
            )?;
        }
        repository.set_identity_mode(IdentityMode::Registered, None, &RequestId::new())?;
    }
    let config = repository.config()?;
    let timestamp: Timestamp = "2026-09-09T00:00:00Z".parse()?;
    let created = Instant::now();
    let mut bytes = 0usize;
    let mut edges = 0usize;
    let mut edit = None;
    for index in 0..count {
        let (relative, original, changed) = if family == "features" {
            let id = feature_id(index);
            let mut metadata: FeatureMetadata = serde_json::from_value(json!({
                "schema": 1, "repository": repository.identity(), "id": id,
                "revision": 1, "name": format!("Capability {index:05} bucket {}", index % 10),
                "created_at": timestamp, "updated_at": timestamp,
                "parent": (index > 0).then(|| feature_id((index - 1) / 8)),
                "prerequisites": if index >= 16 && index % 16 == 0 { vec![feature_id(index - 8)] } else { vec![] },
                "lead": if index % 2 == 0 { "agent-a" } else { "agent-b" }
            }))?;
            edges += usize::from(metadata.parent.is_some()) + metadata.prerequisites.len();
            metadata.validate()?;
            let original = document(&metadata, "# Capability\nEditor-authored scale fixture.\n");
            metadata.name.push_str(" edited");
            metadata.revision = metadata.revision.next()?;
            metadata.updated_at = "2026-09-09T00:01:00Z".parse()?;
            (
                format!("features/{id}.md"),
                original,
                document(&metadata, "# Capability\nEditor-authored scale fixture.\n"),
            )
        } else {
            let id = issue_id(index);
            let mut metadata = IssueMetadata::new(
                &config,
                &format!("Work item {index:05} bucket {}", index % 10),
                timestamp,
            )?;
            metadata.id = id.clone();
            metadata.assignee = Some(if index % 2 == 0 { "agent-a" } else { "agent-b" }.into());
            metadata.parent = (index > 0).then(|| issue_id((index - 1) / 8));
            if index >= 16 && index % 16 == 0 {
                metadata.prerequisites.push(issue_id(index - 8));
            }
            edges += usize::from(metadata.parent.is_some()) + metadata.prerequisites.len();
            metadata.validate(&config)?;
            let original = document(&metadata, "# Work\nEditor-authored scale fixture.\n");
            metadata.title.push_str(" edited");
            metadata.revision = metadata.revision.next()?;
            metadata.updated_at = "2026-09-09T00:01:00Z".parse()?;
            (
                format!("issues/{id}/item.md"),
                original,
                document(&metadata, "# Work\nEditor-authored scale fixture.\n"),
            )
        };
        let path = repository.root().join(relative);
        bytes += original.len();
        write(&path, &original);
        if index == count / 2 {
            edit = Some((path, changed));
        }
    }
    let dataset_ms = created.elapsed().as_secs_f64() * 1000.0;
    let before_rss = peak_rss_bytes();
    let limits = ProjectionLimits::default();
    let mut store = ProjectionStore::open(
        temporary.path(),
        SourceSelector::WorkingTree,
        limits.clone(),
    )?;
    let start = Instant::now();
    let mut cold_stages = Vec::new();
    let view = match store.refresh_with_faults(&ProjectionRefreshRequest { rebuild: true }, |point| {
        cold_stages.push(json!({"point":format!("{point:?}"), "elapsed_ms":start.elapsed().as_secs_f64()*1000.0}));
        Ok(())
    })? {
        ProjectionRefresh::Published(view) => view,
        _ => return Err("fresh source did not publish an index".into()),
    };
    let cold_ms = start.elapsed().as_secs_f64() * 1000.0;
    let mut samples = Vec::with_capacity(190);
    let mut uncached_samples = Vec::with_capacity(10);
    let base = if family == "features" {
        ProjectionQuery::Features {
            query: ProjectionFeatureQuery::default(),
        }
    } else {
        ProjectionQuery::default()
    };
    let handle = view.query(&base)?;
    if handle.total != count {
        return Err(format!("expected {count} records, indexed {}", handle.total).into());
    }
    let first = view.page(&handle, 0, count.min(80))?;
    let last = view.page(&handle, count.saturating_sub(80), count.min(80))?;
    for sample in 0..200 {
        let query = if family == "features" {
            ProjectionQuery::Features {
                query: ProjectionFeatureQuery {
                    query: format!("bucket {}", sample % 10),
                    ..Default::default()
                },
            }
        } else {
            ProjectionQuery::Issues {
                query: IssueQuery {
                    query: format!("bucket {}", sample % 10),
                    ..Default::default()
                },
                group_by: None,
            }
        };
        let start = Instant::now();
        let handle = view.query(&query)?;
        let page = view.page(&handle, 0, handle.total.clamp(1, 80))?;
        std::hint::black_box(page);
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        if sample < 10 {
            uncached_samples.push(elapsed);
        } else {
            samples.push(elapsed);
        }
    }
    let start = Instant::now();
    let mut reopened =
        ProjectionStore::open(temporary.path(), SourceSelector::WorkingTree, limits)?;
    let loaded = reopened.load()?.ok_or("checkpoint did not reload")?;
    let loaded_handle = loaded.query(&base)?;
    let loaded_first = loaded.page(&loaded_handle, 0, count.min(80))?;
    if loaded_first.rows != first.rows {
        return Err("reloaded query differs from original generation".into());
    }
    let reopen_ms = start.elapsed().as_secs_f64() * 1000.0;
    let (path, changed) = edit.unwrap();
    write(&path, &changed);
    let start = Instant::now();
    let mut incremental_stages = Vec::new();
    let refreshed = match store.refresh_with_faults(&ProjectionRefreshRequest::default(), |point| {
        incremental_stages.push(json!({"point":format!("{point:?}"), "elapsed_ms":start.elapsed().as_secs_f64()*1000.0}));
        Ok(())
    })? {
        ProjectionRefresh::Published(view) => view,
        _ => return Err("edited source did not publish a new generation".into()),
    };
    let refresh_ms = start.elapsed().as_secs_f64() * 1000.0;
    if view.id() == refreshed.id() {
        return Err("edit did not change the generation".into());
    }
    if view.page(&handle, 0, count.min(80))? != first {
        return Err("refresh modified an existing read handle".into());
    }
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "dataset": { "version": 1, "family": family, "records": count, "edges": edges, "authored_bytes": bytes, "shape": "8-ary parents; every sixteenth record requires record i-8", "creation_ms": dataset_ms, "authority": "direct editor records; no fabricated mutation receipts", "organization_policy": if registered_policy { "registered identities" } else { "open" } },
            "environment": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "parallelism": std::thread::available_parallelism().map(|n| n.get()).ok(), "debug_assertions": cfg!(debug_assertions) },
            "cold_index_ms": cold_ms, "checkpoint_reopen_and_query_ms": reopen_ms,
            "incremental_refresh_ms": refresh_ms,
            "stage_timings": { "metric": "cumulative milliseconds since refresh start", "cold": cold_stages, "incremental": incremental_stages },
            "uncached_filter_page": { "samples": uncached_samples.len(), "p50_ms": percentile(&uncached_samples,50), "p95_ms": percentile(&uncached_samples,95), "max_ms": uncached_samples.iter().copied().fold(0.0,f64::max) },
            "warm_filter_page": { "samples": samples.len(), "p50_ms": percentile(&samples,50), "p95_ms": percentile(&samples,95), "max_ms": samples.iter().copied().fold(0.0,f64::max) },
            "memory": { "metric": "process high-water RSS; includes fixture creation and retained old/new/checkpoint readers", "before_index_bytes": before_rss, "peak_bytes": peak_rss_bytes() },
            "visible_rows": { "first": first.rows.len(), "last": last.rows.len() },
            "projection": view.id(), "refreshed_projection": refreshed.id()
        }))?
    );
    // Emit measured evidence even when the qualification gate fails. The
    // refresh budgets are calibrated against the documented native baselines;
    // they are not a promise that every smaller or richer workload has the
    // same latency.
    let refresh_target_ms = if family == "features" {
        FEATURES_INCREMENTAL_REFRESH_MAX_MS
    } else {
        ISSUES_INCREMENTAL_REFRESH_MAX_MS
    };
    if enforce_targets
        && (percentile(&samples, 95) > WARM_FILTER_P95_MAX_MS || refresh_ms > refresh_target_ms)
    {
        return Err(format!(
            "PM-10 performance targets unmet: warm p95 {:.3} ms (maximum {:.0}), incremental {:.3} ms (maximum {:.0} for {})",
            percentile(&samples, 95),
            WARM_FILTER_P95_MAX_MS,
            refresh_ms,
            refresh_target_ms,
            family
        ).into());
    }
    Ok(())
}
