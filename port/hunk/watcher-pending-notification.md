# Watcher pending-notification diagnosis

The intermittent atomic-save test failed only at `outcome.reload_pending`, after
successfully asserting one refresh attempt and one refresh. Inspection shows that
`reload_pending` is emitted on entry into event debouncing, not on every refresh.
The test's helper accumulates polls after the mutation but discards its initial
poll's outcome. Native readiness timing is also not guaranteed by its 50 ms sleep.

The deterministic test
`watch_controller::tests::readiness_check_can_refresh_without_an_event_pending_notification`
proves that readiness can request a signature check, then a changed signature can
request refresh, with neither action emitting `reload_pending`. It passed without
sleep or filesystem timing. Thus a successful refresh alone does not imply that
the accumulated result must contain a pending notification.

This identifies a valid controller path inconsistent with the native test's
unconditional assertion. It does not establish which event ordering occurred in
the previously failing native run. No watcher runtime, timeout, native assertion
or source ledger mapping has been changed. A deterministic driver-level event
schedule or captured native event trace is still needed before claiming the
intermittent failure fully diagnosed and repaired.

All 242 VCS library tests passed (7.85 seconds), including the deterministic
readiness regression. Formatting and diff checks passed.
