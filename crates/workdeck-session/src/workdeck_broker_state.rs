//! Workdeck's review-aware specialization of the transport-neutral session broker.

use crate::{
    BrokerCommandOutcome, BrokerSessionCommentFilter, BrokerSessionReviewOptions,
    DaemonSessionSocket, DispatchSessionCommand, HandleCommandResult, ListedSession,
    MarkSessionSeenResult, MirroredReviewPublication, ObserveReviewPublicationInput,
    PendingCommandResult, ReadReviewResourceToolInput, RegisterSessionOptions,
    RegisterSessionResult, ReviewMirror, ReviewMirrorUpdate, ReviewResourceCache,
    ReviewResourceKey, SessionBrokerEntry, SessionBrokerLimitOptions, SessionBrokerListedSession,
    SessionBrokerState, SessionBrokerStateError, SessionBrokerViewAdapter, SessionCommentFilter,
    SessionLiveCommentSummary, SessionReview, SessionReviewFile, SessionReviewOptions,
    SessionSelector, SharedDaemonSessionSocket, UpdateSnapshotResult,
    WorkdeckReviewActionEnvelopeV1, WorkdeckReviewActionResultV1, WorkdeckReviewActionV1,
    WorkdeckReviewActorKindV1, WorkdeckReviewActorV1, WorkdeckReviewFailureCodeV1,
    WorkdeckReviewResourceReadEnvelopeV1, WorkdeckReviewResourceReadResultV1,
    WorkdeckSessionCommandResult, WorkdeckSessionInfo, WorkdeckSessionRegistration,
    WorkdeckSessionSnapshot, WorkdeckSessionState, build_listed_workdeck_session,
    build_selected_workdeck_session_context, build_workdeck_session_review,
    create_workdeck_session_protocol_parsers, list_workdeck_session_comments,
    parse_workdeck_session_registration, parse_workdeck_session_snapshot,
    read_registration_review_catalog, read_snapshot_review_publication,
};
use base64::Engine as _;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use workdeck_review::{
    ExpectedReviewResource, REVIEW_RESOURCE_CHUNK_BYTES, REVIEW_RESOURCE_LOAD_CONCURRENCY,
    ReadReviewResourceRequest, ReviewAssemblyResult, ReviewAssemblyStep, ReviewChunkAssembler,
    ReviewChunkAssemblerOptions, ReviewPublicationAddress, ReviewResourceAddress,
    ReviewResourceDescriptor, ReviewResourceKind, review_resource_ceiling, review_resource_id,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewGenerationRetiredError {
    pub current_generation: Option<String>,
}

impl fmt::Display for ReviewGenerationRetiredError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.current_generation {
            Some(generation) => write!(
                formatter,
                "The review generation retired; the session is now serving {generation}."
            ),
            None => formatter
                .write_str("The review generation retired and the session is no longer connected."),
        }
    }
}

impl std::error::Error for ReviewGenerationRetiredError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewResourceReadError {
    pub code: WorkdeckReviewFailureCodeV1,
    pub message: String,
}

impl fmt::Display for ReviewResourceReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ReviewResourceReadError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkdeckSessionBrokerError {
    Broker(SessionBrokerStateError),
    GenerationRetired(ReviewGenerationRetiredError),
    ResourceRead(ReviewResourceReadError),
    Message(String),
}

impl fmt::Display for WorkdeckSessionBrokerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Broker(error) => error.fmt(formatter),
            Self::GenerationRetired(error) => error.fmt(formatter),
            Self::ResourceRead(error) => error.fmt(formatter),
            Self::Message(message) => formatter.write_str(message),
        }
    }
}

impl std::error::Error for WorkdeckSessionBrokerError {}

