//! Asynchronous source-request ownership translated from Hunk's MIT-licensed
//! `src/ui/hooks/useTerminalReview.ts` at
//! `2c00f4358b89cfc0a6b04459ffc538ba601aa3c2`.
//!
//! Workers only read sources and send completions. The review thread owns state
//! transitions; retired or superseded workers cannot mutate its current state.

use crate::{
    ReviewSourceErrorReason, ReviewSourceLoadError, ReviewSourceLoader, ReviewSourceStatus,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, mpsc},
};
use workdeck_core::{DiffFile, ReviewSide};

#[derive(Clone)]
struct Request {
    id: u64,
    side: ReviewSide,
    loader: Arc<dyn ReviewSourceLoader>,
}

struct Completion {
    key: String,
    path: String,
    runtime_id: String,
    request: Request,
    result: Result<Option<String>, ReviewSourceLoadError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSourceUpdate {
    pub file_key: String,
    pub status: ReviewSourceStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReviewSourceCompletion {
    pub update: Option<ReviewSourceUpdate>,
    /// Stale failures remain diagnosable without replacing current review state.
    pub diagnostic: Option<String>,
}

pub struct ReviewSourceRequests {
    next_id: u64,
    pending: BTreeMap<String, Request>,
    sender: mpsc::Sender<Completion>,
    receiver: mpsc::Receiver<Completion>,
}

impl Default for ReviewSourceRequests {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            next_id: 0,
            pending: BTreeMap::new(),
            sender,
            receiver,
        }
    }
}

