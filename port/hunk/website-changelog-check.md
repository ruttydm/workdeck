# Read-only artifact checking

`cargo xtask changelog artifacts-check <markdown-file> <recorded-dates.json> [notes.json]`
generates the same artifact map as `artifacts`, then compares destination bytes
without writing. Matching output succeeds silently; stale output fails with a
path list on stderr. Missing artifacts are stale, not automatically created.

The checker first inventories immediate `.md` entries in the changelog output
directory. Orphaned entries are reported, never deleted. Like pinned Hunk, it
refuses a result that would remove more than half the existing pages when there
are at least two pages. Exactly half remains reportable. Unreadable directory
errors are propagated rather than treated as an empty directory; an absent
directory is allowed. Orphans and artifacts use deterministic native ordering.

Tests verify the collapsed-output guard leaves all original bytes intact, the
half-orphan boundary, missing artifact reporting without directory creation,
unchanged output, changed-byte reporting and preserved stale files.

This implements only the read-only check branch. Source-exact path/report ordering,
complete CLI edge-case coverage, atomic writing,
backups and actual orphan deletion remain open. No source interval is mapped and
no existing generated file is removed by this work. Hunk MIT attribution remains
in the artifact module and notices.

The real CLI test checks missing-output failure without creating the site
directory, silent success after the test installs matching artifacts, and stale
file reporting with the stale bytes preserved. Both checker unit tests and all
18 changelog CLI tests pass. Strict xtask Clippy, formatting and whitespace checks
pass.

## Missing-card presence checks

After artifact bytes match, the checker now reads the generated card list and
reports absent images in card order. It fails on stderr without stdout and never
creates image directories. Like the source's `existsSync`, this is a presence
check, not PNG decoding or visual validation. The CLI regression test verifies
both expected missing paths, no directory creation, and successful checking once
test-only presence fixtures exist. Real image rendering and validation remain open.

The updated CLI regression, strict xtask Clippy, formatting and whitespace checks
pass. No source-ledger interval is marked complete by this continuation.
