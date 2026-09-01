//! Single-flight, bounded materialization of one publication's review resources.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Condvar, Mutex},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use thiserror::Error;
use workdeck_core::{DiffFile, ReviewSide, review_digest, review_digests_equal};

use crate::{
    ReviewPublication, ReviewResourceChunk, ReviewResourceDescriptor, ReviewResourceErrorCode,
    ReviewResourceFailure, ReviewResourceKind, ReviewResourceRange,
    assert_canonical_file_matches_manifest, build_review_content_manifest_file,
    is_review_resource_range, review_publication_file, review_publication_resource,
    review_resource_ceiling, review_resource_failure,
};

pub const MAX_REVIEW_PRODUCER_RESOURCE_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedReviewResource {
    pub bytes: Arc<[u8]>,
    pub byte_length: u64,
    pub digest: String,
}

pub type ReviewResourceLoad = Result<MaterializedReviewResource, ReviewResourceFailure>;
pub type ReviewResourceRead = Result<ReviewResourceChunk, ReviewResourceFailure>;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ReviewSourceLoadError {
    #[error("source exceeds the readable limit")]
    TooLarge,
    #[error("{0}")]
    Unavailable(String),
}

/// Source access remains injected so VCS-backed snapshots can stay lazy at this boundary.
pub trait ReviewSourceLoader: Send + Sync {
    fn get_full_text(
        &self,
        file: &DiffFile,
        side: ReviewSide,
    ) -> Result<Option<String>, ReviewSourceLoadError>;
}

#[derive(Debug, Default)]
pub struct SnapshotReviewSourceLoader;

impl ReviewSourceLoader for SnapshotReviewSourceLoader {
    fn get_full_text(
        &self,
        file: &DiffFile,
        side: ReviewSide,
    ) -> Result<Option<String>, ReviewSourceLoadError> {
        Ok(match side {
            ReviewSide::Old => file.sources.old.as_ref(),
            ReviewSide::New => file.sources.new.as_ref(),
        }
        .map(|snapshot| snapshot.content.clone()))
    }
}

#[derive(Clone)]
struct Flight {
    settled: Arc<(Mutex<Option<ReviewResourceLoad>>, Condvar)>,
}

impl Flight {
    fn new() -> Self {
        Self {
            settled: Arc::new((Mutex::new(None), Condvar::new())),
        }
    }

    fn settle(&self, result: ReviewResourceLoad) {
        let (settled, ready) = &*self.settled;
        *settled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(result);
        ready.notify_all();
    }

    fn wait(&self) -> ReviewResourceLoad {
        let (settled, ready) = &*self.settled;
        let mut result = settled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while result.is_none() {
            result = ready
                .wait(result)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        result.clone().expect("settled flight has a result")
    }
}

#[derive(Default)]
struct ResourceStoreState {
    materialized: BTreeMap<String, MaterializedReviewResource>,
    oldest_first: VecDeque<String>,
    in_flight: BTreeMap<String, Flight>,
    cached_bytes: u64,
}

pub struct ReviewResourceStoreOptions {
    pub publication: Arc<ReviewPublication>,
    pub concurrency: usize,
    pub max_cache_bytes: u64,
    pub source_loader: Arc<dyn ReviewSourceLoader>,
}

impl ReviewResourceStoreOptions {
    #[must_use]
    pub fn new(publication: Arc<ReviewPublication>) -> Self {
        Self {
            publication,
            concurrency: crate::REVIEW_RESOURCE_LOAD_CONCURRENCY,
            max_cache_bytes: MAX_REVIEW_PRODUCER_RESOURCE_BYTES,
            source_loader: Arc::new(SnapshotReviewSourceLoader),
        }
    }
}

pub struct ReviewResourceStore {
    publication: Arc<ReviewPublication>,
    concurrency: usize,
    max_cache_bytes: u64,
    source_loader: Arc<dyn ReviewSourceLoader>,
    state: Mutex<ResourceStoreState>,
}

impl ReviewResourceStore {
    #[must_use]
    pub fn new(options: ReviewResourceStoreOptions) -> Self {
        Self {
            publication: options.publication,
            concurrency: options.concurrency.max(1),
            max_cache_bytes: options.max_cache_bytes,
            source_loader: options.source_loader,
            state: Mutex::new(ResourceStoreState::default()),
        }
    }