impl ReviewSourceRequests {
    /// The loader must come from the runtime owner, never from deserialized data.
    /// The caller applies the returned loading status before polling completions.
    pub fn start(
        &mut self,
        file: &DiffFile,
        side: ReviewSide,
        loader: Arc<dyn ReviewSourceLoader>,
        current_status: Option<&ReviewSourceStatus>,
    ) -> Option<ReviewSourceUpdate> {
        if matches!(
            current_status,
            Some(ReviewSourceStatus::Loading | ReviewSourceStatus::Loaded { .. })
        ) {
            return None;
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("source request IDs exhausted");
        let request = Request { id, side, loader };
        self.pending.insert(file.key.clone(), request.clone());
        let file = file.clone();
        let key = file.key.clone();
        let path = file.path.clone();
        let runtime_id = file.runtime_id.clone();
        let sender = self.sender.clone();
        let worker_request = request.clone();
        let spawned = std::thread::Builder::new()
            .name("workdeck-source".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker_request.loader.get_full_text(&file, side)
                }))
                .unwrap_or_else(|_| {
                    Err(ReviewSourceLoadError::Unavailable(
                        "source reader panicked".into(),
                    ))
                });
                let _ = sender.send(Completion {
                    key: file.key,
                    path: file.path,
                    runtime_id: file.runtime_id,
                    request: worker_request,
                    result,
                });
            });
        if let Err(error) = spawned {
            let _ = self.sender.send(Completion {
                key: key.clone(),
                path,
                runtime_id,
                request,
                result: Err(ReviewSourceLoadError::Unavailable(error.to_string())),
            });
        }
        Some(ReviewSourceUpdate {
            file_key: key,
            status: ReviewSourceStatus::Loading,
        })
    }

    /// Use the shared document retirement policy before applying a replacement.
    pub fn retire(&mut self, file_keys: &BTreeSet<String>) {
        self.pending.retain(|key, _| !file_keys.contains(key));
    }

    pub fn poll(&mut self) -> Vec<ReviewSourceCompletion> {
        let completions: Vec<_> = self.receiver.try_iter().collect();
        completions
            .into_iter()
            .map(|completion| self.settle(completion))
            .collect()
    }

    fn settle(&mut self, completion: Completion) -> ReviewSourceCompletion {
        let current = self.pending.get(&completion.key).is_some_and(|request| {
            request.id == completion.request.id
                && request.side == completion.request.side
                && Arc::ptr_eq(&request.loader, &completion.request.loader)
        });
        let diagnostic = completion.result.as_ref().err().and_then(|error| {
            if current && matches!(error, ReviewSourceLoadError::TooLarge) {
                None
            } else {
                let side = match completion.request.side {
                    ReviewSide::Old => "old",
                    ReviewSide::New => "new",
                };
                let action = if current {
                    format!("failed to load {side} source")
                } else {
                    format!("ignored stale {side} source load failure")
                };
                Some(format!(
                    "workdeck: {action} for {} ({}). {error}",
                    completion.path, completion.runtime_id
                ))
            }
        });
        if !current {
            return ReviewSourceCompletion {
                update: None,
                diagnostic,
            };
        }
        self.pending.remove(&completion.key);
        let status = match completion.result {
            Ok(Some(text)) => ReviewSourceStatus::Loaded { text },
            Err(ReviewSourceLoadError::TooLarge) => ReviewSourceStatus::Error {
                reason: Some(ReviewSourceErrorReason::TooLarge),
            },
            Ok(None) | Err(ReviewSourceLoadError::Unavailable(_)) => {
                ReviewSourceStatus::Error { reason: None }
            }
        };
        ReviewSourceCompletion {
            update: Some(ReviewSourceUpdate {
                file_key: completion.key,
                status,
            }),
            diagnostic,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{sync::Mutex, time::Duration};

    fn file() -> DiffFile {
        DiffFile {
            key: "file".into(),
            runtime_id: "runtime".into(),
            path: "source.txt".into(),
            previous_path: None,
            change_kind: workdeck_core::FileChangeKind::Modified,
            language: None,
            stats: Default::default(),
            flags: Default::default(),
            patch: String::new(),
            split_row_count: 0,
            stack_row_count: 0,
            hunks: vec![],
            content_identity: "content".into(),
            sources: Default::default(),
            source_identity: Some("source".into()),
            source_capability: None,
            source_attested: false,
            agent: None,
        }
    }

    type SourceResult = Result<Option<String>, ReviewSourceLoadError>;

    struct ControlledLoader {
        receiver: Mutex<mpsc::Receiver<SourceResult>>,
    }

    impl ReviewSourceLoader for ControlledLoader {
        fn get_full_text(
            &self,
            _: &DiffFile,
            _: ReviewSide,
        ) -> Result<Option<String>, ReviewSourceLoadError> {
            self.receiver
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
        }
    }

    fn loader() -> (Arc<dyn ReviewSourceLoader>, mpsc::Sender<SourceResult>) {
        let (sender, receiver) = mpsc::channel();
        (
            Arc::new(ControlledLoader {
                receiver: Mutex::new(receiver),
            }),
            sender,
        )
    }

    fn next(requests: &mut ReviewSourceRequests) -> ReviewSourceCompletion {
        let completion = requests
            .receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        requests.settle(completion)
    }

    #[test]
    fn starts_without_waiting_and_skips_loading_or_loaded_status() {
        let (loader, sender) = loader();
        let mut requests = ReviewSourceRequests::default();
        let update = requests
            .start(&file(), ReviewSide::New, Arc::clone(&loader), None)
            .unwrap();
        assert_eq!(update.status, ReviewSourceStatus::Loading);
        assert!(requests.poll().is_empty());
        for status in [
            ReviewSourceStatus::Loading,
            ReviewSourceStatus::Loaded {
                text: "existing".into(),
            },
        ] {
            assert!(
                requests
                    .start(&file(), ReviewSide::New, Arc::clone(&loader), Some(&status))
                    .is_none()
            );
        }
        sender.send(Ok(Some("loaded".into()))).unwrap();
        assert_eq!(
            next(&mut requests),
            ReviewSourceCompletion {
                update: Some(ReviewSourceUpdate {
                    file_key: "file".into(),
                    status: ReviewSourceStatus::Loaded {
                        text: "loaded".into()
                    }
                }),
                diagnostic: None,
            }
        );
        assert!(requests.pending.is_empty());
    }

    #[test]
    fn missing_and_failed_results_are_retryable_and_size_failure_is_typed() {
        let mut requests = ReviewSourceRequests::default();
        for (result, reason, logs) in [
            (Ok(None), None, false),
            (
                Err(ReviewSourceLoadError::Unavailable("broken".into())),
                None,
                true,
            ),
            (
                Err(ReviewSourceLoadError::TooLarge),
                Some(ReviewSourceErrorReason::TooLarge),
                false,
            ),
        ] {
            let (loader, sender) = loader();
            assert!(
                requests
                    .start(
                        &file(),
                        ReviewSide::Old,
                        loader,
                        Some(&ReviewSourceStatus::Error { reason: None })
                    )
                    .is_some()
            );
            sender.send(result).unwrap();
            let completion = next(&mut requests);
            assert_eq!(
                completion.update.unwrap().status,
                ReviewSourceStatus::Error { reason }
            );
            assert_eq!(completion.diagnostic.is_some(), logs);
            assert!(requests.pending.is_empty());
        }
    }

    #[test]
    fn retired_failure_cannot_replace_a_new_request_and_is_still_diagnosed() {
        let (first, first_sender) = loader();
        let (second, second_sender) = loader();
        let mut requests = ReviewSourceRequests::default();
        requests.start(&file(), ReviewSide::Old, first, None);
        requests.retire(&BTreeSet::from(["file".into()]));
        requests.start(&file(), ReviewSide::New, second, None);
        first_sender
            .send(Err(ReviewSourceLoadError::TooLarge))
            .unwrap();
        let stale = next(&mut requests);
        assert!(stale.update.is_none());
        assert!(stale.diagnostic.unwrap().contains("ignored stale"));
        assert_eq!(requests.pending.len(), 1);
        second_sender.send(Ok(Some("current".into()))).unwrap();
        assert_eq!(
            next(&mut requests).update.unwrap().status,
            ReviewSourceStatus::Loaded {
                text: "current".into()
            }
        );
        assert!(requests.pending.is_empty());
    }

    #[test]
    fn request_id_side_and_loader_identity_all_guard_settlement() {
        let (loader, _) = loader();
        let request = Request {
            id: 4,
            side: ReviewSide::Old,
            loader: Arc::clone(&loader),
        };
        let mut requests = ReviewSourceRequests::default();
        requests.pending.insert("file".into(), request.clone());
        for mismatch in 0..3 {
            let mut stale = request.clone();
            match mismatch {
                0 => stale.id += 1,
                1 => stale.side = ReviewSide::New,
                _ => stale.loader = Arc::new(crate::SnapshotReviewSourceLoader),
            }
            let completion = requests.settle(Completion {
                key: "file".into(),
                path: "source.txt".into(),
                runtime_id: "runtime".into(),
                request: stale,
                result: Ok(Some("stale".into())),
            });
            assert!(completion.update.is_none());
            assert!(completion.diagnostic.is_none());
            assert_eq!(requests.pending.len(), 1);
        }
    }

    #[test]
    fn retired_success_is_ignored_and_unrelated_retirement_keeps_the_request() {
        let (reader, sender) = loader();
        let mut requests = ReviewSourceRequests::default();
        requests.start(&file(), ReviewSide::New, reader, None);
        requests.retire(&BTreeSet::from(["unrelated".into()]));
        assert_eq!(requests.pending.len(), 1);
        requests.retire(&BTreeSet::from(["file".into()]));
        sender.send(Ok(Some("obsolete".into()))).unwrap();
        assert_eq!(
            next(&mut requests),
            ReviewSourceCompletion {
                update: None,
                diagnostic: None
            }
        );
        assert!(requests.pending.is_empty());
    }
}
