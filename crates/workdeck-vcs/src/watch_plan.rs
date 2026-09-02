//! Backend-neutral planning for reloadable review inputs.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use thiserror::Error;
use workdeck_core::CliInput;

use crate::{
    VcsCatalog, VcsCatalogError, VcsLoadContext, VcsReviewInput, VcsWatchCoverage, VcsWatchPlan,
    VcsWatchTarget, VcsWatchTargetSource, create_vcs_watch_plan, get_configured_vcs_adapter,
    normalize_path_for_platform, operation_from_input,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchPlatform {
    Unix,
    MacOs,
    Windows,
}

impl WatchPlatform {
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Unix
        }
    }
}

#[derive(Clone, Copy)]
pub struct WatchPlanContext<'a> {
    pub cwd: &'a Path,
    pub platform: WatchPlatform,
    pub vcs_catalog: Option<&'a VcsCatalog>,
}

impl<'a> WatchPlanContext<'a> {
    #[must_use]
    pub const fn current(cwd: &'a Path, vcs_catalog: Option<&'a VcsCatalog>) -> Self {
        Self {
            cwd,
            platform: WatchPlatform::current(),
            vcs_catalog,
        }
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum WatchPlanError {
    #[error("VCS-backed watch plans require a composed VCS catalog.")]
    MissingVcsCatalog,
    #[error(transparent)]
    Vcs(#[from] VcsCatalogError),
}

#[derive(Debug)]
struct FileTarget<'a> {
    path: &'a str,
    source: VcsWatchTargetSource,
}

/// Resolve one review input into exact parent entries plus provider-owned watch targets.
pub fn resolve_watch_plan(
    input: &CliInput,
    context: WatchPlanContext<'_>,
) -> Result<Option<VcsWatchPlan>, WatchPlanError> {
    if input.options().agent_context.as_deref() == Some("-") {
        return Ok(None);
    }

    let mut file_targets = Vec::new();
    let mut coverage = VcsWatchCoverage::Hybrid;
    let mut adapter_targets = Vec::new();

    match input {
        CliInput::Files(input) => {
            file_targets.push(FileTarget {
                path: &input.left,
                source: VcsWatchTargetSource::Content,
            });
            file_targets.push(FileTarget {
                path: &input.right,
                source: VcsWatchTargetSource::Content,
            });
        }
        CliInput::DiffTool(input) => {
            file_targets.push(FileTarget {
                path: &input.left,
                source: VcsWatchTargetSource::Content,
            });
            file_targets.push(FileTarget {
                path: &input.right,
                source: VcsWatchTargetSource::Content,
            });
        }
        CliInput::Patch(input) => {
            let Some(path) = input.file.as_deref().filter(|path| *path != "-") else {
                return Ok(None);
            };
            file_targets.push(FileTarget {
                path,
                source: VcsWatchTargetSource::Content,
            });
        }
        CliInput::Vcs(input) => {
            let catalog = context
                .vcs_catalog
                .ok_or(WatchPlanError::MissingVcsCatalog)?;
            let operation = operation_from_input(VcsReviewInput::Diff(input.clone()));
            let adapter = get_configured_vcs_adapter(
                configured_vcs_id(input.options.vcs.as_deref()),
                catalog,
            )?;
            let plan = create_vcs_watch_plan(
                adapter,
                &operation,
                &VcsLoadContext {
                    cwd: context.cwd.to_owned(),
                },
                catalog,
            )?;
            coverage = plan.coverage;
            adapter_targets = plan.targets;
        }
        CliInput::Show(input) => {
            let catalog = context
                .vcs_catalog
                .ok_or(WatchPlanError::MissingVcsCatalog)?;
            let operation = operation_from_input(VcsReviewInput::Show(input.clone()));
            let adapter = get_configured_vcs_adapter(
                configured_vcs_id(input.options.vcs.as_deref()),
                catalog,
            )?;
            let plan = create_vcs_watch_plan(
                adapter,
                &operation,
                &VcsLoadContext {
                    cwd: context.cwd.to_owned(),
                },
                catalog,
            )?;
            coverage = plan.coverage;
            adapter_targets = plan.targets;
        }
        CliInput::StashShow(input) => {
            let catalog = context
                .vcs_catalog
                .ok_or(WatchPlanError::MissingVcsCatalog)?;
            let operation = operation_from_input(VcsReviewInput::StashShow(input.clone()));
            let adapter = get_configured_vcs_adapter(
                configured_vcs_id(input.options.vcs.as_deref()),
                catalog,
            )?;
            let plan = create_vcs_watch_plan(
                adapter,
                &operation,
                &VcsLoadContext {
                    cwd: context.cwd.to_owned(),
                },
                catalog,
            )?;
            coverage = plan.coverage;
            adapter_targets = plan.targets;
        }
    }

    if let Some(agent_context) = input.options().agent_context.as_deref() {
        file_targets.push(FileTarget {
            path: agent_context,
            source: VcsWatchTargetSource::Sidecar,
        });
        coverage = VcsWatchCoverage::Hybrid;
    }

    adapter_targets.extend(group_file_targets(file_targets, context));
    Ok(Some(VcsWatchPlan {
        coverage,
        targets: adapter_targets,
    }))
}

fn configured_vcs_id(id: Option<&str>) -> Option<&str> {
    id.filter(|id| *id != "auto")
}

fn group_file_targets(
    targets: Vec<FileTarget<'_>>,
    context: WatchPlanContext<'_>,
) -> Vec<VcsWatchTarget> {
    let mut groups = BTreeMap::<
        String,
        (
            PathBuf,
            BTreeMap<String, String>,
            BTreeSet<VcsWatchTargetSource>,
        ),
    >::new();
    for target in targets {
        let path = resolve_source_path(target.path, context.cwd, context.platform);
        let path_text = path.to_string_lossy().into_owned();
        let directory = parent_path(&path_text, context.platform);
        let directory_key = comparison_key(&directory, context.platform);
        let group = groups
            .entry(directory_key)
            .or_insert_with(|| (PathBuf::from(&directory), BTreeMap::new(), BTreeSet::new()));
        group
            .1
            .entry(comparison_key(&path_text, context.platform))
            .or_insert(path_text);
        group.2.insert(target.source);
    }
    groups
        .into_values()
        .map(
            |(directory, entries, sources)| VcsWatchTarget::DirectoryEntries {
                directory,
                entries: entries.into_values().collect(),
                sources,
            },
        )
        .collect()
}

fn comparison_key(path: &str, platform: WatchPlatform) -> String {
    match platform {
        WatchPlatform::Unix | WatchPlatform::MacOs => path.to_owned(),
        WatchPlatform::Windows => path.to_ascii_lowercase(),
    }
}

fn parent_path(path: &str, platform: WatchPlatform) -> String {
    match platform {
        WatchPlatform::Unix | WatchPlatform::MacOs => Path::new(path)
            .parent()
            .unwrap_or_else(|| Path::new("/"))
            .to_string_lossy()
            .into_owned(),
        WatchPlatform::Windows => path.rfind('\\').map_or_else(
            || ".".into(),
            |index| {
                if index == 2 && path.as_bytes().get(1) == Some(&b':') {
                    path[..=index].to_owned()
                } else {
                    path[..index].to_owned()
                }
            },
        ),
    }
}

fn resolve_source_path(path: &str, cwd: &Path, platform: WatchPlatform) -> PathBuf {
    match platform {
        WatchPlatform::Unix | WatchPlatform::MacOs => normalize_unix_path(path, cwd),
        WatchPlatform::Windows => PathBuf::from(normalize_windows_path(
            &normalize_path_for_platform(path, "win32"),
            &normalize_path_for_platform(&cwd.to_string_lossy(), "win32"),
        )),
    }
}

fn normalize_unix_path(path: &str, cwd: &Path) -> PathBuf {
    let joined = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in joined.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::RootDir | Component::Prefix(_) | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
        }
    }
    normalized
}