    #[must_use]
    pub fn describe(&self, resource_id: &str) -> Option<ReviewResourceDescriptor> {
        let mut descriptor = review_publication_resource(&self.publication, resource_id)?.clone();
        let state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(measured) = state.materialized.get(resource_id) {
            descriptor.base_mut().byte_length = Some(measured.byte_length);
            descriptor.base_mut().digest = Some(measured.digest.clone());
        }
        Some(descriptor)
    }

    #[must_use]
    pub fn describe_all(&self) -> Vec<ReviewResourceDescriptor> {
        self.publication
            .resources
            .iter()
            .map(|resource| {
                self.describe(&resource.base().id)
                    .unwrap_or_else(|| resource.clone())
            })
            .collect()
    }

    /// Produce one resource once. Concurrent readers wait on the same flight.
    pub fn materialize(&self, resource_id: &str) -> ReviewResourceLoad {
        let (flight, produces) = {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(resource) = state.materialized.get(resource_id).cloned() {
                Self::touch(&mut state, resource_id);
                return Ok(resource);
            }
            if let Some(flight) = state.in_flight.get(resource_id) {
                (flight.clone(), false)
            } else {
                let flight = Flight::new();
                state
                    .in_flight
                    .insert(resource_id.to_owned(), flight.clone());
                (flight, true)
            }
        };
        if !produces {
            return flight.wait();
        }

        let result =
            catch_unwind(AssertUnwindSafe(|| self.produce(resource_id))).unwrap_or_else(|_| {
                Err(review_resource_failure(
                    ReviewResourceErrorCode::ResourceUnavailable,
                    format!("Resource producer panicked while loading {resource_id}."),
                ))
            });
        {
            let mut state = self
                .state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Ok(resource) = &result {
                self.retain(&mut state, resource_id, resource.clone());
            }
            flight.settle(result.clone());
            state.in_flight.remove(resource_id);
        }
        result
    }

    /// Produce a de-duplicated resource batch without exceeding configured parallelism.
    pub fn materialize_all(&self, resource_ids: &[String]) -> BTreeMap<String, ReviewResourceLoad> {
        let mut seen = BTreeSet::new();
        let unique = resource_ids
            .iter()
            .filter(|id| seen.insert((*id).clone()))
            .cloned()
            .collect::<Vec<_>>();
        let mut results = BTreeMap::new();
        for group in unique.chunks(self.concurrency) {
            std::thread::scope(|scope| {
                let handles = group
                    .iter()
                    .map(|resource_id| {
                        let resource_id = resource_id.clone();
                        (
                            resource_id.clone(),
                            scope.spawn(move || self.materialize(&resource_id)),
                        )
                    })
                    .collect::<Vec<_>>();
                for (resource_id, handle) in handles {
                    let result = handle.join().unwrap_or_else(|_| {
                        Err(review_resource_failure(
                            ReviewResourceErrorCode::ResourceUnavailable,
                            format!("Resource worker panicked while loading {resource_id}."),
                        ))
                    });
                    results.insert(resource_id, result);
                }
            });
        }
        results
    }

    pub fn read_chunk(&self, resource_id: &str, range: ReviewResourceRange) -> ReviewResourceRead {
        if !is_review_resource_range(range) {
            return Err(review_resource_failure(
                ReviewResourceErrorCode::InvalidRange,
                "Resource reads take a non-negative offset and a length within the shared chunk bound.",
            ));
        }
        let resource = self.materialize(resource_id)?;
        if range.offset > resource.byte_length {
            return Err(review_resource_failure(
                ReviewResourceErrorCode::InvalidRange,
                format!(
                    "Resource {resource_id} has {} bytes; offset {} is past its end.",
                    resource.byte_length, range.offset
                ),
            ));
        }
        let end = resource
            .byte_length
            .min(range.offset.saturating_add(range.length));
        let start = usize::try_from(range.offset).expect("resource ceiling fits usize");
        let end = usize::try_from(end).expect("resource ceiling fits usize");
        let chunk = &resource.bytes[start..end];
        Ok(ReviewResourceChunk {
            generation: self.publication.generation.clone(),
            resource_id: resource_id.to_owned(),
            offset: range.offset,
            byte_length: u64::try_from(chunk.len()).expect("resource ceiling fits u64"),
            encoding: "base64".into(),
            data: BASE64.encode(chunk),
            content_digest: resource.digest,
            content_size: resource.byte_length,
            eof: u64::try_from(end).expect("resource ceiling fits u64") == resource.byte_length,
        })
    }

