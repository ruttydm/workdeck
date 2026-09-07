//! Native port of Hunk's MIT large-stream benchmark fixtures.

use super::*;
use workdeck_core::{
    AppBootstrap, Changeset, ChangesetSource, CliInput, CommonOptions, DiffFile,
    FileSourceSnapshots, InputLayoutMode, ReloadContext, SourceOrigin, SourceSnapshot,
    VcsDiffCommandInput,
};
use workdeck_diff::{FileComparisonOptions, FileSnapshot, diff_from_file_snapshots};

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

pub(super) fn file(
    index: usize,
    lines: usize,
    start: usize,
    end: usize,
    non_ascii: bool,
) -> Result<DiffFile> {
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

pub(super) const DEFAULT_FILE_COUNT: usize = 180;
pub(super) const DEFAULT_LINES_PER_FILE: usize = 120;
pub(super) const HUGE_FILE_COUNT: usize = 1000;
pub(super) const HUGE_LINES_PER_FILE: usize = 300;
pub(super) const GIANT_SINGLE_FILE_LINES: usize = 50000;

pub(super) fn files(
    count: usize,
    lines: usize,
    start: usize,
    end: usize,
    non_ascii: bool,
) -> Result<Vec<DiffFile>> {
    (1..=count)
        .map(|index| file(index, lines, start, end, non_ascii))
        .collect()
}

pub(super) fn giant_file(index: usize, lines: usize, start: usize, end: usize) -> Result<DiffFile> {
    let path = format!("src/stream{index}.ts");
    let hunk_start = start.saturating_sub(3).max(1);
    let hunk_end = lines.min(end.saturating_add(3));
    let count = hunk_end.saturating_add(1).saturating_sub(hunk_start);
    let mut patch =
        format!("--- {path}\n+++ {path}\n@@ -{hunk_start},{count} +{hunk_start},{count} @@\n");
    for n in hunk_start..start {
        patch.push(' ');
        patch.push_str(&line(index, n, false, false));
    }
    for n in start..=end {
        patch.push('-');
        patch.push_str(&line(index, n, false, false));
    }
    for n in start..=end {
        patch.push('+');
        patch.push_str(&line(index, n, true, false));
    }
    for n in end.saturating_add(1)..=hunk_end {
        patch.push(' ');
        patch.push_str(&line(index, n, false, false));
    }
    let mut file = workdeck_diff::parse_single_file_patch(&patch, &path, None)?;
    file.runtime_id = format!("stream:{index}");
    file.patch.clear();
    file.language = Some("typescript".into());
    file.stats.additions = end.saturating_add(1).saturating_sub(start);
    file.stats.deletions = file.stats.additions;
    Ok(file)
}

fn bootstrap(cwd: PathBuf, files: Vec<DiffFile>, id: String) -> AppBootstrap {
    let mut bootstrap = AppBootstrap::new(
        CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: vec![],
            options: CommonOptions {
                mode: Some(InputLayoutMode::Auto),
                ..CommonOptions::default()
            },
        }),
        ReloadContext {
            cwd,
            repo_root: None,
            initial_watch_signature: None,
            vcs_catalog: None,
        },
        Changeset {
            id,
            source_label: "repo".into(),
            title: "repo working tree".into(),
            summary: None,
            agent_summary: None,
            source: ChangesetSource::WorkingTree { staged: false },
            files,
        },
    );
    bootstrap.initial_mode = InputLayoutMode::Split;
    bootstrap.initial_theme = Some("midnight".into());
    bootstrap
}

pub(super) fn large_bootstrap(
    cwd: PathBuf,
    file_count: usize,
    lines: usize,
    start: usize,
    end: usize,
    non_ascii: bool,
) -> Result<AppBootstrap> {
    let files = files(file_count, lines, start, end, non_ascii)?;
    let variant = if non_ascii { "non-ascii" } else { "ascii" };
    Ok(bootstrap(
        cwd,
        files,
        format!("changeset:large-split-stream:{file_count}:{lines}:{variant}"),
    ))
}

pub(super) fn huge_bootstrap(cwd: PathBuf) -> Result<AppBootstrap> {
    let mut files = files(HUGE_FILE_COUNT, HUGE_LINES_PER_FILE, 37, 84, false)?;
    files.push(giant_file(
        HUGE_FILE_COUNT + 1,
        GIANT_SINGLE_FILE_LINES,
        1000,
        45000,
    )?);
    Ok(bootstrap(
        cwd,
        files,
        format!(
            "changeset:huge-stream:{HUGE_FILE_COUNT}:{HUGE_LINES_PER_FILE}:{GIANT_SINGLE_FILE_LINES}"
        ),
    ))
}

