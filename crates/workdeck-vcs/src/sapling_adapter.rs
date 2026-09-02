//! Bundled Sapling adapter composed exclusively through Workdeck's public VCS contract.

use crate::{
    SaplingBackedInput, SaplingCommandContext, VcsAdapter, VcsCatalogError, VcsDetection,
    VcsLoadContext, VcsOperation, VcsOperations, VcsPatchResult, VcsReviewInput,
    VcsReviewOperationKind, VcsWatchPlan, build_sl_diff_args, build_sl_show_args,
    create_sl_staged_error, describe_diff_range, list_sl_untracked_files, resolve_sl_repo_root,
    run_sl_text, stat_signature,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use workdeck_core::{Changeset, ChangesetSource, VcsDiffCommandInput, VcsShowCommandInput};

pub const SAPLING_VCS_DETECTION_BASELINE_PRIORITY: i32 = 100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaplingVcsAdapterOptions {
    pub sl_executable: PathBuf,
}

impl Default for SaplingVcsAdapterOptions {
    fn default() -> Self {
        Self {
            sl_executable: PathBuf::from("sl"),
        }
    }
}

fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), str::to_owned)
}

/// Return whether a `.hg` directory belongs to Sapling rather than Mercurial.
#[must_use]
pub fn is_sapling_hg_repo(hg_directory: &Path) -> bool {
    fs::read_to_string(hg_directory.join("requires"))
        .map(|requires| {
            requires
                .lines()
                .any(|requirement| requirement == "treestate")
        })
        .unwrap_or(false)
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    }
}

/// Walk upward to find `.sl`, or a Sapling-specific `.hg/requires` marker.
#[must_use]
pub fn detect_sapling_repo(cwd: &Path) -> Option<VcsDetection> {
    let mut current = cwd.canonicalize().unwrap_or_else(|_| absolute_path(cwd));
    loop {
        if current.join(".sl").exists() {
            return Some(VcsDetection {
                id: "sl".into(),
                repo_root: current,
            });
        }
        let hg_directory = current.join(".hg");
        if hg_directory.exists() && is_sapling_hg_repo(&hg_directory) {
            return Some(VcsDetection {
                id: "sl".into(),
                repo_root: current,
            });
        }
        if !current.pop() {
            return None;
        }
    }
}

fn command_context(cwd: &Path, sl_executable: &Path) -> SaplingCommandContext {
    SaplingCommandContext {
        cwd: cwd.to_owned(),
        sl_executable: sl_executable.to_owned(),
    }
}

fn load_working_tree(
    input: &VcsDiffCommandInput,
    context: &VcsLoadContext,
    sl_executable: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    if input.staged {
        return Err(VcsCatalogError::User(create_sl_staged_error(input)));
    }
    // Endpoint validation deliberately precedes every Sapling process invocation.
    let arguments = build_sl_diff_args(input).map_err(VcsCatalogError::User)?;
    let backed = SaplingBackedInput::from(input);
    let command_context = command_context(&context.cwd, sl_executable);
    let repo_root = resolve_sl_repo_root(&backed, &command_context)?;
    let repo_name = basename(&repo_root);
    let title = describe_diff_range(input).map_or_else(
        || format!("{repo_name} working copy"),
        |range| format!("{repo_name} {range}"),
    );
    let patch_text = run_sl_text(&backed, &arguments, &command_context)?;
    let untracked_paths = list_sl_untracked_files(input, &command_context, Some(&repo_root))?;
    Ok(VcsPatchResult {
        source_label: repo_root.display().to_string(),
        title,
        patch_text,
        repo_root,
        untracked_paths,
        source_reader: None,
        source_cache_key: None,
        extra_files: Vec::new(),
    })
}

fn load_show(
    input: &VcsShowCommandInput,
    context: &VcsLoadContext,
    sl_executable: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    let backed = SaplingBackedInput::from(input);
    let command_context = command_context(&context.cwd, sl_executable);
    let repo_root = resolve_sl_repo_root(&backed, &command_context)?;
    let repo_name = basename(&repo_root);
    let revset = input.reference.as_deref().unwrap_or(".");
    Ok(VcsPatchResult {
        source_label: repo_root.display().to_string(),
        title: format!("{repo_name} show {revset}"),
        patch_text: run_sl_text(&backed, &build_sl_show_args(input), &command_context)?,
        repo_root,
        untracked_paths: Vec::new(),
        source_reader: None,
        source_cache_key: None,
        extra_files: Vec::new(),
    })
}

