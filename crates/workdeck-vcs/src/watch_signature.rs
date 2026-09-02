//! Stable change-detection signatures for direct files and composed VCS adapters.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use thiserror::Error;
use workdeck_core::CliInput;

use crate::{
    VcsCatalog, VcsCatalogError, VcsLoadContext, VcsReviewInput, create_vcs_watch_signature,
    get_configured_vcs_adapter, operation_from_input,
};

#[derive(Clone, Copy)]
pub struct WatchSignatureContext<'a> {
    pub cwd: &'a Path,
    pub vcs_catalog: Option<&'a VcsCatalog>,
}

#[derive(Debug, Error)]
pub enum WatchSignatureError {
    #[error("VCS-backed watch signatures require a composed VCS catalog.")]
    MissingVcsCatalog,
    #[error("Watch mode requires a patch file path instead of stdin.")]
    PatchUsesStdin,
    #[error("failed to inspect watch path {path}: {source}")]
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Vcs(#[from] VcsCatalogError),
}

/// Compute a signature relative to the source's stable load directory.
pub fn compute_watch_signature(
    input: &CliInput,
    context: WatchSignatureContext<'_>,
) -> Result<String, WatchSignatureError> {
    let mut parts = vec![input_kind(input).to_owned()];
    match input {
        CliInput::Vcs(input) => {
            parts.push(vcs_signature(
                VcsReviewInput::Diff(input.clone()),
                input.options.vcs.as_deref(),
                context,
            )?);
        }
        CliInput::Show(input) => {
            parts.push(vcs_signature(
                VcsReviewInput::Show(input.clone()),
                input.options.vcs.as_deref(),
                context,
            )?);
        }
        CliInput::StashShow(input) => {
            parts.push(vcs_signature(
                VcsReviewInput::StashShow(input.clone()),
                input.options.vcs.as_deref(),
                context,
            )?);
        }
        CliInput::Files(input) => {
            parts.push(stat_signature(&resolve_input_path(
                context.cwd,
                &input.left,
            ))?);
            parts.push(stat_signature(&resolve_input_path(
                context.cwd,
                &input.right,
            ))?);
        }
        CliInput::DiffTool(input) => {
            parts.push(stat_signature(&resolve_input_path(
                context.cwd,
                &input.left,
            ))?);
            parts.push(stat_signature(&resolve_input_path(
                context.cwd,
                &input.right,
            ))?);
        }
        CliInput::Patch(input) => {
            let path = input
                .file
                .as_deref()
                .filter(|path| *path != "-")
                .ok_or(WatchSignatureError::PatchUsesStdin)?;
            parts.push(stat_signature(&resolve_input_path(context.cwd, path))?);
        }
    }
    if let Some(agent_context) = input
        .options()
        .agent_context
        .as_deref()
        .filter(|path| *path != "-")
    {
        parts.push(format!(
            "agent:{}",
            stat_signature(&resolve_input_path(context.cwd, agent_context))?
        ));
    }
    Ok(parts.join("\n---\n"))
}

fn input_kind(input: &CliInput) -> &'static str {
    match input {
        CliInput::Vcs(_) => "vcs",
        CliInput::Show(_) => "show",
        CliInput::StashShow(_) => "stash-show",
        CliInput::Files(_) => "diff",
        CliInput::Patch(_) => "patch",
        CliInput::DiffTool(_) => "difftool",
    }
}

fn vcs_signature(
    input: VcsReviewInput,
    configured_id: Option<&str>,
    context: WatchSignatureContext<'_>,
) -> Result<String, WatchSignatureError> {
    let catalog = context
        .vcs_catalog
        .ok_or(WatchSignatureError::MissingVcsCatalog)?;
    let adapter = get_configured_vcs_adapter(configured_id.filter(|id| *id != "auto"), catalog)?;
    let operation = operation_from_input(input);
    Ok(create_vcs_watch_signature(
        adapter,
        &operation,
        &VcsLoadContext {
            cwd: context.cwd.to_owned(),
        },
        catalog,
    )?)
}

fn resolve_input_path(cwd: &Path, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_owned()
    } else {
        cwd.join(path)
    }
}

fn stat_signature(path: &Path) -> Result<String, WatchSignatureError> {
    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(format!("{}:missing", path.display()));
        }
        Err(source) => {
            return Err(WatchSignatureError::Metadata {
                path: path.to_owned(),
                source,
            });
        }
    };
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos() / 1_000_000);
    Ok(format!(
        "{}:{}:{modified_ms}:{}",
        path.display(),
        metadata.len(),
        file_identity(&metadata)
    ))
}