fn summary(bootstrap: &AppBootstrap) -> serde_json::Value {
    let mode = match bootstrap.initial_mode {
        InputLayoutMode::Auto => "auto",
        InputLayoutMode::Split => "split",
        InputLayoutMode::Stack => "stack",
    };
    serde_json::json!({"id":bootstrap.changeset.id,"sourceLabel":bootstrap.changeset.source_label,"title":bootstrap.changeset.title,"files":bootstrap.changeset.files.len(),"additions":bootstrap.changeset.files.iter().map(|f| f.stats.additions).sum::<usize>(),"deletions":bootstrap.changeset.files.iter().map(|f| f.stats.deletions).sum::<usize>(),"lastFile":bootstrap.changeset.files.last().map(|f| f.runtime_id.as_str()),"initialMode":mode,"initialTheme":bootstrap.initial_theme,"initialShowAgentNotes":bootstrap.initial_show_agent_notes})
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let mode = args.next();
    if args.next().is_some()
        || mode
            .as_deref()
            .is_some_and(|mode| !matches!(mode, "--huge" | "--non-ascii"))
    {
        bail!("benchmark stream-fixture accepts --huge or --non-ascii");
    }
    let cwd = std::env::current_dir()?;
    let bootstrap = if mode.as_deref() == Some("--huge") {
        huge_bootstrap(cwd)?
    } else {
        large_bootstrap(
            cwd,
            DEFAULT_FILE_COUNT,
            DEFAULT_LINES_PER_FILE,
            37,
            84,
            mode.as_deref() == Some("--non-ascii"),
        )?
    };
    println!("{}", summary(&bootstrap));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn custom_stream_files_match_both_pins_in_sources_stats_and_hunk_geometry() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-stream-files.json"
        ))
        .unwrap();
        for case in oracle["cases"].as_array().unwrap() {
            let options = &case["options"];
            let index = case["index"].as_u64().unwrap() as usize;
            let result = file(
                index,
                options["linesPerFile"].as_u64().unwrap_or(120) as usize,
                options["changedStartLine"].as_u64().unwrap_or(37) as usize,
                options["changedEndLine"].as_u64().unwrap_or(84) as usize,
                options["contentVariant"] == "non-ascii",
            )
            .unwrap();
            assert_eq!(result.runtime_id, case["id"].as_str().unwrap());
            assert_eq!(result.path, case["path"].as_str().unwrap());
            assert_eq!(result.language.as_deref(), case["language"].as_str());
            assert_eq!(result.patch, case["patch"].as_str().unwrap());
            assert_eq!(
                serde_json::json!({"additions":result.stats.additions,"deletions":result.stats.deletions}),
                case["stats"]
            );
            assert!(!result.stats.truncated);
            assert_eq!(result.flags.partial, case["isPartial"].as_bool().unwrap());
            for (source, key) in [
                (&result.sources.old, "beforeSha256"),
                (&result.sources.new, "afterSha256"),
            ] {
                let hash = format!(
                    "{:x}",
                    Sha256::digest(source.as_ref().unwrap().content.as_bytes())
                );
                assert_eq!(hash, case[key].as_str().unwrap(), "index {index} {key}");
            }
            let hunks: Vec<_> = result.hunks.iter().map(|h| serde_json::json!({"oldStart":h.old_start,"oldCount":h.old_count,"newStart":h.new_start,"newCount":h.new_count})).collect();
            assert_eq!(serde_json::json!(hunks), case["hunks"], "index {index}");
        }
    }

    #[test]
    fn complete_normal_non_ascii_and_huge_bootstraps_match_both_pinned_summaries() {
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-stream-bootstrap.json"
        ))
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let normal = large_bootstrap(
            root.path().to_owned(),
            DEFAULT_FILE_COUNT,
            DEFAULT_LINES_PER_FILE,
            37,
            84,
            false,
        )
        .unwrap();
        assert_eq!(summary(&normal), oracle["normal"]);
        drop(normal);
        let non_ascii = large_bootstrap(root.path().to_owned(), 2, 120, 37, 84, true).unwrap();
        assert_eq!(summary(&non_ascii), oracle["nonAscii"]);
        drop(non_ascii);
        let huge = huge_bootstrap(root.path().to_owned()).unwrap();
        assert_eq!(summary(&huge), oracle["huge"]);
        assert_eq!(huge.reload_context.cwd, root.path());
        assert!(huge.reload_context.repo_root.is_none());
        assert!(matches!(
            huge.input,
            CliInput::Vcs(VcsDiffCommandInput { staged: false, .. })
        ));
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }

    #[test]
    fn bootstrap_retains_review_input_defaults_context_and_custom_stream_shape() {
        let root = tempfile::tempdir().unwrap();
        let result = large_bootstrap(root.path().to_owned(), 2, 120, 37, 84, true).unwrap();
        assert_eq!(result.reload_context.cwd, root.path());
        assert_eq!(result.initial_mode, InputLayoutMode::Split);
        assert_eq!(result.initial_theme.as_deref(), Some("midnight"));
        assert!(!result.initial_show_agent_notes);
        assert_eq!(result.input.options().mode, Some(InputLayoutMode::Auto));
        assert_eq!(result.changeset.files.len(), 2);
        assert_eq!(result.changeset.files[0].stats.additions, 48);
        assert_eq!(
            result.changeset.id,
            "changeset:large-split-stream:2:120:non-ascii"
        );
        assert!(std::fs::read_dir(root.path()).unwrap().next().is_none());
    }

    #[test]
    fn giant_patch_fixture_has_all_declared_lines_without_running_a_large_diff() {
        let file = giant_file(1001, 50000, 1000, 45000).unwrap();
        assert_eq!(file.runtime_id, "stream:1001");
        assert!(file.flags.partial);
        assert!(file.sources.old.is_none() && file.sources.new.is_none());
        assert!(file.patch.is_empty());
        assert_eq!(file.hunks.len(), 1);
        assert_eq!(file.hunks[0].old_start, 997);
        assert_eq!(file.hunks[0].old_count, 44007);
        assert_eq!(file.hunks[0].lines.len(), 88008);
        assert_eq!(file.stats.additions, 44001);
        assert_eq!(file.stats.deletions, 44001);
    }
}
