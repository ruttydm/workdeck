//! Runtime source-load presentation, separate from immutable provider snapshots.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use workdeck_core::{DiffFile, ReviewSide, SourceOrigin};
use workdeck_review::{
    ExpandedSourceError, ExpandedSourceStatus, ReviewSourceErrorReason, ReviewSourceStatus,
    review_expansion_side,
};

#[derive(Debug, Clone)]
struct Entry {
    source_identity: Option<String>,
    status: Option<Arc<ReviewSourceStatus>>,
}

/// Presentation is embedded in `ReviewOptions`, which render paths clone per
/// frame, so the file table is shared and only deep-copied on mutation.
#[derive(Debug, Clone, Default)]
pub struct ReviewSourcePresentation {
    files: Arc<BTreeMap<String, Entry>>,
    revision: u64,
}

impl ReviewSourcePresentation {
    /// Monotonic revision of the presentation table. Availability affects
    /// review geometry through expandable gap targets, so geometry caches key
    /// on this value instead of disabling themselves while snapshot-backed
    /// sources exist.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn retire(&mut self, keys: &std::collections::BTreeSet<String>) {
        Arc::make_mut(&mut self.files).retain(|key, _| !keys.contains(key));
        self.revision += 1;
    }
    /// Register presentation for a runtime-owned reader. This grants no I/O authority.
    pub fn pending(&mut self, file: &DiffFile) {
        Arc::make_mut(&mut self.files).insert(
            file.key.clone(),
            Entry {
                source_identity: file.source_identity.clone(),
                status: None,
            },
        );
        self.revision += 1;
    }

    pub fn set_status(&mut self, file: &DiffFile, status: ReviewSourceStatus) {
        Arc::make_mut(&mut self.files).insert(
            file.key.clone(),
            Entry {
                source_identity: file.source_identity.clone(),
                status: Some(Arc::new(status)),
            },
        );
        self.revision += 1;
    }

    fn entry(&self, file: &DiffFile) -> Option<&Entry> {
        self.files
            .get(&file.key)
            .filter(|entry| entry.source_identity == file.source_identity)
    }

    pub fn status(&self, file: &DiffFile) -> Option<&ReviewSourceStatus> {
        self.entry(file).and_then(|entry| entry.status.as_deref())
    }

    pub fn available(&self, file: &DiffFile) -> bool {
        self.entry(file).is_some() || snapshot_text(file).is_some()
    }

    pub fn expanded_status<'a>(&'a self, file: &'a DiffFile) -> ExpandedSourceStatus<'a> {
        if let Some(entry) = self.entry(file) {
            return match entry.status.as_deref() {
                None => ExpandedSourceStatus::Pending,
                Some(ReviewSourceStatus::Loading) => ExpandedSourceStatus::Loading,
                Some(ReviewSourceStatus::Loaded { text }) => ExpandedSourceStatus::Loaded(text),
                Some(ReviewSourceStatus::Error { reason }) => {
                    ExpandedSourceStatus::Error(match reason {
                        Some(ReviewSourceErrorReason::TooLarge) => ExpandedSourceError::TooLarge,
                        None => ExpandedSourceError::Unavailable,
                    })
                }
            };
        }
        snapshot_text(file).map_or(
            ExpandedSourceStatus::Error(ExpandedSourceError::Unavailable),
            ExpandedSourceStatus::Loaded,
        )
    }

    pub fn text<'a>(&'a self, file: &'a DiffFile) -> Option<&'a str> {
        match self.expanded_status(file) {
            ExpandedSourceStatus::Loaded(text) => Some(text),
            _ => None,
        }
    }

    /// Match the shared reducer's attestation rule when a document is replaced.
    pub fn reconcile(&mut self, files: &[DiffFile]) {
        let attested_files: BTreeSet<_> = files
            .iter()
            .filter(|file| file.source_attested)
            .map(|file| (file.key.as_str(), file.source_identity.as_deref()))
            .collect();
        let before = self.files.len();
        Arc::make_mut(&mut self.files).retain(|key, entry| {
            attested_files.contains(&(key.as_str(), entry.source_identity.as_deref()))
        });
        if self.files.len() != before {
            self.revision += 1;
        }
    }
}

fn snapshot_text(file: &DiffFile) -> Option<&str> {
    match review_expansion_side(file.change_kind) {
        ReviewSide::Old => file.sources.old.as_ref(),
        ReviewSide::New => file.sources.new.as_ref(),
    }
    .filter(|source| source.origin != SourceOrigin::DiffMetadata)
    .map(|source| source.content.as_str())
}
