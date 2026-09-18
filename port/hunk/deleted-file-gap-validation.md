# Explicit gap validation on a fully deleted file

Baseline `useTerminalReview.test.tsx` bytes 49,784–50,888 (lines 1507–1535)
require an explicit nonexistent `trailing:0` toggle to fail with `does not exist`,
perform no source reads, and leave source status absent. Both pinned source tests
pass (one test and five assertions each).

The native addressed-gap handler previously returned silently. It now returns
the shared `ReviewIntentPlanningError` shape with `GapNotFound` and the addressed
gap/path message. It first preserves the source no-fetcher/missing-file no-op
boundary, then validates the actual gap geometry before any expansion or source
load. Renderer callers surface unexpected addressing errors as status rather
than discarding the result. The selected-gap command still returns quietly when
no gap can be selected.

`addressed_gap_on_fully_deleted_file_errors_without_source_load` retains the
one-line `removed.ts` before source, empty after source, three-line context,
TypeScript language, `removed` runtime ID, empty patch field, no annotations,
and explicitly installed old-side reader. It checks the exact native error,
zero reader calls, absent source status, no expansion or pending reveal, and
unchanged selection; it additionally checks selected-command no-op behavior.

Only the complete 1,104-byte source test and its intent comment are mapped.
Surrounding helpers/tests and the full hook implementation remain incomplete.
This is not a release, full-lifecycle, or terminal-frame parity claim.

Validation: all 1,183 TUI library tests pass in 8.86 seconds with zero failures,
ignored tests, or filters; formatting and diff checks pass. Strict audit still
fails: 1,257 baseline files, 1,403 records, 441 translated-test records, 272
unmapped intervals, and 11 cached upstream commits. Splitting the unfinished
neighbors raises the unmapped interval count while adding 1,104 covered bytes.
The full verifier and strict Clippy were not rerun for this change.
