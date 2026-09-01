use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use workdeck_extension_api::ExtensionFileSide;

type DocumentFetcher =
    Arc<dyn Fn(ExtensionFileSide) -> Result<Option<String>, String> + Send + Sync + 'static>;

#[derive(Debug, Clone, Default)]
pub struct ExtensionRequestCancellation(Arc<AtomicBool>);

impl ExtensionRequestCancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    #[must_use]
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DocumentReadError {
    #[error("The extension request was aborted.")]
    Aborted,
}

#[derive(Debug, Default)]
struct SharedDocumentRead {
    result: Mutex<Option<Option<String>>>,
    ready: Condvar,
}

/// One caller's cancellable wait on a deduplicated source read.
#[derive(Debug, Clone)]
pub struct ExtensionDocumentRead {
    shared: Arc<SharedDocumentRead>,
}

impl ExtensionDocumentRead {
    pub fn wait(
        &self,
        cancellation: &ExtensionRequestCancellation,
    ) -> Result<Option<String>, DocumentReadError> {
        if cancellation.is_cancelled() {
            return Err(DocumentReadError::Aborted);
        }
        let mut result = self
            .shared
            .result
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if cancellation.is_cancelled() {
                return Err(DocumentReadError::Aborted);
            }
            if let Some(value) = result.as_ref() {
                return Ok(value.clone());
            }
            result = self
                .shared
                .ready
                .wait_timeout(result, Duration::from_millis(5))
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .0;
        }
    }
}

/// Shared `readDocument(side)` capability for one native layout request.
///
/// The first read for each side starts one host operation. Later callers wait
/// on the same result, while cancellation only ends their waits and never
/// interrupts the shared host read.
#[derive(Clone)]
pub struct ExtensionDocumentReader {
    fetcher: DocumentFetcher,
    reads: Arc<Mutex<BTreeMap<ExtensionFileSide, Arc<SharedDocumentRead>>>>,
}

impl std::fmt::Debug for ExtensionDocumentReader {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let read_count = self
            .reads
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len();
        formatter
            .debug_struct("ExtensionDocumentReader")
            .field("read_count", &read_count)
            .finish_non_exhaustive()
    }
}

impl ExtensionDocumentReader {
    #[must_use]
    pub fn new(
        fetcher: impl Fn(ExtensionFileSide) -> Result<Option<String>, String> + Send + Sync + 'static,
    ) -> Self {
        Self {
            fetcher: Arc::new(fetcher),
            reads: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    #[must_use]
    pub fn read_document(&self, side: ExtensionFileSide) -> ExtensionDocumentRead {
        let (shared, start) = {
            let mut reads = self
                .reads
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if let Some(shared) = reads.get(&side) {
                (Arc::clone(shared), false)
            } else {
                let shared = Arc::new(SharedDocumentRead::default());
                reads.insert(side, Arc::clone(&shared));
                (shared, true)
            }
        };
        if start {
            let fetcher = Arc::clone(&self.fetcher);
            let task = Arc::clone(&shared);
            std::thread::spawn(move || {
                let value =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| fetcher(side)))
                        .ok()
                        .and_then(Result::ok)
                        .flatten();
                let mut result = task
                    .result
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                *result = Some(value);
                task.ready.notify_all();
            });
        }
        ExtensionDocumentRead { shared }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;

    #[test]
    fn deduplicates_reads_and_cancellation_only_ends_callers_waits() {
        let (called_tx, called_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Arc::new(Mutex::new(release_rx));
        let reader = ExtensionDocumentReader::new({
            let release_rx = Arc::clone(&release_rx);
            move |side| {
                called_tx.send(side).unwrap();
                release_rx.lock().unwrap().recv().unwrap();
                Ok(Some("after".into()))
            }
        });
        let first = reader.read_document(ExtensionFileSide::New);
        let second = reader.read_document(ExtensionFileSide::New);
        assert_eq!(called_rx.recv().unwrap(), ExtensionFileSide::New);
        assert!(called_rx.try_recv().is_err());

        let cancellation = ExtensionRequestCancellation::default();
        cancellation.cancel();
        assert_eq!(first.wait(&cancellation), Err(DocumentReadError::Aborted));
        assert_eq!(second.wait(&cancellation), Err(DocumentReadError::Aborted));
        release_tx.send(()).unwrap();
    }

    #[test]
    fn returns_exact_document_text_and_maps_fetch_failures_to_missing() {
        let cancellation = ExtensionRequestCancellation::default();
        let reader = ExtensionDocumentReader::new(|_| Ok(Some("after\n".into())));
        assert_eq!(
            reader
                .read_document(ExtensionFileSide::New)
                .wait(&cancellation),
            Ok(Some("after\n".into()))
        );

        let missing = ExtensionDocumentReader::new(|_| Err("unreadable".into()));
        assert_eq!(
            missing
                .read_document(ExtensionFileSide::Old)
                .wait(&cancellation),
            Ok(None)
        );
    }
}
