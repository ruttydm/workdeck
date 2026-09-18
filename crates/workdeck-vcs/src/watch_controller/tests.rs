use super::*;

fn config() -> WatchControllerConfig {
    WatchControllerConfig {
        quiet_delay: Duration::from_millis(20),
        maximum_delay: Duration::from_millis(100),
        healthy_check: Duration::from_millis(500),
        degraded_check: Duration::from_millis(50),
        duplicate_error_interval: Duration::from_millis(100),
        startup_timeout: Duration::from_millis(1_000),
    }
}

fn controller(now: Instant) -> WatchController {
    WatchController::new("same", false, true, now, config())
}

fn short_startup_controller(now: Instant) -> WatchController {
    let mut config = config();
    config.startup_timeout = Duration::from_millis(25);
    WatchController::new("same", false, true, now, config)
}

fn error(code: Option<&str>, message: &str) -> WatchSourceError {
    WatchSourceError::new(code.map(str::to_owned), message)
}

#[test]
fn readiness_check_can_refresh_without_an_event_pending_notification() {
    let now = Instant::now();
    let mut controller = controller(now);
    let ready = controller.on_source_ready(now);
    assert!(ready.check_signature);
    assert!(!ready.reload_pending);
    let changed = controller.finish_signature(now, Ok("changed".into()));
    assert!(changed.refresh);
    assert!(!changed.reload_pending);
}

#[test]
fn debounces_a_hint_without_a_recurring_timer() {
    let now = Instant::now();
    let mut controller = controller(now);
    assert!(controller.on_event(now).reload_pending);
    assert_eq!(controller.next_deadline(), Some(now + config().quiet_delay));
    assert!(
        !controller
            .tick(now + Duration::from_millis(19))
            .check_signature
    );
    assert!(
        controller
            .tick(now + Duration::from_millis(20))
            .check_signature
    );
}

#[test]
fn reports_one_pending_reload_for_a_burst() {
    let now = Instant::now();
    let mut controller = controller(now);
    assert!(controller.on_event(now).reload_pending);
    assert!(
        !controller
            .on_event(now + Duration::from_millis(1))
            .reload_pending
    );
    assert!(
        !controller
            .on_event(now + Duration::from_millis(2))
            .reload_pending
    );
}

#[test]
fn signatures_distinguish_changed_and_unchanged_hints() {
    let now = Instant::now();
    let mut controller = controller(now);
    controller.on_event(now);
    controller.tick(now + Duration::from_millis(20));
    assert!(
        !controller
            .finish_signature(now + Duration::from_millis(20), Ok("same".into()))
            .refresh
    );
    controller.on_event(now + Duration::from_millis(30));
    controller.tick(now + Duration::from_millis(50));
    assert!(
        controller
            .finish_signature(now + Duration::from_millis(50), Ok("changed".into()))
            .refresh
    );
}

#[test]
fn event_burst_coalesces_behind_the_quiet_deadline() {
    let now = Instant::now();
    let mut controller = controller(now);
    controller.on_event(now);
    controller.on_event(now + Duration::from_millis(10));
    assert!(
        !controller
            .tick(now + Duration::from_millis(20))
            .check_signature
    );
    assert!(
        controller
            .tick(now + Duration::from_millis(30))
            .check_signature
    );
}

#[test]
fn continuous_noise_progresses_at_the_maximum_delay() {
    let now = Instant::now();
    let mut controller = controller(now);
    for offset in (0..100).step_by(10) {
        controller.on_event(now + Duration::from_millis(offset));
    }
    assert!(
        controller
            .tick(now + Duration::from_millis(100))
            .check_signature
    );
}

#[test]
fn serializes_refreshes_and_performs_one_changed_trailing_check() {
    let now = Instant::now();
    let mut controller = controller(now);
    controller.on_source_ready(now);
    assert!(controller.finish_signature(now, Ok("first".into())).refresh);
    controller.on_event(now);
    assert!(controller.finish_refresh(now, Ok(())).check_signature);
    assert!(
        controller
            .finish_signature(now, Ok("second".into()))
            .refresh
    );
}

#[test]
fn multiple_refresh_time_events_become_one_unchanged_trailing_check() {
    let now = Instant::now();
    let mut controller = controller(now);
    controller.on_source_ready(now);
    controller.finish_signature(now, Ok("first".into()));
    controller.on_event(now);
    controller.on_event(now);
    controller.on_event(now);
    assert!(controller.finish_refresh(now, Ok(())).check_signature);
    assert!(!controller.finish_signature(now, Ok("first".into())).refresh);
    assert_eq!(controller.state().phase, WatchControllerPhase::Idle);
}

#[test]
fn refresh_rejection_retains_the_old_baseline_and_retries() {
    let now = Instant::now();
    let mut controller = controller(now);
    controller.on_source_ready(now);
    controller.finish_signature(now, Ok("changed".into()));
    controller.finish_refresh(now, Err(error(None, "refresh failed")));
    assert_eq!(controller.state().applied_signature, "same");
    controller.on_event(now + Duration::from_millis(1));
    controller.tick(now + Duration::from_millis(21));
    assert!(
        controller
            .finish_signature(now, Ok("changed".into()))
            .refresh
    );
}

#[test]
fn signature_exception_remains_retryable() {
    let now = Instant::now();
    let mut controller = controller(now);
    controller.on_source_ready(now);
    let actions = controller.finish_signature(now, Err(error(None, "signature failed")));
    assert_eq!(actions.errors.len(), 1);
    controller.on_event(now + Duration::from_millis(1));
    controller.tick(now + Duration::from_millis(21));
    assert!(
        controller
            .finish_signature(now, Ok("changed".into()))
            .refresh
    );
}

