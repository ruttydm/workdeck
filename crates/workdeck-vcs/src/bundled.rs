//! Statically linked VCS adapters composed through the public catalog boundary.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use workdeck_core::{
    Changeset, ChangesetSource, ReviewSide, VcsDiffCommandInput, review_content_digest,
};

use crate::{
    DiffRequest, GitProvider, GitVcsAdapterOptions, JujutsuProvider, SaplingProvider,
    SaplingVcsAdapterOptions, VcsAdapter, VcsCatalog, VcsCatalogError, VcsDetection,
    VcsLoadContext, VcsOperation, VcsPatchResult, VcsProvider, VcsReviewInput,
    VcsReviewOperationKind, VcsSourceReader, VcsWatchPlan, create_base_vcs_catalog,
    create_git_vcs_adapter, create_sapling_vcs_adapter,
};

const GIT_PRIORITY: i32 = 0;
const SAPLING_PRIORITY: i32 = 100;
const JUJUTSU_PRIORITY: i32 = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BundledBackend {
    Git,
    Jujutsu,
    Sapling,
}

/// Stable metadata for one statically linked bundled extension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundledExtensionMetadata {
    pub id: &'static str,
    pub source_path: &'static str,
    pub origin: &'static str,
}

/// Memoized bundled-extension result. Compiled Rust registration cannot fail,
/// but issues remain explicit so it retains the same host contract as native
/// user extension loading.
pub struct BundledExtensionLoad {
    pub extensions: Vec<BundledExtensionMetadata>,
    pub catalog: VcsCatalog,
    pub issues: Vec<String>,
}

static BUNDLED_LOAD: OnceLock<BundledExtensionLoad> = OnceLock::new();

/// Load every shipped backend once, in registration order.
#[must_use]
pub fn load_bundled_vcs_extensions() -> &'static BundledExtensionLoad {
    BUNDLED_LOAD.get_or_init(|| {
        let definitions = [
            (
                bundled_metadata("jj", "workdeck:bundled/jj"),
                BundledBackend::Jujutsu,
            ),
            (
                bundled_metadata("sl", "workdeck:bundled/sl"),
                BundledBackend::Sapling,
            ),
            (
                bundled_metadata("git", "workdeck:bundled/git"),
                BundledBackend::Git,
            ),
        ];
        let mut extensions = Vec::new();
        let mut adapters = Vec::new();
        let mut issues = Vec::new();
        for (metadata, backend) in definitions {
            match catch_unwind(AssertUnwindSafe(|| bundled_adapter(backend))) {
                Ok(adapter) => {
                    extensions.push(metadata);
                    adapters.push(adapter);
                }
                Err(_) => issues.push(format!(
                    "bundled extension {} failed during registration",
                    metadata.id
                )),
            }
        }
        BundledExtensionLoad {
            extensions,
            catalog: create_base_vcs_catalog(adapters, "git"),
            issues,
        }
    })
}

fn bundled_metadata(id: &'static str, source_path: &'static str) -> BundledExtensionMetadata {
    BundledExtensionMetadata {
        id,
        source_path,
        origin: "bundled",
    }
}

/// Complete catalog registered by the bundled extension tier.
#[must_use]
pub fn bundled_vcs_catalog() -> &'static VcsCatalog {
    &load_bundled_vcs_extensions().catalog
}

/// Backends in resolved registration order.
#[must_use]
pub fn get_bundled_vcs_adapters() -> &'static [VcsAdapter] {
    &bundled_vcs_catalog().adapters
}

