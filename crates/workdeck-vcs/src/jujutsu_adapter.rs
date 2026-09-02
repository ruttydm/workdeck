//! Bundled Jujutsu adapter composed exclusively through Workdeck's public VCS contract.

use crate::{
    JujutsuBackedInput, JujutsuCommandContext, JujutsuDiffEndpoints, JujutsuFileSourceOptions,
    JujutsuFileSourceSpec, JujutsuPinnedDiff, LimitedSourceTextResult, VcsAdapter, VcsCatalogError,
    VcsDetection, VcsFileSourceResult, VcsLoadContext, VcsOperation, VcsOperations, VcsPatchResult,
    VcsReviewInput, VcsReviewOperationKind, VcsSourceReader, VcsWatchPlan, build_jj_diff_args,
    build_jj_show_args, create_jj_staged_error, describe_diff_range, read_jj_file_source,
    resolve_jj_diff_endpoints, resolve_jj_range_endpoints, resolve_jj_repo_root, run_jj_text,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use workdeck_core::{
    Changeset, ChangesetSource, FileChangeKind, ReviewSide, SourceOrigin, SourceSnapshot,
    VcsDiffCommandInput, VcsShowCommandInput,
};

pub const JUJUTSU_VCS_DETECTION_BASELINE_PRIORITY: i32 = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JujutsuVcsAdapterOptions {
    pub jj_executable: PathBuf,
}

impl Default for JujutsuVcsAdapterOptions {
    fn default() -> Self {
        Self {
            jj_executable: PathBuf::from("jj"),
        }
    }
}

#[derive(Clone)]
struct JujutsuSourceCapability {
    read_file_source: VcsSourceReader,
    source_cache_key: String,
}

fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), str::to_owned)
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

#[must_use]
pub fn detect_jujutsu_repo(cwd: &Path) -> Option<VcsDetection> {
    let mut current = cwd.canonicalize().unwrap_or_else(|_| absolute_path(cwd));
    loop {
        if current.join(".jj").exists() {
            return Some(VcsDetection {
                id: "jj".into(),
                repo_root: current,
            });
        }
        if !current.pop() {
            return None;
        }
    }
}

fn command_context(cwd: &Path, jj_executable: &Path) -> JujutsuCommandContext {
    JujutsuCommandContext {
        cwd: cwd.to_owned(),
        jj_executable: jj_executable.to_owned(),
    }
}

fn old_side_cache_key(old_commit_ids: &[String]) -> String {
    if old_commit_ids.len() == 1 {
        format!("commit:{}", old_commit_ids[0])
    } else {
        format!("merged-parents:{}", old_commit_ids.join(","))
    }
}

fn source_result(result: LimitedSourceTextResult, commit_id: &str) -> VcsFileSourceResult {
    match result {
        LimitedSourceTextResult::Text(content) => VcsFileSourceResult::Source(SourceSnapshot::new(
            content,
            SourceOrigin::Revision {
                revision: commit_id.into(),
            },
            true,
        )),
        LimitedSourceTextResult::Missing => VcsFileSourceResult::Missing,
        LimitedSourceTextResult::TooLarge { max_bytes } => {
            VcsFileSourceResult::TooLarge { max_bytes }
        }
    }
}

