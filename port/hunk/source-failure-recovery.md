# Source failure and recovery through the live controller

`failed_source_loads_render_reason_retry_and_cache_the_recovery` exercises three
real worker results through the mounted review controller: missing source,
an unavailable-source error, and a too-large error. Each must publish the exact
error reason and visible two-line gap label while preserving selection.

The source hook calls `startSourceLoad` after either gap-toggle direction, and
its request layer suppresses loading/loaded requests but not errors. The native
test therefore collapses the failed gap to retry, drains the successful worker
completion while the gap is closed, and verifies that no source line or pending
cursor reveal leaks into the closed gap. Reopening shows recovered source and
reveals line one; further close/open toggles reuse the recovered text without a
third read. Deadlines bound worker waits rather than introducing fixed sleeps.

Both pinned `useTerminalReview` suites were rerun for the original missing,
rejected, too-large, and cached-text tests. Each pin passed four tests with
eighteen assertions and no failures. This native test is additional end-to-end
recovery evidence using the existing two-line source-controller fixture, not a
complete translation of those source test bodies or their alpha-file helper.
It does not capture stderr or prove the rejected-fetch diagnostic assertion.
No additional ledger interval is mapped and full lifecycle parity is not claimed.

Scoped validation passes: all 1,182 TUI library tests, zero failures or ignored
tests, in 8.37 seconds; formatting and diff checks also pass. The broad verifier
was last run at the preceding Darwin test-isolation change and was not rerun
for this test-only addition.
