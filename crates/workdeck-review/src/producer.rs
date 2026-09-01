//! Generation authority for semantic reviews, stores, and bounded resources.

use std::{
    collections::{BTreeMap, HashMap},
    fmt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use serde_json::Value;
use thiserror::Error;
use workdeck_core::DiffFile;

use crate::{
    ReviewDigestProvider, ReviewGenerationError, ReviewGenerationIdentity, ReviewIntentFacts,
    ReviewIntentOutcome, ReviewIntentPlanningError, ReviewPublication, ReviewPublicationAddress,
    ReviewRequestErrorCode, ReviewResourceChunk, ReviewResourceDescriptor, ReviewResourceErrorCode,
    ReviewResourceFailure, ReviewResourceLoad, ReviewResourceStore, ReviewResourceStoreOptions,
    ReviewSourceLoader, SemanticReviewIntent, SemanticReviewState, SemanticReviewStore,
    SnapshotReviewSourceLoader, apply_semantic_review_intent, assert_review_publication_advance,
    build_review_annotation_index, build_review_publication, format_review_generation,
    next_review_generation, parse_read_review_resource_request,
};

pub const MAX_REVIEW_RESOURCE_BATCH: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewProducerErrorCode {
    StaleGeneration,
    InvalidRequest,
    UnknownResource,
    ResourceUnavailable,
    ResourceTooLarge,
    ResourceIntegrity,
    InvalidRange,
}

impl From<ReviewRequestErrorCode> for ReviewProducerErrorCode {
    fn from(value: ReviewRequestErrorCode) -> Self {
        match value {
            ReviewRequestErrorCode::StaleGeneration => Self::StaleGeneration,
            ReviewRequestErrorCode::InvalidRequest => Self::InvalidRequest,
        }
    }
}

impl From<ReviewResourceErrorCode> for ReviewProducerErrorCode {
    fn from(value: ReviewResourceErrorCode) -> Self {
        match value {
            ReviewResourceErrorCode::UnknownResource => Self::UnknownResource,
            ReviewResourceErrorCode::ResourceUnavailable => Self::ResourceUnavailable,
            ReviewResourceErrorCode::ResourceTooLarge => Self::ResourceTooLarge,
            ReviewResourceErrorCode::ResourceIntegrity => Self::ResourceIntegrity,
            ReviewResourceErrorCode::InvalidRange => Self::InvalidRange,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewProducerFailure {
    pub ok: bool,
    pub code: ReviewProducerErrorCode,
    pub message: String,
    pub current_generation: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReviewProducerChunkResult {
    Chunk(ReviewResourceChunk),
    Failure(ReviewProducerFailure),
}

impl ReviewProducerChunkResult {
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        matches!(self, Self::Chunk(_))
    }
}

#[derive(Clone)]
pub struct ReviewProducerOptions {
    pub producer_id: Option<String>,
    pub resource_concurrency: Option<usize>,
    pub digest: Arc<dyn ReviewDigestProvider>,
    pub source_loader: Arc<dyn ReviewSourceLoader>,
}

impl Default for ReviewProducerOptions {
    fn default() -> Self {
        Self {
            producer_id: None,
            resource_concurrency: None,
            digest: Arc::new(workdeck_core::review_digest),
            source_loader: Arc::new(SnapshotReviewSourceLoader),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublishReviewInput {
    pub files: Vec<DiffFile>,
    pub source_label: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreparedState {
    Prepared,
    Reserved,
    Settled,
}

pub struct PreparedReviewPublication {
    pub identity: ReviewGenerationIdentity,
    pub publication: Arc<ReviewPublication>,
    pub resource_store: Arc<ReviewResourceStore>,
    owner_token: u64,
    base_generation: String,
    state: Mutex<PreparedState>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewPublicationCommitOptions {
    pub detach_store: bool,
}

struct ProducerInner {
    identity: ReviewGenerationIdentity,
    publication: Arc<ReviewPublication>,
    resource_store: Arc<ReviewResourceStore>,
    store: Option<SemanticReviewStore>,
    store_generation: Option<String>,
    publication_reservation: Option<u64>,
    next_reservation: u64,
}

#[derive(Clone)]
pub struct ReviewProducer {
    token: u64,
    resource_concurrency: Option<usize>,
    digest: Arc<dyn ReviewDigestProvider>,
    source_loader: Arc<dyn ReviewSourceLoader>,
    inner: Arc<Mutex<ProducerInner>>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReviewProducerLifecycleError {
    #[error(transparent)]
    Generation(#[from] ReviewGenerationError),
    #[error("Cannot reserve a review publication prepared by another producer.")]
    ForeignPreparation,
    #[error("Cannot reserve a review publication more than once.")]
    ReusedPreparation,
    #[error("Cannot reserve a review publication while another reservation is active.")]
    ActiveReservation,
    #[error("Cannot reserve a stale review publication.")]
    StalePreparation,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ReviewProducerIntentError {
    #[error("Review producer has no review state attached.")]
    NoReviewState,
    #[error(transparent)]
    Planning(#[from] ReviewIntentPlanningError),
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("A resource batch may name at most {MAX_REVIEW_RESOURCE_BATCH} resources.")]
pub struct ReviewResourceBatchTooLarge;

pub struct ReservedReviewPublication {
    producer: ReviewProducer,
    prepared: Arc<PreparedReviewPublication>,
    reservation_id: u64,
    settled: AtomicBool,
}

impl fmt::Debug for ReservedReviewPublication {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReservedReviewPublication")
            .field("reservation_id", &self.reservation_id)
            .field("settled", &self.settled.load(Ordering::Acquire))
            .finish_non_exhaustive()
    }
}

impl ReservedReviewPublication {
    #[must_use]
    pub fn commit(&self, options: ReviewPublicationCommitOptions) -> Arc<ReviewPublication> {
        self.settle(true, options)
    }

    pub fn cancel(&self) {
        let _ = self.settle(false, ReviewPublicationCommitOptions::default());
    }

    fn settle(
        &self,
        commit: bool,
        options: ReviewPublicationCommitOptions,
    ) -> Arc<ReviewPublication> {
        if self.settled.swap(true, Ordering::AcqRel) {
            return self.producer.get_publication();
        }
        *self
            .prepared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = PreparedState::Settled;
        let mut inner = self
            .producer
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if inner.publication_reservation != Some(self.reservation_id) {
            return Arc::clone(&inner.publication);
        }
        inner.publication_reservation = None;
        if commit {
            inner.identity = self.prepared.identity.clone();
            inner.publication = Arc::clone(&self.prepared.publication);
            inner.resource_store = Arc::clone(&self.prepared.resource_store);
            if options.detach_store {
                inner.store = None;
                inner.store_generation = None;
            }
        }
        Arc::clone(&inner.publication)
    }
}

impl ReviewProducer {
    pub fn new(
        input: PublishReviewInput,
        options: ReviewProducerOptions,
    ) -> Result<Self, ReviewProducerLifecycleError> {
        static NEXT_PRODUCER: AtomicU64 = AtomicU64::new(1);
        let token = NEXT_PRODUCER.fetch_add(1, Ordering::Relaxed);
        let identity = ReviewGenerationIdentity {
            producer_id: options
                .producer_id
                .clone()
                .unwrap_or_else(|| format!("p{}-{token}", std::process::id())),
            sequence: 0,
        };
        let generation = format_review_generation(&identity)?;
        let publication = Arc::new(build_review_publication(
            &input.files,
            generation,
            input.source_label.as_deref(),
        ));
        let resource_store = Arc::new(Self::create_resource_store(
            Arc::clone(&publication),
            &options,
        ));
        Ok(Self {
            token,
            resource_concurrency: options.resource_concurrency,
            digest: options.digest,
            source_loader: options.source_loader,
            inner: Arc::new(Mutex::new(ProducerInner {
                identity,
                publication,
                resource_store,
                store: None,
                store_generation: None,
                publication_reservation: None,
                next_reservation: 1,
            })),
        })
    }

    #[must_use]
    pub fn get_publication(&self) -> Arc<ReviewPublication> {
        Arc::clone(
            &self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .publication,
        )
    }

    #[must_use]
    pub fn get_publication_address(&self) -> ReviewPublicationAddress {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        ReviewPublicationAddress {
            generation: inner.publication.generation.clone(),
            state_revision: Self::current_store(&inner)
                .map(|store| store.snapshot().state_revision)
                .unwrap_or(0),
        }
    }

    pub fn prepare_publication(
        &self,
        input: &PublishReviewInput,
    ) -> Result<Arc<PreparedReviewPublication>, ReviewProducerLifecycleError> {
        let (previous, identity) = {
            let inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                ReviewPublicationAddress {
                    generation: inner.publication.generation.clone(),
                    state_revision: Self::current_store(&inner)
                        .map(|store| store.snapshot().state_revision)
                        .unwrap_or(0),
                },
                next_review_generation(&inner.identity),
            )
        };
        let generation = format_review_generation(&identity)?;
        assert_review_publication_advance(
            &previous,
            &ReviewPublicationAddress {
                generation: generation.clone(),
                state_revision: 0,
            },
        )?;
        let publication = Arc::new(build_review_publication(
            &input.files,
            generation,
            input.source_label.as_deref(),
        ));
        let resource_store = Arc::new(self.new_resource_store(Arc::clone(&publication)));
        Ok(Arc::new(PreparedReviewPublication {
            identity,
            publication,
            resource_store,
            owner_token: self.token,
            base_generation: previous.generation,
            state: Mutex::new(PreparedState::Prepared),
        }))
    }

    pub fn reserve_publication(
        &self,
        prepared: Arc<PreparedReviewPublication>,
    ) -> Result<ReservedReviewPublication, ReviewProducerLifecycleError> {
        if prepared.owner_token != self.token {
            return Err(ReviewProducerLifecycleError::ForeignPreparation);
        }
        let mut state = prepared
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *state != PreparedState::Prepared {
            return Err(ReviewProducerLifecycleError::ReusedPreparation);
        }
        let reservation_id = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if inner.publication_reservation.is_some() {
                return Err(ReviewProducerLifecycleError::ActiveReservation);
            }
            if prepared.base_generation != inner.publication.generation {
                return Err(ReviewProducerLifecycleError::StalePreparation);
            }
            let reservation_id = inner.next_reservation;
            inner.next_reservation = inner.next_reservation.wrapping_add(1);
            inner.publication_reservation = Some(reservation_id);
            reservation_id
        };
        *state = PreparedState::Reserved;
        drop(state);
        Ok(ReservedReviewPublication {
            producer: self.clone(),
            prepared,
            reservation_id,
            settled: AtomicBool::new(false),
        })
    }

    pub fn publish(
        &self,
        input: &PublishReviewInput,
    ) -> Result<Arc<ReviewPublication>, ReviewProducerLifecycleError> {
        let prepared = self.prepare_publication(input)?;
        Ok(self
            .reserve_publication(prepared)?
            .commit(ReviewPublicationCommitOptions::default()))
    }

    pub fn attach_store(&self, store: SemanticReviewStore) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.store_generation = Some(inner.publication.generation.clone());
        inner.store = Some(store);
    }

    #[must_use]
    pub fn get_review_state(&self) -> Option<Arc<SemanticReviewState>> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::current_store(&inner).map(SemanticReviewStore::snapshot)
    }

    #[must_use]
    pub fn get_positioned_review_state(&self) -> Option<(String, Arc<SemanticReviewState>)> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Self::current_store(&inner)
            .map(|store| (inner.publication.generation.clone(), store.snapshot()))
    }

    pub fn apply_intent(
        &self,
        intent: SemanticReviewIntent,
        mut facts: ReviewIntentFacts,
    ) -> Result<Option<ReviewIntentOutcome>, ReviewProducerIntentError> {
        let (store, files) = {
            let inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let store = Self::current_store(&inner)
                .cloned()
                .ok_or(ReviewProducerIntentError::NoReviewState)?;
            let files = inner
                .publication
                .diff_files_by_key
                .values()
                .cloned()
                .collect::<Vec<_>>();
            (store, files)
        };
        if facts.annotations.is_none() {
            let snapshot = store.snapshot();
            let key_by_runtime_id = snapshot
                .document
                .files
                .iter()
                .map(|file| (file.runtime_id.clone(), file.key.clone()))
                .collect::<HashMap<_, _>>();
            let annotations = build_review_annotation_index(&files, &key_by_runtime_id);
            facts.annotations = Some(crate::SemanticReviewAnnotationIndex {
                annotated_hunk_indices_by_file_key: annotations.annotated_hunk_indices_by_file_key,
                annotated_file_keys: annotations.annotated_file_keys,
            });
        }
        Ok(apply_semantic_review_intent(&store, intent, &facts)?)
    }

    #[must_use]
    pub fn describe_resources(&self) -> Vec<ReviewResourceDescriptor> {
        let store = Arc::clone(
            &self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .resource_store,
        );
        store.describe_all()
    }

    #[must_use]
    pub fn read_resource(&self, request: &Value) -> ReviewProducerChunkResult {
        let Some(parsed) = parse_read_review_resource_request(request) else {
            return self.fail(
                ReviewRequestErrorCode::InvalidRequest.into(),
                format!(
                    "A resource read names a generation, a resource id, a non-negative offset, and a length from 1 to {}.",
                    crate::REVIEW_RESOURCE_CHUNK_BYTES
                ),
            );
        };
        let (publication, store) = {
            let inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (
                Arc::clone(&inner.publication),
                Arc::clone(&inner.resource_store),
            )
        };
        if parsed.generation != publication.generation {
            return self.fail(
                ReviewRequestErrorCode::StaleGeneration.into(),
                format!(
                    "Review generation {} is retired; the review is now at {}.",
                    parsed.generation, publication.generation
                ),
            );
        }
        match store.read_chunk(
            &parsed.resource_id,
            crate::ReviewResourceRange {
                offset: parsed.offset,
                length: parsed.length,
            },
        ) {
            Ok(chunk) => ReviewProducerChunkResult::Chunk(chunk),
            Err(failure) => self.lift_resource_failure(failure),
        }
    }

    pub fn materialize_resources(
        &self,
        resource_ids: &[String],
    ) -> Result<BTreeMap<String, ReviewResourceLoad>, ReviewResourceBatchTooLarge> {
        if resource_ids.len() > MAX_REVIEW_RESOURCE_BATCH {
            return Err(ReviewResourceBatchTooLarge);
        }
        let store = Arc::clone(
            &self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .resource_store,
        );
        Ok(store.materialize_all(resource_ids))
    }

    fn create_resource_store(
        publication: Arc<ReviewPublication>,
        options: &ReviewProducerOptions,
    ) -> ReviewResourceStore {
        let mut store_options = ReviewResourceStoreOptions::new(publication);
        if let Some(concurrency) = options.resource_concurrency {
            store_options.concurrency = concurrency;
        }
        store_options.digest = Arc::clone(&options.digest);
        store_options.source_loader = Arc::clone(&options.source_loader);
        ReviewResourceStore::new(store_options)
    }

    fn new_resource_store(&self, publication: Arc<ReviewPublication>) -> ReviewResourceStore {
        let mut options = ReviewResourceStoreOptions::new(publication);
        if let Some(concurrency) = self.resource_concurrency {
            options.concurrency = concurrency;
        }
        options.digest = Arc::clone(&self.digest);
        options.source_loader = Arc::clone(&self.source_loader);
        ReviewResourceStore::new(options)
    }

    fn current_store(inner: &ProducerInner) -> Option<&SemanticReviewStore> {
        (inner.store_generation.as_deref() == Some(&inner.publication.generation))
            .then_some(inner.store.as_ref())
            .flatten()
    }

    fn fail(&self, code: ReviewProducerErrorCode, message: String) -> ReviewProducerChunkResult {
        ReviewProducerChunkResult::Failure(ReviewProducerFailure {
            ok: false,
            code,
            message,
            current_generation: self.get_publication().generation.clone(),
        })
    }

    fn lift_resource_failure(&self, failure: ReviewResourceFailure) -> ReviewProducerChunkResult {
        self.fail(failure.code.into(), failure.message)
    }
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
    use workdeck_core::{
        AgentAnnotation, AgentFileContext, DiffHunk, FileChangeKind, FileFlags,
        FileSourceSnapshots, FileStats, LineRange, SourceOrigin, SourceSnapshot,
    };

    use super::*;
    use crate::{ReviewResourceAddress, ReviewResourceKind, review_resource_id};

    fn file(id: &str, path: &str, patch: &str, source: Option<&str>) -> DiffFile {
        let mut file = DiffFile {
            key: String::new(),
            runtime_id: id.into(),
            path: path.into(),
            previous_path: None,
            change_kind: FileChangeKind::Modified,
            language: None,
            stats: FileStats::default(),
            flags: FileFlags::default(),
            patch: patch.into(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: vec![DiffHunk {
                index: 0,
                header: "@@ -1 +1 @@".into(),
                context: None,
                old_start: 1,
                old_count: 1,
                new_start: 1,
                new_count: 1,
                split_row_start: 0,
                split_row_count: 0,
                stack_row_start: 0,
                stack_row_count: 0,
                lines: Vec::new(),
            }],
            content_identity: String::new(),
            sources: FileSourceSnapshots::default(),
            source_identity: None,
            source_attested: false,
            agent: None,
        };
        file.refresh_identity();
        if let Some(source) = source {
            file.set_sources(FileSourceSnapshots {
                old: None,
                new: Some(SourceSnapshot::new(
                    source.into(),
                    SourceOrigin::WorkingTree,
                    true,
                )),
            });
        }
        file
    }

    fn input(files: Vec<DiffFile>) -> PublishReviewInput {
        PublishReviewInput {
            files,
            source_label: Some("/repo".into()),
        }
    }

    fn producer(files: Vec<DiffFile>) -> ReviewProducer {
        ReviewProducer::new(
            input(files),
            ReviewProducerOptions {
                producer_id: Some("test".into()),
                ..ReviewProducerOptions::default()
            },
        )
        .unwrap()
    }

    fn resource_id(producer: &ReviewProducer, kind: ReviewResourceKind) -> String {
        let publication = producer.get_publication();
        review_resource_id(&ReviewResourceAddress {
            kind,
            file_key: publication.document.files[0].key.clone(),
            side: (kind == ReviewResourceKind::Source).then_some(workdeck_core::ReviewSide::New),
        })
    }

    fn read_whole(producer: &ReviewProducer, resource_id: &str, length: u64) -> String {
        let mut offset = 0;
        let mut bytes = Vec::new();
        loop {
            let request = serde_json::json!({
                "generation": producer.get_publication().generation,
                "resourceId": resource_id,
                "offset": offset,
                "length": length,
            });
            let ReviewProducerChunkResult::Chunk(chunk) = producer.read_resource(&request) else {
                panic!("resource read failed")
            };
            bytes.extend(BASE64.decode(chunk.data).unwrap());
            offset += chunk.byte_length;
            if chunk.eof {
                return String::from_utf8(bytes).unwrap();
            }
        }
    }

    #[test]
    fn generations_prepare_reserve_commit_cancel_and_reject_invalid_ownership() {
        let producer = producer(vec![file("one", "a.rs", "one", None)]);
        assert_eq!(producer.get_publication().generation, "generation:test:0");
        let prepared = producer
            .prepare_publication(&input(vec![file("one", "a.rs", "two", None)]))
            .unwrap();
        assert_eq!(producer.get_publication().generation, "generation:test:0");
        assert_eq!(prepared.publication.generation, "generation:test:1");
        let reservation = producer.reserve_publication(Arc::clone(&prepared)).unwrap();
        assert_eq!(
            reservation
                .commit(ReviewPublicationCommitOptions::default())
                .generation,
            "generation:test:1"
        );
        assert_eq!(
            producer.reserve_publication(prepared).unwrap_err(),
            ReviewProducerLifecycleError::ReusedPreparation
        );

        let cancelled = producer.prepare_publication(&input(Vec::new())).unwrap();
        let reservation = producer.reserve_publication(cancelled.clone()).unwrap();
        reservation.cancel();
        assert_eq!(producer.get_publication().generation, "generation:test:1");
        assert_eq!(
            producer.reserve_publication(cancelled).unwrap_err(),
            ReviewProducerLifecycleError::ReusedPreparation
        );
    }

    #[test]
    fn rejects_foreign_stale_and_competing_preparations() {
        let producer = producer(Vec::new());
        let other = ReviewProducer::new(
            input(Vec::new()),
            ReviewProducerOptions {
                producer_id: Some("other".into()),
                ..ReviewProducerOptions::default()
            },
        )
        .unwrap();
        let first = producer.prepare_publication(&input(Vec::new())).unwrap();
        let competing = producer.prepare_publication(&input(Vec::new())).unwrap();
        assert_eq!(
            other.reserve_publication(first.clone()).unwrap_err(),
            ReviewProducerLifecycleError::ForeignPreparation
        );
        let reservation = producer.reserve_publication(first).unwrap();
        assert_eq!(
            producer.reserve_publication(competing.clone()).unwrap_err(),
            ReviewProducerLifecycleError::ActiveReservation
        );
        let _ = reservation.commit(ReviewPublicationCommitOptions::default());
        assert_eq!(
            producer.reserve_publication(competing).unwrap_err(),
            ReviewProducerLifecycleError::StalePreparation
        );
    }

    #[test]
    fn generation_advance_retires_old_store_and_resources_until_replacement_mounts() {
        let producer = producer(vec![file("one", "a.rs", "one", None)]);
        let old = producer.get_publication();
        producer.attach_store(SemanticReviewStore::new(Arc::clone(&old.document), false));
        assert_eq!(producer.get_publication_address().state_revision, 0);
        producer
            .publish(&input(vec![file("one", "a.rs", "two", None)]))
            .unwrap();
        assert!(producer.get_review_state().is_none());
        assert_eq!(
            producer
                .apply_intent(
                    SemanticReviewIntent::SetFilter("stale".into()),
                    ReviewIntentFacts::default()
                )
                .unwrap_err(),
            ReviewProducerIntentError::NoReviewState
        );
        let stale_request = serde_json::json!({
            "generation": old.generation,
            "resourceId": old.resources[1].base().id,
            "offset": 0,
            "length": 16,
        });
        let ReviewProducerChunkResult::Failure(failure) = producer.read_resource(&stale_request)
        else {
            panic!("retired generation was served")
        };
        assert_eq!(failure.code, ReviewProducerErrorCode::StaleGeneration);
        let current = producer.get_publication();
        producer.attach_store(SemanticReviewStore::new(
            Arc::clone(&current.document),
            false,
        ));
        assert_eq!(
            producer.get_positioned_review_state().unwrap().0,
            current.generation
        );
    }

    #[test]
    fn detach_commit_removes_the_previous_store_immediately() {
        let producer = producer(Vec::new());
        let current = producer.get_publication();
        producer.attach_store(SemanticReviewStore::new(
            Arc::clone(&current.document),
            false,
        ));
        let prepared = producer.prepare_publication(&input(Vec::new())).unwrap();
        let _ = producer
            .reserve_publication(prepared)
            .unwrap()
            .commit(ReviewPublicationCommitOptions { detach_store: true });
        assert!(producer.get_positioned_review_state().is_none());
    }

    #[test]
    fn reads_patch_canonical_source_paged_empty_and_reports_exact_failures() {
        let review_producer = producer(vec![file("one", "a.rs", "abcdefghij", Some("source\n"))]);
        let patch = resource_id(&review_producer, ReviewResourceKind::Patch);
        assert_eq!(read_whole(&review_producer, &patch, 3), "abcdefghij");
        let canonical = resource_id(&review_producer, ReviewResourceKind::CanonicalFile);
        let parsed: workdeck_core::SemanticReviewFile =
            serde_json::from_str(&read_whole(&review_producer, &canonical, 1024)).unwrap();
        assert_eq!(parsed, review_producer.get_publication().document.files[0]);
        let source = resource_id(&review_producer, ReviewResourceKind::Source);
        assert_eq!(read_whole(&review_producer, &source, 1024), "source\n");
        assert!(
            review_producer
                .describe_resources()
                .iter()
                .find(|resource| resource.base().id == patch)
                .unwrap()
                .is_materialized()
        );

        for (request, code) in [
            (
                serde_json::json!(null),
                ReviewProducerErrorCode::InvalidRequest,
            ),
            (
                serde_json::json!({"generation":"generation:test:0","resourceId":patch,"offset":0,"length":0}),
                ReviewProducerErrorCode::InvalidRequest,
            ),
            (
                serde_json::json!({"generation":"generation:test:0","resourceId":"resource:patch:file:deadbeef","offset":0,"length":16}),
                ReviewProducerErrorCode::UnknownResource,
            ),
            (
                serde_json::json!({"generation":"generation:test:0","resourceId":patch,"offset":999,"length":16}),
                ReviewProducerErrorCode::InvalidRange,
            ),
        ] {
            let ReviewProducerChunkResult::Failure(failure) =
                review_producer.read_resource(&request)
            else {
                panic!("invalid request succeeded")
            };
            assert_eq!(failure.code, code);
        }

        let empty = producer(vec![file("empty", "empty.rs", "", None)]);
        let id = resource_id(&empty, ReviewResourceKind::Patch);
        let request = serde_json::json!({
            "generation": empty.get_publication().generation,
            "resourceId": id,
            "offset": 0,
            "length": 16,
        });
        let ReviewProducerChunkResult::Chunk(chunk) = empty.read_resource(&request) else {
            panic!("empty resource failed")
        };
        assert_eq!(chunk.byte_length, 0);
        assert!(chunk.eof);
    }

    #[test]
    fn resource_batch_is_bounded_and_deduplicated() {
        let producer = producer(vec![file("one", "a.rs", "patch", None)]);
        let resource_id = producer.describe_resources()[0].base().id.clone();
        assert_eq!(
            producer
                .materialize_resources(&[resource_id.clone(), resource_id])
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            producer
                .materialize_resources(&vec![String::new(); MAX_REVIEW_RESOURCE_BATCH + 1])
                .unwrap_err(),
            ReviewResourceBatchTooLarge
        );
    }

    #[test]
    fn intent_planning_uses_attached_state_and_generation_annotation_index() {
        let plain = file("plain", "plain.rs", "plain", None);
        let mut annotated = file("annotated", "annotated.rs", "annotated", None);
        annotated.agent = Some(AgentFileContext {
            path: annotated.path.clone(),
            summary: Some("review me".into()),
            annotations: vec![AgentAnnotation {
                id: None,
                old_range: None,
                new_range: Some(LineRange { start: 1, end: 1 }),
                summary: "review this hunk".into(),
                rationale: None,
                markup: None,
                tags: Vec::new(),
                confidence: None,
                source: None,
                title: None,
                author: None,
                created_at: None,
                updated_at: None,
                editable: false,
            }],
        });
        let producer = producer(vec![plain, annotated]);
        let publication = producer.get_publication();
        producer.attach_store(SemanticReviewStore::new(
            Arc::clone(&publication.document),
            false,
        ));
        producer
            .apply_intent(
                SemanticReviewIntent::Move {
                    scope: crate::ReviewSelectionScope::AnnotatedHunk,
                    delta: 1,
                },
                ReviewIntentFacts::default(),
            )
            .unwrap();
        assert_eq!(
            producer.get_review_state().unwrap().selection.file_key,
            Some(publication.document.files[1].key.clone())
        );
    }

    #[test]
    fn read_request_parser_accepts_exactly_the_shared_fields() {
        let request = serde_json::json!({
            "generation": "generation:test:0",
            "resourceId": "resource:patch:file:abc",
            "offset": 0,
            "length": 16,
        });
        assert!(parse_read_review_resource_request(&request).is_some());
        let mut extra = request;
        extra["extra"] = serde_json::json!(1);
        assert!(parse_read_review_resource_request(&extra).is_none());
    }

    #[test]
    fn injected_digest_is_used_for_materialized_measurements() {
        let producer = ReviewProducer::new(
            input(vec![file("one", "a.rs", "patch", None)]),
            ReviewProducerOptions {
                producer_id: Some("test".into()),
                digest: Arc::new(|_bytes: &[u8]| "a".repeat(64)),
                ..ReviewProducerOptions::default()
            },
        )
        .unwrap();
        let id = resource_id(&producer, ReviewResourceKind::Patch);
        producer
            .materialize_resources(std::slice::from_ref(&id))
            .unwrap();
        assert_eq!(
            producer
                .describe_resources()
                .iter()
                .find(|resource| resource.base().id == id)
                .unwrap()
                .base()
                .digest
                .as_deref(),
            Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
        );
    }
}