fn working_tree_watch_signature(
    input: &VcsDiffCommandInput,
    context: &VcsLoadContext,
    sl_executable: &Path,
) -> Result<String, VcsCatalogError> {
    let arguments = build_sl_diff_args(input).map_err(VcsCatalogError::User)?;
    let backed = SaplingBackedInput::from(input);
    let command_context = command_context(&context.cwd, sl_executable);
    let tracked_patch = run_sl_text(&backed, &arguments, &command_context)?;
    let repo_root = resolve_sl_repo_root(&backed, &command_context)?;
    let mut fragments = vec![tracked_patch];
    fragments.extend(
        list_sl_untracked_files(input, &command_context, Some(&repo_root))?
            .into_iter()
            .map(|path| format!("untracked:{}", stat_signature(&repo_root.join(path)))),
    );
    Ok(fragments.join("\n---\n"))
}

fn show_watch_signature(
    input: &VcsShowCommandInput,
    context: &VcsLoadContext,
    sl_executable: &Path,
) -> Result<String, VcsCatalogError> {
    run_sl_text(
        &SaplingBackedInput::from(input),
        &build_sl_show_args(input),
        &command_context(&context.cwd, sl_executable),
    )
}

/// Load and materialize one Sapling review through the extension adapter path.
pub fn load_sapling_changeset(
    input: &VcsReviewInput,
    context: &VcsLoadContext,
    options: &SaplingVcsAdapterOptions,
) -> Result<Changeset, VcsCatalogError> {
    let result = match input {
        VcsReviewInput::Diff(input) => load_working_tree(input, context, &options.sl_executable)?,
        VcsReviewInput::Show(input) => load_show(input, context, &options.sl_executable)?,
        VcsReviewInput::StashShow(_) => {
            return Err(VcsCatalogError::Operation(
                "Sapling does not support stash review".into(),
            ));
        }
    };
    let (id, source) = match input {
        VcsReviewInput::Diff(input) => {
            let source = if let Some(endpoints) = &input.range_endpoints {
                ChangesetSource::Revision {
                    from: Some(endpoints.from.clone()),
                    to: endpoints.to.clone(),
                }
            } else if let Some(range) = &input.range {
                ChangesetSource::Revision {
                    from: None,
                    to: range.clone(),
                }
            } else {
                ChangesetSource::WorkingTree { staged: false }
            };
            ("sl:diff".into(), source)
        }
        VcsReviewInput::Show(input) => {
            let revset = input.reference.as_deref().unwrap_or(".");
            (
                format!("sl:show:{revset}"),
                ChangesetSource::Revision {
                    from: None,
                    to: revset.into(),
                },
            )
        }
        VcsReviewInput::StashShow(_) => unreachable!("stash returned before materialization"),
    };
    crate::materialize_vcs_patch_result(result, id, source)
}

/// Build the statically linked Sapling adapter, optionally overriding its executable.
#[must_use]
pub fn create_sapling_vcs_adapter(options: SaplingVcsAdapterOptions) -> VcsAdapter {
    let sl_executable = options.sl_executable;
    let mut operations: VcsOperations = BTreeMap::new();

    let load_sl = sl_executable.clone();
    let signature_sl = sl_executable.clone();
    operations.insert(
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsOperation {
            load: Arc::new(move |input, context| match input {
                VcsReviewInput::Diff(input) => load_working_tree(input, context, &load_sl),
                _ => Err(VcsCatalogError::Operation(
                    "Sapling working-tree operation received the wrong input".into(),
                )),
            }),
            watch_signature: Some(Arc::new(move |input, context| match input {
                VcsReviewInput::Diff(input) => {
                    working_tree_watch_signature(input, context, &signature_sl)
                }
                _ => Err(VcsCatalogError::Operation(
                    "Sapling working-tree signature received the wrong input".into(),
                )),
            })),
            watch_plan: Some(Arc::new(|_, _| Ok(VcsWatchPlan::poll_only()))),
        },
    );

    let load_sl = sl_executable.clone();
    let signature_sl = sl_executable;
    operations.insert(
        VcsReviewOperationKind::RevisionShow,
        VcsOperation {
            load: Arc::new(move |input, context| match input {
                VcsReviewInput::Show(input) => load_show(input, context, &load_sl),
                _ => Err(VcsCatalogError::Operation(
                    "Sapling revision operation received the wrong input".into(),
                )),
            }),
            watch_signature: Some(Arc::new(move |input, context| match input {
                VcsReviewInput::Show(input) => show_watch_signature(input, context, &signature_sl),
                _ => Err(VcsCatalogError::Operation(
                    "Sapling revision signature received the wrong input".into(),
                )),
            })),
            watch_plan: Some(Arc::new(|_, _| Ok(VcsWatchPlan::poll_only()))),
        },
    );

    VcsAdapter {
        id: "sl".into(),
        name: "Sapling".into(),
        detect: Arc::new(|cwd| Ok(detect_sapling_repo(cwd))),
        operations,
        detection_priority: Some(SAPLING_VCS_DETECTION_BASELINE_PRIORITY),
    }
}

#[cfg(test)]
mod tests;
