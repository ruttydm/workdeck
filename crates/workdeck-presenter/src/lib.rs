use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};
use workdeck_analysis::{AnalysisResult, ReviewDocument, analyze};
use workdeck_artifacts::ArtifactStore;
pub use workdeck_artifacts::{ArtifactManifest, ArtifactPreview};
pub use workdeck_core::{ApplicationPaths, PortfolioDiscoveryReport};
use workdeck_core::{
    CheckpointSources, EffectiveReviewMark, WorkdeckService, checkout_path_for_repository,
};
use workdeck_domain::{
    ActivityReadCursor, CheckoutRecord, InboxDisposition, InboxPreference, RepositoryRecord,
    ReviewCheckpoint, ReviewDelta, ReviewMark, ReviewMarkState, ReviewSet, ReviewSetId,
    ReviewSource, ReviewUnitVersion, ReviewUnitVersionId, UnitTransition, WorkspaceProject,
    WorktreeAttention, WorktreeRecord,
};
pub use workdeck_git::{
    AttentionScanCancellation, AttentionScanOptions, GitCommit, GitCommitDetail,
    GitCommitPathMatches, GitGraphRow, GitGraphSnapshot, GitReference, GitReferenceKind,
    GitSearchCancellation, GitStatusSummary, search_commits,
};
pub use workdeck_github::GitHubCancellation;

mod client;
pub use client::LocalWorkdeckClient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupPerformanceReport {
    pub schema: u32,
    pub marker: String,
    pub metrics_ms: BTreeMap<String, u64>,
}

/// Non-blocking, process-local performance evidence sink used by packaged
/// native QA. Product UI code only sends small messages; a dedicated worker
/// serializes the report under Workdeck's machine-local log directory.
#[derive(Clone)]
pub struct StartupTelemetry {
    sender: Sender<(String, u64)>,
}

impl StartupTelemetry {
    pub fn spawn(marker: &str) -> Result<Self> {
        let marker = normalize_telemetry_token(marker, "marker")?;
        let paths = ApplicationPaths::discover()?;
        paths.ensure()?;
        let report_path = paths.logs.join(format!("startup-{marker}.json"));
        let (sender, receiver) = mpsc::channel::<(String, u64)>();
        thread::Builder::new()
            .name("workdeck-startup-telemetry".into())
            .spawn(move || {
                let mut report = StartupPerformanceReport {
                    schema: 1,
                    marker,
                    metrics_ms: BTreeMap::new(),
                };
                while let Ok((phase, elapsed_ms)) = receiver.recv() {
                    report.metrics_ms.insert(phase, elapsed_ms);
                    match serde_json::to_vec_pretty(&report)
                        .map_err(anyhow::Error::from)
                        .and_then(|bytes| atomic_write(&report_path, &bytes))
                    {
                        Ok(()) => {}
                        Err(error) => eprintln!(
                            "could not persist startup telemetry {}: {error:#}",
                            report_path.display()
                        ),
                    }
                }
            })
            .context("failed to start startup telemetry worker")?;
        Ok(Self { sender })
    }

    pub fn record(&self, phase: &str, elapsed_ms: u64) {
        let Ok(phase) = normalize_telemetry_token(phase, "phase") else {
            return;
        };
        let _ = self.sender.send((phase, elapsed_ms));
    }
}

fn normalize_telemetry_token(value: &str, name: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("startup telemetry {name} must be 1-64 ASCII letters, digits, '-' or '_'");
    }
    Ok(value.into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub projects: Vec<WorkspaceProject>,
    pub repositories: Vec<RepositoryRecord>,
    pub checkouts: Vec<CheckoutRecord>,
    pub worktrees: Vec<WorktreeRecord>,
    pub worktree_attention: Vec<WorktreeAttention>,
    pub inbox_preferences: Vec<InboxPreference>,
    #[serde(default)]
    pub activity_read_cursors: Vec<ActivityReadCursor>,
    pub review_sets: Vec<ReviewAttention>,
    /// Compact metadata for the latest frozen revision of every active review
    /// set. Keeping this in the workspace snapshot makes global search instant
    /// without touching any reviewed repository or loading full file content.
    pub search_units: Vec<SearchUnit>,
    pub artifacts: Vec<ArtifactManifest>,
    pub loaded_at: DateTime<Utc>,
}

/// Read-only availability as observed by the latest bounded attention scan.
///
/// Catalog availability is intentionally preserved as historical state. A
/// linked worktree can disappear between discovery runs, so consumers must not
/// treat the persisted `available` bit as proof that Git can still be opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorktreeAvailability {
    Available,
    CatalogUnavailable,
    ObservedUnavailable,
    ScanWarning,
}

impl WorktreeAvailability {
    pub fn is_openable(self) -> bool {
        matches!(self, Self::Available | Self::ScanWarning)
    }

    pub fn is_observed_unavailable(self) -> bool {
        self == Self::ObservedUnavailable
    }
}

impl WorkspaceSnapshot {
    pub fn worktree_availability(&self, worktree: &WorktreeRecord) -> WorktreeAvailability {
        let attention = self
            .worktree_attention
            .iter()
            .find(|attention| attention.worktree_id == worktree.id);
        classify_worktree_availability(worktree, attention)
    }

    pub fn worktree_is_openable(&self, worktree: &WorktreeRecord) -> bool {
        self.worktree_availability(worktree).is_openable()
    }

    pub fn checkout_availability(&self, checkout: &CheckoutRecord) -> WorktreeAvailability {
        if !checkout.available {
            return WorktreeAvailability::CatalogUnavailable;
        }
        let mut child_count = 0;
        let mut observed_unavailable = false;
        for state in self
            .worktrees
            .iter()
            .filter(|worktree| worktree.checkout_id == checkout.id)
            .map(|worktree| self.worktree_availability(worktree))
        {
            child_count += 1;
            if state.is_openable() {
                return WorktreeAvailability::Available;
            }
            observed_unavailable |= state.is_observed_unavailable();
        }
        if child_count == 0 {
            WorktreeAvailability::Available
        } else if observed_unavailable {
            WorktreeAvailability::ObservedUnavailable
        } else {
            WorktreeAvailability::CatalogUnavailable
        }
    }
}

pub fn classify_worktree_availability(
    worktree: &WorktreeRecord,
    attention: Option<&WorktreeAttention>,
) -> WorktreeAvailability {
    if !worktree.available {
        return WorktreeAvailability::CatalogUnavailable;
    }
    match attention.and_then(|attention| attention.error.as_deref()) {
        Some(error) if attention_error_means_unavailable(error) => {
            WorktreeAvailability::ObservedUnavailable
        }
        Some(_) => WorktreeAvailability::ScanWarning,
        None => WorktreeAvailability::Available,
    }
}