    fn produce(&self, resource_id: &str) -> ReviewResourceLoad {
        let descriptor =
            review_publication_resource(&self.publication, resource_id).ok_or_else(|| {
                review_resource_failure(
                    ReviewResourceErrorCode::UnknownResource,
                    format!(
                        "Review resource {resource_id} is not part of generation {}.",
                        self.publication.generation
                    ),
                )
            })?;
        let file = review_publication_file(&self.publication, &descriptor.base().file_key)
            .ok_or_else(|| {
                review_resource_failure(
                    ReviewResourceErrorCode::UnknownResource,
                    format!(
                        "Review resource {resource_id} names a file this generation does not have."
                    ),
                )
            })?;
        match descriptor {
            ReviewResourceDescriptor::CanonicalFile { .. } => {
                assert_canonical_file_matches_manifest(
                    file,
                    &build_review_content_manifest_file(file),
                )
                .map_err(|error| {
                    review_resource_failure(
                        ReviewResourceErrorCode::ResourceIntegrity,
                        error.to_string(),
                    )
                })?;
                self.measure(
                    descriptor,
                    serde_json::to_vec(file).expect("semantic files serialize"),
                )
            }
            ReviewResourceDescriptor::Patch { .. } => {
                self.measure(descriptor, file.patch.as_bytes().to_vec())
            }
            ReviewResourceDescriptor::Source { side, .. } => {
                self.produce_source(descriptor, file, *side)
            }
        }
    }

    fn produce_source(
        &self,
        descriptor: &ReviewResourceDescriptor,
        file: &workdeck_core::SemanticReviewFile,
        side: ReviewSide,
    ) -> ReviewResourceLoad {
        let diff_file = self
            .publication
            .diff_files_by_key
            .get(&descriptor.base().file_key)
            .ok_or_else(|| {
                review_resource_failure(
                    ReviewResourceErrorCode::ResourceUnavailable,
                    format!(
                        "Review file {} has no source reader in this generation.",
                        file.path
                    ),
                )
            })?;
        let text = self
            .source_loader
            .get_full_text(diff_file, side)
            .map_err(|error| match error {
                ReviewSourceLoadError::TooLarge => review_resource_failure(
                    ReviewResourceErrorCode::ResourceTooLarge,
                    format!("Source for {} exceeds the readable limit.", file.path),
                ),
                ReviewSourceLoadError::Unavailable(message) => review_resource_failure(
                    ReviewResourceErrorCode::ResourceUnavailable,
                    format!(
                        "Could not read {side:?} source for {}: {message}",
                        file.path
                    ),
                ),
            })?
            .ok_or_else(|| {
                review_resource_failure(
                    ReviewResourceErrorCode::ResourceUnavailable,
                    format!("Review file {} has no {side:?} source to read.", file.path),
                )
            })?;
        let bytes = text.into_bytes();
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX)
            > review_resource_ceiling(ReviewResourceKind::Source)
        {
            return Err(review_resource_failure(
                ReviewResourceErrorCode::ResourceTooLarge,
                format!(
                    "Source for {} is {} bytes, over the {}-byte source limit.",
                    file.path,
                    bytes.len(),
                    review_resource_ceiling(ReviewResourceKind::Source)
                ),
            ));
        }
        self.measure(descriptor, bytes)
    }

    fn measure(&self, descriptor: &ReviewResourceDescriptor, bytes: Vec<u8>) -> ReviewResourceLoad {
        let byte_length = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        let kind = match descriptor {
            ReviewResourceDescriptor::CanonicalFile { .. } => ReviewResourceKind::CanonicalFile,
            ReviewResourceDescriptor::Patch { .. } => ReviewResourceKind::Patch,
            ReviewResourceDescriptor::Source { .. } => ReviewResourceKind::Source,
        };
        let ceiling = review_resource_ceiling(kind);
        if byte_length > ceiling {
            return Err(review_resource_failure(
                ReviewResourceErrorCode::ResourceTooLarge,
                format!(
                    "Review resource {} is {byte_length} bytes, over the {ceiling}-byte resource limit.",
                    descriptor.base().id
                ),
            ));
        }
        let digest = review_digest(&bytes);
        if descriptor.is_materialized()
            && (descriptor.base().byte_length != Some(byte_length)
                || !descriptor
                    .base()
                    .digest
                    .as_deref()
                    .is_some_and(|declared| review_digests_equal(declared, &digest)))
        {
            return Err(review_resource_failure(
                ReviewResourceErrorCode::ResourceIntegrity,
                format!(
                    "Review resource {} does not match the length and digest it was published with.",
                    descriptor.base().id
                ),
            ));
        }
        Ok(MaterializedReviewResource {
            bytes: bytes.into(),
            byte_length,
            digest,
        })
    }

