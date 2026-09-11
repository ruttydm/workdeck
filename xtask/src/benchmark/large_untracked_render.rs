//! Native Ratatui replacement for Hunk's large untracked-file render check.
//!
//! The workload deliberately enters through the Git provider and the production `ReviewApp`;
//! it does not manufacture a `DiffFile` or inspect a renderer-only fixture. This keeps the
//! bounded large-file policy, path handling, and terminal message on the same path a user sees.

use super::*;
use anyhow::{Context, Result, bail, ensure};
use ratatui::{buffer::Buffer, layout::Rect};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use workdeck_core::{CliInput, CommonOptions, InputLayoutMode, VcsDiffCommandInput};
use workdeck_review::LayoutMode;
use workdeck_tui::{ReviewApp, ReviewOptions, render, resolve_theme};
use workdeck_vcs::{
    VcsReviewInput, bundled_vcs_catalog, get_vcs_adapter, load_selected_vcs_changeset_deferred,
};

const SOURCE_PATH: &str = "scripts/test-large-untracked-render.tsx";
const SOURCE_BYTES: usize = 3_135;
const SOURCE_LINES: usize = 94;
const SOURCE_SHA256: &str = "f3b7b71e44149c753d8dfafea08b1c29976e17c79660b8a37364512f4868b4f0";
const BASELINE: &str = "2c00f4358b89cfc0a6b04459ffc538ba601aa3c2";
const STABLE: &str = "4ae6f8f6c8afbdbabcc037e0e0e7fff85d41d6fd";
const DEFAULT_LINE_COUNT: usize = 100_000;
const VIEWPORT: Rect = Rect::new(0, 0, 120, 30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FixtureKind {
    Untracked,
    Tracked,
}

impl FixtureKind {
    fn parse(value: Option<&str>) -> Result<Self> {
        match value {
            None | Some("untracked") => Ok(Self::Untracked),
            Some("tracked") => Ok(Self::Tracked),
            Some(other) => bail!("fixture kind must be `untracked` or `tracked`, got {other:?}"),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Untracked => "untracked",
            Self::Tracked => "tracked",
        }
    }
}

fn create_large_file_body(line_count: usize) -> String {
    let mut body = String::with_capacity(line_count.saturating_mul(2));
    for index in 0..line_count {
        let line = if index == 0 {
            "visible-first-line"
        } else if index + 1 == line_count {
            "the widest generated line"
        } else {
            "x"
        };
        body.push_str(line);
        body.push('\n');
    }
    body
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<(usize, FixtureKind)> {
    let line_count = match args.next() {
        None => DEFAULT_LINE_COUNT,
        Some(value) => value.parse::<usize>().with_context(
            || "Usage: cargo xtask benchmark large-untracked-render [line-count] [tracked]",
        )?,
    };
    ensure!(line_count > 0, "line-count must be positive");
    let kind = FixtureKind::parse(args.next().as_deref())?;
    ensure!(args.next().is_none(), "too many arguments");
    Ok((line_count, kind))
}

fn fixture(line_count: usize, kind: FixtureKind) -> Result<(tempfile::TempDir, String)> {
    let repo = fixtures::temporary("workdeck-large-untracked-render-")?;
    fixtures::git(repo.path(), &["init", "--initial-branch", "main"])?;
    fixtures::git(repo.path(), &["config", "user.name", "Workdeck Benchmark"])?;
    fixtures::git(
        repo.path(),
        &["config", "user.email", "benchmark@example.com"],
    )?;
    fixtures::git(repo.path(), &["config", "commit.gpgsign", "false"])?;
    fs::write(repo.path().join("tracked.txt"), "tracked\n")?;
    let path = if kind == FixtureKind::Tracked {
        "large-tracked.txt"
    } else {
        "large-untracked.txt"
    };
    if kind == FixtureKind::Tracked {
        fs::write(repo.path().join(path), "original\n")?;
    }
    fixtures::git(repo.path(), &["add", "."])?;
    fixtures::git(repo.path(), &["commit", "-m", "initial"])?;
    fs::write(repo.path().join(path), create_large_file_body(line_count))?;
    Ok((repo, path.to_owned()))
}

fn load(repo: &Path) -> Result<workdeck_vcs::LoadedVcsChangeset> {
    let input = VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions {
            mode: Some(InputLayoutMode::Stack),
            ..CommonOptions::default()
        },
    };
    let catalog = bundled_vcs_catalog();
    let adapter = get_vcs_adapter("git", catalog)?;
    Ok(load_selected_vcs_changeset_deferred(
        repo,
        adapter,
        catalog,
        &VcsReviewInput::Diff(input),
    )?)
}

fn frame_text(buffer: &Buffer) -> String {
    let width = usize::from(buffer.area.width);
    let mut text = String::with_capacity(buffer.content.len().saturating_mul(2));
    for row in buffer.content.chunks(width.max(1)) {
        for cell in row {
            text.push_str(cell.symbol());
        }
        text.push('\n');
    }
    text
}

#[derive(Debug, Serialize)]
struct ResultSummary {
    contains_header: bool,
    contains_path: bool,
    contains_skipped_large_message: bool,
    contains_visible_line: bool,
    file_count: usize,
    first_file_stats: Option<workdeck_core::FileStats>,
    first_file_stats_truncated: Option<bool>,
    fixture_kind: &'static str,
    line_count: usize,
    rendered_frame_bytes: usize,
}

fn run_fixture(line_count: usize, kind: FixtureKind) -> Result<ResultSummary> {
    let (repo, path) = fixture(line_count, kind)?;
    let loaded = load(repo.path())?;
    let input = VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: Vec::new(),
        options: CommonOptions {
            mode: Some(InputLayoutMode::Stack),
            ..CommonOptions::default()
        },
    };
    let first = loaded.changeset.files.first().cloned();
    let app = ReviewApp::new(
        loaded.changeset.clone(),
        ReviewOptions {
            layout: LayoutMode::Stack,
            theme: resolve_theme(Some("midnight"), None, &[]),
            command_cwd: Some(repo.path().to_owned()),
            repo: Some(repo.path().to_owned()),
            review_input: Some(CliInput::Vcs(input)),
            source_capabilities: Some(loaded.source_capabilities),
            ..ReviewOptions::default()
        },
    );
    let mut buffer = Buffer::empty(VIEWPORT);
    render(VIEWPORT, &mut buffer, &app);
    let frame = frame_text(&buffer);
    let header = format!("@@ -0,0 +1,{line_count} @@");
    Ok(ResultSummary {
        contains_header: frame.contains(&header),
        contains_path: frame.contains(&path),
        contains_skipped_large_message: frame.contains("File too large to render")
            || frame.contains("Skipped because the file is too large")
            || frame.contains("File exceeds review limits"),
        contains_visible_line: frame.contains("visible-first-line"),
        file_count: loaded.changeset.files.len(),
        first_file_stats: first.as_ref().map(|file| file.stats.clone()),
        first_file_stats_truncated: first.as_ref().map(|file| file.stats.truncated),
        fixture_kind: kind.name(),
        line_count,
        rendered_frame_bytes: frame.len(),
    })
}

