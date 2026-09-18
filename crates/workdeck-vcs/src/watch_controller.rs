//! Serialized debounce, refresh, and degraded-polling state machine.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use crate::WatchSourceError;

pub const DEFAULT_WATCH_EVENT_SOURCE_STARTUP_TIMEOUT: Duration = Duration::from_secs(2);
pub const WATCH_EVENT_SOURCE_STARTUP_TIMEOUT_CODE: &str =
    "WORKDECK_WATCH_EVENT_SOURCE_STARTUP_TIMEOUT";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchControllerPhase {
    Starting,
    Idle,
    Debouncing,
    Checking,
    Refreshing,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchControllerState {
    pub phase: WatchControllerPhase,
    pub dirty: bool,
    pub degraded: bool,
    pub applied_signature: String,
}

#[derive(Debug, Clone)]
pub struct WatchControllerConfig {
    pub quiet_delay: Duration,
    pub maximum_delay: Duration,
    pub healthy_check: Duration,
    pub degraded_check: Duration,
    pub duplicate_error_interval: Duration,
    pub startup_timeout: Duration,
}

impl Default for WatchControllerConfig {
    fn default() -> Self {
        Self {
            quiet_delay: Duration::from_millis(200),
            maximum_delay: Duration::from_secs(1),
            healthy_check: Duration::from_secs(10),
            degraded_check: Duration::from_secs(2),
            duplicate_error_interval: Duration::from_secs(10),
            startup_timeout: DEFAULT_WATCH_EVENT_SOURCE_STARTUP_TIMEOUT,
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WatchControllerActions {
    pub check_signature: bool,
    pub refresh: bool,
    pub close_event_source: bool,
    pub reload_pending: bool,
    pub errors: Vec<WatchSourceError>,
}

impl WatchControllerActions {
    fn merge(&mut self, other: Self) {
        self.check_signature |= other.check_signature;
        self.refresh |= other.refresh;
        self.close_event_source |= other.close_event_source;
        self.reload_pending |= other.reload_pending;
        self.errors.extend(other.errors);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SourceStatus {
    None,
    Starting,
    Ready,
    Closed,
}

pub struct WatchController {
    state: WatchControllerState,
    config: WatchControllerConfig,
    source_status: SourceStatus,
    startup_deadline: Option<Instant>,
    quiet_deadline: Option<Instant>,
    maximum_deadline: Option<Instant>,
    safety_deadline: Option<Instant>,
    pending_signature: Option<String>,
    reported_at: BTreeMap<String, Instant>,
}

impl WatchController {
    #[must_use]
    pub fn new(
        initial_signature: impl Into<String>,
        poll_only: bool,
        has_event_source: bool,
        now: Instant,
        config: WatchControllerConfig,
    ) -> Self {
        let degraded = poll_only;
        let source_status = if has_event_source && !poll_only {
            SourceStatus::Starting
        } else {
            SourceStatus::None
        };
        Self {
            state: WatchControllerState {
                phase: WatchControllerPhase::Idle,
                dirty: false,
                degraded,
                applied_signature: initial_signature.into(),
            },
            startup_deadline: (source_status == SourceStatus::Starting)
                .then_some(now + config.startup_timeout),
            safety_deadline: Some(
                now + if degraded {
                    config.degraded_check
                } else {
                    config.healthy_check
                },
            ),
            config,
            source_status,
            quiet_deadline: None,
            maximum_deadline: None,
            pending_signature: None,
            reported_at: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn state(&self) -> WatchControllerState {
        self.state.clone()
    }

    #[must_use]
    pub fn next_deadline(&self) -> Option<Instant> {
        if matches!(
            self.state.phase,
            WatchControllerPhase::Closed
                | WatchControllerPhase::Checking
                | WatchControllerPhase::Refreshing
        ) {
            return None;
        }
        [
            self.startup_deadline,
            self.quiet_deadline,
            self.maximum_deadline,
            self.safety_deadline,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub fn on_event(&mut self, now: Instant) -> WatchControllerActions {
        if self.state.phase == WatchControllerPhase::Closed
            || !matches!(
                self.source_status,
                SourceStatus::Starting | SourceStatus::Ready
            )
        {
            return WatchControllerActions::default();
        }
        if matches!(
            self.state.phase,
            WatchControllerPhase::Checking | WatchControllerPhase::Refreshing
        ) {
            self.state.dirty = true;
            return WatchControllerActions::default();
        }
        let reload_pending = self.state.phase != WatchControllerPhase::Debouncing;
        self.quiet_deadline = Some(now + self.config.quiet_delay);
        self.maximum_deadline
            .get_or_insert(now + self.config.maximum_delay);
        self.state.phase = WatchControllerPhase::Debouncing;
        WatchControllerActions {
            reload_pending,
            ..WatchControllerActions::default()
        }
    }

    pub fn on_source_ready(&mut self, now: Instant) -> WatchControllerActions {
        if self.state.phase == WatchControllerPhase::Closed
            || self.source_status != SourceStatus::Starting
        {
            return WatchControllerActions::default();
        }
        self.source_status = SourceStatus::Ready;
        self.startup_deadline = None;
        if matches!(
            self.state.phase,
            WatchControllerPhase::Checking | WatchControllerPhase::Refreshing
        ) {
            self.state.dirty = true;
            WatchControllerActions::default()
        } else {
            self.begin_check(now)
        }
    }

    pub fn on_source_error(
        &mut self,
        now: Instant,
        error: WatchSourceError,
    ) -> WatchControllerActions {
        if self.state.phase == WatchControllerPhase::Closed
            || self.source_status == SourceStatus::Closed
        {
            return WatchControllerActions::default();
        }
        let mut actions = WatchControllerActions::default();
        if matches!(error.code.as_deref(), Some("ENOSPC" | "EMFILE")) {
            self.state.degraded = true;
            self.source_status = SourceStatus::Closed;
            self.startup_deadline = None;
            actions.close_event_source = true;
            let degraded_deadline = now + self.config.degraded_check;
            self.safety_deadline =
                Some(self.safety_deadline.map_or(degraded_deadline, |deadline| {
                    deadline.min(degraded_deadline)
                }));
        }
        self.report_error(now, error, &mut actions);
        actions
    }

    pub fn on_source_start_failed(
        &mut self,
        now: Instant,
        error: WatchSourceError,
    ) -> WatchControllerActions {
        if self.state.phase == WatchControllerPhase::Closed {
            return WatchControllerActions::default();
        }
        self.source_status = SourceStatus::Closed;
        self.startup_deadline = None;
        self.state.degraded = true;
        self.safety_deadline = Some(now + self.config.degraded_check);
        let mut actions = WatchControllerActions::default();
        self.report_error(now, error, &mut actions);
        actions
    }

    pub fn tick(&mut self, now: Instant) -> WatchControllerActions {
        if matches!(
            self.state.phase,
            WatchControllerPhase::Closed
                | WatchControllerPhase::Checking
                | WatchControllerPhase::Refreshing
        ) {
            return WatchControllerActions::default();
        }
        if self
            .startup_deadline
            .is_some_and(|deadline| deadline <= now)
        {
            return self.degrade_stalled_source(now);
        }
        let event_due = self.quiet_deadline.is_some_and(|deadline| deadline <= now)
            || self
                .maximum_deadline
                .is_some_and(|deadline| deadline <= now);
        let safety_due = self.safety_deadline.is_some_and(|deadline| deadline <= now);
        if event_due || safety_due {
            self.begin_check(now)
        } else {
            WatchControllerActions::default()
        }
    }

    pub fn finish_signature(
        &mut self,
        now: Instant,
        result: Result<String, WatchSourceError>,
    ) -> WatchControllerActions {
        if self.state.phase != WatchControllerPhase::Checking {
            return WatchControllerActions::default();
        }
        match result {
            Ok(signature) if signature != self.state.applied_signature => {
                self.pending_signature = Some(signature);
                self.state.phase = WatchControllerPhase::Refreshing;
                WatchControllerActions {
                    refresh: true,
                    ..WatchControllerActions::default()
                }
            }
            Ok(_) => self.finish_check(now),
            Err(error) => {
                let mut actions = WatchControllerActions::default();
                self.report_error(now, error, &mut actions);
                actions.merge(self.finish_check(now));
                actions
            }
        }
    }

    pub fn finish_refresh(
        &mut self,
        now: Instant,
        result: Result<(), WatchSourceError>,
    ) -> WatchControllerActions {
        if self.state.phase != WatchControllerPhase::Refreshing {
            return WatchControllerActions::default();
        }
        let mut actions = WatchControllerActions::default();
        match result {
            Ok(()) => {
                if let Some(signature) = self.pending_signature.take() {
                    self.state.applied_signature = signature;
                }
            }
            Err(error) => {
                self.pending_signature = None;
                self.report_error(now, error, &mut actions);
            }
        }
        actions.merge(self.finish_check(now));
        actions
    }

    pub fn close(&mut self) -> WatchControllerActions {
        if self.state.phase == WatchControllerPhase::Closed {
            return WatchControllerActions::default();
        }
        self.state.phase = WatchControllerPhase::Closed;
        self.state.dirty = false;
        self.startup_deadline = None;
        self.quiet_deadline = None;
        self.maximum_deadline = None;
        self.safety_deadline = None;
        self.pending_signature = None;
        let close_event_source = matches!(
            self.source_status,
            SourceStatus::Starting | SourceStatus::Ready
        );
        self.source_status = SourceStatus::Closed;
        WatchControllerActions {
            close_event_source,
            ..WatchControllerActions::default()
        }
    }

    fn begin_check(&mut self, _now: Instant) -> WatchControllerActions {
        if matches!(
            self.state.phase,
            WatchControllerPhase::Closed
                | WatchControllerPhase::Checking
                | WatchControllerPhase::Refreshing
        ) {
            return WatchControllerActions::default();
        }
        self.quiet_deadline = None;
        self.maximum_deadline = None;
        self.safety_deadline = None;
        self.state.phase = WatchControllerPhase::Checking;
        WatchControllerActions {
            check_signature: true,
            ..WatchControllerActions::default()
        }
    }

    fn finish_check(&mut self, now: Instant) -> WatchControllerActions {
        if self.state.phase == WatchControllerPhase::Closed {
            return WatchControllerActions::default();
        }
        self.safety_deadline = Some(now + self.safety_interval());
        self.state.phase = WatchControllerPhase::Idle;
        if self.state.dirty {
            self.state.dirty = false;
            self.begin_check(now)
        } else {
            WatchControllerActions::default()
        }
    }

    fn degrade_stalled_source(&mut self, now: Instant) -> WatchControllerActions {
        if self.source_status != SourceStatus::Starting {
            return WatchControllerActions::default();
        }
        self.source_status = SourceStatus::Closed;
        self.startup_deadline = None;
        self.state.degraded = true;
        self.state.phase = WatchControllerPhase::Idle;
        self.quiet_deadline = None;
        self.maximum_deadline = None;
        self.safety_deadline = Some(now + self.config.degraded_check);
        let mut actions = WatchControllerActions {
            close_event_source: true,
            ..WatchControllerActions::default()
        };
        self.report_error(
            now,
            WatchSourceError::new(
                Some(WATCH_EVENT_SOURCE_STARTUP_TIMEOUT_CODE),
                format!(
                    "The watch event source did not become ready within {} ms.",
                    self.config.startup_timeout.as_millis()
                ),
            ),
            &mut actions,
        );
        actions
    }

    fn safety_interval(&self) -> Duration {
        if self.state.degraded {
            self.config.degraded_check
        } else {
            self.config.healthy_check
        }
    }

    fn report_error(
        &mut self,
        now: Instant,
        error: WatchSourceError,
        actions: &mut WatchControllerActions,
    ) {
        let key = error.code.as_ref().map_or_else(
            || format!("error:{}", error.message),
            |code| format!("code:{code}"),
        );
        if self.reported_at.get(&key).is_some_and(|previous| {
            now.saturating_duration_since(*previous) < self.config.duplicate_error_interval
        }) {
            return;
        }
        self.reported_at.insert(key, now);
        actions.errors.push(error);
    }
}

#[cfg(test)]
mod tests;