fn normalize_windows_path(path: &str, cwd: &str) -> String {
    let path = path.replace('/', "\\");
    let cwd = cwd.replace('/', "\\");
    let absolute = path.as_bytes().get(1) == Some(&b':')
        && path
            .as_bytes()
            .get(2)
            .is_some_and(|separator| *separator == b'\\');
    let joined = if absolute {
        path
    } else if path.starts_with('\\') {
        format!("{}{}", &cwd[..cwd.len().min(2)], path)
    } else {
        format!("{}\\{}", cwd.trim_end_matches('\\'), path)
    };
    let (drive, remainder) = joined.split_at(joined.len().min(2));
    let mut components = Vec::new();
    for component in remainder
        .split('\\')
        .filter(|component| !component.is_empty())
    {
        match component {
            "." => {}
            ".." => {
                components.pop();
            }
            component => components.push(component),
        }
    }
    if components.is_empty() {
        format!("{drive}\\")
    } else {
        format!("{drive}\\{}", components.join("\\"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use workdeck_core::{
        CommonOptions, DiffToolCommandInput, FileCommandInput, PatchCommandInput,
        VcsDiffCommandInput, VcsShowCommandInput,
    };

    use crate::{VcsOperation, VcsPatchResult, VcsReviewOperationKind, create_base_vcs_catalog};

    fn options(agent_context: Option<&str>) -> CommonOptions {
        CommonOptions {
            agent_context: agent_context.map(str::to_owned),
            ..CommonOptions::default()
        }
    }

    fn load_operation(plan: Option<VcsWatchPlan>) -> VcsOperation {
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
            watch_signature: None,
            watch_plan: plan.map(|plan| {
                Arc::new(move |_input: &VcsReviewInput, _context: &VcsLoadContext| plan.clone())
                    as _
            }),
        }
    }

    fn catalog(id: &str, plan: Option<VcsWatchPlan>) -> VcsCatalog {
        let operation = load_operation(plan);
        let adapter = crate::VcsAdapter {
            id: id.into(),
            name: id.to_uppercase(),
            detect: Arc::new(|_| Ok(None)),
            operations: [
                (VcsReviewOperationKind::WorkingTreeDiff, operation.clone()),
                (VcsReviewOperationKind::RevisionShow, operation),
            ]
            .into_iter()
            .collect(),
            detection_priority: None,
        };
        create_base_vcs_catalog(vec![adapter], id)
    }

    fn context<'a>(cwd: &'a Path, catalog: Option<&'a VcsCatalog>) -> WatchPlanContext<'a> {
        WatchPlanContext {
            cwd,
            platform: WatchPlatform::Unix,
            vcs_catalog: catalog,
        }
    }

    fn entries(
        directory: &str,
        paths: &[&str],
        sources: &[VcsWatchTargetSource],
    ) -> VcsWatchTarget {
        VcsWatchTarget::DirectoryEntries {
            directory: directory.into(),
            entries: paths.iter().map(|path| (*path).into()).collect(),
            sources: sources.iter().copied().collect(),
        }
    }

    #[test]
    fn plans_directory_entries_for_diff_difftool_and_patch_file_inputs() {
        let cwd = Path::new("/workspace/review");
        let cases = [
            (
                CliInput::Files(FileCommandInput {
                    left: "before/file.ts".into(),
                    right: "after/file.ts".into(),
                    options: options(None),
                }),
                vec![
                    entries(
                        "/workspace/review/after",
                        &["/workspace/review/after/file.ts"],
                        &[VcsWatchTargetSource::Content],
                    ),
                    entries(
                        "/workspace/review/before",
                        &["/workspace/review/before/file.ts"],
                        &[VcsWatchTargetSource::Content],
                    ),
                ],
            ),
            (
                CliInput::DiffTool(DiffToolCommandInput {
                    left: "tmp/old.ts".into(),
                    right: "tmp/new.ts".into(),
                    path: Some("src/display-only.ts".into()),
                    options: options(None),
                }),
                vec![entries(
                    "/workspace/review/tmp",
                    &[
                        "/workspace/review/tmp/new.ts",
                        "/workspace/review/tmp/old.ts",
                    ],
                    &[VcsWatchTargetSource::Content],
                )],
            ),
            (
                CliInput::Patch(PatchCommandInput {
                    file: Some("incoming/review.patch".into()),
                    text: None,
                    options: options(None),
                }),
                vec![entries(
                    "/workspace/review/incoming",
                    &["/workspace/review/incoming/review.patch"],
                    &[VcsWatchTargetSource::Content],
                )],
            ),
        ];
        for (input, targets) in cases {
            assert_eq!(
                resolve_watch_plan(&input, context(cwd, None)).unwrap(),
                Some(VcsWatchPlan {
                    coverage: VcsWatchCoverage::Hybrid,
                    targets
                })
            );
        }
    }

    #[test]
    fn plans_missing_paths_without_consulting_the_filesystem() {
        let input = CliInput::Patch(PatchCommandInput {
            file: Some("not-created-yet/review.patch".into()),
            text: None,
            options: options(None),
        });
        assert!(
            resolve_watch_plan(&input, context(Path::new("/workspace/review"), None))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn adds_agent_sidecars_to_every_direct_file_input() {
        let cwd = Path::new("/workspace/review");
        let cases = [
            CliInput::Files(FileCommandInput {
                left: "left.ts".into(),
                right: "right.ts".into(),
                options: options(Some("notes/agent.json")),
            }),
            CliInput::DiffTool(DiffToolCommandInput {
                left: "left.ts".into(),
                right: "right.ts".into(),
                path: Some("display.ts".into()),
                options: options(Some("notes/agent.json")),
            }),
            CliInput::Patch(PatchCommandInput {
                file: Some("review.patch".into()),
                text: None,
                options: options(Some("notes/agent.json")),
            }),
        ];
        for input in cases {
            let plan = resolve_watch_plan(&input, context(cwd, None))
                .unwrap()
                .unwrap();
            assert!(plan.targets.iter().any(|target| matches!(target, VcsWatchTarget::DirectoryEntries { sources, .. } if sources.contains(&VcsWatchTargetSource::Sidecar))));
        }
    }

    #[test]
    fn deduplicates_lexical_paths_and_retains_all_sources() {
        let input = CliInput::Files(FileCommandInput {
            left: "src/./file.ts".into(),
            right: "src/other/../file.ts".into(),
            options: options(Some("src/notes.json")),
        });
        let plan = resolve_watch_plan(&input, context(Path::new("/workspace/review"), None))
            .unwrap()
            .unwrap();
        assert_eq!(
            plan.targets,
            [entries(
                "/workspace/review/src",
                &[
                    "/workspace/review/src/file.ts",
                    "/workspace/review/src/notes.json"
                ],
                &[VcsWatchTargetSource::Content, VcsWatchTargetSource::Sidecar]
            )]
        );
    }

    #[test]
    fn preserves_absolute_paths() {
        let input = CliInput::Files(FileCommandInput {
            left: "/snapshots/before.ts".into(),
            right: "after.ts".into(),
            options: options(None),
        });
        let plan = resolve_watch_plan(&input, context(Path::new("/workspace/review"), None))
            .unwrap()
            .unwrap();
        assert_eq!(
            plan.targets[0],
            entries(
                "/snapshots",
                &["/snapshots/before.ts"],
                &[VcsWatchTargetSource::Content]
            )
        );
    }

    #[test]
    fn stdin_patch_and_agent_context_are_unwatchable() {
        let cwd = Path::new("/workspace/review");
        let cases = [
            CliInput::Patch(PatchCommandInput {
                file: Some("-".into()),
                text: None,
                options: options(None),
            }),
            CliInput::Patch(PatchCommandInput {
                file: None,
                text: Some("diff --git a/a b/a".into()),
                options: options(None),
            }),
            CliInput::Files(FileCommandInput {
                left: "left".into(),
                right: "right".into(),
                options: options(Some("-")),
            }),
        ];
        for input in cases {
            assert_eq!(
                resolve_watch_plan(&input, context(cwd, None)).unwrap(),
                None
            );
        }
    }

    #[test]
    fn poll_only_provider_stays_poll_only_until_a_sidecar_adds_event_coverage() {
        let catalog = catalog("jj", None);
        let cwd = Path::new("/workspace/review");
        let diff = CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions {
                vcs: Some("jj".into()),
                ..options(None)
            },
        });
        let show = CliInput::Show(VcsShowCommandInput {
            reference: None,
            pathspecs: Vec::new(),
            options: CommonOptions {
                vcs: Some("jj".into()),
                ..options(Some("agent.json"))
            },
        });
        assert_eq!(
            resolve_watch_plan(&diff, context(cwd, Some(&catalog)))
                .unwrap()
                .unwrap(),
            VcsWatchPlan::poll_only()
        );
        assert_eq!(
            resolve_watch_plan(&show, context(cwd, Some(&catalog)))
                .unwrap()
                .unwrap()
                .coverage,
            VcsWatchCoverage::Hybrid
        );
    }

    #[test]
    fn provider_watch_capability_is_composed_without_conversion() {
        let target = VcsWatchTarget::DirectoryTree {
            directory: "/workspace/review/.hg".into(),
            ignored_roots: Vec::new(),
            sources: [VcsWatchTargetSource::VcsMetadata].into_iter().collect(),
        };
        let catalog = catalog(
            "hg",
            Some(VcsWatchPlan {
                coverage: VcsWatchCoverage::Hybrid,
                targets: vec![target.clone()],
            }),
        );
        let input = CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions {
                vcs: Some("hg".into()),
                ..options(None)
            },
        });
        assert_eq!(
            resolve_watch_plan(
                &input,
                context(Path::new("/workspace/review"), Some(&catalog))
            )
            .unwrap()
            .unwrap()
            .targets,
            [target]
        );
    }

    #[test]
    fn provider_without_watch_capability_uses_polling_fallback() {
        let catalog = catalog("hg", None);
        let input = CliInput::Vcs(VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions {
                vcs: Some("hg".into()),
                ..options(None)
            },
        });
        assert_eq!(
            resolve_watch_plan(
                &input,
                context(Path::new("/workspace/review"), Some(&catalog))
            )
            .unwrap(),
            Some(VcsWatchPlan::poll_only())
        );
    }

    #[test]
    fn windows_paths_are_normalized_case_insensitively() {
        let input = CliInput::Files(FileCommandInput {
            left: "src\\before.ts".into(),
            right: "C:/work/review/src/after.ts".into(),
            options: options(Some("/c/work/review/src/agent.json")),
        });
        let plan = resolve_watch_plan(
            &input,
            WatchPlanContext {
                cwd: Path::new(r"C:\work\review"),
                platform: WatchPlatform::Windows,
                vcs_catalog: None,
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            plan.targets,
            [entries(
                r"C:\work\review\src",
                &[
                    r"C:\work\review\src\after.ts",
                    r"C:\work\review\src\agent.json",
                    r"C:\work\review\src\before.ts"
                ],
                &[VcsWatchTargetSource::Content, VcsWatchTargetSource::Sidecar]
            )]
        );
    }
}
