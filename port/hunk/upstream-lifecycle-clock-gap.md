# Pending lifecycle-clock delta

Inspected against Workdeck `7c9dc85e` and upstream commit
`c828427b31d29bd55e2aca73fb3049d4593789c9` (first cached pending delta).
This is a gap assessment, not a completed port or executable parity evidence.
Baseline parity must still precede closing the upstream queue.

The source commit changes 13 files. Its new clock contract supplies wall-clock
milliseconds, one-shot scheduling, delayed-first fixed-rate intervals, awaitable
delay, and idempotent timer disposal. Native timers do not retain the process.
The same injected clock travels through the connection, application client,
daemon launcher, and interactive composition root.

## Current implementation and gaps

- `crates/workdeck-session/src/broker_connection.rs`: reconnect, handshake and
  heartbeat workers use direct thread sleeps. Epoch and socket-identity checks
  suppress stale effects, but do not provide the source's shared injectable
  timing interface or disposable scheduled handles. The heartbeat loop sleeps
  after callback work, which is not the declared fixed-rate clock contract.
- `crates/workdeck-session/src/broker_client.rs`: startup retry already has an
  injected `SessionBrokerRetryScheduler` with cancellable handles and a
  condition-variable implementation. Preserve this behavior when adapting it;
  do not replace it with an uncancellable sleep or claim timing injection absent
  everywhere. It is not currently a shared wall-clock/interval/delay abstraction.
- `crates/workdeck-session/src/broker_launcher.rs`: availability deadlines and
  health polling use `Instant::now` and direct delays. Launch-lock age also needs
  separate wall-clock semantics; monotonic deadlines and file modification times
  must not be mixed accidentally when translating the source contract.
- The interactive composition root must pass one lifecycle clock consistently
  through client startup, reconnect discovery and connection scheduling.

## Required evidence before closing the commit

Translate the two new native-clock tests and the deterministic clock helper,
then all changed connection, client and launcher tests. Include disposal before
and after settlement; delayed first interval; repeated fixed-rate callbacks;
awaitable delay; authentication timer retirement; stopped/closed heartbeat and
reconnect cancellation; startup retry identity and original deadline preservation;
and launcher health/lock timing. Inspect the complete changed test bodies before
using this list as a coverage claim—it is not an exhaustive test inventory.

The source README example, new public export, release fragment, and interactive
startup injection are part of the same commit and cannot be dropped. No ledger
record, baseline count, or upstream queue state is changed by this assessment.
