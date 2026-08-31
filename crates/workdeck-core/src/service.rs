use crate::ContentStore;
use anyhow::{Context, Result, bail};
use chrono::Utc;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::{
    fs,
    path::{Path, PathBuf},
};
use workdeck_analysis::{AnalysisResult, AnalyzedUnit, ReviewDocument, SourceLanguage};
use workdeck_db::Catalog;
use workdeck_domain::{
    AnalysisConfidence, CheckoutId, CheckoutRecord, RepositoryId, RepositoryRecord,
    ReviewCheckpoint, ReviewDelta, ReviewMark, ReviewMarkState, ReviewSet, ReviewSetId,
    ReviewSource, ReviewUnitKind, ReviewUnitVersion, ReviewUnitVersionId, SnapshotId,
    SnapshotSource, UnitTransition, WorkspaceProject, WorktreeId, WorktreeRecord,
};
use workdeck_git::{RepositoryDiscovery, WorktreeSnapshotManifest};
use workdeck_github::GitHubCancellation;

#[derive(Debug, Clone)]
pub struct ApplicationPaths {
    pub root: PathBuf,
    pub database: PathBuf,
    pub objects: PathBuf,
    pub artifacts: PathBuf,
    pub logs: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortfolioDiscoveryReport {
    pub roots: Vec<PathBuf>,
    pub project_count: usize,
    pub repository_count: usize,
    pub worktree_count: usize,
    pub issues: Vec<String>,
}

impl ApplicationPaths {
    pub fn discover() -> Result<Self> {
        if let Some(root) = std::env::var_os("WORKDECK_DATA_DIR") {
            let root = PathBuf::from(root);
            if !root.is_absolute() || root.parent().is_none() || root == Path::new("/") {
                bail!("WORKDECK_DATA_DIR must be an absolute, non-root path");
            }
            return Ok(Self::at(root));
        }
        let project_dirs = ProjectDirs::from("app", "Ginger Media", "Workdeck")
            .context("could not resolve Workdeck application data directory")?;
        Ok(Self::at(project_dirs.data_local_dir()))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            database: root.join("workdeck.sqlite3"),
            objects: root.join("objects"),
            artifacts: root.join("artifacts"),
            logs: root.join("logs"),
            root,
        }
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        std::fs::create_dir_all(&self.objects)?;
        std::fs::create_dir_all(&self.artifacts)?;
        std::fs::create_dir_all(&self.logs)?;
        Ok(())
    }
}

pub struct WorkdeckService {
    pub paths: ApplicationPaths,
    pub catalog: Catalog,
    pub content: ContentStore,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedSnapshot {
    pub checkpoint: ReviewCheckpoint,
    pub manifests: Vec<WorktreeSnapshotManifest>,
    pub source_manifests: Vec<serde_json::Value>,
    pub analyses: Vec<AnalysisResult>,
    pub review: SnapshotReviewSummary,
}

pub struct CheckpointSources<'a> {
    pub local: &'a [(workdeck_domain::WorktreeId, PathBuf, Option<String>)],
    pub commit_ranges: &'a [(RepositoryId, PathBuf, String, String)],
    pub markdown: &'a [(RepositoryId, PathBuf, Option<String>, PathBuf)],
    pub pull_requests: &'a [(String, u64)],
    pub workflow_runs: &'a [(String, String)],
    pub artifacts: &'a [workdeck_domain::ArtifactId],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SnapshotReviewSummary {
    pub total: usize,
    pub unchanged: usize,
    pub moved: usize,
    pub format_only: usize,
    pub modified: usize,
    pub new: usize,
    pub removed: usize,
    pub review_state_carried: usize,
}

fn portfolio_repository_candidates(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    if workdeck_git::discover(root).is_ok() {
        return Ok(vec![(portfolio_path_label(root), root.to_path_buf())]);
    }
    let mut candidates = Vec::new();
    let mut children = portfolio_directories(root)?;
    children.sort();
    for child in children {
        let project_name = portfolio_path_label(&child);
        if workdeck_git::discover(&child).is_ok() {
            candidates.push((project_name, child));
            continue;
        }
        let mut grandchildren = portfolio_directories(&child).unwrap_or_default();
        grandchildren.sort();
        for repository in grandchildren {
            if workdeck_git::discover(&repository).is_ok() {
                candidates.push((project_name.clone(), repository));
            }
        }
    }
    Ok(candidates)
}

fn portfolio_directories(root: &Path) -> Result<Vec<PathBuf>> {
    Ok(fs::read_dir(root)
        .with_context(|| format!("could not scan {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_type()
                .ok()
                .filter(|kind| kind.is_dir() && !kind.is_symlink())
                .map(|_| entry.path())
        })
        .filter(|path| {
            !path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|name| name.starts_with('.'))
        })
        .collect())
}

