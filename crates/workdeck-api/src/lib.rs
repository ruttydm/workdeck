//! Renderer-independent protocol shared by Workdeck's local runtime, fixture web
//! preview, and future remote transports.
//!
//! This crate deliberately has no filesystem, database, Git, provider, or UI
//! dependencies. The renderer receives immutable snapshots and sends typed
//! intents; repositories remain behind the native runtime boundary.

use async_channel::Receiver;
use chrono::{DateTime, Utc};
use futures::future::{BoxFuture, FutureExt, Shared};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};
use thiserror::Error;
use uuid::Uuid;
use web_time::{Duration, Instant};

macro_rules! string_id {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7().to_string())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_owned())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str(&self.0)
            }
        }
    };
}

string_id!(RequestId);
string_id!(OperationId);
string_id!(ProjectId);
string_id!(RepositoryId);
string_id!(CheckoutId);
string_id!(WorktreeId);
string_id!(ReviewId);
string_id!(ReviewUnitId);
string_id!(ArtifactId);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(pub u64);

impl Revision {
    pub fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Envelope<T> {
    pub request_id: RequestId,
    pub revision: Revision,
    pub payload: T,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkdeckRequest {
    Bootstrap {
        request_id: RequestId,
    },
    LoadInbox {
        request_id: RequestId,
    },
    MarkActivityRead {
        request_id: RequestId,
        target: ActivityTarget,
        revision: String,
    },
    ScanInbox {
        request_id: RequestId,
        operation_id: OperationId,
    },
    LoadPortfolio {
        request_id: RequestId,
    },
    DiscoverPortfolio {
        request_id: RequestId,
        roots: Vec<String>,
    },
    ChoosePortfolioRoot {
        request_id: RequestId,
    },
    Search {
        request_id: RequestId,
        query: String,
    },
    LoadGitGraph {
        request_id: RequestId,
        worktree_id: Option<WorktreeId>,
        cursor: Option<String>,
    },
    LoadReview {
        request_id: RequestId,
        review_id: ReviewId,
    },
    PrepareWorktreeReview {
        request_id: RequestId,
        worktree_id: WorktreeId,
    },
    PrepareCommitRangeReview {
        request_id: RequestId,
        worktree_id: WorktreeId,
        base: String,
        head: String,
    },
    PreparePullRequestReview {
        request_id: RequestId,
        repository: String,
        number: u64,
        title: String,
    },
    LoadPullRequests {
        request_id: RequestId,
        repository: Option<String>,
    },
    LoadCiRuns {
        request_id: RequestId,
        repository: Option<String>,
    },
    LoadCiJobLog {
        request_id: RequestId,
        repository: String,
        job_id: u64,
        job_name: String,
    },
    LoadArtifacts {
        request_id: RequestId,
    },
    ChooseArtifactImport {
        request_id: RequestId,
    },
    OpenArtifact {
        request_id: RequestId,
        artifact_id: ArtifactId,
    },
    CloseArtifact {
        request_id: RequestId,
        session_id: OperationId,
    },
    UpdateReviewMark {
        request_id: RequestId,
        review_id: ReviewId,
        unit_id: ReviewUnitId,
        state: ReviewMarkState,
    },
    CreateCheckpoint {
        request_id: RequestId,
        review_id: ReviewId,
    },
    UpdatePreferences {
        request_id: RequestId,
        patch: UiPreferencePatch,
    },
}

impl WorkdeckRequest {
    pub fn request_id(&self) -> &RequestId {
        match self {
            Self::Bootstrap { request_id }
            | Self::LoadInbox { request_id }
            | Self::MarkActivityRead { request_id, .. }
            | Self::ScanInbox { request_id, .. }
            | Self::LoadPortfolio { request_id }
            | Self::DiscoverPortfolio { request_id, .. }
            | Self::ChoosePortfolioRoot { request_id }
            | Self::Search { request_id, .. }
            | Self::LoadGitGraph { request_id, .. }
            | Self::LoadReview { request_id, .. }
            | Self::PrepareWorktreeReview { request_id, .. }
            | Self::PrepareCommitRangeReview { request_id, .. }
            | Self::PreparePullRequestReview { request_id, .. }
            | Self::LoadPullRequests { request_id, .. }
            | Self::LoadCiRuns { request_id, .. }
            | Self::LoadCiJobLog { request_id, .. }
            | Self::LoadArtifacts { request_id }
            | Self::ChooseArtifactImport { request_id }
            | Self::OpenArtifact { request_id, .. }
            | Self::CloseArtifact { request_id, .. }
            | Self::UpdateReviewMark { request_id, .. }
            | Self::CreateCheckpoint { request_id, .. }
            | Self::UpdatePreferences { request_id, .. } => request_id,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum WorkdeckResponse {
    Bootstrap(BootstrapSnapshot),
    Inbox(InboxSnapshot),
    ActivityRead(ActivityReadState),
    Portfolio(PortfolioSnapshot),
    Search(SearchSnapshot),
    GitGraph(GitGraph),
    Review(ReviewSet),
    PullRequests(Vec<PullRequest>),
    CiRuns(Vec<CiRun>),
    CiJobLog(CiJobLog),
    Artifacts(Vec<ArtifactRecord>),
    ArtifactPreview(ArtifactPreviewSession),
    ReviewMark(ReviewMark),
    Checkpoint(ReviewCheckpoint),
    Preferences(UiPreferences),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkdeckEvent {
    SnapshotUpdated {
        revision: Revision,
    },
    TaskProgress(TaskProgress),
    IncomingBurst {
        review_id: ReviewId,
        burst: IncomingBurst,
    },
    ProviderState {
        provider: String,
        state: ProviderState,
    },
    OperationCancelled {
        operation_id: OperationId,
    },
    Error {
        operation_id: Option<OperationId>,
        error: WorkdeckError,
    },
}

pub type WorkdeckEventStream = Receiver<WorkdeckEvent>;

#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message", rename_all = "snake_case")]
pub enum WorkdeckError {
    #[error("request was cancelled")]
    Cancelled,
    #[error("repository is unavailable: {0}")]
    RepositoryUnavailable(String),
    #[error("provider is unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    #[error("integrity error: {0}")]
    Integrity(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub trait WorkdeckTransport: Send + Sync + 'static {
    fn request(
        &self,
        request: WorkdeckRequest,
    ) -> BoxFuture<'static, Result<Envelope<WorkdeckResponse>, WorkdeckError>>;
    fn subscribe(&self) -> WorkdeckEventStream;
    fn cancel(&self, operation: OperationId);
}

/// Cloneable renderer handle. The inner transport can be native, fixture, or
/// remote without changing any UI component.
#[derive(Clone)]
pub struct WorkdeckClient {
    inner: Arc<dyn WorkdeckTransport>,
    cache: Arc<Mutex<ResponseCache>>,
}

impl PartialEq for WorkdeckClient {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl WorkdeckClient {
    pub fn new(transport: impl WorkdeckTransport) -> Self {
        Self {
            inner: Arc::new(transport),
            cache: Arc::new(Mutex::new(ResponseCache::default())),
        }
    }

    pub fn request(
        &self,
        request: WorkdeckRequest,
    ) -> BoxFuture<'static, Result<Envelope<WorkdeckResponse>, WorkdeckError>> {
        self.invalidate_for_mutation(&request);
        self.inner.request(request)
    }

    /// Coalesces identical in-flight reads and keeps their immutable response
    /// hot for a bounded time. Cached envelopes are always rebound to the
    /// caller's request ID, preserving request/revision safety in renderers.
    pub fn request_cached(
        &self,
        request: WorkdeckRequest,
        max_age: Duration,
    ) -> BoxFuture<'static, Result<Envelope<WorkdeckResponse>, WorkdeckError>> {
        let Some(key) = RequestCacheKey::from_request(&request) else {
            return self.request(request);
        };
        let request_id = request.request_id().clone();
        let shared = {
            let mut cache = match self.cache.lock() {
                Ok(cache) => cache,
                Err(_) => {
                    return Box::pin(async {
                        Err(WorkdeckError::Integrity(
                            "response cache lock was poisoned".into(),
                        ))
                    });
                }
            };
            if let Some(entry) = cache.responses.get(&key)
                && entry.stored_at.elapsed() <= max_age
            {
                let envelope = Envelope {
                    request_id,
                    revision: entry.revision,
                    payload: entry.payload.clone(),
                };
                return Box::pin(async move { Ok(envelope) });
            }
            if let Some(inflight) = cache.inflight.get(&key) {
                inflight.future.clone()
            } else {
                let epoch = cache.next_epoch();
                let sequence = cache.next_sequence();
                let transport = Arc::clone(&self.inner);
                let response_cache = Arc::clone(&self.cache);
                let response_key = key.clone();
                let future = async move {
                    let result = transport.request(request).await;
                    if let Ok(mut cache) = response_cache.lock() {
                        let is_current = cache
                            .inflight
                            .get(&response_key)
                            .is_some_and(|inflight| inflight.epoch == epoch);
                        if is_current && let Ok(envelope) = &result {
                            cache.insert(
                                response_key.clone(),
                                envelope.revision,
                                envelope.payload.clone(),
                            );
                        }
                        if is_current {
                            cache.inflight.remove(&response_key);
                        }
                    }
                    result
                }
                .boxed()
                .shared();
                cache.make_inflight_room();
                cache.inflight.insert(
                    key,
                    InflightResponse {
                        epoch,
                        sequence,
                        future: future.clone(),
                    },
                );
                future
            }
        };
        Box::pin(async move {
            shared.await.map(|envelope| Envelope {
                request_id,
                revision: envelope.revision,
                payload: envelope.payload,
            })
        })
    }

    /// Invalidates one cacheable request identity. Any older in-flight result
    /// is generation-rejected and cannot overwrite the subsequent refresh.
    pub fn invalidate_cached(&self, request: &WorkdeckRequest) {
        let Some(key) = RequestCacheKey::from_request(request) else {
            return;
        };
        if let Ok(mut cache) = self.cache.lock() {
            cache.invalidate(&key);
        }
    }

    /// Returns the most recently cached response, regardless of age. Surfaces
    /// use this only as stale-while-revalidate content while a normal
    /// `request_cached` call refreshes it in the background.
    pub fn cached_response(&self, request: &WorkdeckRequest) -> Option<Envelope<WorkdeckResponse>> {
        let key = RequestCacheKey::from_request(request)?;
        let request_id = request.request_id().clone();
        let cache = self.cache.lock().ok()?;
        let response = cache.responses.get(&key)?;
        Some(Envelope {
            request_id,
            revision: response.revision,
            payload: response.payload.clone(),
        })
    }

    pub fn clear_cached(&self) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.clear();
        }
    }

    pub fn subscribe(&self) -> WorkdeckEventStream {
        self.inner.subscribe()
    }

    pub fn cancel(&self, operation: OperationId) {
        self.inner.cancel(operation);
    }

    fn invalidate_for_mutation(&self, request: &WorkdeckRequest) {
        let mut cache = match self.cache.lock() {
            Ok(cache) => cache,
            Err(_) => return,
        };
        match request {
            WorkdeckRequest::MarkActivityRead { .. } => {
                cache.invalidate_matching(RequestCacheKey::is_pull_request_list);
            }
            WorkdeckRequest::UpdateReviewMark { .. } | WorkdeckRequest::CreateCheckpoint { .. } => {
                cache.invalidate_matching(RequestCacheKey::is_review);
            }
            WorkdeckRequest::DiscoverPortfolio { .. }
            | WorkdeckRequest::ChoosePortfolioRoot { .. }
            | WorkdeckRequest::ScanInbox { .. } => cache.clear(),
            WorkdeckRequest::ChooseArtifactImport { .. } => {
                cache.invalidate_matching(RequestCacheKey::is_artifacts);
            }
            _ => {}
        }
    }
}

const MAX_RESPONSE_CACHE_ENTRIES: usize = 96;
const MAX_INFLIGHT_CACHE_ENTRIES: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RequestCacheKey {
    Search(String),
    GitGraph(Option<WorktreeId>, Option<String>),
    Review(ReviewId),
    WorktreeReview(WorktreeId),
    CommitRangeReview(WorktreeId, String, String),
    PullRequestReview(String, u64),
    PullRequests(Option<String>),
    CiRuns(Option<String>),
    CiJobLog(String, u64),
    Artifacts,
}

impl RequestCacheKey {
    fn from_request(request: &WorkdeckRequest) -> Option<Self> {
        match request {
            WorkdeckRequest::Search { query, .. } => Some(Self::Search(query.trim().to_owned())),
            WorkdeckRequest::LoadGitGraph {
                worktree_id,
                cursor,
                ..
            } => Some(Self::GitGraph(worktree_id.clone(), cursor.clone())),
            WorkdeckRequest::LoadReview { review_id, .. } => Some(Self::Review(review_id.clone())),
            WorkdeckRequest::PrepareWorktreeReview { worktree_id, .. } => {
                Some(Self::WorktreeReview(worktree_id.clone()))
            }
            WorkdeckRequest::PrepareCommitRangeReview {
                worktree_id,
                base,
                head,
                ..
            } => Some(Self::CommitRangeReview(
                worktree_id.clone(),
                base.clone(),
                head.clone(),
            )),
            WorkdeckRequest::PreparePullRequestReview {
                repository, number, ..
            } => Some(Self::PullRequestReview(repository.clone(), *number)),
            WorkdeckRequest::LoadPullRequests { repository, .. } => {
                Some(Self::PullRequests(repository.clone()))
            }
            WorkdeckRequest::LoadCiRuns { repository, .. } => {
                Some(Self::CiRuns(repository.clone()))
            }
            WorkdeckRequest::LoadCiJobLog {
                repository, job_id, ..
            } => Some(Self::CiJobLog(repository.clone(), *job_id)),
            WorkdeckRequest::LoadArtifacts { .. } => Some(Self::Artifacts),
            _ => None,
        }
    }

    fn is_pull_request_list(&self) -> bool {
        matches!(self, Self::PullRequests(_))
    }

    fn is_review(&self) -> bool {
        matches!(
            self,
            Self::Review(_)
                | Self::WorktreeReview(_)
                | Self::CommitRangeReview(_, _, _)
                | Self::PullRequestReview(_, _)
        )
    }

    fn is_artifacts(&self) -> bool {
        matches!(self, Self::Artifacts)
    }
}

#[derive(Clone)]
struct CachedResponse {
    revision: Revision,
    payload: WorkdeckResponse,
    stored_at: Instant,
    sequence: u64,
}

#[derive(Clone)]
struct InflightResponse {
    epoch: u64,
    sequence: u64,
    future: Shared<BoxFuture<'static, Result<Envelope<WorkdeckResponse>, WorkdeckError>>>,
}

#[derive(Default)]
struct ResponseCache {
    responses: BTreeMap<RequestCacheKey, CachedResponse>,
    inflight: BTreeMap<RequestCacheKey, InflightResponse>,
    sequence: u64,
    epoch: u64,
}

impl ResponseCache {
    fn next_sequence(&mut self) -> u64 {
        self.sequence = self.sequence.saturating_add(1);
        self.sequence
    }

    fn next_epoch(&mut self) -> u64 {
        self.epoch = self.epoch.saturating_add(1);
        self.epoch
    }

    fn insert(&mut self, key: RequestCacheKey, revision: Revision, payload: WorkdeckResponse) {
        let sequence = self.next_sequence();
        self.responses.insert(
            key,
            CachedResponse {
                revision,
                payload,
                stored_at: Instant::now(),
                sequence,
            },
        );
        while self.responses.len() > MAX_RESPONSE_CACHE_ENTRIES {
            let Some(oldest) = self
                .responses
                .iter()
                .min_by_key(|(_, response)| response.sequence)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.responses.remove(&oldest);
        }
    }

    fn invalidate(&mut self, key: &RequestCacheKey) {
        self.responses.remove(key);
        self.inflight.remove(key);
    }

    fn make_inflight_room(&mut self) {
        while self.inflight.len() >= MAX_INFLIGHT_CACHE_ENTRIES {
            let Some(oldest) = self
                .inflight
                .iter()
                .min_by_key(|(_, response)| response.sequence)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.inflight.remove(&oldest);
        }
    }

    fn invalidate_matching(&mut self, predicate: impl Fn(&RequestCacheKey) -> bool) {
        let keys = self
            .responses
            .keys()
            .chain(self.inflight.keys())
            .filter(|key| predicate(key))
            .cloned()
            .collect::<BTreeSet<_>>();
        for key in keys {
            self.invalidate(&key);
        }
    }

    fn clear(&mut self) {
        let keys = self
            .responses
            .keys()
            .chain(self.inflight.keys())
            .cloned()
            .collect::<BTreeSet<_>>();
        for key in keys {
            self.invalidate(&key);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BootstrapSnapshot {
    pub portfolio: PortfolioSnapshot,
    pub suggested_roots: Vec<String>,
    pub inbox: InboxSnapshot,
    pub reviews: Vec<ReviewSummary>,
    pub pull_requests: Vec<PullRequest>,
    pub ci_runs: Vec<CiRun>,
    pub artifacts: Vec<ArtifactRecord>,
    pub preferences: UiPreferences,
    pub provider_state: ProviderState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortfolioSnapshot {
    pub projects: Vec<ProjectNode>,
    pub project_count: usize,
    pub repository_count: usize,
    pub worktree_count: usize,
    pub unavailable_count: usize,
    pub scanned_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProjectNode {
    pub id: ProjectId,
    pub name: String,
    pub repositories: Vec<RepositoryNode>,
    pub attention: usize,
    pub empty: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RepositoryNode {
    pub id: RepositoryId,
    pub name: String,
    pub provider: Option<String>,
    pub checkouts: Vec<CheckoutNode>,
    pub attention: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckoutNode {
    pub id: CheckoutId,
    pub label: String,
    pub worktrees: Vec<WorktreeNode>,
    pub available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorktreeNode {
    pub id: WorktreeId,
    pub label: String,
    pub branch: Option<String>,
    pub path_hint: String,
    pub availability: Availability,
    pub changes: usize,
    pub commits: usize,
    pub last_seen: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Unavailable,
    Prunable,
    ScanWarning,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxSnapshot {
    pub items: Vec<InboxItem>,
    pub unread: usize,
    pub commit_updates: usize,
    pub pull_request_updates: usize,
    pub scan: Option<TaskProgress>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InboxItem {
    pub id: String,
    pub target: ActivityTarget,
    pub kind: ActivityKind,
    pub revision: String,
    pub unread: bool,
    pub project: String,
    pub repository: String,
    pub title: String,
    pub branch: String,
    pub summary: String,
    pub changes: usize,
    pub commits: usize,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActivityTarget {
    CommitBranch { worktree_id: WorktreeId },
    PullRequest { repository: String, number: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    CommitBranch,
    PullRequest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityReadState {
    pub target: ActivityTarget,
    pub revision: String,
    pub unread: bool,
    pub read_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewSummary {
    pub id: ReviewId,
    pub title: String,
    pub project: String,
    pub repository: String,
    pub branch: String,
    pub revision: u64,
    pub reviewed: usize,
    pub total: usize,
    pub incoming: usize,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewSet {
    pub summary: ReviewSummary,
    pub checkpoint: ReviewCheckpoint,
    pub units: Vec<ReviewUnit>,
    pub commits: Vec<CommitSummary>,
    pub plan: Option<MarkdownDocument>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewCheckpoint {
    pub revision: u64,
    pub frozen_at: DateTime<Utc>,
    pub reviewed: usize,
    pub total: usize,
    pub source_revision: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IncomingBurst {
    pub commits: usize,
    pub units: usize,
    pub reopened: usize,
    pub arrived_at: DateTime<Utc>,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewUnit {
    pub id: ReviewUnitId,
    pub path: String,
    pub title: String,
    pub language: String,
    pub kind: ReviewUnitKind,
    pub mark: ReviewMarkState,
    pub transition: UnitTransition,
    pub additions: usize,
    pub deletions: usize,
    pub diff: Vec<DiffLine>,
    pub source: Vec<SourceLine>,
    pub calls: Vec<CallNode>,
    pub ast: Vec<AstNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewUnitKind {
    File,
    Symbol,
    MarkdownSection,
    Commit,
    PullRequest,
    CiRun,
    Artifact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewMarkState {
    Unreviewed,
    Reviewed,
    NeedsAttention,
    Dismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnitTransition {
    New,
    Unchanged,
    Changed,
    Moved,
    FormattingOnly,
    Removed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewMark {
    pub review_id: ReviewId,
    pub unit_id: ReviewUnitId,
    pub state: ReviewMarkState,
    pub revision: Revision,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiffLine {
    pub old_number: Option<usize>,
    pub new_number: Option<usize>,
    pub kind: DiffLineKind,
    pub text: String,
    pub spans: Vec<SyntaxSpan>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
    Header,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceLine {
    pub number: usize,
    pub text: String,
    pub spans: Vec<SyntaxSpan>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyntaxSpan {
    pub start: usize,
    pub end: usize,
    pub token: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CallNode {
    pub id: String,
    pub label: String,
    pub detail: String,
    pub depth: usize,
    pub direction: CallDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallDirection {
    Caller,
    Current,
    Callee,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AstNode {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub depth: usize,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownDocument {
    pub title: String,
    pub path: String,
    pub sections: Vec<MarkdownSection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownSection {
    pub id: String,
    pub heading: String,
    pub level: usize,
    pub body: String,
    pub reviewed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CommitSummary {
    pub oid: String,
    pub subject: String,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitGraph {
    pub worktree_id: Option<WorktreeId>,
    pub rows: Vec<GitGraphRow>,
    pub references: Vec<GitReference>,
    pub has_more: bool,
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitGraphRow {
    pub oid: String,
    pub short_oid: String,
    pub subject: String,
    pub author: String,
    pub timestamp: DateTime<Utc>,
    pub lane: usize,
    /// Lanes that enter the row from the commit immediately above it.
    pub lanes_before: Vec<usize>,
    /// Lanes that leave the row toward the commit immediately below it.
    pub lanes_after: Vec<usize>,
    /// Exact parent connections originating at this commit.
    pub edges: Vec<GitGraphEdge>,
    pub references: Vec<String>,
    pub head: bool,
    pub wip: bool,
    pub additions: usize,
    pub deletions: usize,
    pub files_changed: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitGraphEdge {
    pub from_lane: usize,
    pub to_lane: usize,
    pub parent_oid: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GitReference {
    pub name: String,
    pub kind: GitReferenceKind,
    pub target: String,
    pub current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitReferenceKind {
    LocalBranch,
    RemoteBranch,
    Tag,
    Stash,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PullRequest {
    pub number: u64,
    pub title: String,
    pub repository: String,
    pub author: String,
    pub branch: String,
    pub base: String,
    pub state: PullRequestState,
    pub checks: CheckSummary,
    pub additions: usize,
    pub deletions: usize,
    pub files: usize,
    pub comments: usize,
    pub commits: usize,
    pub activity_revision: String,
    pub unread: bool,
    pub updated_at: DateTime<Utc>,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestState {
    Draft,
    Open,
    Merged,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckSummary {
    pub passed: usize,
    pub failed: usize,
    pub pending: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CiRun {
    pub id: String,
    pub name: String,
    pub repository: String,
    pub branch: String,
    pub commit: String,
    pub status: CiStatus,
    pub jobs: Vec<CiJob>,
    pub started_at: DateTime<Utc>,
    pub duration_seconds: Option<u64>,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CiJob {
    pub id: String,
    pub name: String,
    pub status: CiStatus,
    pub steps: Vec<CiStep>,
    pub log_lines: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CiJobLog {
    pub job_id: String,
    pub job_name: String,
    pub lines: Vec<String>,
    pub original_bytes: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CiStep {
    pub name: String,
    pub status: CiStatus,
    pub duration_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CiStatus {
    Queued,
    Running,
    Passed,
    Failed,
    Cancelled,
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRecord {
    pub id: ArtifactId,
    pub name: String,
    pub kind: ArtifactKind,
    pub source: String,
    pub size_bytes: u64,
    pub entry_count: usize,
    pub imported_at: DateTime<Utc>,
    pub preview_available: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactPreviewSession {
    pub session_id: OperationId,
    pub artifact_id: ArtifactId,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Html,
    Zip,
    Log,
    Report,
    Image,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchSnapshot {
    pub query: String,
    pub groups: Vec<SearchResultGroup>,
    pub total: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResultGroup {
    pub kind: SearchResultKind,
    pub label: String,
    pub results: Vec<SearchResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchResultKind {
    Project,
    Repository,
    Checkout,
    Worktree,
    Review,
    File,
    Symbol,
    Markdown,
    Commit,
    Branch,
    PullRequest,
    Ci,
    Artifact,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
    pub id: String,
    pub title: String,
    pub subtitle: String,
    pub metadata: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskProgress {
    pub operation_id: OperationId,
    pub label: String,
    pub completed: usize,
    pub total: usize,
    pub cancellable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderState {
    Ready,
    Offline,
    Loading,
    Partial,
    Unauthenticated,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UiPreferences {
    pub appearance: Appearance,
    pub navigator_visible: bool,
    pub inspector_visible: bool,
    pub navigator_width: f64,
    pub inspector_width: f64,
    pub pane_widths: PaneWidths,
    pub selected_area: String,
    pub active_tab: Option<String>,
    pub saved_searches: Vec<String>,
    pub recent_searches: Vec<String>,
    pub expanded_nodes: BTreeSet<String>,
    pub scroll_positions: BTreeMap<String, f64>,
}

/// Presentation-only split-pane geometry. These values never identify or
/// modify repository data; they simply preserve the reader's preferred
/// balance between lists, content, and evidence across restarts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneWidths {
    pub master_list: f64,
    pub git_references: f64,
    pub git_detail: f64,
    pub review_tree: f64,
    pub review_structure: f64,
    pub artifact_library: f64,
}

impl Default for PaneWidths {
    fn default() -> Self {
        Self {
            master_list: 420.0,
            git_references: 208.0,
            git_detail: 360.0,
            review_tree: 272.0,
            review_structure: 280.0,
            artifact_library: 320.0,
        }
    }
}

impl PaneWidths {
    pub fn bounded(mut self) -> Self {
        self.master_list = self.master_list.clamp(300.0, 640.0);
        self.git_references = self.git_references.clamp(160.0, 360.0);
        self.git_detail = self.git_detail.clamp(280.0, 560.0);
        self.review_tree = self.review_tree.clamp(220.0, 480.0);
        self.review_structure = self.review_structure.clamp(220.0, 480.0);
        self.artifact_library = self.artifact_library.clamp(240.0, 520.0);
        self
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UiPreferencePatch {
    pub appearance: Option<Appearance>,
    pub navigator_visible: Option<bool>,
    pub inspector_visible: Option<bool>,
    pub navigator_width: Option<f64>,
    pub inspector_width: Option<f64>,
    pub pane_widths: Option<PaneWidths>,
    pub selected_area: Option<String>,
    pub active_tab: Option<String>,
    pub saved_searches: Option<Vec<String>>,
    pub recent_searches: Option<Vec<String>>,
    pub expanded_nodes: Option<BTreeSet<String>>,
    pub scroll_positions: Option<BTreeMap<String, f64>>,
}

impl UiPreferences {
    pub fn apply(&mut self, patch: UiPreferencePatch) {
        if let Some(value) = patch.appearance {
            self.appearance = value;
        }
        if let Some(value) = patch.navigator_visible {
            self.navigator_visible = value;
        }
        if let Some(value) = patch.inspector_visible {
            self.inspector_visible = value;
        }
        if let Some(value) = patch.navigator_width {
            self.navigator_width = value.clamp(208.0, 360.0);
        }
        if let Some(value) = patch.inspector_width {
            self.inspector_width = value.clamp(240.0, 720.0);
        }
        if let Some(value) = patch.pane_widths {
            self.pane_widths = value.bounded();
        }
        if let Some(value) = patch.selected_area {
            self.selected_area = value;
        }
        if let Some(value) = patch.active_tab {
            self.active_tab = Some(value);
        }
        if let Some(value) = patch.saved_searches {
            self.saved_searches = value.into_iter().take(24).collect();
        }
        if let Some(value) = patch.recent_searches {
            let mut normalized = Vec::new();
            for query in value {
                let query = query.trim();
                if !query.is_empty() && !normalized.iter().any(|value| value == query) {
                    normalized.push(query.to_owned());
                }
                if normalized.len() == 12 {
                    break;
                }
            }
            self.recent_searches = normalized;
        }
        if let Some(value) = patch.expanded_nodes {
            self.expanded_nodes = value;
        }
        if let Some(value) = patch.scroll_positions {
            self.scroll_positions = value;
        }
    }
}

/// Deterministic renderer client used by browser preview and UI tests.
#[derive(Clone)]
pub struct FixtureWorkdeckClient {
    state: Arc<Mutex<FixtureState>>,
    events_tx: async_channel::Sender<WorkdeckEvent>,
    events_rx: async_channel::Receiver<WorkdeckEvent>,
}

struct FixtureState {
    revision: Revision,
    snapshot: BootstrapSnapshot,
}

impl FixtureWorkdeckClient {
    pub fn polished() -> Self {
        let (events_tx, events_rx) = async_channel::unbounded();
        Self {
            state: Arc::new(Mutex::new(FixtureState {
                revision: Revision(1),
                snapshot: fixtures::polished(),
            })),
            events_tx,
            events_rx,
        }
    }

    pub fn empty() -> Self {
        let client = Self::polished();
        if let Ok(mut state) = client.state.lock() {
            state.snapshot.portfolio.projects.clear();
            state.snapshot.portfolio.project_count = 0;
            state.snapshot.portfolio.repository_count = 0;
            state.snapshot.portfolio.worktree_count = 0;
            state.snapshot.inbox.items.clear();
            state.snapshot.reviews.clear();
        }
        client
    }

    pub fn offline() -> Self {
        let client = Self::polished();
        if let Ok(mut state) = client.state.lock() {
            state.snapshot.provider_state = ProviderState::Offline;
        }
        client
    }

    pub fn into_client(self) -> WorkdeckClient {
        WorkdeckClient::new(self)
    }
}

impl Default for FixtureWorkdeckClient {
    fn default() -> Self {
        Self::polished()
    }
}

impl WorkdeckTransport for FixtureWorkdeckClient {
    fn request(
        &self,
        request: WorkdeckRequest,
    ) -> BoxFuture<'static, Result<Envelope<WorkdeckResponse>, WorkdeckError>> {
        let state = Arc::clone(&self.state);
        let events = self.events_tx.clone();
        Box::pin(async move {
            let request_id = request.request_id().clone();
            let mut state = state
                .lock()
                .map_err(|_| WorkdeckError::Internal("fixture state lock poisoned".into()))?;
            let payload = match request {
                WorkdeckRequest::Bootstrap { .. } => {
                    WorkdeckResponse::Bootstrap(state.snapshot.clone())
                }
                WorkdeckRequest::LoadInbox { .. } => {
                    WorkdeckResponse::Inbox(state.snapshot.inbox.clone())
                }
                WorkdeckRequest::MarkActivityRead {
                    target, revision, ..
                } => {
                    for item in &mut state.snapshot.inbox.items {
                        if item.target == target && item.revision == revision {
                            item.unread = false;
                        }
                    }
                    if let ActivityTarget::PullRequest { repository, number } = &target {
                        for pull in &mut state.snapshot.pull_requests {
                            if &pull.repository == repository
                                && pull.number == *number
                                && pull.activity_revision == revision
                            {
                                pull.unread = false;
                            }
                        }
                    }
                    state.snapshot.inbox.unread = state
                        .snapshot
                        .inbox
                        .items
                        .iter()
                        .filter(|item| item.unread)
                        .count();
                    state.revision = state.revision.next();
                    WorkdeckResponse::ActivityRead(ActivityReadState {
                        target,
                        revision,
                        unread: false,
                        read_at: Utc::now(),
                    })
                }
                WorkdeckRequest::ScanInbox { operation_id, .. } => {
                    let total = state.snapshot.portfolio.worktree_count;
                    let _ = events.try_send(WorkdeckEvent::TaskProgress(TaskProgress {
                        operation_id,
                        label: "Scanning commit updates".into(),
                        completed: total,
                        total,
                        cancellable: true,
                    }));
                    WorkdeckResponse::Inbox(state.snapshot.inbox.clone())
                }
                WorkdeckRequest::LoadPortfolio { .. } => {
                    WorkdeckResponse::Portfolio(state.snapshot.portfolio.clone())
                }
                WorkdeckRequest::DiscoverPortfolio { .. } => {
                    WorkdeckResponse::Portfolio(state.snapshot.portfolio.clone())
                }
                WorkdeckRequest::ChoosePortfolioRoot { .. } => {
                    WorkdeckResponse::Portfolio(state.snapshot.portfolio.clone())
                }
                WorkdeckRequest::Search { query, .. } => {
                    WorkdeckResponse::Search(fixtures::search(&state.snapshot, &query))
                }
                WorkdeckRequest::LoadGitGraph { cursor, .. } => WorkdeckResponse::GitGraph(
                    cursor.map_or_else(fixtures::git_graph, |_| fixtures::older_git_graph()),
                ),
                WorkdeckRequest::LoadReview { review_id, .. } => {
                    let review = fixtures::review(&review_id).ok_or_else(|| {
                        WorkdeckError::InvalidRequest(format!("unknown change source {review_id}"))
                    })?;
                    WorkdeckResponse::Review(review)
                }
                WorkdeckRequest::PrepareWorktreeReview { .. }
                | WorkdeckRequest::PrepareCommitRangeReview { .. }
                | WorkdeckRequest::PreparePullRequestReview { .. } => {
                    let review_id = state
                        .snapshot
                        .reviews
                        .first()
                        .map(|review| review.id.clone())
                        .ok_or_else(|| {
                            WorkdeckError::InvalidRequest("fixture has no changes".into())
                        })?;
                    WorkdeckResponse::Review(
                        fixtures::review(&review_id).expect("fixture review exists"),
                    )
                }
                WorkdeckRequest::LoadPullRequests { repository, .. } => {
                    WorkdeckResponse::PullRequests(
                        state
                            .snapshot
                            .pull_requests
                            .iter()
                            .filter(|pull| {
                                repository.as_ref().is_none_or(|provider| {
                                    provider == &pull.repository
                                        || provider.ends_with(&format!("/{}", pull.repository))
                                })
                            })
                            .cloned()
                            .collect(),
                    )
                }
                WorkdeckRequest::LoadCiRuns { repository, .. } => WorkdeckResponse::CiRuns(
                    state
                        .snapshot
                        .ci_runs
                        .iter()
                        .filter(|run| {
                            repository.as_ref().is_none_or(|provider| {
                                provider == &run.repository
                                    || provider.ends_with(&format!("/{}", run.repository))
                            })
                        })
                        .cloned()
                        .collect(),
                ),
                WorkdeckRequest::LoadCiJobLog {
                    job_id, job_name, ..
                } => WorkdeckResponse::CiJobLog(CiJobLog {
                    job_id: job_id.to_string(),
                    job_name,
                    lines: vec![
                        "Checkout source".into(),
                        "Run tests".into(),
                        "Finished".into(),
                    ],
                    original_bytes: 34,
                    truncated: false,
                }),
                WorkdeckRequest::LoadArtifacts { .. } => {
                    WorkdeckResponse::Artifacts(state.snapshot.artifacts.clone())
                }
                WorkdeckRequest::ChooseArtifactImport { .. } => {
                    WorkdeckResponse::Artifacts(state.snapshot.artifacts.clone())
                }
                WorkdeckRequest::OpenArtifact { artifact_id, .. } => {
                    WorkdeckResponse::ArtifactPreview(ArtifactPreviewSession {
                        session_id: OperationId::new(),
                        artifact_id,
                        url: "about:blank".into(),
                    })
                }
                WorkdeckRequest::CloseArtifact { .. } => {
                    WorkdeckResponse::Artifacts(state.snapshot.artifacts.clone())
                }
                WorkdeckRequest::UpdateReviewMark {
                    review_id,
                    unit_id,
                    state: mark_state,
                    ..
                } => {
                    state.revision = state.revision.next();
                    let revision = state.revision;
                    WorkdeckResponse::ReviewMark(ReviewMark {
                        review_id,
                        unit_id,
                        state: mark_state,
                        revision,
                    })
                }
                WorkdeckRequest::CreateCheckpoint { review_id, .. } => {
                    state.revision = state.revision.next();
                    WorkdeckResponse::Checkpoint(ReviewCheckpoint {
                        revision: state.revision.0,
                        frozen_at: Utc::now(),
                        reviewed: 0,
                        total: fixtures::review(&review_id).map_or(0, |review| review.units.len()),
                        source_revision: "fixture-revision".into(),
                    })
                }
                WorkdeckRequest::UpdatePreferences { patch, .. } => {
                    state.snapshot.preferences.apply(patch);
                    WorkdeckResponse::Preferences(state.snapshot.preferences.clone())
                }
            };
            let revision = state.revision;
            drop(state);
            let _ = events.try_send(WorkdeckEvent::SnapshotUpdated { revision });
            Ok(Envelope {
                request_id,
                revision,
                payload,
            })
        })
    }

    fn subscribe(&self) -> WorkdeckEventStream {
        self.events_rx.clone()
    }

    fn cancel(&self, operation: OperationId) {
        let _ = self.events_tx.try_send(WorkdeckEvent::OperationCancelled {
            operation_id: operation,
        });
    }
}

pub mod fixtures {
    use super::*;

    pub fn empty() -> BootstrapSnapshot {
        let mut snapshot = polished();
        snapshot.portfolio.projects.clear();
        snapshot.portfolio.project_count = 0;
        snapshot.portfolio.repository_count = 0;
        snapshot.portfolio.worktree_count = 0;
        snapshot.portfolio.unavailable_count = 0;
        snapshot.inbox.items.clear();
        snapshot.inbox.unread = 0;
        snapshot.inbox.commit_updates = 0;
        snapshot.inbox.pull_request_updates = 0;
        snapshot.reviews.clear();
        snapshot.pull_requests.clear();
        snapshot.ci_runs.clear();
        snapshot.artifacts.clear();
        snapshot
    }

    pub fn offline() -> BootstrapSnapshot {
        let mut snapshot = polished();
        snapshot.provider_state = ProviderState::Offline;
        snapshot
    }

    pub fn polished() -> BootstrapSnapshot {
        let now = DateTime::parse_from_rfc3339("2026-08-27T10:00:00Z")
            .expect("valid fixture clock")
            .with_timezone(&Utc);
        let projects = vec![
            project("sampleapp", 8, now),
            project("Workdeck", 3, now - chrono::Duration::minutes(12)),
            project("nomad", 2, now - chrono::Duration::hours(2)),
        ];
        let portfolio = PortfolioSnapshot {
            project_count: 74,
            repository_count: 75,
            worktree_count: 109,
            unavailable_count: 4,
            projects,
            scanned_at: now,
        };
        let inbox = InboxSnapshot {
            items: vec![
                InboxItem {
                    id: "commit-branch-sampleapp".into(),
                    target: ActivityTarget::CommitBranch {
                        worktree_id: WorktreeId::from("worktree-sampleapp"),
                    },
                    kind: ActivityKind::CommitBranch,
                    revision: "sampleapp-head-5".into(),
                    unread: true,
                    project: "sampleapp".into(),
                    repository: "sampleapp".into(),
                    title: "5 new commits on feat/feed-first-product-ui".into(),
                    branch: "feat/feed-first-product-ui".into(),
                    summary: "Agent pushes changed 18 files since you last opened this branch"
                        .into(),
                    changes: 18,
                    commits: 5,
                    updated_at: now,
                },
                InboxItem {
                    id: "commit-branch-workdeck".into(),
                    target: ActivityTarget::CommitBranch {
                        worktree_id: WorktreeId::from("worktree-workdeck"),
                    },
                    kind: ActivityKind::CommitBranch,
                    revision: "workdeck-head-2".into(),
                    unread: true,
                    project: "Workdeck".into(),
                    repository: "workdeck".into(),
                    title: "2 new commits on main".into(),
                    branch: "main".into(),
                    summary: "Dioxus shell and Git graph advanced".into(),
                    changes: 12,
                    commits: 2,
                    updated_at: now - chrono::Duration::minutes(12),
                },
                InboxItem {
                    id: "pull-request-sampleapp-42".into(),
                    target: ActivityTarget::PullRequest {
                        repository: "sampleapp".into(),
                        number: 42,
                    },
                    kind: ActivityKind::PullRequest,
                    revision: "2026-08-27T09:52:00+00:00".into(),
                    unread: true,
                    project: "sampleapp".into(),
                    repository: "sampleapp".into(),
                    title: "PR #42 · Feed-first dashboard and attribution".into(),
                    branch: "feat/opportunities".into(),
                    summary: "New push and comments · 1 check failing".into(),
                    changes: 7,
                    commits: 3,
                    updated_at: now - chrono::Duration::minutes(23),
                },
            ],
            unread: 3,
            commit_updates: 2,
            pull_request_updates: 1,
            scan: None,
            updated_at: now,
        };
        let reviews = vec![
            ReviewSummary {
                id: ReviewId::from("review-sampleapp"),
                title: "Feed-first dashboard and attribution".into(),
                project: "sampleapp".into(),
                repository: "sampleapp".into(),
                branch: "feat/feed-first-product-ui".into(),
                revision: 12,
                reviewed: 14,
                total: 18,
                incoming: 7,
                updated_at: now,
            },
            ReviewSummary {
                id: ReviewId::from("review-workdeck"),
                title: "Workdeck Dioxus rewrite".into(),
                project: "Workdeck".into(),
                repository: "workdeck".into(),
                branch: "main".into(),
                revision: 3,
                reviewed: 5,
                total: 12,
                incoming: 2,
                updated_at: now - chrono::Duration::minutes(12),
            },
        ];
        let pull_requests = vec![
            PullRequest {
                number: 42,
                title: "Feed-first dashboard and attribution".into(),
                repository: "sampleapp".into(),
                author: "agent/feed".into(),
                branch: "feat/opportunities".into(),
                base: "main".into(),
                state: PullRequestState::Open,
                checks: CheckSummary {
                    passed: 8,
                    failed: 1,
                    pending: 0,
                },
                additions: 842,
                deletions: 126,
                files: 17,
                comments: 4,
                commits: 5,
                activity_revision: "2026-08-27T09:52:00+00:00".into(),
                unread: true,
                updated_at: now - chrono::Duration::minutes(8),
                url: "https://github.com/example/sampleapp/pull/42".into(),
            },
            PullRequest {
                number: 18,
                title: "Harden repository discovery".into(),
                repository: "workdeck".into(),
                author: "agent/catalog".into(),
                branch: "feat/catalog".into(),
                base: "main".into(),
                state: PullRequestState::Draft,
                checks: CheckSummary {
                    passed: 5,
                    failed: 0,
                    pending: 2,
                },
                additions: 306,
                deletions: 48,
                files: 9,
                comments: 1,
                commits: 2,
                activity_revision: "2026-08-27T09:00:00+00:00".into(),
                unread: true,
                updated_at: now - chrono::Duration::hours(1),
                url: "https://github.com/example/workdeck/pull/18".into(),
            },
        ];
        let ci_runs = vec![CiRun {
            id: "run-1082".into(),
            name: "CI / pull_request".into(),
            repository: "sampleapp".into(),
            branch: "feat/opportunities".into(),
            commit: "c81f4e9".into(),
            status: CiStatus::Failed,
            jobs: vec![CiJob {
                id: "job-tests".into(),
                name: "Laravel tests".into(),
                status: CiStatus::Failed,
                steps: vec![
                    CiStep {
                        name: "Install dependencies".into(),
                        status: CiStatus::Passed,
                        duration_seconds: Some(34),
                    },
                    CiStep {
                        name: "Feature tests".into(),
                        status: CiStatus::Failed,
                        duration_seconds: Some(71),
                    },
                ],
                log_lines: vec![
                    "FAIL  Tests\\Feature\\DashboardTest".into(),
                    "Expected response status code [200] but received 500.".into(),
                    "at tests/Feature/DashboardTest.php:84".into(),
                ],
            }],
            started_at: now - chrono::Duration::minutes(16),
            duration_seconds: Some(108),
            url: "https://github.com/example/sampleapp/actions/runs/1082".into(),
        }];
        let artifacts = vec![
            ArtifactRecord {
                id: ArtifactId::from("artifact-report"),
                name: "coverage-report.zip".into(),
                kind: ArtifactKind::Html,
                source: "CI / pull_request #1082".into(),
                size_bytes: 4_821_304,
                entry_count: 183,
                imported_at: now - chrono::Duration::minutes(14),
                preview_available: true,
            },
            ArtifactRecord {
                id: ArtifactId::from("artifact-playwright"),
                name: "playwright-report.zip".into(),
                kind: ArtifactKind::Html,
                source: "PR #42".into(),
                size_bytes: 11_204_091,
                entry_count: 96,
                imported_at: now - chrono::Duration::hours(1),
                preview_available: true,
            },
        ];
        BootstrapSnapshot {
            portfolio,
            suggested_roots: vec![
                "/Users/example/Projects".into(),
                "/Users/example/Sites".into(),
            ],
            inbox,
            reviews,
            pull_requests,
            ci_runs,
            artifacts,
            preferences: UiPreferences {
                appearance: Appearance::System,
                navigator_visible: true,
                inspector_visible: false,
                navigator_width: 256.0,
                inspector_width: 320.0,
                selected_area: "inbox".into(),
                active_tab: Some("inbox".into()),
                ..UiPreferences::default()
            },
            provider_state: ProviderState::Ready,
        }
    }

    fn project(name: &str, attention: usize, last_seen: DateTime<Utc>) -> ProjectNode {
        let slug = name.to_ascii_lowercase().replace(' ', "-");
        ProjectNode {
            id: ProjectId(format!("project-{slug}")),
            name: name.into(),
            repositories: vec![RepositoryNode {
                id: RepositoryId(format!("repo-{slug}")),
                name: if name == "Workdeck" {
                    "workdeck".into()
                } else {
                    name.into()
                },
                provider: Some(format!("example/{slug}")),
                attention,
                checkouts: vec![CheckoutNode {
                    id: CheckoutId(format!("checkout-{slug}")),
                    label: "Local checkout".into(),
                    available: true,
                    worktrees: vec![WorktreeNode {
                        id: WorktreeId(format!("worktree-{slug}")),
                        label: name.into(),
                        branch: Some("main".into()),
                        path_hint: format!("…/{slug}"),
                        availability: Availability::Available,
                        changes: attention,
                        commits: attention.min(5),
                        last_seen,
                    }],
                }],
            }],
            attention,
            empty: false,
        }
    }

    pub fn review(id: &ReviewId) -> Option<ReviewSet> {
        let bootstrap = polished();
        let summary = bootstrap
            .reviews
            .into_iter()
            .find(|review| &review.id == id)?;
        let checkpoint = ReviewCheckpoint {
            revision: summary.revision,
            frozen_at: summary.updated_at - chrono::Duration::minutes(14),
            reviewed: summary.reviewed,
            total: summary.total,
            source_revision: "c81f4e9".into(),
        };
        Some(ReviewSet {
            summary,
            checkpoint,
            units: sample_units(),
            commits: git_graph()
                .rows
                .into_iter()
                .take(5)
                .map(|row| CommitSummary {
                    oid: row.oid,
                    subject: row.subject,
                    author: row.author,
                    timestamp: row.timestamp,
                    additions: row.additions,
                    deletions: row.deletions,
                })
                .collect(),
            plan: Some(MarkdownDocument {
                title: "Implementation plan".into(),
                path: "docs/IMPLEMENTATION.md".into(),
                sections: vec![
                    MarkdownSection {
                        id: "goals".into(),
                        heading: "Goals".into(),
                        level: 1,
                        body: "Make repository activity calm, explainable, and fast across every active project.".into(),
                        reviewed: true,
                    },
                    MarkdownSection {
                        id: "delivery".into(),
                        heading: "Delivery".into(),
                        level: 2,
                        body: "Ship vertical slices with deterministic visual and interaction evidence.".into(),
                        reviewed: false,
                    },
                ],
            }),
        })
    }

    fn sample_units() -> Vec<ReviewUnit> {
        vec![
            ReviewUnit {
                id: ReviewUnitId::from("unit-dashboard"),
                path: "app/Services/OpportunityFeed.php".into(),
                title: "OpportunityFeed::rank".into(),
                language: "php".into(),
                kind: ReviewUnitKind::Symbol,
                mark: ReviewMarkState::NeedsAttention,
                transition: UnitTransition::Changed,
                additions: 24,
                deletions: 7,
                diff: vec![
                    DiffLine {
                        old_number: Some(41),
                        new_number: Some(41),
                        kind: DiffLineKind::Context,
                        text: "public function rank(Collection $signals): Collection".into(),
                        spans: fixture_php_spans(
                            "public function rank(Collection $signals): Collection",
                        ),
                    },
                    DiffLine {
                        old_number: Some(42),
                        new_number: None,
                        kind: DiffLineKind::Removed,
                        text: "    return $signals->sortByDesc('score');".into(),
                        spans: fixture_php_spans(
                            "    return $signals->sortByDesc('score');",
                        ),
                    },
                    DiffLine {
                        old_number: None,
                        new_number: Some(42),
                        kind: DiffLineKind::Added,
                        text: "    return $signals->sortByDesc(fn ($signal) => $this->priority($signal));".into(),
                        spans: fixture_php_spans(
                            "    return $signals->sortByDesc(fn ($signal) => $this->priority($signal));",
                        ),
                    },
                    DiffLine {
                        old_number: None,
                        new_number: Some(43),
                        kind: DiffLineKind::Added,
                        text: "        ->values();".into(),
                        spans: fixture_php_spans("        ->values();"),
                    },
                ],
                source: vec![
                    SourceLine {
                        number: 41,
                        text: "public function rank(Collection $signals): Collection".into(),
                        spans: fixture_php_spans(
                            "public function rank(Collection $signals): Collection",
                        ),
                    },
                    SourceLine {
                        number: 42,
                        text: "    return $signals->sortByDesc(fn ($signal) => $this->priority($signal));".into(),
                        spans: fixture_php_spans(
                            "    return $signals->sortByDesc(fn ($signal) => $this->priority($signal));",
                        ),
                    },
                    SourceLine {
                        number: 43,
                        text: "        ->values();".into(),
                        spans: fixture_php_spans("        ->values();"),
                    },
                ],
                calls: vec![
                    CallNode {
                        id: "caller-controller".into(),
                        label: "DashboardController::index".into(),
                        detail: "caller · line 28".into(),
                        depth: 0,
                        direction: CallDirection::Caller,
                    },
                    CallNode {
                        id: "current-rank".into(),
                        label: "OpportunityFeed::rank".into(),
                        detail: "current symbol".into(),
                        depth: 1,
                        direction: CallDirection::Current,
                    },
                    CallNode {
                        id: "callee-priority".into(),
                        label: "OpportunityFeed::priority".into(),
                        detail: "callee · line 55".into(),
                        depth: 2,
                        direction: CallDirection::Callee,
                    },
                ],
                ast: vec![
                    AstNode {
                        id: "ast-method".into(),
                        kind: "method_declaration".into(),
                        label: "rank".into(),
                        depth: 0,
                        line: 41,
                    },
                    AstNode {
                        id: "ast-return".into(),
                        kind: "return_statement".into(),
                        label: "return".into(),
                        depth: 1,
                        line: 42,
                    },
                    AstNode {
                        id: "ast-call".into(),
                        kind: "member_call_expression".into(),
                        label: "sortByDesc".into(),
                        depth: 2,
                        line: 42,
                    },
                ],
            },
            ReviewUnit {
                id: ReviewUnitId::from("unit-test"),
                path: "tests/Feature/DashboardTest.php".into(),
                title: "ranks opportunities by decision value".into(),
                language: "php".into(),
                kind: ReviewUnitKind::Symbol,
                mark: ReviewMarkState::Unreviewed,
                transition: UnitTransition::New,
                additions: 31,
                deletions: 0,
                diff: vec![DiffLine {
                    old_number: None,
                    new_number: Some(84),
                    kind: DiffLineKind::Added,
                    text: "expect($response->json('data.0.id'))->toBe($highestValue->id);".into(),
                    spans: fixture_php_spans(
                        "expect($response->json('data.0.id'))->toBe($highestValue->id);",
                    ),
                }],
                source: Vec::new(),
                calls: Vec::new(),
                ast: Vec::new(),
            },
            ReviewUnit {
                id: ReviewUnitId::from("unit-plan"),
                path: "docs/IMPLEMENTATION.md".into(),
                title: "Delivery".into(),
                language: "markdown".into(),
                kind: ReviewUnitKind::MarkdownSection,
                mark: ReviewMarkState::Reviewed,
                transition: UnitTransition::Unchanged,
                additions: 4,
                deletions: 1,
                diff: Vec::new(),
                source: Vec::new(),
                calls: Vec::new(),
                ast: Vec::new(),
            },
        ]
    }

    fn fixture_php_spans(text: &str) -> Vec<SyntaxSpan> {
        const TOKENS: &[(&str, &str)] = &[
            ("public", "keyword"),
            ("function", "keyword"),
            ("return", "keyword"),
            ("fn", "keyword"),
            ("Collection", "type"),
            ("$signals", "parameter"),
            ("$signal", "parameter"),
            ("$response", "variable"),
            ("$highestValue", "variable"),
            ("$this", "builtin"),
            ("rank", "function"),
            ("sortByDesc", "function"),
            ("priority", "function"),
            ("values", "function"),
            ("expect", "function"),
            ("json", "function"),
            ("toBe", "function"),
            ("'score'", "string"),
            ("'data.0.id'", "string"),
            ("id", "property"),
        ];

        let mut candidates = TOKENS
            .iter()
            .flat_map(|(needle, token)| {
                text.match_indices(needle)
                    .map(move |(start, value)| SyntaxSpan {
                        start,
                        end: start + value.len(),
                        token: (*token).to_owned(),
                    })
            })
            .collect::<Vec<_>>();
        candidates.sort_by(|left, right| {
            left.start
                .cmp(&right.start)
                .then_with(|| right.end.cmp(&left.end))
        });

        let mut end = 0;
        candidates
            .into_iter()
            .filter(|span| {
                if span.start < end {
                    false
                } else {
                    end = span.end;
                    true
                }
            })
            .collect()
    }

    pub fn git_graph() -> GitGraph {
        let now = DateTime::parse_from_rfc3339("2026-08-27T12:40:00Z")
            .expect("fixture clock")
            .with_timezone(&Utc);
        GitGraph {
            worktree_id: Some(WorktreeId::from("worktree-sampleapp")),
            rows: vec![
                graph_row(
                    "wip-worktree-sampleapp",
                    "Working tree changes",
                    0,
                    vec![],
                    vec![("c81f4e9", 0)],
                    FixtureNode::Wip,
                    now + chrono::Duration::minutes(1),
                ),
                graph_row(
                    "c81f4e9",
                    "Refine opportunity ranking",
                    0,
                    vec![0],
                    vec![("925ad21", 0)],
                    FixtureNode::Head,
                    now,
                ),
                graph_row(
                    "925ad21",
                    "Add decision-ready feed cards",
                    0,
                    vec![0],
                    vec![("7af2b10", 0)],
                    FixtureNode::Commit,
                    now - chrono::Duration::minutes(9),
                ),
                graph_row(
                    "7af2b10",
                    "Merge main into feat/opportunities",
                    0,
                    vec![0],
                    vec![("13e16bc", 0), ("5b71c02", 1)],
                    FixtureNode::Commit,
                    now - chrono::Duration::minutes(18),
                ),
                graph_row(
                    "5b71c02",
                    "Harden attribution query",
                    1,
                    vec![0, 1],
                    vec![("f0a918d", 1)],
                    FixtureNode::Commit,
                    now - chrono::Duration::minutes(25),
                ),
                graph_row(
                    "13e16bc",
                    "Add dashboard integration coverage",
                    0,
                    vec![0, 1],
                    vec![("f0a918d", 1)],
                    FixtureNode::Commit,
                    now - chrono::Duration::minutes(31),
                ),
                graph_row(
                    "f0a918d",
                    "Feed-first dashboard baseline",
                    1,
                    vec![1],
                    vec![("8d779c1", 1)],
                    FixtureNode::Commit,
                    now - chrono::Duration::hours(1),
                ),
            ],
            references: vec![
                GitReference {
                    name: "feat/opportunities".into(),
                    kind: GitReferenceKind::LocalBranch,
                    target: full_oid("c81f4e9"),
                    current: true,
                },
                GitReference {
                    name: "origin/main".into(),
                    kind: GitReferenceKind::RemoteBranch,
                    target: full_oid("5b71c02"),
                    current: false,
                },
                GitReference {
                    name: "v0.8.0".into(),
                    kind: GitReferenceKind::Tag,
                    target: full_oid("f0a918d"),
                    current: false,
                },
            ],
            has_more: true,
            next_cursor: Some("page-2".into()),
        }
    }

    pub fn older_git_graph() -> GitGraph {
        let now = DateTime::parse_from_rfc3339("2026-08-27T11:40:00Z")
            .expect("fixture clock")
            .with_timezone(&Utc);
        GitGraph {
            worktree_id: Some(WorktreeId::from("worktree-sampleapp")),
            rows: vec![
                graph_row(
                    "8d779c1",
                    "Add activity read cursors",
                    1,
                    vec![1],
                    vec![("31b69f0", 1)],
                    FixtureNode::Commit,
                    now - chrono::Duration::minutes(20),
                ),
                graph_row(
                    "31b69f0",
                    "Add portfolio discovery",
                    1,
                    vec![1],
                    vec![("0b5e8f2", 1)],
                    FixtureNode::Commit,
                    now - chrono::Duration::minutes(48),
                ),
                graph_row(
                    "0b5e8f2",
                    "Initial import",
                    1,
                    vec![1],
                    Vec::new(),
                    FixtureNode::Commit,
                    now - chrono::Duration::hours(2),
                ),
            ],
            references: Vec::new(),
            has_more: false,
            next_cursor: None,
        }
    }

    fn graph_row(
        oid: &str,
        subject: &str,
        lane: usize,
        lanes_before: Vec<usize>,
        parents: Vec<(&str, usize)>,
        kind: FixtureNode,
        timestamp: DateTime<Utc>,
    ) -> GitGraphRow {
        let head = kind == FixtureNode::Head;
        let wip = kind == FixtureNode::Wip;
        let oid = if wip { oid.into() } else { full_oid(oid) };
        let short_oid = if wip {
            "WIP".into()
        } else {
            oid.chars().take(7).collect()
        };
        let mut lanes_after = lanes_before
            .iter()
            .copied()
            .filter(|active_lane| *active_lane != lane)
            .chain(parents.iter().map(|(_, parent_lane)| *parent_lane))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        lanes_after.sort_unstable();
        GitGraphRow {
            oid,
            short_oid,
            subject: subject.into(),
            author: "Workdeck Agent".into(),
            timestamp,
            lane,
            lanes_before,
            lanes_after,
            edges: parents
                .into_iter()
                .map(|(parent, to_lane)| GitGraphEdge {
                    from_lane: lane,
                    to_lane,
                    parent_oid: full_oid(parent),
                })
                .collect(),
            references: if head {
                vec!["feat/opportunities".into()]
            } else if wip {
                vec!["Working tree".into()]
            } else {
                Vec::new()
            },
            head,
            wip,
            additions: if wip { 0 } else { 84 },
            deletions: if wip { 0 } else { 12 },
            files_changed: usize::from(wip) * 6,
        }
    }

    fn full_oid(short: &str) -> String {
        format!("{short:0<40}")
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FixtureNode {
        Commit,
        Head,
        Wip,
    }

    pub fn search(snapshot: &BootstrapSnapshot, query: &str) -> SearchSnapshot {
        let needle = query.trim().to_ascii_lowercase();
        if needle.is_empty() {
            return SearchSnapshot {
                query: query.into(),
                groups: Vec::new(),
                total: 0,
                truncated: false,
            };
        }
        let mut groups = Vec::new();
        let projects = snapshot
            .portfolio
            .projects
            .iter()
            .filter(|project| project.name.to_ascii_lowercase().contains(&needle))
            .map(|project| SearchResult {
                id: project.id.0.clone(),
                title: project.name.clone(),
                subtitle: format!("{} repositories", project.repositories.len()),
                metadata: format!("{} need attention", project.attention),
                target: format!("workspace/project/{}", project.id),
            })
            .collect::<Vec<_>>();
        if !projects.is_empty() {
            groups.push(SearchResultGroup {
                kind: SearchResultKind::Project,
                label: "Projects".into(),
                results: projects,
            });
        }
        let changes = snapshot
            .reviews
            .iter()
            .filter(|review| {
                review.title.to_ascii_lowercase().contains(&needle)
                    || review.project.to_ascii_lowercase().contains(&needle)
            })
            .map(|review| SearchResult {
                id: review.id.0.clone(),
                title: review.title.clone(),
                subtitle: format!("{} / {}", review.project, review.repository),
                metadata: format!("{} changed items", review.total),
                target: format!("changes/{}", review.id),
            })
            .collect::<Vec<_>>();
        if !changes.is_empty() {
            groups.push(SearchResultGroup {
                kind: SearchResultKind::Review,
                label: "Changes".into(),
                results: changes,
            });
        }
        let pull_requests = snapshot
            .pull_requests
            .iter()
            .filter(|pull| {
                pull.title.to_ascii_lowercase().contains(&needle)
                    || pull.repository.to_ascii_lowercase().contains(&needle)
            })
            .map(|pull| SearchResult {
                id: format!("pr-{}", pull.number),
                title: format!("#{} {}", pull.number, pull.title),
                subtitle: pull.repository.clone(),
                metadata: format!("{} files", pull.files),
                target: format!("pull-request/{}", pull.number),
            })
            .collect::<Vec<_>>();
        if !pull_requests.is_empty() {
            groups.push(SearchResultGroup {
                kind: SearchResultKind::PullRequest,
                label: "Pull requests".into(),
                results: pull_requests,
            });
        }
        let total = groups.iter().map(|group| group.results.len()).sum();
        SearchSnapshot {
            query: query.into(),
            groups,
            total,
            truncated: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingTransport {
        requests: Arc<AtomicUsize>,
        events: Receiver<WorkdeckEvent>,
    }

    impl CountingTransport {
        fn new() -> (Self, Arc<AtomicUsize>) {
            let (events_tx, events) = async_channel::bounded(1);
            drop(events_tx);
            let requests = Arc::new(AtomicUsize::new(0));
            (
                Self {
                    requests: Arc::clone(&requests),
                    events,
                },
                requests,
            )
        }
    }

    impl WorkdeckTransport for CountingTransport {
        fn request(
            &self,
            request: WorkdeckRequest,
        ) -> BoxFuture<'static, Result<Envelope<WorkdeckResponse>, WorkdeckError>> {
            let sequence = self.requests.fetch_add(1, Ordering::SeqCst) + 1;
            let request_id = request.request_id().clone();
            let payload = match request {
                WorkdeckRequest::Search { query, .. } => WorkdeckResponse::Search(SearchSnapshot {
                    query,
                    groups: Vec::new(),
                    total: 0,
                    truncated: false,
                }),
                WorkdeckRequest::LoadGitGraph { .. } => {
                    WorkdeckResponse::GitGraph(fixtures::git_graph())
                }
                _ => WorkdeckResponse::Preferences(UiPreferences::default()),
            };
            Box::pin(async move {
                Ok(Envelope {
                    request_id,
                    revision: Revision(sequence as u64),
                    payload,
                })
            })
        }

        fn subscribe(&self) -> WorkdeckEventStream {
            self.events.clone()
        }

        fn cancel(&self, _operation: OperationId) {}
    }

    #[test]
    fn fixture_bootstrap_is_deterministic_and_scaled() {
        let snapshot = fixtures::polished();
        assert_eq!(snapshot.portfolio.project_count, 74);
        assert_eq!(snapshot.portfolio.repository_count, 75);
        assert_eq!(snapshot.portfolio.worktree_count, 109);
        assert_eq!(snapshot.portfolio.unavailable_count, 4);
        assert!(!snapshot.inbox.items.is_empty());
    }

    #[test]
    fn client_envelopes_requests_and_revisions() {
        let client = FixtureWorkdeckClient::polished().into_client();
        let request_id = RequestId::from("bootstrap-test");
        let result = block_on(client.request(WorkdeckRequest::Bootstrap {
            request_id: request_id.clone(),
        }))
        .expect("fixture response");
        assert_eq!(result.request_id, request_id);
        assert_eq!(result.revision, Revision(1));
        assert!(matches!(result.payload, WorkdeckResponse::Bootstrap(_)));
    }

    #[test]
    fn cached_reads_rebind_request_ids_without_repeating_transport_work() {
        let (transport, requests) = CountingTransport::new();
        let client = WorkdeckClient::new(transport);
        let first_id = RequestId::from("git-cache-first");
        let second_id = RequestId::from("git-cache-second");
        let first = block_on(client.request_cached(
            WorkdeckRequest::LoadGitGraph {
                request_id: first_id.clone(),
                worktree_id: None,
                cursor: None,
            },
            Duration::from_secs(60),
        ))
        .expect("first graph response");
        let second = block_on(client.request_cached(
            WorkdeckRequest::LoadGitGraph {
                request_id: second_id.clone(),
                worktree_id: None,
                cursor: None,
            },
            Duration::from_secs(60),
        ))
        .expect("cached graph response");

        assert_eq!(requests.load(Ordering::SeqCst), 1);
        assert_eq!(first.request_id, first_id);
        assert_eq!(second.request_id, second_id);
        assert_eq!(first.revision, second.revision);
        let peek_id = RequestId::from("git-cache-peek");
        let peek = client
            .cached_response(&WorkdeckRequest::LoadGitGraph {
                request_id: peek_id.clone(),
                worktree_id: None,
                cursor: None,
            })
            .expect("stale-while-revalidate response");
        assert_eq!(peek.request_id, peek_id);
        assert_eq!(peek.revision, first.revision);
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn identical_inflight_reads_are_coalesced() {
        let (transport, requests) = CountingTransport::new();
        let client = WorkdeckClient::new(transport);
        let first = client.request_cached(
            WorkdeckRequest::LoadGitGraph {
                request_id: RequestId::from("coalesced-first"),
                worktree_id: None,
                cursor: None,
            },
            Duration::from_secs(60),
        );
        let second = client.request_cached(
            WorkdeckRequest::LoadGitGraph {
                request_id: RequestId::from("coalesced-second"),
                worktree_id: None,
                cursor: None,
            },
            Duration::from_secs(60),
        );
        let (first, second) = block_on(futures::future::join(first, second));

        assert!(first.is_ok());
        assert!(second.is_ok());
        assert_eq!(requests.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn explicit_invalidation_forces_a_fresh_read() {
        let (transport, requests) = CountingTransport::new();
        let client = WorkdeckClient::new(transport);
        let request = WorkdeckRequest::LoadGitGraph {
            request_id: RequestId::from("invalidate-first"),
            worktree_id: Some(WorktreeId::from("worktree-workdeck")),
            cursor: None,
        };
        block_on(client.request_cached(request.clone(), Duration::from_secs(60)))
            .expect("first graph response");
        client.invalidate_cached(&request);
        block_on(client.request_cached(
            WorkdeckRequest::LoadGitGraph {
                request_id: RequestId::from("invalidate-second"),
                worktree_id: Some(WorktreeId::from("worktree-workdeck")),
                cursor: None,
            },
            Duration::from_secs(60),
        ))
        .expect("refreshed graph response");

        assert_eq!(requests.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn invalidated_inflight_response_cannot_replace_the_fresh_generation() {
        let (transport, requests) = CountingTransport::new();
        let client = WorkdeckClient::new(transport);
        let first_request = WorkdeckRequest::LoadGitGraph {
            request_id: RequestId::from("stale-generation"),
            worktree_id: None,
            cursor: None,
        };
        let stale = client.request_cached(first_request.clone(), Duration::from_secs(60));
        client.invalidate_cached(&first_request);
        let fresh = block_on(client.request_cached(
            WorkdeckRequest::LoadGitGraph {
                request_id: RequestId::from("fresh-generation"),
                worktree_id: None,
                cursor: None,
            },
            Duration::from_secs(60),
        ))
        .expect("fresh response");
        let stale = block_on(stale).expect("old caller can still finish safely");
        let retained = client
            .cached_response(&WorkdeckRequest::LoadGitGraph {
                request_id: RequestId::from("retained-generation"),
                worktree_id: None,
                cursor: None,
            })
            .expect("fresh generation remains cached");

        assert_eq!(requests.load(Ordering::SeqCst), 2);
        assert_ne!(stale.revision, fresh.revision);
        assert_eq!(retained.revision, fresh.revision);
    }

    #[test]
    fn abandoned_inflight_reads_remain_bounded() {
        let (transport, _requests) = CountingTransport::new();
        let client = WorkdeckClient::new(transport);
        for index in 0..(MAX_INFLIGHT_CACHE_ENTRIES + 12) {
            drop(client.request_cached(
                WorkdeckRequest::Search {
                    request_id: RequestId::new(),
                    query: format!("abandoned-{index}"),
                },
                Duration::from_secs(60),
            ));
        }

        assert_eq!(
            client.cache.lock().expect("cache lock").inflight.len(),
            MAX_INFLIGHT_CACHE_ENTRIES
        );
    }

    #[test]
    fn response_cache_remains_bounded() {
        let (transport, _requests) = CountingTransport::new();
        let client = WorkdeckClient::new(transport);
        for index in 0..(MAX_RESPONSE_CACHE_ENTRIES + 12) {
            block_on(client.request_cached(
                WorkdeckRequest::Search {
                    request_id: RequestId::new(),
                    query: format!("query-{index}"),
                },
                Duration::from_secs(60),
            ))
            .expect("search response");
        }

        assert_eq!(
            client.cache.lock().expect("cache lock").responses.len(),
            MAX_RESPONSE_CACHE_ENTRIES
        );
    }

    #[test]
    fn activity_read_cursor_clears_only_the_matching_revision() {
        let client = FixtureWorkdeckClient::polished().into_client();
        let snapshot = fixtures::polished();
        let item = snapshot.inbox.items[0].clone();
        let response = block_on(client.request(WorkdeckRequest::MarkActivityRead {
            request_id: RequestId::from("mark-read"),
            target: item.target.clone(),
            revision: item.revision.clone(),
        }))
        .expect("mark read");
        assert!(matches!(
            response.payload,
            WorkdeckResponse::ActivityRead(ActivityReadState { unread: false, .. })
        ));
        let refreshed = block_on(client.request(WorkdeckRequest::Bootstrap {
            request_id: RequestId::from("after-read"),
        }))
        .expect("bootstrap");
        let WorkdeckResponse::Bootstrap(snapshot) = refreshed.payload else {
            panic!("expected bootstrap");
        };
        assert_eq!(snapshot.inbox.unread, 2);
        assert!(
            snapshot
                .inbox
                .items
                .iter()
                .find(|candidate| candidate.id == item.id)
                .is_some_and(|candidate| !candidate.unread)
        );
    }

    #[test]
    fn preference_widths_are_bounded() {
        let mut preferences = UiPreferences::default();
        preferences.apply(UiPreferencePatch {
            navigator_width: Some(1.0),
            inspector_width: Some(10_000.0),
            pane_widths: Some(PaneWidths {
                master_list: 1.0,
                git_references: 10_000.0,
                git_detail: 1.0,
                review_tree: 10_000.0,
                review_structure: 1.0,
                artifact_library: 10_000.0,
            }),
            ..UiPreferencePatch::default()
        });
        assert_eq!(preferences.navigator_width, 208.0);
        assert_eq!(preferences.inspector_width, 720.0);
        assert_eq!(preferences.pane_widths.master_list, 300.0);
        assert_eq!(preferences.pane_widths.git_references, 360.0);
        assert_eq!(preferences.pane_widths.git_detail, 280.0);
        assert_eq!(preferences.pane_widths.review_tree, 480.0);
        assert_eq!(preferences.pane_widths.review_structure, 220.0);
        assert_eq!(preferences.pane_widths.artifact_library, 520.0);
    }

    #[test]
    fn search_groups_relevant_results() {
        let snapshot = fixtures::polished();
        let search = fixtures::search(&snapshot, "sampleapp");
        assert!(search.total >= 2);
        assert!(
            search
                .groups
                .iter()
                .any(|group| group.kind == SearchResultKind::Project)
        );
        assert!(
            search
                .groups
                .iter()
                .any(|group| group.kind == SearchResultKind::Review)
        );
    }

    #[test]
    fn polished_review_fixture_exercises_semantic_syntax_rendering() {
        let review =
            fixtures::review(&ReviewId::from("review-sampleapp")).expect("polished fixture review");
        let spans = review
            .units
            .iter()
            .flat_map(|unit| unit.diff.iter())
            .flat_map(|line| line.spans.iter())
            .collect::<Vec<_>>();

        for token in ["keyword", "type", "function", "parameter", "string"] {
            assert!(
                spans.iter().any(|span| span.token == token),
                "missing fixture token {token}"
            );
        }
        assert!(review.units.iter().flat_map(|unit| &unit.diff).all(|line| {
            line.spans.iter().all(|span| {
                span.start < span.end
                    && span.end <= line.text.len()
                    && line.text.is_char_boundary(span.start)
                    && line.text.is_char_boundary(span.end)
            })
        }));
    }

    #[test]
    fn git_fixture_preserves_topology_across_the_pagination_boundary() {
        let first = fixtures::git_graph();
        let second = fixtures::older_git_graph();
        assert!(first.rows[0].wip);
        assert_eq!(first.rows[0].short_oid, "WIP");
        assert_eq!(first.rows[0].files_changed, 6);
        assert!(first.rows.iter().any(|row| row.edges.len() == 2));
        assert!(
            first
                .rows
                .windows(2)
                .all(|rows| { rows[0].lanes_after == rows[1].lanes_before })
        );
        assert_eq!(
            first.rows.last().unwrap().lanes_after,
            second.rows.first().unwrap().lanes_before
        );
        assert!(
            second
                .rows
                .windows(2)
                .all(|rows| { rows[0].lanes_after == rows[1].lanes_before })
        );
        assert!(first.rows.iter().chain(second.rows.iter()).all(|row| {
            row.edges
                .iter()
                .all(|edge| edge.from_lane == row.lane && row.lanes_after.contains(&edge.to_lane))
        }));
    }

    #[test]
    fn fixture_client_returns_unique_bounded_git_pages() {
        let client = FixtureWorkdeckClient::polished().into_client();
        let first = block_on(client.request(WorkdeckRequest::LoadGitGraph {
            request_id: RequestId::from("git-first"),
            worktree_id: None,
            cursor: None,
        }))
        .unwrap();
        let second = block_on(client.request(WorkdeckRequest::LoadGitGraph {
            request_id: RequestId::from("git-second"),
            worktree_id: None,
            cursor: Some("page-2".into()),
        }))
        .unwrap();
        let WorkdeckResponse::GitGraph(first) = first.payload else {
            panic!("expected first Git page");
        };
        let WorkdeckResponse::GitGraph(second) = second.payload else {
            panic!("expected second Git page");
        };
        assert!(first.has_more);
        assert!(!second.has_more);
        assert!(
            first
                .rows
                .iter()
                .all(|left| { second.rows.iter().all(|right| left.oid != right.oid) })
        );
    }
}