#[test]
fn source_readiness_checks_the_bootstrap_signature_immediately() {
    let now = Instant::now();
    let mut controller = controller(now);
    assert!(controller.on_source_ready(now).check_signature);
}

#[test]
fn stalled_source_degrades_at_the_default_startup_deadline() {
    let now = Instant::now();
    let mut default =
        WatchController::new("same", false, true, now, WatchControllerConfig::default());
    let actions = default.tick(now + DEFAULT_WATCH_EVENT_SOURCE_STARTUP_TIMEOUT);
    assert!(default.state().degraded && actions.close_event_source);
    assert_eq!(
        actions.errors[0].code.as_deref(),
        Some(WATCH_EVENT_SOURCE_STARTUP_TIMEOUT_CODE)
    );
}

#[test]
fn readiness_just_before_the_startup_deadline_is_accepted() {
    let now = Instant::now();
    let mut controller = short_startup_controller(now);
    assert!(
        controller
            .on_source_ready(now + Duration::from_millis(24))
            .check_signature
    );
    assert!(
        !controller
            .tick(now + Duration::from_millis(25))
            .close_event_source
    );
    assert!(!controller.state().degraded);
}

#[test]
fn startup_degradation_ignores_late_readiness_events_and_errors() {
    let now = Instant::now();
    let mut controller = short_startup_controller(now);
    controller.tick(now + Duration::from_millis(25));
    assert!(
        !controller
            .on_source_ready(now + Duration::from_millis(26))
            .check_signature
    );
    assert!(
        !controller
            .on_event(now + Duration::from_millis(26))
            .reload_pending
    );
    assert!(
        controller
            .on_source_error(now + Duration::from_millis(26), error(Some("EIO"), "late"))
            .errors
            .is_empty()
    );
}

#[test]
fn closing_during_startup_cancels_the_deadline_and_closes_once() {
    let now = Instant::now();
    let mut controller = controller(now);
    assert!(controller.close().close_event_source);
    assert!(!controller.close().close_event_source);
    assert!(
        controller
            .tick(now + Duration::from_millis(25))
            .errors
            .is_empty()
    );
}

#[test]
fn healthy_safety_check_runs_without_an_event() {
    let now = Instant::now();
    let mut controller = controller(now);
    assert!(
        controller
            .tick(now + config().healthy_check)
            .check_signature
    );
}

#[test]
fn colliding_event_and_safety_deadlines_coalesce_into_one_check() {
    let now = Instant::now();
    let mut config = config();
    config.quiet_delay = config.healthy_check;
    let mut controller = WatchController::new("same", false, true, now, config.clone());
    controller.on_event(now);
    assert!(controller.tick(now + config.healthy_check).check_signature);
    assert!(!controller.tick(now + config.healthy_check).check_signature);
}

#[test]
fn watcher_startup_failure_falls_back_to_degraded_polling() {
    let now = Instant::now();
    let mut controller = controller(now);
    let actions = controller.on_source_start_failed(now, error(None, "construction failed"));
    assert!(controller.state().degraded);
    assert_eq!(actions.errors.len(), 1);
    assert!(
        controller
            .tick(now + config().degraded_check)
            .check_signature
    );
}

#[test]
fn resource_exhaustion_degrades_and_rate_limits_duplicate_codes() {
    let now = Instant::now();
    for code in ["ENOSPC", "EMFILE"] {
        let mut controller = controller(now);
        let first = controller.on_source_error(now, error(Some(code), "exhausted"));
        assert!(first.close_event_source && controller.state().degraded);
        assert_eq!(first.errors.len(), 1);
        assert!(
            controller
                .on_source_error(now, error(Some(code), "again"))
                .errors
                .is_empty()
        );
    }
}

#[test]
fn unknown_source_errors_are_reported_without_degrading() {
    let now = Instant::now();
    let mut controller = controller(now);
    let actions = controller.on_source_error(now, error(Some("EIO"), "unknown"));
    assert_eq!(actions.errors.len(), 1);
    assert!(!controller.state().degraded && !actions.close_event_source);
}

#[test]
fn poll_only_plan_uses_the_degraded_interval() {
    let now = Instant::now();
    let mut controller = WatchController::new("same", true, false, now, config());
    assert!(controller.state().degraded);
    assert!(
        !controller
            .tick(now + Duration::from_millis(49))
            .check_signature
    );
    assert!(
        controller
            .tick(now + Duration::from_millis(50))
            .check_signature
    );
}

#[test]
fn close_is_idempotent_and_ignores_late_completion() {
    let now = Instant::now();
    let mut controller = controller(now);
    controller.on_source_ready(now);
    assert!(controller.close().close_event_source);
    assert_eq!(controller.state().phase, WatchControllerPhase::Closed);
    assert!(!controller.finish_signature(now, Ok("late".into())).refresh);
    assert_eq!(controller.state().applied_signature, "same");
}

#[test]
fn replacing_a_controller_isolates_the_old_input_from_the_new_one() {
    let now = Instant::now();
    let mut old = WatchController::new("old", false, true, now, config());
    let mut new = WatchController::new("new", false, true, now, config());
    old.close();
    assert!(!old.on_event(now).reload_pending);
    assert!(new.on_event(now).reload_pending);
    new.tick(now + config().quiet_delay);
    assert!(new.finish_signature(now, Ok("newer".into())).refresh);
}
