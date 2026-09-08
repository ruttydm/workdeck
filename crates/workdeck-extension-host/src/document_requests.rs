//! Request-scoped source authority for native document callbacks.

use std::collections::{BTreeMap, BTreeSet};

use workdeck_extension_api::ExtensionFileSide;

use crate::{ExtensionDocumentRead, ExtensionDocumentReader};

pub const MAX_PENDING_DOCUMENT_REQUESTS: usize = 32;
pub const MAX_DOCUMENT_REQUEST_IDS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DocumentRequestError {
    #[error("document request belongs to a different parent request")]
    WrongParent,
    #[error("document request owner has retired")]
    Retired,
    #[error("document request ID was already used")]
    DuplicateId,
    #[error("document request limit exceeded")]
    Limit,
}

/// Serves only the captured reader of one active parent operation.
/// Child-supplied paths cannot grant or redirect source authority.
#[derive(Debug)]
pub struct ExtensionDocumentRequests {
    parent_id: u64,
    reader: Option<ExtensionDocumentReader>,
    pending: BTreeMap<u64, ExtensionDocumentRead>,
    used_ids: BTreeSet<u64>,
}

impl ExtensionDocumentRequests {
    #[must_use]
    pub fn new(parent_id: u64, reader: ExtensionDocumentReader) -> Self {
        Self {
            parent_id,
            reader: Some(reader),
            pending: BTreeMap::new(),
            used_ids: BTreeSet::new(),
        }
    }

    pub fn request(
        &mut self,
        id: u64,
        parent_id: u64,
        side: ExtensionFileSide,
    ) -> Result<(), DocumentRequestError> {
        let reader = self.reader.as_ref().ok_or(DocumentRequestError::Retired)?;
        if parent_id != self.parent_id {
            return Err(DocumentRequestError::WrongParent);
        }
        if self.used_ids.contains(&id) {
            return Err(DocumentRequestError::DuplicateId);
        }
        if self.pending.len() >= MAX_PENDING_DOCUMENT_REQUESTS
            || self.used_ids.len() >= MAX_DOCUMENT_REQUEST_IDS
        {
            return Err(DocumentRequestError::Limit);
        }
        self.used_ids.insert(id);
        self.pending.insert(id, reader.read_document(side));
        Ok(())
    }

    /// Return ready responses without waiting for source I/O. IDs remain spent.
    pub fn poll(&mut self) -> Vec<(u64, Option<String>)> {
        std::iter::from_fn(|| self.poll_next()).collect()
    }

    /// Drain one ready response, avoiding a batch of copied source documents.
    /// A pending earlier ID does not block a ready later ID.
    pub fn poll_next(&mut self) -> Option<(u64, Option<String>)> {
        let ready = self
            .pending
            .iter()
            .find_map(|(id, read)| read.try_result().map(|value| (*id, value)))?;
        self.pending.remove(&ready.0);
        Some(ready)
    }

    /// Revoke publication and new requests without cancelling shared source I/O.
    pub fn retire(&mut self) {
        self.reader = None;
        self.pending.clear();
        self.used_ids.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};

    #[test]
    fn single_response_poll_leaves_other_ready_requests_pending() {
        let reader = ExtensionDocumentReader::new(|_| Ok(Some("source".into())));
        reader
            .read_document(ExtensionFileSide::New)
            .wait_until(
                &crate::ExtensionRequestCancellation::default(),
                Instant::now() + Duration::from_secs(5),
            )
            .unwrap();
        let mut requests = ExtensionDocumentRequests::new(7, reader);
        for id in 0..MAX_PENDING_DOCUMENT_REQUESTS as u64 {
            requests.request(id, 7, ExtensionFileSide::New).unwrap();
        }
        for id in 0..MAX_PENDING_DOCUMENT_REQUESTS as u64 {
            assert_eq!(requests.poll_next(), Some((id, Some("source".into()))));
            assert_eq!(
                requests.pending.len(),
                MAX_PENDING_DOCUMENT_REQUESTS - id as usize - 1
            );
            assert_eq!(
                requests.request(id, 7, ExtensionFileSide::New),
                Err(DocumentRequestError::DuplicateId)
            );
        }
        assert_eq!(requests.poll_next(), None);
    }

    #[test]
    fn validates_authority_and_limits_before_starting_reads() {
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_rx = Mutex::new(release_rx);
        let observed = calls.clone();
        let reader = ExtensionDocumentReader::new(move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
            started_tx.send(()).unwrap();
            release_rx
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
            Ok(Some("source".into()))
        });
        let retained = reader.clone();
        let mut requests = ExtensionDocumentRequests::new(7, reader);
        assert_eq!(
            requests.request(1, 8, ExtensionFileSide::New),
            Err(DocumentRequestError::WrongParent)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        for id in 0..MAX_PENDING_DOCUMENT_REQUESTS as u64 {
            requests.request(id, 7, ExtensionFileSide::New).unwrap();
        }
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            requests.request(0, 7, ExtensionFileSide::Old),
            Err(DocumentRequestError::DuplicateId)
        );
        assert_eq!(
            requests.request(99, 7, ExtensionFileSide::Old),
            Err(DocumentRequestError::Limit)
        );
        assert!(requests.poll().is_empty());
        requests.retire();
        assert_eq!(
            requests.request(99, 7, ExtensionFileSide::Old),
            Err(DocumentRequestError::Retired)
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            retained
                .read_document(ExtensionFileSide::New)
                .wait_until(
                    &crate::ExtensionRequestCancellation::default(),
                    Instant::now() + Duration::from_secs(5)
                )
                .unwrap(),
            Some("source".into())
        );
        assert!(requests.poll().is_empty());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn completed_ids_cannot_be_replayed_and_total_ids_are_bounded() {
        let reader = ExtensionDocumentReader::new(|_| Ok(None));
        let mut requests = ExtensionDocumentRequests::new(7, reader);
        for id in 0..MAX_DOCUMENT_REQUEST_IDS as u64 {
            requests.request(id, 7, ExtensionFileSide::Old).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let ready = requests.poll();
                if !ready.is_empty() {
                    assert_eq!(ready, [(id, None)]);
                    break;
                }
                assert!(Instant::now() < deadline);
                std::thread::yield_now();
            }
        }
        assert_eq!(
            requests.request(0, 7, ExtensionFileSide::Old),
            Err(DocumentRequestError::DuplicateId)
        );
        assert_eq!(
            requests.request(999, 7, ExtensionFileSide::New),
            Err(DocumentRequestError::Limit)
        );
    }
}
