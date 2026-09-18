//! Native MIT port of Hunk's `benchmarks/highlight-cache-layers.ts` workload.
//!
//! This workload keeps the production terminal and worker cache boundaries visible: the first
//! request is cold, the immediate second request is a decoded terminal-cache hit, and seven other
//! large files evict the first decoded result while leaving the compact worker LRU resident.

use super::*;
use std::time::{Duration, Instant};
use workdeck_core::DiffFile;
use workdeck_diff::{
    FileComparisonOptions, FileSnapshot, HighlightedDiffCode, diff_from_file_snapshots,
};
use workdeck_tui::{AppTheme, HighlightedDiffRuntime, highlighted_diff_cache_key, resolve_theme};

const FILE_COUNT: usize = 8;
const CHANGED_LINES: usize = 8_000;
// The source awaits the worker promise; use a wall-clock ceiling rather than a
// poll count so faster native polling does not shorten the allowed settle time.
const MAX_WAIT: Duration = Duration::from_secs(10);

fn file(index: usize) -> Result<DiffFile> {
    let additions = (0..CHANGED_LINES)
        .map(|line| format!("export const marker{index}_{line} = {line};\n"))
        .collect::<String>();
    let path = format!("src/benchmark-{index}.ts");
    let before = "export const prior = 1;\n";
    let after = format!("export const prior = {index};\n{additions}\n");
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            name: &path,
            contents: before,
            cache_key: &format!("before:{index}"),
        },
        FileSnapshot {
            name: &path,
            contents: &after,
            cache_key: &format!("after:{index}"),
        },
        FileComparisonOptions { context_radius: 3 },
    )?;
    // The source metadata is intentionally alias-context only, matching the worker benchmark's
    // explicit `aliasContext: true` request and avoiding a source-backed highlight plan.
    file.sources = workdeck_core::FileSourceSnapshots::default();
    file.flags.partial = true;
    file.language = Some("typescript".into());
    file.runtime_id = format!("benchmark:{index}");
    file.refresh_identity();
    Ok(file)
}

fn request(
    runtime: &mut HighlightedDiffRuntime,
    file: &DiffFile,
    theme: &AppTheme,
) -> Result<(f64, HighlightedDiffCode)> {
    let started = Instant::now();
    let deadline = Instant::now() + MAX_WAIT;
    let highlighted = loop {
        let highlighted = runtime.prefetch_highlighted_diff(file, theme, true);
        if highlighted.is_some() || Instant::now() >= deadline {
            break highlighted;
        }
        std::thread::yield_now();
    };
    let highlighted = highlighted.context("native highlight worker did not settle")?;
    Ok((started.elapsed().as_secs_f64() * 1_000.0, highlighted))
}

fn measure() -> Result<(f64, f64, f64, usize, usize)> {
    let theme = resolve_theme(Some("github-dark-default"), None, &[]);
    let files = (0..FILE_COUNT).map(file).collect::<Result<Vec<_>>>()?;
    let first = files.first().context("expected benchmark files")?;
    let mut runtime = HighlightedDiffRuntime::default();

    let (cold_ms, cold) = request(&mut runtime, first, &theme)?;
    let first_key = highlighted_diff_cache_key(&theme, first);
    ensure!(
        runtime
            .resolve_snapshot(Some(first), &theme, None, None)
            .is_some(),
        "cold result did not enter the terminal cache"
    );
    let (main_cache_hit_ms, main_hit) = request(&mut runtime, first, &theme)?;
    ensure!(
        cold == main_hit,
        "terminal cache hit changed the highlighted result"
    );

    // Eight 8k-line entries exceed the decoded terminal cache's line budget while fitting the
    // compact worker budget. The first terminal entry must be gone after these seven requests.
    for file in files.iter().skip(1) {
        let _ = request(&mut runtime, file, &theme)?;
    }
    ensure!(
        runtime
            .resolve_snapshot(Some(first), &theme, None, None)
            .is_none(),
        "terminal cache retained the first entry past its line budget ({first_key})"
    );
    let worker_cache_bytes = runtime.engine_mut().worker_cache_bytes();
    ensure!(
        worker_cache_bytes > 0,
        "worker cache did not retain compact results after terminal eviction"
    );
    let (worker_cache_hit_after_main_eviction_ms, worker_hit) =
        request(&mut runtime, first, &theme)?;
    ensure!(
        cold == worker_hit,
        "worker cache revisit changed the highlighted result"
    );
    runtime.dispose_worker();
    Ok((
        cold_ms,
        main_cache_hit_ms,
        worker_cache_hit_after_main_eviction_ms,
        FILE_COUNT,
        worker_cache_bytes,
    ))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark highlight-cache-layers accepts no arguments");
    }
    let (
        cold_ms,
        main_cache_hit_ms,
        worker_cache_hit_after_main_eviction_ms,
        files,
        _worker_cache_bytes,
    ) = measure()?;
    println!("METRIC cold_ms={}", fixed(cold_ms, 2));
    println!("METRIC main_cache_hit_ms={}", fixed(main_cache_hit_ms, 2));
    println!(
        "METRIC worker_cache_hit_after_main_eviction_ms={}",
        fixed(worker_cache_hit_after_main_eviction_ms, 2)
    );
    println!("METRIC files={files}");
    println!("METRIC changed_lines={CHANGED_LINES}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_cache_layers_distinguish_terminal_hit_from_worker_revisit() {
        let (cold_ms, main_cache_hit_ms, worker_hit_ms, files, worker_bytes) = measure().unwrap();
        assert!(cold_ms.is_finite() && cold_ms > 0.0);
        assert!(main_cache_hit_ms.is_finite() && main_cache_hit_ms > 0.0);
        assert!(worker_hit_ms.is_finite() && worker_hit_ms > 0.0);
        assert_eq!(files, FILE_COUNT);
        assert!(worker_bytes > 0);
    }

    #[test]
    fn source_files_retain_unique_paths_and_large_worker_inputs() {
        let first = file(0).unwrap();
        let second = file(1).unwrap();
        assert_ne!(first.path, second.path);
        assert!(first.flags.partial);
        assert!(first.sources.old.is_none() && first.sources.new.is_none());
        assert!(
            first.hunks[0]
                .lines
                .iter()
                .filter(|line| line.new_line.is_some())
                .count()
                >= CHANGED_LINES
        );
    }

    #[test]
    fn highlight_cache_layers_rejects_options() {
        assert!(run(["--unexpected".into()].into_iter()).is_err());
    }
}
