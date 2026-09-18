# Cleanup safety test translation

Pinned main `scripts/generate-changelog.test.ts` bytes `[26370,27169)` / lines
729–750 contain the complete two-test orphan-cleanup safety group. Both are
translated as `source_cleanup_*` tests in the native artifact module.

The harness reads the exact baseline CHANGELOG, recorded dates and notes through
`git show 2c00f4358b89cfc0a6b04459ffc538ba601aa3c2:PATH`. It generates native
counterparts and installs them only in a disposable directory. This replaces the
source test's reliance on pre-existing generated files in the source checkout;
it does not replace the baseline inputs with a synthetic miniature changelog.

The first test requests an empty artifact map and asserts the same refusal to
remove most pages. The second verifies the source's first artifact (dates.json),
changes only its expected contents to `stale`, and checks that exactly that path
is reported. Explicitly selecting dates.json avoids confusing native map order
with the source's insertion order. All three original expectations are retained,
with additional assertions that installed output bytes are untouched.

Both source tests pass under the disposable pinned Bun runtime. Both Rust tests
pass against native generation/checking. The mapped 799 bytes are test coverage,
not full rendered-page parity or write/delete behavior. MIT Modem Labs Inc.
attribution remains in the native module and notices.

Strict audit remains failing with 1,257 files, 1,459 intervals, 280 unmapped
intervals and 92 queued upstream commits. Strict xtask Clippy, formatting and
diff whitespace checks pass after translation.
