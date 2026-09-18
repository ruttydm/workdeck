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

## Driver-level follow-up

`watched_input::tests::delayed_readiness_refreshes_changed_input_without_pending_event`
now reproduces the complete schedule deterministically: initial poll at 50 ms,
signature mutation, ready callback, then poll at 60 ms. Exactly one refresh
applies the new signature without a pending notification. No sleeps or native
event timing are involved.

The two native refresh tests no longer require a pending notification from every
successful refresh. They still require the actual updated patch content, exactly
one refresh attempt, exactly one success, no errors and bounded completion.
Event-notification assertions remain in `event_changes_debounce_and_refresh_exactly_once`,
which explicitly injects an event and verifies the pending signal followed by
one debounced refresh. Production watcher behavior and timeouts are unchanged.
This repairs the invalid unconditional assertion; it does not claim to have
captured the original native run's exact event ordering.

Follow-up validation: all 1,199 TUI library tests passed (8.65 seconds), followed
by five consecutive passing runs of all 12 watched-input tests. Formatting and
diff checks passed. Source ledger dispositions are unchanged.
