//! Partial MIT port of Hunk benchmarks/geometry-memory.ts.
//! Native diagnostics are not substitutes for JavaScript heap statistics.
use super::*;
use std::time::Instant;
use workdeck_review::LayoutMode;
use workdeck_tui::{DiffSectionGeometryCache, DiffSectionGeometryOptions, resolve_theme};

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

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark geometry-memory currently accepts no arguments");
    }
    native_memory::snapshot()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&measure(
            stream::DEFAULT_FILE_COUNT,
            stream::DEFAULT_LINES_PER_FILE,
            240,
            stream::GIANT_SINGLE_FILE_LINES,
            true
        )?)?
    );
    Ok(())
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
