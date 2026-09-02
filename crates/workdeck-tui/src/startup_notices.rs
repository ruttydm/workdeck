//! App-lifetime startup notice queue with deduplication and timed presentation.

use std::collections::{BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use workdeck_core::StartupNotice;

use crate::TimedNotice;

pub const DEFAULT_STARTUP_NOTICE_DELAY: Duration = Duration::from_millis(1_200);
pub const DEFAULT_STARTUP_NOTICE_DURATION: Duration = Duration::from_millis(7_000);
pub const DEFAULT_STARTUP_NOTICE_REPEAT: Duration = Duration::from_millis(21_600_000);

/// Queue local and asynchronously resolved startup notices for one shared footer surface.
#[derive(Debug, Clone)]
pub struct StartupNoticeQueue {
    enabled: bool,
    duration: Duration,
    shown_keys: BTreeSet<String>,
    pending_keys: BTreeSet<String>,
    pending: VecDeque<StartupNotice>,
    active: TimedNotice,
}

impl StartupNoticeQueue {
    #[must_use]
    pub fn new(enabled: bool, duration: Duration) -> Self {
        Self {
            enabled,
            duration,
            shown_keys: BTreeSet::new(),
            pending_keys: BTreeSet::new(),
            pending: VecDeque::new(),
            active: TimedNotice::default(),
        }
    }

    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.active.text()
    }

    #[must_use]
    pub fn has_shown(&self, key: &str) -> bool {
        self.shown_keys.contains(key)
    }

    /// Rebuild local pending state while retaining app-lifetime deduplication.
    pub fn restart(
        &mut self,
        enabled: bool,
        duration: Duration,
        notices: impl IntoIterator<Item = StartupNotice>,
        now: Instant,
    ) {
        self.enabled = enabled;
        self.duration = duration;
        self.pending.clear();
        self.pending_keys.clear();
        self.active.clear();
        if !enabled {
            return;
        }
        for notice in notices {
            self.enqueue_pending(notice);
        }
        self.show_next(now);
    }

    /// Enqueue one resolved notice unless it was already shown or is already pending.
    pub fn enqueue(&mut self, notice: Option<StartupNotice>, now: Instant) -> bool {
        if !self.enabled {
            return false;
        }
        let Some(notice) = notice else {
            return false;
        };
        if !self.enqueue_pending(notice) {
            return false;
        }
        self.show_next(now);
        true
    }

    /// Advance dismissal and immediately present the next queued notice.
    pub fn tick(&mut self, now: Instant) -> bool {
        if !self.active.tick(now) {
            return false;
        }
        self.show_next(now);
        true
    }

    fn enqueue_pending(&mut self, notice: StartupNotice) -> bool {
        if self.shown_keys.contains(&notice.key) || self.pending_keys.contains(&notice.key) {
            return false;
        }
        self.pending_keys.insert(notice.key.clone());
        self.pending.push_back(notice);
        true
    }

    fn show_next(&mut self, now: Instant) {
        if self.active.text().is_some() {
            return;
        }
        let Some(notice) = self.pending.pop_front() else {
            return;
        };
        self.pending_keys.remove(&notice.key);
        self.shown_keys.insert(notice.key);
        self.active.show_at(notice.message, now, self.duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(key: &str, message: &str) -> StartupNotice {
        StartupNotice::new(key, message)
    }

    #[test]
    fn async_notice_follows_an_immediate_local_notice() {
        let start = Instant::now();
        let mut queue = StartupNoticeQueue::new(true, Duration::from_millis(50));
        queue.restart(
            true,
            Duration::from_millis(50),
            [notice("legacy", "Legacy config detected")],
            start,
        );
        queue.enqueue(
            Some(notice("latest:9.9.9", "Update available: 9.9.9")),
            start + Duration::from_millis(5),
        );
        assert_eq!(queue.text(), Some("Legacy config detected"));
        queue.tick(start + Duration::from_millis(50));
        assert_eq!(queue.text(), Some("Update available: 9.9.9"));
    }

    #[test]
    fn restart_requeues_pending_content_without_replaying_shown_keys() {
        let start = Instant::now();
        let mut queue = StartupNoticeQueue::new(true, Duration::from_millis(30));
        queue.restart(
            true,
            Duration::from_millis(30),
            [notice("legacy", "Legacy config detected")],
            start,
        );
        queue.restart(
            true,
            Duration::from_millis(30),
            [notice("legacy", "Legacy config detected")],
            start + Duration::from_millis(5),
        );
        assert_eq!(queue.text(), None);
        queue.enqueue(
            Some(notice("latest:2.0.0", "Update available: 2.0.0")),
            start + Duration::from_millis(10),
        );
        assert_eq!(queue.text(), Some("Update available: 2.0.0"));
    }

    #[test]
    fn repeated_checks_show_one_notice_key_once_per_app_lifetime() {
        let start = Instant::now();
        let mut queue = StartupNoticeQueue::new(true, Duration::from_millis(5));
        assert!(queue.enqueue(
            Some(notice("latest:9.9.9", "Update available: 9.9.9")),
            start,
        ));
        queue.tick(start + Duration::from_millis(5));
        assert!(!queue.enqueue(
            Some(notice("latest:9.9.9", "Update available: 9.9.9")),
            start + Duration::from_millis(10),
        ));
        assert_eq!(queue.text(), None);
        assert!(queue.has_shown("latest:9.9.9"));
    }

    #[test]
    fn disabled_or_restarted_resolver_results_do_not_reach_the_surface() {
        let start = Instant::now();
        let mut queue = StartupNoticeQueue::new(true, Duration::from_secs(1));
        queue.restart(false, Duration::from_secs(1), [], start);
        assert!(!queue.enqueue(Some(notice("latest:1.0.0", "stale resolver")), start,));
        queue.restart(true, Duration::from_secs(1), [], start);
        assert!(queue.enqueue(Some(notice("latest:2.0.0", "current resolver")), start,));
        assert_eq!(queue.text(), Some("current resolver"));
    }
}
