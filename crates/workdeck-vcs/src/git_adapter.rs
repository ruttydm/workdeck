//! Bundled Git adapter composed exclusively through Workdeck's public VCS contract.

use crate::{
    GitBackedInput, GitCommandContext, GitDiffEndpoint, GitDiffEndpoints, GitFileSourceOptions,
    GitMetadata, GitNumstatFile, LimitedSourceTextResult, VcsAdapter, VcsCatalogError,
    VcsDetection, VcsFileSourceResult, VcsLoadContext, VcsOperation, VcsOperations, VcsPatchResult,
    VcsReviewInput, VcsReviewOperationKind, VcsSourceReader, VcsWatchCoverage, VcsWatchPlan,
    VcsWatchTarget, VcsWatchTargetSource, build_git_diff_args, build_git_diff_numstat_args,
    build_git_show_args, build_git_stash_show_args, describe_diff_range, git_endpoint_source_spec,
    list_git_ignored_directory_roots, list_git_untracked_files, parse_git_numstat,
    read_git_file_source, resolve_git_color_moved_options, resolve_git_commit_ref,
    resolve_git_diff_endpoints, resolve_git_metadata, resolve_git_repo_root, run_git_text,
    should_skip_large_tracked_diff,
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use workdeck_core::{
    Changeset, ChangesetSource, DiffFile, FileChangeKind, FileFlags, FileSourceSnapshots,
    FileStats, ReviewSide, SourceOrigin, SourceSnapshot, VcsDiffCommandInput, VcsShowCommandInput,
    VcsStashShowCommandInput,
};

pub const GIT_VCS_DETECTION_BASELINE_PRIORITY: i32 = 0;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitVcsAdapterOptions {
    pub git_executable: PathBuf,
}

impl Default for GitVcsAdapterOptions {
    fn default() -> Self {
        Self {
            git_executable: PathBuf::from("git"),
        }
    }
}

#[derive(Clone)]
struct GitSourceCapability {
    read_file_source: VcsSourceReader,
    source_cache_key: String,
}

fn command_context(
    cwd: &Path,
    repo_root: Option<&Path>,
    git_executable: &Path,
    prevent_optional_locks: bool,
) -> GitCommandContext {
    GitCommandContext {
        cwd: cwd.to_owned(),
        repo_root: repo_root.map(Path::to_owned),
        git_executable: git_executable.to_owned(),
        prevent_optional_locks,
    }
}

fn basename(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .map_or_else(|| path.display().to_string(), str::to_owned)
}

fn detect_git_repo(cwd: &Path) -> Option<VcsDetection> {
    let mut current = cwd.canonicalize().unwrap_or_else(|_| absolute_path(cwd));
    loop {
        if current.join(".git").exists() {
            return Some(VcsDetection {
                id: "git".into(),
                repo_root: current,
            });
        }
        if !current.pop() {
            return None;
        }
    }
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

/// Stable stat fragment used to represent untracked content without reading it.
pub fn stat_signature(path: &Path) -> String {
    let Ok(metadata) = fs::metadata(path) else {
        return format!("{}:missing", path.display());
    };
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0_u128, |duration| duration.as_millis());
    format!(
        "{}:{}:{modified_ms}:{}",
        path.display(),
        metadata.len(),
        metadata_inode(&metadata)
    )
}

#[cfg(unix)]
fn metadata_inode(metadata: &fs::Metadata) -> u64 {
    use std::os::unix::fs::MetadataExt;
    metadata.ino()
}

#[cfg(windows)]
fn metadata_inode(metadata: &fs::Metadata) -> u64 {
    use std::os::windows::fs::MetadataExt;
    metadata.file_index().unwrap_or(0)
}

#[cfg(not(any(unix, windows)))]
fn metadata_inode(_metadata: &fs::Metadata) -> u64 {
    0
}

fn git_index_cache_key(
    input: &GitBackedInput,
    repo_root: &Path,
    git_executable: &Path,
) -> Result<String, VcsCatalogError> {
    let entries = run_git_text(
        input,
        &["ls-files".into(), "--stage".into(), "-z".into()],
        &command_context(repo_root, Some(repo_root), git_executable, false),
    )?;
    Ok(format!("{:x}", Sha256::digest(entries.as_bytes())))
}

fn git_endpoint_cache_key(endpoint: &GitDiffEndpoint, index_cache_key: &str) -> String {
    match endpoint {
        GitDiffEndpoint::GitRef(reference) => format!("ref:{reference}"),
        GitDiffEndpoint::Index => format!("index:{index_cache_key}"),
        GitDiffEndpoint::None => "none".into(),
        GitDiffEndpoint::Worktree => "worktree".into(),
    }
}

fn create_git_source_capability(
    input: &GitBackedInput,
    repo_root: &Path,
    endpoints: GitDiffEndpoints,
    git_executable: &Path,
) -> Result<GitSourceCapability, VcsCatalogError> {
    let needs_index =
        endpoints.old == GitDiffEndpoint::Index || endpoints.new == GitDiffEndpoint::Index;
    let index_key = if needs_index {
        git_index_cache_key(input, repo_root, git_executable)?
    } else {
        "unused".into()
    };
    let source_cache_key = format!(
        "git-source-v1:{}:{}",
        git_endpoint_cache_key(&endpoints.old, &index_key),
        git_endpoint_cache_key(&endpoints.new, &index_key)
    );
    let root = repo_root.to_owned();
    let executable = git_executable.to_owned();
    let reader: VcsSourceReader = Arc::new(move |request| {
        if request.side == ReviewSide::Old
            && matches!(
                request.change_kind,
                FileChangeKind::Added | FileChangeKind::Untracked
            )
        {
            return Ok(VcsFileSourceResult::Missing);
        }
        if request.side == ReviewSide::New && request.change_kind == FileChangeKind::Deleted {
            return Ok(VcsFileSourceResult::Missing);
        }
        let (endpoint, path) = match request.side {
            ReviewSide::Old => (
                &endpoints.old,
                request.previous_path.as_deref().unwrap_or(&request.path),
            ),
            ReviewSide::New => (&endpoints.new, request.path.as_str()),
        };
        let result = read_git_file_source(
            &git_endpoint_source_spec(endpoint, &root, Path::new(path)),
            &GitFileSourceOptions {
                git_executable: executable.clone(),
                ..GitFileSourceOptions::default()
            },
        );
        Ok(match result {
            LimitedSourceTextResult::Text(content) => {
                let origin = match endpoint {
                    GitDiffEndpoint::GitRef(reference) => SourceOrigin::Revision {
                        revision: reference.clone(),
                    },
                    GitDiffEndpoint::Index => SourceOrigin::Index,
                    GitDiffEndpoint::Worktree => SourceOrigin::WorkingTree,
                    GitDiffEndpoint::None => return Ok(VcsFileSourceResult::Missing),
                };
                VcsFileSourceResult::Source(SourceSnapshot::new(content, origin, true))
            }
            LimitedSourceTextResult::Missing => VcsFileSourceResult::Missing,
            LimitedSourceTextResult::TooLarge { max_bytes } => {
                VcsFileSourceResult::TooLarge { max_bytes }
            }
        })
    });
    Ok(GitSourceCapability {
        read_file_source: reader,
        source_cache_key,
    })
}

fn create_git_revision_source_capability(
    input: &GitBackedInput,
    reference: &str,
    repo_root: &Path,
    git_executable: &Path,
) -> Result<GitSourceCapability, VcsCatalogError> {
    let context = command_context(repo_root, Some(repo_root), git_executable, false);
    let new_reference = resolve_git_commit_ref(input, reference, &context)?;
    create_git_source_capability(
        input,
        repo_root,
        GitDiffEndpoints {
            old: GitDiffEndpoint::GitRef(format!("{new_reference}^")),
            new: GitDiffEndpoint::GitRef(new_reference),
        },
        git_executable,
    )
}

fn create_git_diff_source_capability(
    input: &VcsDiffCommandInput,
    repo_root: &Path,
    cwd: &Path,
    git_executable: &Path,
) -> Result<Option<GitSourceCapability>, VcsCatalogError> {
    let context = command_context(cwd, Some(repo_root), git_executable, false);
    resolve_git_diff_endpoints(input, &context)?
        .map(|endpoints| {
            create_git_source_capability(
                &GitBackedInput::from(input),
                repo_root,
                endpoints,
                git_executable,
            )
        })
        .transpose()
}

fn directory_contains(parent: &Path, child: &Path) -> bool {
    child.strip_prefix(parent).is_ok()
}

fn metadata_targets(metadata: &GitMetadata) -> Vec<VcsWatchTarget> {
    let directories = if directory_contains(&metadata.common_dir, &metadata.git_dir) {
        vec![metadata.common_dir.clone()]
    } else {
        vec![metadata.git_dir.clone(), metadata.common_dir.clone()]
    };
    directories
        .into_iter()
        .map(|directory| VcsWatchTarget::DirectoryTree {
            ignored_roots: vec![directory.join("objects")],
            directory,
            sources: BTreeSet::from([VcsWatchTargetSource::VcsMetadata]),
        })
        .collect()
}

fn build_git_watch_plan(
    input: &GitBackedInput,
    cwd: &Path,
    git_executable: &Path,
) -> Result<VcsWatchPlan, VcsCatalogError> {
    let base_context = command_context(cwd, None, git_executable, false);
    let metadata = resolve_git_metadata(input, &base_context)?;
    let mut targets = Vec::new();
    if let GitBackedInput::Diff(diff) = input {
        let endpoint_context =
            command_context(cwd, Some(&metadata.repo_root), git_executable, false);
        if resolve_git_diff_endpoints(diff, &endpoint_context)?
            .is_some_and(|endpoints| endpoints.new == GitDiffEndpoint::Worktree)
        {
            let mut ignored_roots = vec![metadata.repo_root.join(".git")];
            ignored_roots.extend(list_git_ignored_directory_roots(input, &endpoint_context));
            let mut seen = HashSet::new();
            ignored_roots.retain(|path| seen.insert(path.clone()));
            targets.push(VcsWatchTarget::DirectoryTree {
                directory: metadata.repo_root.clone(),
                ignored_roots,
                sources: BTreeSet::from([VcsWatchTargetSource::Worktree]),
            });
        }
    }
    targets.extend(metadata_targets(&metadata));
    Ok(VcsWatchPlan {
        coverage: VcsWatchCoverage::Hybrid,
        targets,
    })
}

fn skipped_tracked_file(file: GitNumstatFile, source_label: &str, index: usize) -> DiffFile {
    let mut result = DiffFile {
        key: String::new(),
        runtime_id: format!("git:skipped:{index}:{}", file.path),
        path: file.path,
        previous_path: None,
        change_kind: FileChangeKind::Modified,
        language: None,
        stats: FileStats {
            additions: file.additions,
            deletions: file.deletions,
            truncated: false,
        },
        flags: FileFlags {
            too_large: true,
            ..FileFlags::default()
        },
        patch: String::new(),
        split_row_count: 0,
        stack_row_count: 0,
        hunks: Vec::new(),
        content_identity: String::new(),
        sources: FileSourceSnapshots::default(),
        source_identity: None,
        source_capability: None,
        source_attested: false,
        agent: None,
    };
    result.refresh_identity();
    result.refresh_address(source_label, index);
    result
}

fn load_working_tree(
    input: &VcsDiffCommandInput,
    context: &VcsLoadContext,
    git_executable: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    let backed = GitBackedInput::from(input);
    let base_context = command_context(&context.cwd, None, git_executable, false);
    let repo_root = resolve_git_repo_root(&backed, &base_context)?;
    let repo_name = basename(&repo_root);
    let range = describe_diff_range(input);
    let title = if input.staged {
        format!("{repo_name} staged changes")
    } else if let Some(range) = range {
        format!("{repo_name} {range}")
    } else {
        format!("{repo_name} working tree")
    };
    let numstat = build_git_diff_numstat_args(input).map_err(VcsCatalogError::User)?;
    let large_files = parse_git_numstat(&run_git_text(&backed, &numstat, &base_context)?)
        .into_iter()
        .filter(|file| should_skip_large_tracked_diff(file, &repo_root))
        .collect::<Vec<_>>();
    let color_moved = resolve_git_color_moved_options(&backed, &base_context)?;
    let source_capability =
        create_git_diff_source_capability(input, &repo_root, &context.cwd, git_executable)?;
    let exclusions = large_files
        .iter()
        .map(|file| file.path.clone())
        .collect::<Vec<_>>();
    let patch_arguments = build_git_diff_args(input, &exclusions, color_moved.as_ref())
        .map_err(VcsCatalogError::User)?;
    let source_label = repo_root.display().to_string();
    let extra_files = large_files
        .into_iter()
        .enumerate()
        .map(|(index, file)| skipped_tracked_file(file, &source_label, index))
        .collect();
    let untracked_context = command_context(&context.cwd, Some(&repo_root), git_executable, false);
    Ok(VcsPatchResult {
        repo_root,
        source_label,
        title,
        patch_text: run_git_text(&backed, &patch_arguments, &base_context)?,
        untracked_paths: list_git_untracked_files(input, &untracked_context)?,
        source_reader: source_capability
            .as_ref()
            .map(|capability| Arc::clone(&capability.read_file_source)),
        source_cache_key: source_capability.map(|capability| capability.source_cache_key),
        extra_files,
    })
}

fn load_show(
    input: &VcsShowCommandInput,
    context: &VcsLoadContext,
    git_executable: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    let backed = GitBackedInput::from(input);
    let base_context = command_context(&context.cwd, None, git_executable, false);
    let repo_root = resolve_git_repo_root(&backed, &base_context)?;
    let repo_name = basename(&repo_root);
    let reference = input.reference.as_deref().unwrap_or("HEAD");
    let source =
        create_git_revision_source_capability(&backed, reference, &repo_root, git_executable)?;
    let color_moved = resolve_git_color_moved_options(&backed, &base_context)?;
    let arguments =
        build_git_show_args(input, color_moved.as_ref()).map_err(VcsCatalogError::User)?;
    Ok(VcsPatchResult {
        source_label: repo_root.display().to_string(),
        title: format!("{repo_name} show {reference}"),
        patch_text: run_git_text(&backed, &arguments, &base_context)?,
        repo_root,
        untracked_paths: Vec::new(),
        source_reader: Some(source.read_file_source),
        source_cache_key: Some(source.source_cache_key),
        extra_files: Vec::new(),
    })
}

fn load_stash(
    input: &VcsStashShowCommandInput,
    context: &VcsLoadContext,
    git_executable: &Path,
) -> Result<VcsPatchResult, VcsCatalogError> {
    let backed = GitBackedInput::from(input);
    let base_context = command_context(&context.cwd, None, git_executable, false);
    let repo_root = resolve_git_repo_root(&backed, &base_context)?;
    let repo_name = basename(&repo_root);
    let reference = input.reference.as_deref().unwrap_or("stash@{0}");
    let source =
        create_git_revision_source_capability(&backed, reference, &repo_root, git_executable)?;
    let color_moved = resolve_git_color_moved_options(&backed, &base_context)?;
    let arguments =
        build_git_stash_show_args(input, color_moved.as_ref()).map_err(VcsCatalogError::User)?;
    Ok(VcsPatchResult {
        source_label: repo_root.display().to_string(),
        title: input.reference.as_ref().map_or_else(
            || format!("{repo_name} stash"),
            |reference| format!("{repo_name} stash {reference}"),
        ),
        patch_text: run_git_text(&backed, &arguments, &base_context)?,
        repo_root,
        untracked_paths: Vec::new(),
        source_reader: Some(source.read_file_source),
        source_cache_key: Some(source.source_cache_key),
        extra_files: Vec::new(),
    })
}

fn diff_watch_signature(
    input: &VcsDiffCommandInput,
    context: &VcsLoadContext,
    git_executable: &Path,
) -> Result<String, VcsCatalogError> {
    let backed = GitBackedInput::from(input);
    let base = command_context(&context.cwd, None, git_executable, true);
    let arguments = build_git_diff_args(input, &[], None).map_err(VcsCatalogError::User)?;
    let tracked = run_git_text(&backed, &arguments, &base)?;
    let repo_root = resolve_git_repo_root(&backed, &base)?;
    let untracked_context = command_context(&context.cwd, Some(&repo_root), git_executable, true);
    let mut fragments = vec![tracked];
    fragments.extend(
        list_git_untracked_files(input, &untracked_context)?
            .into_iter()
            .map(|path| format!("untracked:{}", stat_signature(&repo_root.join(path)))),
    );
    Ok(fragments.join("\n---\n"))
}

fn patch_watch_signature(
    input: &GitBackedInput,
    arguments: Vec<String>,
    context: &VcsLoadContext,
    git_executable: &Path,
) -> Result<String, VcsCatalogError> {
    run_git_text(
        input,
        &arguments,
        &command_context(&context.cwd, None, git_executable, true),
    )
}

/// Load and materialize one Git review through the same adapter path exposed to extensions.
pub fn load_git_changeset(
    input: &VcsReviewInput,
    context: &VcsLoadContext,
    options: &GitVcsAdapterOptions,
) -> Result<Changeset, VcsCatalogError> {
    let result = match input {
        VcsReviewInput::Diff(input) => load_working_tree(input, context, &options.git_executable)?,
        VcsReviewInput::Show(input) => load_show(input, context, &options.git_executable)?,
        VcsReviewInput::StashShow(input) => load_stash(input, context, &options.git_executable)?,
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
                    from: Some(range.clone()),
                    to: "WORKTREE".into(),
                }
            } else {
                ChangesetSource::WorkingTree {
                    staged: input.staged,
                }
            };
            ("git:working".into(), source)
        }
        VcsReviewInput::Show(input) => {
            let reference = input.reference.as_deref().unwrap_or("HEAD");
            (
                format!("git:show:{reference}"),
                ChangesetSource::Revision {
                    from: None,
                    to: reference.into(),
                },
            )
        }
        VcsReviewInput::StashShow(input) => {
            let reference = input.reference.as_deref().unwrap_or("stash@{0}");
            (
                format!("git:stash:{reference}"),
                ChangesetSource::Stash {
                    reference: reference.into(),
                },
            )
        }
    };
    crate::materialize_vcs_patch_result(result, id, source)
}

