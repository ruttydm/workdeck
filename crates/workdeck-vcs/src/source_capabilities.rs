//! Runtime-owned source capabilities, kept separate from serializable review data.
//!
//! Per-file/per-side caching follows Hunk's MIT-licensed
//! `src/extensions/vcsPatchResult.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.

use crate::{VcsCatalogError, VcsFileSourceRequest, VcsFileSourceResult, VcsSourceReader};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use workdeck_core::{DiffFile, ReviewSide};

static NEXT_SOURCE_RUNTIME_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// One provider invocation bound to one file. Caller-supplied metadata cannot
/// substitute a different path or revision into the captured request.
pub struct VcsFileSourceCapability {
    runtime_identity: u64,
    reader: VcsSourceReader,
    request: VcsFileSourceRequest,
    resolved: Mutex<[Option<VcsFileSourceResult>; 2]>,
}

impl VcsFileSourceCapability {
    pub(crate) fn new(reader: VcsSourceReader, file: &DiffFile) -> Self {
        Self {
            runtime_identity: NEXT_SOURCE_RUNTIME_ID
                .fetch_update(
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                    |identity| identity.checked_add(1),
                )
                .expect("VCS source runtime identity exhausted"),
            reader,
            request: VcsFileSourceRequest {
                path: file.path.clone(),
                previous_path: file.previous_path.clone(),
                change_kind: file.change_kind,
                is_untracked: file.flags.untracked,
                side: ReviewSide::Old,
            },
            resolved: Mutex::new([None, None]),
        }
    }

    pub fn read(&self, side: ReviewSide) -> Result<VcsFileSourceResult, VcsCatalogError> {
        let index = match side {
            ReviewSide::Old => 0,
            ReviewSide::New => 1,
        };
        if let Some(value) = &self
            .resolved
            .lock()
            .unwrap_or_else(|error| error.into_inner())[index]
        {
            return Ok(value.clone());
        }
        let mut request = self.request.clone();
        request.side = side;
        // Do not cache in-flight requests or ordinary errors. Missing and typed
        // size failures are resolved results and remain cached independently.
        let value = (self.reader)(&request)?;
        self.resolved
            .lock()
            .unwrap_or_else(|error| error.into_inner())[index] = Some(value.clone());
        Ok(value)
    }

    /// Process-local identity of this executable reader, never a serialized source key.
    pub fn runtime_identity(&self) -> u64 {
        self.runtime_identity
    }
}

#[derive(Clone)]
struct BoundCapability {
    source_identity: Option<String>,
    capability: Arc<VcsFileSourceCapability>,
}

/// Cloneable ownership of executable providers, never serialized with a changeset.
#[derive(Clone, Default)]
pub struct VcsSourceCapabilities {
    files: BTreeMap<String, BoundCapability>,
}

impl std::fmt::Debug for VcsSourceCapabilities {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VcsSourceCapabilities")
            .field("file_count", &self.files.len())
            .finish_non_exhaustive()
    }
}

impl VcsSourceCapabilities {
    /// Update one host-owned file's identity without changing its executable reader.
    pub fn rebind_file(&mut self, original: &DiffFile, replacement: &DiffFile) {
        let capability = self.get(original);
        self.files.remove(&original.key);
        if let Some(capability) = capability {
            self.insert(replacement, capability);
        }
    }

