//! Bounded parent-specific inboxes for multiplexed native request dispatch.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

/// Queue bound per active parent; a stalled consumer cannot retain unlimited frames.
pub const MAX_ROUTED_RESPONSE_FRAMES: usize = 32;
pub const MAX_ROUTED_PARENTS: usize = 4;

#[derive(Debug, Default)]
pub struct ExtensionResponseRoutes {
    routes: BTreeMap<u64, SyncSender<String>>,
    closed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseRouteError {
    DuplicateParent,
    ParentLimit,
    QueueFull,
    ReceiverClosed,
    Closed,
}

impl ExtensionResponseRoutes {
    /// A render/event-loop probe must not wait behind stdout frame dispatch.
    pub(crate) fn active_or_contended(routes: &std::sync::Mutex<Self>) -> bool {
        match routes.try_lock() {
            Ok(routes) => routes.has_active_parents(),
            Err(std::sync::TryLockError::WouldBlock) => true,
            Err(std::sync::TryLockError::Poisoned(error)) => {
                error.into_inner().has_active_parents()
            }
        }
    }
    pub fn has_active_parents(&self) -> bool {
        !self.routes.is_empty()
    }
    /// Classify without granting authority: callbacks are validated by their
    /// owning parent after routing. Notifications and malformed frames remain
    /// available to the legacy/error dispatcher, never an arbitrary inbox.
    pub fn dispatch_frame(&mut self, frame: String) -> Result<Option<String>, ResponseRouteError> {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&frame) else {
            return Ok(Some(frame));
        };
        if value.get("jsonrpc").and_then(serde_json::Value::as_str) != Some("2.0") {
            return Ok(Some(frame));
        }
        let parent = if value.get("method").and_then(serde_json::Value::as_str)
            == Some(workdeck_extension_api::EXTENSION_DOCUMENT_READ_METHOD)
        {
            // A child ID may numerically equal a different active parent ID.
            value
                .pointer("/params/parentRequestId")
                .and_then(serde_json::Value::as_u64)
        } else if value.get("method").is_none() {
            value.get("id").and_then(serde_json::Value::as_u64)
        } else {
            None
        };
        match parent {
            Some(parent) => self.dispatch(parent, frame),
            None => Ok(Some(frame)),
        }
    }

    pub fn register(&mut self, parent: u64) -> Result<Receiver<String>, ResponseRouteError> {
        if self.closed {
            return Err(ResponseRouteError::Closed);
        }
        if self.routes.contains_key(&parent) {
            return Err(ResponseRouteError::DuplicateParent);
        }
        if self.routes.len() >= MAX_ROUTED_PARENTS {
            return Err(ResponseRouteError::ParentLimit);
        }
        let (sender, receiver) = mpsc::sync_channel(MAX_ROUTED_RESPONSE_FRAMES);
        self.routes.insert(parent, sender);
        Ok(receiver)
    }

    /// Unknown parents remain with the caller for legacy dispatch or stale rejection.
    /// Overflow retires only the offending inbox, never another parent's request.
    pub fn dispatch(
        &mut self,
        parent: u64,
        frame: String,
    ) -> Result<Option<String>, ResponseRouteError> {
        let Some(sender) = self.routes.get(&parent) else {
            return Ok(Some(frame));
        };
        match sender.try_send(frame) {
            Ok(()) => Ok(None),
            Err(error) => {
                self.routes.remove(&parent);
                Err(match error {
                    TrySendError::Full(_) => ResponseRouteError::QueueFull,
                    TrySendError::Disconnected(_) => ResponseRouteError::ReceiverClosed,
                })
            }
        }
    }

    pub fn retire(&mut self, parent: u64) {
        self.routes.remove(&parent);
    }

