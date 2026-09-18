//! Synchronous framework-free storage for semantic review state.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, Weak},
};

use workdeck_core::SemanticReviewDocument;

use crate::{SemanticReviewAction, SemanticReviewState, reduce_semantic_review_state};

type Listener = Arc<dyn Fn() + Send + Sync + 'static>;

struct StoreInner {
    snapshot: Arc<SemanticReviewState>,
    next_listener_id: u64,
    listeners: BTreeMap<u64, Listener>,
}

#[derive(Clone)]
pub struct SemanticReviewStore {
    inner: Arc<Mutex<StoreInner>>,
}

pub struct ReviewSubscription {
    inner: Weak<Mutex<StoreInner>>,
    listener_id: Option<u64>,
}

impl ReviewSubscription {
    pub fn unsubscribe(mut self) {
        self.remove();
    }

    fn remove(&mut self) {
        let Some(listener_id) = self.listener_id.take() else {
            return;
        };
        if let Some(inner) = self.inner.upgrade() {
            inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .listeners
                .remove(&listener_id);
        }
    }
}

impl Drop for ReviewSubscription {
    fn drop(&mut self) {
        self.remove();
    }
}

impl SemanticReviewStore {
    #[must_use]
    pub fn new(document: Arc<SemanticReviewDocument>, show_agent_notes: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StoreInner {
                snapshot: Arc::new(SemanticReviewState::new(document, show_agent_notes)),
                next_listener_id: 0,
                listeners: BTreeMap::new(),
            })),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> Arc<SemanticReviewState> {
        Arc::clone(
            &self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .snapshot,
        )
    }

    pub fn subscribe<F>(&self, listener: F) -> ReviewSubscription
    where
        F: Fn() + Send + Sync + 'static,
    {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let listener_id = inner.next_listener_id;
        inner.next_listener_id = inner.next_listener_id.wrapping_add(1);
        inner.listeners.insert(listener_id, Arc::new(listener));
        ReviewSubscription {
            inner: Arc::downgrade(&self.inner),
            listener_id: Some(listener_id),
        }
    }

    /// Apply one action and synchronously notify a snapshot of the subscriber set.
    #[must_use]
    pub fn dispatch(&self, action: SemanticReviewAction) -> Arc<SemanticReviewState> {
        let (snapshot, listeners) = {
            let mut inner = self
                .inner
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(mut next) = reduce_semantic_review_state(&inner.snapshot, action) else {
                return Arc::clone(&inner.snapshot);
            };
            next.state_revision = inner.snapshot.state_revision.wrapping_add(1);
            let snapshot = Arc::new(next);
            inner.snapshot = Arc::clone(&snapshot);
            let listeners = inner.listeners.values().cloned().collect::<Vec<_>>();
            (snapshot, listeners)
        };
        for listener in listeners {
            listener();
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::{
        ReviewNoteResolution, ReviewStoredNote,
        semantic_test_support::{document, note},
    };
    use workdeck_core::ReviewNoteSource;

    #[test]
    fn publishes_new_snapshots_and_returns_the_state_just_produced() {
        let store = SemanticReviewStore::new(document(&[("alpha", 1), ("beta", 1)]), false);
        let revisions = Arc::new(Mutex::new(Vec::new()));
        let listener_store = store.clone();
        let listener_revisions = Arc::clone(&revisions);
        let _subscription = store.subscribe(move || {
            listener_revisions
                .lock()
                .unwrap()
                .push(listener_store.snapshot().state_revision);
        });
        let first = store.dispatch(SemanticReviewAction::SetFilter("alpha".into()));
        let second = store.dispatch(SemanticReviewAction::SetNoteVisibility(true));
        assert_eq!(*revisions.lock().unwrap(), [1, 2]);
        assert_eq!(first.filter, "alpha");
        assert!(Arc::ptr_eq(&second, &store.snapshot()));
    }

    #[test]
    fn semantic_noop_preserves_snapshot_identity_and_skips_notification() {
        let store = SemanticReviewStore::new(document(&[("alpha", 1)]), false);
        let before = store.snapshot();
        let notified = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&notified);
        let _subscription = store.subscribe(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let result = store.dispatch(SemanticReviewAction::SetFilter(String::new()));
        assert!(Arc::ptr_eq(&before, &result));
        assert_eq!(notified.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn dispatch_returns_saved_note_in_current_snapshot() {
        let store = SemanticReviewStore::new(document(&[("alpha", 1)]), false);
        let next = store.dispatch(SemanticReviewAction::AddLiveNotes(vec![ReviewStoredNote {
            note: note("live-1", "alpha", ReviewNoteSource::Agent),
            resolution: ReviewNoteResolution::Active,
        }]));
        assert_eq!(next.live_notes.len(), 1);
        assert!(Arc::ptr_eq(&next, &store.snapshot()));
    }

    #[test]
    fn unsubscribe_stops_notifications() {
        let store = SemanticReviewStore::new(document(&[("alpha", 1)]), false);
        let notified = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&notified);
        let subscription = store.subscribe(move || {
            count.fetch_add(1, Ordering::SeqCst);
        });
        let _ = store.dispatch(SemanticReviewAction::SetFilter("a".into()));
        subscription.unsubscribe();
        let _ = store.dispatch(SemanticReviewAction::SetFilter("ab".into()));
        assert_eq!(notified.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn listener_may_unsubscribe_while_the_copied_set_is_notified() {
        let store = SemanticReviewStore::new(document(&[("alpha", 1)]), false);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let first_slot = Arc::new(Mutex::new(None::<ReviewSubscription>));
        let first_seen = Arc::clone(&seen);
        let first_slot_callback = Arc::clone(&first_slot);
        let first = store.subscribe(move || {
            first_seen.lock().unwrap().push("first");
            first_slot_callback.lock().unwrap().take();
        });
        *first_slot.lock().unwrap() = Some(first);
        let second_seen = Arc::clone(&seen);
        let _second = store.subscribe(move || second_seen.lock().unwrap().push("second"));
        let _ = store.dispatch(SemanticReviewAction::SetFilter("a".into()));
        assert_eq!(*seen.lock().unwrap(), ["first", "second"]);
    }
}