    fn retain(
        &self,
        state: &mut ResourceStoreState,
        resource_id: &str,
        resource: MaterializedReviewResource,
    ) {
        if let Some(previous) = state
            .materialized
            .insert(resource_id.to_owned(), resource.clone())
        {
            state.cached_bytes = state.cached_bytes.saturating_sub(previous.byte_length);
        }
        state.cached_bytes = state.cached_bytes.saturating_add(resource.byte_length);
        Self::touch(state, resource_id);
        while state.cached_bytes > self.max_cache_bytes {
            let Some(oldest_id) = state.oldest_first.front().cloned() else {
                break;
            };
            if oldest_id == resource_id {
                break;
            }
            state.oldest_first.pop_front();
            if let Some(oldest) = state.materialized.remove(&oldest_id) {
                state.cached_bytes = state.cached_bytes.saturating_sub(oldest.byte_length);
            }
        }
    }

    fn touch(state: &mut ResourceStoreState, resource_id: &str) {
        state.oldest_first.retain(|id| id != resource_id);
        state.oldest_first.push_back(resource_id.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };

    use workdeck_core::{
        DiffHunk, FileChangeKind, FileFlags, FileSourceSnapshots, FileStats, SourceOrigin,
        SourceSnapshot,
    };

    use super::*;
    use crate::{ReviewResourceAddress, review_resource_id};

    fn file(path: &str, patch: &str, source: Option<&str>) -> DiffFile {
        let mut file = DiffFile {
            key: String::new(),
            runtime_id: format!("runtime:{path}"),
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

    fn store(files: &[DiffFile]) -> (Arc<ReviewPublication>, ReviewResourceStore) {
        let publication = Arc::new(crate::build_review_publication(
            files,
            "generation:test:0",
            Some("/repo"),
        ));
        let store =
            ReviewResourceStore::new(ReviewResourceStoreOptions::new(Arc::clone(&publication)));
        (publication, store)
    }

    fn id(kind: ReviewResourceKind, file_key: &str, side: Option<ReviewSide>) -> String {
        review_resource_id(&ReviewResourceAddress {
            kind,
            file_key: file_key.into(),
            side,
        })
    }

    #[test]
    fn serves_patch_canonical_and_source_bytes_with_stable_measurements() {
        let files = [file("a.rs", "@@ patch\n", Some("one\ntwo\n"))];
        let (publication, store) = store(&files);
        let file = &publication.document.files[0];
        let patch_id = id(ReviewResourceKind::Patch, &file.key, None);
        let canonical_id = id(ReviewResourceKind::CanonicalFile, &file.key, None);
        let source_id = id(ReviewResourceKind::Source, &file.key, Some(ReviewSide::New));

        assert_eq!(&*store.materialize(&patch_id).unwrap().bytes, b"@@ patch\n");
        let canonical = store.materialize(&canonical_id).unwrap();
        assert_eq!(
            serde_json::from_slice::<workdeck_core::SemanticReviewFile>(&canonical.bytes).unwrap(),
            *file
        );
        assert_eq!(
            &*store.materialize(&source_id).unwrap().bytes,
            b"one\ntwo\n"
        );
        let measured = store.describe(&patch_id).unwrap();
        assert_eq!(measured.base().byte_length, Some(9));
        assert_eq!(measured.base().digest.as_deref().map(str::len), Some(64));
    }

    #[test]
    fn reads_bounded_base64_windows_empty_content_and_rejects_bad_ranges() {
        let files = [file("a.rs", "abcdefghij", None), file("empty.rs", "", None)];
        let (publication, store) = store(&files);
        let patch = id(
            ReviewResourceKind::Patch,
            &publication.document.files[0].key,
            None,
        );
        let chunk = store
            .read_chunk(
                &patch,
                ReviewResourceRange {
                    offset: 2,
                    length: 4,
                },
            )
            .unwrap();
        assert_eq!(BASE64.decode(chunk.data).unwrap(), b"cdef");
        assert!(!chunk.eof);
        let empty = id(
            ReviewResourceKind::Patch,
            &publication.document.files[1].key,
            None,
        );
        let chunk = store
            .read_chunk(
                &empty,
                ReviewResourceRange {
                    offset: 0,
                    length: 16,
                },
            )
            .unwrap();
        assert_eq!(chunk.byte_length, 0);
        assert!(chunk.eof);
        for range in [
            ReviewResourceRange {
                offset: 0,
                length: 0,
            },
            ReviewResourceRange {
                offset: 0,
                length: crate::REVIEW_RESOURCE_CHUNK_BYTES + 1,
            },
            ReviewResourceRange {
                offset: 99,
                length: 1,
            },
        ] {
            assert_eq!(
                store.read_chunk(&patch, range).unwrap_err().code,
                ReviewResourceErrorCode::InvalidRange
            );
        }
    }

    struct CountingLoader {
        calls: AtomicUsize,
        result: Result<Option<String>, ReviewSourceLoadError>,
    }

    struct BoundedLoader {
        active: AtomicUsize,
        peak: AtomicUsize,
        calls: AtomicUsize,
        wave: Barrier,
    }

    impl ReviewSourceLoader for BoundedLoader {
        fn get_full_text(
            &self,
            _file: &DiffFile,
            _side: ReviewSide,
        ) -> Result<Option<String>, ReviewSourceLoadError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.peak.fetch_max(active, Ordering::SeqCst);
            self.wave.wait();
            self.active.fetch_sub(1, Ordering::SeqCst);
            Ok(Some("source".into()))
        }
    }

    impl ReviewSourceLoader for CountingLoader {
        fn get_full_text(
            &self,
            _file: &DiffFile,
            _side: ReviewSide,
        ) -> Result<Option<String>, ReviewSourceLoadError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.result.clone()
        }
    }

    #[test]
    fn source_failures_remain_distinct_and_success_is_single_flight_cached() {
        for (result, code) in [
            (Ok(None), ReviewResourceErrorCode::ResourceUnavailable),
            (
                Err(ReviewSourceLoadError::Unavailable(
                    "permission denied".into(),
                )),
                ReviewResourceErrorCode::ResourceUnavailable,
            ),
            (
                Err(ReviewSourceLoadError::TooLarge),
                ReviewResourceErrorCode::ResourceTooLarge,
            ),
        ] {
            let source = file("a.rs", "patch", Some("source"));
            let publication = Arc::new(crate::build_review_publication(
                &[source],
                "generation:test:0",
                None,
            ));
            let loader = Arc::new(CountingLoader {
                calls: AtomicUsize::new(0),
                result,
            });
            let mut options = ReviewResourceStoreOptions::new(Arc::clone(&publication));
            options.source_loader = loader;
            let store = ReviewResourceStore::new(options);
            let resource_id = publication
                .resources
                .iter()
                .find(|resource| matches!(resource, ReviewResourceDescriptor::Source { .. }))
                .unwrap()
                .base()
                .id
                .clone();
            assert_eq!(store.materialize(&resource_id).unwrap_err().code, code);
        }

        let source = file("a.rs", "patch", Some("source"));
        let publication = Arc::new(crate::build_review_publication(
            &[source],
            "generation:test:0",
            None,
        ));
        let loader = Arc::new(CountingLoader {
            calls: AtomicUsize::new(0),
            result: Ok(Some("source".into())),
        });
        let mut options = ReviewResourceStoreOptions::new(Arc::clone(&publication));
        options.source_loader = loader.clone();
        let store = Arc::new(ReviewResourceStore::new(options));
        let resource_id = publication.resources[2].base().id.clone();
        std::thread::scope(|scope| {
            let handles = (0..16)
                .map(|_| {
                    let store = Arc::clone(&store);
                    let resource_id = resource_id.clone();
                    scope.spawn(move || store.materialize(&resource_id).unwrap())
                })
                .collect::<Vec<_>>();
            for handle in handles {
                assert_eq!(&*handle.join().unwrap().bytes, b"source");
            }
        });
        assert_eq!(loader.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn bulk_load_deduplicates_and_unknown_and_declared_corruption_are_not_misses() {
        let files = [file("a.rs", "patch-a", None), file("b.rs", "patch-b", None)];
        let mut publication = crate::build_review_publication(&files, "generation:test:0", None);
        let first_id = publication.resources[1].base().id.clone();
        publication.resources[1].base_mut().byte_length = Some(7);
        publication.resources[1].base_mut().digest = Some("0".repeat(64));
        let publication = Arc::new(publication);
        let mut options = ReviewResourceStoreOptions::new(Arc::clone(&publication));
        options.concurrency = 2;
        let store = ReviewResourceStore::new(options);
        let second_id = publication.resources[3].base().id.clone();
        let loaded =
            store.materialize_all(&[first_id.clone(), second_id.clone(), second_id.clone()]);
        assert_eq!(loaded.len(), 2);
        assert_eq!(
            loaded[&first_id].as_ref().unwrap_err().code,
            ReviewResourceErrorCode::ResourceIntegrity
        );
        assert_eq!(
            store
                .materialize("resource:patch:file:deadbeef")
                .unwrap_err()
                .code,
            ReviewResourceErrorCode::UnknownResource
        );
    }

    #[test]
    fn bulk_materialization_never_exceeds_its_explicit_concurrency_limit() {
        let files = (0..12)
            .map(|index| file(&format!("{index}.rs"), "patch", Some("source")))
            .collect::<Vec<_>>();
        let publication = Arc::new(crate::build_review_publication(
            &files,
            "generation:test:0",
            None,
        ));
        let loader = Arc::new(BoundedLoader {
            active: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            wave: Barrier::new(3),
        });
        let mut options = ReviewResourceStoreOptions::new(Arc::clone(&publication));
        options.concurrency = 3;
        options.source_loader = loader.clone();
        let store = ReviewResourceStore::new(options);
        let ids = publication
            .resources
            .iter()
            .filter(|resource| matches!(resource, ReviewResourceDescriptor::Source { .. }))
            .map(|resource| resource.base().id.clone())
            .collect::<Vec<_>>();

        assert_eq!(store.materialize_all(&ids).len(), 12);
        assert_eq!(loader.calls.load(Ordering::SeqCst), 12);
        assert_eq!(loader.peak.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn lru_budget_evicts_the_oldest_untouched_resource() {
        let files = [
            file("a.rs", "aaaa", None),
            file("b.rs", "bbbb", None),
            file("c.rs", "cccc", None),
        ];
        let publication = Arc::new(crate::build_review_publication(
            &files,
            "generation:test:0",
            None,
        ));
        let mut options = ReviewResourceStoreOptions::new(Arc::clone(&publication));
        options.max_cache_bytes = 8;
        let store = ReviewResourceStore::new(options);
        let ids = publication
            .document
            .files
            .iter()
            .map(|file| id(ReviewResourceKind::Patch, &file.key, None))
            .collect::<Vec<_>>();
        store.materialize(&ids[0]).unwrap();
        store.materialize(&ids[1]).unwrap();
        store.materialize(&ids[0]).unwrap();
        store.materialize(&ids[2]).unwrap();

        assert!(store.describe(&ids[0]).unwrap().is_materialized());
        assert!(!store.describe(&ids[1]).unwrap().is_materialized());
        assert!(store.describe(&ids[2]).unwrap().is_materialized());
    }

    struct PanickingLoader;

    impl ReviewSourceLoader for PanickingLoader {
        fn get_full_text(
            &self,
            _file: &DiffFile,
            _side: ReviewSide,
        ) -> Result<Option<String>, ReviewSourceLoadError> {
            panic!("reader details must not escape")
        }
    }

    #[test]
    fn panicking_source_reader_is_contained_and_does_not_poison_the_flight() {
        let source = file("a.rs", "patch", Some("source"));
        let publication = Arc::new(crate::build_review_publication(
            &[source],
            "generation:test:0",
            None,
        ));
        let mut options = ReviewResourceStoreOptions::new(Arc::clone(&publication));
        options.source_loader = Arc::new(PanickingLoader);
        let store = ReviewResourceStore::new(options);
        let resource_id = publication.resources[2].base().id.clone();
        for _ in 0..2 {
            let failure = store.materialize(&resource_id).unwrap_err();
            assert_eq!(failure.code, ReviewResourceErrorCode::ResourceUnavailable);
            assert!(!failure.message.contains("reader details"));
        }
    }
}