fn bundled_adapter(backend: BundledBackend) -> VcsAdapter {
    match backend {
        BundledBackend::Git => return create_git_vcs_adapter(GitVcsAdapterOptions::default()),
        BundledBackend::Sapling => {
            return create_sapling_vcs_adapter(SaplingVcsAdapterOptions::default());
        }
        BundledBackend::Jujutsu => {}
    }
    let (id, name, priority) = match backend {
        BundledBackend::Git => ("git", "Git", GIT_PRIORITY),
        BundledBackend::Jujutsu => ("jj", "Jujutsu", JUJUTSU_PRIORITY),
        BundledBackend::Sapling => ("sl", "Sapling", SAPLING_PRIORITY),
    };
    let operation_kinds: &[VcsReviewOperationKind] = match backend {
        BundledBackend::Git => &[
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewOperationKind::RevisionShow,
            VcsReviewOperationKind::StashShow,
        ],
        BundledBackend::Jujutsu | BundledBackend::Sapling => &[
            VcsReviewOperationKind::WorkingTreeDiff,
            VcsReviewOperationKind::RevisionShow,
        ],
    };
    let operations = operation_kinds
        .iter()
        .copied()
        .map(|kind| (kind, bundled_operation(backend)))
        .collect::<BTreeMap<_, _>>();
    VcsAdapter {
        id: id.into(),
        name: name.into(),
        detect: Arc::new(move |cwd| detect_backend(backend, cwd)),
        operations,
        detection_priority: Some(priority),
    }
}

fn bundled_operation(backend: BundledBackend) -> VcsOperation {
    VcsOperation {
        load: Arc::new(move |input, context| {
            let (root, changeset) = load_backend_changeset(backend, input, context)?;
            Ok(changeset_patch_result(root, changeset))
        }),
        watch_signature: Some(Arc::new(move |input, context| {
            let (_, changeset) = load_backend_changeset(backend, input, context)?;
            Ok(changeset_signature(&changeset))
        })),
        // Provider-specific native metadata targets are ported with each
        // adapter. Until then the controller's bounded degraded interval is
        // the correct complete fallback, rather than a renderer-owned timer.
        watch_plan: Some(Arc::new(|_input, _context| Ok(VcsWatchPlan::poll_only()))),
    }
}

fn detect_backend(backend: BundledBackend, cwd: &Path) -> Result<Option<VcsDetection>, String> {
    let root = match backend {
        BundledBackend::Git => {
            GitProvider::discover(cwd).map(|provider| provider.root().to_owned())
        }
        BundledBackend::Jujutsu => {
            JujutsuProvider::discover(cwd).map(|provider| provider.root().to_owned())
        }
        BundledBackend::Sapling => {
            SaplingProvider::discover(cwd).map(|provider| provider.root().to_owned())
        }
    };
    match root {
        Ok(repo_root) => Ok(Some(VcsDetection {
            id: backend_id(backend).into(),
            repo_root,
        })),
        Err(_) => Ok(None),
    }
}

fn backend_id(backend: BundledBackend) -> &'static str {
    match backend {
        BundledBackend::Git => "git",
        BundledBackend::Jujutsu => "jj",
        BundledBackend::Sapling => "sl",
    }
}

fn load_backend_changeset(
    backend: BundledBackend,
    input: &VcsReviewInput,
    context: &VcsLoadContext,
) -> Result<(PathBuf, Changeset), VcsCatalogError> {
    match backend {
        BundledBackend::Git => {
            let provider = GitProvider::discover(&context.cwd).map_err(operation_error)?;
            let changeset = load_git_changeset(&provider, input)?;
            Ok((provider.root().to_owned(), changeset))
        }
        BundledBackend::Jujutsu => {
            let provider = JujutsuProvider::discover(&context.cwd).map_err(operation_error)?;
            let changeset = load_provider_changeset(&provider, input)?;
            Ok((provider.root().to_owned(), changeset))
        }
        BundledBackend::Sapling => {
            let provider = SaplingProvider::discover(&context.cwd).map_err(operation_error)?;
            let changeset = load_provider_changeset(&provider, input)?;
            Ok((provider.root().to_owned(), changeset))
        }
    }
}

