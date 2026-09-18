//! Generation-safe timed notice state for the Ratatui event loop.

use std::time::{Duration, Instant};

/// One presentation-independent notice channel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TimedNotice {
    text: Option<String>,
    generation: u64,
    expires_at: Option<(Instant, u64)>,
}

impl TimedNotice {
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        self.text.as_deref()
    }

    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Show or replace a notice and start one complete visibility window.
    pub fn show_at(&mut self, text: impl Into<String>, now: Instant, duration: Duration) -> u64 {
        self.generation = self.generation.wrapping_add(1);
        self.text = Some(text.into());
        self.expires_at = Some((now + duration, self.generation));
        self.generation
    }

    /// Clear the current notice and invalidate any previously scheduled expiry.
    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.text = None;
        self.expires_at = None;
    }

    /// Deliver one scheduled expiry, ignoring a callback from an older generation.
    pub fn expire_generation(&mut self, generation: u64) -> bool {
        if self.text.is_none() || generation != self.generation {
            return false;
        }
        self.text = None;
        self.expires_at = None;
        true
    }

    /// Expire the current notice once its monotonic deadline has elapsed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let Some((deadline, generation)) = self.expires_at else {
            return false;
        };
        if now < deadline {
            return false;
        }
        self.expire_generation(generation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shows_for_the_configured_duration() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        notice.show_at("Saved", start, Duration::from_millis(75));
        assert_eq!(notice.text(), Some("Saved"));
        assert!(!notice.tick(start + Duration::from_millis(74)));
        assert!(notice.tick(start + Duration::from_millis(75)));
        assert_eq!(notice.text(), None);
    }

    #[test]
    fn same_text_restarts_the_full_window_and_invalidates_the_old_expiry() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        let old = notice.show_at("Copied", start, Duration::from_millis(100));
        let current = notice.show_at(
            "Copied",
            start + Duration::from_millis(40),
            Duration::from_millis(100),
        );
        assert_ne!(old, current);
        assert!(!notice.expire_generation(old));
        assert!(!notice.tick(start + Duration::from_millis(139)));
        assert!(notice.tick(start + Duration::from_millis(140)));
    }

    #[test]
    fn different_text_cannot_be_cleared_by_the_replaced_expiry() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        let old = notice.show_at("First", start, Duration::from_secs(1));
        notice.show_at("Second", start, Duration::from_secs(1));
        assert!(!notice.expire_generation(old));
        assert_eq!(notice.text(), Some("Second"));
    }

    #[test]
    fn explicit_clear_invalidates_stale_delivery() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        let generation = notice.show_at("Working", start, Duration::from_secs(1));
        notice.clear();
        assert_eq!(notice.text(), None);
        assert!(!notice.expire_generation(generation));
    }

    #[test]
    fn a_cleared_generation_cannot_retire_a_newer_notice() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        let old = notice.show_at("Old", start, Duration::from_secs(1));
        notice.clear();
        notice.show_at("New", start, Duration::from_secs(1));
        assert!(!notice.expire_generation(old));
        assert_eq!(notice.text(), Some("New"));
    }

    #[test]
    fn independently_configured_channels_remain_isolated() {
        let start = Instant::now();
        let mut transient = TimedNotice::default();
        let mut session = TimedNotice::default();
        transient.show_at("Copied", start, Duration::from_secs(3));
        session.show_at("Config failed", start, Duration::from_secs(4));
        transient.tick(start + Duration::from_secs(3));
        session.tick(start + Duration::from_secs(3));
        assert_eq!(transient.text(), None);
        assert_eq!(session.text(), Some("Config failed"));
        session.tick(start + Duration::from_secs(4));
        assert_eq!(session.text(), None);
    }

    #[test]
    fn clearing_an_unmounted_channel_is_idempotent() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        notice.show_at("Mounted", start, Duration::from_secs(1));
        notice.clear();
        notice.clear();
        assert_eq!(notice.text(), None);
    }

    #[test]
    fn replayed_lifecycle_uses_only_the_latest_generation() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        let first = notice.show_at("Strict notice", start, Duration::from_secs(1));
        notice.clear();
        let second = notice.show_at("Strict notice", start, Duration::from_secs(1));
        assert!(!notice.expire_generation(first));
        assert!(notice.expire_generation(second));
    }

    #[test]
    fn a_reconfigured_duration_applies_to_the_next_show() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        notice.show_at("From A", start, Duration::from_millis(10));
        notice.show_at("From B", start, Duration::from_millis(25));
        assert!(!notice.tick(start + Duration::from_millis(24)));
        assert!(notice.tick(start + Duration::from_millis(25)));
    }

    #[test]
    fn controller_identity_stays_stable_while_options_change() {
        let start = Instant::now();
        let mut notice = TimedNotice::default();
        let address = std::ptr::addr_of!(notice);
        notice.show_at("First", start, Duration::from_millis(10));
        notice.show_at("Second", start, Duration::from_millis(25));
        assert_eq!(address, std::ptr::addr_of!(notice));
        assert_eq!(notice.text(), Some("Second"));
    }
}
