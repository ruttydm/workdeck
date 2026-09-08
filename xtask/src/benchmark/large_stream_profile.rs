//! MIT translation of Hunk benchmarks/large-stream-profile.ts (Modem Labs Inc.).
//! Time the three pure planning stages before any terminal renderer is involved.

use super::*;
use std::time::Instant;
use workdeck_review::LayoutMode;
use workdeck_tui::{
    DiffSectionGeometryCache, DiffSectionGeometryOptions, ReviewRenderPlanOptions,
    build_review_render_plan, build_split_rows, resolve_theme,
};

fn measure() -> Result<Vec<(&'static str, f64)>> {
    let theme = resolve_theme(Some("midnight"), None, &[]);
    let files = stream::files(
        stream::DEFAULT_FILE_COUNT,
        stream::DEFAULT_LINES_PER_FILE,
        37,
        84,
        false,
    )?;
    let mut geometry = DiffSectionGeometryCache::default();
    let start = Instant::now();
    for file in &files {
        std::hint::black_box(geometry.measure(DiffSectionGeometryOptions::new(
            file,
            LayoutMode::Split,
            &theme,
        )));
    }
    let geometry_ms = start.elapsed().as_secs_f64() * 1000.0;

    let mut split_rows = 0;
    let start = Instant::now();
    for file in &files {
        split_rows += build_split_rows(file, None, &theme, 4).len();
    }
    let split_ms = start.elapsed().as_secs_f64() * 1000.0;

    let mut planned_rows = 0;
    let start = Instant::now();
    for file in &files {
        let rows = build_split_rows(file, None, &theme, 4);
        planned_rows +=
            build_review_render_plan(ReviewRenderPlanOptions::new(&file.runtime_id, &rows, true))
                .len();
    }
    let plan_ms = start.elapsed().as_secs_f64() * 1000.0;
    Ok(vec![
        ("section_geometry_ms", geometry_ms),
        ("split_rows_ms", split_ms),
        ("review_plan_ms", plan_ms),
        ("split_rows", split_rows as f64),
        ("planned_rows", planned_rows as f64),
        ("files", stream::DEFAULT_FILE_COUNT as f64),
        ("lines_per_file", stream::DEFAULT_LINES_PER_FILE as f64),
    ])
}

fn output(metrics: &[(&str, f64)]) -> String {
    metrics
        .iter()
        .map(|(name, value)| {
            let value = if name.ends_with("_ms") {
                fixed(*value, 2)
            } else {
                value.to_string()
            };
            format!("METRIC {name}={value}\n")
        })
        .collect()
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("large-stream-profile accepts no arguments");
    }
    print!("{}", output(&measure()?));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_three_stage_profile_matches_both_pinned_oracles() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-large-stream-profile.json"
        ))
        .unwrap();
        let measured = measure().unwrap();
        let actual = runner::parse_metrics(&output(&measured));
        assert_eq!(actual.len(), 7);
        let runs = oracle["runs"].as_array().unwrap();
        assert_eq!(runs.len(), 2);
        for run in runs {
            assert_eq!(run["exitCode"], 0);
            let expected = runner::parse_metrics(run["combinedOutput"].as_str().unwrap());
            assert_eq!(expected.len(), actual.len());
            for ((name, value), (expected_name, expected_value)) in actual.iter().zip(&expected) {
                assert_eq!(name, expected_name);
                if name.ends_with("_ms") {
                    assert!(value.is_finite() && *value >= 0.0);
                } else {
                    assert_eq!(value, expected_value, "{name}");
                }
            }
        }
        for (_, value) in measured.iter().take(3) {
            assert!(value.is_finite() && *value > 0.0);
        }
    }

    #[test]
    fn prints_source_metric_order_and_precision() {
        assert_eq!(
            output(&[("section_geometry_ms", 1.25), ("split_rows", 10260.0)]),
            "METRIC section_geometry_ms=1.25\nMETRIC split_rows=10260\n"
        );
        assert!(run(["unexpected".into()].into_iter()).is_err());
    }
}
