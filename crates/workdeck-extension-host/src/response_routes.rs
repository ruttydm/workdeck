//! Bounded parent-specific inboxes for multiplexed native request dispatch.

use std::collections::BTreeMap;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};

/// Queue bound per active parent; a stalled consumer cannot retain unlimited frames.
pub const MAX_ROUTED_RESPONSE_FRAMES: usize = 32;
pub const MAX_ROUTED_PARENTS: usize = 4;

#[derive(Debug, Default)]
pub struct ExtensionResponseRoutes {
    routes: BTreeMap<u64, SyncSender<String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseRouteError {
    DuplicateParent,
    ParentLimit,
    QueueFull,
    ReceiverClosed,
}

impl ExtensionResponseRoutes {
    pub fn register(&mut self, parent: u64) -> Result<Receiver<String>, ResponseRouteError> {
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