fn portfolio_path_label(path: &Path) -> String {
    path.file_name()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("repository")
        .to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectiveReviewMark {
    pub version_id: ReviewUnitVersionId,
    pub state: ReviewMarkState,
    pub reviewer: String,
    pub recorded_at: chrono::DateTime<Utc>,
    pub inherited_from: Option<ReviewUnitVersionId>,
}

impl WorkdeckService {
    pub fn open_default() -> Result<Self> {
        Self::open(ApplicationPaths::discover()?)
    }

    pub fn open(paths: ApplicationPaths) -> Result<Self> {
        paths.ensure()?;
        let catalog = Catalog::open(&paths.database)?;
        let content = ContentStore::new(&paths.objects)?;
        Ok(Self {
            paths,
            catalog,
            content,
        })
    }

    pub fn create_project(&self, name: &str) -> Result<WorkspaceProject> {
        let name = name.trim();
        if name.is_empty() {
            bail!("project name cannot be empty");
        }
        if self.catalog.project_by_name(name)?.is_some() {
            bail!("project {name} already exists");
        }
        let project = WorkspaceProject::new(name);
        self.catalog.save_project(&project)?;
        self.catalog.append_event(
            "project_created",
            &serde_json::json!({ "project_id": project.id, "name": project.name }),
        )?;
        Ok(project)
    }

    /// Discovers repositories below portfolio roots without modifying any
    /// repository. Repeated calls reuse project, repository, checkout, and
    /// worktree identities through the catalog's canonical Git metadata.
    pub fn discover_portfolio_roots(&self, roots: &[PathBuf]) -> Result<PortfolioDiscoveryReport> {
        let mut issues = Vec::new();
        for root in roots {
            if !root.is_dir() {
                issues.push(format!("{} is unavailable", root.display()));
                continue;
            }
            for (project_name, repository_path) in portfolio_repository_candidates(root)? {
                let project = self
                    .catalog
                    .project_by_name(&project_name)?
                    .map_or_else(|| self.create_project(&project_name), Ok)?;
                if let Err(error) = self.add_repository(&project, &repository_path) {
                    issues.push(format!("{}: {error:#}", repository_path.display()));
                }
            }
        }
        Ok(PortfolioDiscoveryReport {
            roots: roots.to_vec(),
            project_count: self.catalog.list_projects(true)?.len(),
            repository_count: self.catalog.all_repositories()?.len(),
            worktree_count: self.catalog.all_worktrees()?.len(),
            issues,
        })
    }

    pub fn add_repository(
        &self,
        project: &WorkspaceProject,
        path: &Path,
    ) -> Result<(RepositoryRecord, RepositoryDiscovery)> {
        let discovery = workdeck_git::discover(path)?;
        let now = Utc::now();
        let all_checkouts = self.catalog.all_checkouts()?;
        let existing_checkout = self.catalog.checkout_by_path(&discovery.root)?.or_else(|| {
            all_checkouts
                .iter()
                .find(|checkout| checkout.git_common_dir == discovery.git_common_dir)
                .cloned()
        });
        let existing_repository = if let Some(checkout) = &existing_checkout {
            self.catalog.repository(&checkout.repository_id)?
        } else if discovery.remotes.is_empty() {
            None
        } else {
            self.catalog
                .list_repositories(&project.id)?
                .into_iter()
                .find(|repository| {
                    repository
                        .normalized_remotes
                        .iter()
                        .any(|remote| discovery.remotes.contains(remote))
                })
        };
        if let Some(repository) = &existing_repository
            && repository.project_id != project.id
        {
            bail!(
                "repository {} is already registered in project {}",
                repository.name,
                repository.project_id
            );
        }
        let mut repository = existing_repository.unwrap_or_else(|| RepositoryRecord {
            id: RepositoryId::new(),
            project_id: project.id.clone(),
            name: discovery.name.clone(),
            provider: None,
            provider_owner: None,
            provider_name: None,
            normalized_remotes: Vec::new(),
            created_at: now,
            updated_at: now,
        });
        repository.name = discovery.name.clone();
        repository.provider = discovery
            .provider
            .as_ref()
            .map(|value| value.provider.clone());
        repository.provider_owner = discovery.provider.as_ref().map(|value| value.owner.clone());
        repository.provider_name = discovery.provider.as_ref().map(|value| value.name.clone());
        repository.normalized_remotes = discovery.remotes.clone();
        repository.updated_at = now;
        self.catalog.save_repository(&repository)?;

        let checkout = existing_checkout.unwrap_or_else(|| CheckoutRecord {
            id: CheckoutId::new(),
            repository_id: repository.id.clone(),
            path: discovery.root.clone(),
            git_common_dir: discovery.git_common_dir.clone(),
            available: true,
            last_seen_at: now,
        });
        let checkout = CheckoutRecord {
            repository_id: repository.id.clone(),
            git_common_dir: discovery.git_common_dir.clone(),
            available: checkout.path.exists(),
            last_seen_at: now,
            ..checkout
        };
        self.catalog.save_checkout(&checkout)?;

        let existing_worktrees = self.catalog.list_worktrees(&checkout.id)?;
        let mut worktrees = Vec::new();
        for discovered in &discovery.worktrees {
            let canonical =
                std::fs::canonicalize(&discovered.path).unwrap_or_else(|_| discovered.path.clone());
            let existing = existing_worktrees
                .iter()
                .find(|worktree| worktree.path == canonical);
            let worktree = WorktreeRecord {
                id: existing
                    .map(|worktree| worktree.id.clone())
                    .unwrap_or_else(WorktreeId::new),
                checkout_id: checkout.id.clone(),
                path: canonical,
                head: discovered.head.clone(),
                branch: discovered.branch.clone(),
                locked: discovered.locked,
                prunable: discovered.prunable,
                available: discovered.path.exists(),
                last_seen_at: now,
            };
            self.catalog.save_worktree(&worktree)?;
            worktrees.push(worktree);
        }
        for existing in existing_worktrees
            .iter()
            .filter(|existing| !worktrees.iter().any(|current| current.id == existing.id))
        {
            let mut unavailable = existing.clone();
            unavailable.available = false;
            self.catalog.save_worktree(&unavailable)?;
        }
        self.catalog.append_event(
            "repository_refreshed",
            &serde_json::json!({
                "project_id": project.id,
                "repository_id": repository.id,
                "path": discovery.root,
                "worktrees": worktrees.len(),
            }),
        )?;
        Ok((repository, discovery))
    }

    pub fn create_review(&self, title: &str) -> Result<ReviewSet> {
        let title = title.trim();
        if title.is_empty() {
            bail!("review title cannot be empty");
        }
        let review = ReviewSet::new(title);
        self.catalog.save_review(&review)?;
        self.catalog.append_event(
            "review_created",
            &serde_json::json!({ "review_set_id": review.id, "title": review.title }),
        )?;
        Ok(review)
    }

    pub fn attach_source(&self, review_set_id: &ReviewSetId, source: &ReviewSource) -> Result<()> {
        if self.catalog.review_sources(review_set_id)?.contains(source) {
            return Ok(());
        }
        self.catalog.attach_review_source(review_set_id, source)?;
        self.catalog.append_event(
            "review_source_attached",
            &serde_json::json!({ "review_set_id": review_set_id, "source": source }),
        )?;
        Ok(())
    }

    pub fn live_source_revision(&self, source: &ReviewSource) -> Result<String> {
        match source {
            ReviewSource::LocalWorktree { worktree_id, .. } => {
                let worktree = self
                    .catalog
                    .find_worktree(worktree_id.as_str())?
                    .with_context(|| format!("worktree {worktree_id} does not exist"))?;
                workdeck_git::live_worktree_revision(&worktree.path)
            }
            ReviewSource::CommitRange {
                repository_id,
                head,
                ..
            } => {
                let repository = self
                    .catalog
                    .repository(repository_id)?
                    .with_context(|| format!("repository {repository_id} does not exist"))?;
                let root = checkout_path_for_repository(&self.catalog, &repository)?;
                workdeck_git::resolve_revision(&root, head)
            }
            ReviewSource::Markdown {
                repository_id,
                revision,
                path,
            } => {
                validate_repository_path(path)?;
                let repository = self
                    .catalog
                    .repository(repository_id)?
                    .with_context(|| format!("repository {repository_id} does not exist"))?;
                let root = checkout_path_for_repository(&self.catalog, &repository)?;
                if let Some(revision) = revision {
                    workdeck_git::resolve_revision(&root, revision)
                } else {
                    let bytes = std::fs::read(root.join(path))?;
                    Ok(format!("working:{}", workdeck_domain::content_hash(&bytes)))
                }
            }
            ReviewSource::PullRequest {
                provider,
                repository,
                number,
            } if provider == "github" => {
                let pull =
                    workdeck_github::GitHubClient::default().pull_request(repository, *number)?;
                let manifest = serde_json::to_value(&pull)?;
                provider_revision("github-pr", &pull.head_sha, &manifest)
            }
            ReviewSource::CiRun {
                provider,
                repository,
                run_id,
            } if provider == "github" => {
                let run =
                    workdeck_github::GitHubClient::default().workflow_run(repository, run_id)?;
                let manifest = serde_json::to_value(&run)?;
                provider_revision("github-run", &run.head_sha, &manifest)
            }
            ReviewSource::Artifact { artifact_id } => {
                let store = workdeck_artifacts::ArtifactStore::new(&self.paths.artifacts)?;
                let manifest = store
                    .find(artifact_id.as_str())?
                    .with_context(|| format!("artifact {artifact_id} does not exist"))?;
                Ok(format!(
                    "artifact:{}:{}",
                    artifact_id,
                    manifest.imported_at.to_rfc3339()
                ))
            }
            ReviewSource::PullRequest { provider, .. } | ReviewSource::CiRun { provider, .. } => {
                bail!("provider {provider} is not supported by this prototype")
            }
        }
    }

    pub fn capture_local_worktree_checkpoint(
        &mut self,
        review: &ReviewSet,
        sources: &[(workdeck_domain::WorktreeId, PathBuf, Option<String>)],
    ) -> Result<CapturedSnapshot> {
        let sequence = self.catalog.next_snapshot_sequence(&review.id)?;
        let snapshot_id = SnapshotId::new();
        let mut manifests = Vec::new();
        let mut snapshot_sources = Vec::new();
        for (worktree_id, path, base) in sources {
            let manifest = workdeck_git::capture_worktree(path, &mut self.content)
                .with_context(|| format!("failed to capture worktree {}", path.display()))?;
            let manifest_bytes = serde_json::to_vec(&manifest)?;
            let manifest_hash = workdeck_git::ObjectSink::put(&mut self.content, &manifest_bytes)?;
            snapshot_sources.push(SnapshotSource {
                source: ReviewSource::LocalWorktree {
                    worktree_id: worktree_id.clone(),
                    base: base.clone(),
                },
                revision: workdeck_git::worktree_manifest_revision(&manifest),
                manifest_hash,
            });
            manifests.push(manifest);
        }
        let checkpoint = ReviewCheckpoint {
            id: snapshot_id,
            review_set_id: review.id.clone(),
            sequence,
            created_at: Utc::now(),
            sources: snapshot_sources,
        };
        self.catalog.save_checkpoint(&checkpoint)?;
        let analyses = self.analyze_local_manifests(&manifests, sources)?;
        let review = self.persist_review_analyses(&checkpoint, &analyses)?;
        self.catalog.append_event(
            "checkpoint_created",
            &serde_json::json!({
                "review_set_id": checkpoint.review_set_id,
                "snapshot_id": checkpoint.id,
                "sequence": checkpoint.sequence,
            }),
        )?;
        Ok(CapturedSnapshot {
            checkpoint,
            manifests,
            source_manifests: Vec::new(),
            analyses,
            review,
        })
    }

    fn analyze_local_manifests(
        &self,
        manifests: &[WorktreeSnapshotManifest],
        sources: &[(workdeck_domain::WorktreeId, PathBuf, Option<String>)],
    ) -> Result<Vec<AnalysisResult>> {
        let mut analyses = Vec::new();
        for (manifest, (worktree_id, _, _)) in manifests.iter().zip(sources) {
            let repository = self
                .catalog
                .repository_for_worktree(worktree_id)?
                .with_context(|| format!("repository for worktree {worktree_id} does not exist"))?;
            for file in manifest.files.iter().filter(|file| !file.binary) {
                let Some(hash) = file.content_hash.as_deref() else {
                    continue;
                };
                let bytes = self.content.get(hash)?;
                let Ok(content) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                analyses.push(workdeck_analysis::analyze(&ReviewDocument {
                    repository_id: Some(repository.id.clone()),
                    path: file.path.clone(),
                    content,
                })?);
            }
        }
        Ok(analyses)
    }

    pub fn capture_pull_request_checkpoint(
        &mut self,
        review: &ReviewSet,
        sources: &[(String, u64)],
    ) -> Result<CapturedSnapshot> {
        self.capture_pull_request_checkpoint_with_cancellation(
            review,
            sources,
            &GitHubCancellation::default(),
        )
    }

    pub fn capture_pull_request_checkpoint_with_cancellation(
        &mut self,
        review: &ReviewSet,
        sources: &[(String, u64)],
        cancellation: &GitHubCancellation,
    ) -> Result<CapturedSnapshot> {
        if sources.is_empty() {
            bail!("review {} has no pull request sources", review.title);
        }
        let sequence = self.catalog.next_snapshot_sequence(&review.id)?;
        let checkpoint_id = SnapshotId::new();
        let github = workdeck_github::GitHubClient::default();
        let mut checkpoint_sources = Vec::new();
        let mut source_manifests = Vec::new();
        let mut analyses = Vec::new();
        for (repository, number) in sources {
            ensure_capture_active(cancellation)?;
            let pull = github.pull_request_with_cancellation(repository, *number, cancellation)?;
            for file in pull.files.iter().filter(|file| file.status != "removed") {
                ensure_capture_active(cancellation)?;
                let path = PathBuf::from(&file.filename);
                let bytes = github
                    .file_at_revision_with_cancellation(
                        repository,
                        &pull.head_sha,
                        &path,
                        cancellation,
                    )
                    .with_context(|| {
                        format!(
                            "failed to load {repository}:{} at {}",
                            file.filename, pull.head_sha
                        )
                    })?;
                if let Ok(content) = std::str::from_utf8(&bytes) {
                    analyses.push(workdeck_analysis::analyze(&ReviewDocument {
                        repository_id: None,
                        path,
                        content,
                    })?);
                }
                workdeck_git::ObjectSink::put(&mut self.content, &bytes)?;
            }
            let manifest = serde_json::to_value(&pull)?;
            let manifest_hash =
                workdeck_git::ObjectSink::put(&mut self.content, &serde_json::to_vec(&manifest)?)?;
            checkpoint_sources.push(SnapshotSource {
                source: ReviewSource::PullRequest {
                    provider: "github".to_string(),
                    repository: repository.clone(),
                    number: *number,
                },
                revision: provider_revision("github-pr", &pull.head_sha, &manifest)?,
                manifest_hash,
            });
            source_manifests.push(manifest);
        }
        ensure_capture_active(cancellation)?;
        let checkpoint = ReviewCheckpoint {
            id: checkpoint_id,
            review_set_id: review.id.clone(),
            sequence,
            created_at: Utc::now(),
            sources: checkpoint_sources,
        };
        self.catalog.save_checkpoint(&checkpoint)?;
        let review = self.persist_review_analyses(&checkpoint, &analyses)?;
        self.catalog.append_event(
            "checkpoint_created",
            &serde_json::json!({
                "review_set_id": checkpoint.review_set_id,
                "snapshot_id": checkpoint.id,
                "sequence": checkpoint.sequence,
                "provider": "github",
            }),
        )?;
        Ok(CapturedSnapshot {
            checkpoint,
            manifests: Vec::new(),
            source_manifests,
            analyses,
            review,
        })
    }

    pub fn capture_sources_checkpoint(
        &mut self,
        review: &ReviewSet,
        sources: CheckpointSources<'_>,
    ) -> Result<CapturedSnapshot> {
        self.capture_sources_checkpoint_with_cancellation(
            review,
            sources,
            &GitHubCancellation::default(),
        )
    }

    pub fn capture_sources_checkpoint_with_cancellation(
        &mut self,
        review: &ReviewSet,
        sources: CheckpointSources<'_>,
        cancellation: &GitHubCancellation,
    ) -> Result<CapturedSnapshot> {
        let CheckpointSources {
            local: local_sources,
            commit_ranges: commit_range_sources,
            markdown: markdown_sources,
            pull_requests: pull_request_sources,
            workflow_runs: workflow_sources,
            artifacts: artifact_sources,
        } = sources;
        if local_sources.is_empty()
            && commit_range_sources.is_empty()
            && markdown_sources.is_empty()
            && pull_request_sources.is_empty()
            && workflow_sources.is_empty()
            && artifact_sources.is_empty()
        {
            bail!("review {} has no capturable sources", review.title);
        }

        let sequence = self.catalog.next_snapshot_sequence(&review.id)?;
        let checkpoint_id = SnapshotId::new();
        let github = workdeck_github::GitHubClient::default();
        let mut checkpoint_sources = Vec::new();
        let mut manifests = Vec::new();
        let mut source_manifests = Vec::new();
        let mut analyses = Vec::new();

        for (worktree_id, path, base) in local_sources {
            ensure_capture_active(cancellation)?;
            let manifest = workdeck_git::capture_worktree(path, &mut self.content)
                .with_context(|| format!("failed to capture worktree {}", path.display()))?;
            let mut local_analyses = self.analyze_local_manifests(
                std::slice::from_ref(&manifest),
                &[(worktree_id.clone(), path.clone(), base.clone())],
            )?;
            let committed_range = match (base.as_deref(), manifest.head.as_deref()) {
                (Some(base), Some(head)) => {
                    let repository = self
                        .catalog
                        .repository_for_worktree(worktree_id)?
                        .with_context(|| {
                            format!("repository for worktree {worktree_id} does not exist")
                        })?;
                    let (_, range_manifest, mut range_analyses) =
                        self.capture_commit_range(&repository.id, path, base, head)?;
                    let dirty_paths = manifest
                        .files
                        .iter()
                        .map(|file| file.path.clone())
                        .collect::<BTreeSet<_>>();
                    range_analyses.retain(|analysis| {
                        analysis
                            .units
                            .first()
                            .is_none_or(|unit| !dirty_paths.contains(&unit.path))
                    });
                    local_analyses.extend(range_analyses);
                    Some(range_manifest)
                }
                _ => None,
            };
            let source_manifest = serde_json::json!({
                "worktree": &manifest,
                "committed_range": committed_range,
            });
            let manifest_hash = workdeck_git::ObjectSink::put(
                &mut self.content,
                &serde_json::to_vec(&source_manifest)?,
            )?;
            checkpoint_sources.push(SnapshotSource {
                source: ReviewSource::LocalWorktree {
                    worktree_id: worktree_id.clone(),
                    base: base.clone(),
                },
                revision: workdeck_git::worktree_manifest_revision(&manifest),
                manifest_hash,
            });
            analyses.extend(local_analyses);
            source_manifests.push(source_manifest);
            manifests.push(manifest);
        }

        for (repository_id, root, base, head) in commit_range_sources {
            ensure_capture_active(cancellation)?;
            let (resolved_head, manifest, range_analyses) =
                self.capture_commit_range(repository_id, root, base, head)?;
            let manifest_hash =
                workdeck_git::ObjectSink::put(&mut self.content, &serde_json::to_vec(&manifest)?)?;
            checkpoint_sources.push(SnapshotSource {
                source: ReviewSource::CommitRange {
                    repository_id: repository_id.clone(),
                    base: base.clone(),
                    head: head.clone(),
                },
                revision: resolved_head,
                manifest_hash,
            });
            analyses.extend(range_analyses);
            source_manifests.push(manifest);
        }

        for (repository_id, root, revision, path) in markdown_sources {
            ensure_capture_active(cancellation)?;
            validate_repository_path(path)?;
            let (resolved_revision, bytes) = if let Some(revision) = revision {
                let resolved = workdeck_git::resolve_revision(root, revision)?;
                let bytes = workdeck_git::read_blob_at_revision(root, &resolved, path)?
                    .with_context(|| format!("{} does not exist at {revision}", path.display()))?;
                (resolved, bytes)
            } else {
                let absolute = root.join(path);
                let bytes = std::fs::read(&absolute)
                    .with_context(|| format!("failed to read {}", absolute.display()))?;
                let content_hash = workdeck_domain::content_hash(&bytes);
                (format!("working:{content_hash}"), bytes)
            };
            let content_hash = workdeck_git::ObjectSink::put(&mut self.content, &bytes)?;
            let manifest = serde_json::json!({
                "repository_id": repository_id,
                "path": path,
                "requested_revision": revision,
                "resolved_revision": resolved_revision,
                "content_hash": content_hash,
                "size": bytes.len(),
            });
            let manifest_hash =
                workdeck_git::ObjectSink::put(&mut self.content, &serde_json::to_vec(&manifest)?)?;
            let content = std::str::from_utf8(&bytes)
                .with_context(|| format!("{} is not valid UTF-8 Markdown", path.display()))?;
            analyses.push(workdeck_analysis::analyze(&ReviewDocument {
                repository_id: Some(repository_id.clone()),
                path: path.clone(),
                content,
            })?);
            checkpoint_sources.push(SnapshotSource {
                source: ReviewSource::Markdown {
                    repository_id: repository_id.clone(),
                    revision: revision.clone(),
                    path: path.clone(),
                },
                revision: resolved_revision,
                manifest_hash,
            });
            source_manifests.push(manifest);
        }

        for (repository, number) in pull_request_sources {
            ensure_capture_active(cancellation)?;
            let pull = github.pull_request_with_cancellation(repository, *number, cancellation)?;
            for file in pull.files.iter().filter(|file| file.status != "removed") {
                ensure_capture_active(cancellation)?;
                let path = PathBuf::from(&file.filename);
                let bytes = github.file_at_revision_with_cancellation(
                    repository,
                    &pull.head_sha,
                    &path,
                    cancellation,
                )?;
                if let Ok(content) = std::str::from_utf8(&bytes) {
                    analyses.push(workdeck_analysis::analyze(&ReviewDocument {
                        repository_id: None,
                        path,
                        content,
                    })?);
                }
                workdeck_git::ObjectSink::put(&mut self.content, &bytes)?;
            }
            let manifest = serde_json::to_value(&pull)?;
            let manifest_hash =
                workdeck_git::ObjectSink::put(&mut self.content, &serde_json::to_vec(&manifest)?)?;
            checkpoint_sources.push(SnapshotSource {
                source: ReviewSource::PullRequest {
                    provider: "github".to_string(),
                    repository: repository.clone(),
                    number: *number,
                },
                revision: provider_revision("github-pr", &pull.head_sha, &manifest)?,
                manifest_hash,
            });
            source_manifests.push(manifest);
        }

        for (repository, run_id) in workflow_sources {
            ensure_capture_active(cancellation)?;
            let run = github.workflow_run_with_cancellation(repository, run_id, cancellation)?;
            let manifest = serde_json::to_value(&run)?;
            let manifest_hash =
                workdeck_git::ObjectSink::put(&mut self.content, &serde_json::to_vec(&manifest)?)?;
            checkpoint_sources.push(SnapshotSource {
                source: ReviewSource::CiRun {
                    provider: "github".to_string(),
                    repository: repository.clone(),
                    run_id: run_id.clone(),
                },
                revision: provider_revision("github-run", &run.head_sha, &manifest)?,
                manifest_hash,
            });
            analyses.push(workflow_analysis(repository, &run));
            source_manifests.push(manifest);
        }

        let artifact_store = workdeck_artifacts::ArtifactStore::new(&self.paths.artifacts)?;
        for artifact_id in artifact_sources {
            ensure_capture_active(cancellation)?;
            let manifest = artifact_store
                .find(artifact_id.as_str())?
                .with_context(|| format!("artifact {artifact_id} does not exist"))?;
            let files_root = artifact_store.files_root(artifact_id)?;
            for file in manifest.files.iter().filter(|file| {
                file.media_type.starts_with("text/")
                    || matches!(
                        file.path.extension().and_then(|value| value.to_str()),
                        Some("js" | "ts" | "tsx" | "rs" | "py" | "go" | "php" | "vue")
                    )
            }) {
                if file.size > 10_000_000 {
                    continue;
                }
                let bytes = std::fs::read(files_root.join(&file.path))?;
                let Ok(content) = std::str::from_utf8(&bytes) else {
                    continue;
                };
                analyses.push(workdeck_analysis::analyze(&ReviewDocument {
                    repository_id: None,
                    path: PathBuf::from("artifact")
                        .join(artifact_id.as_str())
                        .join(&file.path),
                    content,
                })?);
            }
            let manifest_value = serde_json::to_value(&manifest)?;
            let manifest_hash = workdeck_git::ObjectSink::put(
                &mut self.content,
                &serde_json::to_vec(&manifest_value)?,
            )?;
            checkpoint_sources.push(SnapshotSource {
                source: ReviewSource::Artifact {
                    artifact_id: artifact_id.clone(),
                },
                revision: format!(
                    "artifact:{}:{}",
                    artifact_id,
                    manifest.imported_at.to_rfc3339()
                ),
                manifest_hash,
            });
            source_manifests.push(manifest_value);
        }

        ensure_capture_active(cancellation)?;
        let checkpoint = ReviewCheckpoint {
            id: checkpoint_id,
            review_set_id: review.id.clone(),
            sequence,
            created_at: Utc::now(),
            sources: checkpoint_sources,
        };
        self.catalog.save_checkpoint(&checkpoint)?;
        let review = self.persist_review_analyses(&checkpoint, &analyses)?;
        self.catalog.append_event(
            "checkpoint_created",
            &serde_json::json!({
                "review_set_id": checkpoint.review_set_id,
                "snapshot_id": checkpoint.id,
                "sequence": checkpoint.sequence,
                "mixed_sources": true,
            }),
        )?;
        Ok(CapturedSnapshot {
            checkpoint,
            manifests,
            source_manifests,
            analyses,
            review,
        })
    }

    fn capture_commit_range(
        &mut self,
        repository_id: &RepositoryId,
        root: &Path,
        base: &str,
        head: &str,
    ) -> Result<(String, serde_json::Value, Vec<AnalysisResult>)> {
        let resolved_base = workdeck_git::resolve_revision(root, base)?;
        let resolved_head = workdeck_git::resolve_revision(root, head)?;
        let paths = workdeck_git::changed_paths_for_range(root, &resolved_base, &resolved_head)?;
        let patch = workdeck_git::diff_for_range(root, &resolved_base, &resolved_head, None)?;
        let patch_hash = workdeck_git::ObjectSink::put(&mut self.content, patch.as_bytes())?;
        let mut files = Vec::new();
        let mut analyses = Vec::new();
        for path in &paths {
            let base_bytes = workdeck_git::read_blob_at_revision(root, &resolved_base, path)?;
            let head_bytes = workdeck_git::read_blob_at_revision(root, &resolved_head, path)?;
            let base_hash = match &base_bytes {
                Some(bytes) => Some(workdeck_git::ObjectSink::put(&mut self.content, bytes)?),
                None => None,
            };
            let head_hash = match &head_bytes {
                Some(bytes) => Some(workdeck_git::ObjectSink::put(&mut self.content, bytes)?),
                None => None,
            };
            if let Some(bytes) = head_bytes
                && let Ok(content) = std::str::from_utf8(&bytes)
            {
                analyses.push(workdeck_analysis::analyze(&ReviewDocument {
                    repository_id: Some(repository_id.clone()),
                    path: path.clone(),
                    content,
                })?);
            }
            files.push(serde_json::json!({
                "path": path,
                "base_content_hash": base_hash,
                "head_content_hash": head_hash,
            }));
        }
        let manifest = serde_json::json!({
            "kind": "commit_range",
            "repository_id": repository_id,
            "root": root,
            "requested_base": base,
            "requested_head": head,
            "resolved_base": resolved_base,
            "resolved_head": resolved_head,
            "patch_hash": patch_hash,
            "files": files,
        });
        Ok((resolved_head, manifest, analyses))
    }

    fn persist_review_analyses(
        &mut self,
        checkpoint: &ReviewCheckpoint,
        analyses: &[AnalysisResult],
    ) -> Result<SnapshotReviewSummary> {
        let previous_checkpoint = self
            .catalog
            .list_checkpoints(&checkpoint.review_set_id)?
            .into_iter()
            .filter(|candidate| candidate.sequence < checkpoint.sequence)
            .max_by_key(|candidate| candidate.sequence);
        let previous_versions = match previous_checkpoint {
            Some(previous) => self
                .catalog
                .review_unit_versions_for_snapshot(&previous.id)?,
            None => Vec::new(),
        };
        let engine = crate::ReviewEngine;
        let mut current_versions = Vec::new();
        let mut used_previous = BTreeSet::new();
        let inputs = analyses
            .iter()
            .flat_map(|analysis| &analysis.units)
            .map(|input| (input.logical_key.as_str(), input))
            .collect::<BTreeMap<_, _>>();

        for input in inputs.into_values() {
            let stored_hash =
                workdeck_git::ObjectSink::put(&mut self.content, input.content.as_bytes())?;
            let historical_id = self.catalog.review_unit_id_for_key(&input.logical_key)?;
            let matched_previous = match historical_id.as_ref() {
                Some(unit_id) => previous_versions
                    .iter()
                    .find(|version| &version.unit_id == unit_id),
                None => unique_previous_match(input, &previous_versions, &used_previous),
            };
            let unit_id =
                historical_id.or_else(|| matched_previous.map(|version| version.unit_id.clone()));
            if let Some(previous) = matched_previous {
                used_previous.insert(previous.id.clone());
            }
            let version = engine.version_for_analyzed(checkpoint.id.clone(), unit_id, input);
            if stored_hash != version.anchor.content_hash {
                bail!("review unit content hash disagrees with object store");
            }
            self.catalog
                .save_review_unit_version(&input.logical_key, &version)?;
            current_versions.push(version);
        }

        let mut review = SnapshotReviewSummary::default();
        for current in &current_versions {
            let previous = previous_versions
                .iter()
                .find(|version| version.unit_id == current.unit_id);
            let delta = engine.compare(previous, Some(current));
            count_transition(&mut review, &delta);
            self.catalog.save_review_delta(&checkpoint.id, &delta)?;
        }
        for previous in previous_versions
            .iter()
            .filter(|version| !used_previous.contains(&version.id))
            .filter(|version| {
                !current_versions
                    .iter()
                    .any(|current| current.unit_id == version.unit_id)
            })
        {
            let delta = engine.compare(Some(previous), None);
            count_transition(&mut review, &delta);
            self.catalog.save_review_delta(&checkpoint.id, &delta)?;
        }
        review.total = current_versions.len();
        Ok(review)
    }

    pub fn mark_reviewed(
        &self,
        version_id: &ReviewUnitVersionId,
        state: ReviewMarkState,
        reviewer: &str,
    ) -> Result<ReviewMark> {
        self.catalog
            .review_unit_version(version_id)?
            .with_context(|| format!("review unit version {version_id} does not exist"))?;
        let mark = ReviewMark {
            unit_version_id: version_id.clone(),
            state,
            reviewer: reviewer.trim().to_string(),
            recorded_at: Utc::now(),
        };
        if mark.reviewer.is_empty() {
            bail!("reviewer cannot be empty");
        }
        self.catalog.save_review_mark(&mark)?;
        self.catalog.append_event(
            "review_marked",
            &serde_json::json!({
                "unit_version_id": version_id,
                "state": state,
                "reviewer": mark.reviewer,
            }),
        )?;
        Ok(mark)
    }

    pub fn effective_review_mark(
        &self,
        version_id: &ReviewUnitVersionId,
    ) -> Result<Option<EffectiveReviewMark>> {
        self.effective_review_mark_inner(version_id, &mut BTreeSet::new())
    }

    pub fn effective_review_marks(
        &self,
        version_ids: &[ReviewUnitVersionId],
    ) -> Result<BTreeMap<ReviewUnitVersionId, EffectiveReviewMark>> {
        let latest_marks = self.catalog.all_review_marks()?.into_iter().fold(
            BTreeMap::<ReviewUnitVersionId, ReviewMark>::new(),
            |mut marks, mark| {
                let replace = marks
                    .get(&mark.unit_version_id)
                    .is_none_or(|current| mark.recorded_at >= current.recorded_at);
                if replace {
                    marks.insert(mark.unit_version_id.clone(), mark);
                }
                marks
            },
        );
        let deltas = self
            .catalog
            .all_review_deltas()?
            .into_iter()
            .filter_map(|delta| delta.to_version.clone().map(|version| (version, delta)))
            .collect::<BTreeMap<_, _>>();
        let mut memo = BTreeMap::<ReviewUnitVersionId, Option<EffectiveReviewMark>>::new();
        let mut result = BTreeMap::new();
        for version_id in version_ids {
            if let Some(mark) = resolve_effective_review_mark(
                version_id,
                &latest_marks,
                &deltas,
                &mut memo,
                &mut BTreeSet::new(),
            )? {
                result.insert(version_id.clone(), mark);
            }
        }
        Ok(result)
    }

    fn effective_review_mark_inner(
        &self,
        version_id: &ReviewUnitVersionId,
        visited: &mut BTreeSet<ReviewUnitVersionId>,
    ) -> Result<Option<EffectiveReviewMark>> {
        if !visited.insert(version_id.clone()) {
            bail!("cycle detected in review-state lineage at {version_id}");
        }
        if let Some(mark) = self
            .catalog
            .review_marks_for_version(version_id)?
            .into_iter()
            .max_by_key(|mark| mark.recorded_at)
        {
            return Ok(Some(EffectiveReviewMark {
                version_id: version_id.clone(),
                state: mark.state,
                reviewer: mark.reviewer,
                recorded_at: mark.recorded_at,
                inherited_from: None,
            }));
        }
        let Some(delta) = self.catalog.review_delta_for_version(version_id)? else {
            return Ok(None);
        };
        if !delta.carry_review_state {
            return Ok(None);
        }
        let Some(previous_id) = delta.from_version else {
            return Ok(None);
        };
        Ok(self
            .effective_review_mark_inner(&previous_id, visited)?
            .map(|mut mark| {
                mark.version_id = version_id.clone();
                mark.inherited_from = Some(previous_id);
                mark
            }))
    }
}

fn resolve_effective_review_mark(
    version_id: &ReviewUnitVersionId,
    marks: &BTreeMap<ReviewUnitVersionId, ReviewMark>,
    deltas: &BTreeMap<ReviewUnitVersionId, ReviewDelta>,
    memo: &mut BTreeMap<ReviewUnitVersionId, Option<EffectiveReviewMark>>,
    visited: &mut BTreeSet<ReviewUnitVersionId>,
) -> Result<Option<EffectiveReviewMark>> {
    if let Some(mark) = memo.get(version_id) {
        return Ok(mark.clone());
    }
    if !visited.insert(version_id.clone()) {
        bail!("cycle detected in review-state lineage at {version_id}");
    }
    let resolved = if let Some(mark) = marks.get(version_id) {
        Some(EffectiveReviewMark {
            version_id: version_id.clone(),
            state: mark.state,
            reviewer: mark.reviewer.clone(),
            recorded_at: mark.recorded_at,
            inherited_from: None,
        })
    } else if let Some(delta) = deltas
        .get(version_id)
        .filter(|delta| delta.carry_review_state)
    {
        match delta.from_version.as_ref() {
            Some(previous_id) => {
                resolve_effective_review_mark(previous_id, marks, deltas, memo, visited)?.map(
                    |mut mark| {
                        mark.version_id = version_id.clone();
                        mark.inherited_from = Some(previous_id.clone());
                        mark
                    },
                )
            }
            None => None,
        }
    } else {
        None
    };
    visited.remove(version_id);
    memo.insert(version_id.clone(), resolved.clone());
    Ok(resolved)
}

fn workflow_analysis(
    repository: &str,
    run: &workdeck_github::WorkflowRunSnapshot,
) -> AnalysisResult {
    let path = PathBuf::from(format!("ci/{repository}/{}.json", run.id));
    let mut units = Vec::new();
    for job in &run.jobs {
        if job.steps.is_empty() {
            let content = format!(
                "{}:{}:{}",
                job.name,
                job.status,
                job.conclusion.as_deref().unwrap_or("pending")
            );
            units.push(ci_unit(repository, &path, &job.name, content));
            continue;
        }
        for step in &job.steps {
            let name = format!("{} / {}", job.name, step.name);
            let content = format!(
                "{}:{}:{}:{}",
                job.id,
                step.number,
                step.status,
                step.conclusion.as_deref().unwrap_or("pending")
            );
            units.push(ci_unit(repository, &path, &name, content));
        }
    }
    AnalysisResult {
        language: SourceLanguage::Text,
        units,
        symbols: Vec::new(),
        calls: Vec::new(),
        canonical_tree: None,
        diagnostics: Vec::new(),
    }
}

fn ci_unit(repository: &str, path: &Path, name: &str, content: String) -> AnalyzedUnit {
    AnalyzedUnit {
        logical_key: format!("github:{repository}:ci-step:{name}"),
        repository_id: None,
        path: path.to_path_buf(),
        qualified_name: Some(name.to_string()),
        kind: ReviewUnitKind::CiStep,
        title: name.to_string(),
        semantic_content: content.clone(),
        content,
        start_byte: 0,
        end_byte: 0,
        start_line: 0,
        end_line: 0,
        provenance: "github-actions".to_string(),
        confidence: AnalysisConfidence::Observed,
    }
}

fn provider_revision(prefix: &str, head: &str, manifest: &serde_json::Value) -> Result<String> {
    let bytes = serde_json::to_vec(manifest)?;
    Ok(format!(
        "{prefix}:{head}:{}",
        workdeck_domain::content_hash(&bytes)
    ))
}

fn ensure_capture_active(cancellation: &GitHubCancellation) -> Result<()> {
    if cancellation.is_cancelled() {
        bail!("checkpoint capture was cancelled");
    }
    Ok(())
}

fn unique_previous_match<'a>(
    input: &workdeck_analysis::AnalyzedUnit,
    previous: &'a [ReviewUnitVersion],
    used: &BTreeSet<ReviewUnitVersionId>,
) -> Option<&'a ReviewUnitVersion> {
    let content_hash = workdeck_domain::content_hash(input.content.as_bytes());
    let exact: Vec<_> = previous
        .iter()
        .filter(|version| !used.contains(&version.id))
        .filter(|version| version.anchor.content_hash == content_hash)
        .collect();
    if exact.len() == 1 {
        return exact.first().copied();
    }
    let named: Vec<_> = previous
        .iter()
        .filter(|version| !used.contains(&version.id))
        .filter(|version| version.kind == input.kind)
        .filter(|version| version.anchor.qualified_name == input.qualified_name)
        .collect();
    (named.len() == 1).then(|| named[0])
}