/// Build the statically linked Git adapter, optionally overriding the provider executable.
pub fn create_git_vcs_adapter(options: GitVcsAdapterOptions) -> VcsAdapter {
    let git_executable = options.git_executable;
    let mut operations: VcsOperations = BTreeMap::new();

    let load_git = git_executable.clone();
    let plan_git = git_executable.clone();
    let signature_git = git_executable.clone();
    operations.insert(
        VcsReviewOperationKind::WorkingTreeDiff,
        VcsOperation {
            load: Arc::new(move |input, context| match input {
                VcsReviewInput::Diff(input) => load_working_tree(input, context, &load_git),
                _ => Err(VcsCatalogError::Operation(
                    "Git working-tree operation received the wrong input".into(),
                )),
            }),
            watch_plan: Some(Arc::new(move |input, context| {
                build_git_watch_plan(&GitBackedInput::try_from(input)?, &context.cwd, &plan_git)
            })),
            watch_signature: Some(Arc::new(move |input, context| match input {
                VcsReviewInput::Diff(input) => diff_watch_signature(input, context, &signature_git),
                _ => Err(VcsCatalogError::Operation(
                    "Git working-tree signature received the wrong input".into(),
                )),
            })),
        },
    );

    let load_git = git_executable.clone();
    let plan_git = git_executable.clone();
    let signature_git = git_executable.clone();
    operations.insert(
        VcsReviewOperationKind::RevisionShow,
        VcsOperation {
            load: Arc::new(move |input, context| match input {
                VcsReviewInput::Show(input) => load_show(input, context, &load_git),
                _ => Err(VcsCatalogError::Operation(
                    "Git revision operation received the wrong input".into(),
                )),
            }),
            watch_plan: Some(Arc::new(move |input, context| {
                build_git_watch_plan(&GitBackedInput::try_from(input)?, &context.cwd, &plan_git)
            })),
            watch_signature: Some(Arc::new(move |input, context| match input {
                VcsReviewInput::Show(input) => {
                    let backed = GitBackedInput::from(input);
                    let arguments =
                        build_git_show_args(input, None).map_err(VcsCatalogError::User)?;
                    patch_watch_signature(&backed, arguments, context, &signature_git)
                }
                _ => Err(VcsCatalogError::Operation(
                    "Git revision signature received the wrong input".into(),
                )),
            })),
        },
    );

    let load_git = git_executable.clone();
    let plan_git = git_executable.clone();
    let signature_git = git_executable;
    operations.insert(
        VcsReviewOperationKind::StashShow,
        VcsOperation {
            load: Arc::new(move |input, context| match input {
                VcsReviewInput::StashShow(input) => load_stash(input, context, &load_git),
                _ => Err(VcsCatalogError::Operation(
                    "Git stash operation received the wrong input".into(),
                )),
            }),
            watch_plan: Some(Arc::new(move |input, context| {
                build_git_watch_plan(&GitBackedInput::try_from(input)?, &context.cwd, &plan_git)
            })),
            watch_signature: Some(Arc::new(move |input, context| match input {
                VcsReviewInput::StashShow(input) => {
                    let backed = GitBackedInput::from(input);
                    let arguments =
                        build_git_stash_show_args(input, None).map_err(VcsCatalogError::User)?;
                    patch_watch_signature(&backed, arguments, context, &signature_git)
                }
                _ => Err(VcsCatalogError::Operation(
                    "Git stash signature received the wrong input".into(),
                )),
            })),
        },
    );

    VcsAdapter {
        id: "git".into(),
        name: "Git".into(),
        detect: Arc::new(|cwd| Ok(detect_git_repo(cwd))),
        operations,
        detection_priority: Some(GIT_VCS_DETECTION_BASELINE_PRIORITY),
    }
}

impl TryFrom<&VcsReviewInput> for GitBackedInput {
    type Error = VcsCatalogError;

    fn try_from(input: &VcsReviewInput) -> Result<Self, Self::Error> {
        Ok(match input {
            VcsReviewInput::Diff(input) => Self::from(input),
            VcsReviewInput::Show(input) => Self::from(input),
            VcsReviewInput::StashShow(input) => Self::from(input),
        })
    }
}

#[cfg(test)]
mod tests;
