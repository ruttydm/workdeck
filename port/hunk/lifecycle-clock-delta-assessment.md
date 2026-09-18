# First catch-up delta: lifecycle clock

Assessed upstream `c828427b31d29bd55e2aca73fb3049d4593789c9`, the first commit
in the post-baseline topological queue. It remains pending. Baseline parity has
not been completed, so this assessment is not a shadow-port completion record.

The commit changes thirteen files: its release fragment; session-broker README;
connection implementation/tests; package exports; new lifecycle-clock
implementation/tests; broker client implementation/tests; broker launcher
implementation/tests; interactive composition root; and a shared fake-clock test
helper. Translating only the new clock module would omit its consumers and tests.

The source clock contract includes wall-clock milliseconds, disposable one-shot
scheduling, delayed-first fixed-rate intervals, and awaitable delay. Native timers
do not retain process lifetime. Disposal is idempotent before and after settlement.
The new clock tests exercise cancellation, one-shot settlement, delayed-first
intervals, repeated disposal and asynchronous delay settlement.

Current Rust evidence:

- `broker_connection.rs` directly spawns sleeping threads for reconnect,
  handshake timeout and heartbeat work (around lines 810, 859 and 887).
- `broker_launcher.rs` uses direct `Instant::now` deadlines for launcher waits.
- `SessionBrokerClock` in `broker_authenticator.rs` is only an injectable `u64`
  time function; it is not an implementation of scheduling, disposal or delay.

Before this delta can be closed, its timing contract must be implemented and
connected to every changed consumer, the fake-clock tests translated and run,
native scheduling behavior verified, exports/composition updated, and docs/release
content migrated. The immediately following late-settlement-fencing delta must
remain separately accounted for; an injected clock alone does not prove fencing.

This read-only assessment adds no ledger mappings, runtime code, test-pass claims
or catch-up completion. The current queue remains 92 commits.