impl From<SessionBrokerStateError> for WorkdeckSessionBrokerError {
    fn from(error: SessionBrokerStateError) -> Self {
        Self::Broker(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewPublicationEvent {
    Published { session_id: String },
    Retired { session_id: String },
}

impl ReviewPublicationEvent {
    #[must_use]
    pub fn session_id(&self) -> &str {
        match self {
            Self::Published { session_id } | Self::Retired { session_id } => session_id,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReviewResourceUsage {
    pub cached_bytes: usize,
    pub reserved_bytes: usize,
    pub entry_count: usize,
}

type PublicationWatcher = Arc<dyn Fn(ReviewPublicationEvent) + Send + Sync>;

pub struct ReviewPublicationSubscription {
    id: u64,
    watchers: Weak<Mutex<BTreeMap<u64, PublicationWatcher>>>,
}

impl Drop for ReviewPublicationSubscription {
    fn drop(&mut self) {
        if let Some(watchers) = self.watchers.upgrade() {
            watchers
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .remove(&self.id);
        }
    }
}

struct WorkdeckBrokerView {
    parsers: crate::WorkdeckSessionProtocolParsers,
}

impl WorkdeckBrokerView {
    fn new() -> Self {
        Self {
            parsers: create_workdeck_session_protocol_parsers()
                .expect("the immutable Workdeck broker parser registry is valid"),
        }
    }
}

impl SessionBrokerListedSession for ListedSession {
    fn selectable_session(&self) -> crate::SelectableSession {
        crate::SelectableSession {
            session_id: self.session_id.clone(),
            cwd: PathBuf::from(&self.cwd),
            repo_root: self.repo_root.as_deref().map(PathBuf::from),
        }
    }

    fn title(&self) -> &str {
        &self.title
    }

    fn snapshot_updated_at(&self) -> &str {
        &self.snapshot.updated_at
    }
}

impl SessionBrokerViewAdapter for WorkdeckBrokerView {
    type Info = WorkdeckSessionInfo;
    type State = WorkdeckSessionState;
    type CommandInput = Value;
    type CommandResult = WorkdeckSessionCommandResult;
    type ListedSession = ListedSession;
    type SelectedContext = crate::SelectedSessionContext;
    type SessionReview = SessionReview;
    type SessionCommentSummary = SessionLiveCommentSummary;

    fn parse_registration(&self, value: &Value) -> Option<WorkdeckSessionRegistration> {
        parse_workdeck_session_registration(value)
    }

    fn parse_snapshot(&self, value: &Value) -> Option<WorkdeckSessionSnapshot> {
        parse_workdeck_session_snapshot(value)
    }

    fn parse_command_input(&self, command: &str, version: u64, value: &Value) -> Option<Value> {
        self.parsers
            .parse_command_input(command, version, value)
            .ok()
            .map(|_| value.clone())
    }

    fn parse_command_result(
        &self,
        command: &str,
        version: u64,
        value: &Value,
    ) -> Option<WorkdeckSessionCommandResult> {
        self.parsers
            .parse_command_result(command, version, value)
            .ok()
    }

    fn build_listed_session(
        &self,
        entry: &SessionBrokerEntry<WorkdeckSessionInfo, WorkdeckSessionState>,
    ) -> ListedSession {
        build_listed_workdeck_session(&crate::WorkdeckSessionEntry {
            registration: entry.registration.clone(),
            snapshot: entry.snapshot.clone(),
        })
    }

    fn build_selected_context(&self, session: &ListedSession) -> crate::SelectedSessionContext {
        build_selected_workdeck_session_context(session)
    }

    fn build_session_review(
        &self,
        entry: &SessionBrokerEntry<WorkdeckSessionInfo, WorkdeckSessionState>,
        options: BrokerSessionReviewOptions,
    ) -> SessionReview {
        build_workdeck_session_review(
            &crate::WorkdeckSessionEntry {
                registration: entry.registration.clone(),
                snapshot: entry.snapshot.clone(),
            },
            SessionReviewOptions {
                include_patch: options.include_patch,
                include_notes: options.include_notes,
            },
        )
    }

    fn list_comments(
        &self,
        session: &ListedSession,
        filter: BrokerSessionCommentFilter,
    ) -> Vec<SessionLiveCommentSummary> {
        list_workdeck_session_comments(
            session,
            SessionCommentFilter {
                file_path: filter.file_path.as_deref(),
            },
        )
    }
}

struct ResourceLoad {
    result: Mutex<Option<Result<Arc<[u8]>, WorkdeckSessionBrokerError>>>,
    ready: Condvar,
}

impl ResourceLoad {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn complete(&self, result: Result<Arc<[u8]>, WorkdeckSessionBrokerError>) {
        *self
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(result);
        self.ready.notify_all();
    }

    fn wait(&self) -> Result<Arc<[u8]>, WorkdeckSessionBrokerError> {
        let mut result = self
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        loop {
            if let Some(result) = &*result {
                return result.clone();
            }
            result = self
                .ready
                .wait(result)
                .unwrap_or_else(|error| error.into_inner());
        }
    }
}

/// Generic broker state specialized with Workdeck's review mirror and resource path.
pub struct WorkdeckSessionBrokerState {
    core: SessionBrokerState<WorkdeckBrokerView>,
    mirror: Mutex<ReviewMirror>,
    resources: Mutex<ReviewResourceCache>,
    loads: Mutex<BTreeMap<String, Arc<ResourceLoad>>>,
    capability_digests: Mutex<BTreeMap<String, String>>,
    publication_watchers: Arc<Mutex<BTreeMap<u64, PublicationWatcher>>>,
    next_watcher_id: AtomicU64,
}

impl Default for WorkdeckSessionBrokerState {
    fn default() -> Self {
        Self::new(ReviewResourceCache::default())
    }
}

impl WorkdeckSessionBrokerState {
    #[must_use]
    pub fn new(resources: ReviewResourceCache) -> Self {
        Self::with_options(resources, &SessionBrokerLimitOptions::default())
            .expect("the default broker limits are valid")
    }

    pub fn with_options(
        resources: ReviewResourceCache,
        limit_options: &SessionBrokerLimitOptions,
    ) -> Result<Self, crate::BrokerLimitError> {
        Ok(Self {
            core: SessionBrokerState::new(WorkdeckBrokerView::new(), limit_options)?,
            mirror: Mutex::new(ReviewMirror::new()),
            resources: Mutex::new(resources),
            loads: Mutex::new(BTreeMap::new()),
            capability_digests: Mutex::new(BTreeMap::new()),
            publication_watchers: Arc::new(Mutex::new(BTreeMap::new())),
            next_watcher_id: AtomicU64::new(1),
        })
    }

    #[must_use]
    pub fn list_sessions(&self) -> Vec<ListedSession> {
        self.core.list_sessions()
    }

    pub fn get_session(
        &self,
        selector: &SessionSelector,
    ) -> Result<ListedSession, SessionBrokerStateError> {
        self.core.get_session(selector)
    }

    pub fn get_selected_context(
        &self,
        selector: &SessionSelector,
    ) -> Result<crate::SelectedSessionContext, SessionBrokerStateError> {
        self.core.get_selected_context(selector)
    }

    pub fn list_comments(
        &self,
        selector: &SessionSelector,
        file_path: Option<String>,
    ) -> Result<Vec<SessionLiveCommentSummary>, SessionBrokerStateError> {
        self.core
            .list_comments(selector, BrokerSessionCommentFilter { file_path })
    }

    #[must_use]
    pub fn session_count(&self) -> usize {
        self.core.session_count()
    }

    #[must_use]
    pub fn pending_command_count(&self) -> usize {
        self.core.pending_command_count()
    }

    pub fn register_session(
        &self,
        socket: SharedDaemonSessionSocket,
        registration_input: &Value,
        snapshot_input: &Value,
        options: RegisterSessionOptions,
    ) -> RegisterSessionResult {
        let parsed_registration = parse_workdeck_session_registration(registration_input);
        let registered =
            self.core
                .register_session(socket, registration_input, snapshot_input, options);
        self.reconcile_mirrored_sessions();
        if registered != RegisterSessionResult::Registered {
            return registered;
        }
        let Some(registration) = parsed_registration else {
            return registered;
        };
        let session_id = registration.session_id;
        let mut digests = self
            .capability_digests
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        match registration.info.review_capability_digest {
            Some(digest) => {
                digests.insert(session_id.clone(), digest);
            }
            None => {
                digests.remove(&session_id);
            }
        }
        drop(digests);
        self.observe_publication(
            &session_id,
            read_registration_review_catalog(registration_input),
            read_snapshot_review_publication(snapshot_input),
        );
        registered
    }

    pub fn update_snapshot(
        &self,
        socket: &SharedDaemonSessionSocket,
        session_id: &str,
        snapshot_input: &Value,
    ) -> UpdateSnapshotResult {
        let result = self
            .core
            .update_snapshot(socket, session_id, snapshot_input);
        if result == UpdateSnapshotResult::Updated {
            let catalog = self
                .mirror
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .get(session_id)
                .map(|publication| publication.catalog.clone());
            let address = read_snapshot_review_publication(snapshot_input);
            self.observe_publication(session_id, catalog, address);
        }
        result
    }

    pub fn mark_session_seen(
        &self,
        socket: &SharedDaemonSessionSocket,
        session_id: &str,
    ) -> MarkSessionSeenResult {
        self.core.mark_session_seen(socket, session_id)
    }

    pub fn unregister_socket(&self, socket: &SharedDaemonSessionSocket) {
        self.core.unregister_socket(socket);
        self.reconcile_mirrored_sessions();
    }

    pub fn prune_stale_sessions(&self, ttl_ms: u64, now_ms: Option<i64>) -> usize {
        let removed = self.core.prune_stale_sessions(ttl_ms, now_ms);
        if removed > 0 {
            self.reconcile_mirrored_sessions();
        }
        removed
    }

    pub fn dispatch_command(
        &self,
        request: DispatchSessionCommand,
    ) -> Result<PendingCommandResult<WorkdeckSessionCommandResult>, SessionBrokerStateError> {
        self.core.dispatch_command(request)
    }

    pub fn handle_command_result(
        &self,
        socket: &SharedDaemonSessionSocket,
        request_id: &str,
        outcome: BrokerCommandOutcome,
    ) -> HandleCommandResult {
        self.core.handle_command_result(socket, request_id, outcome)
    }

    pub fn shutdown(&self, error: Option<SessionBrokerStateError>) {
        let watched = self
            .mirror
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .session_ids();
        self.core.shutdown(error);
        self.mirror
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        self.resources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        self.loads
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        self.capability_digests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
        for session_id in watched {
            self.notify_publication_watchers(ReviewPublicationEvent::Retired { session_id });
        }
        self.publication_watchers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .clear();
    }

    #[must_use]
    pub fn get_review_publication(&self, session_id: &str) -> Option<MirroredReviewPublication> {
        self.mirror
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(session_id)
            .cloned()
    }

    #[must_use]
    pub fn get_review_capability_digest(&self, session_id: &str) -> Option<String> {
        self.capability_digests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(session_id)
            .cloned()
    }

    pub fn subscribe_review_publications(
        &self,
        watcher: impl Fn(ReviewPublicationEvent) + Send + Sync + 'static,
    ) -> ReviewPublicationSubscription {
        let id = self.next_watcher_id.fetch_add(1, Ordering::Relaxed);
        self.publication_watchers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(id, Arc::new(watcher));
        ReviewPublicationSubscription {
            id,
            watchers: Arc::downgrade(&self.publication_watchers),
        }
    }

    #[must_use]
    pub fn get_review_resource_usage(&self) -> ReviewResourceUsage {
        let resources = self
            .resources
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        ReviewResourceUsage {
            cached_bytes: resources.cached_bytes(),
            reserved_bytes: resources.reserved_bytes(),
            entry_count: resources.entry_count(),
        }
    }

    pub fn get_session_review_with_resources(
        &self,
        selector: &SessionSelector,
        options: BrokerSessionReviewOptions,
    ) -> Result<SessionReview, WorkdeckSessionBrokerError> {
        let mut review = self.core.get_session_review(selector, options)?;
        let publication = options
            .include_patch
            .then(|| self.get_review_publication(&review.session_id))
            .flatten();
        let Some(publication) = publication else {
            return Ok(review);
        };
        let pending = review
            .files
            .iter()
            .filter(|file| file.patch.is_none())
            .cloned()
            .collect::<Vec<_>>();
        let loaded = self.load_file_patches(&review.session_id, &publication, &pending)?;
        let patch_by_file_id = pending
            .iter()
            .zip(loaded)
            .map(|(file, patch)| (file.summary.id.clone(), patch))
            .collect::<BTreeMap<_, _>>();
        for file in &mut review.files {
            if let Some(patch) = patch_by_file_id.get(&file.summary.id) {
                file.patch = Some(patch.clone());
            }
        }
        if let Some(selected) = &mut review.selected_file
            && let Some(updated) = review
                .files
                .iter()
                .find(|file| file.summary.id == selected.summary.id)
        {
            *selected = updated.clone();
        }
        Ok(review)
    }

    pub fn load_review_resource(
        &self,
        session_id: &str,
        generation: &str,
        resource_id: &str,
    ) -> Result<Arc<[u8]>, WorkdeckSessionBrokerError> {
        let descriptor = self.require_descriptor(session_id, generation, resource_id)?;
        let key = ReviewResourceKey {
            session_id: session_id.into(),
            generation: generation.into(),
            resource_id: resource_id.into(),
        };
        if let Some(cached) = self
            .resources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .get(&key)
        {
            return Ok(cached);
        }
        let load_key = serde_json::to_string(&(session_id, generation, resource_id))
            .expect("resource load keys serialize");
        let (load, leader) = {
            let mut loads = self.loads.lock().unwrap_or_else(|error| error.into_inner());
            if let Some(load) = loads.get(&load_key) {
                (Arc::clone(load), false)
            } else {
                let load = Arc::new(ResourceLoad::new());
                loads.insert(load_key.clone(), Arc::clone(&load));
                (load, true)
            }
        };
        if !leader {
            return load.wait();
        }
        let result = self.assemble_resource(key, &descriptor);
        load.complete(result.clone());
        self.loads
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&load_key);
        result
    }

    pub fn apply_review_action(
        &self,
        session_id: &str,
        generation: &str,
        action: WorkdeckReviewActionV1,
        actor: Option<WorkdeckReviewActorV1>,
        expected_state_revision: Option<u64>,
    ) -> Result<WorkdeckReviewActionResultV1, WorkdeckSessionBrokerError> {
        self.assert_generation_active(session_id, generation)?;
        let input = crate::ApplyReviewActionToolInput {
            target_session: SessionSelector {
                session_id: Some(session_id.into()),
                ..SessionSelector::default()
            },
            review: WorkdeckReviewActionEnvelopeV1 {
                protocol_version: crate::WORKDECK_REVIEW_PROTOCOL_VERSION,
                generation: generation.into(),
                expected_state_revision,
                actor: actor.unwrap_or_else(daemon_actor),
                action,
            },
        };
        let pending = self.dispatch_command(DispatchSessionCommand::new(
            selector_for(session_id),
            "apply_review_action",
            serde_json::to_value(input).expect("review action input serializes"),
            "Timed out waiting for the session to apply the review action.",
        ))?;
        match pending.receive()? {
            WorkdeckSessionCommandResult::ReviewAction(result) => Ok(result),
            _ => Err(WorkdeckSessionBrokerError::Message(
                "The session returned the wrong review action result.".into(),
            )),
        }
    }

    fn load_file_patches(
        &self,
        session_id: &str,
        publication: &MirroredReviewPublication,
        files: &[SessionReviewFile],
    ) -> Result<Vec<String>, WorkdeckSessionBrokerError> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        let next = AtomicUsize::new(0);
        let results = Mutex::new(
            (0..files.len())
                .map(|_| None)
                .collect::<Vec<Option<Result<String, WorkdeckSessionBrokerError>>>>(),
        );
        std::thread::scope(|scope| {
            for _ in 0..REVIEW_RESOURCE_LOAD_CONCURRENCY.min(files.len()) {
                scope.spawn(|| {
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(file) = files.get(index) else {
                            break;
                        };
                        let result = self.load_file_patch(session_id, publication, file);
                        results.lock().unwrap_or_else(|error| error.into_inner())[index] =
                            Some(result);
                    }
                });
            }
        });
        results
            .into_inner()
            .unwrap_or_else(|error| error.into_inner())
            .into_iter()
            .map(|result| result.expect("every patch worker records its result"))
            .collect()
    }

    fn load_file_patch(
        &self,
        session_id: &str,
        publication: &MirroredReviewPublication,
        file: &SessionReviewFile,
    ) -> Result<String, WorkdeckSessionBrokerError> {
        let file_key = publication
            .catalog
            .file_keys_by_runtime_id
            .get(&file.summary.id)
            .ok_or_else(|| {
                WorkdeckSessionBrokerError::Message(format!(
                    "Could not read the raw diff for {} from the live session.",
                    file.summary.path
                ))
            })?;
        let resource_id = review_resource_id(&ReviewResourceAddress {
            kind: ReviewResourceKind::Patch,
            file_key: file_key.clone(),
            side: None,
        });
        let bytes =
            self.load_review_resource(session_id, &publication.address.generation, &resource_id)?;
        String::from_utf8(bytes.to_vec()).map_err(|_| {
            WorkdeckSessionBrokerError::Message(format!(
                "Review resource {resource_id} is not valid UTF-8."
            ))
        })
    }

    fn observe_publication(
        &self,
        session_id: &str,
        catalog: Option<crate::WorkdeckReviewResourceCatalogV1>,
        address: Option<ReviewPublicationAddress>,
    ) {
        let update = self
            .mirror
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .observe(ObserveReviewPublicationInput {
                session_id,
                catalog: catalog.as_ref(),
                address: address.as_ref(),
            });
        if let ReviewMirrorUpdate::Replaced {
            previous_generation,
            ..
        } = &update
        {
            self.resources
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .evict_generation(session_id, previous_generation);
        }
        if update != ReviewMirrorUpdate::Ignored {
            self.notify_publication_watchers(ReviewPublicationEvent::Published {
                session_id: session_id.into(),
            });
        }
    }

    fn reconcile_mirrored_sessions(&self) {
        let live = self
            .list_sessions()
            .into_iter()
            .map(|session| session.session_id)
            .collect::<BTreeSet<_>>();
        self.capability_digests
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .retain(|session_id, _| live.contains(session_id));
        let mirrored = self
            .mirror
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .session_ids();
        for session_id in mirrored {
            if live.contains(&session_id) {
                continue;
            }
            self.mirror
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .forget(&session_id);
            self.resources
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .evict_session(&session_id);
            self.notify_publication_watchers(ReviewPublicationEvent::Retired { session_id });
        }
    }

    fn notify_publication_watchers(&self, event: ReviewPublicationEvent) {
        let watchers = self
            .publication_watchers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .values()
            .cloned()
            .collect::<Vec<_>>();
        for watcher in watchers {
            let event = event.clone();
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| watcher(event)));
        }
    }

    fn require_descriptor(
        &self,
        session_id: &str,
        generation: &str,
        resource_id: &str,
    ) -> Result<ReviewResourceDescriptor, WorkdeckSessionBrokerError> {
        let publication = self.assert_generation_active(session_id, generation)?;
        publication
            .catalog
            .resources
            .iter()
            .find(|resource| resource.base().id == resource_id)
            .cloned()
            .ok_or_else(|| {
                WorkdeckSessionBrokerError::ResourceRead(ReviewResourceReadError {
                    code: WorkdeckReviewFailureCodeV1::UnknownResource,
                    message: format!(
                        "Review resource {resource_id} is not part of generation {generation}."
                    ),
                })
            })
    }

    fn assert_generation_active(
        &self,
        session_id: &str,
        generation: &str,
    ) -> Result<MirroredReviewPublication, WorkdeckSessionBrokerError> {
        let publication = self.get_review_publication(session_id);
        if publication
            .as_ref()
            .is_none_or(|publication| publication.address.generation != generation)
        {
            return Err(WorkdeckSessionBrokerError::GenerationRetired(
                ReviewGenerationRetiredError {
                    current_generation: publication
                        .as_ref()
                        .map(|publication| publication.address.generation.clone()),
                },
            ));
        }
        Ok(publication.expect("active publication was checked"))
    }

    fn assemble_resource(
        &self,
        key: ReviewResourceKey,
        descriptor: &ReviewResourceDescriptor,
    ) -> Result<Arc<[u8]>, WorkdeckSessionBrokerError> {
        let kind = descriptor_kind(descriptor);
        let base = descriptor.base();
        let measured = descriptor.is_materialized();
        let initial_bytes = if measured {
            base.byte_length.expect("materialized length")
        } else {
            REVIEW_RESOURCE_CHUNK_BYTES.min(review_resource_ceiling(kind))
        };
        let mut reservation = self
            .resources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .reserve(
                key.clone(),
                usize::try_from(initial_bytes).unwrap_or(usize::MAX),
            )
            .map_err(|error| WorkdeckSessionBrokerError::Message(error.to_string()))?;
        let result = (|| {
            let mut assembler = ReviewChunkAssembler::new(ReviewChunkAssemblerOptions {
                resource_id: key.resource_id.clone(),
                generation: key.generation.clone(),
                digest: Arc::new(workdeck_core::review_digest),
                max_bytes: review_resource_ceiling(kind),
                expected: measured.then(|| ExpectedReviewResource {
                    byte_length: base.byte_length.expect("materialized length"),
                    digest: base.digest.clone().expect("materialized digest"),
                }),
            });
            loop {
                self.assert_generation_active(&key.session_id, &key.generation)?;
                let result = self.read_chunk(&key, assembler.next_offset())?;
                let chunk = match result {
                    WorkdeckReviewResourceReadResultV1::Chunk { chunk, .. } => chunk,
                    WorkdeckReviewResourceReadResultV1::Failed(failure) => {
                        return Err(WorkdeckSessionBrokerError::ResourceRead(
                            ReviewResourceReadError {
                                code: failure.code,
                                message: failure.message,
                            },
                        ));
                    }
                };
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(&chunk.data)
                    .map_err(|_| {
                        WorkdeckSessionBrokerError::ResourceRead(ReviewResourceReadError {
                            code: WorkdeckReviewFailureCodeV1::ResourceIntegrity,
                            message: format!(
                                "Review resource {} returned invalid base64 data.",
                                key.resource_id
                            ),
                        })
                    })?;
                let done = match assembler.accept(&chunk, &bytes) {
                    ReviewAssemblyStep::Accepted { done } => done,
                    ReviewAssemblyStep::Failed(failure) => {
                        return Err(WorkdeckSessionBrokerError::ResourceRead(
                            ReviewResourceReadError {
                                code: failure.code.into(),
                                message: failure.message,
                            },
                        ));
                    }
                };
                let declared = assembler
                    .declared_size()
                    .unwrap_or(u64::try_from(reservation.byte_length).unwrap_or(u64::MAX));
                self.resources
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .resize(
                        &mut reservation,
                        usize::try_from(declared).unwrap_or(usize::MAX),
                    )
                    .map_err(|error| WorkdeckSessionBrokerError::Message(error.to_string()))?;
                if done {
                    break;
                }
            }
            let bytes = match assembler.finish() {
                ReviewAssemblyResult::Assembled { bytes } => Arc::<[u8]>::from(bytes),
                ReviewAssemblyResult::Failed(failure) => {
                    return Err(WorkdeckSessionBrokerError::ResourceRead(
                        ReviewResourceReadError {
                            code: failure.code.into(),
                            message: failure.message,
                        },
                    ));
                }
            };
            self.assert_generation_active(&key.session_id, &key.generation)?;
            self.resources
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .store(key.clone(), Arc::clone(&bytes));
            Ok(bytes)
        })();
        self.resources
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .release(&reservation);
        result
    }

    fn read_chunk(
        &self,
        key: &ReviewResourceKey,
        offset: u64,
    ) -> Result<WorkdeckReviewResourceReadResultV1, WorkdeckSessionBrokerError> {
        let input = ReadReviewResourceToolInput {
            target_session: selector_for(&key.session_id),
            review: WorkdeckReviewResourceReadEnvelopeV1 {
                protocol_version: crate::WORKDECK_REVIEW_PROTOCOL_VERSION,
                actor: daemon_actor(),
                request: ReadReviewResourceRequest {
                    generation: key.generation.clone(),
                    resource_id: key.resource_id.clone(),
                    offset,
                    length: REVIEW_RESOURCE_CHUNK_BYTES,
                },
            },
        };
        let mut request = DispatchSessionCommand::new(
            selector_for(&key.session_id),
            "read_review_resource",
            serde_json::to_value(input).expect("review read input serializes"),
            "Timed out reading a review resource from the session.",
        );
        request.timeout_ms = Some(30_000);
        match self.dispatch_command(request)?.receive()? {
            WorkdeckSessionCommandResult::ReviewResource(result) => Ok(result),
            _ => Err(WorkdeckSessionBrokerError::Message(
                "The session returned the wrong review resource result.".into(),
            )),
        }
    }
}

fn selector_for(session_id: &str) -> SessionSelector {
    SessionSelector {
        session_id: Some(session_id.into()),
        ..SessionSelector::default()
    }
}

fn daemon_actor() -> WorkdeckReviewActorV1 {
    WorkdeckReviewActorV1 {
        client_id: "workdeck-daemon".into(),
        kind: WorkdeckReviewActorKindV1::Agent,
        display_name: None,
    }
}

const fn descriptor_kind(descriptor: &ReviewResourceDescriptor) -> ReviewResourceKind {
    match descriptor {
        ReviewResourceDescriptor::CanonicalFile { .. } => ReviewResourceKind::CanonicalFile,
        ReviewResourceDescriptor::Patch { .. } => ReviewResourceKind::Patch,
        ReviewResourceDescriptor::Source { .. } => ReviewResourceKind::Source,
    }
}

/// Wire the generic broker core to Workdeck's concrete session and review models.
#[must_use]
pub fn create_workdeck_session_broker_state() -> WorkdeckSessionBrokerState {
    WorkdeckSessionBrokerState::default()
}

// Keep the transport trait reachable from this concrete state module's public API docs.
const _: Option<&dyn DaemonSessionSocket> = None;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        SESSION_BROKER_REGISTRATION_VERSION, SessionFileSummary, SessionReviewHunk,
        WorkdeckReviewResourceCatalogV1, WorkdeckSessionInputKind,
    };
    use base64::engine::general_purpose::STANDARD;
    use serde::Serialize;
    use serde_json::json;
    use std::sync::atomic::AtomicUsize;
    use std::thread;
    use std::time::Duration;
    use workdeck_review::{
        REVIEW_PATCH_CONTENT_TYPE, ReviewResourceChunk, ReviewResourceDescriptorBase,
    };

    const SESSION_ID: &str = "session-1";
    const GENERATION_ONE: &str = "generation:p1:0";
    const GENERATION_TWO: &str = "generation:p1:1";

    struct TestProducerSocket {
        state: Mutex<Weak<WorkdeckSessionBrokerState>>,
        own_socket: Mutex<Option<Weak<dyn DaemonSessionSocket>>>,
        resources: Mutex<BTreeMap<String, Vec<u8>>>,
        sent: Mutex<Vec<Value>>,
        delay_ms: AtomicU64,
        action_revision: AtomicU64,
    }

    impl TestProducerSocket {
        fn new(resources: BTreeMap<String, Vec<u8>>) -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new(Weak::new()),
                own_socket: Mutex::new(None),
                resources: Mutex::new(resources),
                sent: Mutex::new(Vec::new()),
                delay_ms: AtomicU64::new(0),
                action_revision: AtomicU64::new(1),
            })
        }

        fn bind(self: &Arc<Self>, state: &Arc<WorkdeckSessionBrokerState>) {
            *self.state.lock().unwrap_or_else(|error| error.into_inner()) = Arc::downgrade(state);
            let socket: SharedDaemonSessionSocket = Arc::clone(self) as SharedDaemonSessionSocket;
            *self
                .own_socket
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(Arc::downgrade(&socket));
        }

        fn shared(self: &Arc<Self>) -> SharedDaemonSessionSocket {
            Arc::clone(self) as SharedDaemonSessionSocket
        }

        fn messages(&self) -> Vec<Value> {
            self.sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone()
        }

        fn reads_for(&self, resource_id: &str) -> usize {
            self.messages()
                .iter()
                .filter(|message| {
                    message["command"] == "read_review_resource"
                        && message["input"]["request"]["resourceId"] == resource_id
                })
                .count()
        }

        fn replace_resources(&self, resources: BTreeMap<String, Vec<u8>>) {
            *self
                .resources
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = resources;
        }
    }

    impl DaemonSessionSocket for TestProducerSocket {
        fn send(&self, data: &str) -> Result<bool, SessionBrokerStateError> {
            let message = serde_json::from_str::<Value>(data)
                .map_err(|error| SessionBrokerStateError::message(error.to_string()))?;
            self.sent
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(message.clone());
            let request_id = message["requestId"]
                .as_str()
                .ok_or_else(|| SessionBrokerStateError::message("missing request id"))?
                .to_owned();
            let state = self
                .state
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone();
            let socket = self
                .own_socket
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
                .cloned()
                .ok_or_else(|| SessionBrokerStateError::message("socket is not bound"))?;
            let delay_ms = self.delay_ms.load(Ordering::Acquire);
            let result = match message["command"].as_str() {
                Some("read_review_resource") => {
                    let request = &message["input"]["request"];
                    let resource_id = request["resourceId"].as_str().unwrap_or_default();
                    let generation = request["generation"].as_str().unwrap_or_default();
                    let offset = request["offset"].as_u64().unwrap_or_default();
                    let length = request["length"].as_u64().unwrap_or_default();
                    let resource = self
                        .resources
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .get(resource_id)
                        .cloned();
                    match resource {
                        Some(resource) => {
                            let start = usize::try_from(offset).unwrap_or(usize::MAX);
                            let length = usize::try_from(length).unwrap_or(usize::MAX);
                            let end = start.saturating_add(length).min(resource.len());
                            let bytes = resource.get(start..end).unwrap_or_default();
                            serde_json::to_value(WorkdeckReviewResourceReadResultV1::Chunk {
                                ok: true,
                                chunk: ReviewResourceChunk {
                                    generation: generation.into(),
                                    resource_id: resource_id.into(),
                                    offset,
                                    byte_length: bytes.len() as u64,
                                    encoding: "base64".into(),
                                    data: STANDARD.encode(bytes),
                                    content_digest: workdeck_core::review_digest(&resource),
                                    content_size: resource.len() as u64,
                                    eof: end == resource.len(),
                                },
                            })
                            .unwrap()
                        }
                        None => serde_json::to_value(WorkdeckReviewResourceReadResultV1::Failed(
                            crate::WorkdeckReviewFailureV1 {
                                ok: false,
                                code: WorkdeckReviewFailureCodeV1::UnknownResource,
                                message: "unknown resource".into(),
                                current_generation: generation.into(),
                            },
                        ))
                        .unwrap(),
                    }
                }
                Some("apply_review_action") => {
                    let generation = message["input"]["generation"].as_str().unwrap_or_default();
                    serde_json::to_value(WorkdeckReviewActionResultV1::Applied(
                        crate::WorkdeckReviewActionAppliedV1 {
                            ok: true,
                            generation: generation.into(),
                            state_revision: self.action_revision.fetch_add(1, Ordering::AcqRel),
                        },
                    ))
                    .unwrap()
                }
                _ => return Err(SessionBrokerStateError::message("unsupported test command")),
            };
            thread::spawn(move || {
                if delay_ms > 0 {
                    thread::sleep(Duration::from_millis(delay_ms));
                }
                if let (Some(state), Some(socket)) = (state.upgrade(), socket.upgrade()) {
                    state.handle_command_result(
                        &socket,
                        &request_id,
                        BrokerCommandOutcome::Success(result),
                    );
                }
            });
            Ok(true)
        }
    }

    #[derive(Clone)]
    struct FileFixture {
        runtime_id: String,
        file_key: String,
        path: String,
        patch: Vec<u8>,
    }

    impl FileFixture {
        fn new(index: usize, lines: usize) -> Self {
            let patch = (0..lines)
                .map(|line| format!("+line {line}: {}\n", "x".repeat(32)))
                .collect::<String>()
                .into_bytes();
            Self {
                runtime_id: format!("file-{index}"),
                file_key: format!("file:{:016x}", index + 1),
                path: format!("src/file-{index}.rs"),
                patch,
            }
        }

        fn resource_id(&self) -> String {
            review_resource_id(&ReviewResourceAddress {
                kind: ReviewResourceKind::Patch,
                file_key: self.file_key.clone(),
                side: None,
            })
        }
    }

    fn registration(
        generation: Option<&str>,
        files: &[FileFixture],
        inline_patch: bool,
    ) -> WorkdeckSessionRegistration {
        let review_catalog = generation.map(|generation| WorkdeckReviewResourceCatalogV1 {
            generation: generation.into(),
            file_keys_by_runtime_id: files
                .iter()
                .map(|file| (file.runtime_id.clone(), file.file_key.clone()))
                .collect(),
            resources: files
                .iter()
                .map(|file| ReviewResourceDescriptor::Patch {
                    descriptor: ReviewResourceDescriptorBase {
                        id: file.resource_id(),
                        generation: generation.into(),
                        file_key: file.file_key.clone(),
                        byte_length: Some(file.patch.len() as u64),
                        digest: Some(workdeck_core::review_digest(&file.patch)),
                    },
                    content_type: REVIEW_PATCH_CONTENT_TYPE.into(),
                })
                .collect(),
        });
        WorkdeckSessionRegistration {
            registration_version: SESSION_BROKER_REGISTRATION_VERSION,
            session_id: SESSION_ID.into(),
            pid: 123,
            cwd: "/repo".into(),
            repo_root: Some("/repo".into()),
            launched_at: "2026-03-22T00:00:00.000Z".into(),
            terminal: None,
            info: WorkdeckSessionInfo {
                input_kind: WorkdeckSessionInputKind::Vcs,
                title: "repo working tree".into(),
                source_label: "/repo".into(),
                experimental_features: Some(Vec::new()),
                files: files
                    .iter()
                    .map(|file| SessionReviewFile {
                        summary: SessionFileSummary {
                            id: file.runtime_id.clone(),
                            path: file.path.clone(),
                            previous_path: None,
                            additions: 1,
                            deletions: 0,
                            hunk_count: 1,
                        },
                        patch: inline_patch.then(|| String::from_utf8(file.patch.clone()).unwrap()),
                        hunks: vec![SessionReviewHunk {
                            index: 0,
                            header: "@@ -1 +1 @@".into(),
                            old_range: None,
                            new_range: None,
                        }],
                    })
                    .collect(),
                review_catalog,
                review_capability_digest: Some("a".repeat(64)),
            },
        }
    }

    fn snapshot(generation: Option<&str>, selected_file: &FileFixture) -> WorkdeckSessionSnapshot {
        WorkdeckSessionSnapshot {
            updated_at: "2026-03-22T00:00:00.000Z".into(),
            state: WorkdeckSessionState {
                selected_file_id: Some(selected_file.runtime_id.clone()),
                selected_file_path: Some(selected_file.path.clone()),
                selected_hunk_index: 0,
                selected_hunk_old_range: None,
                selected_hunk_new_range: None,
                show_agent_notes: false,
                note_markup_width: None,
                live_comment_count: 0,
                live_comments: Vec::new(),
                review_note_count: Some(0),
                review_notes: Some(Vec::new()),
                review_publication: generation.map(|generation| ReviewPublicationAddress {
                    generation: generation.into(),
                    state_revision: 0,
                }),
            },
        }
    }

    fn values(
        generation: Option<&str>,
        files: &[FileFixture],
        inline_patch: bool,
    ) -> (Value, Value) {
        (
            serde_json::to_value(registration(generation, files, inline_patch)).unwrap(),
            serde_json::to_value(snapshot(generation, &files[0])).unwrap(),
        )
    }

    fn resources(files: &[FileFixture]) -> BTreeMap<String, Vec<u8>> {
        files
            .iter()
            .map(|file| (file.resource_id(), file.patch.clone()))
            .collect()
    }

    fn connect_with_cache(
        files: &[FileFixture],
        cache: ReviewResourceCache,
    ) -> (Arc<WorkdeckSessionBrokerState>, Arc<TestProducerSocket>) {
        let state = Arc::new(WorkdeckSessionBrokerState::new(cache));
        let socket = TestProducerSocket::new(resources(files));
        socket.bind(&state);
        (state, socket)
    }

    fn connect(
        files: &[FileFixture],
    ) -> (Arc<WorkdeckSessionBrokerState>, Arc<TestProducerSocket>) {
        connect_with_cache(files, ReviewResourceCache::default())
    }

    fn register(
        state: &WorkdeckSessionBrokerState,
        socket: &Arc<TestProducerSocket>,
        generation: Option<&str>,
        files: &[FileFixture],
        inline_patch: bool,
    ) -> RegisterSessionResult {
        let (registration, snapshot) = values(generation, files, inline_patch);
        state.register_session(
            socket.shared(),
            &registration,
            &snapshot,
            RegisterSessionOptions::default(),
        )
    }

    fn review_options(include_patch: bool) -> BrokerSessionReviewOptions {
        BrokerSessionReviewOptions {
            include_patch,
            include_notes: false,
        }
    }

    #[test]
    fn mirrors_registration_capability_and_lifecycle_events() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&events);
        let _subscription = state.subscribe_review_publications(move |event| {
            captured
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .push(event);
        });
        assert_eq!(
            register(&state, &socket, Some(GENERATION_ONE), &files, false),
            RegisterSessionResult::Registered
        );
        assert_eq!(
            state
                .get_review_publication(SESSION_ID)
                .unwrap()
                .address
                .generation,
            GENERATION_ONE
        );
        assert_eq!(
            state
                .get_review_capability_digest(SESSION_ID)
                .as_deref()
                .map(str::len),
            Some(64)
        );
        state.unregister_socket(&socket.shared());
        assert_eq!(state.get_review_publication(SESSION_ID), None);
        let events = events.lock().unwrap_or_else(|error| error.into_inner());
        assert!(matches!(
            events[0],
            ReviewPublicationEvent::Published { .. }
        ));
        assert!(matches!(events[1], ReviewPublicationEvent::Retired { .. }));
    }

    #[test]
    fn reconstructs_patch_and_selected_file_byte_for_byte() {
        let files = vec![FileFixture::new(0, 4), FileFixture::new(1, 3)];
        let (state, socket) = connect(&files);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        let review = state
            .get_session_review_with_resources(&selector_for(SESSION_ID), review_options(true))
            .unwrap();
        assert_eq!(
            review.files[0].patch.as_deref(),
            Some(std::str::from_utf8(&files[0].patch).unwrap())
        );
        assert_eq!(review.selected_file.unwrap().patch, review.files[0].patch);
    }

    #[test]
    fn omits_patch_and_transport_reads_when_not_requested() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        let review = state
            .get_session_review_with_resources(&selector_for(SESSION_ID), review_options(false))
            .unwrap();
        assert!(review.files.iter().all(|file| file.patch.is_none()));
        assert!(socket.messages().is_empty());
    }

    #[test]
    fn reads_large_resources_in_bounded_windows() {
        let files = vec![FileFixture::new(
            0,
            usize::try_from(REVIEW_RESOURCE_CHUNK_BYTES / 32).unwrap() + 1_000,
        )];
        assert!(files[0].patch.len() as u64 > REVIEW_RESOURCE_CHUNK_BYTES);
        let (state, socket) = connect(&files);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        let review = state
            .get_session_review_with_resources(&selector_for(SESSION_ID), review_options(true))
            .unwrap();
        assert_eq!(
            review.files[0].patch.as_deref(),
            Some(std::str::from_utf8(&files[0].patch).unwrap())
        );
        assert!(socket.reads_for(&files[0].resource_id()) > 1);
        assert!(socket.messages().iter().all(|message| {
            message["command"] != "read_review_resource"
                || message["input"]["request"]["length"].as_u64()
                    <= Some(REVIEW_RESOURCE_CHUNK_BYTES)
        }));
    }

    #[test]
    fn collapses_concurrent_reads_and_reuses_completed_cache() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        socket.delay_ms.store(15, Ordering::Release);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let mut threads = Vec::new();
        for _ in 0..2 {
            let state = Arc::clone(&state);
            let barrier = Arc::clone(&barrier);
            let resource_id = files[0].resource_id();
            threads.push(thread::spawn(move || {
                barrier.wait();
                state
                    .load_review_resource(SESSION_ID, GENERATION_ONE, &resource_id)
                    .unwrap()
            }));
        }
        barrier.wait();
        let left = threads.remove(0).join().unwrap();
        let right = threads.remove(0).join().unwrap();
        assert_eq!(left, right);
        assert_eq!(socket.reads_for(&files[0].resource_id()), 1);
        state
            .load_review_resource(SESSION_ID, GENERATION_ONE, &files[0].resource_id())
            .unwrap();
        assert_eq!(socket.reads_for(&files[0].resource_id()), 1);
        assert_eq!(
            state.get_review_resource_usage(),
            ReviewResourceUsage {
                cached_bytes: files[0].patch.len(),
                reserved_bytes: 0,
                entry_count: 1,
            }
        );
    }

    #[test]
    fn generation_replacement_evicts_cache_and_retires_old_reads() {
        let first_files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&first_files);
        register(&state, &socket, Some(GENERATION_ONE), &first_files, false);
        state
            .load_review_resource(SESSION_ID, GENERATION_ONE, &first_files[0].resource_id())
            .unwrap();
        assert_eq!(state.get_review_resource_usage().entry_count, 1);
        let mut second_files = first_files.clone();
        second_files[0]
            .patch
            .extend_from_slice(b"+new generation\n");
        socket.replace_resources(resources(&second_files));
        register(&state, &socket, Some(GENERATION_TWO), &second_files, false);
        assert_eq!(state.get_review_resource_usage().entry_count, 0);
        assert!(matches!(
            state
                .load_review_resource(SESSION_ID, GENERATION_ONE, &first_files[0].resource_id())
                .unwrap_err(),
            WorkdeckSessionBrokerError::GenerationRetired(_)
        ));
    }

    #[test]
    fn unknown_resource_is_distinct_from_retired_generation() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        let error = state
            .load_review_resource(SESSION_ID, GENERATION_ONE, "resource:patch:file:deadbeef")
            .unwrap_err();
        assert!(matches!(
            error,
            WorkdeckSessionBrokerError::ResourceRead(ReviewResourceReadError {
                code: WorkdeckReviewFailureCodeV1::UnknownResource,
                ..
            })
        ));
        assert!(error.to_string().contains("is not part of generation"));
    }

    #[test]
    fn refuses_load_without_daemon_inflight_budget() {
        let files = vec![FileFixture::new(0, 3)];
        let cache = ReviewResourceCache::new(crate::ReviewResourceCacheLimits {
            in_flight_resources: 0,
            ..crate::ReviewResourceCacheLimits::default()
        });
        let (state, socket) = connect_with_cache(&files, cache);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        assert!(
            state
                .get_session_review_with_resources(&selector_for(SESSION_ID), review_options(true))
                .unwrap_err()
                .to_string()
                .contains("already assembling")
        );
    }

    #[test]
    fn legacy_inline_patch_is_served_without_a_mirror_or_read() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        register(&state, &socket, None, &files, true);
        let review = state
            .get_session_review_with_resources(&selector_for(SESSION_ID), review_options(true))
            .unwrap();
        assert_eq!(state.get_review_publication(SESSION_ID), None);
        assert_eq!(
            review.files[0].patch.as_deref(),
            Some(std::str::from_utf8(&files[0].patch).unwrap())
        );
        assert!(socket.messages().is_empty());
    }

    #[test]
    fn forwards_action_and_reports_producer_position() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        let result = state
            .apply_review_action(
                SESSION_ID,
                GENERATION_ONE,
                WorkdeckReviewActionV1::FilterSet {
                    filter: "alpha".into(),
                },
                None,
                None,
            )
            .unwrap();
        assert!(matches!(
            result,
            WorkdeckReviewActionResultV1::Applied(crate::WorkdeckReviewActionAppliedV1 {
                generation,
                ..
            }) if generation == GENERATION_ONE
        ));
        let action = socket
            .messages()
            .into_iter()
            .find(|message| message["command"] == "apply_review_action")
            .unwrap();
        assert_eq!(action["input"]["actor"]["clientId"], "workdeck-daemon");
        assert_eq!(action["input"]["action"]["type"], "filter/set");
    }

    #[test]
    fn refuses_action_for_retired_generation_before_transport() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        let error = state
            .apply_review_action(
                SESSION_ID,
                GENERATION_TWO,
                WorkdeckReviewActionV1::FilterSet {
                    filter: "alpha".into(),
                },
                None,
                None,
            )
            .unwrap_err();
        assert!(matches!(
            error,
            WorkdeckSessionBrokerError::GenerationRetired(_)
        ));
        assert!(socket.messages().is_empty());
    }

    #[test]
    fn watcher_panics_do_not_block_other_watchers() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        let called = Arc::new(AtomicUsize::new(0));
        let _panicking = state.subscribe_review_publications(|_| panic!("watcher bug"));
        let called_by_watcher = Arc::clone(&called);
        let _healthy = state.subscribe_review_publications(move |_| {
            called_by_watcher.fetch_add(1, Ordering::AcqRel);
        });
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        assert_eq!(called.load(Ordering::Acquire), 1);
    }

    #[test]
    fn shutdown_clears_mirror_cache_loads_and_watchers() {
        let files = vec![FileFixture::new(0, 3)];
        let (state, socket) = connect(&files);
        register(&state, &socket, Some(GENERATION_ONE), &files, false);
        state
            .load_review_resource(SESSION_ID, GENERATION_ONE, &files[0].resource_id())
            .unwrap();
        let events = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&events);
        let _watcher = state.subscribe_review_publications(move |_| {
            observed.fetch_add(1, Ordering::AcqRel);
        });
        state.shutdown(None);
        assert_eq!(state.get_review_publication(SESSION_ID), None);
        assert_eq!(
            state.get_review_resource_usage(),
            ReviewResourceUsage {
                cached_bytes: 0,
                reserved_bytes: 0,
                entry_count: 0,
            }
        );
        assert_eq!(events.load(Ordering::Acquire), 1);
        assert_eq!(
            register(&state, &socket, Some(GENERATION_ONE), &files, false),
            RegisterSessionResult::Shutdown
        );
        assert_eq!(events.load(Ordering::Acquire), 1);
    }

    #[test]
    fn fixture_wire_values_are_strictly_accepted() {
        let files = vec![FileFixture::new(0, 3)];
        let (registration, snapshot) = values(Some(GENERATION_ONE), &files, false);
        assert!(parse_workdeck_session_registration(&registration).is_some());
        assert!(parse_workdeck_session_snapshot(&snapshot).is_some());
        assert_eq!(registration["info"]["files"][0]["hunkCount"], json!(1));
    }

    fn _assert_serialize<T: Serialize>() {}
}
