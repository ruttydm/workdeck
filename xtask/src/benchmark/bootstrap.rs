//! MIT translation of Hunk's bootstrap workload using production Rust loading boundaries.

use super::*;
use std::time::Instant;
use workdeck_core::{
    AppBootstrap, ChangesetSource, CliInput, CommonOptions, FileCommandInput, InputLayoutMode,
    ReloadContext, VcsDiffCommandInput,
};
use workdeck_vcs::{
    VcsCatalog, VcsReviewInput, bundled_vcs_catalog, get_vcs_adapter, load_file_comparison,
    load_selected_vcs_changeset,
};

const FILE_COUNT: usize = 64;
const LINES_PER_FILE: usize = 420;

fn contents(index: usize, changed: bool) -> String {
    (0..LINES_PER_FILE).map(|offset| {
        let line = offset + 1;
        if changed && (120..300).contains(&offset) {
            format!("export function feature{index}_{line}(value: number) {{ return value * {line} + {index}; }}\n")
        } else {
            format!("export function feature{index}_{line}(value: number) {{ return value + {line}; }}\n")
        }
    }).collect()
}

fn fixture() -> Result<tempfile::TempDir> {
    let root = fixtures::temporary("workdeck-bootstrap-benchmark-")?;
    fixtures::git(root.path(), &["init"])?;
    fixtures::git(root.path(), &["config", "user.name", "Benchmark User"])?;
    fixtures::git(
        root.path(),
        &["config", "user.email", "benchmark@example.com"],
    )?;
    fixtures::git(root.path(), &["config", "commit.gpgsign", "false"])?;
    for index in 1..=FILE_COUNT {
        std::fs::write(
            root.path().join(format!("file{index}.ts")),
            contents(index, false),
        )?;
    }
    fixtures::git(root.path(), &["add", "."])?;
    fixtures::git(root.path(), &["commit", "-m", "initial"])?;
    for index in 1..=FILE_COUNT {
        std::fs::write(
            root.path().join(format!("file{index}.ts")),
            contents(index, true),
        )?;
    }
    Ok(root)
}

pub(super) fn load_vcs(cwd: &Path) -> Result<AppBootstrap<(), VcsCatalog>> {
    let input = VcsDiffCommandInput {
        range: None,
        range_endpoints: None,
        staged: false,
        pathspecs: vec![],
        options: CommonOptions {
            mode: Some(InputLayoutMode::Auto),
            ..CommonOptions::default()
        },
    };
    let catalog = bundled_vcs_catalog();
    let adapter = get_vcs_adapter("git", catalog)?;
    let loaded =
        load_selected_vcs_changeset(cwd, adapter, catalog, &VcsReviewInput::Diff(input.clone()))?;
    Ok(AppBootstrap::new(
        CliInput::Vcs(input),
        ReloadContext {
            cwd: cwd.to_owned(),
            repo_root: Some(loaded.repo_root),
            initial_watch_signature: None,
            vcs_catalog: Some(catalog.clone()),
        },
        loaded.changeset,
    ))
}

struct Measurement {
    git_bootstrap_ms: f64,
    git_diff_subprocess_ms: f64,
    git_parse_patch_ms: f64,
    git_split_patch_chunks_ms: f64,
    file_pair_bootstrap_ms: f64,
    files: usize,
    parsed_files: usize,
    patch_chunks: usize,
    pair_files: usize,
}