fn create_jujutsu_source_capability(
    repo_root: &Path,
    endpoints: &JujutsuDiffEndpoints,
    jj_executable: &Path,
) -> JujutsuSourceCapability {
    let old_commit_id =
        (endpoints.old_commit_ids.len() == 1).then(|| endpoints.old_commit_ids[0].clone());
    let source_cache_key = format!(
        "jj-source-v1:{}:commit:{}",
        old_side_cache_key(&endpoints.old_commit_ids),
        endpoints.new_commit_id
    );
    let root = repo_root.to_owned();
    let new_commit_id = endpoints.new_commit_id.clone();
    let executable = jj_executable.to_owned();
    let reader: VcsSourceReader = Arc::new(move |request| match request.side {
        ReviewSide::Old => {
            if matches!(
                request.change_kind,
                FileChangeKind::Added | FileChangeKind::Untracked
            ) {
                return VcsFileSourceResult::Missing;
            }
            let Some(commit_id) = &old_commit_id else {
                return VcsFileSourceResult::Missing;
            };
            let path = request.previous_path.as_deref().unwrap_or(&request.path);
            source_result(
                read_jj_file_source(
                    &JujutsuFileSourceSpec {
                        repo_root: root.clone(),
                        commit_id: commit_id.clone(),
                        path: path.into(),
                    },
                    &JujutsuFileSourceOptions {
                        jj_executable: executable.clone(),
                        ..JujutsuFileSourceOptions::default()
                    },
                ),
                commit_id,
            )
        }
        ReviewSide::New if request.change_kind == FileChangeKind::Deleted => {
            VcsFileSourceResult::Missing
        }
        ReviewSide::New => source_result(
            read_jj_file_source(
                &JujutsuFileSourceSpec {
                    repo_root: root.clone(),
                    commit_id: new_commit_id.clone(),
                    path: request.path.clone(),
                },
                &JujutsuFileSourceOptions {
                    jj_executable: executable.clone(),
                    ..JujutsuFileSourceOptions::default()
                },
            ),
            &new_commit_id,
        ),
    });
    JujutsuSourceCapability {
        read_file_source: reader,
        source_cache_key,
    }
}

fn load_working_tree(
    input: &VcsDiffCommandInput,
    context: &VcsLoadContext,
    jj_executable: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    if input.staged {
        return Err(VcsCatalogError::User(create_jj_staged_error(input)));
    }
    let backed = JujutsuBackedInput::from(input);
    let command_context = command_context(&context.cwd, jj_executable);
    let repo_root = resolve_jj_repo_root(&backed, &command_context)?;
    let repo_name = basename(&repo_root);
    let source_endpoints = if let Some(endpoints) = &input.range_endpoints {
        resolve_jj_range_endpoints(input, endpoints, &command_context)?
    } else {
        resolve_jj_diff_endpoints(
            &backed,
            input.range.as_deref().unwrap_or("@"),
            &command_context,
        )?
    };
    let source_capability = source_endpoints
        .as_ref()
        .map(|endpoints| create_jujutsu_source_capability(&repo_root, endpoints, jj_executable));
    let pinned = if input.range_endpoints.is_some() {
        source_endpoints.as_ref().and_then(|endpoints| {
            (endpoints.old_commit_ids.len() == 1).then(|| {
                JujutsuPinnedDiff::Range(workdeck_core::VcsRangeEndpoints {
                    from: endpoints.old_commit_ids[0].clone(),
                    to: endpoints.new_commit_id.clone(),
                })
            })
        })
    } else {
        source_endpoints
            .as_ref()
            .map(|endpoints| JujutsuPinnedDiff::Revision(endpoints.new_commit_id.clone()))
    };
    let arguments =
        build_jj_diff_args(input, pinned.as_ref(), false).map_err(VcsCatalogError::User)?;
    let title = describe_diff_range(input).map_or_else(
        || format!("{repo_name} working copy"),
        |range| format!("{repo_name} {range}"),
    );
    Ok(VcsPatchResult {
        source_label: repo_root.display().to_string(),
        title,
        patch_text: run_jj_text(&backed, &arguments, &command_context)?,
        repo_root,
        untracked_paths: Vec::new(),
        source_reader: source_capability
            .as_ref()
            .map(|capability| Arc::clone(&capability.read_file_source)),
        source_cache_key: source_capability.map(|capability| capability.source_cache_key),
        extra_files: Vec::new(),
    })
}

fn load_show(
    input: &VcsShowCommandInput,
    context: &VcsLoadContext,
    jj_executable: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    let backed = JujutsuBackedInput::from(input);
    let command_context = command_context(&context.cwd, jj_executable);
    let repo_root = resolve_jj_repo_root(&backed, &command_context)?;
    let repo_name = basename(&repo_root);
    let revset = input.reference.as_deref().unwrap_or("@");
    let source_endpoints = resolve_jj_diff_endpoints(&backed, revset, &command_context)?;
    let source_capability = source_endpoints
        .as_ref()
        .map(|endpoints| create_jujutsu_source_capability(&repo_root, endpoints, jj_executable));
    let arguments = build_jj_show_args(
        input,
        source_endpoints
            .as_ref()
            .map(|endpoints| endpoints.new_commit_id.as_str()),
    );
    Ok(VcsPatchResult {
        source_label: repo_root.display().to_string(),
        title: format!("{repo_name} show {revset}"),
        patch_text: run_jj_text(&backed, &arguments, &command_context)?,
        repo_root,
        untracked_paths: Vec::new(),
        source_reader: source_capability
            .as_ref()
            .map(|capability| Arc::clone(&capability.read_file_source)),
        source_cache_key: source_capability.map(|capability| capability.source_cache_key),
        extra_files: Vec::new(),
    })
}