fn attention_error_means_unavailable(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    [
        "no such file or directory",
        "cannot change to",
        "not a git repository",
        "repository path is unavailable",
        "could not find repository",
    ]
    .iter()
    .any(|needle| error.contains(needle))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct WorkspaceCache {
    schema: u32,
    snapshot: WorkspaceSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchUnit {
    pub review_set_id: ReviewSetId,
    pub review_title: String,
    pub version_id: ReviewUnitVersionId,
    pub title: String,
    pub path: String,
    pub kind: workdeck_domain::ReviewUnitKind,
    pub provenance: String,
    pub repository_backed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPreferences {
    /// Version of the presentation-only preference schema. Durable catalog
    /// records are intentionally outside this migration boundary.
    pub ui_schema_version: u32,
    /// Typed viewport state serialized by `workdeck-ui`. Keeping it as JSON at
    /// the persistence boundary lets newer UI models fail closed and migrate
    /// without coupling the presenter to visual navigation types.
    pub viewport_state: Option<serde_json::Value>,
    pub sidebar_collapsed: bool,
    pub projects_collapsed: bool,
    /// The portfolio navigator keeps the long tail and empty projects folded
    /// by default so active work remains immediately scannable.
    pub workspace_active_collapsed: bool,
    pub workspace_other_collapsed: bool,
    pub workspace_empty_collapsed: bool,
    pub tree_visible: bool,
    pub inspector_visible: bool,
    pub git_references_collapsed: bool,
    pub git_history_scope: String,
    pub collapsed_git_reference_groups: Vec<GitReferenceKind>,
    pub expanded_projects: Vec<workdeck_domain::ProjectId>,
    /// Expanded repository identities in the Workspaces hierarchy. Repository
    /// identity is stable across linked checkouts and process restarts.
    pub expanded_repositories: Vec<workdeck_domain::RepositoryId>,
    pub expanded_checkouts: Vec<workdeck_domain::CheckoutId>,
    pub destination: String,
    pub workspace_tab: String,
    pub review_mode: String,
    pub unit_filter: String,
    pub diff_layout: String,
    pub diff_ignore_whitespace: bool,
    pub selected_project: Option<workdeck_domain::ProjectId>,
    pub selected_review: Option<ReviewSetId>,
    pub selected_worktree: Option<workdeck_domain::WorktreeId>,
    pub selected_workspace_worktree: Option<workdeck_domain::WorktreeId>,
    pub selected_unit: Option<ReviewUnitVersionId>,
    /// User-curated global searches. These are presentation state only and are
    /// deliberately bounded by the UI before persistence.
    pub saved_searches: Vec<String>,
    /// Most recently activated global searches. Kept separate from saved
    /// searches so a transient query never becomes a user-curated favorite.
    pub recent_searches: Vec<String>,
    /// Stable identifier for the selected Inbox triage view.
    pub inbox_view: String,
}

impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            ui_schema_version: 8,
            viewport_state: None,
            sidebar_collapsed: false,
            projects_collapsed: false,
            workspace_active_collapsed: false,
            workspace_other_collapsed: true,
            workspace_empty_collapsed: true,
            tree_visible: true,
            inspector_visible: false,
            git_references_collapsed: false,
            git_history_scope: "all_refs".into(),
            collapsed_git_reference_groups: Vec::new(),
            expanded_projects: Vec::new(),
            expanded_repositories: Vec::new(),
            expanded_checkouts: Vec::new(),
            destination: "inbox".into(),
            workspace_tab: "changes".into(),
            review_mode: "diff".into(),
            unit_filter: "open".into(),
            diff_layout: "unified".into(),
            diff_ignore_whitespace: false,
            selected_project: None,
            selected_review: None,
            selected_worktree: None,
            selected_workspace_worktree: None,
            selected_unit: None,
            saved_searches: Vec::new(),
            recent_searches: Vec::new(),
            inbox_view: "today".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInboxItem {
    pub project: WorkspaceProject,
    pub repository: RepositoryRecord,
    pub worktree: WorktreeRecord,
    pub attention: WorktreeAttention,
    pub preference: Option<InboxPreference>,
}

/// Explainable facts used to rank and describe a suggested Inbox review.
///
/// Counts are retained only as effort context. Ordering uses bounded boolean
/// signals, so a huge generated tree cannot monopolize the reviewer's queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum InboxAttentionReason {
    Pinned,
    ChangedAfterBaseline,
    ScanFailed {
        retained_history: bool,
    },
    Uncaptured {
        commits: usize,
        changes: usize,
        truncated: bool,
    },
}

impl WorktreeInboxItem {
    pub fn attention_reasons(&self) -> Vec<InboxAttentionReason> {
        let mut reasons = Vec::with_capacity(3);
        if self
            .preference
            .as_ref()
            .is_some_and(|preference| preference.disposition == InboxDisposition::Pinned)
        {
            reasons.push(InboxAttentionReason::Pinned);
        }
        if self.preference.as_ref().is_some_and(|preference| {
            preference.disposition == InboxDisposition::Baseline
                && !preference.hides_signature(&self.attention.fingerprint)
        }) {
            reasons.push(InboxAttentionReason::ChangedAfterBaseline);
        }
        if self.attention.error.is_some() {
            reasons.push(InboxAttentionReason::ScanFailed {
                retained_history: self.attention.has_reviewable_work()
                    || !self.attention.fingerprint.is_empty(),
            });
        } else if self.attention.has_reviewable_work() {
            reasons.push(InboxAttentionReason::Uncaptured {
                commits: self.attention.commit_count,
                changes: self.attention.change_count,
                truncated: self.attention.truncated,
            });
        }
        reasons
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitWorkspaceSnapshot {
    pub project: WorkspaceProject,
    pub repository: RepositoryRecord,
    pub checkout: CheckoutRecord,
    pub worktree: WorktreeRecord,
    pub graph: GitGraphSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboxScanEvent {
    Started {
        total: usize,
        concurrency: usize,
    },
    Updated(WorktreeAttention),
    Progress {
        completed: usize,
        total: usize,
        dirty: usize,
        errors: usize,
    },
    Finished {
        completed: usize,
        total: usize,
        dirty: usize,
        errors: usize,
        cancelled: bool,
        duration_ms: u64,
    },
    Failed(String),
}

pub struct InboxScan {
    pub receiver: Receiver<InboxScanEvent>,
    pub cancellation: AttentionScanCancellation,
}

impl WorkspaceSnapshot {
    pub fn open_units(&self) -> usize {
        self.review_sets.iter().map(|review| review.open).sum()
    }

    pub fn source_advances(&self) -> usize {
        self.review_sets
            .iter()
            .map(|review| review.source_advances.len())
            .sum()
    }

    /// Returns actionable worktrees that are not already represented by an
    /// active review review. This is derived from the cached attention scan so
    /// the global inbox updates progressively without creating repository-local
    /// state or eagerly snapshotting every dirty checkout.
    pub fn worktree_inbox(&self) -> Vec<WorktreeInboxItem> {
        self.worktree_inbox_candidates(false)
    }

    /// Returns explicitly snoozed work so the Inbox can expose and restore it
    /// without allowing it back into the default decision queue early.
    pub fn snoozed_worktree_inbox(&self) -> Vec<WorktreeInboxItem> {
        self.worktree_inbox_candidates(true)
    }

    /// Provider/read failures are decisions about missing evidence, not review
    /// suggestions. Keep them in the Waiting view while preserving the last
    /// successfully cached attention state.
    pub fn waiting_worktree_inbox(&self) -> Vec<WorktreeInboxItem> {
        self.worktree_inbox()
            .into_iter()
            .filter(|item| item.attention.error.is_some())
            .collect()
    }

    /// Uncaptured review candidates that have enough evidence to start now.
    pub fn suggested_worktree_inbox(&self) -> Vec<WorktreeInboxItem> {
        self.worktree_inbox()
            .into_iter()
            .filter(|item| item.attention.error.is_none())
            .collect()
    }

    fn worktree_inbox_candidates(&self, snoozed_only: bool) -> Vec<WorktreeInboxItem> {
        let attached = self
            .review_sets
            .iter()
            .flat_map(|review| review.sources.iter())
            .filter_map(|source| match source {
                ReviewSource::LocalWorktree { worktree_id, .. } => Some(worktree_id.clone()),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let mut items = self
            .worktree_attention
            .iter()
            .filter(|attention| attention.has_reviewable_work() || attention.error.is_some())
            .filter(|attention| !attached.contains(&attention.worktree_id))
            .filter(|attention| {
                let preference = self
                    .inbox_preferences
                    .iter()
                    .find(|preference| preference.worktree_id == attention.worktree_id);
                if snoozed_only {
                    preference.is_some_and(|preference| preference.is_snoozed(Utc::now()))
                } else {
                    preference.is_none_or(|preference| {
                        !preference.is_snoozed(Utc::now())
                            && !preference.hides_signature(&attention.fingerprint)
                    })
                }
            })
            .filter_map(|attention| {
                let worktree = self
                    .worktrees
                    .iter()
                    .find(|worktree| worktree.id == attention.worktree_id)?;
                let checkout = self
                    .checkouts
                    .iter()
                    .find(|checkout| checkout.id == worktree.checkout_id)?;
                let repository = self
                    .repositories
                    .iter()
                    .find(|repository| repository.id == checkout.repository_id)?;
                let project = self
                    .projects
                    .iter()
                    .find(|project| project.id == repository.project_id)?;
                let preference = self
                    .inbox_preferences
                    .iter()
                    .find(|preference| preference.worktree_id == attention.worktree_id)
                    .cloned();
                Some(WorktreeInboxItem {
                    project: project.clone(),
                    repository: repository.clone(),
                    worktree: worktree.clone(),
                    attention: attention.clone(),
                    preference,
                })
            })
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            inbox_priority(right)
                .cmp(&inbox_priority(left))
                .then_with(|| {
                    left.project
                        .name
                        .to_ascii_lowercase()
                        .cmp(&right.project.name.to_ascii_lowercase())
                })
                .then_with(|| left.worktree.path.cmp(&right.worktree.path))
        });
        items
    }

    pub fn review_queue_items(&self) -> usize {
        self.review_sets.len() + self.worktree_inbox().len()
    }
}

fn inbox_priority(item: &WorktreeInboxItem) -> (bool, bool, bool, bool, bool, DateTime<Utc>) {
    let attention = &item.attention;
    (
        item.preference
            .as_ref()
            .is_some_and(|preference| preference.disposition == InboxDisposition::Pinned),
        item.preference.as_ref().is_some_and(|preference| {
            preference.disposition == InboxDisposition::Baseline
                && !preference.hides_signature(&attention.fingerprint)
        }),
        attention.change_count <= 500 && attention.commit_count <= 100,
        attention.change_count > 0 && attention.commit_count > 0,
        attention.commit_count > 0,
        attention.scanned_at,
    )
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewAttention {
    pub review: ReviewSet,
    pub sources: Vec<ReviewSource>,
    pub checkpoint_id: Option<String>,
    pub revision: u64,
    pub total: usize,
    pub open: usize,
    pub reviewed: usize,
    /// Open questions remain decisions even after the reviewer has seen the
    /// underlying unit. Keeping this count separate prevents the Inbox from
    /// presenting a questioned review as ordinary unfinished inventory.
    #[serde(default)]
    pub questioned: usize,
    pub inherited: usize,
    pub changed: usize,
    #[serde(default)]
    pub uncertain: usize,
    pub carried: usize,
    /// The smallest currently open semantic unit. This is presentation-only
    /// metadata derived while loading the frozen checkpoint; it never reads or
    /// advances the live repository.
    #[serde(default)]
    pub next_open_title: Option<String>,
    #[serde(default)]
    pub next_open_path: Option<String>,
    pub source_advances: Vec<SourceAdvance>,
}

impl ReviewAttention {
    pub fn progress(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            self.reviewed as f32 / self.total as f32
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceAdvance {
    pub source: ReviewSource,
    pub from: String,
    pub to: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewSnapshot {
    pub review: ReviewSet,
    pub sources: Vec<ReviewSource>,
    pub checkpoints: Vec<ReviewCheckpoint>,
    pub attention: ReviewAttention,
    pub selected_checkpoint: Option<ReviewCheckpoint>,
    /// Immutable provider/source manifests captured with the selected checkpoint.
    /// A corrupt or oversized manifest is isolated to its source instead of
    /// preventing the rest of the review from loading.
    pub evidence: Vec<SourceEvidenceSnapshot>,
    pub units: Vec<ReviewUnitSummary>,
    pub removed: Vec<ReviewDelta>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceEvidenceSnapshot {
    pub source: ReviewSource,
    pub revision: String,
    pub manifest: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderJobLog {
    pub repository: String,
    pub job_id: u64,
    pub job_name: String,
    pub text: String,
    pub original_bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewUnitSummary {
    pub version: ReviewUnitVersion,
    pub delta: Option<ReviewDelta>,
    pub review: Option<EffectiveReviewMark>,
}

impl ReviewUnitSummary {
    pub fn is_open(&self) -> bool {
        self.review
            .as_ref()
            .is_none_or(|review| !review.state.closes_review())
    }

    pub fn path(&self) -> String {
        self.version
            .anchor
            .path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default()
    }

    pub fn transition(&self) -> UnitTransition {
        self.delta
            .as_ref()
            .map(|delta| delta.transition)
            .unwrap_or(UnitTransition::Ambiguous)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewUnitDetail {
    pub summary: ReviewUnitSummary,
    pub content: String,
    pub previous_content: Option<String>,
    pub analysis: AnalysisResult,
}

#[derive(Debug, Clone)]
pub struct RuntimeHandle {
    sender: Sender<RuntimeCommand>,
}

impl RuntimeHandle {
    pub fn spawn_default() -> Result<Self> {
        Self::spawn(ApplicationPaths::discover()?)
    }

    pub fn spawn(paths: ApplicationPaths) -> Result<Self> {
        paths.ensure()?;
        let service = WorkdeckService::open(paths).context("failed to open Workdeck runtime")?;
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("workdeck-runtime".into())
            .spawn(move || runtime_loop(service, receiver))
            .context("failed to start Workdeck runtime")?;
        Ok(Self { sender })
    }

    pub fn load_workspace(&self) -> Receiver<Result<WorkspaceSnapshot>> {
        self.request(|service| load_workspace(service))
    }

    pub fn discover_portfolio(
        &self,
        roots: Vec<PathBuf>,
    ) -> Receiver<Result<PortfolioDiscoveryReport>> {
        self.request(move |service| service.discover_portfolio_roots(&roots))
    }

    pub fn load_cached_workspace(&self) -> Receiver<Result<Option<WorkspaceSnapshot>>> {
        self.request(|service| read_workspace_cache(&service.paths))
    }

    pub fn load_ui_preferences(&self) -> Receiver<Result<UiPreferences>> {
        self.request(|service| read_ui_preferences(&service.paths))
    }

    pub fn save_ui_preferences(&self, preferences: UiPreferences) -> Receiver<Result<()>> {
        self.request(move |service| write_ui_preferences(&service.paths, &preferences))
    }

    pub fn save_activity_read_cursor(
        &self,
        cursor: ActivityReadCursor,
    ) -> Receiver<Result<ActivityReadCursor>> {
        self.request(move |service| {
            service.catalog.save_activity_read_cursor(&cursor)?;
            Ok(cursor)
        })
    }

    pub fn scan_inbox(
        &self,
        mut worktrees: Vec<WorktreeRecord>,
        cached: &[WorktreeAttention],
        options: AttentionScanOptions,
    ) -> InboxScan {
        worktrees.sort_by_key(|worktree| {
            let cached = cached
                .iter()
                .find(|attention| attention.worktree_id == worktree.id);
            (
                !cached.is_some_and(WorktreeAttention::has_reviewable_work),
                cached.map(|attention| attention.scanned_at),
                worktree.path.clone(),
            )
        });
        let scan = workdeck_git::start_attention_scan(worktrees, options);
        let cancellation = scan.cancellation.clone();
        let runtime = self.clone();
        let (events, receiver) = mpsc::channel();
        let cancellation_for_thread = cancellation.clone();
        thread::Builder::new()
            .name("workdeck-inbox-persistence".into())
            .spawn(move || {
                let mut pending = Vec::new();
                while let Ok(event) = scan.receiver.recv() {
                    let mapped = match event {
                        workdeck_git::AttentionScanEvent::Started { total, concurrency } => {
                            InboxScanEvent::Started { total, concurrency }
                        }
                        workdeck_git::AttentionScanEvent::Updated(attention) => {
                            pending.push(attention.clone());
                            InboxScanEvent::Updated(attention)
                        }
                        workdeck_git::AttentionScanEvent::Progress {
                            completed,
                            total,
                            dirty,
                            errors,
                        } => InboxScanEvent::Progress {
                            completed,
                            total,
                            dirty,
                            errors,
                        },
                        workdeck_git::AttentionScanEvent::Finished {
                            completed,
                            total,
                            dirty,
                            errors,
                            cancelled,
                            duration_ms,
                        } => {
                            let batch = std::mem::take(&mut pending);
                            match runtime
                                .request(move |service| {
                                    service.catalog.save_worktree_attentions(&batch)
                                })
                                .recv()
                            {
                                Ok(Ok(())) => InboxScanEvent::Finished {
                                    completed,
                                    total,
                                    dirty,
                                    errors,
                                    cancelled,
                                    duration_ms,
                                },
                                Ok(Err(error)) => {
                                    cancellation_for_thread.cancel();
                                    InboxScanEvent::Failed(format!(
                                        "failed to save inbox results: {error:#}"
                                    ))
                                }
                                Err(error) => {
                                    cancellation_for_thread.cancel();
                                    InboxScanEvent::Failed(format!(
                                        "runtime disconnected while saving inbox: {error}"
                                    ))
                                }
                            }
                        }
                    };
                    let terminal = matches!(
                        mapped,
                        InboxScanEvent::Finished { .. } | InboxScanEvent::Failed(_)
                    );
                    if events.send(mapped).is_err() || terminal {
                        break;
                    }
                }
            })
            .expect("failed to start Workdeck inbox persistence");
        InboxScan {
            receiver,
            cancellation,
        }
    }

    pub fn load_review_snapshot(
        &self,
        review_set_id: ReviewSetId,
    ) -> Receiver<Result<ReviewSnapshot>> {
        self.request(move |service| load_review_snapshot(service, &review_set_id))
    }

    pub fn load_unit(&self, version_id: ReviewUnitVersionId) -> Receiver<Result<ReviewUnitDetail>> {
        self.request(move |service| load_unit(service, &version_id))
    }

    pub fn load_git_workspace(
        &self,
        worktree_id: workdeck_domain::WorktreeId,
        commit_limit: usize,
    ) -> Receiver<Result<GitWorkspaceSnapshot>> {
        self.request(move |service| load_git_workspace(service, &worktree_id, commit_limit))
    }

    pub fn load_git_commit_detail(
        &self,
        worktree_id: workdeck_domain::WorktreeId,
        oid: String,
    ) -> Receiver<Result<GitCommitDetail>> {
        self.request(move |service| load_git_commit_detail(service, &worktree_id, &oid))
    }

    pub fn filter_git_commits_by_path(
        &self,
        worktree_id: workdeck_domain::WorktreeId,
        commit_oids: Vec<String>,
        path_fragment: String,
        cancellation: GitSearchCancellation,
    ) -> Receiver<Result<GitCommitPathMatches>> {
        self.request(move |service| {
            filter_git_commits_by_path(
                service,
                &worktree_id,
                &commit_oids,
                &path_fragment,
                &cancellation,
            )
        })
    }

    pub fn mark_unit(
        &self,
        version_id: ReviewUnitVersionId,
        state: ReviewMarkState,
        reviewer: impl Into<String>,
    ) -> Receiver<Result<ReviewMark>> {
        let reviewer = reviewer.into();
        self.request(move |service| service.mark_reviewed(&version_id, state, &reviewer))
    }

    pub fn create_project(&self, name: impl Into<String>) -> Receiver<Result<WorkspaceProject>> {
        let name = name.into();
        self.request(move |service| service.create_project(&name))
    }

    pub fn add_repository(
        &self,
        project_id: workdeck_domain::ProjectId,
        path: PathBuf,
    ) -> Receiver<Result<RepositoryRecord>> {
        self.request(move |service| {
            let project = service
                .catalog
                .list_projects(true)?
                .into_iter()
                .find(|project| project.id == project_id)
                .with_context(|| format!("project {project_id} does not exist"))?;
            service.add_repository(&project, &path).map(|value| value.0)
        })
    }

    pub fn create_review(&self, title: impl Into<String>) -> Receiver<Result<ReviewSet>> {
        let title = title.into();
        self.request(move |service| service.create_review(&title))
    }

    pub fn attach_worktree(
        &self,
        review_set_id: ReviewSetId,
        worktree_id: workdeck_domain::WorktreeId,
    ) -> Receiver<Result<()>> {
        self.request(move |service| {
            service.attach_source(
                &review_set_id,
                &ReviewSource::LocalWorktree {
                    worktree_id,
                    base: None,
                },
            )
        })
    }

    pub fn attach_source(
        &self,
        review_set_id: ReviewSetId,
        source: ReviewSource,
    ) -> Receiver<Result<()>> {
        self.request(move |service| service.attach_source(&review_set_id, &source))
    }

    pub fn capture_review(&self, review_set_id: ReviewSetId) -> Receiver<Result<ReviewCheckpoint>> {
        self.request(move |service| capture_review(service, &review_set_id))
    }

    pub fn capture_review_with_cancellation(
        &self,
        review_set_id: ReviewSetId,
        cancellation: GitHubCancellation,
    ) -> Receiver<Result<ReviewCheckpoint>> {
        self.request(move |service| {
            capture_review_with_cancellation(service, &review_set_id, &cancellation)
        })
    }

    pub fn prepare_worktree_review(
        &self,
        worktree_id: workdeck_domain::WorktreeId,
    ) -> Receiver<Result<ReviewSet>> {
        self.request(move |service| prepare_worktree_review(service, &worktree_id))
    }

    pub fn prepare_commit_range_review(
        &self,
        repository_id: workdeck_domain::RepositoryId,
        base: String,
        head: String,
    ) -> Receiver<Result<ReviewSet>> {
        self.request(move |service| {
            prepare_commit_range_review(service, &repository_id, &base, &head)
        })
    }

    pub fn set_inbox_disposition(
        &self,
        worktree_id: workdeck_domain::WorktreeId,
        disposition: InboxDisposition,
    ) -> Receiver<Result<Option<InboxPreference>>> {
        self.request(move |service| {
            if disposition == InboxDisposition::Active {
                service.catalog.remove_inbox_preference(&worktree_id)?;
                return Ok(None);
            }
            let attention = service
                .catalog
                .all_worktree_attention()?
                .into_iter()
                .find(|attention| attention.worktree_id == worktree_id)
                .with_context(|| format!("worktree {worktree_id} has not been scanned yet"))?;
            let baseline_signature = if disposition == InboxDisposition::Baseline {
                if attention.fingerprint.is_empty() {
                    bail!("refresh this worktree before marking its current state as baseline");
                }
                Some(attention.fingerprint)
            } else {
                None
            };
            let snoozed_until = (disposition == InboxDisposition::Snoozed)
                .then(|| Utc::now() + ChronoDuration::days(1));
            let preference = InboxPreference {
                worktree_id,
                disposition,
                snoozed_until,
                baseline_signature,
                updated_at: Utc::now(),
            };
            service.catalog.save_inbox_preference(&preference)?;
            Ok(Some(preference))
        })
    }

    pub fn import_artifact(
        &self,
        zip_path: PathBuf,
        name: impl Into<String>,
    ) -> Receiver<Result<ArtifactManifest>> {
        let name = name.into();
        self.request(move |service| {
            ArtifactStore::new(&service.paths.artifacts)?.import_zip(
                &zip_path,
                name,
                zip_path.display().to_string(),
            )
        })
    }

    pub fn preview_artifact(
        &self,
        artifact_id: workdeck_domain::ArtifactId,
    ) -> Receiver<Result<ArtifactPreview>> {
        self.request(move |service| {
            let store = ArtifactStore::new(&service.paths.artifacts)?;
            let manifest = store
                .find(artifact_id.as_str())?
                .with_context(|| format!("artifact {artifact_id} does not exist"))?;
            store.start_preview(&manifest)
        })
    }

    pub fn load_github_job_log(
        &self,
        repository: String,
        job_id: u64,
        job_name: String,
        cancellation: GitHubCancellation,
    ) -> Receiver<Result<ProviderJobLog>> {
        self.request(move |_| {
            let bytes = workdeck_github::GitHubClient::default().job_log_with_cancellation(
                &repository,
                job_id,
                &cancellation,
            )?;
            Ok(provider_job_log(repository, job_id, job_name, &bytes))
        })
    }

    pub fn import_github_artifact(
        &self,
        repository: String,
        artifact_id: u64,
        artifact_name: String,
        cancellation: GitHubCancellation,
    ) -> Receiver<Result<ArtifactManifest>> {
        self.request(move |service| {
            let temporary = tempfile::Builder::new()
                .prefix("github-artifact-")
                .tempdir_in(&service.paths.root)?;
            let archive = workdeck_github::GitHubClient::default()
                .download_artifact_with_cancellation(
                    &repository,
                    artifact_id,
                    temporary.path(),
                    &cancellation,
                )?;
            ArtifactStore::new(&service.paths.artifacts)?.import_zip(
                &archive,
                artifact_name,
                format!("github:{repository}:artifact:{artifact_id}"),
            )
        })
    }

    fn request<T, F>(&self, action: F) -> Receiver<Result<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut WorkdeckService) -> Result<T> + Send + 'static,
    {
        let (reply, receiver) = mpsc::channel();
        let disconnected_reply = reply.clone();
        if self
            .sender
            .send(RuntimeCommand::Run(Box::new(move |service| {
                let _ = reply.send(action(service));
            })))
            .is_err()
        {
            let _ = disconnected_reply.send(Err(anyhow!("Workdeck runtime is not available")));
        }
        receiver
    }
}

fn provider_job_log(
    repository: String,
    job_id: u64,
    job_name: String,
    bytes: &[u8],
) -> ProviderJobLog {
    const UI_LOG_LIMIT: usize = 8 * 1024 * 1024;
    let truncated = bytes.len() > UI_LOG_LIMIT;
    let bounded = &bytes[..bytes.len().min(UI_LOG_LIMIT)];
    let text = strip_terminal_controls(&String::from_utf8_lossy(bounded));
    ProviderJobLog {
        repository,
        job_id,
        job_name,
        text,
        original_bytes: bytes.len(),
        truncated,
    }
}

fn strip_terminal_controls(value: &str) -> String {
    #[derive(Clone, Copy)]
    enum State {
        Text,
        Escape,
        Csi,
        Osc,
        OscEscape,
    }
    let mut state = State::Text;
    let mut result = String::with_capacity(value.len());
    for character in value.chars() {
        state = match state {
            State::Text if character == '\u{1b}' => State::Escape,
            State::Text => {
                if character == '\n' || character == '\t' || (!character.is_control()) {
                    result.push(character);
                }
                State::Text
            }
            State::Escape if character == '[' => State::Csi,
            State::Escape if character == ']' => State::Osc,
            State::Escape => State::Text,
            State::Csi if ('@'..='~').contains(&character) => State::Text,
            State::Csi => State::Csi,
            State::Osc if character == '\u{7}' => State::Text,
            State::Osc if character == '\u{1b}' => State::OscEscape,
            State::Osc => State::Osc,
            State::OscEscape if character == '\\' => State::Text,
            State::OscEscape => State::Osc,
        };
    }
    result
}

pub fn load_default_appearance() -> Result<String> {
    let paths = ApplicationPaths::discover()?;
    let path = paths.root.join("appearance");
    match std::fs::read_to_string(&path) {
        Ok(value) => normalize_appearance(&value).map(str::to_string),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok("system".into()),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

pub fn save_default_appearance(value: &str) -> Result<()> {
    let normalized = normalize_appearance(value)?;
    let paths = ApplicationPaths::discover()?;
    paths.ensure()?;
    atomic_write(&paths.root.join("appearance"), normalized.as_bytes())
}

fn normalize_appearance(value: &str) -> Result<&'static str> {
    Ok(match value.trim().to_ascii_lowercase().as_str() {
        "system" => "system",
        "light" => "light",
        "dark" => "dark",
        _ => return Err(anyhow!("appearance must be system, light, or dark")),
    })
}

enum RuntimeCommand {
    Run(Box<dyn FnOnce(&mut WorkdeckService) + Send>),
}

fn runtime_loop(mut service: WorkdeckService, receiver: Receiver<RuntimeCommand>) {
    while let Ok(command) = receiver.recv() {
        match command {
            RuntimeCommand::Run(action) => action(&mut service),
        }
    }
}

fn read_ui_preferences(paths: &ApplicationPaths) -> Result<UiPreferences> {
    let path = paths.root.join("ui-preferences.json");
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .with_context(|| format!("could not parse {}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(UiPreferences::default()),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

fn write_ui_preferences(paths: &ApplicationPaths, preferences: &UiPreferences) -> Result<()> {
    let path = paths.root.join("ui-preferences.json");
    if let Ok(existing) = std::fs::read(&path) {
        let existing_version = serde_json::from_slice::<serde_json::Value>(&existing)
            .ok()
            .and_then(|value| {
                value
                    .get("ui_schema_version")
                    .and_then(|value| value.as_u64())
            })
            .unwrap_or_default();
        if existing_version < u64::from(preferences.ui_schema_version) {
            let backup = paths
                .root
                .join(format!("ui-preferences.v{existing_version}.backup.json"));
            if !backup.exists() {
                atomic_write(&backup, &existing)?;
            }
        }
    }
    let bytes = serde_json::to_vec_pretty(preferences)?;
    atomic_write(&path, &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)
            .with_context(|| format!("could not open {}", temporary.display()))?;
        file.write_all(bytes)
            .with_context(|| format!("could not write {}", temporary.display()))?;
        file.sync_all()
            .with_context(|| format!("could not sync {}", temporary.display()))?;
        std::fs::rename(&temporary, path)
            .with_context(|| format!("could not replace {}", path.display()))?;
        if let Some(parent) = path.parent() {
            std::fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .with_context(|| format!("could not sync {}", parent.display()))?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn read_workspace_cache(paths: &ApplicationPaths) -> Result<Option<WorkspaceSnapshot>> {
    let path = paths.root.join("workspace-cache.json");
    match std::fs::read(&path) {
        Ok(bytes) => {
            let cache = serde_json::from_slice::<WorkspaceCache>(&bytes)
                .with_context(|| format!("could not parse {}", path.display()))?;
            Ok((cache.schema == 1).then_some(cache.snapshot))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("could not read {}", path.display())),
    }
}

fn write_workspace_cache(paths: &ApplicationPaths, snapshot: &WorkspaceSnapshot) -> Result<()> {
    let bytes = serde_json::to_vec(&WorkspaceCache {
        schema: 1,
        snapshot: snapshot.clone(),
    })?;
    atomic_write(&paths.root.join("workspace-cache.json"), &bytes)
}

fn load_workspace(service: &WorkdeckService) -> Result<WorkspaceSnapshot> {
    let projects = service.catalog.list_projects(false)?;
    let repositories = service.catalog.all_repositories()?;
    let checkouts = service.catalog.all_checkouts()?;
    let worktrees = service.catalog.all_worktrees()?;
    let worktree_attention = service.catalog.all_worktree_attention()?;
    let inbox_preferences = service.catalog.all_inbox_preferences()?;
    let activity_read_cursors = service.catalog.all_activity_read_cursors()?;
    let artifacts = ArtifactStore::new(&service.paths.artifacts)?.list()?;
    let mut review_rows = service
        .catalog
        .list_reviews(false)?
        .iter()
        .map(|review| review_attention_with_versions(service, review))
        .collect::<Result<Vec<_>>>()?;
    review_rows.sort_by_key(|(review, _)| {
        std::cmp::Reverse((
            !review.source_advances.is_empty(),
            review.open,
            review.changed,
            review.review.updated_at,
        ))
    });
    let mut search_units = Vec::new();
    for (review, versions) in &review_rows {
        search_units.extend(versions.iter().cloned().map(|version| {
            SearchUnit {
                review_set_id: review.review.id.clone(),
                review_title: review.review.title.clone(),
                version_id: version.id,
                title: version.title,
                path: version
                    .anchor
                    .path
                    .as_ref()
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                kind: version.kind,
                provenance: version.provenance,
                repository_backed: version.anchor.repository_id.is_some(),
            }
        }));
    }
    search_units.sort_by(|left, right| {
        left.review_title
            .to_ascii_lowercase()
            .cmp(&right.review_title.to_ascii_lowercase())
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.title.cmp(&right.title))
    });
    let review_sets = review_rows.into_iter().map(|(review, _)| review).collect();
    let snapshot = WorkspaceSnapshot {
        projects,
        repositories,
        checkouts,
        worktrees,
        worktree_attention,
        inbox_preferences,
        activity_read_cursors,
        review_sets,
        search_units,
        artifacts,
        loaded_at: Utc::now(),
    };
    if let Err(error) = write_workspace_cache(&service.paths, &snapshot) {
        eprintln!("could not update workspace cache: {error:#}");
    }
    Ok(snapshot)
}

fn load_git_workspace(
    service: &WorkdeckService,
    worktree_id: &workdeck_domain::WorktreeId,
    commit_limit: usize,
) -> Result<GitWorkspaceSnapshot> {
    let worktree = service
        .catalog
        .find_worktree(worktree_id.as_str())?
        .with_context(|| format!("worktree {worktree_id} does not exist"))?;
    if !worktree.available || !worktree.path.exists() {
        bail!(
            "worktree {} is currently unavailable",
            worktree.path.display()
        );
    }
    let checkout = service
        .catalog
        .all_checkouts()?
        .into_iter()
        .find(|checkout| checkout.id == worktree.checkout_id)
        .with_context(|| format!("checkout {} does not exist", worktree.checkout_id))?;
    let repository = service
        .catalog
        .repository(&checkout.repository_id)?
        .with_context(|| format!("repository {} does not exist", checkout.repository_id))?;
    let project = service
        .catalog
        .list_projects(true)?
        .into_iter()
        .find(|project| project.id == repository.project_id)
        .with_context(|| format!("project {} does not exist", repository.project_id))?;
    let mut graph = workdeck_git::load_graph_without_status(&worktree.path, commit_limit)?;
    if let Some(attention) = service
        .catalog
        .all_worktree_attention()?
        .into_iter()
        .find(|attention| attention.worktree_id == worktree.id)
    {
        graph.status.unstaged = attention.change_count;
    }
    Ok(GitWorkspaceSnapshot {
        project,
        repository,
        checkout,
        worktree,
        graph,
    })
}

fn load_git_commit_detail(
    service: &WorkdeckService,
    worktree_id: &workdeck_domain::WorktreeId,
    oid: &str,
) -> Result<GitCommitDetail> {
    let worktree = service
        .catalog
        .find_worktree(worktree_id.as_str())?
        .with_context(|| format!("worktree {worktree_id} does not exist"))?;
    if !worktree.available || !worktree.path.exists() {
        bail!(
            "worktree {} is currently unavailable",
            worktree.path.display()
        );
    }
    workdeck_git::load_commit_detail(&worktree.path, oid, 500)
}

fn filter_git_commits_by_path(
    service: &WorkdeckService,
    worktree_id: &workdeck_domain::WorktreeId,
    commit_oids: &[String],
    path_fragment: &str,
    cancellation: &GitSearchCancellation,
) -> Result<GitCommitPathMatches> {
    let worktree = service
        .catalog
        .find_worktree(worktree_id.as_str())?
        .with_context(|| format!("worktree {worktree_id} does not exist"))?;
    if !worktree.available || !worktree.path.exists() {
        bail!(
            "worktree {} is currently unavailable",
            worktree.path.display()
        );
    }
    workdeck_git::commits_touching_path(&worktree.path, commit_oids, path_fragment, cancellation)
}

fn review_attention(service: &WorkdeckService, review: &ReviewSet) -> Result<ReviewAttention> {
    review_attention_with_versions(service, review).map(|(attention, _)| attention)
}

fn review_attention_with_versions(
    service: &WorkdeckService,
    review: &ReviewSet,
) -> Result<(ReviewAttention, Vec<ReviewUnitVersion>)> {
    let checkpoints = service.catalog.list_checkpoints(&review.id)?;
    let sources = service.catalog.review_sources(&review.id)?;
    let Some(latest) = checkpoints.iter().max_by_key(|value| value.sequence) else {
        return Ok((
            ReviewAttention {
                review: review.clone(),
                sources,
                checkpoint_id: None,
                revision: 0,
                total: 0,
                open: 0,
                reviewed: 0,
                questioned: 0,
                inherited: 0,
                changed: 0,
                uncertain: 0,
                carried: 0,
                next_open_title: None,
                next_open_path: None,
                source_advances: Vec::new(),
            },
            Vec::new(),
        ));
    };
    let versions = service
        .catalog
        .review_unit_versions_for_snapshot(&latest.id)?;
    let deltas = service.catalog.review_deltas_for_snapshot(&latest.id)?;
    let mut open = 0;
    let mut reviewed = 0;
    let mut questioned = 0;
    let mut inherited = 0;
    let mut next_open_title = None;
    let mut next_open_path = None;
    let effective_marks = service.effective_review_marks(
        &versions
            .iter()
            .map(|version| version.id.clone())
            .collect::<Vec<_>>(),
    )?;
    for version in &versions {
        match effective_marks.get(&version.id) {
            Some(mark) => {
                inherited += usize::from(mark.inherited_from.is_some());
                questioned += usize::from(mark.state == ReviewMarkState::Questioned);
                if mark.state.closes_review() {
                    reviewed += 1;
                } else {
                    open += 1;
                    if next_open_title.is_none() {
                        next_open_title = Some(version.title.clone());
                        next_open_path = version
                            .anchor
                            .path
                            .as_ref()
                            .map(|path| path.to_string_lossy().into_owned());
                    }
                }
            }
            None => {
                open += 1;
                if next_open_title.is_none() {
                    next_open_title = Some(version.title.clone());
                    next_open_path = version
                        .anchor
                        .path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned());
                }
            }
        }
    }
    let source_advances = latest
        .sources
        .iter()
        .filter_map(
            |source| match service.live_source_revision(&source.source) {
                Ok(to) if to != source.revision => Some(SourceAdvance {
                    source: source.source.clone(),
                    from: source.revision.clone(),
                    to: Some(to),
                    error: None,
                }),
                Ok(_) => None,
                Err(error) => Some(SourceAdvance {
                    source: source.source.clone(),
                    from: source.revision.clone(),
                    to: None,
                    error: Some(error.to_string()),
                }),
            },
        )
        .collect();
    Ok((
        ReviewAttention {
            review: review.clone(),
            sources,
            checkpoint_id: Some(latest.id.to_string()),
            revision: latest.sequence,
            total: versions.len(),
            open,
            reviewed,
            questioned,
            inherited,
            changed: deltas
                .iter()
                .filter(|delta| delta.to_version.is_some() && !delta.carry_review_state)
                .count(),
            uncertain: deltas
                .iter()
                .filter(|delta| delta.transition == UnitTransition::Ambiguous)
                .count(),
            carried: deltas
                .iter()
                .filter(|delta| delta.carry_review_state)
                .count(),
            next_open_title,
            next_open_path,
            source_advances,
        },
        versions,
    ))
}

fn prepare_worktree_review(
    service: &mut WorkdeckService,
    worktree_id: &workdeck_domain::WorktreeId,
) -> Result<ReviewSet> {
    let worktree = service
        .catalog
        .find_worktree(worktree_id.as_str())?
        .with_context(|| format!("worktree {worktree_id} does not exist"))?;
    if !worktree.available || !worktree.path.exists() {
        bail!(
            "worktree {} is currently unavailable",
            worktree.path.display()
        );
    }

    for review in service.catalog.list_reviews(false)? {
        let represents_worktree =
            service
                .catalog
                .review_sources(&review.id)?
                .iter()
                .any(|source| {
                    matches!(
                        source,
                        ReviewSource::LocalWorktree {
                            worktree_id: attached,
                            ..
                        } if attached == worktree_id
                    )
                });
        if represents_worktree {
            if service.catalog.list_checkpoints(&review.id)?.is_empty() {
                capture_review(service, &review.id)?;
            }
            return Ok(review);
        }
    }

    let checkout = service
        .catalog
        .all_checkouts()?
        .into_iter()
        .find(|checkout| checkout.id == worktree.checkout_id)
        .with_context(|| format!("checkout {} does not exist", worktree.checkout_id))?;
    let repository = service
        .catalog
        .repository(&checkout.repository_id)?
        .with_context(|| format!("repository {} does not exist", checkout.repository_id))?;
    let project = service
        .catalog
        .list_projects(true)?
        .into_iter()
        .find(|project| project.id == repository.project_id)
        .with_context(|| format!("project {} does not exist", repository.project_id))?;
    let branch = worktree
        .branch
        .as_deref()
        .unwrap_or("detached")
        .trim_start_matches("refs/heads/");
    let title = worktree_review_title(&project.name, &repository.name, branch);
    let review = service.create_review(&title)?;
    service.attach_source(
        &review.id,
        &ReviewSource::LocalWorktree {
            worktree_id: worktree.id.clone(),
            base: None,
        },
    )?;
    capture_review(service, &review.id)
        .with_context(|| format!("failed to prepare review review {title}"))?;
    Ok(review)
}

fn worktree_review_title(project: &str, repository: &str, branch: &str) -> String {
    let context = if project.eq_ignore_ascii_case(repository) {
        project.to_string()
    } else {
        format!("{project} / {repository}")
    };
    if matches!(branch, "main" | "master") {
        context
    } else {
        format!("{context} · {branch}")
    }
}

fn prepare_commit_range_review(
    service: &mut WorkdeckService,
    repository_id: &workdeck_domain::RepositoryId,
    base: &str,
    head: &str,
) -> Result<ReviewSet> {
    if base.trim().is_empty() || head.trim().is_empty() || base == head {
        bail!("choose two different commits for a range review");
    }
    let source = ReviewSource::CommitRange {
        repository_id: repository_id.clone(),
        base: base.to_string(),
        head: head.to_string(),
    };
    for review in service.catalog.list_reviews(false)? {
        if service
            .catalog
            .review_sources(&review.id)?
            .contains(&source)
        {
            if service.catalog.list_checkpoints(&review.id)?.is_empty() {
                capture_review(service, &review.id)?;
            }
            return Ok(review);
        }
    }
    let repository = service
        .catalog
        .repository(repository_id)?
        .with_context(|| format!("repository {repository_id} does not exist"))?;
    let project = service
        .catalog
        .list_projects(true)?
        .into_iter()
        .find(|project| project.id == repository.project_id)
        .with_context(|| format!("project {} does not exist", repository.project_id))?;
    let context = if project.name.eq_ignore_ascii_case(&repository.name) {
        project.name
    } else {
        format!("{} / {}", project.name, repository.name)
    };
    let title = format!(
        "{context} · {}…{}",
        compact_revision(base),
        compact_revision(head)
    );
    let review = service.create_review(&title)?;
    service.attach_source(&review.id, &source)?;
    capture_review(service, &review.id)
        .with_context(|| format!("failed to prepare range review {title}"))?;
    Ok(review)
}

fn load_review_snapshot(
    service: &WorkdeckService,
    review_set_id: &ReviewSetId,
) -> Result<ReviewSnapshot> {
    let review = service
        .catalog
        .list_reviews(true)?
        .into_iter()
        .find(|review| &review.id == review_set_id)
        .with_context(|| format!("review {review_set_id} does not exist"))?;
    let sources = service.catalog.review_sources(review_set_id)?;
    let checkpoints = service.catalog.list_checkpoints(review_set_id)?;
    let selected_checkpoint = checkpoints
        .iter()
        .max_by_key(|checkpoint| checkpoint.sequence)
        .cloned();
    let evidence = selected_checkpoint
        .as_ref()
        .map(|checkpoint| {
            checkpoint
                .sources
                .iter()
                .map(|source| load_source_evidence(service, source))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let (units, removed) = match &selected_checkpoint {
        Some(checkpoint) => {
            let versions = service
                .catalog
                .review_unit_versions_for_snapshot(&checkpoint.id)?;
            let deltas = service.catalog.review_deltas_for_snapshot(&checkpoint.id)?;
            let effective_marks = service.effective_review_marks(
                &versions
                    .iter()
                    .map(|version| version.id.clone())
                    .collect::<Vec<_>>(),
            )?;
            let units = versions
                .into_iter()
                .map(|version| {
                    let delta = deltas
                        .iter()
                        .find(|delta| delta.unit_id == version.unit_id)
                        .cloned();
                    let review = effective_marks.get(&version.id).cloned();
                    Ok(ReviewUnitSummary {
                        version,
                        delta,
                        review,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let removed = deltas
                .into_iter()
                .filter(|delta| delta.to_version.is_none())
                .collect();
            (units, removed)
        }
        None => (Vec::new(), Vec::new()),
    };
    Ok(ReviewSnapshot {
        attention: review_attention(service, &review)?,
        review,
        sources,
        checkpoints,
        selected_checkpoint,
        evidence,
        units,
        removed,
    })
}

fn load_source_evidence(
    service: &WorkdeckService,
    source: &workdeck_domain::SnapshotSource,
) -> SourceEvidenceSnapshot {
    const MAX_MANIFEST_BYTES: usize = 16 * 1024 * 1024;
    let result = service
        .content
        .get(&source.manifest_hash)
        .and_then(|bytes| {
            if bytes.len() > MAX_MANIFEST_BYTES {
                anyhow::bail!(
                    "captured manifest is {} MiB; the review limit is 16 MiB",
                    bytes.len().div_ceil(1024 * 1024)
                );
            }
            serde_json::from_slice(&bytes).context("captured manifest is not valid JSON")
        });
    match result {
        Ok(manifest) => SourceEvidenceSnapshot {
            source: source.source.clone(),
            revision: source.revision.clone(),
            manifest: Some(manifest),
            error: None,
        },
        Err(error) => SourceEvidenceSnapshot {
            source: source.source.clone(),
            revision: source.revision.clone(),
            manifest: None,
            error: Some(format!("{error:#}")),
        },
    }
}

fn load_unit(
    service: &WorkdeckService,
    version_id: &ReviewUnitVersionId,
) -> Result<ReviewUnitDetail> {
    let version = service
        .catalog
        .review_unit_version(version_id)?
        .with_context(|| format!("review unit version {version_id} does not exist"))?;
    let delta = service.catalog.review_delta_for_version(version_id)?;
    let review = service.effective_review_mark(version_id)?;
    let content =
        String::from_utf8_lossy(&service.content.get(&version.anchor.content_hash)?).into_owned();
    let previous_content = delta
        .as_ref()
        .and_then(|delta| delta.from_version.as_ref())
        .and_then(|previous_id| {
            service
                .catalog
                .review_unit_version(previous_id)
                .ok()
                .flatten()
        })
        .and_then(|previous| service.content.get(&previous.anchor.content_hash).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    let path = version.anchor.path.clone().unwrap_or_default();
    let analysis = analyze(&ReviewDocument {
        repository_id: version.anchor.repository_id.clone(),
        path,
        content: &content,
    })?;
    Ok(ReviewUnitDetail {
        summary: ReviewUnitSummary {
            version,
            delta,
            review,
        },
        content,
        previous_content,
        analysis,
    })
}

fn capture_review(
    service: &mut WorkdeckService,
    review_set_id: &ReviewSetId,
) -> Result<ReviewCheckpoint> {
    capture_review_with_cancellation(service, review_set_id, &GitHubCancellation::default())
}

fn capture_review_with_cancellation(
    service: &mut WorkdeckService,
    review_set_id: &ReviewSetId,
    cancellation: &GitHubCancellation,
) -> Result<ReviewCheckpoint> {
    let review = service
        .catalog
        .list_reviews(true)?
        .into_iter()
        .find(|review| &review.id == review_set_id)
        .with_context(|| format!("review {review_set_id} does not exist"))?;
    ensure_worktree_commit_sources(service, review_set_id)?;
    let mut local = Vec::new();
    let mut ranges = Vec::new();
    let mut markdown = Vec::new();
    let mut pull_requests = Vec::new();
    let mut workflow_runs = Vec::new();
    let mut artifacts = Vec::new();
    for source in service.catalog.review_sources(review_set_id)? {
        match source {
            ReviewSource::LocalWorktree { worktree_id, base } => {
                let worktree = service
                    .catalog
                    .find_worktree(worktree_id.as_str())?
                    .with_context(|| format!("worktree {worktree_id} does not exist"))?;
                local.push((worktree_id, worktree.path, base));
            }
            ReviewSource::CommitRange {
                repository_id,
                base,
                head,
            } => {
                let repository = service
                    .catalog
                    .repository(&repository_id)?
                    .with_context(|| format!("repository {repository_id} does not exist"))?;
                let path = checkout_path_for_repository(&service.catalog, &repository)?;
                ranges.push((repository_id, path, base, head));
            }
            ReviewSource::Markdown {
                repository_id,
                revision,
                path,
            } => {
                let repository = service
                    .catalog
                    .repository(&repository_id)?
                    .with_context(|| format!("repository {repository_id} does not exist"))?;
                let root = checkout_path_for_repository(&service.catalog, &repository)?;
                markdown.push((repository_id, root, revision, path));
            }
            ReviewSource::PullRequest {
                provider,
                repository,
                number,
            } if provider == "github" => pull_requests.push((repository, number)),
            ReviewSource::CiRun {
                provider,
                repository,
                run_id,
            } if provider == "github" => workflow_runs.push((repository, run_id)),
            ReviewSource::Artifact { artifact_id } => artifacts.push(artifact_id),
            _ => {}
        }
    }
    if local.is_empty()
        && ranges.is_empty()
        && markdown.is_empty()
        && pull_requests.is_empty()
        && workflow_runs.is_empty()
        && artifacts.is_empty()
    {
        bail!("attach at least one source before capturing a checkpoint");
    }
    Ok(service
        .capture_sources_checkpoint_with_cancellation(
            &review,
            CheckpointSources {
                local: &local,
                commit_ranges: &ranges,
                markdown: &markdown,
                pull_requests: &pull_requests,
                workflow_runs: &workflow_runs,
                artifacts: &artifacts,
            },
            cancellation,
        )?
        .checkpoint)
}

fn ensure_worktree_commit_sources(
    service: &WorkdeckService,
    review_set_id: &ReviewSetId,
) -> Result<()> {
    let local_worktrees = service
        .catalog
        .review_sources(review_set_id)?
        .into_iter()
        .filter_map(|source| match source {
            ReviewSource::LocalWorktree { worktree_id, .. } => Some(worktree_id),
            _ => None,
        })
        .collect::<Vec<_>>();
    let attention = service.catalog.all_worktree_attention()?;
    for worktree_id in local_worktrees {
        let Some(attention) = attention
            .iter()
            .find(|attention| attention.worktree_id == worktree_id)
            .filter(|attention| attention.commit_count > 0)
        else {
            continue;
        };
        let Some(base) = attention.base_ref.clone() else {
            continue;
        };
        let worktree = service
            .catalog
            .find_worktree(worktree_id.as_str())?
            .with_context(|| format!("worktree {worktree_id} does not exist"))?;
        let head = worktree
            .branch
            .clone()
            .or(worktree.head)
            .with_context(|| format!("worktree {worktree_id} has no reviewable HEAD"))?;
        let repository = service
            .catalog
            .repository_for_worktree(&worktree_id)?
            .with_context(|| format!("worktree {worktree_id} has no repository"))?;
        service.attach_source(
            review_set_id,
            &ReviewSource::CommitRange {
                repository_id: repository.id,
                base,
                head,
            },
        )?;
    }
    Ok(())
}

pub fn source_label(source: &ReviewSource) -> String {
    match source {
        ReviewSource::LocalWorktree { .. } => "Worktree".into(),
        ReviewSource::CommitRange { base, head, .. } => format!("{base}..{head}"),
        ReviewSource::PullRequest {
            repository, number, ..
        } => format!("{repository}#{number}"),
        ReviewSource::Markdown { path, .. } => path.display().to_string(),
        ReviewSource::CiRun {
            repository, run_id, ..
        } => format!("{repository} · {run_id}"),
        ReviewSource::Artifact { artifact_id } => artifact_id.to_string(),
    }
}

pub fn shortened_title(title: &str) -> &str {
    const PREFIXES: &[&str] = &[
        "function_item ",
        "struct_item ",
        "impl_item ",
        "enum_item ",
        "trait_item ",
        "mod_item ",
        "function_definition ",
        "method_definition ",
        "class_definition ",
        "function_declaration ",
        "method_declaration ",
        "class_declaration ",
        "interface_declaration ",
        "trait_declaration ",
        "enum_declaration ",
        "type_declaration ",
    ];
    PREFIXES
        .iter()
        .find_map(|prefix| title.strip_prefix(prefix))
        .unwrap_or(title)
}

pub fn compact_revision(value: &str) -> String {
    if value.chars().all(|character| character.is_ascii_hexdigit()) && value.len() > 10 {
        value[..10].to_string()
    } else if value.chars().count() > 24 {
        format!("{}…", value.chars().take(23).collect::<String>())
    } else {
        value.to_string()
    }
}

pub fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_logs_are_bounded_and_terminal_controls_are_never_rendered() {
        let log = provider_job_log(
            "owner/repository".into(),
            42,
            "quality".into(),
            b"\x1b[31mfailed\x1b[0m\r\n\x1b]8;;https://secret.test\x07link\x1b]8;;\x07\n",
        );
        assert_eq!(log.text, "failed\nlink\n");
        assert!(!log.text.contains('\u{1b}'));
        assert!(!log.truncated);
        assert_eq!(log.original_bytes, 53);
    }

    #[test]
    fn runtime_round_trips_workspace_mutations() {
        let root = tempfile::tempdir().unwrap();
        let runtime = RuntimeHandle::spawn(ApplicationPaths::at(root.path())).unwrap();
        runtime.create_project("Workdeck").recv().unwrap().unwrap();
        runtime
            .create_review("Agent burst")
            .recv()
            .unwrap()
            .unwrap();
        let snapshot = runtime.load_workspace().recv().unwrap().unwrap();
        assert_eq!(snapshot.projects.len(), 1);
        assert_eq!(snapshot.review_sets.len(), 1);
        assert_eq!(snapshot.open_units(), 0);
        let cached = runtime
            .load_cached_workspace()
            .recv()
            .unwrap()
            .unwrap()
            .expect("workspace cache");
        assert_eq!(cached.projects, snapshot.projects);
        assert_eq!(cached.review_sets.len(), snapshot.review_sets.len());
        assert_eq!(
            cached.review_sets[0].review.id,
            snapshot.review_sets[0].review.id
        );
    }

    #[test]
    fn ui_preferences_round_trip_without_repository_state() {
        let root = tempfile::tempdir().unwrap();
        let paths = ApplicationPaths::at(root.path());
        paths.ensure().unwrap();
        let preferences = UiPreferences {
            sidebar_collapsed: true,
            destination: "git".into(),
            diff_layout: "split".into(),
            saved_searches: vec!["sampleapp failing CI".into()],
            inbox_view: "waiting".into(),
            ..UiPreferences::default()
        };
        write_ui_preferences(&paths, &preferences).unwrap();
        let restored = read_ui_preferences(&paths).unwrap();
        assert!(restored.sidebar_collapsed);
        assert_eq!(restored.destination, "git");
        assert_eq!(restored.diff_layout, "split");
        assert_eq!(restored.saved_searches, ["sampleapp failing CI"]);
        assert_eq!(restored.inbox_view, "waiting");
        assert!(root.path().join("ui-preferences.json").is_file());
    }

    #[test]
    fn ui_preference_migration_keeps_one_versioned_backup_and_writes_atomically() {
        let root = tempfile::tempdir().unwrap();
        let paths = ApplicationPaths::at(root.path());
        paths.ensure().unwrap();
        let legacy = serde_json::json!({
            "ui_schema_version": 1,
            "destination": "review",
            "workspace_tab": "artifacts"
        });
        let legacy_bytes = serde_json::to_vec_pretty(&legacy).unwrap();
        std::fs::write(paths.root.join("ui-preferences.json"), &legacy_bytes).unwrap();

        let preferences = UiPreferences {
            ui_schema_version: 7,
            destination: "inbox".into(),
            workspace_tab: "changes".into(),
            ..UiPreferences::default()
        };
        write_ui_preferences(&paths, &preferences).unwrap();
        write_ui_preferences(&paths, &preferences).unwrap();

        let backup = paths.root.join("ui-preferences.v1.backup.json");
        assert_eq!(std::fs::read(&backup).unwrap(), legacy_bytes);
        assert_eq!(read_ui_preferences(&paths).unwrap().ui_schema_version, 7);
        assert!(std::fs::read_dir(&paths.root).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains(".tmp-")
        }));
    }

    #[test]
    fn appearance_values_are_normalized_and_strict() {
        assert_eq!(normalize_appearance(" System ").unwrap(), "system");
        assert_eq!(normalize_appearance("LIGHT").unwrap(), "light");
        assert_eq!(normalize_appearance("dark").unwrap(), "dark");
        assert!(normalize_appearance("sepia").is_err());
    }

    #[test]
    fn startup_telemetry_tokens_cannot_escape_the_log_directory() {
        assert_eq!(
            normalize_telemetry_token("qa_run-1", "marker").unwrap(),
            "qa_run-1"
        );
        assert!(normalize_telemetry_token("../escape", "marker").is_err());
        assert!(normalize_telemetry_token("contains space", "phase").is_err());
        assert!(normalize_telemetry_token("", "phase").is_err());
    }

    #[test]
    fn global_inbox_surfaces_dirty_worktrees_without_duplicate_reviews() {
        let now = Utc::now();
        let project = WorkspaceProject::new("SampleApp");
        let repository = RepositoryRecord {
            id: workdeck_domain::RepositoryId::new(),
            project_id: project.id.clone(),
            name: "sampleapp".into(),
            provider: None,
            provider_owner: None,
            provider_name: None,
            normalized_remotes: Vec::new(),
            created_at: now,
            updated_at: now,
        };
        let checkout = CheckoutRecord {
            id: workdeck_domain::CheckoutId::new(),
            repository_id: repository.id.clone(),
            path: "/tmp/sampleapp".into(),
            git_common_dir: "/tmp/sampleapp/.git".into(),
            available: true,
            last_seen_at: now,
        };
        let attached_worktree = WorktreeRecord {
            id: workdeck_domain::WorktreeId::new(),
            checkout_id: checkout.id.clone(),
            path: "/tmp/sampleapp".into(),
            head: None,
            branch: Some("main".into()),
            locked: false,
            prunable: false,
            available: true,
            last_seen_at: now,
        };
        let uncaptured_worktree = WorktreeRecord {
            id: workdeck_domain::WorktreeId::new(),
            path: "/tmp/sampleapp-agent".into(),
            branch: Some("feat/agent".into()),
            ..attached_worktree.clone()
        };
        let review = ReviewSet::new("SampleApp");
        let mut workspace = WorkspaceSnapshot {
            projects: vec![project],
            repositories: vec![repository],
            checkouts: vec![checkout],
            worktrees: vec![attached_worktree.clone(), uncaptured_worktree.clone()],
            worktree_attention: vec![
                WorktreeAttention {
                    worktree_id: attached_worktree.id.clone(),
                    change_count: 3,
                    commit_count: 0,
                    base_ref: Some("origin/main".into()),
                    fingerprint: "attached-v1".into(),
                    truncated: false,
                    error: None,
                    scanned_at: now,
                    duration_ms: 1,
                },
                WorktreeAttention {
                    worktree_id: uncaptured_worktree.id.clone(),
                    change_count: 76,
                    commit_count: 5,
                    base_ref: Some("origin/main".into()),
                    fingerprint: "uncaptured-v1".into(),
                    truncated: false,
                    error: None,
                    scanned_at: now,
                    duration_ms: 2,
                },
            ],
            inbox_preferences: Vec::new(),
            activity_read_cursors: Vec::new(),
            review_sets: vec![ReviewAttention {
                sources: vec![ReviewSource::LocalWorktree {
                    worktree_id: attached_worktree.id,
                    base: None,
                }],
                review,
                checkpoint_id: None,
                revision: 0,
                total: 0,
                open: 0,
                reviewed: 0,
                questioned: 0,
                inherited: 0,
                changed: 0,
                uncertain: 0,
                carried: 0,
                next_open_title: None,
                next_open_path: None,
                source_advances: Vec::new(),
            }],
            search_units: Vec::new(),
            artifacts: Vec::new(),
            loaded_at: now,
        };

        let inbox = workspace.worktree_inbox();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].worktree.id, uncaptured_worktree.id);
        assert_eq!(inbox[0].attention.change_count, 76);
        assert_eq!(workspace.review_queue_items(), 2);

        let mut legacy_attention = serde_json::to_value(&workspace.review_sets[0]).unwrap();
        let legacy_object = legacy_attention.as_object_mut().unwrap();
        legacy_object.remove("questioned");
        legacy_object.remove("uncertain");
        legacy_object.remove("next_open_title");
        legacy_object.remove("next_open_path");
        let restored: ReviewAttention = serde_json::from_value(legacy_attention).unwrap();
        assert_eq!(restored.questioned, 0);
        assert_eq!(restored.uncertain, 0);
        assert!(restored.next_open_title.is_none());
        assert!(restored.next_open_path.is_none());

        workspace.inbox_preferences.push(InboxPreference {
            worktree_id: uncaptured_worktree.id.clone(),
            disposition: InboxDisposition::Baseline,
            snoozed_until: None,
            baseline_signature: Some("uncaptured-v1".into()),
            updated_at: now,
        });
        assert!(workspace.worktree_inbox().is_empty());
        workspace
            .worktree_attention
            .iter_mut()
            .find(|attention| attention.worktree_id == uncaptured_worktree.id)
            .unwrap()
            .fingerprint = "uncaptured-v2".into();
        let reopened = workspace.worktree_inbox();
        assert_eq!(reopened.len(), 1);
        assert!(
            reopened[0]
                .attention_reasons()
                .contains(&InboxAttentionReason::ChangedAfterBaseline)
        );

        workspace.inbox_preferences[0] = InboxPreference {
            worktree_id: uncaptured_worktree.id.clone(),
            disposition: InboxDisposition::Snoozed,
            snoozed_until: Some(now + ChronoDuration::days(1)),
            baseline_signature: None,
            updated_at: now,
        };
        assert!(workspace.worktree_inbox().is_empty());
        assert_eq!(workspace.snoozed_worktree_inbox().len(), 1);

        workspace.inbox_preferences[0] = InboxPreference {
            worktree_id: uncaptured_worktree.id.clone(),
            disposition: InboxDisposition::Pinned,
            snoozed_until: None,
            baseline_signature: None,
            updated_at: now,
        };
        let pinned = workspace.worktree_inbox();
        assert_eq!(pinned.len(), 1);
        assert!(
            pinned[0]
                .attention_reasons()
                .contains(&InboxAttentionReason::Pinned)
        );

        let mut smaller = pinned[0].clone();
        smaller.preference = None;
        smaller.attention.change_count = 18;
        smaller.attention.commit_count = 3;
        let mut oversized = smaller.clone();
        oversized.attention.change_count = 15_817;
        oversized.attention.commit_count = 30;
        oversized.attention.scanned_at = now + ChronoDuration::minutes(1);
        assert!(
            inbox_priority(&smaller) > inbox_priority(&oversized),
            "a recently scanned giant repository must not outrank a bounded next review"
        );

        let mut failed = smaller;
        failed.attention.error = Some("worktree unavailable".into());
        assert!(
            failed
                .attention_reasons()
                .contains(&InboxAttentionReason::ScanFailed {
                    retained_history: true,
                })
        );
    }

    #[test]
    fn preparing_a_worktree_review_is_lazy_and_idempotent() {
        let root = tempfile::tempdir().expect("temporary catalog");
        let repository_path = root.path().join("sampleapp");
        std::fs::create_dir(&repository_path).expect("repository directory");
        run_git(&repository_path, &["init"]);
        run_git(
            &repository_path,
            &["config", "user.email", "workdeck@example.test"],
        );
        run_git(&repository_path, &["config", "user.name", "Workdeck Test"]);
        std::fs::write(repository_path.join("README.md"), "initial\n").expect("initial file");
        run_git(&repository_path, &["add", "README.md"]);
        run_git(&repository_path, &["commit", "-m", "initial"]);
        run_git(
            &repository_path,
            &["update-ref", "refs/remotes/origin/main", "HEAD"],
        );
        run_git(&repository_path, &["checkout", "-b", "feat/review"]);
        std::fs::write(repository_path.join("README.md"), "initial\nagent change\n")
            .expect("changed file");
        run_git(&repository_path, &["commit", "-am", "agent change"]);

        let paths = ApplicationPaths::at(root.path().join("catalog"));
        let mut service = WorkdeckService::open(paths).expect("service");
        let project = service.create_project("SampleApp").expect("project");
        service
            .add_repository(&project, &repository_path)
            .expect("repository");
        let worktree = service
            .catalog
            .all_worktrees()
            .expect("worktrees")
            .into_iter()
            .next()
            .expect("worktree");
        service
            .catalog
            .save_worktree_attention(&WorktreeAttention {
                worktree_id: worktree.id.clone(),
                change_count: 0,
                commit_count: 1,
                base_ref: Some("origin/main".into()),
                fingerprint: "commit-range-v1".into(),
                truncated: false,
                error: None,
                scanned_at: Utc::now(),
                duration_ms: 1,
            })
            .expect("attention");
        assert_eq!(load_workspace(&service).unwrap().worktree_inbox().len(), 1);

        let first = prepare_worktree_review(&mut service, &worktree.id).expect("first review");
        let second = prepare_worktree_review(&mut service, &worktree.id).expect("second review");

        assert_eq!(first.id, second.id);
        assert_eq!(first.title, "SampleApp · feat/review");
        assert_eq!(service.catalog.list_reviews(false).unwrap().len(), 1);
        assert_eq!(service.catalog.review_sources(&first.id).unwrap().len(), 2);
        assert_eq!(
            service.catalog.list_checkpoints(&first.id).unwrap().len(),
            1
        );
        let indexed_workspace = load_workspace(&service).unwrap();
        assert!(indexed_workspace.worktree_inbox().is_empty());
        let attention = indexed_workspace
            .review_sets
            .iter()
            .find(|attention| attention.review.id == first.id)
            .expect("prepared review attention");
        assert_eq!(attention.revision, 1);
        assert_eq!(attention.next_open_path.as_deref(), Some("README.md"));
        assert!(attention.next_open_title.is_some());
        assert!(
            indexed_workspace
                .search_units
                .iter()
                .any(|unit| unit.review_set_id == first.id && unit.path == "README.md"),
            "latest frozen review units must be available to global search"
        );

        let repository = service
            .catalog
            .repository_for_worktree(&worktree.id)
            .unwrap()
            .unwrap();
        let range_first =
            prepare_commit_range_review(&mut service, &repository.id, "origin/main", "HEAD")
                .expect("range review");
        let range_second =
            prepare_commit_range_review(&mut service, &repository.id, "origin/main", "HEAD")
                .expect("same range review");
        assert_eq!(range_first.id, range_second.id);
        assert_eq!(service.catalog.list_reviews(false).unwrap().len(), 2);
        assert_eq!(
            service
                .catalog
                .list_checkpoints(&range_first.id)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn deleted_linked_worktree_is_observed_unavailable_without_catalog_mutation() {
        let root = tempfile::tempdir().expect("temporary Git fixture");
        let repository = root.path().join("repository");
        let linked = root.path().join("linked-agent-worktree");
        std::fs::create_dir(&repository).expect("repository directory");
        run_git(&repository, &["init"]);
        run_git(
            &repository,
            &["config", "user.email", "workdeck@example.test"],
        );
        run_git(&repository, &["config", "user.name", "Workdeck Test"]);
        std::fs::write(repository.join("README.md"), "fixture\n").expect("fixture file");
        run_git(&repository, &["add", "README.md"]);
        run_git(&repository, &["commit", "-m", "fixture"]);
        run_git(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                "feat/deleted-agent",
                linked.to_str().expect("UTF-8 fixture path"),
            ],
        );

        std::fs::remove_dir_all(&linked).expect("remove linked worktree fixture");
        let worktree = WorktreeRecord {
            id: workdeck_domain::WorktreeId::new(),
            checkout_id: workdeck_domain::CheckoutId::new(),
            path: linked,
            head: None,
            branch: Some("feat/deleted-agent".into()),
            locked: false,
            prunable: false,
            available: true,
            last_seen_at: Utc::now(),
        };

        let scan = workdeck_git::start_attention_scan(
            vec![worktree.clone()],
            AttentionScanOptions::default(),
        );
        let attention = scan
            .receiver
            .iter()
            .find_map(|event| match event {
                workdeck_git::AttentionScanEvent::Updated(attention) => Some(attention),
                _ => None,
            })
            .expect("bounded scan result");

        assert!(
            worktree.available,
            "persisted catalog record stays unchanged"
        );
        assert_eq!(
            classify_worktree_availability(&worktree, Some(&attention)),
            WorktreeAvailability::ObservedUnavailable
        );
        assert!(attention.error.is_some());
    }

    fn run_git(repository: &Path, arguments: &[&str]) {
        let status = std::process::Command::new("git")
            .args(arguments)
            .current_dir(repository)
            .status()
            .expect("git command");
        assert!(status.success(), "git {arguments:?}");
    }

    #[test]
    fn title_and_revision_formatting_stays_compact() {
        assert_eq!(shortened_title("function_item capture"), "capture");
        assert_eq!(
            shortened_title("method_declaration Opportunity::execute"),
            "Opportunity::execute"
        );
        assert_eq!(compact_revision("0123456789abcdef"), "0123456789");
    }

    #[test]
    fn corrupt_source_evidence_isolated_from_the_rest_of_a_review() {
        let root = tempfile::tempdir().unwrap();
        let service = WorkdeckService::open(ApplicationPaths::at(root.path())).unwrap();
        let source = workdeck_domain::SnapshotSource {
            source: ReviewSource::CiRun {
                provider: "github".into(),
                repository: "example/workdeck".into(),
                run_id: "42".into(),
            },
            revision: "github-run:head:manifest".into(),
            manifest_hash: "0".repeat(64),
        };

        let evidence = load_source_evidence(&service, &source);

        assert!(evidence.manifest.is_none());
        assert!(
            evidence
                .error
                .as_deref()
                .is_some_and(|error| { error.contains("failed to read object") })
        );
        assert_eq!(evidence.source, source.source);
    }

    #[test]
    fn disconnected_runtime_replies_once() {
        let (sender, receiver) = mpsc::channel();
        drop(receiver);
        let runtime = RuntimeHandle { sender };
        let reply = runtime.load_workspace();

        assert!(reply.recv().unwrap().is_err());
        assert!(matches!(
            reply.try_recv(),
            Err(mpsc::TryRecvError::Disconnected)
        ));
    }
}