fn measure(root: &Path) -> Result<Measurement> {
    let started = Instant::now();
    let bootstrap = load_vcs(root)?;
    let git_bootstrap_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let patch = fixtures::git(
        root,
        &["diff", "--no-ext-diff", "--find-renames", "--no-color"],
    )?;
    let git_diff_subprocess_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let parsed = workdeck_diff::parse_patch(
        &patch,
        "patch",
        "patch",
        ChangesetSource::Patch {
            label: "patch".into(),
        },
    )?;
    let git_parse_patch_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let chunks = workdeck_diff::split_patch_into_file_chunks(&patch);
    let git_split_patch_chunks_ms = started.elapsed().as_secs_f64() * 1000.0;
    let left = root.join("before.ts");
    let right = root.join("after.ts");
    std::fs::write(&left, contents(999, false))?;
    std::fs::write(&right, contents(999, true))?;
    let started = Instant::now();
    // The source restores its caller cwd after the VCS probe, before loading this absolute pair.
    let cwd = std::env::current_dir()?;
    let changeset = load_file_comparison(&cwd, &left, &right)?;
    let pair: AppBootstrap = AppBootstrap::new(
        CliInput::Files(FileCommandInput {
            left: left.to_string_lossy().into_owned(),
            right: right.to_string_lossy().into_owned(),
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
        changeset,
    );
    let file_pair_bootstrap_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok(Measurement {
        git_bootstrap_ms,
        git_diff_subprocess_ms,
        git_parse_patch_ms,
        git_split_patch_chunks_ms,
        file_pair_bootstrap_ms,
        files: bootstrap.changeset.files.len(),
        parsed_files: parsed.files.len(),
        patch_chunks: chunks.len(),
        pair_files: pair.changeset.files.len(),
    })
}

pub(super) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    if args.next().is_some() {
        bail!("benchmark bootstrap-load accepts no arguments");
    }
    let root = fixture()?;
    let result = measure(root.path())?;
    for (name, value) in [
        ("git_bootstrap_ms", result.git_bootstrap_ms),
        ("git_diff_subprocess_ms", result.git_diff_subprocess_ms),
        ("git_parse_patch_ms", result.git_parse_patch_ms),
        (
            "git_split_patch_chunks_ms",
            result.git_split_patch_chunks_ms,
        ),
        ("file_pair_bootstrap_ms", result.file_pair_bootstrap_ms),
    ] {
        println!("METRIC {name}={}", fixed(value, 2));
    }
    for (name, value) in [
        ("files", result.files),
        ("parsed_files", result.parsed_files),
        ("patch_chunks", result.patch_chunks),
        ("lines_per_file", LINES_PER_FILE),
    ] {
        println!("METRIC {name}={value}");
    }
    if result.pair_files != 1 {
        bail!("bootstrap fixture did not produce its single direct-file comparison");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_bootstrap_workload_matches_both_pinned_structural_results() {
        use sha2::{Digest, Sha256};
        let oracle: serde_json::Value = serde_json::from_str(include_str!(
            "../../../port/hunk/oracles/benchmark-bootstrap-load.json"
        ))
        .unwrap();
        for case in oracle["contents"].as_array().unwrap() {
            let text = contents(
                case["index"].as_u64().unwrap() as usize,
                case["changed"].as_bool().unwrap(),
            );
            assert_eq!(text.len() as u64, case["bytes"].as_u64().unwrap());
            assert_eq!(
                format!("{:x}", Sha256::digest(text.as_bytes())),
                case["sha256"].as_str().unwrap()
            );
        }
        let cwd = std::env::current_dir().unwrap();
        let root = fixture().unwrap();
        assert_eq!(
            fixtures::git(root.path(), &["show", "HEAD:file1.ts"]).unwrap(),
            contents(1, false)
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("file1.ts")).unwrap(),
            contents(1, true)
        );
        let result = measure(root.path()).unwrap();
        assert_eq!(std::env::current_dir().unwrap(), cwd);
        assert_eq!(
            std::fs::read_to_string(root.path().join("before.ts")).unwrap(),
            contents(999, false)
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("after.ts")).unwrap(),
            contents(999, true)
        );
        assert_eq!(
            result.files as u64,
            oracle["counts"]["files"].as_u64().unwrap()
        );
        assert_eq!(
            result.parsed_files as u64,
            oracle["counts"]["parsed_files"].as_u64().unwrap()
        );
        assert_eq!(
            result.patch_chunks as u64,
            oracle["counts"]["patch_chunks"].as_u64().unwrap()
        );
        assert_eq!(result.pair_files, 1);
        assert!(!root.path().join(".agents").exists());
        assert!(run(["extra".into()].into_iter()).is_err());
        let path = root.path().to_owned();
        drop(root);
        assert!(!path.exists());
    }
}