fn working_tree_watch_signature(
    input: &VcsDiffCommandInput,
    context: &VcsLoadContext,
    jj_executable: &Path,
) -> Result<String, VcsCatalogError> {
    let backed = JujutsuBackedInput::from(input);
    let arguments = build_jj_diff_args(input, None, true).map_err(VcsCatalogError::User)?;
    run_jj_text(
        &backed,
        &arguments,
        &command_context(&context.cwd, jj_executable),
    )
}

fn show_watch_signature(
    input: &VcsShowCommandInput,
    context: &VcsLoadContext,
    jj_executable: &Path,
) -> Result<String, VcsCatalogError> {
    run_jj_text(
        &JujutsuBackedInput::from(input),
        &build_jj_show_args(input, None),
        &command_context(&context.cwd, jj_executable),
    )
}

pub fn load_jujutsu_changeset(
    input: &VcsReviewInput,
    context: &VcsLoadContext,
    options: &JujutsuVcsAdapterOptions,
) -> Result<Changeset, VcsCatalogError> {
    let result = match input {
        VcsReviewInput::Diff(input) => load_working_tree(input, context, &options.jj_executable)?,
        VcsReviewInput::Show(input) => load_show(input, context, &options.jj_executable)?,
        VcsReviewInput::StashShow(_) => {
            return Err(VcsCatalogError::Operation(
                "Jujutsu does not support stash review".into(),
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
            ("jj:diff".into(), source)
        }
        VcsReviewInput::Show(input) => {
            let revset = input.reference.as_deref().unwrap_or("@");
            (
                format!("jj:show:{revset}"),
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

#[must_use]
pub fn create_jujutsu_vcs_adapter(options: JujutsuVcsAdapterOptions) -> VcsAdapter {
    let jj_executable = options.jj_executable;
    let mut operations: VcsOperations = BTreeMap::new();

    let load_jj = jj_executable.clone();
    let signature_jj = jj_executable.clone();
    operations.insert(
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsOperation {
            load: Arc::new(move |input, context| match input {
                VcsReviewInput::Diff(input) => load_working_tree(input, context, &load_jj),
                _ => Err(VcsCatalogError::Operation(
                    "Jujutsu working-tree operation received the wrong input".into(),
                )),
            }),
            watch_signature: Some(Arc::new(move |input, context| match input {
                VcsReviewInput::Diff(input) => {
                    working_tree_watch_signature(input, context, &signature_jj)
                }
                _ => Err(VcsCatalogError::Operation(
                    "Jujutsu working-tree signature received the wrong input".into(),
                )),
            })),
            watch_plan: Some(Arc::new(|_, _| Ok(VcsWatchPlan::poll_only()))),
        },
    );

    let load_jj = jj_executable.clone();
    let signature_jj = jj_executable;
    operations.insert(
        VcsReviewOperationKind::RevisionShow,
        VcsOperation {
            load: Arc::new(move |input, context| match input {
                VcsReviewInput::Show(input) => load_show(input, context, &load_jj),
                _ => Err(VcsCatalogError::Operation(
                    "Jujutsu revision operation received the wrong input".into(),
                )),
            }),
            watch_signature: Some(Arc::new(move |input, context| match input {
                VcsReviewInput::Show(input) => show_watch_signature(input, context, &signature_jj),
                _ => Err(VcsCatalogError::Operation(
                    "Jujutsu revision signature received the wrong input".into(),
                )),
            })),
            watch_plan: Some(Arc::new(|_, _| Ok(VcsWatchPlan::poll_only()))),
        },
    );

    VcsAdapter {
        id: "jj".into(),
        name: "Jujutsu".into(),
        detect: Arc::new(|cwd| Ok(detect_jujutsu_repo(cwd))),
        operations,
        detection_priority: Some(JUJUTSU_VCS_DETECTION_BASELINE_PRIORITY),
    }
}

#[cfg(test)]
mod tests;
