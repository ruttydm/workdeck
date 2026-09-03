//! Provider-neutral adapter catalog, operation dispatch, and checkout detection.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use workdeck_core::{
    CliInput, DiffFile, FileChangeKind, ReviewSide, SourceSnapshot, VcsDiffCommandInput,
    VcsShowCommandInput, VcsStashShowCommandInput, WorkdeckUserError,
};

pub const DEFAULT_VCS_PROVIDER_ID: &str = "git";
pub const BUNDLED_VCS_PROVIDER_IDS: &[&str] = &["jj", "sl", "git"];
pub const DEFAULT_EXTENSION_VCS_DETECTION_PRIORITY: i32 = -100;

pub type VcsId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsDetection {
    pub id: VcsId,
    pub repo_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsLoadContext {
    pub cwd: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcsReviewInput {
    Diff(VcsDiffCommandInput),
    Show(VcsShowCommandInput),
    StashShow(VcsStashShowCommandInput),
}

/// Compact comparison spelling used by titles and Git arguments.
#[must_use]
pub fn describe_diff_range(input: &VcsDiffCommandInput) -> Option<String> {
    input.range_endpoints.as_ref().map_or_else(
        || input.range.clone(),
        |endpoints| Some(format!("{}..{}", endpoints.from, endpoints.to)),
    )
}

/// Positional spelling required by Jujutsu and Sapling process arguments.
#[must_use]
pub fn describe_diff_targets(input: &VcsDiffCommandInput) -> Option<String> {
    input.range_endpoints.as_ref().map_or_else(
        || input.range.clone(),
        |endpoints| Some(format!("{} {}", endpoints.from, endpoints.to)),
    )
}

#[must_use]
pub fn has_explicit_diff_target(input: &VcsDiffCommandInput) -> bool {
    describe_diff_range(input).is_some()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VcsReviewOperationKind {
    WorkingTreeDiff,
    RevisionShow,
    StashShow,
}

impl VcsReviewOperationKind {
    fn wire_name(self) -> &'static str {
        match self {
            Self::WorkingTreeDiff => "working-tree-diff",
            Self::RevisionShow => "revision-show",
            Self::StashShow => "stash-show",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsReviewOperation {
    pub kind: VcsReviewOperationKind,
    pub input: VcsReviewInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsFileSourceRequest {
    pub path: String,
    pub previous_path: Option<String>,
    pub change_kind: FileChangeKind,
    pub is_untracked: bool,
    pub side: ReviewSide,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcsFileSourceResult {
    Source(SourceSnapshot),
    Missing,
    TooLarge { max_bytes: usize },
}

pub type VcsSourceReader = Arc<
    dyn Fn(&VcsFileSourceRequest) -> Result<VcsFileSourceResult, VcsCatalogError> + Send + Sync,
>;

#[derive(Clone)]
pub struct VcsPatchResult {
    pub repo_root: PathBuf,
    pub source_label: String,
    pub title: String,
    pub patch_text: String,
    pub untracked_paths: Vec<PathBuf>,
    pub source_reader: Option<VcsSourceReader>,
    pub source_cache_key: Option<String>,
    pub extra_files: Vec<DiffFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VcsWatchCoverage {
    Hybrid,
    PollOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum VcsWatchTargetSource {
    Content,
    Sidecar,
    Worktree,
    VcsMetadata,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcsWatchTarget {
    DirectoryEntries {
        directory: PathBuf,
        entries: Vec<String>,
        sources: BTreeSet<VcsWatchTargetSource>,
    },
    DirectoryTree {
        directory: PathBuf,
        ignored_roots: Vec<PathBuf>,
        sources: BTreeSet<VcsWatchTargetSource>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VcsWatchPlan {
    pub coverage: VcsWatchCoverage,
    pub targets: Vec<VcsWatchTarget>,
}

impl VcsWatchPlan {
    #[must_use]
    pub const fn poll_only() -> Self {
        Self {
            coverage: VcsWatchCoverage::PollOnly,
            targets: Vec::new(),
        }
    }
}

type LoadOperation = Arc<
    dyn Fn(&VcsReviewInput, &VcsLoadContext) -> Result<VcsPatchResult, VcsCatalogError>
        + Send
        + Sync,
>;
type WatchSignature =
    Arc<dyn Fn(&VcsReviewInput, &VcsLoadContext) -> Result<String, VcsCatalogError> + Send + Sync>;
type WatchPlan = Arc<
    dyn Fn(&VcsReviewInput, &VcsLoadContext) -> Result<VcsWatchPlan, VcsCatalogError> + Send + Sync,
>;

#[derive(Clone)]
pub struct VcsOperation {
    pub load: LoadOperation,
    pub watch_signature: Option<WatchSignature>,
    pub watch_plan: Option<WatchPlan>,
}

pub type VcsOperations = BTreeMap<VcsReviewOperationKind, VcsOperation>;
type DetectAdapter = Arc<dyn Fn(&Path) -> Result<Option<VcsDetection>, String> + Send + Sync>;

#[derive(Clone)]
pub struct VcsAdapter {
    pub id: VcsId,
    pub name: String,
    pub detect: DetectAdapter,
    pub operations: VcsOperations,
    pub detection_priority: Option<i32>,
}

#[derive(Clone)]
pub struct VcsCatalog {
    pub adapters: Vec<VcsAdapter>,
    pub default_adapter_id: VcsId,
    pub reserved_ids: BTreeSet<VcsId>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum VcsCatalogError {
    #[error(transparent)]
    User(#[from] WorkdeckUserError),
    #[error("Unsupported VCS: {0}")]
    UnsupportedVcs(String),
    #[error("{adapter} does not support watch signatures for {operation}.")]
    MissingWatchSignature { adapter: String, operation: String },
    #[error("VCS operation failed: {0}")]
    Operation(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledVcsCatalog {
    pub default_provider_id: &'static str,
    pub provider_ids: &'static [&'static str],
}

pub const fn bundled_vcs_catalog_metadata() -> BundledVcsCatalog {
    BundledVcsCatalog {
        default_provider_id: DEFAULT_VCS_PROVIDER_ID,
        provider_ids: BUNDLED_VCS_PROVIDER_IDS,
    }
}

pub fn create_vcs_catalog(
    mut adapters: Vec<VcsAdapter>,
    default_adapter_id: impl Into<String>,
    reserved_ids: impl IntoIterator<Item = VcsId>,
) -> VcsCatalog {
    adapters.sort_by(|left, right| {
        right
            .detection_priority
            .unwrap_or(DEFAULT_EXTENSION_VCS_DETECTION_PRIORITY)
            .cmp(
                &left
                    .detection_priority
                    .unwrap_or(DEFAULT_EXTENSION_VCS_DETECTION_PRIORITY),
            )
    });
    VcsCatalog {
        adapters,
        default_adapter_id: default_adapter_id.into(),
        reserved_ids: reserved_ids.into_iter().collect(),
    }
}

#[must_use]
pub fn create_base_vcs_catalog(
    adapters: Vec<VcsAdapter>,
    default_adapter_id: impl Into<String>,
) -> VcsCatalog {
    let reserved = adapters
        .iter()
        .map(|adapter| adapter.id.clone())
        .collect::<Vec<_>>();
    create_vcs_catalog(adapters, default_adapter_id, reserved)
}

#[must_use]
pub fn extend_vcs_catalog(base: &VcsCatalog, extra_adapters: Vec<VcsAdapter>) -> VcsCatalog {
    let mut claimed = base
        .adapters
        .iter()
        .map(|adapter| adapter.id.clone())
        .collect::<BTreeSet<_>>();
    let accepted = extra_adapters
        .into_iter()
        .filter(|adapter| {
            !base.reserved_ids.contains(&adapter.id) && claimed.insert(adapter.id.clone())
        })
        .collect::<Vec<_>>();
    create_vcs_catalog(
        base.adapters.iter().cloned().chain(accepted).collect(),
        base.default_adapter_id.clone(),
        base.reserved_ids.iter().cloned(),
    )
}

pub fn get_default_vcs_adapter(catalog: &VcsCatalog) -> Result<&VcsAdapter, VcsCatalogError> {
    catalog
        .adapters
        .iter()
        .find(|adapter| adapter.id == catalog.default_adapter_id)
        .ok_or_else(|| {
            WorkdeckUserError::new(
                format!(
                    "Workdeck's default {} backend failed to load.",
                    catalog.default_adapter_id
                ),
                vec!["Reinstall Workdeck, or report this upstream.".into()],
            )
            .into()
        })
}

pub fn get_configured_vcs_adapter<'a>(
    id: Option<&str>,
    catalog: &'a VcsCatalog,
) -> Result<&'a VcsAdapter, VcsCatalogError> {
    id.map_or_else(
        || get_default_vcs_adapter(catalog),
        |id| get_vcs_adapter(id, catalog),
    )
}

pub fn get_vcs_adapter<'a>(
    id: &str,
    catalog: &'a VcsCatalog,
) -> Result<&'a VcsAdapter, VcsCatalogError> {
    catalog
        .adapters
        .iter()
        .find(|adapter| adapter.id == id)
        .ok_or_else(|| VcsCatalogError::UnsupportedVcs(id.to_owned()))
}

#[must_use]
pub fn is_vcs_id(value: &str, catalog: &VcsCatalog) -> bool {
    catalog.reserved_ids.contains(value)
}

#[must_use]
pub fn detect_vcs(cwd: &Path, catalog: &VcsCatalog) -> Option<VcsDetection> {
    let start = cwd.canonicalize().unwrap_or_else(|_| cwd.to_owned());
    let mut best: Option<(usize, VcsDetection)> = None;
    for adapter in &catalog.adapters {
        let Ok(Some(detection)) = (adapter.detect)(&start) else {
            continue;
        };
        let distance = relative_component_distance(&detection.repo_root, &start);
        if best
            .as_ref()
            .is_none_or(|(best_distance, _)| distance < *best_distance)
        {
            best = Some((distance, detection));
        }
    }
    best.map(|(_, detection)| detection)
}

#[must_use]
pub const fn is_vcs_review_input(input: &CliInput) -> bool {
    matches!(
        input,
        CliInput::Vcs(_) | CliInput::Show(_) | CliInput::StashShow(_)
    )
}

#[must_use]
pub fn operation_from_input(input: VcsReviewInput) -> VcsReviewOperation {
    let kind = match input {
        VcsReviewInput::Diff(_) => VcsReviewOperationKind::WorkingTreeDiff,
        VcsReviewInput::Show(_) => VcsReviewOperationKind::RevisionShow,
        VcsReviewInput::StashShow(_) => VcsReviewOperationKind::StashShow,
    };
    VcsReviewOperation { kind, input }
}

#[must_use]
pub fn get_vcs_operation<'a>(
    adapter: &'a VcsAdapter,
    operation: &VcsReviewOperation,
) -> Option<&'a VcsOperation> {
    adapter.operations.get(&operation.kind)
}

pub fn load_vcs_review(
    adapter: &VcsAdapter,
    operation: &VcsReviewOperation,
    context: &VcsLoadContext,
    catalog: &VcsCatalog,
) -> Result<VcsPatchResult, VcsCatalogError> {
    let handler = get_vcs_operation(adapter, operation)
        .ok_or_else(|| create_unsupported_vcs_operation_error(adapter, operation.kind, catalog))?;
    (handler.load)(&operation.input, context)
}

pub fn create_vcs_watch_plan(
    adapter: &VcsAdapter,
    operation: &VcsReviewOperation,
    context: &VcsLoadContext,
    catalog: &VcsCatalog,
) -> Result<VcsWatchPlan, VcsCatalogError> {
    let handler = get_vcs_operation(adapter, operation)
        .ok_or_else(|| create_unsupported_vcs_operation_error(adapter, operation.kind, catalog))?;
    handler.watch_plan.as_ref().map_or_else(
        || Ok(VcsWatchPlan::poll_only()),
        |plan| plan(&operation.input, context),
    )
}

pub fn create_vcs_watch_signature(
    adapter: &VcsAdapter,
    operation: &VcsReviewOperation,
    context: &VcsLoadContext,
    catalog: &VcsCatalog,
) -> Result<String, VcsCatalogError> {
    let handler = get_vcs_operation(adapter, operation)
        .ok_or_else(|| create_unsupported_vcs_operation_error(adapter, operation.kind, catalog))?;
    let signature =
        handler
            .watch_signature
            .as_ref()
            .ok_or_else(|| VcsCatalogError::MissingWatchSignature {
                adapter: adapter.name.clone(),
                operation: operation.kind.wire_name().into(),
            })?;
    signature(&operation.input, context)
}

#[must_use]
pub fn create_unsupported_vcs_operation_error(
    adapter: &VcsAdapter,
    operation: VcsReviewOperationKind,
    catalog: &VcsCatalog,
) -> VcsCatalogError {
    let supporting = catalog
        .adapters
        .iter()
        .find(|candidate| candidate.operations.contains_key(&operation));
    if operation == VcsReviewOperationKind::StashShow
        && let Some(supporting) = supporting
    {
        return WorkdeckUserError::new(
            format!(
                "`workdeck stash show` requires {} VCS mode.",
                supporting.name
            ),
            vec![format!(
                "Set `vcs = \"{}\"` in Workdeck config, then try again.",
                supporting.id
            )],
        )
        .into();
    }
    WorkdeckUserError::new(
        format!(
            "{} does not support {}.",
            adapter.name,
            operation.wire_name()
        ),
        vec!["Use a supported VCS mode or command for this repository.".into()],
    )
    .into()
}

fn relative_component_distance(root: &Path, start: &Path) -> usize {
    if let Ok(relative) = start.strip_prefix(root) {
        return relative.components().count();
    }
    let root = root
        .components()
        .filter(normal_component)
        .collect::<Vec<_>>();
    let start = start
        .components()
        .filter(normal_component)
        .collect::<Vec<_>>();
    let common = root
        .iter()
        .zip(&start)
        .take_while(|(left, right)| left == right)
        .count();
    root.len() - common + start.len() - common
}

fn normal_component(component: &Component<'_>) -> bool {
    !matches!(component, Component::CurDir)
}

pub fn find_project_root_candidate(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.canonicalize().ok()?;
    loop {
        if current.join(".agents/workdeck").is_dir()
            || [".jj", ".sl", ".git"]
                .iter()
                .any(|marker| current.join(marker).exists())
        {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;
    use workdeck_core::{CommonOptions, PatchCommandInput, VcsRangeEndpoints};

    use super::*;

    fn adapter(
        id: &str,
        root: Option<&str>,
        priority: Option<i32>,
        operations: VcsOperations,
    ) -> VcsAdapter {
        let id_owned = id.to_owned();
        let root = root.map(PathBuf::from);
        VcsAdapter {
            id: id_owned.clone(),
            name: id.to_uppercase(),
            detection_priority: priority,
            detect: Arc::new(move |_| {
                Ok(root.as_ref().map(|root| VcsDetection {
                    id: id_owned.clone(),
                    repo_root: root.clone(),
                }))
            }),
            operations,
        }
    }

    fn load_operation() -> VcsOperation {
        VcsOperation {
            load: Arc::new(|_, context| {
                Ok(VcsPatchResult {
                    repo_root: context.cwd.clone(),
                    source_label: context.cwd.display().to_string(),
                    title: "review".into(),
                    patch_text: String::new(),
                    untracked_paths: Vec::new(),
                    source_reader: None,
                    source_cache_key: None,
                    extra_files: Vec::new(),
                })
            }),
            watch_signature: None,
            watch_plan: None,
        }
    }

    fn diff_input() -> VcsDiffCommandInput {
        VcsDiffCommandInput {
            range: None,
            range_endpoints: None,
            staged: false,
            pathspecs: Vec::new(),
            options: CommonOptions::default(),
        }
    }

    fn show_input() -> VcsShowCommandInput {
        VcsShowCommandInput {
            reference: None,
            pathspecs: Vec::new(),
            options: CommonOptions::default(),
        }
    }

    #[test]
    fn diff_range_descriptions_preserve_endpoint_and_backend_spellings() {
        let mut input = diff_input();
        assert_eq!(describe_diff_range(&input), None);
        assert_eq!(describe_diff_targets(&input), None);
        assert!(!has_explicit_diff_target(&input));

        input.range = Some("main...topic".into());
        assert_eq!(describe_diff_range(&input).as_deref(), Some("main...topic"));
        assert_eq!(
            describe_diff_targets(&input).as_deref(),
            Some("main...topic")
        );
        assert!(has_explicit_diff_target(&input));

        input.range_endpoints = Some(VcsRangeEndpoints {
            from: "release".into(),
            to: "topic".into(),
        });
        assert_eq!(
            describe_diff_range(&input).as_deref(),
            Some("release..topic")
        );
        assert_eq!(
            describe_diff_targets(&input).as_deref(),
            Some("release topic")
        );
    }

    #[test]
    fn catalog_orders_priority_stably_and_owns_bundled_defaults() {
        let catalog = create_base_vcs_catalog(
            vec![
                adapter("first", None, Some(10), BTreeMap::new()),
                adapter("low", None, None, BTreeMap::new()),
                adapter("second", None, Some(10), BTreeMap::new()),
            ],
            "first",
        );
        assert_eq!(
            catalog
                .adapters
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "low"]
        );
        assert_eq!(
            bundled_vcs_catalog_metadata().provider_ids,
            ["jj", "sl", "git"]
        );
    }

    #[test]
    fn extension_rejects_reserved_and_duplicate_ids() {
        let base =
            create_base_vcs_catalog(vec![adapter("git", None, None, BTreeMap::new())], "git");
        let catalog = extend_vcs_catalog(
            &base,
            vec![
                adapter("git", None, None, BTreeMap::new()),
                adapter("hg", None, None, BTreeMap::new()),
                adapter("hg", None, None, BTreeMap::new()),
            ],
        );
        assert_eq!(
            catalog
                .adapters
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            ["git", "hg"]
        );
        assert!(is_vcs_id("git", &catalog));
        assert!(!is_vcs_id("hg", &catalog));
    }

    #[test]
    fn resolves_configured_default_missing_and_unsupported_adapters() {
        let catalog = create_vcs_catalog(
            vec![
                adapter("git", None, None, BTreeMap::new()),
                adapter("hg", None, None, BTreeMap::new()),
            ],
            "git",
            ["git".into()],
        );
        assert_eq!(get_default_vcs_adapter(&catalog).unwrap().id, "git");
        assert_eq!(
            get_configured_vcs_adapter(None, &catalog).unwrap().id,
            "git"
        );
        assert_eq!(get_vcs_adapter("hg", &catalog).unwrap().id, "hg");
        assert!(matches!(
            get_vcs_adapter("missing", &catalog),
            Err(VcsCatalogError::UnsupportedVcs(_))
        ));
        let missing = create_vcs_catalog(Vec::new(), "git", Vec::new());
        assert!(matches!(
            get_default_vcs_adapter(&missing),
            Err(VcsCatalogError::User(_))
        ));
    }

    #[test]
    fn nearest_detection_wins_then_colocated_priority_and_failures_are_isolated() {
        let mut broken = adapter("broken", None, Some(1_000), BTreeMap::new());
        broken.detect = Arc::new(|_| Err("boom".into()));
        let catalog = create_base_vcs_catalog(
            vec![
                broken,
                adapter("outer", Some("/repo"), Some(100), BTreeMap::new()),
                adapter("inner", Some("/repo/nested"), Some(0), BTreeMap::new()),
            ],
            "outer",
        );
        assert_eq!(
            detect_vcs(Path::new("/repo/nested/src"), &catalog)
                .unwrap()
                .id,
            "inner"
        );
        let colocated = create_base_vcs_catalog(
            vec![
                adapter("low", Some("/repo"), None, BTreeMap::new()),
                adapter("high", Some("/repo"), Some(5), BTreeMap::new()),
            ],
            "low",
        );
        assert_eq!(
            detect_vcs(Path::new("/repo/src"), &colocated).unwrap().id,
            "high"
        );
    }

    #[test]
    fn classifies_inputs_maps_operations_and_loads_selected_handler() {
        assert!(is_vcs_review_input(&CliInput::Vcs(diff_input())));
        assert!(!is_vcs_review_input(&CliInput::Patch(PatchCommandInput {
            file: None,
            text: None,
            options: CommonOptions::default(),
        })));
        let operation = operation_from_input(VcsReviewInput::Diff(diff_input()));
        assert_eq!(operation.kind, VcsReviewOperationKind::WorkingTreeDiff);
        let mut operations = BTreeMap::new();
        operations.insert(VcsReviewOperationKind::WorkingTreeDiff, load_operation());
        let git = adapter("git", None, None, operations);
        let catalog = create_base_vcs_catalog(vec![git.clone()], "git");
        let result = load_vcs_review(
            &git,
            &operation,
            &VcsLoadContext {
                cwd: "/repo".into(),
            },
            &catalog,
        )
        .unwrap();
        assert_eq!(result.repo_root, Path::new("/repo"));
    }

    #[test]
    fn watch_plan_falls_back_to_polling_and_signature_is_explicit() {
        let operation = operation_from_input(VcsReviewInput::Show(show_input()));
        let mut operations = BTreeMap::new();
        operations.insert(VcsReviewOperationKind::RevisionShow, load_operation());
        let git = adapter("git", None, None, operations);
        let catalog = create_base_vcs_catalog(vec![git.clone()], "git");
        let context = VcsLoadContext {
            cwd: "/repo".into(),
        };
        assert_eq!(
            create_vcs_watch_plan(&git, &operation, &context, &catalog).unwrap(),
            VcsWatchPlan::poll_only()
        );
        assert!(matches!(
            create_vcs_watch_signature(&git, &operation, &context, &catalog),
            Err(VcsCatalogError::MissingWatchSignature { .. })
        ));
    }

    #[test]
    fn stash_error_recommends_the_catalog_adapter_that_supports_it() {
        let mut operations = BTreeMap::new();
        operations.insert(VcsReviewOperationKind::StashShow, load_operation());
        let git = adapter("git", None, None, operations);
        let jj = adapter("jj", None, None, BTreeMap::new());
        let catalog = create_base_vcs_catalog(vec![jj.clone(), git], "git");
        let error = create_unsupported_vcs_operation_error(
            &jj,
            VcsReviewOperationKind::StashShow,
            &catalog,
        );
        assert!(error.to_string().contains("requires GIT VCS mode"));
        let VcsCatalogError::User(error) = error else {
            panic!("unsupported operation must remain user-facing");
        };
        assert!(error.suggestions[0].contains("vcs = \"git\""));
    }

    #[test]
    fn workdeck_boundary_is_directory_only_and_nearest_nested_checkout_wins() {
        let temporary = tempdir().unwrap();
        let inner = temporary.path().join("vendor/nested");
        let source = inner.join("src");
        fs::create_dir_all(temporary.path().join(".git")).unwrap();
        fs::create_dir_all(inner.join(".git")).unwrap();
        fs::create_dir_all(&source).unwrap();
        assert_eq!(
            find_project_root_candidate(&source),
            Some(inner.canonicalize().unwrap())
        );

        let other = tempdir().unwrap();
        let nested = other.path().join("src");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(other.path().join(".agents")).unwrap();
        fs::write(other.path().join(".agents/workdeck"), "not a directory\n").unwrap();
        assert_eq!(find_project_root_candidate(&nested), None);
    }
}