#[cfg(unix)]
fn file_identity(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(not(unix))]
fn file_identity(_metadata: &fs::Metadata) -> u64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;
    use tempfile::tempdir;
    use workdeck_core::{
        CommonOptions, FileCommandInput, PatchCommandInput, VcsDiffCommandInput,
        VcsStashShowCommandInput,
    };

    use crate::{
        VcsAdapter, VcsOperation, VcsPatchResult, VcsReviewOperationKind, create_base_vcs_catalog,
    };

    type TestSignature =
        Arc<dyn Fn(&workdeck_core::VcsDiffCommandInput, &Path) -> String + Send + Sync>;

    fn git(cwd: &Path, arguments: &[&str]) {
        let output = Command::new("git")
            .args(arguments)
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn load_operation(signature: TestSignature) -> VcsOperation {
        VcsOperation {
            load: Arc::new(|_, context| {
                Ok(VcsPatchResult {
                    repo_root: context.cwd.clone(),
                    source_label: "review".into(),
                    title: "review".into(),
                    patch_text: String::new(),
                    untracked_paths: Vec::new(),
                    source_reader: None,
                    extra_files: Vec::new(),
                })
            }),
            watch_signature: Some(Arc::new(move |input, context| {
                let VcsReviewInput::Diff(input) = input else {
                    return Ok("unsupported".into());
                };
                Ok(signature(input, &context.cwd))
            })),
            watch_plan: None,
        }
    }

    fn catalog_with_signature(
        id: &str,
        signature: impl Fn(&workdeck_core::VcsDiffCommandInput, &Path) -> String + Send + Sync + 'static,
    ) -> VcsCatalog {
        let adapter = VcsAdapter {
            id: id.into(),
            name: id.to_uppercase(),
            detect: Arc::new(|_| Ok(None)),
            operations: [(
                VcsReviewOperationKind::WorkingTreeDiff,
                load_operation(Arc::new(signature)),
            )]
            .into_iter()
            .collect(),
            detection_priority: None,
        };
        create_base_vcs_catalog(vec![adapter], id)
    }

    fn git_signature(input: &workdeck_core::VcsDiffCommandInput, cwd: &Path) -> String {
        let tracked = Command::new("git")
            .args(["diff", "--no-ext-diff", "--find-renames"])
            .current_dir(cwd)
            .output()
            .unwrap();
        assert!(tracked.status.success());
        let mut parts = vec![String::from_utf8(tracked.stdout).unwrap()];
        if input.options.exclude_untracked != Some(true) {
            let status = Command::new("git")
                .args([
                    "--no-optional-locks",
                    "status",
                    "--porcelain=v1",
                    "-z",
                    "--untracked-files=all",
                ])
                .current_dir(cwd)
                .output()
                .unwrap();
            assert!(status.status.success());
            for entry in String::from_utf8(status.stdout)
                .unwrap()
                .split('\0')
                .filter(|entry| entry.starts_with("?? "))
            {
                parts.push(format!(
                    "untracked:{}",
                    stat_signature(&cwd.join(&entry[3..])).unwrap()
                ));
            }
        }
        parts.join("\n---\n")
    }

    fn vcs_input(exclude_untracked: bool) -> CliInput {
        CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions {
                exclude_untracked: Some(exclude_untracked),
                ..CommonOptions::default()
            },
        })
    }

    #[test]
    fn resolves_direct_patch_and_agent_paths_against_the_supplied_cwd() {
        let directory = tempdir().unwrap();
        for (path, contents) in [
            ("left.ts", "one\n"),
            ("right.ts", "two\n"),
            ("review.patch", "patch\n"),
            ("agent.json", "{}\n"),
        ] {
            fs::write(directory.path().join(path), contents).unwrap();
        }
        let direct = CliInput::Files(FileCommandInput {
            left: "left.ts".into(),
            right: "right.ts".into(),
            options: CommonOptions {
                agent_context: Some("agent.json".into()),
                ..CommonOptions::default()
            },
        });
        let patch = CliInput::Patch(PatchCommandInput {
            file: Some("review.patch".into()),
            text: None,
            options: CommonOptions::default(),
        });
        let context = WatchSignatureContext {
            cwd: directory.path(),
            vcs_catalog: None,
        };
        let direct = compute_watch_signature(&direct, context).unwrap();
        let patch = compute_watch_signature(&patch, context).unwrap();
        for path in ["left.ts", "right.ts", "agent.json"] {
            assert!(direct.contains(&directory.path().join(path).display().to_string()));
        }
        assert!(patch.contains(&directory.path().join("review.patch").display().to_string()));
    }

    #[test]
    fn untracked_signatures_change_without_embedding_file_contents() {
        let directory = tempdir().unwrap();
        git(directory.path(), &["init"]);
        let marker = "UNTRACKED-CONTENT-".repeat(1024);
        fs::write(directory.path().join("large.txt"), &marker).unwrap();
        let catalog = catalog_with_signature("git", git_signature);
        let context = WatchSignatureContext {
            cwd: directory.path(),
            vcs_catalog: Some(&catalog),
        };
        let initial = compute_watch_signature(&vcs_input(false), context).unwrap();
        thread::sleep(Duration::from_millis(2));
        fs::write(
            directory.path().join("large.txt"),
            format!("{marker}changed"),
        )
        .unwrap();
        let changed = compute_watch_signature(&vcs_input(false), context).unwrap();
        assert!(!initial.contains(&marker) && !changed.contains(&marker));
        assert_ne!(initial, changed);
    }

    #[test]
    fn excluding_untracked_files_keeps_the_signature_stable() {
        let directory = tempdir().unwrap();
        git(directory.path(), &["init"]);
        fs::write(directory.path().join("note.txt"), "first").unwrap();
        let catalog = catalog_with_signature("git", git_signature);
        let context = WatchSignatureContext {
            cwd: directory.path(),
            vcs_catalog: Some(&catalog),
        };
        let initial = compute_watch_signature(&vcs_input(true), context).unwrap();
        fs::write(directory.path().join("note.txt"), "second").unwrap();
        assert_eq!(
            compute_watch_signature(&vcs_input(true), context).unwrap(),
            initial
        );
    }

    #[test]
    fn extension_adapter_signature_is_threaded_through_the_context() {
        let catalog = catalog_with_signature("hg", |input, _| {
            format!("hg:{}", input.range.as_deref().unwrap_or("working-copy"))
        });
        let input = CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions {
                vcs: Some("hg".into()),
                ..CommonOptions::default()
            },
        });
        assert_eq!(
            compute_watch_signature(
                &input,
                WatchSignatureContext {
                    cwd: Path::new("/repo"),
                    vcs_catalog: Some(&catalog)
                }
            )
            .unwrap(),
            "vcs\n---\nhg:working-copy"
        );
    }

    #[test]
    fn unsupported_watch_operations_fail_before_a_signature_callback() {
        let jj = VcsAdapter {
            id: "jj".into(),
            name: "Jujutsu".into(),
            detect: Arc::new(|_| Ok(None)),
            operations: [(
                VcsReviewOperationKind::WorkingTreeDiff,
                load_operation(Arc::new(|_, _| panic!("must not run"))),
            )]
            .into_iter()
            .collect(),
            detection_priority: None,
        };
        let git = VcsAdapter {
            id: "git".into(),
            name: "Git".into(),
            detect: Arc::new(|_| Ok(None)),
            operations: [(
                VcsReviewOperationKind::StashShow,
                load_operation(Arc::new(|_, _| "unused".into())),
            )]
            .into_iter()
            .collect(),
            detection_priority: None,
        };
        let catalog = create_base_vcs_catalog(vec![jj, git], "jj");
        let input = CliInput::StashShow(VcsStashShowCommandInput {
            reference: None,
            options: CommonOptions {
                vcs: Some("jj".into()),
                ..CommonOptions::default()
            },
        });
        let error = compute_watch_signature(
            &input,
            WatchSignatureContext {
                cwd: Path::new("/repo"),
                vcs_catalog: Some(&catalog),
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("`workdeck stash show` requires"));
    }

    #[test]
    fn working_tree_against_one_ref_still_tracks_untracked_files() {
        let directory = tempdir().unwrap();
        git(directory.path(), &["init"]);
        fs::write(directory.path().join("note.txt"), "first").unwrap();
        let catalog = catalog_with_signature("git", git_signature);
        let mut input = vcs_input(false);
        let CliInput::Vcs(input) = &mut input else {
            unreachable!()
        };
        input.range = Some("main".into());
        let context = WatchSignatureContext {
            cwd: directory.path(),
            vcs_catalog: Some(&catalog),
        };
        let initial = compute_watch_signature(&CliInput::Vcs(input.clone()), context);
        thread::sleep(Duration::from_millis(2));
        fs::write(directory.path().join("note.txt"), "second").unwrap();
        let changed = compute_watch_signature(&CliInput::Vcs(input.clone()), context);
        assert_ne!(initial.unwrap(), changed.unwrap());
    }
}
