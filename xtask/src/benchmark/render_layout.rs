//! Native row-planning workloads from Hunk's MIT render-layout benchmark.
//! The containing stream fixture remains partial until huge/bootstrap helpers are translated.

use super::*;
use std::time::Instant;
use workdeck_core::{DiffFile, FileSourceSnapshots, SourceOrigin, SourceSnapshot};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};
use workdeck_review::LayoutMode;
use workdeck_tui::{
    DiffSectionGeometryCache, DiffSectionGeometryOptions, ReviewRenderPlanOptions,
    build_review_render_plan, build_split_rows, build_stack_rows, resolve_theme,
};

const DECORATIONS: [&str; 6] = [
    "日本語のコメント",
    "中文注释内容",
    "한국어 주석",
    "🚀✨🔧💡",
    "┌──┬──┐│▌▾│└──┴──┘",
    "héllo wörld — naïve café",
];

fn line(index: usize, line: usize, changed: bool, non_ascii: bool) -> String {
    let body = if changed {
        format!(
            "export function stream{index}_{line}(value: number) {{ return value * {line} + {index}; }}"
        )
    } else {
        format!("export function stream{index}_{line}(value: number) {{ return value + {line}; }}")
    };
    if non_ascii {
        format!(
            "{body} // {}\n",
            DECORATIONS[(index + line) % DECORATIONS.len()]
        )
    } else {
        format!("{body}\n")
    }
}

fn file(index: usize, lines: usize, start: usize, end: usize, non_ascii: bool) -> Result<DiffFile> {
    let path = format!("src/stream{index}.ts");
    let before: String = (1..=lines)
        .map(|n| line(index, n, false, non_ascii))
        .collect();
    let after: String = (1..=lines)
        .map(|n| line(index, n, (start..=end).contains(&n), non_ascii))
        .collect();
    let variant = if non_ascii { "non-ascii" } else { "ascii" };
    let before_key = format!("stream:{index}:before:{lines}:{variant}");
    let after_key = format!("stream:{index}:after:{lines}:{variant}");
    let mut file = diff_from_file_snapshots(
        FileSnapshot {
            name: &path,
            contents: &before,
            cache_key: &before_key,
        },
        FileSnapshot {
            name: &path,
            contents: &after,
            cache_key: &after_key,
        },
        FileComparisonOptions { context_radius: 3 },
    )?;
    file.runtime_id = format!("stream:{index}");
    file.patch.clear();
    file.language = Some("typescript".into());
    // Preserve the fixture's declared range statistics even when its range exceeds the source.
    file.stats.additions = end.saturating_add(1).saturating_sub(start);
    file.stats.deletions = file.stats.additions;
    file.set_sources(FileSourceSnapshots {
        old: Some(SourceSnapshot::new(
            before,
            SourceOrigin::File { path: path.clone() },
            true,
        )),
        new: Some(SourceSnapshot::new(
            after,
            SourceOrigin::File { path },
            true,
        )),
    });
    Ok(file)
}

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
    let files = (1..=scenario.files)
        .map(|index| file(index, scenario.lines, scenario.start, scenario.end, false))
        .collect::<Result<Vec<_>>>()?;
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