    /// Carry existing authority across a host-validated file transformation.
    /// Input pairs must name the original host file and its validated replacement;
    /// public metadata alone never constructs a reader or changes its captured request.
    pub fn rebind<'a>(
        &self,
        pairs: impl IntoIterator<Item = (&'a DiffFile, &'a DiffFile)>,
    ) -> Self {
        let mut rebound = Self::default();
        for (original, replacement) in pairs {
            if let Some(capability) = self.get(original) {
                rebound.insert(replacement, capability);
            }
        }
        rebound
    }

    /// Retire handles without affecting already captured publication generations.
    pub fn retire(&mut self, keys: &std::collections::BTreeSet<String>) {
        self.files.retain(|key, _| !keys.contains(key));
    }

    /// Materialize a consumer-owned copy through captured provider authority.
    /// The immutable review document and serialized descriptors remain unchanged.
    pub fn with_source_snapshots(&self, file: &DiffFile) -> Result<DiffFile, VcsCatalogError> {
        let mut result = file.clone();
        if let Some(capability) = self.get(file) {
            let snapshot = |side| {
                capability.read(side).map(|value| match value {
                    VcsFileSourceResult::Source(snapshot) => Some(snapshot),
                    VcsFileSourceResult::Missing | VcsFileSourceResult::TooLarge { .. } => None,
                })
            };
            result.set_sources(workdeck_core::FileSourceSnapshots {
                old: snapshot(ReviewSide::Old)?,
                new: snapshot(ReviewSide::New)?,
            });
        }
        Ok(result)
    }

    pub(crate) fn insert(&mut self, file: &DiffFile, capability: Arc<VcsFileSourceCapability>) {
        self.files.insert(
            file.key.clone(),
            BoundCapability {
                source_identity: file.source_identity.clone(),
                capability,
            },
        );
    }

    /// A matching descriptor is necessary but not sufficient: only this registry
    /// owns authority. Unknown files or retired identities have no executable reader.
    pub fn get(&self, file: &DiffFile) -> Option<Arc<VcsFileSourceCapability>> {
        self.files
            .get(&file.key)
            .filter(|bound| bound.source_identity == file.source_identity)
            .map(|bound| Arc::clone(&bound.capability))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use workdeck_core::{ChangesetSource, SourceCapabilityIdentity, SourceOrigin, SourceSnapshot};

    fn file() -> DiffFile {
        let mut changeset = workdeck_diff::changeset_from_patch(
            "diff --git a/source.txt b/source.txt\n--- a/source.txt\n+++ b/source.txt\n@@ -1 +1 @@\n-old\n+new\n",
            "review",
            "review",
            "review",
            ChangesetSource::WorkingTree { staged: false },
            None,
        );
        let mut file = changeset.files.remove(0);
        file.set_source_capability(Some(SourceCapabilityIdentity {
            cache_key: Some("snapshot".into()),
        }));
        file
    }

    #[test]
    fn resolved_sides_and_typed_limits_are_cached_per_file_not_per_path() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let reader: VcsSourceReader = Arc::new(move |request| {
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(match request.side {
                ReviewSide::Old => VcsFileSourceResult::Missing,
                ReviewSide::New => VcsFileSourceResult::TooLarge { max_bytes: 42 },
            })
        });
        let first = VcsFileSourceCapability::new(Arc::clone(&reader), &file());
        let second = VcsFileSourceCapability::new(reader, &file());
        assert_ne!(first.runtime_identity(), second.runtime_identity());
        for _ in 0..3 {
            assert_eq!(
                first.read(ReviewSide::Old).unwrap(),
                VcsFileSourceResult::Missing
            );
            assert_eq!(
                first.read(ReviewSide::New).unwrap(),
                VcsFileSourceResult::TooLarge { max_bytes: 42 }
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            second.read(ReviewSide::Old).unwrap(),
            VcsFileSourceResult::Missing
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn language_rebinding_preserves_only_existing_reader_authority() {
        let original = file();
        let reader: VcsSourceReader = Arc::new(|request| {
            assert_eq!(request.path, "source.txt");
            Ok(VcsFileSourceResult::Missing)
        });
        let capability = Arc::new(VcsFileSourceCapability::new(reader, &original));
        let mut registry = VcsSourceCapabilities::default();
        registry.insert(&original, Arc::clone(&capability));
        let mut replacement = original.clone();
        replacement.language = Some("rust".into());
        replacement.refresh_identity();
        assert_ne!(replacement.source_identity, original.source_identity);
        registry.rebind_file(&original, &replacement);
        assert!(registry.get(&original).is_none());
        assert!(Arc::ptr_eq(
            &registry.get(&replacement).unwrap(),
            &capability
        ));
        assert_eq!(
            registry
                .get(&replacement)
                .unwrap()
                .read(ReviewSide::New)
                .unwrap(),
            VcsFileSourceResult::Missing
        );
        let mut empty = VcsSourceCapabilities::default();
        empty.rebind_file(&original, &replacement);
        assert!(empty.get(&replacement).is_none());
    }

    #[test]
    fn ordinary_errors_retry_and_success_retains_snapshot_metadata() {
        let calls = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&calls);
        let snapshot = SourceSnapshot::new(
            "loaded".into(),
            SourceOrigin::Revision {
                revision: "abc".into(),
            },
            true,
        );
        let expected = VcsFileSourceResult::Source(snapshot.clone());
        let capability = VcsFileSourceCapability::new(
            Arc::new(move |_| {
                if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(VcsCatalogError::Operation("retry me".into()))
                } else {
                    Ok(VcsFileSourceResult::Source(snapshot.clone()))
                }
            }),
            &file(),
        );
        assert!(capability.read(ReviewSide::Old).is_err());
        assert_eq!(capability.read(ReviewSide::Old).unwrap(), expected);
        assert_eq!(capability.read(ReviewSide::Old).unwrap(), expected);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn registry_requires_runtime_authority_and_keeps_the_captured_request() {
        let original = file();
        let capability = Arc::new(VcsFileSourceCapability::new(
            Arc::new(|request| {
                assert_eq!(request.path, "source.txt");
                Ok(VcsFileSourceResult::Missing)
            }),
            &original,
        ));
        let mut registry = VcsSourceCapabilities::default();
        assert!(registry.get(&original).is_none());
        registry.insert(&original, Arc::clone(&capability));
        assert!(Arc::ptr_eq(&registry.get(&original).unwrap(), &capability));
        let mut altered = original.clone();
        altered.key.push_str("-unknown");
        assert!(registry.get(&altered).is_none());
        altered = original.clone();
        altered.set_source_capability(Some(SourceCapabilityIdentity {
            cache_key: Some("replacement".into()),
        }));
        assert!(registry.get(&altered).is_none());
        altered = serde_json::from_value(serde_json::to_value(&original).unwrap()).unwrap();
        altered.path = "not-authorized.txt".into();
        assert_eq!(
            registry
                .get(&altered)
                .unwrap()
                .read(ReviewSide::Old)
                .unwrap(),
            VcsFileSourceResult::Missing
        );
        assert!(VcsSourceCapabilities::default().get(&altered).is_none());
    }
}
