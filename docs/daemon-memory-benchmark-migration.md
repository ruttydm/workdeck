# Daemon memory benchmark migration

The pinned `scripts/daemon-memory-check.ts` workload is replaced by
`cargo xtask benchmark daemon-memory`. The native harness keeps the source
defaults (50 cycles, five warmups, two sessions per cycle, 30 files, four
hunks, 18 lines, API/update counts, settle delay, and RSS growth/slope limits)
and accepts the same option names plus `--json-out`.

Each cycle creates authenticated native `ReviewSessionServer` instances,
requests health/snapshot/review/comment-list data, adds real anchored review
comments, navigates files, queues reloads, samples live state, drops every
session, and samples after cleanup. It reports current RSS and lifetime
high-water resident bytes; a managed JavaScript heap value is never fabricated.
The result is a JSON summary and four `METRIC` lines, with the same growth and
linear-slope gate semantics. The pinned source is 21,227 bytes / 624 lines with
SHA-256 `72130a835789b27384af8fdb6d9d8700387622bd8648754f59347866ab116ee4` at
both protected anchors.