pub(super) fn run(args: impl Iterator<Item = String>) -> Result<()> {
    let (line_count, kind) = parse_args(args)?;
    let summary = run_fixture(line_count, kind)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

pub(crate) fn verify(repo: &Path, baseline: &str) -> Result<()> {
    if baseline != BASELINE {
        return Ok(());
    }
    for (pin, expected_bytes, expected_lines) in [
        (BASELINE, SOURCE_BYTES, SOURCE_LINES),
        (STABLE, SOURCE_BYTES, SOURCE_LINES),
    ] {
        let source = crate::git_stdout_bytes(repo, ["show", &format!("{pin}:{SOURCE_PATH}")])?;
        ensure!(
            source.len() == expected_bytes,
            "pinned {SOURCE_PATH} {pin} changed size"
        );
        ensure!(
            source.split(|byte| *byte == b'\n').count() == expected_lines + 1,
            "pinned {SOURCE_PATH} {pin} changed line count"
        );
        ensure!(
            format!("{:x}", Sha256::digest(&source)) == SOURCE_SHA256,
            "pinned {SOURCE_PATH} {pin} changed SHA-256"
        );
    }
    for (path, marker) in [
        (
            "xtask/src/benchmark/large_untracked_render.rs",
            "create_large_file_body",
        ),
        (
            "xtask/src/benchmark/large_untracked_render.rs",
            "contains_skipped_large_message",
        ),
        ("xtask/src/main.rs", "benchmark large-untracked-render"),
        ("docs/large-untracked-render-migration.md", "Ratatui"),
    ] {
        let contents = fs::read_to_string(repo.join(path))
            .with_context(|| format!("read large-untracked-render native surface {path}"))?;
        ensure!(
            contents.contains(marker),
            "large-untracked-render native surface {path} is missing {marker:?}"
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reduced_untracked_workload_uses_git_provider_and_bounded_rendering() {
        let result = run_fixture(20_001, FixtureKind::Untracked).unwrap();
        assert!(result.contains_path);
        assert!(result.contains_skipped_large_message);
        assert!(!result.contains_header);
        assert_eq!(result.file_count, 1);
        assert_eq!(result.first_file_stats.as_ref().unwrap().additions, 20_001);
        assert_eq!(result.first_file_stats_truncated, Some(false));
    }

    #[test]
    fn tracked_and_untracked_fixtures_keep_the_same_large_file_policy() {
        for kind in [FixtureKind::Tracked, FixtureKind::Untracked] {
            let result = run_fixture(20_001, kind).unwrap();
            assert!(result.contains_path);
            assert!(result.contains_skipped_large_message);
        }
    }

    #[test]
    fn parser_matches_script_shape_and_rejects_invalid_inputs() {
        assert_eq!(
            parse_args(Vec::<String>::new().into_iter()).unwrap(),
            (DEFAULT_LINE_COUNT, FixtureKind::Untracked)
        );
        assert_eq!(
            parse_args(["10", "tracked"].into_iter().map(str::to_owned)).unwrap(),
            (10, FixtureKind::Tracked)
        );
        assert!(parse_args(["0"].into_iter().map(str::to_owned)).is_err());
        assert!(parse_args(["10", "other"].into_iter().map(str::to_owned)).is_err());
        assert!(parse_args(["not-a-number"].into_iter().map(str::to_owned)).is_err());
    }

    #[test]
    fn pinned_source_capture_is_checked_from_both_anchors() {
        verify(&crate::repo_root().unwrap(), BASELINE).unwrap();
    }
}
