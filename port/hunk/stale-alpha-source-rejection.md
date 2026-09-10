# Stale source rejection diagnostics

The source test at `src/ui/hooks/useTerminalReview.test.tsx` bytes 56,202–57,799
(lines 1679–1723) is translated by
`stale_alpha_source_rejection_logs_context_without_repopulating_review`.
Both pinned source runs pass: one test, seven assertions, zero failures per pin.

The native test starts a deferred unversioned reader on the complete alpha8=800
fixture, replaces it with the alpha8=900 fixture and a new reader, then rejects
the old request with `stale failure`. It drains the real worker completion and
checks that source status remains absent, no gap or cursor reveal is resurrected,
and selection stays on the replacement review.

A dedicated test subprocess captures the real controller's stderr, replacing
the source test's temporary console-error interception without changing global
output capture in the parallel native test process. The parent requires success,
one stale-new-source diagnostic, `alpha.ts`, runtime ID `alpha`, and the failure
text. A diagnostic assembled in a disconnected helper is not used as evidence.

Only the complete 1,597-byte test body is mapped. Shared alpha fixture helpers,
the remaining source test corpus, and the full runtime hook remain incomplete.

Validation: all 1,187 TUI library tests pass, zero failures/ignored/filtered, in
8.42 seconds. Formatting and diff checks pass. Strict audit still fails with
1,257 files, 1,406 records, 443 translated-test records, 273 unmapped intervals,
and 11 cached upstream commits. Full workspace verification and strict Clippy
were not rerun for this test-only change.