fn load_git_changeset(
    provider: &GitProvider,
    input: &VcsReviewInput,
) -> Result<Changeset, VcsCatalogError> {
    match input {
        VcsReviewInput::StashShow(input) => provider
            .stash(input.reference.as_deref())
            .map_err(operation_error),
        _ => load_provider_changeset(provider, input),
    }
}

fn load_provider_changeset(
    provider: &impl VcsProvider,
    input: &VcsReviewInput,
) -> Result<Changeset, VcsCatalogError> {
    match input {
        VcsReviewInput::Diff(input) => provider
            .working_tree(&diff_request(input))
            .map_err(operation_error),
        VcsReviewInput::Show(input) => provider
            .show(input.reference.as_deref(), &input.pathspecs)
            .map_err(operation_error),
        VcsReviewInput::StashShow(_) => Err(VcsCatalogError::Operation(
            "this bundled backend does not support stash review".into(),
        )),
    }
}

fn diff_request(input: &VcsDiffCommandInput) -> DiffRequest {
    let (from, target) = input.range_endpoints.as_ref().map_or_else(
        || (None, input.range.clone()),
        |endpoints| (Some(endpoints.from.clone()), Some(endpoints.to.clone())),
    );
    DiffRequest {
        target,
        from,
        staged: input.staged,
        exclude_untracked: input.options.exclude_untracked.unwrap_or(false),
        pathspec: input.pathspecs.clone(),
        color_moved: input.options.color_moved,
    }
}

fn operation_error(error: impl std::fmt::Display) -> VcsCatalogError {
    VcsCatalogError::Operation(error.to_string())
}

fn changeset_signature(changeset: &Changeset) -> String {
    let mut owned = vec![changeset.id.clone(), changeset.title.clone()];
    for file in &changeset.files {
        owned.push(file.path.clone());
        owned.push(file.previous_path.clone().unwrap_or_default());
        owned.push(file.content_identity.clone());
    }
    let borrowed = owned.iter().map(String::as_str).collect::<Vec<_>>();
    review_content_digest(&borrowed)
}

fn changeset_patch_result(repo_root: PathBuf, changeset: Changeset) -> VcsPatchResult {
    let source_label = source_label(&repo_root, &changeset.source);
    let source_files = Arc::new(changeset.files.clone());
    let source_reader: VcsSourceReader = Arc::new(move |request| {
        source_files
            .iter()
            .find(|file| {
                file.path == request.path
                    || request
                        .previous_path
                        .as_deref()
                        .is_some_and(|path| file.previous_path.as_deref() == Some(path))
            })
            .and_then(|file| match request.side {
                ReviewSide::Old => file.sources.old.as_ref(),
                ReviewSide::New => file.sources.new.as_ref(),
            })
            .cloned()
            .map_or(
                crate::VcsFileSourceResult::Missing,
                crate::VcsFileSourceResult::Source,
            )
    });
    let untracked_paths = changeset
        .files
        .iter()
        .filter(|file| file.flags.untracked)
        .map(|file| repo_root.join(&file.path))
        .collect::<Vec<_>>();
    let extra_files = changeset
        .files
        .iter()
        .filter(|file| file.flags.untracked)
        .cloned()
        .collect::<Vec<_>>();
    let patch_text = changeset
        .files
        .iter()
        .filter(|file| !file.flags.untracked)
        .map(|file| file.patch.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    VcsPatchResult {
        repo_root,
        source_label,
        title: changeset.title,
        patch_text,
        untracked_paths,
        source_reader: Some(source_reader),
        source_cache_key: None,
        extra_files,
    }
}

fn source_label(repo_root: &Path, source: &ChangesetSource) -> String {
    match source {
        ChangesetSource::WorkingTree { .. } => repo_root.to_string_lossy().into_owned(),
        ChangesetSource::Revision { to, .. } => to.clone(),
        ChangesetSource::Stash { reference } => reference.clone(),
        ChangesetSource::Patch { label } => label.clone(),
        ChangesetSource::Files { left, right } => format!("{left} → {right}"),
    }
}

#[cfg(test)]
mod tests;