    /// Disconnect every waiter on stdout EOF or terminal transport failure.
    /// Buffered frames can still be drained; registration cannot revive a dead child.
    pub fn close(&mut self) {
        self.closed = true;
        self.routes.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_probe_does_not_wait_for_dispatch_lock() {
        let routes = std::sync::Arc::new(std::sync::Mutex::new(ExtensionResponseRoutes::default()));
        assert!(!ExtensionResponseRoutes::active_or_contended(&routes));
        let guard = routes.lock().unwrap();
        let captured = routes.clone();
        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            sender
                .send(ExtensionResponseRoutes::active_or_contended(&captured))
                .unwrap();
        });
        let observed = receiver.recv_timeout(std::time::Duration::from_secs(5));
        drop(guard);
        worker.join().unwrap();
        assert!(observed.unwrap());
        assert!(!ExtensionResponseRoutes::active_or_contended(&routes));
        let _inbox = routes.lock().unwrap().register(1).unwrap();
        assert!(ExtensionResponseRoutes::active_or_contended(&routes));
    }

    #[test]
    fn closing_routes_disconnects_all_waiters_and_cannot_be_reopened() {
        let mut routes = ExtensionResponseRoutes::default();
        let first = routes.register(1).unwrap();
        let second = routes.register(2).unwrap();
        routes.dispatch(1, "buffered".into()).unwrap();
        routes.close();
        routes.close();
        assert_eq!(first.try_recv().unwrap(), "buffered");
        assert_eq!(
            first.try_recv().unwrap_err(),
            mpsc::TryRecvError::Disconnected
        );
        assert_eq!(
            second.try_recv().unwrap_err(),
            mpsc::TryRecvError::Disconnected
        );
        assert_eq!(routes.register(1).unwrap_err(), ResponseRouteError::Closed);
        assert_eq!(routes.register(3).unwrap_err(), ResponseRouteError::Closed);
    }

    #[test]
    fn callbacks_route_by_parent_even_when_child_id_matches_another_parent() {
        let mut routes = ExtensionResponseRoutes::default();
        let first = routes.register(1).unwrap();
        let second = routes.register(2).unwrap();
        let callback = serde_json::json!({"jsonrpc":"2.0","id":1,"method":"workdeck/document/read","params":{"parentRequestId":2,"side":"new"}}).to_string();
        assert_eq!(routes.dispatch_frame(callback.clone()), Ok(None));
        assert_eq!(second.try_recv().unwrap(), callback);
        assert!(first.try_recv().is_err());
        let response = serde_json::json!({"jsonrpc":"2.0","id":1,"result":[]}).to_string();
        assert_eq!(routes.dispatch_frame(response.clone()), Ok(None));
        assert_eq!(first.try_recv().unwrap(), response);
        for frame in [
            "not json",
            r#"{"jsonrpc":"1.0","id":1,"result":[]}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"workdeck/document/read","params":{"parentRequestId":"2"}}"#,
            r#"{"jsonrpc":"2.0","id":1,"method":"workdeck/notification"}"#,
        ] {
            assert_eq!(routes.dispatch_frame(frame.into()), Ok(Some(frame.into())));
        }
        assert!(first.try_recv().is_err());
        assert!(second.try_recv().is_err());
    }

    #[test]
    fn four_parent_inboxes_deliver_out_of_order_and_retire_independently() {
        let mut routes = ExtensionResponseRoutes::default();
        let receivers = (1..=4)
            .map(|id| routes.register(id).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            routes.register(1).unwrap_err(),
            ResponseRouteError::DuplicateParent
        );
        assert_eq!(
            routes.register(5).unwrap_err(),
            ResponseRouteError::ParentLimit
        );
        for id in [4, 2, 3, 1] {
            assert_eq!(routes.dispatch(id, id.to_string()), Ok(None));
        }
        for (index, receiver) in receivers.iter().enumerate() {
            assert_eq!(receiver.try_recv().unwrap(), (index + 1).to_string());
        }
        routes.retire(2);
        assert_eq!(routes.dispatch(2, "late".into()), Ok(Some("late".into())));
        assert_eq!(routes.dispatch(3, "alive".into()), Ok(None));
        assert_eq!(receivers[2].try_recv().unwrap(), "alive");
        assert!(routes.register(5).is_ok());
    }

    #[test]
    fn stalled_parent_queue_is_bounded_and_does_not_block_other_parents() {
        let mut routes = ExtensionResponseRoutes::default();
        let held = routes.register(1).unwrap();
        let active = routes.register(2).unwrap();
        for _ in 0..MAX_ROUTED_RESPONSE_FRAMES {
            routes.dispatch(1, "held".into()).unwrap();
        }
        assert_eq!(
            routes.dispatch(1, "overflow".into()),
            Err(ResponseRouteError::QueueFull)
        );
        routes.dispatch(2, "active".into()).unwrap();
        assert_eq!(active.try_recv().unwrap(), "active");
        assert_eq!(held.try_iter().count(), MAX_ROUTED_RESPONSE_FRAMES);
        drop(active);
        assert_eq!(
            routes.dispatch(2, "closed".into()),
            Err(ResponseRouteError::ReceiverClosed)
        );
    }
}
