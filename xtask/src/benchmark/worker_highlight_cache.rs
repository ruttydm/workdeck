//! Native MIT port of Hunk's `benchmarks/worker-highlight-cache.ts` workload.
//!
//! The production syntect/Oniguruma worker owns a compact result LRU. The benchmark first
//! settles a large TypeScript diff, releases only the decoded terminal cache, and then measures
//! the same immutable metadata through the worker-owned cache. No JavaScript runtime or synthetic
//! highlight payload is involved.

use super::*;
use std::time::{Duration, Instant};
use workdeck_diff::{
    FileComparisonOptions, FileSnapshot, HighlightAppearance, HighlightedDiffLine, HighlightedFile,
    PIERRE_DARK_THEME, diff_from_file_snapshots,
};

const CHANGED_LINES: usize = 8_000;
// The source awaits the worker promise; use a wall-clock ceiling rather than a
// poll count so faster native polling does not shorten the allowed settle time.
const MAX_WAIT: Duration = Duration::from_secs(10);

fn file() -> Result<workdeck_core::DiffFile> {
    let additions = (0..CHANGED_LINES)
        .map(|index| format!("export const marker{index} = {index};\n"))
        .collect::<String>();
    let before = "export const prior = 1;\n";
    let after = format!("export const prior = 2;\n{additions}\n");
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            name: "large.ts",
            contents: before,
            cache_key: "measure:before",
        },
        FileSnapshot {
            name: "large.ts",
            contents: &after,
            cache_key: "measure:after",
        },
        FileComparisonOptions { context_radius: 3 },
    )?;

    // The source workload submits parsed metadata with aliasContext=true. Removing complete
    // source capabilities and retaining partial patch semantics selects that same worker path;
    // the hunk lines remain the authoritative text for token reconstruction.
    file.sources = workdeck_core::FileSourceSnapshots::default();
    file.flags.partial = true;
    file.language = Some("typescript".into());
    file.runtime_id = "worker-highlight-cache".into();
    file.refresh_identity();
    Ok(file)
}

fn request(
    cache: &mut workdeck_diff::HighlightCache,
    file: &workdeck_core::DiffFile,
) -> Result<(f64, HighlightedFile)> {
    let started = Instant::now();
    let deadline = Instant::now() + MAX_WAIT;
    let highlighted = loop {
        let highlighted = cache.highlight_with_syntax_theme_live(
            file,
            HighlightAppearance::Dark,
            Some(PIERRE_DARK_THEME),
            &[],
        );
        if highlighted.is_some() || Instant::now() >= deadline {
            break highlighted;
        }
        std::thread::yield_now();
    };
    let highlighted = highlighted.context("native highlight worker did not settle")?;
    Ok((started.elapsed().as_secs_f64() * 1_000.0, highlighted))
}

fn addition_run_count(file: &HighlightedFile) -> usize {
    file.iter()
        .flatten()
        .filter_map(|line: &HighlightedDiffLine| line.addition.as_ref())
        .map(Vec::len)
        .sum()
}

fn measure() -> Result<(f64, f64, usize, usize)> {
    let file = file()?;
    let mut cache = workdeck_diff::HighlightCache::default();
    let (cold_ms, cold) = request(&mut cache, &file)?;
    let compact_payload_bytes = cache.worker_cache_bytes();
    ensure!(
        compact_payload_bytes > 0,
        "worker cache retained no compact payload"
    );
    let cold_runs = addition_run_count(&cold);

    // Force the second request through the worker-owned compact LRU instead of the terminal cache.
    cache.clear_rendered_diff_cache();
    let (warm_ms, warm) = request(&mut cache, &file)?;
    let warm_runs = addition_run_count(&warm);
    ensure!(
        cold_runs == warm_runs,
        "worker cache changed syntax run count"
    );
    ensure!(warm_runs > 0, "worker produced no addition syntax runs");
    cache.dispose_worker();
    Ok((cold_ms, warm_ms, compact_payload_bytes, warm_runs))
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark worker-highlight-cache accepts no arguments");
    }
    let (cold_ms, warm_ms, compact_payload_bytes, _) = measure()?;
    println!("METRIC cold_ms={}", fixed(cold_ms, 2));
    println!("METRIC worker_cache_hit_ms={}", fixed(warm_ms, 2));
    println!("METRIC compact_payload_bytes={compact_payload_bytes}");
    println!("METRIC changed_lines={CHANGED_LINES}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_worker_cache_reuses_compact_payload_after_terminal_eviction() {
        let (cold_ms, warm_ms, payload_bytes, syntax_runs) = measure().unwrap();
        assert!(cold_ms.is_finite() && cold_ms > 0.0);
        assert!(warm_ms.is_finite() && warm_ms > 0.0);
        assert!(payload_bytes > 0);
        assert!(syntax_runs > 0);
    }

    #[test]
    fn worker_highlight_cache_rejects_options() {
        assert!(run(["--unexpected".into()].into_iter()).is_err());
    }

    #[test]
    fn source_file_selects_alias_context_worker_mode() {
        let file = file().unwrap();
        assert!(file.flags.partial);
        assert!(file.sources.old.is_none() && file.sources.new.is_none());
        assert_eq!(file.language.as_deref(), Some("typescript"));
        assert_eq!(file.hunks.len(), 1);
        assert!(
            file.hunks[0]
                .lines
                .iter()
                .filter(|line| line.new_line.is_some())
                .count()
                >= CHANGED_LINES
        );
    }
}
