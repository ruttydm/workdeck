//! Native row-planning workloads from Hunk's MIT render-layout benchmark.
//! Uses the shared native stream fixtures, including full-source gap geometry.

#[cfg(test)]
use super::stream::file;
use super::*;
use std::time::Instant;
use workdeck_review::LayoutMode;
use workdeck_tui::{
    DiffSectionGeometryCache, DiffSectionGeometryOptions, ReviewRenderPlanOptions,
    build_review_render_plan, build_split_rows, build_stack_rows, resolve_theme,
};

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    files: usize,
    lines: usize,
    start: usize,
    end: usize,
}
const SCENARIOS: [Scenario; 3] = [
    Scenario {
        name: "many_small_files",
        files: 360,
        lines: 48,
        start: 37,
        end: 84,
    },
    Scenario {
        name: "balanced_stream",
        files: 180,
        lines: 120,
        start: 37,
        end: 84,
    },
    Scenario {
        name: "large_single_file",
        files: 1,
        lines: 18000,
        start: 1000,
        end: 17000,
    },
];

struct Measurement {
    name: &'static str,
    files: usize,
    split_rows: usize,
    stack_rows: usize,
    planned_rows: usize,
    split_ms: f64,
    stack_ms: f64,
    geometry_ms: f64,
    plan_ms: f64,
}

fn measure(scenario: Scenario) -> Result<Measurement> {
    let files = super::stream::files(
        scenario.files,
        scenario.lines,
        scenario.start,
        scenario.end,
        false,
    )?;
    let theme = resolve_theme(Some("midnight"), None, &[]);
    let started = Instant::now();
    let split_rows = files
        .iter()
        .map(|file| build_split_rows(file, None, &theme, 4).len())
        .sum();
    let split_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let stack_rows = files
        .iter()
        .map(|file| build_stack_rows(file, None, &theme, 4).len())
        .sum();
    let stack_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut geometry = DiffSectionGeometryCache::default();
    let started = Instant::now();
    for file in &files {
        std::hint::black_box(geometry.measure(DiffSectionGeometryOptions::new(
            file,
            LayoutMode::Split,
            &theme,
        )));
    }
    let geometry_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let planned_rows = files
        .iter()
        .map(|file| {
            let rows = build_split_rows(file, None, &theme, 4);
            build_review_render_plan(ReviewRenderPlanOptions::new(&file.runtime_id, &rows, true))
                .len()
        })
        .sum();
    let plan_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(Measurement {
        name: scenario.name,
        files: files.len(),
        split_rows,
        stack_rows,
        planned_rows,
        split_ms,
        stack_ms,
        geometry_ms,
        plan_ms,
    })
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark render-layout accepts no arguments");
    }
    for scenario in SCENARIOS {
        let result = measure(scenario)?;
        for (name, value) in [
            ("split_rows", result.split_ms),
            ("stack_rows", result.stack_ms),
            ("geometry", result.geometry_ms),
            ("review_plan", result.plan_ms),
        ] {
            println!("METRIC {}_{name}_ms={}", result.name, fixed(value, 2));
        }
        for (name, value) in [
            ("files", result.files),
            ("split_rows", result.split_rows),
            ("stack_rows", result.stack_rows),
            ("planned_rows", result.planned_rows),
        ] {
            println!("METRIC {}_{name}={value}", result.name);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_native_row_counts_match_both_pinned_workloads() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-render-layout.json"
        ))
        .unwrap();
        let cases = oracle["scenarios"].as_array().unwrap();
        assert_eq!(cases.len(), SCENARIOS.len());
        for (scenario, expected) in SCENARIOS.into_iter().zip(cases) {
            let result = measure(scenario).unwrap();
            assert_eq!(
                (
                    result.files,
                    result.split_rows,
                    result.stack_rows,
                    result.planned_rows
                ),
                (
                    expected["files"].as_u64().unwrap() as usize,
                    expected["split_rows"].as_u64().unwrap() as usize,
                    expected["stack_rows"].as_u64().unwrap() as usize,
                    expected["planned_rows"].as_u64().unwrap() as usize
                ),
                "{}",
                scenario.name
            );
        }
    }

    #[test]
    fn stream_fixture_keeps_declared_statistics_full_sources_and_decorations() {
        let file = file(1, 48, 37, 84, true).unwrap();
        assert_eq!(file.runtime_id, "stream:1");
        assert!(!file.flags.partial);
        assert_eq!(file.stats.additions, 48);
        assert_eq!(file.stats.deletions, 48);
        assert!(file.patch.is_empty());
        let before = &file.sources.old.as_ref().unwrap().content;
        let after = &file.sources.new.as_ref().unwrap().content;
        assert_eq!(before.lines().count(), 48);
        assert_eq!(after.lines().count(), 48);
        assert!(before.starts_with(
            "export function stream1_1(value: number) { return value + 1; } // 한국어 주석\n"
        ));
        assert!(after.contains("return value * 37 + 1;"));
    }
}
