//! Partial MIT port of Hunk's working-tree benchmark and shared synthetic fixtures.
//! Measures the native catalog/materialization path; full application bootstrap remains separate.

use super::*;
use std::time::Instant;
use workdeck_core::{ChangesetSource, CommonOptions, InputLayoutMode, VcsDiffCommandInput};
use workdeck_vcs::{
    VcsLoadContext, VcsReviewInput, bundled_vcs_catalog, get_vcs_adapter, load_vcs_review,
    materialize_vcs_patch_result, operation_from_input,
};

#[derive(Clone, Copy)]
struct Scenario {
    name: &'static str,
    files: usize,
    lines: usize,
    untracked_files: usize,
    untracked_lines: usize,
}

const SCENARIOS: [Scenario; 5] = [
    Scenario {
        name: "small_worktree",
        files: 16,
        lines: 80,
        untracked_files: 0,
        untracked_lines: 0,
    },
    Scenario {
        name: "medium_worktree",
        files: 96,
        lines: 180,
        untracked_files: 0,
        untracked_lines: 0,
    },
    Scenario {
        name: "large_worktree",
        files: 240,
        lines: 220,
        untracked_files: 0,
        untracked_lines: 0,
    },
    Scenario {
        name: "untracked_many_small",
        files: 16,
        lines: 80,
        untracked_files: 120,
        untracked_lines: 36,
    },
    Scenario {
        name: "untracked_few_large",
        files: 8,
        lines: 80,
        untracked_files: 6,
        untracked_lines: 5000,
    },
];

#[cfg(test)]
fn synthetic_source(index: usize, changed: bool, lines: usize) -> String {
    fixtures::source(
        index,
        changed,
        &fixtures::Options {
            file_count: 1.0,
            lines: lines as f64,
            changed_start: None,
            changed_lines: None,
            extension: "ts".into(),
            prefix: "src/bench".into(),
        },
    )
}

fn fixture(scenario: Scenario) -> Result<tempfile::TempDir> {
    let root = fixtures::changed_repo(&fixtures::Options {
        file_count: scenario.files as f64,
        lines: scenario.lines as f64,
        changed_start: None,
        changed_lines: None,
        extension: "ts".into(),
        prefix: "src/bench".into(),
    })?;
    fixtures::add_untracked(
        root.path(),
        scenario.untracked_files as f64,
        scenario.untracked_lines as f64,
    )?;
    Ok(root)
}

#[derive(Debug, Serialize)]
struct Measurement {
    name: &'static str,
    load_ms: f64,
    files: usize,
    additions: usize,
    deletions: usize,
}

fn measure(scenario: Scenario) -> Result<Measurement> {
    let fixture = fixture(scenario)?;
    let start = Instant::now();
    let catalog = bundled_vcs_catalog();
    let adapter = get_vcs_adapter("git", catalog)?;
    let operation = operation_from_input(VcsReviewInput::Diff(VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: vec![],
        options: CommonOptions {
            mode: Some(InputLayoutMode::Auto),
            ..CommonOptions::default()
        },
    }));
    let result = load_vcs_review(
        adapter,
        &operation,
        &VcsLoadContext {
            cwd: fixture.path().to_owned(),
        },
        catalog,
    )?;
    let changeset = materialize_vcs_patch_result(
        result,
        "benchmark:working",
        ChangesetSource::WorkingTree { staged: false },
    )?;
    let load_ms = start.elapsed().as_secs_f64() * 1000.0;
    let measurement = Measurement {
        name: scenario.name,
        load_ms,
        files: changeset.files.len(),
        additions: changeset.files.iter().map(|f| f.stats.additions).sum(),
        deletions: changeset.files.iter().map(|f| f.stats.deletions).sum(),
    };
    if fixture.path().join(".agents").exists() {
        bail!("benchmark viewing created repository state");
    }
    Ok(measurement)
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark working-tree accepts no arguments");
    }
    for scenario in SCENARIOS {
        let result = measure(scenario)?;
        println!(
            "METRIC {}_load_ms={}",
            result.name,
            fixed(result.load_ms, 2)
        );
        println!("METRIC {}_files={}", result.name, result.files);
        println!("METRIC {}_additions={}", result.name, result.additions);
        println!("METRIC {}_deletions={}", result.name, result.deletions);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::git;
    use super::*;
    use std::fs;

    #[test]
    fn native_working_tree_scenarios_match_both_pinned_structural_counts() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-working-tree.json"
        ))
        .unwrap();
        let cases = oracle["scenarios"].as_array().unwrap();
        assert_eq!(cases.len(), SCENARIOS.len());
        for (scenario, expected) in SCENARIOS.into_iter().zip(cases) {
            let result = measure(scenario).unwrap();
            assert_eq!(result.name, expected["name"].as_str().unwrap());
            assert_eq!(
                (result.files, result.additions, result.deletions),
                (
                    expected["files"].as_u64().unwrap() as usize,
                    expected["additions"].as_u64().unwrap() as usize,
                    expected["deletions"].as_u64().unwrap() as usize
                ),
                "{}",
                scenario.name
            );
            assert!(result.load_ms.is_finite() && result.load_ms >= 0.0);
        }
    }

    #[test]
    fn fixture_retains_committed_before_bytes_and_cleans_up_after_drop() {
        let scenario = Scenario {
            name: "fixture",
            files: 1,
            lines: 12,
            untracked_files: 1,
            untracked_lines: 6,
        };
        let root = fixture(scenario).unwrap();
        assert_eq!(
            git(root.path(), &["show", "HEAD:src/bench1.ts"]).unwrap(),
            synthetic_source(1, false, 12)
        );
        assert_eq!(
            fs::read_to_string(root.path().join("src/bench1.ts")).unwrap(),
            synthetic_source(1, true, 12)
        );
        assert_eq!(
            fs::read_to_string(root.path().join("untracked/new1.ts")).unwrap(),
            synthetic_source(1, true, 6)
        );
        let path = root.path().to_owned();
        drop(root);
        assert!(!path.exists());
    }
}