fn count_transition(summary: &mut SnapshotReviewSummary, delta: &ReviewDelta) {
    match delta.transition {
        UnitTransition::Unchanged => summary.unchanged += 1,
        UnitTransition::Moved | UnitTransition::Rebased => summary.moved += 1,
        UnitTransition::FormatOnly => summary.format_only += 1,
        UnitTransition::Modified | UnitTransition::DependencyImpact | UnitTransition::Ambiguous => {
            summary.modified += 1
        }
        UnitTransition::New => summary.new += 1,
        UnitTransition::Removed => summary.removed += 1,
    }
    if delta.carry_review_state {
        summary.review_state_carried += 1;
    }
}

pub fn resolve_project(catalog: &Catalog, value: &str) -> Result<WorkspaceProject> {
    if let Some(project) = catalog.project_by_name(value)? {
        return Ok(project);
    }
    catalog
        .list_projects(true)?
        .into_iter()
        .find(|project| project.id.as_str() == value)
        .with_context(|| format!("project {value} does not exist"))
}

pub fn resolve_review(catalog: &Catalog, value: &str) -> Result<ReviewSet> {
    catalog
        .list_reviews(true)?
        .into_iter()
        .find(|review| review.id.as_str() == value || review.title.eq_ignore_ascii_case(value))
        .with_context(|| format!("review {value} does not exist"))
}

pub fn resolve_repository(catalog: &Catalog, value: &str) -> Result<RepositoryRecord> {
    catalog
        .all_repositories()?
        .into_iter()
        .find(|repository| {
            repository.id.as_str() == value || repository.name.eq_ignore_ascii_case(value)
        })
        .with_context(|| format!("repository {value} does not exist"))
}

pub fn checkout_path_for_repository(
    catalog: &Catalog,
    repository: &RepositoryRecord,
) -> Result<PathBuf> {
    catalog
        .list_checkouts(&repository.id)?
        .into_iter()
        .find(|checkout| checkout.available && checkout.path.exists())
        .map(|checkout| checkout.path)
        .with_context(|| format!("repository {} has no available checkout", repository.name))
}

fn validate_repository_path(path: &Path) -> Result<()> {
    use std::path::Component;
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
    {
        bail!(
            "repository path {} must stay inside the checkout",
            path.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attaching_the_same_source_is_idempotent() {
        let root = tempfile::tempdir().expect("temporary catalog");
        let service = WorkdeckService::open(ApplicationPaths::at(root.path())).expect("service");
        let review = service.create_review("Global inbox").expect("review");
        let source = ReviewSource::LocalWorktree {
            worktree_id: WorktreeId::new(),
            base: None,
        };

        service
            .attach_source(&review.id, &source)
            .expect("first attach");
        service
            .attach_source(&review.id, &source)
            .expect("duplicate attach");

        assert_eq!(
            service.catalog.review_sources(&review.id).unwrap(),
            vec![source]
        );
    }

    #[test]
    fn batched_review_lineage_matches_direct_and_inherited_semantics() {
        let previous = ReviewUnitVersionId::new();
        let current = ReviewUnitVersionId::new();
        let recorded_at = Utc::now();
        let marks = BTreeMap::from([(
            previous.clone(),
            ReviewMark {
                unit_version_id: previous.clone(),
                state: ReviewMarkState::Reviewed,
                reviewer: "reviewer".into(),
                recorded_at,
            },
        )]);
        let deltas = BTreeMap::from([(
            current.clone(),
            ReviewDelta {
                unit_id: workdeck_domain::ReviewUnitId::new(),
                from_version: Some(previous.clone()),
                to_version: Some(current.clone()),
                transition: UnitTransition::Unchanged,
                carry_review_state: true,
                reason: "same semantic unit".into(),
            },
        )]);
        let inherited = resolve_effective_review_mark(
            &current,
            &marks,
            &deltas,
            &mut BTreeMap::new(),
            &mut BTreeSet::new(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(inherited.version_id, current);
        assert_eq!(inherited.inherited_from, Some(previous));
        assert_eq!(inherited.state, ReviewMarkState::Reviewed);
        assert_eq!(inherited.recorded_at, recorded_at);
    }

    #[test]
    fn cancelled_capture_never_persists_a_partial_checkpoint() {
        let root = tempfile::tempdir().expect("temporary catalog");
        let mut service =
            WorkdeckService::open(ApplicationPaths::at(root.path())).expect("service");
        let review = service.create_review("Cancelled capture").expect("review");
        let artifacts = [workdeck_domain::ArtifactId::new()];
        let cancellation = GitHubCancellation::default();
        cancellation.cancel();

        let error = service
            .capture_sources_checkpoint_with_cancellation(
                &review,
                CheckpointSources {
                    local: &[],
                    commit_ranges: &[],
                    markdown: &[],
                    pull_requests: &[],
                    workflow_runs: &[],
                    artifacts: &artifacts,
                },
                &cancellation,
            )
            .unwrap_err()
            .to_string();

        assert!(error.contains("cancelled"));
        assert!(
            service
                .catalog
                .list_checkpoints(&review.id)
                .unwrap()
                .is_empty()
        );
    }
}
